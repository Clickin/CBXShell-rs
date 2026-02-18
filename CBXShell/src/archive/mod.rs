use crate::utils::error::{CbxError, Result};
///! Archive format handling
///!
///! Supports ZIP, RAR, and 7z formats for comic book archives
use std::path::Path;

mod config;
mod rar;
mod sevenz;
pub mod stream_reader;
mod utils;
mod zip;

// Re-export utilities for internal use only (not used in public API)
pub use config::should_sort_images;

// Re-export image verification function (used by COM shell extension)
pub use utils::verify_image_data;

#[allow(dead_code)] // Used by open_archive function and part of public API
pub use rar::RarArchive;
#[allow(dead_code)] // Used by open_archive function and part of public API
pub use sevenz::SevenZipArchive;
#[allow(dead_code)] // Used by open_archive function and part of public API
pub use zip::ZipArchive;

// Re-export stream reader utilities (detect_archive_type_from_bytes is used publicly)
pub use stream_reader::{detect_archive_type_from_bytes, IStreamReader};

/// Represents an entry in an archive
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    #[allow(dead_code)] // Part of public API, may be used in future
    pub is_directory: bool,
}

/// Archive metadata
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of public API, may be used in future
pub struct ArchiveMetadata {
    pub total_files: usize,
    pub image_count: usize,
    pub compressed_size: u64,
    pub archive_type: ArchiveType,
}

/// Archive type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    Zip,
    Rar,
    SevenZip,
}

impl ArchiveType {
    /// Detect archive type from file extension
    #[allow(dead_code)] // Part of public API, may be used in future
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "zip" | "cbz" | "epub" | "phz" => Some(Self::Zip),
            "rar" | "cbr" => Some(Self::Rar),
            "7z" | "cb7" => Some(Self::SevenZip),
            _ => None,
        }
    }

    #[allow(dead_code)] // Part of public API, may be used in future
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Rar => "RAR",
            Self::SevenZip => "7-Zip",
        }
    }
}

/// Archive trait for different archive formats
#[allow(dead_code)] // Part of public API, used by archive implementations
pub trait Archive {
    /// Open an archive from a file path
    fn open(path: &Path) -> Result<Box<dyn Archive>>
    where
        Self: Sized;

    /// Find the first image in the archive (optionally sorted alphabetically)
    fn find_first_image(&self, sort: bool) -> Result<ArchiveEntry>;

    /// Extract an entry to a byte vector
    fn extract_entry(&self, entry: &ArchiveEntry) -> Result<Vec<u8>>;

    /// Get archive metadata
    fn get_metadata(&self) -> Result<ArchiveMetadata>;

    /// Get archive type
    fn archive_type(&self) -> ArchiveType;
}

/// Open an archive of any supported type from a file path
#[allow(dead_code)] // Part of public API, may be used in future
pub fn open_archive(path: &Path) -> Result<Box<dyn Archive>> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or(CbxError::InvalidPath)?;

    let archive_type = ArchiveType::from_extension(extension)
        .ok_or_else(|| CbxError::UnsupportedFormat(extension.to_string()))?;

    match archive_type {
        ArchiveType::Zip => <ZipArchive as Archive>::open(path),
        ArchiveType::Rar => <RarArchive as Archive>::open(path),
        ArchiveType::SevenZip => <SevenZipArchive as Archive>::open(path),
    }
}

/// Open an archive from a stream (OPTIMIZED for IStream)
///
/// This function provides significant performance improvements by streaming data directly
/// instead of loading the entire archive into memory first.
///
/// # Performance Comparison (1GB archive)
/// - **Memory-based**: Load 1GB to memory (~3s) + process
/// - **Stream-based**: Stream directly (~50-100ms for metadata + image)
///
/// # Supported Formats
/// - **ZIP**: Direct streaming (20-50x faster for large archives)
/// - **RAR**: Streaming write to temp file (2-3x faster, temp file still required)
/// - **7z**: Streaming with RefCell pattern (19-28x faster for large archives)
///
/// # Arguments
/// * `reader` - Any Read implementer (IStreamReader, File, etc.)
///
/// # Returns
/// * `Ok(Box<dyn Archive>)` - Opened archive handler
/// * `Err(CbxError)` - If the format is unsupported or opening fails
///
/// # Example
/// ```ignore
/// use cbxshell::archive::{open_archive_from_stream, IStreamReader};
/// use windows::Win32::System::Com::IStream;
///
/// let stream: IStream = ...; // from IInitializeWithStream
/// let reader = IStreamReader::new(stream);
/// let archive = open_archive_from_stream(reader)?;
/// let entry = archive.find_first_image(true)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn open_archive_from_stream<R: std::io::Read + std::io::Seek + 'static>(
    mut reader: R,
) -> Result<Box<dyn Archive>> {
    use std::io::SeekFrom;

    crate::utils::debug_log::debug_log(">>>>> open_archive_from_stream STARTING (OPTIMIZED) <<<<<");

    // Read first 16 bytes for magic byte detection
    let mut magic_bytes = [0u8; 16];
    reader
        .read_exact(&mut magic_bytes)
        .map_err(|e| CbxError::Archive(format!("Failed to read magic bytes: {}", e)))?;

    // Detect archive type
    let archive_type = detect_archive_type_from_bytes(&magic_bytes)?;
    crate::utils::debug_log::debug_log(&format!("Detected archive type: {:?}", archive_type));

    // Seek back to beginning
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

    match archive_type {
        ArchiveType::Zip => {
            // ZIP: Direct streaming (FASTEST!)
            crate::utils::debug_log::debug_log("Using optimized ZIP streaming");
            Ok(Box::new(zip::ZipArchiveFromStream::new(reader)?))
        }
        ArchiveType::Rar => {
            // RAR: Stream to temp file (OPTIMIZED)
            crate::utils::debug_log::debug_log("Using optimized RAR streaming to temp file");
            Ok(Box::new(rar::RarArchiveFromMemory::new_from_stream(
                reader,
            )?))
        }
        ArchiveType::SevenZip => {
            // 7z: Streaming with RefCell (OPTIMIZED!)
            crate::utils::debug_log::debug_log("Using optimized 7z streaming");
            Ok(Box::new(sevenz::SevenZipArchiveFromStream::new(reader)?))
        }
    }
}
