//! Thumbnail generation pipeline
//!
//! This module orchestrates the complete thumbnail generation process:
//! 1. Decode image from raw bytes
//! 2. Calculate target thumbnail size (aspect ratio preserved)
//! 3. Resize image using high-quality algorithm
//! 4. Apply white background for transparent images (C++ behavior)
//! 5. Convert RGBA to BGRA format
//! 6. Create Windows HBITMAP
//!
//! This matches the C++ implementation in cbxArchive.h:628-666 (OnExtract).

use crate::utils::error::CbxError;
use image::{GenericImageView, RgbaImage};
use windows::Win32::Graphics::Gdi::HBITMAP;

use super::decoder;
use super::hbitmap;
use super::resizer::{self, ResizeFilter};

type Result<T> = std::result::Result<T, CbxError>;

/// Thumbnail generation configuration
///
/// Controls all aspects of thumbnail creation including size limits,
/// background color, and resize algorithm quality.
#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    /// Maximum thumbnail width in pixels
    pub max_width: u32,

    /// Maximum thumbnail height in pixels
    pub max_height: u32,

    /// Background color for transparent images (RGBA format)
    /// Default: (255, 255, 255, 255) - opaque white
    pub background_color: (u8, u8, u8, u8),

    /// Resize algorithm to use
    /// Default: Triangle (matches C++ HALFTONE mode)
    pub resize_filter: ResizeFilter,
}

impl Default for ThumbnailConfig {
    /// Default configuration matching C++ behavior
    ///
    /// - Max size: 256x256 (Windows default thumbnail size)
    /// - Background: White (RGB 255, 255, 255)
    /// - Filter: Triangle/Bilinear (matches HALFTONE)
    fn default() -> Self {
        Self {
            max_width: 256,
            max_height: 256,
            background_color: (255, 255, 255, 255), // White background
            resize_filter: ResizeFilter::Triangle,  // Match C++ HALFTONE
        }
    }
}

/// Create thumbnail HBITMAP from image data
///
/// This is the main entry point for thumbnail generation. It orchestrates
/// the complete pipeline and matches the C++ implementation behavior.
///
/// # Arguments
/// * `image_data` - Raw image file bytes (any supported format)
/// * `config` - Thumbnail generation configuration
///
/// # Returns
/// * `Ok(HBITMAP)` - Successfully created thumbnail bitmap
/// * `Err(CbxError)` - Failed to create thumbnail
///
/// # Pipeline Steps
/// 1. Decode: Parse image format and decode to RGBA
/// 2. Calculate: Determine thumbnail size (aspect ratio preserved, no upscaling)
/// 3. Resize: High-quality downscale using selected algorithm
/// 4. Composite: Apply white background to transparent areas
/// 5. Convert: RGBA to BGRA for Windows compatibility
/// 6. Create: Generate HBITMAP using CreateDIBSection
///
/// # C++ Equivalent (cbxArchive.h:628-666)
/// ```cpp
/// HRESULT CCBXArchive::OnExtract(LPCRECT prgSize, HBITMAP *phBmpThumbnail) {
///     // 1. Extract image from archive to IStream
///     // 2. Load with CImage (GDI+)
///     // 3. Calculate dimensions maintaining aspect ratio
///     // 4. Create DC and bitmap
///     // 5. StretchBlt with HALFTONE mode
///     // 6. Fill white background
///     // 7. Return HBITMAP
/// }
/// ```
///
/// # Examples
/// ```ignore
/// use cbxshell::image_processor::thumbnail::{create_thumbnail, ThumbnailConfig};
///
/// let jpeg_data = std::fs::read("comic_page.jpg")?;
/// let config = ThumbnailConfig::default();
/// let hbitmap = create_thumbnail(&jpeg_data, config)?;
///
/// // Use hbitmap with Windows APIs
/// // Remember to DeleteObject(hbitmap) when done
/// ```
pub fn create_thumbnail(image_data: &[u8], config: ThumbnailConfig) -> Result<HBITMAP> {
    // Step 1: Decode image from bytes
    crate::utils::debug_log::debug_log(&format!(
        "Decoding image from {} bytes...",
        image_data.len()
    ));
    let img = match decoder::decode_image(image_data) {
        Ok(img) => {
            crate::utils::debug_log::debug_log(&format!(
                "Image decoded successfully: {}x{}",
                img.width(),
                img.height()
            ));
            img
        }
        Err(e) => {
            crate::utils::debug_log::debug_log(&format!("ERROR: Image decoding failed: {}", e));
            // Try to detect format from magic bytes for better error message
            let format_hint = if image_data.len() >= 4 {
                match &image_data[0..4] {
                    [0xFF, 0xD8, 0xFF, _] => "JPEG",
                    [0x89, 0x50, 0x4E, 0x47] => "PNG",
                    [0x47, 0x49, 0x46, 0x38] => "GIF",
                    [0x42, 0x4D, _, _] => "BMP",
                    [0x52, 0x49, 0x46, 0x46]
                        if image_data.len() >= 12 && &image_data[8..12] == b"WEBP" =>
                    {
                        "WebP"
                    }
                    _ => "Unknown",
                }
            } else {
                "Too small"
            };
            crate::utils::debug_log::debug_log(&format!("Detected format: {}", format_hint));
            return Err(e);
        }
    };

    // Step 2: Calculate target thumbnail size
    let (src_width, src_height) = img.dimensions();
    let (target_width, target_height) = resizer::calculate_thumbnail_size(
        src_width,
        src_height,
        config.max_width,
        config.max_height,
    );

    // Handle edge case: zero dimensions
    if target_width == 0 || target_height == 0 {
        return Err(CbxError::Image(
            "Invalid image dimensions (0x0)".to_string(),
        ));
    }

    // Step 3: Convert to RGBA format
    let mut rgba = img.to_rgba8();

    // Step 4: Resize if dimensions changed
    if (target_width, target_height) != (src_width, src_height) {
        rgba = resizer::resize_image(&rgba, target_width, target_height, config.resize_filter)?;
    }

    // Step 5: Apply white background for transparency (C++ behavior)
    // This matches the C++ code which fills the background with white (RGB 255,255,255)
    // before drawing the image
    apply_background(&mut rgba, config.background_color);

    // Step 6: Convert RGBA to BGRA (Windows format)
    let bgra = hbitmap::rgba_to_bgra(rgba.as_raw());

    // Step 7: Create Windows HBITMAP
    hbitmap::create_hbitmap_from_bgra(&bgra, target_width, target_height)
}

