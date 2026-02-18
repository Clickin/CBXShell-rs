//! Image format detection using magic headers (file signatures)
//!
//! This module provides fast, reliable image format detection by examining
//! the first few bytes (magic bytes/file signature) of image data.
//!
//! ## Supported Formats
//!
//! - **JPEG**: `FF D8 FF` (all JPEG variants including JFIF, EXIF)
//! - **PNG**: `89 50 4E 47 0D 0A 1A 0A` (PNG signature)
//! - **GIF**: `47 49 46 38` (GIF87a/GIF89a)
//! - **BMP**: `42 4D` (BM header)
//! - **TIFF**: `49 49 2A 00` (little-endian) or `4D 4D 00 2A` (big-endian)
//! - **ICO**: `00 00 01 00` (icon format)
//! - **WebP**: `52 49 46 46 ... 57 45 42 50` (RIFF...WEBP)
//! - **AVIF**: `... 66 74 79 70 61 76 69 66` (...ftypavif in ftyp box)
//!
//! ## Why Magic Headers?
//!
//! Magic header verification provides several benefits over extension-based detection:
//!
//! 1. **Security**: Prevents misidentified files from being processed
//! 2. **Accuracy**: Works even when files have wrong extensions
//! 3. **Performance**: Very fast (only reads first ~32 bytes)
//! 4. **Reliability**: Industry-standard file identification method

use crate::utils::error::{CbxError, Result};

/// Represents a detected image format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// JPEG image (FF D8 FF)
    Jpeg,
    /// PNG image (89 50 4E 47 0D 0A 1A 0A)
    Png,
    /// GIF image (47 49 46 38)
    Gif,
    /// BMP image (42 4D)
    Bmp,
    /// TIFF image (49 49 2A 00 or 4D 4D 00 2A)
    Tiff,
    /// ICO icon (00 00 01 00)
    Ico,
    /// WebP image (52 49 46 46 ... 57 45 42 50)
    WebP,
    /// AVIF image (ftyp box with 'avif' brand)
    Avif,
}

impl ImageFormat {
    /// Get format name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Ico => "ICO",
            Self::WebP => "WebP",
            Self::Avif => "AVIF",
        }
    }
}

