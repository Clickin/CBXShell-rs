//! IStream-based archive reading for IThumbnailProvider
//!
//! This module provides utilities for reading archives from IStream interfaces
//! instead of file paths, which is required for IThumbnailProvider.

use crate::archive::ArchiveType;
use crate::utils::error::{CbxError, Result};
use std::io::{self, Read, Seek, SeekFrom};
use windows::Win32::System::Com::*;

/// IStream adapter that implements Read and Seek traits
///
/// This wrapper allows using Windows IStream with Rust libraries that expect
/// std::io::Read and std::io::Seek traits (like zip, sevenz-rust).
///
/// # Benefits
/// - **No memory copy**: Streams data directly from IStream
/// - **Fast**: Avoids loading entire archive into memory
/// - **Efficient**: Only reads what's needed for metadata and target file
///
/// # Example
/// ```no_run
/// let stream: IStream = ...; // from IInitializeWithStream
/// let reader = IStreamReader::new(stream);
/// let archive = ZipArchive::new(reader)?; // Direct streaming!
/// ```
pub struct IStreamReader {
    stream: IStream,
    position: u64,
}

impl IStreamReader {
    /// Create a new IStreamReader from an IStream
    pub fn new(stream: IStream) -> Self {
        unsafe {
            let mut pos = 0u64;
            let _ = stream.Seek(0, STREAM_SEEK_SET, Some(&mut pos));
        }

        Self {
            stream,
            position: 0,
        }
    }
}

impl Read for IStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        // UNAVOIDABLE UNSAFE: IStream::Read is a COM method
        // Why unsafe is required:
        // 1. COM method call: IStream::Read uses C++ vtable
        // 2. Raw buffer pointer: COM API requires *mut c_void
        // 3. No safe alternative: This adapter enables using zip/7z crates
        //
        // Safety guarantees:
        // - buf is valid mutable slice (Rust guarantees)
        // - Buffer size passed correctly (buf.len())
        // - bytes_read validated before use
        unsafe {
            let mut bytes_read = 0u32;
            let hr = self.stream.Read(
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                Some(&mut bytes_read),
            );

            if hr.is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("IStream::Read failed: {:?}", hr),
                ));
            }

            self.position += bytes_read as u64;
            Ok(bytes_read as usize)
        }
    }
}

impl Seek for IStreamReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        // UNAVOIDABLE UNSAFE: IStream::Seek is a COM method
        // Why unsafe is required:
        // 1. COM method call: IStream::Seek uses C++ vtable
        // 2. No safe alternative: Required for archive reading
        //
        // Safety guarantees:
        // - stream is valid (owned by self)
        // - new_position properly initialized and checked
        unsafe {
            let (offset, origin) = match pos {
                SeekFrom::Start(n) => (n as i64, STREAM_SEEK_SET),
                SeekFrom::End(n) => (n, STREAM_SEEK_END),
                SeekFrom::Current(n) => (n, STREAM_SEEK_CUR),
            };

            let mut new_position = 0u64;
            self.stream
                .Seek(offset, origin, Some(&mut new_position))
                .map_err(|e| {
                    io::Error::new(io::ErrorKind::Other, format!("IStream::Seek failed: {}", e))
                })?;

            self.position = new_position;
            Ok(new_position)
        }
    }
}

