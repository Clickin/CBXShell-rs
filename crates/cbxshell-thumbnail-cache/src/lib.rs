//! CBXShell persistent thumbnail L2 cache core.
//!
//! Implements the cache-identity, filesystem-layout, and maintenance parts
//! of `THUMBNAIL_L2_CACHE_PLAN.md` (sections 4-11): SHA-256 source/variant
//! keys, a sharded WebP store with atomic publication, and offline capacity
//! maintenance. Encoding/decoding of the stored bytes lives elsewhere (the
//! canonical proxy codec); this crate treats entries as opaque blobs.
//!
//! Like the rest of the workspace this crate is Windows-targeted; path
//! hashing uses raw UTF-16 code units via `OsStr::encode_wide`.
//!
//! Cache failures are soft by contract (plan goal 10): callers must treat
//! every `Err` as "behave as if the cache missed".

pub mod config;
pub mod key;
pub mod maintenance;
pub mod store;

use thiserror::Error;

/// Crate-wide result alias. The error defaults to [`CacheError`] so common
/// call sites stay short while callers can still pin a precise error type.
pub type Result<T, E = CacheError> = std::result::Result<T, E>;

/// Errors produced by cache operations. All are soft failures for callers.
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("%LOCALAPPDATA% is not set; cannot determine the default cache root")]
    LocalAppDataMissing,
}
