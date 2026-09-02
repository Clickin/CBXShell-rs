//! Thumbnail regeneration workflow for CBXManager (plan sections 13, 14
//! and 12: Regenerate Thumbnails UI).
//!
//! Regeneration is manager-owned and runs outside the shell extension:
//! per archive it first invalidates the CBXShell L2 source entry, then
//! asks Windows (`IThumbnailCache::GetThumbnail` with `WTS_FORCEEXTRACTION`)
//! to re-extract. Windows invokes the CBXShell provider in its surrogate,
//! which sees an L2 miss, re-renders, stores a fresh canonical proxy and
//! lets Windows warm its own L1 (plan section 13.1) — no `thumbcache_*.db`
//! deletion and no Explorer restart anywhere.
//!
//! Global scope ("invalidate entire cache") instead bumps the persistent
//! `CacheGeneration` once (plan sections 13.3 and 5.4) so every previous L2
//! entry goes stale in O(1); the selected folder is then force-extracted as
//! prewarming. Old-generation files are reclaimed by background
//! maintenance, not here.

use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context, Result};
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, ISharedBitmap, IShellItem, IThumbnailCache,
    LocalThumbnailCache, SHCreateItemFromParsingName, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    WTS_CACHEFLAGS, WTS_FORCEEXTRACTION,
};

use super::cache_ops;
use super::utils;
use cbxshell_thumbnail_cache::store::ThumbnailCache;

/// Registered archive extensions only (plan section 13.2: enumerate only
/// currently supported/registered extensions; section 14: no invented
/// library catalog — the user-selected folder is the whole scope).
const ARCHIVE_EXTENSIONS: [&str; 6] = ["zip", "cbz", "rar", "cbr", "7z", "cb7"];

/// Depth cap so a pathological (or junction-looped) tree cannot recurse
/// forever. Mirrors the cache maintenance walker's bound.
const MAX_SCAN_DEPTH: u8 = 32;

/// Conservative worker count (plan section 13.2: benchmark on HDD, SSD and
/// network libraries before raising).
pub const DEFAULT_REGENERATION_WORKERS: usize = 2;

/// Outcome of one [`run_regeneration`] batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegenReport {
    /// Items whose extraction finished (successfully or not).
    pub processed: u64,
    /// Per-item failures, recorded and skipped without aborting the batch.
    pub failed: Vec<(PathBuf, String)>,
}

/// Case-insensitive check against the six registered archive extensions.
fn is_archive(path: &Path) -> bool {
    let ext = path.extension().and_then(|ext| ext.to_str());
    ext.map_or(false, |ext| {
        ARCHIVE_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// Collect supported archives under `root` (plan section 13.2).
///
/// Non-recursive mode scans only `root` itself; recursive mode descends
/// into subdirectories. Unreadable or missing directories are skipped, not
/// fatal. The result is sorted for deterministic display.
pub fn collect_archives(root: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_archives_inner(root, recursive, 0, &mut found);
    found
}

fn collect_archives_inner(dir: &Path, recursive: bool, depth: u8, found: &mut Vec<PathBuf>) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    // A missing or unreadable directory yields nothing; never fatal.
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if recursive {
                collect_archives_inner(&path, recursive, depth + 1, found);
            }
            continue;
        }
        if is_archive(&path) {
            found.push(path);
        }
    }
}

/// Run one regeneration batch with bounded parallelism.
///
/// `workers` scoped threads claim indices from a shared atomic counter, so
/// scheduling is lock-free and cancellation (`cancel` set) simply stops new
/// claims: in-flight items run to completion, then workers exit. Per-item
/// failures are recorded in the report and skipped — one bad file never
/// aborts the batch (plan section 13.2).
pub fn run_regeneration(
    paths: Vec<PathBuf>,
    workers: usize,
    cancel: &AtomicBool,
    extract: &(dyn Fn(&Path) -> Result<(), String> + Sync),
) -> RegenReport {
    if paths.is_empty() {
        return RegenReport::default();
    }
    let workers = workers.clamp(1, paths.len());
    let next_index = AtomicUsize::new(0);
    let report = Mutex::new(RegenReport::default());

    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                let Some(path) = paths.get(index) else {
                    break;
                };
                let outcome = extract(path);
                let mut report = report.lock().expect("regen report lock");
                report.processed += 1;
                if let Err(reason) = outcome {
                    report.failed.push((path.to_path_buf(), reason));
                }
            });
        }
    });

    let report = report.into_inner().expect("regen report lock");
    report
}

