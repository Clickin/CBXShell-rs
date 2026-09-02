//! Cache key computation (plan section 5).
//!
//! Two independent identities locate one cache entry:
//!
//! * [`SourceIdentity::source_id`] — identifies the logical archive
//!   location; used as the directory name.
//! * [`CacheVariant::variant_id`] — identifies one valid rendering of that
//!   source (source metadata plus rendering/configuration state); used as
//!   the file name.
//!
//! # Path normalization policy (v1)
//!
//! The locator is used **exactly as provided**: no case folding and no
//! canonicalization (no `\\?\` prefixing, short-name expansion, or symlink
//! resolution). Explorer supplies consistent absolute paths for a given
//! item, and plan section 4.1 only requires O(metadata) lookups, so a rare
//! differently-cased locator costs one redundant render rather than a
//! correctness bug. This is a documented simplification; revisit only with
//! real-world evidence.

use std::path::Path;

use sha2::{Digest, Sha256};

/// Layout version of the [`CacheVariant`] canonical serialization. Bump it
/// when the field list or encoding changes so stale entries simply vanish.
const VARIANT_LAYOUT_VERSION: u8 = 1;

/// Serialized size of the canonical [`CacheVariant`] form:
/// 1 version tag + 8 + 8 + 1 + 4 + 8 + 2 + 2.
const VARIANT_SERIALIZED_LEN: usize = 34;

/// Stable metadata identity of one archive file (plan section 4).
///
/// `locator_wide` is the absolute Windows path as UTF-16 code units (what
/// `OsStr::encode_wide` produces). Hashing raw code units — never a UTF-8
/// or lossy re-encoding — keeps paths that are not representable in UTF-8
/// (e.g. containing unpaired surrogates) distinct (plan section 4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity {
    pub locator_wide: Vec<u16>,
    pub size: u64,
    pub modified_filetime: u64,
}

impl SourceIdentity {
    /// Build an identity from a filesystem path, encoding the locator as
    /// raw UTF-16 code units. Size and mtime start at zero; callers fill
    /// them from file metadata afterwards.
    ///
    /// Deliberately not `to_string_lossy`: lossy conversion would collapse
    /// distinct non-UTF-8-representable paths onto the same key.
    pub fn from_path(path: &Path) -> Self {
        use std::os::windows::ffi::OsStrExt;

        Self {
            locator_wide: path.as_os_str().encode_wide().collect(),
            size: 0,
            modified_filetime: 0,
        }
    }

    /// SHA-256 over the UTF-16LE bytes of the locator only.
    ///
    /// Size and mtime are intentionally excluded: they belong to the
    /// variant identity so a changed file lands in the *same* source
    /// directory, letting maintenance retire the old variant in place
    /// (plan sections 5.1-5.2 and 6).
    pub fn source_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for unit in &self.locator_wide {
            hasher.update(unit.to_le_bytes());
        }
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }
}

/// Everything besides the requested size that changes the rendered pixels
/// (plan section 5.2).
///
/// There is deliberately **no requested-size (`cx`) field** — every
/// Explorer size is derived from one canonical proxy (plan section 2.1) —
/// and **no source-codec field** — a source format change necessarily
/// changes the file metadata, which already produces a different variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheVariant {
    pub source_size: u64,
    pub source_mtime: u64,
    pub sort_images: bool,
    pub renderer_revision: u32,
    pub cache_generation: u64,
    pub proxy_max_dimension: u16,
    pub cache_codec_revision: u16,
}

impl CacheVariant {
    /// SHA-256 over the canonical serialization of the variant.
    ///
    /// Layout (34 bytes, integers little-endian, `bool` as one 0/1 byte),
    /// fields in declaration order:
    ///
    /// ```text
    /// offset  len  field
    /// ------  ---  ---------------------------
    ///      0    1  layout version tag (= 1)
    ///      1    8  source_size (u64)
    ///      9    8  source_mtime (u64)
    ///     17    1  sort_images (bool)
    ///     18    4  renderer_revision (u32)
    ///     22    8  cache_generation (u64)
    ///     30    2  proxy_max_dimension (u16)
    ///     32    2  cache_codec_revision (u16)
    /// ```
    pub fn variant_id(&self) -> [u8; 32] {
        let mut buf = [0u8; VARIANT_SERIALIZED_LEN];
        buf[0] = VARIANT_LAYOUT_VERSION;
        buf[1..9].copy_from_slice(&self.source_size.to_le_bytes());
        buf[9..17].copy_from_slice(&self.source_mtime.to_le_bytes());
        buf[17] = u8::from(self.sort_images);
        buf[18..22].copy_from_slice(&self.renderer_revision.to_le_bytes());
        buf[22..30].copy_from_slice(&self.cache_generation.to_le_bytes());
        buf[30..32].copy_from_slice(&self.proxy_max_dimension.to_le_bytes());
        buf[32..34].copy_from_slice(&self.cache_codec_revision.to_le_bytes());
        sha256(&buf)
    }
}