/// Detect image format from magic bytes
///
/// This function examines the first few bytes of the data to determine
/// the image format. It's much faster than trying to decode the entire image.
///
/// # Arguments
/// * `data` - Raw image data (needs at least 32 bytes for reliable detection)
///
/// # Returns
/// * `Ok(ImageFormat)` - Successfully detected format
/// * `Err(CbxError)` - Not an image or unrecognized format
///
/// # Examples
/// ```ignore
/// let jpeg_data = std::fs::read("photo.jpg")?;
/// let format = detect_image_format(&jpeg_data)?;
/// assert_eq!(format, ImageFormat::Jpeg);
/// ```
pub fn detect_image_format(data: &[u8]) -> Result<ImageFormat> {
    if data.is_empty() {
        return Err(CbxError::Image("Empty data".to_string()));
    }

    // Minimum bytes needed for detection
    const MIN_BYTES: usize = 4;
    if data.len() < MIN_BYTES {
        return Err(CbxError::Image(format!(
            "Insufficient data for format detection (need {} bytes, got {})",
            MIN_BYTES,
            data.len()
        )));
    }

    // JPEG: FF D8 FF
    // Most common format in comic archives, check first
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Ok(ImageFormat::Jpeg);
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A (‰PNG\r\n\x1A\n)
    // Second most common format
    if data.len() >= 8
        && data[0] == 0x89
        && data[1] == 0x50
        && data[2] == 0x4E
        && data[3] == 0x47
        && data[4] == 0x0D
        && data[5] == 0x0A
        && data[6] == 0x1A
        && data[7] == 0x0A
    {
        return Ok(ImageFormat::Png);
    }

    // GIF: 47 49 46 38 (GIF8)
    if data.len() >= 4 && data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 && data[3] == 0x38 {
        return Ok(ImageFormat::Gif);
    }

    // BMP: 42 4D (BM)
    if data.len() >= 2 && data[0] == 0x42 && data[1] == 0x4D {
        return Ok(ImageFormat::Bmp);
    }

    // TIFF: 49 49 2A 00 (little-endian) or 4D 4D 00 2A (big-endian)
    if data.len() >= 4 {
        if (data[0] == 0x49 && data[1] == 0x49 && data[2] == 0x2A && data[3] == 0x00)
            || (data[0] == 0x4D && data[1] == 0x4D && data[2] == 0x00 && data[3] == 0x2A)
        {
            return Ok(ImageFormat::Tiff);
        }
    }

    // ICO: 00 00 01 00
    if data.len() >= 4 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 && data[3] == 0x00 {
        return Ok(ImageFormat::Ico);
    }

    // WebP: 52 49 46 46 ... 57 45 42 50 (RIFF....WEBP)
    // Need at least 12 bytes: RIFF (4) + size (4) + WEBP (4)
    if data.len() >= 12
        && data[0] == 0x52
        && data[1] == 0x49
        && data[2] == 0x46
        && data[3] == 0x46 // RIFF
        && data[8] == 0x57
        && data[9] == 0x45
        && data[10] == 0x42
        && data[11] == 0x50
    // WEBP
    {
        return Ok(ImageFormat::WebP);
    }

    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let major_brand = &data[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Ok(ImageFormat::Avif);
        }
    }

    if data.len() >= 16 {
        let mut cursor = 0usize;
        let parse_limit = data.len().min(4096);

        while cursor + 8 <= parse_limit {
            let size = u32::from_be_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;

            let box_type = &data[cursor + 4..cursor + 8];
            let (header_size, box_end) = if size == 1 {
                if cursor + 16 > parse_limit {
                    break;
                }

                let large_size = u64::from_be_bytes([
                    data[cursor + 8],
                    data[cursor + 9],
                    data[cursor + 10],
                    data[cursor + 11],
                    data[cursor + 12],
                    data[cursor + 13],
                    data[cursor + 14],
                    data[cursor + 15],
                ]) as usize;
                if large_size < 16 {
                    break;
                }
                let end = cursor.saturating_add(large_size);
                if end > parse_limit {
                    break;
                }
                (16usize, end)
            } else {
                if size < 8 {
                    break;
                }
                let end = cursor.saturating_add(size);
                if end > parse_limit {
                    break;
                }
                (8usize, end)
            };

            if box_type == b"ftyp" {
                if cursor + header_size + 4 > box_end {
                    break;
                }

                let major_brand = &data[cursor + header_size..cursor + header_size + 4];
                if major_brand == b"avif" || major_brand == b"avis" {
                    return Ok(ImageFormat::Avif);
                }

                if cursor + header_size + 8 <= box_end {
                    let mut brand_offset = cursor + header_size + 8;
                    while brand_offset + 4 <= box_end {
                        let brand = &data[brand_offset..brand_offset + 4];
                        if brand == b"avif" || brand == b"avis" {
                            return Ok(ImageFormat::Avif);
                        }
                        brand_offset += 4;
                    }
                }
                break;
            }

            cursor = box_end;
        }

        let scan_limit = parse_limit.min(64);
        for i in 0..=scan_limit.saturating_sub(8) {
            if &data[i..i + 8] == b"ftypavif" || &data[i..i + 8] == b"ftypavis" {
                return Ok(ImageFormat::Avif);
            }
        }
    }

    // No recognized format
    Err(CbxError::Image(format!(
        "Unrecognized image format (first 16 bytes: {:02X?})",
        &data[..data.len().min(16)]
    )))
}