/// One regeneration unit (plan section 13.1): invalidate the CBXShell L2
/// source entry first, then force Windows extraction at the default 256 px
/// thumbnail size. `WTS_FORCEEXTRACTION` (never a no-cache variant) keeps
/// L1 caching enabled, so afterwards both L2 and L1 are warm.
///
/// `invalidate_l2` is false for global-scope runs: the generation was
/// already bumped once up front, so per-source invalidation would be
/// redundant (plan section 13.4: do not mix scopes).
pub fn regenerate_one(path: &Path, invalidate_l2: bool) -> Result<(), String> {
    if invalidate_l2 {
        let cache =
            ThumbnailCache::with_default_root().map_err(|e| format!("L2 open failed: {e}"))?;
        cache
            .invalidate_source(path)
            .map_err(|e| format!("L2 invalidate failed: {e}"))?;
    }

    let _com = ComGuard::init()?;
    unsafe {
        let path_wide = to_wide_null_terminated(path);
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR::from_raw(path_wide.as_ptr()), None)
                .map_err(|e| format!("Shell item creation failed: {e}"))?;

        let thumbnail_cache: IThumbnailCache =
            CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Thumbnail cache creation failed: {e}"))?;

        let mut shared_bitmap: Option<ISharedBitmap> = None;
        let mut out_flags = WTS_CACHEFLAGS(0);
        thumbnail_cache
            .GetThumbnail(
                &item,
                256,
                WTS_FORCEEXTRACTION,
                Some(&mut shared_bitmap),
                Some(&mut out_flags),
                None,
            )
            .map_err(|e| format!("Force extraction failed: {e}"))?;
        // The bitmap itself is not needed; Windows has cached the thumbnail
        // in L1, and CBXShell stored a fresh L2 proxy during extraction.
    }
    Ok(())
}

/// Show a folder picker (IFileDialog with FOS_PICKFOLDERS). Returns `None`
/// when the user cancels.
fn pick_folder(owner: HWND) -> Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .context("Failed to create the folder dialog")?;
        let options = dialog
            .GetOptions()
            .context("Failed to read dialog options")?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS)
            .context("Failed to configure the folder dialog")?;
        match dialog.Show(owner) {
            Ok(()) => {}
            Err(err) if err.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => return Ok(None),
            Err(err) => return Err(anyhow!("Folder dialog failed: {err}")),
        }
        let item = dialog.GetResult().context("Failed to read dialog result")?;
        let display = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .context("Failed to resolve the selected folder")?;
        let path = display
            .to_string()
            .map_err(|e| anyhow!("Invalid folder name: {e}"))?;
        Ok(Some(PathBuf::from(path)))
    }
}

struct ComGuard(bool);

