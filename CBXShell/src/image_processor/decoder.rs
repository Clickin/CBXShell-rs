//! Image decoding from raw bytes
//!
//! Uses a hybrid decoding strategy:
//! 1. Windows WIC decoder first (uses OS codecs when available)
//! 2. Fallback to Rust `image` crate decoder
//!
//! This keeps compatibility while enabling newer Windows codec capabilities
//! (e.g., AVIF via installed system codec) without bundling large codec libraries.

use crate::utils::debug_log::debug_log;
use crate::utils::error::CbxError;
use image::{DynamicImage, ImageBuffer, ImageReader, RgbaImage};
use std::io::Cursor;

type Result<T> = std::result::Result<T, CbxError>;

/// Decode image from raw bytes
///
/// This function attempts to automatically detect the image format and decode it.
/// It supports all formats enabled in the `image` crate dependency.
///
/// # Arguments
/// * `data` - Raw image file bytes
///
/// # Returns
/// * `Ok(DynamicImage)` - Successfully decoded image
/// * `Err(CbxError::Image)` - Failed to decode (invalid format or corrupt data)
///
/// # Examples
/// ```no_run
/// let jpeg_data = std::fs::read("image.jpg")?;
/// let img = decode_image(&jpeg_data)?;
/// println!("Image dimensions: {}x{}", img.width(), img.height());
/// ```
pub fn decode_image(data: &[u8]) -> Result<DynamicImage> {
    if data.is_empty() {
        return Err(CbxError::Image("Empty image data".to_string()));
    }

    debug_log(&format!(
        "WIC decode attempt started for {} bytes",
        data.len()
    ));

    // Fast path: try Windows WIC decoder first.
    // WIC can use OS-installed codecs and may leverage platform-specific optimizations.
    if let Some(img) = try_decode_with_wic(data)? {
        debug_log(&format!(
            "WIC decode path used successfully: {}x{}",
            img.width(),
            img.height()
        ));
        return Ok(img);
    }

    debug_log("WIC decode path unavailable, falling back to image crate");

    // Fallback path: decode via Rust image crate for broad compatibility.
    decode_with_image_crate(data)
}

fn decode_with_image_crate(data: &[u8]) -> Result<DynamicImage> {
    // Create a reader from the byte slice
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| CbxError::Image(format!("Format detection failed: {}", e)))?;

    // Decode the image
    reader
        .decode()
        .map_err(|e| CbxError::Image(format!("Failed to decode image: {}", e)))
}

