//! Thumbnail-cache operations for CBXManager (plan sections 11 and 12).
//!
//! Bridges the manager UI and background work to
//! `cbxshell-thumbnail-cache`: the configured capacity limit lives in the
//! registry as `MaxCacheGiB` under the manager config key, and the stats /
//! maintenance / clear operations run against the plan-default cache root.
//!
//! Clearing (plan section 12.2) only deletes stored L2 data. It never scans
//! a library and never warms Windows L1 — the shell extension simply
//! re-misses and re-renders thumbnails on demand, so no mass regeneration
//! can be triggered from here.

use std::path::PathBuf;

use anyhow::{Context, Result};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use cbxshell_thumbnail_cache::config::{CacheLimits, DEFAULT_STALE_TEMP_AFTER_SECS};
use cbxshell_thumbnail_cache::maintenance::MaintenanceReport;
use cbxshell_thumbnail_cache::store::ThumbnailCache;

/// Same configuration key used by `registry_ops` for the manager settings.
const CONFIG_KEY_PATH: &str = "Software\\CBXShell-rs\\{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}";

/// Registry value name for the configured maximum cache size.
const MAX_CACHE_GIB_VALUE: &str = "MaxCacheGiB";

/// Default capacity in GiB (plan section 11.1: 8 GiB). `0` means unlimited.
/// Used by `read_max_cache_gib` as fallback and by the GUI for display.
pub(crate) const DEFAULT_MAX_CACHE_GIB: u32 = 8;

/// Registry value holding the logical cache generation. Must match the
/// value the COM side reads (`CBXShell/src/com/l2_cache.rs`, default 0).
const CACHE_GENERATION_VALUE: &str = "CacheGeneration";

/// Stored value unit rationale: the UI offers up to 32 GiB, which overflows
/// a REG_DWORD when expressed in bytes (u32 max is 4 GiB). A QWORD byte
/// count would work, but GiB units keep the stored value human-readable in
/// `regedit` and match the fixed size choices offered in the dialog.
///
/// # Errors
///
/// Propagates registry write failures.
pub fn write_max_cache_gib(gib: u32) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(CONFIG_KEY_PATH)
        .context("Failed to create config key")?;
    key.set_value(MAX_CACHE_GIB_VALUE, &gib)
        .context("Failed to set MaxCacheGiB value")?;
    Ok(())
}

/// Read the configured maximum cache size in GiB. Missing value or key
/// defaults to 8 GiB; a missing config key is not an error.
pub fn read_max_cache_gib() -> Result<u32> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let value = match hkcu.open_subkey(CONFIG_KEY_PATH) {
        Ok(key) => key
            .get_value::<u32, _>(MAX_CACHE_GIB_VALUE)
            .unwrap_or(DEFAULT_MAX_CACHE_GIB),
        Err(_) => DEFAULT_MAX_CACHE_GIB,
    };
    Ok(value)
}

/// Pure mapping from the stored GiB value to [`CacheLimits`] (`0` means
/// unlimited). Units conversion lives here so it can be tested without
/// touching the registry.
fn limits_from_gib(gib: u32) -> CacheLimits {
    CacheLimits {
        max_bytes: if gib == 0 {
            None
        } else {
            // u32 GiB cannot overflow u64 bytes (max 4_294_967_295 GiB).
            Some(u64::from(gib) * 1024 * 1024 * 1024)
        },
        stale_temp_after_secs: DEFAULT_STALE_TEMP_AFTER_SECS,
    }
}

/// [`CacheLimits`] for the currently configured limit.
fn cache_limits() -> CacheLimits {
    limits_from_gib(read_max_cache_gib().unwrap_or(DEFAULT_MAX_CACHE_GIB))
}

/// Next generation value. Saturating on purpose: a wrap-around would make
/// bumped generations collide with long-dead ones, and plan section 18
/// (Phase 8 acceptance) requires previous generations to stay unreachable.
fn next_generation(prev: u32) -> u32 {
    prev.saturating_add(1)
}

