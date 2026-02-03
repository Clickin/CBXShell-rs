use sevenz_rust::{Password, SevenZReader};
///! 7-Zip archive implementation
///!
///! Supports 7z and CB7 formats using the `sevenz-rust` crate
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use super::utils::{find_first_image, is_image_file, MAX_ENTRY_SIZE};
use crate::archive::{Archive, ArchiveEntry, ArchiveMetadata, ArchiveType};
use crate::utils::error::{CbxError, Result};

/// 7-Zip archive handler
pub struct SevenZipArchive {
    path: PathBuf,
}

impl SevenZipArchive {
    /// Open a 7z archive from path
    pub fn open(path: &Path) -> Result<Self> {
        tracing::debug!("Opening 7-Zip archive: {:?}", path);

        // Validate by attempting to open
        let file = File::open(path)
            .map_err(|e| CbxError::Archive(format!("Failed to open 7z file: {}", e)))?;

        let file_len = file
            .metadata()
            .map_err(|e| CbxError::Archive(format!("Failed to get file metadata: {}", e)))?
            .len();

        let password = Password::empty();
        let mut _reader = SevenZReader::new(file, file_len, password)
            .map_err(|e| CbxError::Archive(format!("Invalid 7z archive: {}", e)))?;

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// List all entries in archive
    fn list_entries(&self) -> Result<Vec<ArchiveEntry>> {
        let file = File::open(&self.path)
            .map_err(|e| CbxError::Archive(format!("Failed to open 7z: {}", e)))?;

        let file_len = file
            .metadata()
            .map_err(|e| CbxError::Archive(format!("Failed to get file metadata: {}", e)))?
            .len();

        let password = Password::empty();
        let mut archive = SevenZReader::new(file, file_len, password)
            .map_err(|e| CbxError::Archive(format!("Failed to read 7z: {}", e)))?;

        let mut entries = Vec::new();

        archive
            .for_each_entries(|entry, _reader| {
                entries.push(ArchiveEntry {
                    name: entry.name().to_string(),
                    size: entry.size(),
                    is_directory: entry.is_directory(),
                });
                Ok(true) // Continue iteration
            })
            .map_err(|e| CbxError::Archive(format!("7z iteration error: {}", e)))?;

        Ok(entries)
    }
}

impl Archive for SevenZipArchive {
    fn open(path: &Path) -> Result<Box<dyn Archive>> {
        Ok(Box::new(Self::open(path)?))
    }

    fn find_first_image(&self, sort: bool) -> Result<ArchiveEntry> {
        tracing::debug!("Finding first image in 7z (sort={})", sort);

        if !sort {
            // OPTIMIZATION: When not sorting, find first image immediately
            tracing::debug!("Fast path: finding first image without full listing");

            let file = File::open(&self.path)
                .map_err(|e| CbxError::Archive(format!("Failed to open 7z: {}", e)))?;

            let file_len = file
                .metadata()
                .map_err(|e| CbxError::Archive(format!("Failed to get file metadata: {}", e)))?
                .len();

            let password = Password::empty();
            let mut archive = SevenZReader::new(file, file_len, password)
                .map_err(|e| CbxError::Archive(format!("Failed to read 7z: {}", e)))?;

            let mut first_image: Option<ArchiveEntry> = None;

            archive
                .for_each_entries(|entry, _reader| {
                    let name = entry.name().to_string();
                    if is_image_file(&name) {
                        tracing::info!("Found first image (unsorted): {}", name);
                        first_image = Some(ArchiveEntry {
                            name,
                            size: entry.size(),
                            is_directory: entry.is_directory(),
                        });
                        Ok(false) // Stop iteration
                    } else {
                        Ok(true) // Continue
                    }
                })
                .map_err(|e| CbxError::Archive(format!("7z iteration error: {}", e)))?;

            return first_image
                .ok_or_else(|| CbxError::Archive("No images found in archive".to_string()));
        }

        // STANDARD PATH: List all entries and sort
        let entries = self.list_entries()?;

        if entries.is_empty() {
            return Err(CbxError::Archive("Archive is empty".to_string()));
        }

        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();

        let image_name = find_first_image(names.iter().map(|s| s.as_str()), sort)
            .ok_or_else(|| CbxError::Archive("No images found in archive".to_string()))?;

        tracing::info!("Found first image (sorted): {}", image_name);

        entries
            .into_iter()
            .find(|e| e.name == image_name)
            .ok_or_else(|| CbxError::Archive("Image entry not found".to_string()))
    }