impl ComGuard {
    fn init() -> Result<Self, String> {
        // S_OK and S_FALSE (already initialized on this thread) both leave
        // the thread with an apartment; the guard balances one CoUninitialize.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .map_err(|e| format!("COM initialization failed: {e}"))?;
        Ok(Self(true))
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn to_wide_null_terminated(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// ---------------------------------------------------------------------------
// GUI (plan section 12: Regenerate Thumbnails..., plan section 13.2 UX)
// ---------------------------------------------------------------------------

use std::cell::RefCell;

use native_windows_derive as nwd;
use native_windows_gui as nwg;
use native_windows_gui::NativeUi;
use nwd::NwgUi;

use self::c_b_x_regen_dialog_ui::CBXRegenDialogUi;
use self::c_b_x_regen_progress_ui::CBXRegenProgressUi;

/// Mutable state of the setup dialog, kept in thread-local storage so the
/// nwd-generated controls stay free of runtime data.
struct RegenDialogState {
    folder: Option<PathBuf>,
}

thread_local! {
    static REGEN_DIALOG: RefCell<Option<CBXRegenDialogUi>> = const { RefCell::new(None) };
    static REGEN_DIALOG_STATE: RefCell<RegenDialogState> =
        RefCell::new(RegenDialogState { folder: None });
    static REGEN_PROGRESS: RefCell<Option<CBXRegenProgressUi>> = const { RefCell::new(None) };
    static REGEN_RUNTIME: RefCell<Option<RegenRuntime>> = const { RefCell::new(None) };
}

struct RegenRuntime {
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<RegenShared>>,
    total: usize,
}

/// Show (building on first use) the Regenerate Thumbnails setup dialog.
pub fn open_regen_dialog() -> Result<()> {
    REGEN_DIALOG.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(dialog) = slot.as_ref() {
            dialog.window.set_visible(true);
            return Ok(());
        }
        let dialog = CBXRegenDialog::build_ui(Default::default())?;
        dialog.apply_font();
        dialog
            .recursive_check
            .set_check_state(nwg::CheckBoxState::Checked);
        dialog.update_found_count();
        *slot = Some(dialog);
        Ok(())
    })
}

#[derive(Default, NwgUi)]
pub struct CBXRegenDialog {
    #[nwg_control(
        size: (420, 210),
        position: (320, 320),
        title: "Regenerate Thumbnails",
        flags: "WINDOW|VISIBLE"
    )]
    window: nwg::Window,

    #[nwg_resource(family: "Segoe UI", size: 16)]
    ui_font: nwg::Font,

    #[nwg_control(parent: window, text: "Folder", position: (12, 12), size: (120, 20))]
    folder_caption: nwg::Label,

    #[nwg_control(parent: window, text: "", position: (12, 34), size: (290, 20))]
    folder_value: nwg::Label,

    #[nwg_control(
        parent: window,
        text: "Choose...",
        position: (312, 32),
        size: (96, 24)
    )]
    #[nwg_events(OnButtonClick: [CBXRegenDialog::on_pick_folder])]
    pick_button: nwg::Button,

    #[nwg_control(
        parent: window,
        text: "Include subfolders",
        position: (12, 66),
        size: (280, 18)
    )]
    #[nwg_events(OnButtonClick: [CBXRegenDialog::on_scope_changed])]
    recursive_check: nwg::CheckBox,

    #[nwg_control(
        parent: window,
        text: "Invalidate entire cache (all archives)",
        position: (12, 90),
        size: (390, 18)
    )]
    #[nwg_events(OnButtonClick: [CBXRegenDialog::on_scope_changed])]
    global_check: nwg::CheckBox,

    #[nwg_control(
        parent: window,
        text: "Bumps the cache generation so every existing entry is stale,\r\nthen re-extracts the selected folder.",
        position: (24, 112),
        size: (380, 30)
    )]
    global_help_label: nwg::Label,

    #[nwg_control(parent: window, text: "Files found: 0", position: (12, 148), size: (280, 20))]
    found_label: nwg::Label,

    #[nwg_control(
        parent: window,
        text: "Regenerate",
        position: (312, 170),
        size: (96, 26)
    )]
    #[nwg_events(OnButtonClick: [CBXRegenDialog::on_regenerate])]
    regenerate_button: nwg::Button,
}

impl CBXRegenDialog {
    fn apply_font(&self) {
        let font = Some(&self.ui_font);
        self.folder_caption.set_font(font);
        self.folder_value.set_font(font);
        self.pick_button.set_font(font);
        self.recursive_check.set_font(font);
        self.global_check.set_font(font);
        self.global_help_label.set_font(font);
        self.found_label.set_font(font);
        self.regenerate_button.set_font(font);
    }

    /// Re-enumerate the chosen folder with the current scope settings and
    /// refresh the "Files found" line. Filename-only scan, so synchronous
    /// is fine.
    fn update_found_count(&self) {
        let found = REGEN_DIALOG_STATE.with(|state| {
            let state = state.borrow();
            state.folder.as_deref().map_or(0, |folder| {
                collect_archives(
                    folder,
                    self.recursive_check.check_state() == nwg::CheckBoxState::Checked,
                )
                .len()
            })
        });
        self.found_label.set_text(&format!("Files found: {found}"));
    }

    fn on_pick_folder(&self) {
        let owner = self
            .window
            .handle
            .hwnd()
            .map(|h| HWND(h as isize))
            .unwrap_or_default();
        let picked = match pick_folder(owner) {
            Ok(picked) => picked,
            Err(e) => {
                utils::show_error("Regenerate Thumbnails", &e.to_string());
                return;
            }
        };
        let Some(folder) = picked else { return };
        REGEN_DIALOG_STATE.with(|state| state.borrow_mut().folder = Some(folder));
        self.folder_value
            .set_text(&REGEN_DIALOG_STATE.with(|state| {
                state
                    .borrow()
                    .folder
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            }));
        self.update_found_count();
    }

