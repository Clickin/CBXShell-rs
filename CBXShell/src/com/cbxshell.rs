use std::sync::atomic::AtomicU32;
use std::sync::Mutex;
///! CBXShell main COM object implementation
///!
///! Migrated to IThumbnailProvider + IInitializeWithStream (modern Windows API)
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::HBITMAP, Win32::System::Com::*,
    Win32::UI::Shell::PropertiesSystem::*, Win32::UI::Shell::*,
};

/// CBXShell COM object
/// Implements: IThumbnailProvider, IInitializeWithStream, IQueryInfo
///
/// CRITICAL: Modern thumbnail API (IThumbnailProvider) replaces legacy IExtractImage
/// - IThumbnailProvider: Modern thumbnail extraction (Vista+)
/// - IInitializeWithStream: Stream-based initialization (replaces IPersistFile)
/// - IQueryInfo: Tooltips (unchanged)
#[implement(IThumbnailProvider, IInitializeWithStream, IQueryInfo)]
pub struct CBXShell {
    #[allow(dead_code)] // Used by COM infrastructure through #[implement] macro
    ref_count: AtomicU32,
    stream: Mutex<Option<IStream>>,
}

impl CBXShell {
    /// Create a new CBXShell instance
    pub fn new() -> Result<IThumbnailProvider> {
        tracing::debug!("Creating CBXShell instance (IThumbnailProvider)");
        crate::utils::debug_log::debug_log("===== CBXShell::new() CALLED =====");

        let cbxshell = CBXShell {
            ref_count: AtomicU32::new(1),
            stream: Mutex::new(None),
        };

        crate::add_dll_ref();
        crate::utils::debug_log::debug_log("CBXShell instance created successfully");
        Ok(cbxshell.into())
    }

    /// Get the stored IStream
    fn get_stream(&self) -> Option<IStream> {
        self.stream.lock().unwrap().clone()
    }