/// Apply background color to transparent areas
///
/// This function composites the image with a solid background color,
/// matching the C++ behavior of rendering transparent images on white.
///
/// The alpha channel is used to blend the foreground pixel with the background:
/// ```text
/// final_color = pixel_color * alpha + background_color * (1 - alpha)
/// ```
///
/// After blending, the alpha channel is set to 255 (fully opaque) since
/// Windows Explorer doesn't properly handle alpha in thumbnails.
///
/// # Arguments
/// * `rgba` - Image to modify (in-place)
/// * `bg` - Background color (R, G, B, A)
///
/// # C++ Equivalent (cbxArchive.h:658-662)
/// ```cpp
/// HBRUSH hBrush = CreateSolidBrush(RGB(255, 255, 255));
/// FillRect(hdcDest, &rcDest, hBrush);
/// DeleteObject(hBrush);
/// ```
fn apply_background(rgba: &mut RgbaImage, bg: (u8, u8, u8, u8)) {
    for pixel in rgba.pixels_mut() {
        let alpha = pixel[3] as f32 / 255.0;

        if alpha < 1.0 {
            // Blend with background using alpha compositing
            pixel[0] = ((pixel[0] as f32 * alpha) + (bg.0 as f32 * (1.0 - alpha))) as u8;
            pixel[1] = ((pixel[1] as f32 * alpha) + (bg.1 as f32 * (1.0 - alpha))) as u8;
            pixel[2] = ((pixel[2] as f32 * alpha) + (bg.2 as f32 * (1.0 - alpha))) as u8;
        }

        // Set alpha to fully opaque (Windows Explorer doesn't handle transparency well)
        pixel[3] = 255;
    }
}