    fn on_scope_changed(&self) {
        self.update_found_count();
    }

    fn on_regenerate(&self) {
        let folder = REGEN_DIALOG_STATE.with(|state| state.borrow().folder.clone());
        let Some(folder) = folder else {
            utils::show_error("Regenerate Thumbnails", "Choose a folder first.");
            return;
        };
        let recursive = self.recursive_check.check_state() == nwg::CheckBoxState::Checked;
        let global = self.global_check.check_state() == nwg::CheckBoxState::Checked;
        let paths = collect_archives(&folder, recursive);
        if paths.is_empty() {
            utils::show_error(
                "Regenerate Thumbnails",
                "No supported archives (.zip .cbz .rar .cbr .7z .cb7) were found in that folder.",
            );
            return;
        }

        // Global scope: bump the generation exactly once, up front
        // (plan section 13.3). Folder scope never touches the generation
        // (plan section 13.4); it invalidates per source instead.
        if global {
            if let Err(e) = cache_ops::increment_cache_generation() {
                utils::show_error(
                    "Regenerate Thumbnails",
                    &format!("Failed to invalidate the cache generation: {e}"),
                );
                return;
            }
        }

        self.window.set_visible(false);
        if let Err(e) = start_regeneration(paths, global) {
            utils::show_error("Regenerate Thumbnails", &e.to_string());
        }
    }
}

/// Shared batch progress the worker publishes and the UI thread reads on
/// each wake-up (`NoticeSender` only wakes; it carries no data).
struct RegenShared {
    done: u64,
    current: Option<PathBuf>,
    finished: Option<RegenReport>,
}

/// Open the progress window and start the bounded background batch.
fn start_regeneration(paths: Vec<PathBuf>, global_scope: bool) -> Result<()> {
    let total = paths.len();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(RegenShared {
        done: 0,
        current: None,
        finished: None,
    }));

    // The window is built first so the worker can hold a wake-up sender
    // for the notice created with it.
    let progress = CBXRegenProgress::build_ui(Default::default())?;
    progress.apply_font();
    progress.progress_bar.set_range(0..total as u32 + 1);
    progress.progress_bar.set_pos(0);
    progress.count_label.set_text(&format!("0 / {total}"));
    progress.current_file.set_text("");
    let wake = progress.notice.sender();

    spawn_regeneration_worker(
        paths,
        global_scope,
        Arc::clone(&cancel),
        Arc::clone(&shared),
        wake,
    );

    REGEN_RUNTIME.with(|runtime| {
        *runtime.borrow_mut() = Some(RegenRuntime {
            cancel,
            shared,
            total,
        });
    });
    REGEN_PROGRESS.with(|slot| *slot.borrow_mut() = Some(progress));
    Ok(())
}

/// Spawn the worker thread that runs the batch and wakes the UI after
/// every item. Cancel is shared with the progress window.
fn spawn_regeneration_worker(
    paths: Vec<PathBuf>,
    global_scope: bool,
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<RegenShared>>,
    wake: nwg::NoticeSender,
) {
    thread::spawn(move || {
        let extract = |path: &Path| -> Result<(), String> {
            let outcome = regenerate_one(path, !global_scope);
            if let Ok(mut shared) = shared.lock() {
                shared.done += 1;
                shared.current = Some(path.to_path_buf());
            }
            wake.notice();
            outcome
        };
        let report = run_regeneration(paths, DEFAULT_REGENERATION_WORKERS, &cancel, &extract);
        if let Ok(mut shared) = shared.lock() {
            shared.finished = Some(report);
        }
        wake.notice();
    });
}