/// Bump the global logical cache generation (plan sections 5.4 and 13.3).
///
/// Every existing L2 entry becomes stale immediately, because variant IDs
/// embed the generation, without synchronously deleting a multi-gigabyte
/// directory; old-generation files are reclaimed later by background
/// maintenance. Meant for explicit "invalidate entire cache" scope only —
/// a folder-scope regeneration invalidates per source instead.
///
/// # Errors
///
/// Propagates registry failures.
pub fn increment_cache_generation() -> Result<u64> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(CONFIG_KEY_PATH)
        .context("Failed to create config key")?;
    let prev = key.get_value::<u32, _>(CACHE_GENERATION_VALUE).unwrap_or(0);
    let next = next_generation(prev);
    key.set_value(CACHE_GENERATION_VALUE, &next)
        .context("Failed to set CacheGeneration value")?;
    Ok(u64::from(next))
}

/// Approximate cache size and entry count plus the cache root for display
/// (plan sections 12.1 and Phase 5). Scan-only; deletes nothing.
///
/// # Errors
///
/// Propagates root resolution or scan failures (both soft by contract).
pub fn collect_stats() -> Result<(MaintenanceReport, PathBuf)> {
    let cache = ThumbnailCache::with_default_root()?;
    let root = cache.root().to_path_buf();
    let report = cache.stats()?;
    Ok((report, root))
}

/// Run offline cache maintenance: stale temp cleanup and capacity eviction
/// with the configured limit (plan section 11.3).
///
/// # Errors
///
/// Propagates root resolution or maintenance failures.
pub fn run_maintenance() -> Result<MaintenanceReport> {
    let cache = ThumbnailCache::with_default_root()?;
    Ok(cache.maintenance(cache_limits())?)
}

/// Physically delete all cached entries, keeping the root directory
/// (plan section 12.2). Deliberately no library scan and no regeneration:
/// callers only reclaim storage.
///
/// # Errors
///
/// Propagates root resolution or deletion failures.
pub fn clear_all() -> Result<()> {
    let cache = ThumbnailCache::with_default_root()?;
    Ok(cache.clear_all_physical()?)
}

/// Human-readable byte size for the GUI (plan section 12.1 shows
/// "4.7 GB / 8 GB"). Uses binary units labeled GB/MB per the plan's display
/// convention; whole-GiB sizes drop the decimal.
pub(crate) fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        let tenths = (bytes * 10 + GIB / 2) / GIB;
        if tenths % 10 == 0 {
            format!("{} GB", tenths / 10)
        } else {
            format!("{}.{} GB", tenths / 10, tenths % 10)
        }
    } else if bytes >= MIB {
        format!("{} MB", (bytes + MIB / 2) / MIB)
    } else {
        format!("{} KB", (bytes + 512) / 1024)
    }
}

/// Thousands-separated integer for the Entries field (plan section 12.1
/// shows "31,482").
pub(crate) fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::limits_from_gib;

    #[test]
    fn next_generation_increments() {
        assert_eq!(super::next_generation(0), 1);
        assert_eq!(super::next_generation(6), 7);
    }

    #[test]
    fn next_generation_saturates_at_u32_max() {
        assert_eq!(super::next_generation(u32::MAX), u32::MAX);
    }

    #[test]
    fn zero_gib_maps_to_unlimited() {
        let limits = limits_from_gib(0);
        assert_eq!(limits.max_bytes, None);
        assert!(limits.stale_temp_after_secs > 0);
    }

    #[test]
    fn eight_gib_maps_to_default_capacity() {
        let limits = limits_from_gib(8);
        assert_eq!(limits.max_bytes, Some(8 * 1024 * 1024 * 1024));
    }

    #[test]
    fn thirty_two_gib_does_not_overflow_dword_bytes() {
        // The reason MaxCacheGiB stores GiB units instead of bytes: a byte
        // value like this does not fit in a REG_DWORD.
        let limits = limits_from_gib(32);
        assert_eq!(limits.max_bytes, Some(32 * 1024 * 1024 * 1024));
    }

    #[test]
    fn format_bytes_whole_gib_drops_decimal() {
        assert_eq!(super::format_bytes(8 * 1024 * 1024 * 1024), "8 GB");
        assert_eq!(super::format_bytes(32 * 1024 * 1024 * 1024), "32 GB");
    }

    #[test]
    fn format_bytes_fractional_and_small_sizes() {
        let four_point_seven = (4.7 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(super::format_bytes(four_point_seven), "4.7 GB");
        assert_eq!(super::format_bytes(512 * 1024 * 1024), "512 MB");
        assert_eq!(super::format_bytes(0), "0 KB");
    }

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(super::format_count(31_482), "31,482");
        assert_eq!(super::format_count(1_234_567), "1,234,567");
        assert_eq!(super::format_count(999), "999");
    }
}
