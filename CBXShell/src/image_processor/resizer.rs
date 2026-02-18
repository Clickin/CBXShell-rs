//! Image resizing with aspect ratio preservation
//!
//! This module handles thumbnail size calculation and high-quality image resizing
//! using the fast_image_resize crate, matching the C++ HALFTONE behavior.

use crate::utils::error::CbxError;
use fast_image_resize as fr;
use fast_image_resize::images::Image;
use image::RgbaImage;

type Result<T> = std::result::Result<T, CbxError>;

/// Resize filter algorithm
///
/// Maps to fast_image_resize filter types with quality/performance tradeoffs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeFilter {
    /// Bilinear filtering (fastest, good quality)
    /// Matches C++ StretchBlt with HALFTONE mode
    Triangle,

    /// Lanczos3 filtering (slower, highest quality)
    /// Best for photographic content where quality is critical
    Lanczos3,
}

impl From<ResizeFilter> for fr::FilterType {
    fn from(filter: ResizeFilter) -> Self {
        match filter {
            ResizeFilter::Triangle => fr::FilterType::Bilinear,
            ResizeFilter::Lanczos3 => fr::FilterType::Lanczos3,
        }
    }
}

/// Calculate thumbnail dimensions maintaining aspect ratio
///
/// This function replicates the C++ behavior from cbxArchive.h:649-650:
/// ```cpp
/// rx = (float)cx.cx / (float)pSize->cx;
/// ry = (float)cx.cy / (float)pSize->cy;
/// r = min(rx, ry);  // Maintain aspect ratio
/// ```ignore
///
/// # Arguments
/// * `src_width` - Original image width
/// * `src_height` - Original image height
/// * `max_width` - Maximum thumbnail width
/// * `max_height` - Maximum thumbnail height
///
/// # Returns
/// * `(width, height)` - Calculated thumbnail dimensions
///
/// # Behavior
/// - Maintains original aspect ratio
/// - Never upscales (returns original size if smaller than max)
/// - Fits within max_width x max_height bounds
///
/// # Examples
/// ```
/// // Landscape image 1000x500 -> 256x128 (2:1 ratio preserved)
/// let (w, h) = calculate_thumbnail_size(1000, 500, 256, 256);
/// assert_eq!((w, h), (256, 128));
///
/// // Small image 100x100 -> 100x100 (no upscaling)
/// let (w, h) = calculate_thumbnail_size(100, 100, 256, 256);
/// assert_eq!((w, h), (100, 100));
/// ```
pub fn calculate_thumbnail_size(
    src_width: u32,
    src_height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    if src_width == 0 || src_height == 0 {
        return (0, 0);
    }

    // Calculate scale factors for width and height
    let rx = max_width as f32 / src_width as f32;
    let ry = max_height as f32 / src_height as f32;

    // Use the smaller scale to maintain aspect ratio
    let scale = rx.min(ry);

    // Don't upscale images (C++ behavior: if scale >= 1.0, return original)
    if scale >= 1.0 {
        return (src_width, src_height);
    }

    // Calculate new dimensions
    let new_width = (src_width as f32 * scale).round() as u32;
    let new_height = (src_height as f32 * scale).round() as u32;

    // Ensure at least 1x1 pixel result
    (new_width.max(1), new_height.max(1))
}

