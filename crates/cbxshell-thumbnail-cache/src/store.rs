//! Sharded on-disk store with crash-safe entry publication (plan sections
//! 6 and 9).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::default_cache_root;
use crate::key::{hex_encode, CacheVariant, SourceIdentity};
use crate::{CacheError, Result};

/// Monotonic-per-process component of temporary file names. Combined with
/// the sub-second clock nanos, two threads in one process cannot collide
/// on a `.tmp` name.
static TMP_NONCE: AtomicU64 = AtomicU64::new(0);

fn next_nonce() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    nanos.wrapping_add(TMP_NONCE.fetch_add(1, Ordering::Relaxed))
}

/// Persistent thumbnail cache rooted at one directory.
///
/// Layout (plan section 6):
///
/// ```text
/// <root>/<2 hex chars of source_id>/<full source_id hex>/<variant_id hex>.webp
/// ```
///
/// The two-hex shard level keeps tens of thousands of sources out of one
/// directory. Invalidation of one archive is deleting one source directory;
/// stale variants of a changed file simply coexist until maintenance
/// removes them. There is no index and no lock anywhere.
pub struct ThumbnailCache {
    root: PathBuf,
}

impl ThumbnailCache {
    /// Use `root` as the cache root. It is created on demand; nothing is
    /// touched at construction time.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Use the plan-default root
    /// `%LOCALAPPDATA%\CBXShell\thumbnail-cache\v1`.
    pub fn with_default_root() -> Result<Self> {
        Ok(Self::new(default_cache_root()?))
    }

    /// The cache root, e.g. for GUI display (plan section 12.1).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/<shard>/<source hex>` directory for one source.
    pub(crate) fn source_dir(&self, source: &SourceIdentity) -> PathBuf {
        let hex = hex_encode(&source.source_id());
        self.root.join(&hex[..2]).join(hex)
    }

    /// Final file name for one source/variant pair.
    pub(crate) fn entry_path(&self, source: &SourceIdentity, variant: &CacheVariant) -> PathBuf {
        self.source_dir(source)
            .join(format!("{}.webp", hex_encode(&variant.variant_id())))
    }

    /// Load the encoded entry bytes, or `None` on a miss.
    ///
    /// # Errors
    ///
    /// Only genuine read failures (permissions, truncation races) are
    /// errors; a missing entry is a normal `Ok(None)`.
    pub fn load(&self, source: &SourceIdentity, variant: &CacheVariant) -> Result<Option<Vec<u8>>> {
        match fs::read(self.entry_path(source, variant)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(CacheError::Io(err)),
        }
    }

