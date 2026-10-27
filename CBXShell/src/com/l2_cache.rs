//! L2 thumbnail cache integration helpers (plan sections 4, 5.4, 8)
//!
//! Bridges the COM thumbnail flow ([`super::cbxshell`]) to the
//! `cbxshell-thumbnail-cache` crate and the canonical preview-proxy codec:
//!
//! * [`resolve_source_identity`] builds the O(metadata)
//!   [`SourceIdentity`] from either a filesystem path
//!   (IInitializeWithFile/IInitializeWithItem) or an `IStream::Stat` name
//!   (IInitializeWithStream). Streams without an absolute locator use a
//!   ZIP cover identity, reading only the directory and selected image.
//!   Other streams without an absolute locator bypass L2.
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

use sha2::{Digest, Sha256};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cbxshell_thumbnail_cache::key::{CacheVariant, SourceIdentity};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{
    CoTaskMemFree, IStream, STATFLAG_DEFAULT, STATSTG, STREAM_SEEK_CUR, STREAM_SEEK_SET,
};
use winreg::enums::*;
use winreg::RegKey;

use super::cbxshell::CBXShell;
use crate::image_processor::decoder::preview::{
    DecodedPreview, PreviewCacheCodec, WebpLosslessPreviewCodec, CACHE_CODEC_REVISION,
    PROXY_MAX_DIMENSION,
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
/// absolute path. ZIP streams with only a filename or no name hash the
/// selected image, preserving cover selection and avoiding archive-wide reads.
/// Other unresolved streams bypass L2. The original position is restored.
pub(crate) fn resolve_source_identity(
    file_path: Option<&Path>,
    stream: Option<&IStream>,
    sort: bool,
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
        return resolve_stream_content_identity(stream, sort);
    }

    if stat.pwcsName.is_null() {
        return resolve_stream_content_identity(stream, sort);
    }

    let raw_name = stat.pwcsName;
    let name_result = unsafe { raw_name.to_string() };
    unsafe {
        CoTaskMemFree(Some(raw_name.0 as _));
    }

    // Only an absolute, path-like name is a stable locator.
    let path = name_result
        .ok()
        .and_then(|name| CBXShell::normalize_stream_name_to_path(&name));
    let Some(path) = path else {
        return resolve_stream_content_identity(stream, sort);
    };

    let mut identity = SourceIdentity::from_path(&path);
    identity.size = stat.cbSize;
    identity.modified_filetime = filetime_to_u64(&stat.mtime);
    Some(identity)
}

fn resolve_stream_content_identity(stream: &IStream, sort: bool) -> Option<SourceIdentity> {
    let mut original_position = 0;
    unsafe { stream.Seek(0, STREAM_SEEK_CUR, Some(&mut original_position)) }.ok()?;
    let original_position = i64::try_from(original_position).ok()?;
    let result = zip_cover_identity(crate::archive::IStreamReader::new(stream.clone()), sort);
    unsafe { stream.Seek(original_position, STREAM_SEEK_SET, None) }.ok()?;
    result
}

fn zip_cover_identity(reader: impl Read + Seek, sort: bool) -> Option<SourceIdentity> {
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    let name = if sort {
        crate::archive::find_first_image(archive.file_names(), true)?
    } else {
        (0..archive.len()).find_map(|index| {
            let entry = archive.by_index(index).ok()?;
            crate::image_processor::is_image_file(entry.name()).then(|| entry.name().to_owned())
        })?
    };
    let mut entry = archive.by_name(&name).ok()?;
    const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;
    if entry.size() > MAX_COVER_BYTES || entry.compressed_size() > MAX_COVER_BYTES {
        return None;
    }
    let mut digest = Sha256::new();
    digest.update((name.len() as u64).to_le_bytes());
    digest.update(name.as_bytes());
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let count = entry.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        size = size.checked_add(count as u64)?;
        if size > MAX_COVER_BYTES {
            return None;
        }
        digest.update(&buffer[..count]);
    }
    let locator = format!("cbxshell:zip-cover:sha256:{:x}", digest.finalize());
    Some(SourceIdentity {
        locator_wide: locator.encode_utf16().collect(),
        size,
        modified_filetime: 0,
    })
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

pub(crate) fn encode_proxy(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    WebpLosslessPreviewCodec.encode(width, height, rgba)
}

pub(crate) fn decode_proxy(data: &[u8]) -> Result<DecodedPreview> {
    WebpLosslessPreviewCodec.decode(data)
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
            resolve_source_identity(Some(&path), None, false).expect("identity for a real file");

        assert!(!identity.locator_wide.is_empty());
        assert_eq!(identity.size, 4);
        assert!(identity.modified_filetime > 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolve_source_identity_missing_file_is_none() {
        let path = unique_temp_path("missing");
        assert!(resolve_source_identity(Some(&path), None, false).is_none());
    }

    #[test]
    fn cache_proxy_encoding_produces_a_webp_entry() {
        let rgba = vec![
            0xE0, 0x10, 0x20, 0xFF, 0x30, 0x40, 0x50, 0xFF, 0x60, 0x70, 0x80, 0xFF, 0x90, 0xA0,
            0xB0, 0xFF,
        ];

        let encoded = encode_proxy(2, 2, &rgba).expect("cache proxy should encode");

        assert_eq!(&encoded[0..4], b"RIFF");
        assert_eq!(&encoded[8..12], b"WEBP");
        let decoded = decode_proxy(&encoded).expect("cache proxy should decode");
        assert_eq!((decoded.width, decoded.height), (2, 2));
    }

    #[test]
    fn zip_identity_reads_only_selected_cover_for_both_sort_settings() {
        use std::io::{Cursor, SeekFrom, Write};

        struct CountedReader {
            cursor: Cursor<Vec<u8>>,
            bytes_read: usize,
        }
        impl Read for CountedReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let count = self.cursor.read(buffer)?;
                self.bytes_read += count;
                Ok(count)
            }
        }
        impl Seek for CountedReader {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.cursor.seek(position)
            }
        }

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("z.png", options).unwrap();
        writer.write_all(&[1; 4096]).unwrap();
        writer.start_file("bulk.bin", options).unwrap();
        writer.write_all(&vec![0; 16 * 1024 * 1024]).unwrap();
        writer.start_file("a.png", options).unwrap();
        writer.write_all(&[2; 2048]).unwrap();
        let mut reader = CountedReader {
            cursor: writer.finish().unwrap(),
            bytes_read: 0,
        };

        for (sort, expected_size) in [(false, 4096), (true, 2048)] {
            reader.bytes_read = 0;
            reader.cursor.set_position(0);
            let identity = zip_cover_identity(&mut reader, sort).unwrap();
            assert_eq!(identity.size, expected_size);
            assert!(
                reader.bytes_read < 256 * 1024,
                "read {} bytes",
                reader.bytes_read
            );
            eprintln!(
                "sort={sort}: {} bytes read from 16 MiB ZIP",
                reader.bytes_read
            );
        }
    }
}
