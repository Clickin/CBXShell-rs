///! RAR/CBR archive implementation
///!
///! Supports RAR and CBR formats using the `unrar` crate
use std::fs::File;
use std::hash::BuildHasher;
use std::io::{Read, Write as IoWrite};
use std::path::{Path, PathBuf};
use unrar::Archive as UnrarArchive;

use super::utils::{find_first_image, is_image_file, MAX_ENTRY_SIZE};
use crate::archive::{Archive, ArchiveEntry, ArchiveMetadata, ArchiveType};
use crate::utils::error::{CbxError, Result};

/// RAR archive handler
pub struct RarArchive {
    path: PathBuf,
}

impl RarArchive {
    /// Open a RAR archive from path
    pub fn open(path: &Path) -> Result<Self> {
        tracing::debug!("Opening RAR archive: {:?}", path);

        // Validate by attempting to list entries
        let archive = UnrarArchive::new(path)
            .open_for_listing()
            .map_err(|e| CbxError::Archive(format!("Failed to open RAR archive: {:?}", e)))?;

        // Check if archive is accessible
        let mut has_entries = false;
        for entry_result in archive {
            match entry_result {
                Ok(_) => {
                    has_entries = true;
                    break;
                }
                Err(e) => {
                    return Err(CbxError::Archive(format!("RAR listing error: {:?}", e)));
                }
            }
        }

        if !has_entries {
            tracing::warn!("RAR archive appears to be empty: {:?}", path);
        }

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// List all entries in archive
    fn list_entries(&self) -> Result<Vec<ArchiveEntry>> {
        let archive = UnrarArchive::new(&self.path)
            .open_for_listing()
            .map_err(|e| CbxError::Archive(format!("Failed to open RAR for listing: {:?}", e)))?;

        let mut entries = Vec::new();

        for entry_result in archive {
            let entry =
                entry_result.map_err(|e| CbxError::Archive(format!("RAR entry error: {:?}", e)))?;

            // Get filename from entry
            let filename = entry.filename.to_string_lossy().to_string();

            entries.push(ArchiveEntry {
                name: filename,
                size: entry.unpacked_size,
                is_directory: entry.is_directory(),
            });
        }

        Ok(entries)
    }
}

impl Archive for RarArchive {
    fn open(path: &Path) -> Result<Box<dyn Archive>> {
        Ok(Box::new(Self::open(path)?))
    }

    fn find_first_image(&self, sort: bool) -> Result<ArchiveEntry> {
        tracing::debug!("Finding first image in RAR (sort={})", sort);

        if !sort {
            // OPTIMIZATION: When not sorting, extract first image immediately
            // without listing all entries (faster for large archives)
            tracing::debug!("Fast path: finding first image without full listing");

            let archive = UnrarArchive::new(&self.path)
                .open_for_listing()
                .map_err(|e| {
                    CbxError::Archive(format!("Failed to open RAR for listing: {:?}", e))
                })?;

            for entry_result in archive {
                let entry = entry_result
                    .map_err(|e| CbxError::Archive(format!("RAR entry error: {:?}", e)))?;

                let filename = entry.filename.to_string_lossy().to_string();

                if is_image_file(&filename) {
                    tracing::info!("Found first image (unsorted): {}", filename);
                    return Ok(ArchiveEntry {
                        name: filename,
                        size: entry.unpacked_size,
                        is_directory: entry.is_directory(),
                    });
                }
            }

            return Err(CbxError::Archive("No images found in archive".to_string()));
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

        let mut archive = UnrarArchive::new(&self.path)
            .open_for_processing()
            .map_err(|e| {
                CbxError::Archive(format!("Failed to open RAR for processing: {:?}", e))
            })?;

        let mut extracted_data = None;

        // Iterate through entries to find and extract the target
        loop {
            match archive.read_header() {
                Ok(Some(header)) => {
                    let current_name = header.entry().filename.to_string_lossy().to_string();

                    if current_name == entry.name {
                        // Extract to memory
                        let (data, _) = header.read().map_err(|e| {
                            CbxError::Archive(format!("Failed to extract RAR entry: {:?}", e))
                        })?;

                        tracing::debug!("Extracted {} bytes from RAR", data.len());
                        extracted_data = Some(data);
                        break;
                    } else {
                        // Skip this entry and continue with next archive state
                        archive = header.skip().map_err(|e| {
                            CbxError::Archive(format!("Failed to skip RAR entry: {:?}", e))
                        })?;
                    }
                }
                Ok(None) => {
                    // No more entries
                    break;
                }
                Err(e) => {
                    return Err(CbxError::Archive(format!(
                        "Failed to read RAR header: {:?}",
                        e
                    )));
                }
            }
        }

        extracted_data
            .ok_or_else(|| CbxError::Archive(format!("Entry not found in RAR: {}", entry.name)))
    }

    fn get_metadata(&self) -> Result<ArchiveMetadata> {
        let entries = self.list_entries()?;
        let total_files = entries.len();
        let image_count = entries.iter().filter(|e| is_image_file(&e.name)).count();

        let compressed_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);

        tracing::debug!(
            "RAR metadata: {} files, {} images, {} bytes",
            total_files,
            image_count,
            compressed_size
        );

        Ok(ArchiveMetadata {
            total_files,
            image_count,
            compressed_size,
            archive_type: ArchiveType::Rar,
        })
    }