/// Detect archive type from magic bytes
///
/// This function inspects the first few bytes of data to determine the archive type.
/// This is more reliable than extension-based detection for IStream sources.
///
/// # Magic Bytes
/// - ZIP: `50 4B 03 04` or `50 4B 05 06` or `50 4B 07 08` (PK\x03\x04, PK\x05\x06, PK\x07\x08)
/// - RAR: `52 61 72 21 1A 07 00` (Rar!\x1A\x07\x00) - RAR 4.x
/// - RAR5: `52 61 72 21 1A 07 01 00` (Rar!\x1A\x07\x01\x00) - RAR 5.x
/// - 7z: `37 7A BC AF 27 1C` (7z¼¯'\x1C)
///
/// # Arguments
/// * `data` - The raw archive data (at least first 16 bytes)
///
/// # Returns
/// * `Ok(ArchiveType)` - The detected archive type
/// * `Err(CbxError)` - If the format is not recognized
pub fn detect_archive_type_from_bytes(data: &[u8]) -> Result<ArchiveType> {
    crate::utils::debug_log::debug_log(">>>>> detect_archive_type_from_bytes STARTING <<<<<");

    if data.len() < 8 {
        crate::utils::debug_log::debug_log(&format!("ERROR: Data too short: {} bytes", data.len()));
        return Err(CbxError::UnsupportedFormat("Data too short".to_string()));
    }

    // Log first 16 bytes as hex for debugging
    let preview_len = data.len().min(16);
    let hex_preview: Vec<String> = data[..preview_len]
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect();
    crate::utils::debug_log::debug_log(&format!(
        "First {} bytes: {}",
        preview_len,
        hex_preview.join(" ")
    ));

    // Check ZIP magic bytes
    if data.len() >= 4 {
        let magic = &data[0..4];
        if magic == b"PK\x03\x04" || magic == b"PK\x05\x06" || magic == b"PK\x07\x08" {
            crate::utils::debug_log::debug_log("Detected: ZIP format");
            return Ok(ArchiveType::Zip);
        }
    }

    // Check 7z magic bytes
    if data.len() >= 6 {
        let magic = &data[0..6];
        if magic == b"7z\xBC\xAF\x27\x1C" {
            crate::utils::debug_log::debug_log("Detected: 7-Zip format");
            return Ok(ArchiveType::SevenZip);
        }
    }

    // Check RAR magic bytes (RAR 4.x and 5.x)
    if data.len() >= 7 {
        let magic = &data[0..7];
        if magic == b"Rar!\x1A\x07\x00" {
            crate::utils::debug_log::debug_log("Detected: RAR 4.x format");
            return Ok(ArchiveType::Rar);
        }
    }

    if data.len() >= 8 {
        let magic = &data[0..8];
        if magic == b"Rar!\x1A\x07\x01\x00" {
            crate::utils::debug_log::debug_log("Detected: RAR 5.x format");
            return Ok(ArchiveType::Rar);
        }
    }

    crate::utils::debug_log::debug_log("ERROR: Unrecognized archive format");
    Err(CbxError::UnsupportedFormat(
        "Unrecognized archive format".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_zip_format() {
        // ZIP local file header signature
        let zip_data = b"PK\x03\x04\x14\x00\x00\x00\x08\x00";
        assert_eq!(
            detect_archive_type_from_bytes(zip_data).unwrap(),
            ArchiveType::Zip
        );
    }

    #[test]
    fn test_detect_zip_central_directory() {
        // ZIP central directory signature
        let zip_data = b"PK\x05\x06\x00\x00\x00\x00";
        assert_eq!(
            detect_archive_type_from_bytes(zip_data).unwrap(),
            ArchiveType::Zip
        );
    }

    #[test]
    fn test_detect_7z_format() {
        let sevenz_data = b"7z\xBC\xAF\x27\x1C\x00\x00";
        assert_eq!(
            detect_archive_type_from_bytes(sevenz_data).unwrap(),
            ArchiveType::SevenZip
        );
    }

    #[test]
    fn test_detect_rar4_format() {
        let rar_data = b"Rar!\x1A\x07\x00\x00";
        assert_eq!(
            detect_archive_type_from_bytes(rar_data).unwrap(),
            ArchiveType::Rar
        );
    }

    #[test]
    fn test_detect_rar5_format() {
        let rar_data = b"Rar!\x1A\x07\x01\x00";
        assert_eq!(
            detect_archive_type_from_bytes(rar_data).unwrap(),
            ArchiveType::Rar
        );
    }

    #[test]
    fn test_detect_unknown_format() {
        let unknown_data = b"UNKNOWN\x00\x00\x00\x00";
        assert!(detect_archive_type_from_bytes(unknown_data).is_err());
    }

    #[test]
    fn test_detect_data_too_short() {
        let short_data = b"PK";
        assert!(detect_archive_type_from_bytes(short_data).is_err());
    }
}
