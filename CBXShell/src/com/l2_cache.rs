//! L2 thumbnail cache integration helpers (plan sections 4, 5.4, 8)
//!
//! Bridges the COM thumbnail flow ([`super::cbxshell`]) to the
//! `cbxshell-thumbnail-cache` crate and the canonical preview-proxy codec:
//!
//! * [`resolve_source_identity`] builds the O(metadata)
//!   [`SourceIdentity`] from either a filesystem path
//!   (IInitializeWithFile/IInitializeWithItem) or an `IStream::Stat` name
//!   (IInitializeWithStream). Anonymous or non-path-like streams yield
//!   `None` and the caller bypasses the cache entirely (plan section 4.4).
//! * [`build_variant`] wires the source metadata together with the
//!   rendering/configuration state ([`RENDERER_REVISION`],
//!   [`PROXY_MAX_DIMENSION`], [`CACHE_CODEC_REVISION`], sort policy, and
//!   the registry cache generation) into the invalidation key.
//! * [`proxy_to_hbitmap`] derives the Explorer-requested HBITMAP from a
//!   decoded canonical proxy without ever upscaling it.
//!
//! Everything here is best-effort by contract: the caller treats every
//! error as a cache bypass or miss and falls back to the normal archive
//! extraction path. No cache failure may ever fail `GetThumbnail`
//! (plan goal 10).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cbxshell_thumbnail_cache::key::{CacheVariant, SourceIdentity};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{CoTaskMemFree, IStream, STATFLAG_DEFAULT, STATSTG};
use winreg::enums::*;
use winreg::RegKey;

use super::cbxshell::CBXShell;
use crate::image_processor::decoder::preview::{
    DecodedPreview, CACHE_CODEC_REVISION, PROXY_MAX_DIMENSION,
};
use crate::image_processor::hbitmap;
use crate::image_processor::resizer::{self, ResizeFilter};
use crate::image_processor::thumbnail::RENDERER_REVISION;
use crate::utils::error::Result;

/// Registry key holding CBXShell-rs per-user configuration. Same location
/// as the NoSort setting read by the archive config; duplicated as a
/// constant because that module keeps its copy private.
const CONFIG_KEY_PATH: &str = "Software\\CBXShell-rs\\{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}";

/// Registry value holding the logical cache generation (plan section 5.4).
const CACHE_GENERATION_VALUE: &str = "CacheGeneration";

/// Combine a Win32 [`FILETIME`] into its 64-bit 100-nanosecond tick value.
pub(crate) fn filetime_to_u64(ft: &FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// 100 ns ticks between the Windows epoch (1601-01-01) and the Unix epoch
/// (1970-01-01).
const UNIX_TO_WINDOWS_EPOCH_TICKS: u64 = 11_644_473_600 * 10_000_000;

/// Convert a Unix [`SystemTime`] into Windows FILETIME ticks so path and
/// stream identities share one time representation.
///
/// std derives `metadata().modified()` on Windows from the native FILETIME
/// with exact 100 ns granularity, so this round-trips without precision
/// loss and agrees with `IStream::Stat` mtimes.
pub(crate) fn systemtime_to_filetime_u64(modified: SystemTime) -> u64 {
    let to_ticks =
        |d: std::time::Duration| d.as_secs() * 10_000_000 + u64::from(d.subsec_nanos()) / 100;
    match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => UNIX_TO_WINDOWS_EPOCH_TICKS.saturating_add(to_ticks(d)),
        // Timestamps before 1970: clamp at the Windows epoch.
        Err(e) => UNIX_TO_WINDOWS_EPOCH_TICKS.saturating_sub(to_ticks(e.duration())),
    }
}

/// Resolve the stable source identity for cache lookups (plan section 4).
///
/// A filesystem path wins (IInitializeWithFile/IInitializeWithItem);
/// otherwise the IStream's `Stat` name is used when it normalizes to an
/// absolute path. Any failure — unreadable metadata, anonymous stream,
/// non-path-like name — returns `None` and the caller must bypass the L2
/// cache entirely (plan section 4.4: correctness beats caching anonymous
/// streams).
pub(crate) fn resolve_source_identity(
    file_path: Option<&Path>,
    stream: Option<&IStream>,
) -> Option<SourceIdentity> {
    if let Some(path) = file_path {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let mut identity = SourceIdentity::from_path(path);
        identity.size = meta.len();
        identity.modified_filetime = systemtime_to_filetime_u64(modified);
        return Some(identity);
    }

    let stream = stream?;
    let mut stat: STATSTG = unsafe { std::mem::zeroed() };
    if unsafe { stream.Stat(&mut stat, STATFLAG_DEFAULT) }.is_err() {
        return None;
    }

    if stat.pwcsName.is_null() {
        return None;
    }

    let raw_name = stat.pwcsName;
    let name_result = unsafe { raw_name.to_string() };
    unsafe {
        CoTaskMemFree(Some(raw_name.0 as _));
    }

    // Only an absolute, path-like name is a stable locator.
    let path = CBXShell::normalize_stream_name_to_path(&name_result.ok()?)?;

    let mut identity = SourceIdentity::from_path(&path);
    identity.size = stat.cbSize;
    identity.modified_filetime = filetime_to_u64(&stat.mtime);
    Some(identity)
}