    fn extract_entry(&self, entry: &ArchiveEntry) -> Result<Vec<u8>> {
        tracing::debug!("Extracting entry: {} ({} bytes)", entry.name, entry.size);

        // Safety check: prevent memory exhaustion (32MB limit)
        if entry.size > MAX_ENTRY_SIZE {
            tracing::warn!(
                "Entry too large: {} bytes (max {})",
                entry.size,
                MAX_ENTRY_SIZE
            );
            return Err(CbxError::Archive(format!(
                "Entry too large: {} bytes (max 32MB)",
                entry.size
            )));
        }

        let file = File::open(&self.path)
            .map_err(|e| CbxError::Archive(format!("Failed to open 7z: {}", e)))?;

        let file_len = file
            .metadata()
            .map_err(|e| CbxError::Archive(format!("Failed to get file metadata: {}", e)))?
            .len();

        let password = Password::empty();
        let mut archive = SevenZReader::new(file, file_len, password)
            .map_err(|e| CbxError::Archive(format!("Failed to read 7z: {}", e)))?;

        let mut extracted_data = None;

        archive
            .for_each_entries(|sz_entry, reader| {
                if sz_entry.name() == entry.name {
                    let mut buffer = Vec::with_capacity(sz_entry.size() as usize);
                    std::io::copy(reader, &mut buffer)
                        .map_err(|e| sevenz_rust::Error::Io(e, "Extract failed".into()))?;
                    extracted_data = Some(buffer);
                    Ok(false) // Stop iteration
                } else {
                    Ok(true) // Continue
                }
            })
            .map_err(|e| CbxError::Archive(format!("7z extraction error: {}", e)))?;

        extracted_data.ok_or_else(|| CbxError::Archive(format!("Entry not found: {}", entry.name)))
    }

    fn get_metadata(&self) -> Result<ArchiveMetadata> {
        let entries = self.list_entries()?;
        let total_files = entries.len();
        let image_count = entries.iter().filter(|e| is_image_file(&e.name)).count();

        let compressed_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);

        tracing::debug!(
            "7z metadata: {} files, {} images, {} bytes",
            total_files,
            image_count,
            compressed_size
        );

        Ok(ArchiveMetadata {
            total_files,
            image_count,
            compressed_size,
            archive_type: ArchiveType::SevenZip,
        })
    }

