//! Capacity enforcement and stale-temp cleanup (plan section 11).
//!
//! Maintenance runs outside the thumbnail hot path (CBXManager background
//! work); a `GetThumbnail` call never enumerates the cache or computes
//! cache size (plan section 11.2). Everything here is best-effort:
//! unreadable entries are skipped and counted, never fatal.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::CacheLimits;
use crate::store::ThumbnailCache;
use crate::Result;

/// Outcome of one [`ThumbnailCache::maintenance`] run.
///
/// `entries` and `total_bytes` describe the cache **after** maintenance
/// (what a GUI would display, plan section 12.1); `deleted_*` summarize
/// what this run removed — stale temp files plus capacity evictions
/// combined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// `.webp` cache entries seen during the scan.
    pub entries: u64,
    /// Bytes occupied by surviving `.webp` entries after maintenance.
    pub total_bytes: u64,
    /// Files deleted by this run.
    pub deleted_files: u64,
    /// Bytes of the deleted files.
    pub deleted_bytes: u64,
    /// Entries that could not be read (directory/metadata/delete errors).
    pub skipped: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileKind {
    /// A published cache entry (`*.webp`).
    Entry,
    /// An unpublished or abandoned temporary file (`*.tmp`).
    Temp,
    /// Anything else; left alone, only counted as skipped.
    Other,
}

struct ScannedFile {
    path: PathBuf,
    size: u64,
    /// Seconds since the Unix epoch; pre-epoch mtimes clamp to 0.
    modified_secs: u64,
    kind: FileKind,
}

impl ThumbnailCache {
    /// Clean up the cache tree and enforce capacity limits.
    ///
    /// 1. Delete `*.tmp` files older than `limits.stale_temp_after_secs`
    ///    (plan section 9).
    /// 2. If `limits.max_bytes` is set and the surviving entries exceed it,
    ///    delete oldest-mtime-first until the total is at or below ~90% of
    ///    the cap — in bulk, not one file per pass (plan section 11.3).
    ///
    /// Only published `.webp` entries count toward capacity and are
    /// evicted; unreadable entries are skipped and counted.
    pub fn maintenance(&self, limits: CacheLimits) -> Result<MaintenanceReport> {
        let mut files = Vec::new();
        let mut skipped = 0u64;
        collect_files(self.root(), 0, &mut files, &mut skipped);

        let mut report = summarize(&files, skipped);

        // 1. Stale temporary files.
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let stale_cutoff = now_secs.saturating_sub(limits.stale_temp_after_secs);
        files.retain(|file| {
            file.kind != FileKind::Temp
                || file.modified_secs >= stale_cutoff
                || delete_counted(file, &mut report)
        });

        // 2. Capacity eviction, oldest first, down to ~90% of the cap.
        let mut total = entry_bytes(&files);
        if let Some(max_bytes) = limits.max_bytes {
            if total > max_bytes {
                let target = (max_bytes / 10).saturating_mul(9);
                let mut candidates: Vec<&ScannedFile> =
                    files.iter().filter(|f| f.kind == FileKind::Entry).collect();
                candidates.sort_unstable_by(|a, b| {
                    a.modified_secs
                        .cmp(&b.modified_secs)
                        .then_with(|| a.path.cmp(&b.path))
                });
                for candidate in candidates {
                    if total <= target {
                        break;
                    }
                    if delete_counted(candidate, &mut report) {
                        total -= candidate.size;
                    }
                }
            }
        }
        report.total_bytes = total;

        Ok(report)
    }

    /// Scan-only counterpart of [`ThumbnailCache::maintenance`] for the
    /// manager GUI (plan section 12.1): reports `entries`, `total_bytes`
    /// and `skipped` and deletes NOTHING. The counting rules are identical
    /// to `maintenance`; stale `.tmp` files and stray files are ignored
    /// (treated like `FileKind::Other`) and left in place.
    pub fn stats(&self) -> Result<MaintenanceReport> {
        let mut files = Vec::new();
        let mut skipped = 0u64;
        collect_files(self.root(), 0, &mut files, &mut skipped);
        Ok(summarize(&files, skipped))
    }
}

/// Published-entry statistics shared by [`ThumbnailCache::maintenance`]
/// and [`ThumbnailCache::stats`].
fn summarize(files: &[ScannedFile], skipped: u64) -> MaintenanceReport {
    MaintenanceReport {
        entries: files.iter().filter(|f| f.kind == FileKind::Entry).count() as u64,
        total_bytes: entry_bytes(files),
        skipped,
        ..MaintenanceReport::default()
    }
}