    fn archive_type(&self) -> ArchiveType {
        ArchiveType::Rar
    }
}

/// RAR archive handler for in-memory data (IStream support)
pub struct RarArchiveFromMemory {
    temp_path: PathBuf,
}

impl RarArchiveFromMemory {
    /// Create a RAR archive from a streaming reader (OPTIMIZED)
    ///
    /// This version streams data directly from the reader to a temp file
    /// without loading the entire archive into memory first.
    ///
    /// # Performance
    /// - **Old approach**: IStream → Memory (1GB) → Temp File (~5 seconds)
    /// - **New approach**: IStream → Temp File (streaming, ~2 seconds)
    ///
    /// # Arguments
    /// * `reader` - Any Read implementer (IStreamReader, File, etc.)
    ///
    /// # Returns
    /// * `Ok(Self)` - RAR archive ready for processing
    /// * `Err(CbxError)` - If writing or validation fails
    pub fn new_from_stream<R: Read>(mut reader: R) -> Result<Self> {
        tracing::debug!("Creating RAR archive from stream (optimized)");
        crate::utils::debug_log::debug_log(
            ">>>>> RarArchiveFromMemory::new_from_stream STARTING <<<<<",
        );

        // Create temporary file with unique name
        let temp_dir = std::env::temp_dir();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let random: u32 = std::collections::hash_map::RandomState::new().hash_one(timestamp) as u32;
        let thread_id = std::thread::current().id();
        let temp_filename = format!(
            "cbxshell_rar_stream_{}_{:?}_{}_{:08x}.tmp",
            std::process::id(),
            thread_id,
            timestamp,
            random
        );
        let temp_path = temp_dir.join(temp_filename);

        crate::utils::debug_log::debug_log(&format!("Temp file: {:?}", temp_path));

        // Stream data to temp file in chunks (no full memory load!)
        let mut file = File::create(&temp_path)
            .map_err(|e| CbxError::Archive(format!("Failed to create temp RAR file: {}", e)))?;

        let mut total_written = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024]; // 1MB chunks

        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| CbxError::Archive(format!("Failed to read from stream: {}", e)))?;

            if bytes_read == 0 {
                break; // EOF
            }

            file.write_all(&buffer[..bytes_read])
                .map_err(|e| CbxError::Archive(format!("Failed to write to temp file: {}", e)))?;

            total_written += bytes_read as u64;

            if total_written % (10 * 1024 * 1024) == 0 {
                // Log every 10MB
                crate::utils::debug_log::debug_log(&format!(
                    "Streamed {} MB to temp file",
                    total_written / (1024 * 1024)
                ));
            }
        }

        file.sync_all()
            .map_err(|e| CbxError::Archive(format!("Failed to sync temp RAR file: {}", e)))?;

        drop(file);

        crate::utils::debug_log::debug_log(&format!("Total streamed: {} bytes", total_written));

        // Validate the temp file is a valid RAR
        let _test = UnrarArchive::new(&temp_path)
            .open_for_listing()
            .map_err(|e| {
                // Clean up temp file on error
                let _ = std::fs::remove_file(&temp_path);

                // Check if this is a password-protected archive
                let error_msg = format!("{:?}", e);
                if error_msg.contains("password")
                    || error_msg.contains("encrypted")
                    || error_msg.contains("BadPassword")
                {
                    tracing::info!("Skipping password-protected RAR archive");
                    crate::utils::debug_log::debug_log(
                        "RAR archive is password-protected - skipping",
                    );
                    CbxError::Archive("Password-protected RAR archive (not supported)".to_string())
                } else {
                    tracing::warn!("Invalid RAR data: {:?}", e);
                    CbxError::Archive(format!("Invalid RAR data: {:?}", e))
                }
            })?;

        tracing::debug!("Temporary RAR file created from stream: {:?}", temp_path);
        crate::utils::debug_log::debug_log(
            ">>>>> RarArchiveFromMemory::new_from_stream COMPLETED <<<<<",
        );

        Ok(Self { temp_path })
    }

    /// List all entries in archive
    fn list_entries(&self) -> Result<Vec<ArchiveEntry>> {
        let archive = UnrarArchive::new(&self.temp_path)
            .open_for_listing()
            .map_err(|e| CbxError::Archive(format!("Failed to open RAR for listing: {:?}", e)))?;

        let mut entries = Vec::new();

        for entry_result in archive {
            let entry =
                entry_result.map_err(|e| CbxError::Archive(format!("RAR entry error: {:?}", e)))?;

            let filename = entry.filename.to_string_lossy().to_string();

            entries.push(ArchiveEntry {
                name: filename,
                size: entry.unpacked_size,
                is_directory: entry.is_directory(),
            });
        }

        Ok(entries)
    }
}