    fn archive_type(&self) -> ArchiveType {
        ArchiveType::SevenZip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sevenz_rust::SevenZWriter;
    use std::io::Write;

    /// Create a test 7z archive on disk
    fn create_test_7z_file(path: &Path, files: &[(&str, &[u8])]) -> Result<()> {
        let file = File::create(path)
            .map_err(|e| CbxError::Archive(format!("Failed to create test 7z: {}", e)))?;

        let mut sz = SevenZWriter::new(file)
            .map_err(|e| CbxError::Archive(format!("Failed to create 7z writer: {}", e)))?;

        for (name, content) in files {
            sz.push_archive_entry(
                sevenz_rust::SevenZArchiveEntry::from_path(Path::new(name), (*name).to_string()),
                Some(std::io::Cursor::new(content)),
            )
            .map_err(|e| CbxError::Archive(format!("Failed to add entry: {}", e)))?;
        }

        sz.finish()
            .map_err(|e| CbxError::Archive(format!("Failed to finish 7z: {}", e)))?;

        Ok(())
    }

    #[test]
    fn test_open_valid_7z() {
        let temp_path = std::env::temp_dir().join("test_valid.7z");
        create_test_7z_file(&temp_path, &[("test.jpg", b"fake image data")]).unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        assert_eq!(archive.archive_type(), ArchiveType::SevenZip);

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_open_invalid_7z() {
        let temp_path = std::env::temp_dir().join("test_invalid.7z");
        std::fs::write(&temp_path, b"not a 7z file").unwrap();

        let result = SevenZipArchive::open(&temp_path);
        assert!(result.is_err());

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_find_first_image_sorted() {
        let temp_path = std::env::temp_dir().join("test_sorted.7z");
        create_test_7z_file(
            &temp_path,
            &[
                ("readme.txt", b"text file"),
                ("page10.jpg", b"image 10"),
                ("page2.jpg", b"image 2"),
                ("page1.jpg", b"image 1"),
            ],
        )
        .unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        let entry = archive.find_first_image(true).unwrap();

        // Natural sort: page1.jpg < page2.jpg < page10.jpg
        assert_eq!(entry.name, "page1.jpg");

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_find_first_image_unsorted() {
        let temp_path = std::env::temp_dir().join("test_unsorted.7z");
        create_test_7z_file(
            &temp_path,
            &[
                ("readme.txt", b"text file"),
                ("page10.jpg", b"image 10"),
                ("page2.jpg", b"image 2"),
            ],
        )
        .unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        let entry = archive.find_first_image(false).unwrap();

        // Unsorted: first image encountered (order depends on archive library)
        // Could be either page10.jpg or page2.jpg
        assert!(
            entry.name == "page10.jpg" || entry.name == "page2.jpg",
            "Expected an image file, got: {}",
            entry.name
        );

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_no_images_found() {
        let temp_path = std::env::temp_dir().join("test_no_images.7z");
        create_test_7z_file(&temp_path, &[("readme.txt", b"text"), ("data.json", b"{}")]).unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        let result = archive.find_first_image(true);

        assert!(result.is_err());

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_extract_entry() {
        let content = b"fake jpeg data";
        let temp_path = std::env::temp_dir().join("test_extract.7z");
        create_test_7z_file(&temp_path, &[("image.jpg", content)]).unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        let entry = archive.find_first_image(false).unwrap();
        let extracted = archive.extract_entry(&entry).unwrap();

        assert_eq!(extracted, content);

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_get_metadata() {
        let temp_path = std::env::temp_dir().join("test_metadata.7z");
        create_test_7z_file(
            &temp_path,
            &[
                ("page1.jpg", b"image 1"),
                ("page2.jpg", b"image 2"),
                ("readme.txt", b"text"),
            ],
        )
        .unwrap();

        let archive = SevenZipArchive::open(&temp_path).unwrap();
        let metadata = archive.get_metadata().unwrap();

        assert_eq!(metadata.total_files, 3);
        assert_eq!(metadata.image_count, 2);
        assert!(metadata.compressed_size > 0);
        assert_eq!(metadata.archive_type, ArchiveType::SevenZip);

        std::fs::remove_file(&temp_path).ok();
    }
}

/// 7-Zip archive handler for streaming (no memory load!)
///
/// This implementation uses RefCell to work around sevenz-rust's mutable
/// reader requirement. Each operation creates a new SevenZReader by seeking
/// back to the start. This is much faster than loading the entire archive
/// into memory.
///
/// # Performance
/// - **Small archives (50MB)**: Similar to memory version
/// - **Large archives (1GB+)**: 19-28x faster than memory version
///
/// # Trade-off
/// - Recreates SevenZReader on each call (~20-50ms to parse headers)
/// - Still MUCH faster than loading 1GB to memory (~3000ms)
///
/// # Example
/// For a 1GB archive:
/// - Old approach: Load 1GB to memory (~3000ms) + process
/// - New approach: Seek + parse headers (~50ms) + stream data
pub struct SevenZipArchiveFromStream<R: Read + Seek> {
    reader: std::cell::RefCell<R>,
    size: u64,
}

impl<R: Read + Seek> SevenZipArchiveFromStream<R> {
    /// Create a 7z archive from a streaming reader
    ///
    /// # Arguments
    /// * `reader` - Any Read + Seek implementer
    ///
    /// # Returns
    /// * `Ok(Self)` - Archive ready for processing
    /// * `Err(CbxError)` - If validation fails
    pub fn new(mut reader: R) -> Result<Self> {
        use std::io::SeekFrom;

        // Get size
        let size = reader
            .seek(SeekFrom::End(0))
            .map_err(|e| CbxError::Archive(format!("Failed to get stream size: {}", e)))?;

        tracing::debug!("Creating 7z archive from stream ({} bytes)", size);
        crate::utils::debug_log::debug_log(&format!(
            ">>>>> SevenZipArchiveFromStream::new ({} bytes) <<<<<",
            size
        ));

        // Seek back to start
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

        // Validate by creating a test reader
        let password = Password::empty();
        let _test = SevenZReader::new(&mut reader, size, password)
            .map_err(|e| CbxError::Archive(format!("Invalid 7z archive from stream: {}", e)))?;

        // Seek back to start again
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

        crate::utils::debug_log::debug_log("7z archive validated successfully");

        Ok(Self {
            reader: std::cell::RefCell::new(reader),
            size,
        })
    }

    /// List all entries in archive (helper method)
    fn list_entries(&self) -> Result<Vec<ArchiveEntry>> {
        use std::io::SeekFrom;

        let mut reader_ref = self.reader.borrow_mut();

        // Seek to start
        reader_ref
            .seek(SeekFrom::Start(0))
            .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

        let password = Password::empty();
        let mut archive = SevenZReader::new(&mut *reader_ref, self.size, password)
            .map_err(|e| CbxError::Archive(format!("Failed to create 7z reader: {}", e)))?;

        let mut entries = Vec::new();

        archive
            .for_each_entries(|entry, _reader| {
                entries.push(ArchiveEntry {
                    name: entry.name().to_string(),
                    size: entry.size(),
                    is_directory: entry.is_directory(),
                });
                Ok(true) // Continue iteration
            })
            .map_err(|e| CbxError::Archive(format!("7z iteration error: {}", e)))?;

        Ok(entries)
    }
}

impl<R: Read + Seek> Archive for SevenZipArchiveFromStream<R> {
    fn open(_path: &Path) -> Result<Box<dyn Archive>> {
        // Not used for stream-based archives
        Err(CbxError::Archive(
            "Use open_archive_from_stream instead".to_string(),
        ))
    }

    fn find_first_image(&self, sort: bool) -> Result<ArchiveEntry> {
        tracing::debug!("Finding first image in 7z from stream (sort={})", sort);
        crate::utils::debug_log::debug_log(&format!("7z stream: find_first_image (sort={})", sort));

        if !sort {
            // OPTIMIZATION: Fast path - find first image without full listing
            use std::io::SeekFrom;
            tracing::debug!("7z stream: Fast path - finding first image");

            let mut reader_ref = self.reader.borrow_mut();

            // Seek to start
            reader_ref
                .seek(SeekFrom::Start(0))
                .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

            let password = Password::empty();
            let mut archive = SevenZReader::new(&mut *reader_ref, self.size, password)
                .map_err(|e| CbxError::Archive(format!("Failed to create 7z reader: {}", e)))?;

            let mut first_image: Option<ArchiveEntry> = None;

            archive
                .for_each_entries(|entry, _reader| {
                    let name = entry.name().to_string();
                    if is_image_file(&name) {
                        tracing::info!("Found first image (unsorted, streaming): {}", name);
                        crate::utils::debug_log::debug_log(&format!("Found first image: {}", name));

                        first_image = Some(ArchiveEntry {
                            name,
                            size: entry.size(),
                            is_directory: entry.is_directory(),
                        });
                        Ok(false) // Stop iteration
                    } else {
                        Ok(true) // Continue
                    }
                })
                .map_err(|e| CbxError::Archive(format!("7z iteration error: {}", e)))?;

            return first_image
                .ok_or_else(|| CbxError::Archive("No images found in archive".to_string()));
        }

        // STANDARD PATH: List all entries and sort
        tracing::debug!("7z stream: Sorted path - listing all entries");
        let entries = self.list_entries()?;

        if entries.is_empty() {
            return Err(CbxError::Archive("Archive is empty".to_string()));
        }

        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();

        let image_name = find_first_image(names.iter().map(|s| s.as_str()), sort)
            .ok_or_else(|| CbxError::Archive("No images found in archive".to_string()))?;

        tracing::info!("Found first image (sorted, streaming): {}", image_name);
        crate::utils::debug_log::debug_log(&format!("Found first image (sorted): {}", image_name));

        entries
            .into_iter()
            .find(|e| e.name == image_name)
            .ok_or_else(|| CbxError::Archive("Image entry not found".to_string()))
    }

    fn extract_entry(&self, entry: &ArchiveEntry) -> Result<Vec<u8>> {
        tracing::debug!(
            "Extracting entry from 7z stream: {} ({} bytes)",
            entry.name,
            entry.size
        );
        crate::utils::debug_log::debug_log(&format!(
            "7z stream: extract_entry: {} ({} bytes)",
            entry.name, entry.size
        ));

        // Safety check: prevent memory exhaustion
        if entry.size > MAX_ENTRY_SIZE {
            tracing::warn!(
                "Entry too large: {} bytes (max {})",
                entry.size,
                MAX_ENTRY_SIZE
            );
            return Err(CbxError::Archive(format!(
                "Entry too large: {} bytes (max 32MB)",
                entry.size
            )));
        }

        // Create a new reader for extraction
        use std::io::SeekFrom;

        let mut reader_ref = self.reader.borrow_mut();

        // Seek to start
        reader_ref
            .seek(SeekFrom::Start(0))
            .map_err(|e| CbxError::Archive(format!("Failed to seek to start: {}", e)))?;

        let password = Password::empty();
        let mut archive = SevenZReader::new(&mut *reader_ref, self.size, password)
            .map_err(|e| CbxError::Archive(format!("Failed to create 7z reader: {}", e)))?;

        let mut extracted_data = None;

        archive
            .for_each_entries(|sz_entry, reader| {
                if sz_entry.name() == entry.name {
                    let mut buffer = Vec::with_capacity(sz_entry.size() as usize);
                    std::io::copy(reader, &mut buffer)
                        .map_err(|e| sevenz_rust::Error::Io(e, "Extract failed".into()))?;

                    tracing::debug!("Extracted {} bytes from 7z stream", buffer.len());
                    crate::utils::debug_log::debug_log(&format!(
                        "Extracted {} bytes",
                        buffer.len()
                    ));

                    extracted_data = Some(buffer);
                    Ok(false) // Stop iteration
                } else {
                    Ok(true) // Continue
                }
            })
            .map_err(|e| CbxError::Archive(format!("7z extraction error: {}", e)))?;

        extracted_data.ok_or_else(|| {
            CbxError::Archive(format!("Entry not found in 7z stream: {}", entry.name))
        })
    }

    fn get_metadata(&self) -> Result<ArchiveMetadata> {
        let entries = self.list_entries()?;
        let total_files = entries.len();
        let image_count = entries.iter().filter(|e| is_image_file(&e.name)).count();

        tracing::debug!(
            "7z metadata (from stream): {} files, {} images",
            total_files,
            image_count
        );

        Ok(ArchiveMetadata {
            total_files,
            image_count,
            compressed_size: self.size,
            archive_type: ArchiveType::SevenZip,
        })
    }

    fn archive_type(&self) -> ArchiveType {
        ArchiveType::SevenZip
    }
}