/// Resize image to target dimensions using high-quality algorithm
///
/// Uses fast_image_resize for efficient SIMD-optimized resizing.
/// Matches C++ StretchBlt behavior with HALFTONE stretch mode.
///
/// # Arguments
/// * `source` - Source RGBA image
/// * `target_width` - Desired output width
/// * `target_height` - Desired output height
/// * `filter` - Resize algorithm to use
///
/// # Returns
/// * `Ok(RgbaImage)` - Successfully resized image
/// * `Err(CbxError::Image)` - Resize operation failed
///
/// # Performance
/// - Uses SIMD optimization when available
/// - Efficient for downscaling operations
/// - Memory-safe with Rust's ownership guarantees
pub fn resize_image(
    source: &RgbaImage,
    target_width: u32,
    target_height: u32,
    filter: ResizeFilter,
) -> Result<RgbaImage> {
    let (src_width, src_height) = source.dimensions();

    // Validate dimensions
    if target_width == 0 || target_height == 0 {
        return Err(CbxError::Image(
            "Target dimensions must be greater than zero".to_string(),
        ));
    }

    // If dimensions match, no resize needed
    if src_width == target_width && src_height == target_height {
        return Ok(source.clone());
    }

    // Create source image view for fast_image_resize
    let src_view = Image::from_vec_u8(
        src_width,
        src_height,
        source.as_raw().to_vec(),
        fr::PixelType::U8x4,
    )
    .map_err(|e| CbxError::Image(format!("Failed to create source view: {}", e)))?;

    // Create destination image buffer
    let mut dst_image = Image::new(target_width, target_height, fr::PixelType::U8x4);

    // Create resizer with selected algorithm
    let mut resizer = fr::Resizer::new();

    // Perform resize operation with algorithm specified in options
    resizer
        .resize(
            &src_view,
            &mut dst_image,
            &fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(filter.into())),
        )
        .map_err(|e| CbxError::Image(format!("Resize operation failed: {}", e)))?;

    // Convert back to RgbaImage
    RgbaImage::from_raw(target_width, target_height, dst_image.into_vec())
        .ok_or_else(|| CbxError::Image("Failed to create output image".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn test_aspect_ratio_landscape() {
        // Landscape image 1000x500 should scale to 256x128 (2:1 ratio)
        let (w, h) = calculate_thumbnail_size(1000, 500, 256, 256);
        assert_eq!(w, 256);
        assert_eq!(h, 128);

        // Verify aspect ratio is preserved
        let original_ratio = 1000.0 / 500.0;
        let new_ratio = w as f32 / h as f32;
        assert!((original_ratio - new_ratio).abs() < 0.01);
    }

    #[test]
    fn test_aspect_ratio_portrait() {
        // Portrait image 500x1000 should scale to 128x256 (1:2 ratio)
        let (w, h) = calculate_thumbnail_size(500, 1000, 256, 256);
        assert_eq!(w, 128);
        assert_eq!(h, 256);

        // Verify aspect ratio is preserved
        let original_ratio = 500.0 / 1000.0;
        let new_ratio = w as f32 / h as f32;
        assert!((original_ratio - new_ratio).abs() < 0.01);
    }

    #[test]
    fn test_aspect_ratio_square() {
        // Square image 1000x1000 should scale to 256x256
        let (w, h) = calculate_thumbnail_size(1000, 1000, 256, 256);
        assert_eq!(w, 256);
        assert_eq!(h, 256);
    }

    #[test]
    fn test_no_upscale() {
        // Small image 100x100 should not be upscaled
        let (w, h) = calculate_thumbnail_size(100, 100, 256, 256);
        assert_eq!(w, 100);
        assert_eq!(h, 100);

        // Very small image
        let (w, h) = calculate_thumbnail_size(50, 75, 256, 256);
        assert_eq!(w, 50);
        assert_eq!(h, 75);
    }

    #[test]
    fn test_exact_fit() {
        // Image exactly matches max size
        let (w, h) = calculate_thumbnail_size(256, 256, 256, 256);
        assert_eq!(w, 256);
        assert_eq!(h, 256);
    }

    #[test]
    fn test_zero_dimensions() {
        let (w, h) = calculate_thumbnail_size(0, 0, 256, 256);
        assert_eq!(w, 0);
        assert_eq!(h, 0);

        let (w, h) = calculate_thumbnail_size(100, 0, 256, 256);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_very_wide_image() {
        // Extremely wide image 4000x100
        let (w, h) = calculate_thumbnail_size(4000, 100, 256, 256);
        assert_eq!(w, 256);
        assert_eq!(h, 6); // 256 * (100/4000) = 6.4 rounded to 6
    }

    #[test]
    fn test_resize_image_downscale() {
        // Create a simple 4x4 red image
        let mut source = RgbaImage::new(4, 4);
        for pixel in source.pixels_mut() {
            *pixel = Rgba([255, 0, 0, 255]); // Red
        }

        // Resize to 2x2
        let result = resize_image(&source, 2, 2, ResizeFilter::Triangle);
        assert!(result.is_ok());

        let resized = result.unwrap();
        assert_eq!(resized.width(), 2);
        assert_eq!(resized.height(), 2);

        // Check that pixels are still predominantly red
        let pixel = resized.get_pixel(0, 0);
        assert!(pixel[0] > 200); // Red channel dominant
        assert!(pixel[3] == 255); // Alpha fully opaque
    }

    #[test]
    fn test_resize_image_same_size() {
        // Create a 10x10 image
        let source = RgbaImage::new(10, 10);

        // Resize to same dimensions (should return clone)
        let result = resize_image(&source, 10, 10, ResizeFilter::Triangle);
        assert!(result.is_ok());

        let resized = result.unwrap();
        assert_eq!(resized.width(), 10);
        assert_eq!(resized.height(), 10);
    }

    #[test]
    fn test_resize_image_invalid_dimensions() {
        let source = RgbaImage::new(10, 10);

        // Zero width
        let result = resize_image(&source, 0, 10, ResizeFilter::Triangle);
        assert!(result.is_err());

        // Zero height
        let result = resize_image(&source, 10, 0, ResizeFilter::Triangle);
        assert!(result.is_err());
    }

    #[test]
    fn test_resize_filters() {
        let source = RgbaImage::new(100, 100);

        // Test Triangle filter
        let result = resize_image(&source, 50, 50, ResizeFilter::Triangle);
        assert!(result.is_ok());

        // Test Lanczos3 filter
        let result = resize_image(&source, 50, 50, ResizeFilter::Lanczos3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resize_large_to_small() {
        // Create a large gradient image
        let mut source = RgbaImage::new(1000, 800);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            let r = (x * 255 / 1000) as u8;
            let g = (y * 255 / 800) as u8;
            *pixel = Rgba([r, g, 0, 255]);
        }

        // Resize to thumbnail
        let result = resize_image(&source, 200, 160, ResizeFilter::Lanczos3);
        assert!(result.is_ok());

        let resized = result.unwrap();
        assert_eq!(resized.width(), 200);
        assert_eq!(resized.height(), 160);
    }
}