/// Build the cache variant identity (plan section 5.2): source metadata
/// plus every rendering/configuration input that changes the rendered
/// pixels. The requested size (`cx`) is deliberately absent — every size
/// is derived from the one canonical proxy (plan section 2.1).
pub(crate) fn build_variant(
    identity: &SourceIdentity,
    sort_images: bool,
    cache_generation: u64,
) -> CacheVariant {
    CacheVariant {
        source_size: identity.size,
        source_mtime: identity.modified_filetime,
        sort_images,
        renderer_revision: RENDERER_REVISION,
        cache_generation,
        proxy_max_dimension: PROXY_MAX_DIMENSION,
        cache_codec_revision: CACHE_CODEC_REVISION,
    }
}

/// Read the logical cache generation from
/// `HKCU\Software\CBXShell-rs\{CLSID}\CacheGeneration` (plan section 5.4).
///
/// A missing key, missing value, or unreadable registry all default to 0
/// so a broken registry can never break thumbnail extraction.
pub(crate) fn read_cache_generation() -> u64 {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    match hkcu.open_subkey(CONFIG_KEY_PATH) {
        Ok(key) => key
            .get_value::<u32, _>(CACHE_GENERATION_VALUE)
            .map(u64::from)
            .unwrap_or(0),
        Err(_) => 0, // Missing key = generation 0
    }
}

/// First 4 bytes of the source ID as 8 hex characters, for debug markers
/// (plan section 15: include the source ID without excessive detail).
pub(crate) fn source_id_tag(identity: &SourceIdentity) -> String {
    identity
        .source_id()
        .iter()
        .take(4)
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Derive the Explorer-requested HBITMAP from a decoded canonical proxy.
///
/// [`resizer::calculate_thumbnail_size`] preserves the aspect ratio and
/// never upscales, so the proxy's canonical resolution bounds the output.
/// Only a genuine dimension change triggers the Triangle-filter resize,
/// matching the rendering pipeline's default quality.
pub(crate) fn proxy_to_hbitmap(proxy: &DecodedPreview, cx: u32) -> Result<HBITMAP> {
    let (target_width, target_height) =
        resizer::calculate_thumbnail_size(proxy.width, proxy.height, cx, cx);

    let resized;
    let rgba: &[u8] = if target_width == proxy.width && target_height == proxy.height {
        &proxy.rgba
    } else {
        resized = resizer::resize_rgba_image(
            &proxy.rgba,
            proxy.width,
            proxy.height,
            target_width,
            target_height,
            ResizeFilter::Triangle,
        )?;
        &resized
    };

    let bgra = hbitmap::rgba_to_bgra(rgba);
    hbitmap::create_hbitmap_from_bgra(&bgra, target_width, target_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique temp path (process id + counter), same pattern as the
    /// debug_log tests, so tests stay isolated and parallel-safe.
    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "cbxshell_l2_{}_{}_{}",
            std::process::id(),
            name,
            nonce
        ))
    }

    #[test]
    fn filetime_to_u64_combines_high_and_low_words() {
        let ft = FILETIME {
            dwLowDateTime: 0x1234_5678,
            dwHighDateTime: 0x0000_00AB,
        };
        assert_eq!(filetime_to_u64(&ft), (0xABu64 << 32) | 0x1234_5678);
    }

    #[test]
    fn filetime_to_u64_zero_is_zero() {
        let ft = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        assert_eq!(filetime_to_u64(&ft), 0);
    }

    #[test]
    fn systemtime_to_filetime_offsets_from_windows_epoch() {
        // Unix epoch = 1970-01-01; Windows epoch = 1601-01-01.
        assert_eq!(
            systemtime_to_filetime_u64(UNIX_EPOCH),
            11_644_473_600 * 10_000_000
        );
    }

    #[test]
    fn build_variant_wires_every_field() {
        let identity = SourceIdentity {
            locator_wide: vec![0x0043, 0x003A, 0x005C], // "C:\"
            size: 0x1122_3344_5566_7788,
            modified_filetime: 0x99AA_BBCC_DDEE_FF00,
        };

        let variant = build_variant(&identity, true, 7);

        assert_eq!(variant.source_size, 0x1122_3344_5566_7788);
        assert_eq!(variant.source_mtime, 0x99AA_BBCC_DDEE_FF00);
        assert!(variant.sort_images);
        assert_eq!(variant.renderer_revision, RENDERER_REVISION);
        assert_eq!(variant.cache_generation, 7);
        assert_eq!(variant.proxy_max_dimension, PROXY_MAX_DIMENSION);
        assert_eq!(variant.cache_codec_revision, CACHE_CODEC_REVISION);
    }

    #[test]
    fn resolve_source_identity_reads_real_file_metadata() {
        let path = unique_temp_path("identity");
        std::fs::write(&path, [0xDE, 0xAD, 0xBE, 0xEF]).expect("write temp file");

        let identity =
            resolve_source_identity(Some(&path), None).expect("identity for a real file");

        assert!(!identity.locator_wide.is_empty());
        assert_eq!(identity.size, 4);
        assert!(identity.modified_filetime > 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolve_source_identity_missing_file_is_none() {
        let path = unique_temp_path("missing");
        assert!(resolve_source_identity(Some(&path), None).is_none());
    }
}