impl Drop for RarArchiveFromMemory {
    fn drop(&mut self) {
        // Clean up temporary file
        if self.temp_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.temp_path) {
                tracing::warn!("Failed to remove temp RAR file {:?}: {}", self.temp_path, e);
            } else {
                tracing::debug!("Cleaned up temp RAR file: {:?}", self.temp_path);
            }
        }
    }
}

impl Archive for RarArchiveFromMemory {
    fn open(_path: &Path) -> Result<Box<dyn Archive>> {
        // Not used for in-memory archives
        Err(CbxError::Archive(
            "Use open_archive_from_stream instead".to_string(),
        ))
    }

    fn find_first_image(&self, sort: bool) -> Result<ArchiveEntry> {
        tracing::debug!("Finding first image in RAR from memory (sort={})", sort);

        if !sort {
            // OPTIMIZATION: When not sorting, find first image immediately
            tracing::debug!("Fast path: finding first image without full listing");

            let archive = UnrarArchive::new(&self.temp_path)
                .open_for_listing()
                .map_err(|e| {
                    CbxError::Archive(format!("Failed to open RAR for listing: {:?}", e))
                })?;

            for entry_result in archive {
                let entry = entry_result
                    .map_err(|e| CbxError::Archive(format!("RAR entry error: {:?}", e)))?;

                let filename = entry.filename.to_string_lossy().to_string();

                if is_image_file(&filename) {
                    tracing::info!("Found first image (unsorted): {}", filename);
                    return Ok(ArchiveEntry {
                        name: filename,
                        size: entry.unpacked_size,
                        is_directory: entry.is_directory(),
                    });
                }
            }

            return Err(CbxError::Archive("No images found in archive".to_string()));
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
        tracing::debug!(
            "Extracting entry from memory: {} ({} bytes)",
            entry.name,
            entry.size
        );

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

        let mut archive = UnrarArchive::new(&self.temp_path)
            .open_for_processing()
            .map_err(|e| {
                CbxError::Archive(format!("Failed to open RAR for processing: {:?}", e))
            })?;

        let mut extracted_data = None;

        // Iterate through entries to find and extract the target
        loop {
            match archive.read_header() {
                Ok(Some(header)) => {
                    let current_name = header.entry().filename.to_string_lossy().to_string();

                    if current_name == entry.name {
                        // Extract to memory
                        let (data, _) = header.read().map_err(|e| {
                            CbxError::Archive(format!("Failed to extract RAR entry: {:?}", e))
                        })?;

                        tracing::debug!("Extracted {} bytes from RAR", data.len());
                        extracted_data = Some(data);
                        break;
                    } else {
                        // Skip this entry and continue with next archive state
                        archive = header.skip().map_err(|e| {
                            CbxError::Archive(format!("Failed to skip RAR entry: {:?}", e))
                        })?;
                    }
                }
                Ok(None) => {
                    // No more entries
                    break;
                }
                Err(e) => {
                    return Err(CbxError::Archive(format!(
                        "Failed to read RAR header: {:?}",
                        e
                    )));
                }
            }
        }

        extracted_data
            .ok_or_else(|| CbxError::Archive(format!("Entry not found in RAR: {}", entry.name)))
    }

    fn get_metadata(&self) -> Result<ArchiveMetadata> {
        let entries = self.list_entries()?;
        let total_files = entries.len();
        let image_count = entries.iter().filter(|e| is_image_file(&e.name)).count();

        let compressed_size = std::fs::metadata(&self.temp_path)
            .map(|m| m.len())
            .unwrap_or(0);

        tracing::debug!(
            "RAR metadata (from memory): {} files, {} images, {} bytes",
            total_files,
            image_count,
            compressed_size
        );

        Ok(ArchiveMetadata {
            total_files,
            image_count,
            compressed_size: compressed_size,
            archive_type: ArchiveType::Rar,
        })
    }

    fn archive_type(&self) -> ArchiveType {
        ArchiveType::Rar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Creating valid RAR archives programmatically is not possible
    // with the unrar crate (it's extraction-only). Tests will need to use
    // pre-existing RAR files or be integration tests.

    #[test]
    fn test_open_nonexistent_rar() {
        let path = Path::new("nonexistent.rar");
        let result = RarArchive::open(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_open_invalid_rar() {
        let temp_path = std::env::temp_dir().join("test_invalid.rar");
        std::fs::write(&temp_path, b"not a rar file").unwrap();

        let result = RarArchive::open(&temp_path);
        assert!(result.is_err());

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_rar_archive_type() {
        // This test doesn't need a real RAR file
        let rar = RarArchive {
            path: PathBuf::from("test.rar"),
        };
        assert_eq!(rar.archive_type(), ArchiveType::Rar);
    }

    // Note: More comprehensive tests require actual RAR files
    // These should be added as integration tests with test fixtures
}