#[cfg(target_os = "windows")]
fn try_decode_with_wic(data: &[u8]) -> Result<Option<DynamicImage>> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapDecoder,
        IWICFormatConverter, IWICImagingFactory, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    let factory: IWICImagingFactory =
        match unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) } {
            Ok(factory) => factory,
            Err(e) => {
                // Some callers (including tests) may run on threads without COM initialization.
                // Treat WIC setup failures as non-fatal so decode_image can still use image-crate fallback.
                tracing::debug!("WIC factory creation failed, fallback to image crate: {e}");
                debug_log(&format!(
                    "WIC factory creation failed, fallback to image crate: {}",
                    e
                ));
                return Ok(None);
            }
        };

    let stream = unsafe {
        factory
            .CreateStream()
            .map_err(|e| CbxError::Image(format!("WIC stream creation failed: {}", e)))?
    };

    // WIC API expects mutable memory; WIC won't modify data when decoding from memory.
    // Data lifetime is guaranteed for the duration of this function.
    unsafe {
        stream
            .InitializeFromMemory(data)
            .map_err(|e| CbxError::Image(format!("WIC stream initialization failed: {}", e)))?;
    }

    let decoder: IWICBitmapDecoder = match unsafe {
        factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
    } {
        Ok(decoder) => decoder,
        Err(e) => {
            tracing::debug!("WIC decoder unavailable for image, fallback to image crate: {e}");
            debug_log(&format!(
                "WIC decoder unavailable for image, fallback to image crate: {}",
                e
            ));
            return Ok(None);
        }
    };

    let frame = match unsafe { decoder.GetFrame(0) } {
        Ok(frame) => frame,
        Err(e) => {
            tracing::debug!("WIC frame decode failed, fallback to image crate: {e}");
            debug_log(&format!(
                "WIC frame decode failed, fallback to image crate: {}",
                e
            ));
            return Ok(None);
        }
    };

    let mut width = 0u32;
    let mut height = 0u32;
    unsafe {
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|e| CbxError::Image(format!("WIC failed to read image size: {}", e)))?;
    }

    if width == 0 || height == 0 {
        return Err(CbxError::Image(
            "WIC decoded invalid image size".to_string(),
        ));
    }

    let converter: IWICFormatConverter = unsafe {
        factory
            .CreateFormatConverter()
            .map_err(|e| CbxError::Image(format!("WIC format converter creation failed: {}", e)))?
    };

    unsafe {
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| CbxError::Image(format!("WIC format conversion setup failed: {}", e)))?;
    }

    let stride = width.checked_mul(4).ok_or_else(|| {
        CbxError::Image("Image stride overflow while decoding with WIC".to_string())
    })?;
    let buffer_size = stride.checked_mul(height).ok_or_else(|| {
        CbxError::Image("Image buffer overflow while decoding with WIC".to_string())
    })?;

    let mut pixels = vec![0u8; buffer_size as usize];
    unsafe {
        converter
            .CopyPixels(std::ptr::null(), stride, pixels.as_mut_slice())
            .map_err(|e| CbxError::Image(format!("WIC pixel copy failed: {}", e)))?;
    }

    let rgba: RgbaImage = ImageBuffer::from_raw(width, height, pixels).ok_or_else(|| {
        CbxError::Image("WIC decoded data had unexpected pixel buffer size".to_string())
    })?;

    debug_log(&format!("WIC decode succeeded: {}x{}", width, height));
    tracing::debug!("Decoded image with WIC: {}x{}", width, height);
    Ok(Some(DynamicImage::ImageRgba8(rgba)))
}


#[cfg(not(target_os = "windows"))]
fn try_decode_with_wic(_data: &[u8]) -> Result<Option<DynamicImage>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid JPEG file (1x1 red pixel)
    /// This is a base64 decoded JPEG file that represents a 1x1 red pixel
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

    /// Minimal valid PNG file (1x1 red pixel)
    /// This is a valid 1x1 PNG generated and verified
    const MINIMAL_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 dimensions
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44,
        0x41, 0x54, // IDAT chunk (12 bytes)
        0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, // Compressed data
        0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, // CRC corrected
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
        0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn test_decode_jpeg() {
        let result = decode_image(MINIMAL_JPEG);
        assert!(result.is_ok(), "Failed to decode JPEG: {:?}", result.err());

        let img = result.unwrap();
        assert_eq!(img.width(), 1);
        assert_eq!(img.height(), 1);
    }

    #[test]
    fn test_decode_png() {
        let result = decode_image(MINIMAL_PNG);
        assert!(result.is_ok(), "Failed to decode PNG: {:?}", result.err());

        let img = result.unwrap();
        assert_eq!(img.width(), 1);
        assert_eq!(img.height(), 1);
    }

    #[test]
    fn test_decode_empty_data() {
        let result = decode_image(&[]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CbxError::Image(_)));
    }

    #[test]
    fn test_decode_corrupt_data() {
        let corrupt = vec![0xFF, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let result = decode_image(&corrupt);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CbxError::Image(_)));
    }

    #[test]
    fn test_decode_partial_data() {
        // Only JPEG signature, no actual image data
        let partial = vec![0xFF, 0xD8, 0xFF, 0xE0];
        let result = decode_image(&partial);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_wrong_format() {
        // This is not an image file, just random bytes
        let not_image = b"This is not an image file content";
        let result = decode_image(not_image);
        assert!(result.is_err());
    }
}