/// Verify that data is a valid image and return its format
///
/// This is a convenience wrapper around `detect_image_format` that
/// provides a more semantic API for verification use cases.
///
/// # Arguments
/// * `data` - Raw image data to verify
///
/// # Returns
/// * `Ok(ImageFormat)` - Valid image of detected format
/// * `Err(CbxError)` - Not a valid image
pub fn verify_image_format(data: &[u8]) -> Result<ImageFormat> {
    detect_image_format(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid JPEG (1x1 red pixel)
    const MINIMAL_JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x03, 0x02, 0x02,
    ];

    /// Minimal valid PNG (1x1 red pixel)
    const MINIMAL_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
    ];

    /// GIF header
    const GIF_HEADER: &[u8] = b"GIF89a\x01\x00\x01\x00\x00\x00\x00";

    /// BMP header
    const BMP_HEADER: &[u8] = &[0x42, 0x4D, 0x46, 0x00, 0x00, 0x00];

    /// TIFF header (little-endian)
    const TIFF_HEADER_LE: &[u8] = &[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00];

    /// TIFF header (big-endian)
    const TIFF_HEADER_BE: &[u8] = &[0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x08];

    /// ICO header
    const ICO_HEADER: &[u8] = &[0x00, 0x00, 0x01, 0x00, 0x01, 0x00];

    /// WebP header
    const WEBP_HEADER: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";

    /// AVIF header (simplified)
    const AVIF_HEADER: &[u8] = b"\x00\x00\x00\x18ftypavif";

    #[test]
    fn test_detect_jpeg() {
        let format = detect_image_format(MINIMAL_JPEG).unwrap();
        assert_eq!(format, ImageFormat::Jpeg);
        assert_eq!(format.as_str(), "JPEG");
    }

    #[test]
    fn test_detect_png() {
        let format = detect_image_format(MINIMAL_PNG).unwrap();
        assert_eq!(format, ImageFormat::Png);
        assert_eq!(format.as_str(), "PNG");
    }

    #[test]
    fn test_detect_gif() {
        let format = detect_image_format(GIF_HEADER).unwrap();
        assert_eq!(format, ImageFormat::Gif);
        assert_eq!(format.as_str(), "GIF");
    }

    #[test]
    fn test_detect_bmp() {
        let format = detect_image_format(BMP_HEADER).unwrap();
        assert_eq!(format, ImageFormat::Bmp);
        assert_eq!(format.as_str(), "BMP");
    }

    #[test]
    fn test_detect_tiff_le() {
        let format = detect_image_format(TIFF_HEADER_LE).unwrap();
        assert_eq!(format, ImageFormat::Tiff);
        assert_eq!(format.as_str(), "TIFF");
    }

    #[test]
    fn test_detect_tiff_be() {
        let format = detect_image_format(TIFF_HEADER_BE).unwrap();
        assert_eq!(format, ImageFormat::Tiff);
    }

    #[test]
    fn test_detect_ico() {
        let format = detect_image_format(ICO_HEADER).unwrap();
        assert_eq!(format, ImageFormat::Ico);
        assert_eq!(format.as_str(), "ICO");
    }

    #[test]
    fn test_detect_webp() {
        let format = detect_image_format(WEBP_HEADER).unwrap();
        assert_eq!(format, ImageFormat::WebP);
        assert_eq!(format.as_str(), "WebP");
    }

    #[test]
    fn test_detect_avif() {
        let format = detect_image_format(AVIF_HEADER).unwrap();
        assert_eq!(format, ImageFormat::Avif);
        assert_eq!(format.as_str(), "AVIF");
    }

    #[test]
    fn test_empty_data() {
        let result = detect_image_format(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_insufficient_data() {
        let result = detect_image_format(&[0xFF, 0xD8]); // Only 2 bytes
        assert!(result.is_err());
    }

    #[test]
    fn test_unrecognized_format() {
        let data = b"This is not an image file";
        let result = detect_image_format(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_image_format() {
        assert!(verify_image_format(MINIMAL_JPEG).is_ok());
        assert!(verify_image_format(MINIMAL_PNG).is_ok());
        assert!(verify_image_format(b"not an image").is_err());
    }

    #[test]
    fn test_format_ordering_performance() {
        // JPEG should be detected first (most common in comics)
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let _ = detect_image_format(MINIMAL_JPEG);
        }
        let jpeg_time = start.elapsed();

        // Detection should be very fast (well under 1ms for 1000 iterations)
        assert!(jpeg_time.as_millis() < 10, "Format detection too slow");
    }
}