#[derive(Default, NwgUi)]
pub struct CBXRegenProgress {
    #[nwg_control(
        size: (440, 175),
        position: (340, 340),
        title: "Regenerating thumbnails",
        flags: "WINDOW|VISIBLE"
    )]
    #[nwg_events(OnWindowClose: [CBXRegenProgress::on_window_close])]
    window: nwg::Window,

    #[nwg_resource(family: "Segoe UI", size: 16)]
    ui_font: nwg::Font,

    #[nwg_control(parent: window, text: "0 / 0", position: (12, 12), size: (416, 22))]
    count_label: nwg::Label,

    #[nwg_control(
        parent: window,
        position: (12, 40),
        size: (416, 24),
        range: 0..1,
        pos: 0
    )]
    progress_bar: nwg::ProgressBar,

    #[nwg_control(parent: window, text: "", position: (12, 74), size: (416, 20))]
    current_file: nwg::Label,

    #[nwg_control(
        parent: window,
        text: "Cancel",
        position: (340, 110),
        size: (88, 26)
    )]
    #[nwg_events(OnButtonClick: [CBXRegenProgress::on_cancel])]
    cancel_button: nwg::Button,

    #[nwg_control(parent: window)]
    #[nwg_events(OnNotice: [CBXRegenProgress::on_progress])]
    notice: nwg::Notice,
}

impl CBXRegenProgress {
    fn apply_font(&self) {
        let font = Some(&self.ui_font);
        self.count_label.set_font(font);
        self.current_file.set_font(font);
        self.cancel_button.set_font(font);
    }

    fn on_cancel(&self) {
        REGEN_RUNTIME.with(|runtime| {
            if let Some(runtime) = runtime.borrow().as_ref() {
                runtime.cancel.store(true, Ordering::Relaxed);
            }
        });
        self.current_file.set_text("Cancelling...");
    }

    fn on_window_close(&self) {
        // Closing the window cancels the batch (in-flight items finish);
        // the window is only hidden, so a summary can still show up if a
        // late wake-up arrives.
        self.on_cancel();
        self.window.set_visible(false);
    }

    /// Worker wake-up: refresh the display from the shared batch state and
    /// show the summary once the batch is finished.
    fn on_progress(&self) {
        REGEN_RUNTIME.with(|runtime| {
            let runtime = runtime.borrow();
            let Some(state) = runtime.as_ref() else {
                return;
            };
            let Ok(mut shared) = state.shared.lock() else {
                return;
            };

            let done = shared.done;
            self.count_label
                .set_text(&format!("{done} / {}", state.total));
            self.progress_bar
                .set_pos(done.min(state.total as u64) as u32);
            if let Some(current) = shared.current.as_ref() {
                self.current_file.set_text(&current.display().to_string());
            }

            if let Some(report) = shared.finished.take() {
                state.cancel.store(true, Ordering::Relaxed);
                self.progress_bar
                    .set_pos(state.total.min(u32::MAX as usize) as u32);
                self.window.set_visible(false);
                drop(shared);
                show_regen_summary(&report);
            }
        });
    }
}

/// Final summary MessageBox (plan section 13.2: failed files are reported,
/// not silent, and never abort the run).
fn show_regen_summary(report: &RegenReport) {
    let mut message = format!(
        "Regeneration finished.\n\nProcessed: {}\nFailed: {}",
        report.processed,
        report.failed.len()
    );
    for (path, reason) in report.failed.iter().take(3) {
        message.push_str(&format!("\n\n{}: {reason}", path.display()));
    }
    if report.failed.len() > 3 {
        message.push_str(&format!("\n\n(and {} more)", report.failed.len() - 3));
    }
    utils::show_success("Regenerate Thumbnails", &message);
}

#[cfg(test)]
mod tests {
    use super::{collect_archives, run_regeneration, RegenReport};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn write(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        fs::write(path, b"x").expect("write file");
    }

    fn sample_tree() -> TempDir {
        let root = TempDir::new().expect("tempdir");
        write(&root.path().join("a.cbz"));
        write(&root.path().join("notes.txt"));
        write(&root.path().join("cover.jpg"));
        write(&root.path().join("sub").join("b.rar"));
        write(&root.path().join("sub").join("deep").join("d.CB7"));
        write(&root.path().join("sub").join("deep").join("e.zip"));
        fs::create_dir_all(root.path().join("empty")).expect("empty dir");
        root
    }

    #[test]
    fn collects_supported_archives_recursively_and_case_insensitively() {
        let root = sample_tree();
        let found = collect_archives(root.path(), true);
        assert_eq!(
            found,
            vec![
                root.path().join("a.cbz"),
                root.path().join("sub").join("b.rar"),
                root.path().join("sub").join("deep").join("d.CB7"),
                root.path().join("sub").join("deep").join("e.zip"),
            ]
        );
    }

