//! Image processing module for thumbnail generation
//!
//! This module handles decoding images from archive data, resizing them
//! to thumbnail size, and converting to Windows HBITMAP format.
//!
//! # Architecture
//!
//! The module is organized into four main components:
//!
//! - **decoder**: Decodes images from raw bytes using the `image` crate
//! - **resizer**: Calculates thumbnail dimensions and performs high-quality resizing
//! - **hbitmap**: Converts pixel data to Windows HBITMAP format
//! - **thumbnail**: Orchestrates the complete pipeline
//!
//! # Pipeline
//!
//! The thumbnail generation pipeline matches the C++ implementation in cbxArchive.h:
//!
//! 1. Decode image from compressed archive data
//! 2. Calculate target size (aspect ratio preserved, no upscaling)
//! 3. Resize using high-quality algorithm (Triangle/Lanczos3)
//! 4. Apply white background to transparent areas (Windows compatibility)
//! 5. Convert RGBA to BGRA format (Windows native)
//! 6. Create HBITMAP using CreateDIBSection
//!
//! # Supported Image Formats
//!
//! - JPEG (.jpg, .jpeg, .jpe, .jfif)
//! - PNG (.png)
//! - GIF (.gif)
//! - BMP (.bmp)
//! - WebP (.webp) - NEW in Rust version!
//! - AVIF (.avif) - NEW in Rust version!
//! - TIFF (.tif, .tiff)
//! - ICO (.ico)
//!
//! # Examples
//!
//! ```ignore
//! use cbxshell::image_processor::thumbnail::{create_thumbnail, ThumbnailConfig};
//!
//! // Load image from file or archive
//! let image_data = std::fs::read("comic_page.jpg")?;
//!
//! // Create thumbnail with default settings (256x256, white background)
//! let config = ThumbnailConfig::default();
//! let hbitmap = create_thumbnail(&image_data, config)?;
//!
//! // Use hbitmap with Windows APIs
//! // Don't forget to call DeleteObject(hbitmap) when done!
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Performance Considerations
//!
//! - Uses SIMD-optimized resizing via `fast_image_resize`
//! - Zero-copy where possible
//! - Memory-efficient streaming for large images
//! - Matches or exceeds C++ performance
//!
//! # C++ Compatibility
//!
//! This module provides exact behavioral compatibility with the original C++ code:
//!
//! - Same aspect ratio calculation algorithm
//! - Same "no upscaling" behavior
//! - Same white background for transparent images
//! - Same HALFTONE-equivalent resize quality (Triangle/Bilinear)

mod decoder;
mod hbitmap;
pub mod magic;
mod resizer;
pub mod thumbnail;

/// Supported image file extensions
///
/// This matches the C++ implementation in cbxArchive.h:553-567 plus new formats.
#[allow(dead_code)] // Used by is_image_file function
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", // JPEG
    "png",  // PNG
    "gif",  // GIF
    "bmp",  // BMP
    "webp", // WebP (NEW!)
    "avif", // AVIF (NEW!)
    "tif", "tiff", // TIFF
    "ico",  // Icon
];

/// Check if a file is a supported image format
///
/// This function checks the file extension against the list of supported formats.
/// It's case-insensitive and matches the C++ behavior in cbxArchive.h:553-567.
///
/// # Arguments
/// * `filename` - File name or path to check
///
/// # Returns
/// * `true` if the file extension is supported
/// * `false` otherwise
///
/// # Examples
/// ```ignore
/// use cbxshell::image_processor::is_image_file;
///
/// assert!(is_image_file("page001.jpg"));
/// assert!(is_image_file("cover.PNG"));  // Case insensitive
/// assert!(is_image_file("photo.webp")); // New format
/// assert!(!is_image_file("readme.txt"));
/// ```
#[allow(dead_code)] // Part of public API, may be used in future
pub fn is_image_file(filename: &str) -> bool {
    if let Some(ext) = std::path::Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
    {
        SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_file_jpeg() {
        assert!(is_image_file("image.jpg"));
        assert!(is_image_file("image.jpeg"));
        assert!(is_image_file("image.jpe"));
        assert!(is_image_file("image.jfif"));
    }

    #[test]
    fn test_is_image_file_png() {
        assert!(is_image_file("image.png"));
        assert!(is_image_file("IMAGE.PNG")); // Case insensitive
    }

    #[test]
    fn test_is_image_file_webp() {
        assert!(is_image_file("image.webp"));
        assert!(is_image_file("photo.WEBP"));
    }

    #[test]
    fn test_is_image_file_avif() {
        assert!(is_image_file("image.avif"));
    }

    #[test]
    fn test_is_image_file_other_formats() {
        assert!(is_image_file("image.gif"));
        assert!(is_image_file("image.bmp"));
        assert!(is_image_file("image.tif"));
        assert!(is_image_file("image.tiff"));
        assert!(is_image_file("icon.ico"));
    }

    #[test]
    fn test_is_image_file_not_image() {
        assert!(!is_image_file("document.txt"));
        assert!(!is_image_file("archive.zip"));
        assert!(!is_image_file("video.mp4"));
        assert!(!is_image_file("audio.mp3"));
    }

    #[test]
    fn test_is_image_file_no_extension() {
        assert!(!is_image_file("README"));
        assert!(!is_image_file("file"));
    }

    #[test]
    fn test_is_image_file_with_path() {
        assert!(is_image_file("path/to/image.jpg"));
        assert!(is_image_file(r"C:\Users\test\photo.png"));
        assert!(!is_image_file("path/to/document.txt"));
    }

    #[test]
    fn test_is_image_file_case_insensitive() {
        assert!(is_image_file("image.JPG"));
        assert!(is_image_file("image.JpG"));
        assert!(is_image_file("image.PnG"));
        assert!(is_image_file("image.WEBP"));
    }

    #[test]
    fn test_is_image_file_edge_cases() {
        assert!(!is_image_file(""));
        assert!(!is_image_file(".jpg")); // Just extension, no filename
        assert!(is_image_file("a.jpg")); // Single character filename
    }
}