    /// Extract thumbnail from archive (internal implementation)
    ///
    /// This is the core thumbnail extraction logic for IThumbnailProvider that:
    /// 1. Gets the IStream from IInitializeWithStream
    /// 2. Reads archive data from stream into memory
    /// 3. Detects archive type from magic bytes
    /// 4. Opens the archive from memory
    /// 5. Reads sort preference from registry
    /// 6. Finds the first image (alphabetically if sorted)
    /// 7. Extracts the image data
    /// 8. Creates thumbnail HBITMAP with requested size
    ///
    /// # Arguments
    /// * `cx` - Maximum thumbnail width/height in pixels
    ///
    /// # Returns
    /// * `Ok(HBITMAP)` - Successfully created thumbnail
    /// * `Err(CbxError)` - Failed to extract or create thumbnail
    fn extract_thumbnail_internal(&self, cx: u32) -> crate::utils::error::Result<HBITMAP> {
        use crate::archive::{open_archive_from_stream, should_sort_images, IStreamReader};
        use crate::image_processor::thumbnail::create_thumbnail_with_size;
        use crate::utils::error::CbxError;

        crate::utils::debug_log::debug_log(
            ">>>>> extract_thumbnail_internal STARTING (OPTIMIZED STREAMING) <<<<<",
        );
        crate::utils::debug_log::debug_log(&format!("Requested thumbnail size: {}x{}", cx, cx));

        // Step 1: Get IStream from IInitializeWithStream
        let stream = self.get_stream().ok_or_else(|| {
            crate::utils::debug_log::debug_log(
                "ERROR: No IStream set in extract_thumbnail_internal",
            );
            CbxError::Archive("No stream initialized".to_string())
        })?;

        tracing::info!("Extracting thumbnail from IStream (streaming mode)");
        crate::utils::debug_log::debug_log("Step 1: IStream retrieved successfully");

        // Step 2: Create streaming reader (NO MEMORY COPY!)
        crate::utils::debug_log::debug_log("Step 2: Creating streaming reader (OPTIMIZED)...");
        let reader = IStreamReader::new(stream);
        tracing::debug!("IStreamReader created for direct streaming");
        crate::utils::debug_log::debug_log("Step 2: IStreamReader created - ready for streaming");

        // Step 3: Open archive from stream (OPTIMIZED!)
        crate::utils::debug_log::debug_log("Step 3: Opening archive from stream (NO FULL LOAD)...");
        let archive = open_archive_from_stream(reader)?;
        tracing::debug!("Archive opened successfully from stream");
        crate::utils::debug_log::debug_log("Step 3: Archive opened successfully in streaming mode");

        // Step 4: Read sort preference from registry
        let sort = should_sort_images();
        tracing::debug!("Sort preference: {}", sort);
        crate::utils::debug_log::debug_log(&format!("Step 4: Sort preference: {}", sort));

        // Step 5: Find first image in archive
        crate::utils::debug_log::debug_log("Step 5: Finding first image...");
        let entry = archive.find_first_image(sort)?;
        tracing::info!("Found image: {} ({} bytes)", entry.name, entry.size);
        crate::utils::debug_log::debug_log(&format!(
            "Step 5: Found image: {} ({} bytes)",
            entry.name, entry.size
        ));

        // Step 6: Extract image data
        crate::utils::debug_log::debug_log("Step 6: Extracting image data...");
        let image_data = archive.extract_entry(&entry)?;
        tracing::debug!("Extracted {} bytes of image data", image_data.len());
        crate::utils::debug_log::debug_log(&format!(
            "Step 6: Extracted {} bytes of image data",
            image_data.len()
        ));

        // Step 6b: Verify image format using magic headers
        crate::utils::debug_log::debug_log("Step 6b: Verifying image format with magic headers...");
        crate::archive::verify_image_data(&image_data, &entry.name)?;
        crate::utils::debug_log::debug_log("Step 6b: Image format verification passed");

        // Step 7: Use requested size from IThumbnailProvider::GetThumbnail
        // IThumbnailProvider provides cx (max dimension), we create square thumbnails
        let thumbnail_size = if cx == 0 { 256 } else { cx };
        tracing::debug!(
            "Creating thumbnail with size: {}x{}",
            thumbnail_size,
            thumbnail_size
        );
        crate::utils::debug_log::debug_log(&format!(
            "Step 7: Creating thumbnail with size: {}x{}",
            thumbnail_size, thumbnail_size
        ));

        // Step 8: Create thumbnail HBITMAP
        crate::utils::debug_log::debug_log("Step 8: Creating thumbnail HBITMAP...");
        let hbitmap = match create_thumbnail_with_size(&image_data, thumbnail_size, thumbnail_size)
        {
            Ok(bmp) => {
                tracing::info!("Thumbnail created successfully: {:?}", bmp);
                crate::utils::debug_log::debug_log(&format!(
                    "Step 8: Thumbnail created successfully - HBITMAP: {:?} (handle: 0x{:x})",
                    bmp, bmp.0 as usize
                ));
                bmp
            }
            Err(e) => {
                tracing::error!("Failed to create thumbnail: {}", e);
                crate::utils::debug_log::debug_log(&format!(
                    "ERROR Step 8: Thumbnail creation failed: {}",
                    e
                ));
                crate::utils::debug_log::debug_log(&format!(
                    "ERROR: Image data size: {} bytes, requested size: {}x{}",
                    image_data.len(),
                    thumbnail_size,
                    thumbnail_size
                ));
                return Err(e);
            }
        };

        crate::utils::debug_log::debug_log(
            ">>>>> extract_thumbnail_internal COMPLETED SUCCESSFULLY <<<<<",
        );
        Ok(hbitmap)
    }
}

impl Drop for CBXShell {
    fn drop(&mut self) {
        crate::release_dll_ref();
        tracing::debug!("CBXShell dropped");
    }
}

// IInitializeWithStream implementation (replaces IPersistFile)
impl IInitializeWithStream_Impl for CBXShell {
    fn Initialize(&self, pstream: Option<&IStream>, _grfmode: u32) -> Result<()> {
        crate::utils::debug_log::debug_log("===== IInitializeWithStream::Initialize CALLED =====");
        tracing::info!("IInitializeWithStream::Initialize called");

        // Get the IStream and clone it (this calls AddRef)
        let stream = pstream
            .ok_or_else(|| {
                crate::utils::debug_log::debug_log("ERROR: IStream pointer is null");
                Error::from(E_POINTER)
            })?
            .clone();

        crate::utils::debug_log::debug_log("IStream received and cloned successfully");

        // Store the cloned stream (properly ref-counted)
        *self.stream.lock().unwrap() = Some(stream);

        crate::utils::debug_log::debug_log("SUCCESS: IInitializeWithStream::Initialize completed");
        Ok(())
    }
}