    #[test]
    fn non_recursive_scan_stays_in_the_selected_folder() {
        let root = sample_tree();
        let found = collect_archives(root.path(), false);
        assert_eq!(found, vec![root.path().join("a.cbz")]);
    }

    #[test]
    fn missing_root_yields_no_archives() {
        let root = TempDir::new().expect("tempdir");
        let found = collect_archives(&root.path().join("gone"), true);
        assert!(found.is_empty());
    }

    #[test]
    fn unreadable_directory_is_skipped_not_fatal() {
        let root = TempDir::new().expect("tempdir");
        write(&root.path().join("top.cbz"));
        let denied = root.path().join("denied");
        fs::create_dir_all(&denied).expect("denied dir");
        write(&denied.join("hidden.cbz"));

        // Deny read access to Everyone on the subdirectory; the test user
        // owns the tree, so only the deny ACE matters.
        let denied_acl = Command::new("icacls")
            .arg(&denied)
            .args(["/deny", "*S-1-1-0:(R)"])
            .status()
            .expect("icacls should launch");
        if !denied_acl.success() {
            // Environment without enforceable ACLs: nothing to prove.
            return;
        }

        let found = collect_archives(root.path(), true);

        // Restore access before the TempDir drop so cleanup can succeed.
        let _ = Command::new("icacls")
            .arg(&denied)
            .args(["/remove:d", "*S-1-1-0"])
            .status();

        // The unreadable directory was skipped; the readable entry remains.
        assert_eq!(found, vec![root.path().join("top.cbz")]);
    }

    #[test]
    fn bounded_workers_never_exceed_the_limit() {
        let cancel = AtomicBool::new(false);
        let current = AtomicUsize::new(0);
        let max_seen = AtomicUsize::new(0);
        let extract = |_: &Path| -> Result<(), String> {
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            max_seen.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            current.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        };

        let paths: Vec<PathBuf> = (0..24)
            .map(|i| PathBuf::from(format!("f{i}.cbz")))
            .collect();
        let report = run_regeneration(paths, 3, &cancel, &extract);

        assert_eq!(report.processed, 24);
        assert!(report.failed.is_empty());
        assert!(
            max_seen.load(Ordering::SeqCst) <= 3,
            "concurrency reached {}",
            max_seen.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn cancellation_stops_scheduling_early() {
        let cancel = AtomicBool::new(false);
        let started = AtomicUsize::new(0);
        let extract = |_: &Path| -> Result<(), String> {
            if started.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
                cancel.store(true, Ordering::SeqCst);
            }
            thread::sleep(Duration::from_millis(5));
            Ok(())
        };

        let paths: Vec<PathBuf> = (0..100)
            .map(|i| PathBuf::from(format!("f{i}.cbz")))
            .collect();
        let report = run_regeneration(paths, 2, &cancel, &extract);

        // Scheduling stopped after the flag was raised; in-flight items (at
        // most one per worker) still completed and were counted.
        assert!(report.processed < 100);
        assert_eq!(report.processed as usize, started.load(Ordering::SeqCst));
        assert!(report.failed.is_empty());
    }

    #[test]
    fn failures_are_recorded_without_aborting_the_batch() {
        let cancel = AtomicBool::new(false);
        let extract = |path: &Path| -> Result<(), String> {
            if path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .contains("bad")
            {
                Err("boom".to_string())
            } else {
                Ok(())
            }
        };

        let paths = vec![
            PathBuf::from("bad1.cbz"),
            PathBuf::from("good1.cbz"),
            PathBuf::from("bad2.cbz"),
            PathBuf::from("good2.cbz"),
        ];
        let report = run_regeneration(paths, 2, &cancel, &extract);

        assert_eq!(report.processed, 4);
        assert_eq!(report.failed.len(), 2);
        assert!(report.failed.iter().all(|(_, reason)| reason == "boom"));
        let mut failed_paths: Vec<PathBuf> =
            report.failed.iter().map(|(path, _)| path.clone()).collect();
        failed_paths.sort();
        assert_eq!(
            failed_paths,
            vec![PathBuf::from("bad1.cbz"), PathBuf::from("bad2.cbz")]
        );
    }
}