/// Lowercase hex encoding, used to turn IDs into filesystem-safe names.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
    }
    out
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(wide: &[u16]) -> SourceIdentity {
        SourceIdentity {
            locator_wide: wide.to_vec(),
            size: 10,
            modified_filetime: 20,
        }
    }

    fn base_variant() -> CacheVariant {
        CacheVariant {
            source_size: 100,
            source_mtime: 200,
            sort_images: true,
            renderer_revision: 1,
            cache_generation: 7,
            proxy_max_dimension: 1024,
            cache_codec_revision: 1,
        }
    }

    #[test]
    fn source_id_is_deterministic() {
        let first = identity(&[0x0041, 0x002E, 0x007A, 0x0069, 0x0070]).source_id();
        let second = identity(&[0x0041, 0x002E, 0x007A, 0x0069, 0x0070]).source_id();
        assert_eq!(first, second);
    }

    #[test]
    fn source_id_differs_for_different_locators() {
        assert_ne!(
            identity(&[0x0041]).source_id(),
            identity(&[0x0042]).source_id()
        );
        assert_ne!(
            identity(&[0x0041, 0x0042]).source_id(),
            identity(&[0x0042, 0x0041]).source_id()
        );
    }

    #[test]
    fn variant_id_is_deterministic() {
        assert_eq!(base_variant().variant_id(), base_variant().variant_id());
    }

    #[test]
    fn variant_id_changes_when_any_field_changes() {
        let base = base_variant();
        let base_id = base.variant_id();

        let mutations = [
            (
                "source_size",
                CacheVariant {
                    source_size: 101,
                    ..base
                },
            ),
            (
                "source_mtime",
                CacheVariant {
                    source_mtime: 201,
                    ..base
                },
            ),
            (
                "sort_images",
                CacheVariant {
                    sort_images: false,
                    ..base
                },
            ),
            (
                "renderer_revision",
                CacheVariant {
                    renderer_revision: 2,
                    ..base
                },
            ),
            (
                "cache_generation",
                CacheVariant {
                    cache_generation: 8,
                    ..base
                },
            ),
            (
                "proxy_max_dimension",
                CacheVariant {
                    proxy_max_dimension: 512,
                    ..base
                },
            ),
            (
                "cache_codec_revision",
                CacheVariant {
                    cache_codec_revision: 2,
                    ..base
                },
            ),
        ];

        for (field, mutated) in mutations {
            assert_ne!(
                mutated.variant_id(),
                base_id,
                "variant_id must change when {field} changes"
            );
        }
    }

    #[test]
    fn non_bmp_locator_hash_is_utf16_not_utf8_or_lossy() {
        // U+1D11E (MUSICAL SYMBOL G CLEF) as a hand-built UTF-16 surrogate
        // pair so the test never depends on any path-encoding behavior.
        let wide = vec![0x0041, 0xD834, 0xDD1E];
        let utf16_id = identity(&wide).source_id();

        // Hashing the same logical string as UTF-8 bytes must differ.
        let utf8_digest = Sha256::digest("A\u{1D11E}".as_bytes());
        assert_ne!(&utf8_digest[..], &utf16_id[..]);

        // Lossy UTF-16 (replacement character) must differ too.
        let lossy: Vec<u16> = "A\u{FFFD}".encode_utf16().collect();
        assert_ne!(identity(&lossy).source_id(), utf16_id);
    }

    #[test]
    fn from_path_hashes_raw_utf16_code_units() {
        use std::os::windows::ffi::OsStringExt;

        // An unpaired low surrogate cannot be represented in UTF-8 or in a
        // Rust `str`, but it is valid WTF-16 path data on Windows.
        let raw: Vec<u16> = vec![0x0043, 0x003A, 0x005C, 0xDC00];
        let path = std::ffi::OsString::from_wide(&raw);
        let via_path = SourceIdentity::from_path(Path::new(&path));

        assert_eq!(via_path.locator_wide, raw);

        // It must not collapse onto the lossy replacement encoding.
        let lossy: Vec<u16> = "C:\u{5C}\u{FFFD}".encode_utf16().collect();
        assert_ne!(via_path.source_id(), identity(&lossy).source_id());
    }
}