// IThumbnailProvider implementation (replaces IExtractImage/IExtractImage2)
impl IThumbnailProvider_Impl for CBXShell {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> Result<()> {
        tracing::info!("IThumbnailProvider::GetThumbnail called (cx={})", cx);
        crate::utils::debug_log::debug_log(&format!(
            "===== IThumbnailProvider::GetThumbnail CALLED (cx={}) =====",
            cx
        ));

        // Validate output pointers
        if phbmp.is_null() {
            crate::utils::debug_log::debug_log("ERROR: phbmp is null");
            return Err(Error::from(E_POINTER));
        }

        // Call internal extraction method
        match self.extract_thumbnail_internal(cx) {
            Ok(hbitmap) => {
                tracing::info!("GetThumbnail succeeded, returning HBITMAP: {:?}", hbitmap);
                crate::utils::debug_log::debug_log(&format!(
                    "SUCCESS: GetThumbnail completed - HBITMAP: {:?} (handle: 0x{:x})",
                    hbitmap, hbitmap.0 as usize
                ));

                // UNAVOIDABLE UNSAFE: Writing to COM output parameters
                // Why unsafe is required:
                // 1. COM out parameters: phbmp and pdwalpha are raw pointers from COM caller
                // 2. Standard COM pattern: IThumbnailProvider interface specification
                // 3. No safe alternative: COM ABI requires raw pointer mutation
                //
                // Safety guarantees:
                // - phbmp validated as non-null above (line 189)
                // - pdwalpha null-checked before dereferencing
                // - hbitmap is valid (created by our code)
                // - COM caller is responsible for pointer validity (COM contract)
                unsafe {
                    *phbmp = hbitmap;

                    // Set alpha type if requested
                    // We composite images with white background, removing transparency
                    // WTS_ALPHATYPE: WTSAT_UNKNOWN=0, WTSAT_RGB=1 (no alpha), WTSAT_ARGB=2 (has alpha)
                    // Use WTSAT_RGB since we've removed all transparency
                    if !pdwalpha.is_null() {
                        *pdwalpha = WTSAT_RGB; // Value should be 1
                        crate::utils::debug_log::debug_log(
                            "Alpha type set to WTSAT_RGB (no alpha channel)",
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                tracing::error!("GetThumbnail failed: {}", e);
                crate::utils::debug_log::debug_log(&format!("ERROR: GetThumbnail failed - {}", e));
                // Convert CbxError to HRESULT
                let hresult: HRESULT = e.into();
                crate::utils::debug_log::debug_log(&format!("Returning HRESULT: {:?}", hresult));
                Err(Error::from(hresult))
            }
        }
    }
}

// IQueryInfo implementation
impl IQueryInfo_Impl for CBXShell {
    fn GetInfoTip(&self, _dwflags: &QITIPF_FLAGS) -> Result<PWSTR> {
        tracing::info!("IQueryInfo::GetInfoTip called");

        // TODO: Implement info tip generation
        // For now, return E_FAIL as stub
        tracing::warn!("GetInfoTip not yet implemented - returning E_FAIL");
        Err(Error::from(E_FAIL))
    }

    fn GetInfoFlags(&self) -> Result<u32> {
        // Return 0 flags as not implemented
        Err(Error::from(E_NOTIMPL))
    }
}

#[cfg(all(test, windows, feature = "e2e-windows"))]
mod tests {
    use super::*;
    use std::io::Write as _;
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, IStream, COINIT_APARTMENTTHREADED, STREAM_SEEK_SET,
    };
    use zip::write::{FileOptions, ZipWriter};

    /// Minimal valid JPEG (1x1 red pixel)
    const MINIMAL_JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x03, 0x02, 0x02, 0x02, 0x02,
        0x02, 0x03, 0x02, 0x02, 0x02, 0x03, 0x03, 0x03, 0x03, 0x04, 0x06, 0x04, 0x04, 0x04, 0x04,
        0x04, 0x08, 0x06, 0x06, 0x05, 0x06, 0x09, 0x08, 0x0A, 0x0A, 0x09, 0x08, 0x09, 0x09, 0x0A,
        0x0C, 0x0F, 0x0C, 0x0A, 0x0B, 0x0E, 0x0B, 0x09, 0x09, 0x0D, 0x11, 0x0D, 0x0E, 0x0F, 0x10,
        0x10, 0x11, 0x10, 0x0A, 0x0C, 0x12, 0x13, 0x12, 0x10, 0x13, 0x0F, 0x10, 0x10, 0x10, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x09, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08,
        0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x54, 0xDF, 0xFF, 0xD9,
    ];