/// Sum of the sizes of published `.webp` entries.
fn entry_bytes(files: &[ScannedFile]) -> u64 {
    files
        .iter()
        .filter(|f| f.kind == FileKind::Entry)
        .map(|f| f.size)
        .sum()
}

/// Delete `file`, folding the outcome into the report. Returns whether the
/// file is gone (deleted, or already gone because someone else won that
/// race).
fn delete_counted(file: &ScannedFile, report: &mut MaintenanceReport) -> bool {
    match fs::remove_file(&file.path) {
        Ok(()) => {
            report.deleted_files += 1;
            report.deleted_bytes += file.size;
            true
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => true,
        Err(_) => {
            report.skipped += 1;
            false
        }
    }
}

/// Recursively collect files under `root`. The layout is shard/source/file
/// (three levels below the root); the depth cap keeps a pathological tree
/// from recursing without bound. Failures increment `skipped` instead of
/// failing the whole run.
fn collect_files(dir: &Path, depth: u8, out: &mut Vec<ScannedFile>, skipped: &mut u64) {
    if depth > 3 {
        *skipped += 1;
        return;
    }
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) => {
            // A missing (root) directory simply has no files.
            if err.kind() != io::ErrorKind::NotFound {
                *skipped += 1;
            }
            return;
        }
    };
    for entry in read {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                *skipped += 1;
                continue;
            }
        };
        let path = entry.path();
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                *skipped += 1;
                continue;
            }
        };
        if meta.is_dir() {
            collect_files(&path, depth + 1, out, skipped);
            continue;
        }
        let kind = match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("webp") => FileKind::Entry,
            Some(ext) if ext.eq_ignore_ascii_case("tmp") => FileKind::Temp,
            _ => FileKind::Other,
        };
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        out.push(ScannedFile {
            path,
            size: meta.len(),
            modified_secs,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::SourceIdentity;
    use tempfile::TempDir;

    fn set_mtime(path: &Path, unix_secs: i64) {
        filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(unix_secs, 0))
            .expect("setting mtime in test");
    }

    fn source(wide: &[u16]) -> SourceIdentity {
        SourceIdentity {
            locator_wide: wide.to_vec(),
            size: 1,
            modified_filetime: 2,
        }
    }

    fn variant() -> crate::key::CacheVariant {
        crate::key::CacheVariant {
            source_size: 10,
            source_mtime: 20,
            sort_images: true,
            renderer_revision: 1,
            cache_generation: 1,
            proxy_max_dimension: 1024,
            cache_codec_revision: 1,
        }
    }

    /// Store one entry with a 100-byte payload and pin its mtime.
    fn put_entry(
        cache: &ThumbnailCache,
        wide: &[u16],
        mtime_secs: i64,
    ) -> (SourceIdentity, crate::key::CacheVariant) {
        let source = source(wide);
        let variant = variant();
        cache
            .store(&source, &variant, &vec![b'x'; 100])
            .expect("storing test entry");
        set_mtime(&cache.entry_path(&source, &variant), mtime_secs);
        (source, variant)
    }

    #[test]
    fn deletes_stale_temp_files_but_keeps_fresh_ones() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        cache.store(&source, &variant(), b"entry").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let stale = cache.source_dir(&source).join("abc.pid.1.tmp");
        fs::write(&stale, b"abandoned").unwrap();
        set_mtime(&stale, now - 2 * 60 * 60);

        let fresh = cache.source_dir(&source).join("def.pid.2.tmp");
        fs::write(&fresh, b"in progress").unwrap();

        let report = cache
            .maintenance(CacheLimits {
                max_bytes: None,
                stale_temp_after_secs: 60 * 60,
            })
            .unwrap();

        assert!(!stale.exists());
        assert!(fresh.exists());
        assert_eq!(report.deleted_files, 1);
        assert_eq!(report.deleted_bytes, 9);
        assert_eq!(report.entries, 1);
        assert_eq!(report.total_bytes, 5);
    }

    #[test]
    fn evicts_oldest_entries_first_down_to_about_ninety_percent() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());

        let base: i64 = 1_700_000_000;
        let oldest = put_entry(&cache, &[0x0041], base);
        let old = put_entry(&cache, &[0x0042], base + 3_600);
        let newer = put_entry(&cache, &[0x0043], base + 2 * 3_600);
        let newest = put_entry(&cache, &[0x0044], base + 3 * 3_600);

        // 4 x 100 bytes = 400 total, cap 300, target 270:
        // deleting the two oldest (200 bytes) is the first point at which
        // the total lands at or below the target.
        let report = cache
            .maintenance(CacheLimits {
                max_bytes: Some(300),
                stale_temp_after_secs: 60 * 60,
            })
            .unwrap();

        assert_eq!(cache.load(&oldest.0, &oldest.1).unwrap(), None);
        assert_eq!(cache.load(&old.0, &old.1).unwrap(), None);
        assert_eq!(
            cache.load(&newer.0, &newer.1).unwrap().as_deref(),
            Some(&[b'x'; 100][..])
        );
        assert_eq!(
            cache.load(&newest.0, &newest.1).unwrap().as_deref(),
            Some(&[b'x'; 100][..])
        );
        assert_eq!(report.deleted_files, 2);
        assert_eq!(report.deleted_bytes, 200);
        assert_eq!(report.entries, 4);
        assert_eq!(report.total_bytes, 200);
        assert_eq!(report.skipped, 0);
    }

    #[test]
    fn no_eviction_under_cap() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());

        put_entry(&cache, &[0x0041], 1_700_000_000);
        put_entry(&cache, &[0x0042], 1_700_003_600);

        let report = cache
            .maintenance(CacheLimits {
                max_bytes: Some(10_000),
                stale_temp_after_secs: 60 * 60,
            })
            .unwrap();

        assert_eq!(report.deleted_files, 0);
        assert_eq!(report.deleted_bytes, 0);
        assert_eq!(report.entries, 2);
        assert_eq!(report.total_bytes, 200);
    }

    #[test]
    fn maintenance_on_missing_root_is_empty_report() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().join("missing"));

        let report = cache.maintenance(CacheLimits::default()).unwrap();
        assert_eq!(
            report,
            MaintenanceReport {
                entries: 0,
                total_bytes: 0,
                deleted_files: 0,
                deleted_bytes: 0,
                skipped: 0,
            }
        );
    }

    #[test]
    fn unreachable_files_are_counted_as_skipped_not_fatal() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        cache.store(&source, &variant(), b"entry").unwrap();

        // A file with neither a known cache extension nor directory meaning.
        let stray = cache.source_dir(&source).join("notes.txt");
        fs::write(&stray, b"stray").unwrap();

        let report = cache.maintenance(CacheLimits::unlimited()).unwrap();

        // Stray files are ignored (never deleted, never counted as bytes),
        // and maintenance succeeds.
        assert!(stray.exists());
        assert_eq!(report.entries, 1);
        assert_eq!(report.total_bytes, 5);
        assert_eq!(report.deleted_files, 0);
    }

    #[test]
    fn stale_cutoff_uses_seconds_resolution() {
        // A file stamped "now" must survive even a 1-second stale window;
        // Duration-based arithmetic on whole seconds keeps this stable.
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        cache.store(&source, &variant(), b"entry").unwrap();

        let report = cache
            .maintenance(CacheLimits {
                max_bytes: None,
                stale_temp_after_secs: 1,
            })
            .unwrap();

        assert_eq!(report.entries, 1);
        assert_eq!(report.deleted_files, 0);
    }

    #[test]
    fn stats_on_missing_root_is_empty_report() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().join("missing"));

        let report = cache.stats().unwrap();
        assert_eq!(report, MaintenanceReport::default());
    }

    #[test]
    fn stats_counts_entries_and_bytes_without_deleting() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());

        let first = put_entry(&cache, &[0x0041], 1_700_000_000);
        let second = put_entry(&cache, &[0x0042], 1_700_003_600);

        let report = cache.stats().unwrap();

        assert_eq!(report.entries, 2);
        assert_eq!(report.total_bytes, 200);
        assert_eq!(report.deleted_files, 0);
        assert_eq!(report.deleted_bytes, 0);
        assert_eq!(report.skipped, 0);
        // Scan-only: both entries still load afterwards.
        assert_eq!(
            cache.load(&first.0, &first.1).unwrap().as_deref(),
            Some(&[b'x'; 100][..])
        );
        assert_eq!(
            cache.load(&second.0, &second.1).unwrap().as_deref(),
            Some(&[b'x'; 100][..])
        );
    }

    #[test]
    fn stats_leaves_stale_temp_files_in_place() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        cache.store(&source, &variant(), b"entry").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let stale = cache.source_dir(&source).join("abc.pid.1.tmp");
        fs::write(&stale, b"abandoned").unwrap();
        set_mtime(&stale, now - 2 * 60 * 60);

        let report = cache.stats().unwrap();

        // Unlike maintenance, stats never deletes: the abandoned temp file
        // is ignored like any other non-entry file and skipped counting
        // stays identical to maintenance.
        assert!(stale.exists());
        assert_eq!(report.entries, 1);
        assert_eq!(report.total_bytes, 5);
        assert_eq!(report.deleted_files, 0);
        assert_eq!(report.skipped, 0);
    }
}