/// Create thumbnail with custom dimensions
///
/// Convenience function for quick thumbnail creation with custom size.
///
/// # Arguments
/// * `image_data` - Raw image file bytes
/// * `max_width` - Maximum thumbnail width
/// * `max_height` - Maximum thumbnail height
///
/// # Returns
/// * `Ok(HBITMAP)` - Successfully created thumbnail
/// * `Err(CbxError)` - Failed to create thumbnail
pub fn create_thumbnail_with_size(
    image_data: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<HBITMAP> {
    let config = ThumbnailConfig {
        max_width,
        max_height,
        ..Default::default()
    };
    create_thumbnail(image_data, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use windows::Win32::Graphics::Gdi::DeleteObject;

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

    #[test]
    fn test_create_thumbnail_default_config() {
        let result = create_thumbnail(MINIMAL_JPEG, ThumbnailConfig::default());
        assert!(
            result.is_ok(),
            "Failed to create thumbnail: {:?}",
            result.err()
        );

        // Clean up
        if let Ok(hbitmap) = result {
            unsafe {
                assert_ne!(hbitmap.0, 0);
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_thumbnail_custom_size() {
        let config = ThumbnailConfig {
            max_width: 128,
            max_height: 128,
            ..Default::default()
        };

        let result = create_thumbnail(MINIMAL_JPEG, config);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_thumbnail_with_size_convenience() {
        let result = create_thumbnail_with_size(MINIMAL_JPEG, 64, 64);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_thumbnail_invalid_data() {
        let invalid_data = b"This is not an image";
        let result = create_thumbnail(invalid_data, ThumbnailConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_thumbnail_empty_data() {
        let result = create_thumbnail(&[], ThumbnailConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_background_opaque() {
        // Create a fully opaque red pixel
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));

        // Apply white background
        apply_background(&mut img, (255, 255, 255, 255));

        // Should remain red since it's fully opaque
        let pixel = img.get_pixel(0, 0);
        assert_eq!(pixel[0], 255); // Red
        assert_eq!(pixel[1], 0); // Green
        assert_eq!(pixel[2], 0); // Blue
        assert_eq!(pixel[3], 255); // Alpha
    }

    #[test]
    fn test_apply_background_transparent() {
        // Create a fully transparent pixel
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 0]));

        // Apply white background
        apply_background(&mut img, (255, 255, 255, 255));

        // Should become white since original is fully transparent
        let pixel = img.get_pixel(0, 0);
        assert_eq!(pixel[0], 255); // Red (from white)
        assert_eq!(pixel[1], 255); // Green (from white)
        assert_eq!(pixel[2], 255); // Blue (from white)
        assert_eq!(pixel[3], 255); // Alpha (opaque)
    }

    #[test]
    fn test_apply_background_semi_transparent() {
        // Create a semi-transparent red pixel (50% alpha)
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 128]));

        // Apply white background
        apply_background(&mut img, (255, 255, 255, 255));

        // Should be blend of red and white
        let pixel = img.get_pixel(0, 0);
        // 255 * 0.5 + 255 * 0.5 = 255
        assert!(pixel[0] > 200); // Red component (blend)
        assert!(pixel[1] > 100); // Green component (from white)
        assert!(pixel[2] > 100); // Blue component (from white)
        assert_eq!(pixel[3], 255); // Alpha (opaque)
    }

    #[test]
    fn test_config_default_values() {
        let config = ThumbnailConfig::default();
        assert_eq!(config.max_width, 256);
        assert_eq!(config.max_height, 256);
        assert_eq!(config.background_color, (255, 255, 255, 255));
        assert_eq!(config.resize_filter, ResizeFilter::Triangle);
    }

    #[test]
    fn test_thumbnail_with_lanczos3() {
        let config = ThumbnailConfig {
            resize_filter: ResizeFilter::Lanczos3,
            ..Default::default()
        };

        let result = create_thumbnail(MINIMAL_JPEG, config);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_thumbnail_custom_background() {
        // Create thumbnail with black background instead of white
        let config = ThumbnailConfig {
            background_color: (0, 0, 0, 255), // Black
            ..Default::default()
        };

        let result = create_thumbnail(MINIMAL_JPEG, config);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_thumbnail_very_large_size() {
        // Test with very large max dimensions
        let config = ThumbnailConfig {
            max_width: 4096,
            max_height: 4096,
            ..Default::default()
        };

        // 1x1 image should not be upscaled
        let result = create_thumbnail(MINIMAL_JPEG, config);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_thumbnail_very_small_size() {
        // Test with very small max dimensions
        let config = ThumbnailConfig {
            max_width: 16,
            max_height: 16,
            ..Default::default()
        };

        let result = create_thumbnail(MINIMAL_JPEG, config);
        assert!(result.is_ok());

        if let Ok(hbitmap) = result {
            unsafe {
                DeleteObject(hbitmap);
            }
        }
    }
}