    /// Create a test CBZ archive in memory and return as IStream
    fn create_test_cbz_stream() -> Result<IStream> {
        // Create ZIP in memory
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));
            zip.start_file("page001.jpg", FileOptions::default())
                .unwrap();
            zip.write_all(MINIMAL_JPEG).unwrap();
            zip.finish().unwrap();
        }

        // Create IStream from HGLOBAL
        unsafe {
            let stream = CreateStreamOnHGlobal(None, BOOL(1))?;

            // Write ZIP data to stream
            let mut bytes_written = 0u32;
            if stream
                .Write(
                    buffer.as_ptr() as *const _,
                    buffer.len() as u32,
                    Some(&mut bytes_written),
                )
                .is_err()
            {
                return Err(Error::from(E_FAIL));
            }

            // Seek back to beginning
            if stream.Seek(0, STREAM_SEEK_SET, None).is_err() {
                return Err(Error::from(E_FAIL));
            }

            Ok(stream)
        }
    }

    #[test]
    #[ignore = "requires Windows COM/GDI runtime"]
    fn test_extract_thumbnail_pipeline() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            // Create test CBZ stream
            let stream = create_test_cbz_stream().expect("Failed to create test stream");

            // Create CBXShell instance
            let thumbnail_provider = CBXShell::new().expect("Failed to create CBXShell");

            // Cast to IInitializeWithStream and initialize
            let init_stream: IInitializeWithStream = thumbnail_provider
                .cast()
                .expect("Failed to cast to IInitializeWithStream");

            init_stream
                .Initialize(Some(&stream), STGM_READ.0)
                .expect("IInitializeWithStream::Initialize failed");

            // Cast to IThumbnailProvider
            let thumb_provider: IThumbnailProvider = init_stream
                .cast()
                .expect("Failed to cast to IThumbnailProvider");

            // Get thumbnail
            let mut hbitmap = HBITMAP::default();
            let mut alpha_type = WTS_ALPHATYPE::default();

            thumb_provider
                .GetThumbnail(256, &mut hbitmap, &mut alpha_type)
                .expect("IThumbnailProvider::GetThumbnail failed");

            assert_ne!(hbitmap.0, 0, "HBITMAP should not be null");

            DeleteObject(hbitmap).expect("Failed to delete HBITMAP");
            CoUninitialize();
        }
    }

    #[test]
    #[ignore = "requires Windows COM runtime"]
    fn test_extract_without_initialize_fails() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            // Create CBXShell without initializing stream
            let thumbnail_provider = CBXShell::new().expect("Failed to create CBXShell");

            let thumb_provider: IThumbnailProvider = thumbnail_provider
                .cast()
                .expect("Failed to cast to IThumbnailProvider");

            let mut hbitmap = HBITMAP::default();
            let mut alpha_type = WTS_ALPHATYPE::default();

            let result = thumb_provider.GetThumbnail(256, &mut hbitmap, &mut alpha_type);
            assert!(
                result.is_err(),
                "GetThumbnail should fail without Initialize"
            );

            CoUninitialize();
        }
    }

    #[test]
    #[ignore = "requires Windows COM/GDI runtime"]
    fn test_thumbnail_size_parameter() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let stream = create_test_cbz_stream().expect("Failed to create test stream");
            let thumbnail_provider = CBXShell::new().expect("Failed to create CBXShell");

            let init_stream: IInitializeWithStream = thumbnail_provider.cast().unwrap();
            init_stream.Initialize(Some(&stream), STGM_READ.0).unwrap();

            let thumb_provider: IThumbnailProvider = init_stream.cast().unwrap();

            // Test with specific size
            let mut hbitmap = HBITMAP::default();
            let mut alpha_type = WTS_ALPHATYPE::default();

            thumb_provider
                .GetThumbnail(128, &mut hbitmap, &mut alpha_type)
                .expect("GetThumbnail failed with cx=128");

            assert_ne!(hbitmap.0, 0, "HBITMAP should not be null");

            DeleteObject(hbitmap).ok();
            CoUninitialize();
        }
    }
}
