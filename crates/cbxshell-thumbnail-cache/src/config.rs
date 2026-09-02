//! Cache configuration: capacity limits and the default on-disk location.

use std::path::PathBuf;

use crate::{CacheError, Result};

/// 8 GiB default capacity (plan section 11.1).
pub const DEFAULT_MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Temporary files older than 24 hours are considered abandoned (plan
/// section 9).
pub const DEFAULT_STALE_TEMP_AFTER_SECS: u64 = 24 * 60 * 60;

/// Capacity and cleanup knobs for [`crate::store::ThumbnailCache::maintenance`].
///
/// `max_bytes = None` means unlimited; [`CacheLimits::unlimited`] is the
/// friendly constructor for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheLimits {
    /// Maximum bytes of `.webp` entries tolerated before eviction.
    /// `None` disables capacity eviction.
    pub max_bytes: Option<u64>,
    /// Age (seconds) after which a `.tmp` file counts as abandoned.
    pub stale_temp_after_secs: u64,
}

impl CacheLimits {
    /// No capacity limit; still cleans up stale temporary files.
    pub fn unlimited() -> Self {
        Self {
            max_bytes: None,
            stale_temp_after_secs: DEFAULT_STALE_TEMP_AFTER_SECS,
        }
    }
}

impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_bytes: Some(DEFAULT_MAX_BYTES),
            stale_temp_after_secs: DEFAULT_STALE_TEMP_AFTER_SECS,
        }
    }
}

/// `%LOCALAPPDATA%\CBXShell\thumbnail-cache\v1`.
///
/// The trailing `v1` is part of the layout (plan section 6): a future
/// incompatible layout change can switch to `v2` and leave the old tree to
/// be removed by maintenance or uninstall, instead of migrating in place.
///
/// # Errors
///
/// [`CacheError::LocalAppDataMissing`] when `LOCALAPPDATA` is unset or
/// empty.
pub fn default_cache_root() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or(CacheError::LocalAppDataMissing)?;
    Ok(PathBuf::from(base)
        .join("CBXShell")
        .join("thumbnail-cache")
        .join("v1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_match_plan() {
        let limits = CacheLimits::default();
        assert_eq!(limits.max_bytes, Some(8 * 1024 * 1024 * 1024));
        assert_eq!(limits.stale_temp_after_secs, 24 * 60 * 60);
    }

    #[test]
    fn unlimited_disables_capacity_only() {
        let limits = CacheLimits::unlimited();
        assert_eq!(limits.max_bytes, None);
        assert_eq!(limits.stale_temp_after_secs, DEFAULT_STALE_TEMP_AFTER_SECS);
    }
}