    /// Atomically publish `encoded` under the entry's final name.
    ///
    /// Writes `<variant>.<pid>.<nonce>.tmp` in the destination directory,
    /// flushes it to disk, then renames it into place. Concurrent writers
    /// of the same variant race without any lock (plan section 9): the
    /// loser of the race discards its temp file and reports success —
    /// duplicate work is preferable to contention in a shell extension.
    ///
    /// On Windows a rename over an existing destination reports
    /// `AlreadyExists`; that case is treated as success.
    pub fn store(
        &self,
        source: &SourceIdentity,
        variant: &CacheVariant,
        encoded: &[u8],
    ) -> Result<()> {
        let dir = self.source_dir(source);
        fs::create_dir_all(&dir)?;

        let variant_hex = hex_encode(&variant.variant_id());
        let final_path = dir.join(format!("{variant_hex}.webp"));
        let temp_path = dir.join(format!(
            "{variant_hex}.{}.{}.tmp",
            std::process::id(),
            next_nonce()
        ));

        let mut file = fs::File::create(&temp_path)?;
        file.write_all(encoded)?;
        // Flush before publishing so a crash can never leave a truncated
        // file under the final name.
        file.sync_all()?;
        drop(file);

        match fs::rename(&temp_path, &final_path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                // Another writer already published this exact variant.
                let _ = fs::remove_file(&temp_path);
                Ok(())
            }
            Err(err) => Err(CacheError::Io(err)),
        }
    }

    /// Best-effort delete of one entry; a missing file is success. This is
    /// the corrupt-entry recovery path (plan section 10).
    pub fn remove_entry(&self, source: &SourceIdentity, variant: &CacheVariant) -> Result<()> {
        match fs::remove_file(self.entry_path(source, variant)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(CacheError::Io(err)),
        }
    }

    /// Invalidate every cached variant of the archive at `path` by removing
    /// its whole source directory (plan section 6). A missing directory is
    /// success.
    pub fn invalidate_source(&self, path: &Path) -> Result<()> {
        let dir = self.source_dir(&SourceIdentity::from_path(path));
        match fs::remove_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(CacheError::Io(err)),
        }
    }

    /// Delete everything under the root, keeping the root directory itself.
    /// A missing root is success.
    pub fn clear_all_physical(&self) -> Result<()> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(CacheError::Io(err)),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn source(wide: &[u16]) -> SourceIdentity {
        SourceIdentity {
            locator_wide: wide.to_vec(),
            size: 1,
            modified_filetime: 2,
        }
    }

    fn variant() -> CacheVariant {
        CacheVariant {
            source_size: 10,
            source_mtime: 20,
            sort_images: true,
            renderer_revision: 1,
            cache_generation: 1,
            proxy_max_dimension: 1024,
            cache_codec_revision: 1,
        }
    }

    #[test]
    fn store_and_load_roundtrip() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        let variant = variant();

        cache.store(&source, &variant, b"proxy bytes").unwrap();
        assert_eq!(
            cache.load(&source, &variant).unwrap().as_deref(),
            Some(&b"proxy bytes"[..])
        );
    }

    #[test]
    fn load_missing_entry_is_none() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        assert_eq!(cache.load(&source(&[0x0041]), &variant()).unwrap(), None);
    }

    #[test]
    fn different_variant_misses() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        cache.store(&source, &variant(), b"a").unwrap();

        let other = CacheVariant {
            cache_generation: 2,
            ..variant()
        };
        assert_eq!(cache.load(&source, &other).unwrap(), None);
    }

    #[test]
    fn store_twice_is_ok_with_single_final_file_and_no_temp() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        let variant = variant();

        cache.store(&source, &variant, b"first").unwrap();
        cache.store(&source, &variant, b"second").unwrap();

        let dir = cache.source_dir(&source);
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let webp = names.iter().filter(|n| n.ends_with(".webp")).count();
        let tmp = names.iter().filter(|n| n.ends_with(".tmp")).count();
        assert_eq!(webp, 1, "exactly one final file, got {names:?}");
        assert_eq!(tmp, 0, "no leftover temp files, got {names:?}");
    }

    #[test]
    fn shard_and_source_dirs_are_created_on_demand() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);

        let dir = cache.source_dir(&source);
        assert!(!dir.exists());
        cache.store(&source, &variant(), b"x").unwrap();
        assert!(dir.is_dir());
        // Shard dir is the first two hex chars of the source id.
        let shard = hex_encode(&source.source_id());
        let expected_shard = root.path().join(&shard[..2]);
        assert!(expected_shard.is_dir());
    }

    #[test]
    fn remove_entry_deletes_only_that_variant() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        let source = source(&[0x0041]);
        let kept = variant();
        let removed = CacheVariant {
            source_mtime: 999,
            ..variant()
        };

        cache.store(&source, &kept, b"keep").unwrap();
        cache.store(&source, &removed, b"drop").unwrap();

        cache.remove_entry(&source, &removed).unwrap();
        assert_eq!(cache.load(&source, &removed).unwrap(), None);
        assert_eq!(
            cache.load(&source, &kept).unwrap().as_deref(),
            Some(&b"keep"[..])
        );

        // Removing again is fine (missing = success).
        cache.remove_entry(&source, &removed).unwrap();
    }

    #[test]
    fn invalidate_source_removes_whole_source_dir() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());

        let path_a = root.path().join("album a.cbz");
        let path_b = root.path().join("album b.cbz");
        let source_a = SourceIdentity::from_path(&path_a);
        let source_b = SourceIdentity::from_path(&path_b);

        cache.store(&source_a, &variant(), b"a").unwrap();
        cache.store(&source_b, &variant(), b"b").unwrap();

        cache.invalidate_source(&path_a).unwrap();

        assert_eq!(cache.load(&source_a, &variant()).unwrap(), None);
        assert!(!cache.source_dir(&source_a).exists());
        assert_eq!(
            cache.load(&source_b, &variant()).unwrap().as_deref(),
            Some(&b"b"[..])
        );

        // Invalidating an unknown path is success.
        cache
            .invalidate_source(&root.path().join("never cached.cbz"))
            .unwrap();
    }

    #[test]
    fn clear_all_physical_empties_root_but_keeps_root() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().to_path_buf());
        cache.store(&source(&[0x0041]), &variant(), b"a").unwrap();
        cache.store(&source(&[0x0042]), &variant(), b"b").unwrap();

        cache.clear_all_physical().unwrap();

        assert!(root.path().is_dir());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn operations_on_missing_root_are_graceful() {
        let root = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(root.path().join("does-not-exist"));

        assert_eq!(cache.load(&source(&[0x0041]), &variant()).unwrap(), None);
        cache.remove_entry(&source(&[0x0041]), &variant()).unwrap();
        cache.invalidate_source(&root.path().join("x.zip")).unwrap();
        cache.clear_all_physical().unwrap();
    }
}
