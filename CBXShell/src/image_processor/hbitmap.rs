//! Windows HBITMAP creation from pixel data
//!
//! This module handles the conversion of RGBA pixel data to Windows HBITMAP format.
//! It performs color channel swapping (RGBA -> BGRA) and uses CreateDIBSection for
//! efficient bitmap creation compatible with Windows GDI.

use crate::utils::error::CbxError;
use std::ptr;
use windows::Win32::Graphics::Gdi::*;

type Result<T> = std::result::Result<T, CbxError>;

/// Convert RGBA pixel data to BGRA format (Windows native)
///
/// Windows GDI expects pixels in BGRA byte order, while the image crate
/// produces RGBA. This function swaps the red and blue channels.
///
/// # Arguments
/// * `rgba` - Source RGBA pixel data (4 bytes per pixel)
///
/// # Returns
/// * `Vec<u8>` - BGRA pixel data with swapped R and B channels
///
/// # Format
/// - Input:  R G B A | R G B A | ...
/// - Output: B G R A | B G R A | ...
///
/// # Examples
/// ```ignore
/// let rgba = vec![255, 0, 0, 255];  // Red pixel (RGBA)
/// let bgra = rgba_to_bgra(&rgba);
/// assert_eq!(bgra, vec![0, 0, 255, 255]);  // Red pixel (BGRA)
/// ```
pub fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = rgba.to_vec();

    // Swap R and B channels in each pixel (4 bytes)
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    bgra
}

/// Create Windows HBITMAP from BGRA pixel data
///
/// This function creates a device-independent bitmap (DIB) using CreateDIBSection,
/// matching the C++ implementation in cbxArchive.h:628-666.
///
/// # Arguments
/// * `bgra_data` - BGRA pixel data (4 bytes per pixel, bottom-up)
/// * `width` - Image width in pixels
/// * `height` - Image height in pixels
///
/// # Returns
/// * `Ok(HBITMAP)` - Successfully created bitmap handle
/// * `Err(CbxError)` - Creation failed
///
/// # Safety
/// - The returned HBITMAP must be deleted with DeleteObject when no longer needed
/// - The bitmap is created in RGBA32 format (32-bit with alpha)
/// - Pixel data is copied to the DIB section, so bgra_data can be dropped
///
/// # Windows API Used
/// - `CreateDIBSection`: Creates a DIB that applications can write to directly
/// - Format: 32-bit RGBA with BI_RGB (no compression)
///
/// # C++ Equivalent
/// ```cpp
/// BITMAPINFO bi = {0};
/// bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
/// bi.bmiHeader.biWidth = cx;
/// bi.bmiHeader.biHeight = cy;
/// bi.bmiHeader.biPlanes = 1;
/// bi.bmiHeader.biBitCount = 32;
/// bi.bmiHeader.biCompression = BI_RGB;
///
/// void* pvBits = NULL;
/// HBITMAP hBmp = CreateDIBSection(NULL, &bi, DIB_RGB_COLORS, &pvBits, NULL, 0);
/// memcpy(pvBits, data, size);
/// ```
pub fn create_hbitmap_from_bgra(bgra_data: &[u8], width: u32, height: u32) -> Result<HBITMAP> {
    if width == 0 || height == 0 {
        return Err(CbxError::Image(
            "Width and height must be greater than zero".to_string(),
        ));
    }

    let expected_size = (width * height * 4) as usize;
    if bgra_data.len() != expected_size {
        return Err(CbxError::Image(format!(
            "Invalid data size: expected {} bytes, got {}",
            expected_size,
            bgra_data.len()
        )));
    }

    // UNAVOIDABLE UNSAFE: CreateDIBSection and raw memory operations
    // Why unsafe is required:
    // 1. CreateDIBSection is a Windows GDI FFI call (gdi32.dll)
    // 2. No safe alternative: HBITMAP is a Windows-specific resource
    // 3. Raw pointer manipulation: pv_bits is allocated by Windows
    // 4. Memory copy to unmanaged memory: Windows owns the DIB pixel buffer
    //
    // Why this cannot be made safe:
    // - CreateDIBSection returns a raw pointer (pv_bits) to Windows-managed memory
    // - We must write pixel data to this Windows-allocated buffer
    // - Even if we create a slice, it still requires unsafe (same safety requirements)
    // - The memory ownership is split: HBITMAP handle vs pixel buffer
    //
    // Safety guarantees:
    // - Dimensions validated (width, height > 0)
    // - Data size validated (matches width * height * 4)
    // - pv_bits null-checked before use
    // - HBITMAP validity checked before returning
    // - copy_nonoverlapping: src and dst are valid, non-overlapping, properly aligned
    unsafe {
        // Create BITMAPINFO structure
        // Using BITMAPV5HEADER for better alpha channel support
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // Negative for top-down DIB
                biPlanes: 1,
                biBitCount: 32, // 32-bit RGBA
                biCompression: BI_RGB.0 as u32,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD::default(); 1],
        };

        // Pointer to receive DIB pixel data address
        let mut pv_bits: *mut std::ffi::c_void = ptr::null_mut();

        // Create DIB section
        let hbitmap = CreateDIBSection(
            None,           // Device context (NULL = screen)
            &bmi,           // Bitmap info
            DIB_RGB_COLORS, // Color usage
            &mut pv_bits,   // Pointer to bits
            None,           // File mapping object (NULL)
            0,              // Offset in file mapping
        )?; // Use ? to propagate the Result

        if hbitmap.is_invalid() || hbitmap.0 == 0 {
            return Err(CbxError::Windows(windows::core::Error::from_win32()));
        }

        if pv_bits.is_null() {
            let _ = DeleteObject(hbitmap);
            return Err(CbxError::Image(
                "CreateDIBSection succeeded but returned NULL bits pointer".to_string(),
            ));
        }

        // Copy pixel data to DIB section
        ptr::copy_nonoverlapping(bgra_data.as_ptr(), pv_bits as *mut u8, bgra_data.len());

        Ok(hbitmap)
    }
}

/// Convert RGBA image to HBITMAP (convenience function)
///
/// This is a high-level wrapper that combines rgba_to_bgra and create_hbitmap_from_bgra.
///
/// # Arguments
/// * `rgba_data` - Source RGBA pixel data
/// * `width` - Image width in pixels
/// * `height` - Image height in pixels
///
/// # Returns
/// * `Ok(HBITMAP)` - Successfully created bitmap handle
/// * `Err(CbxError)` - Conversion failed
#[allow(dead_code)] // Part of public API, may be used in future
pub fn create_hbitmap_from_rgba(rgba_data: &[u8], width: u32, height: u32) -> Result<HBITMAP> {
    let bgra_data = rgba_to_bgra(rgba_data);
    create_hbitmap_from_bgra(&bgra_data, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_to_bgra_conversion() {
        // Single red pixel (RGBA)
        let rgba = vec![255, 0, 0, 255];
        let bgra = rgba_to_bgra(&rgba);
        assert_eq!(bgra, vec![0, 0, 255, 255]); // Blue channel now has 255

        // Single green pixel
        let rgba = vec![0, 255, 0, 255];
        let bgra = rgba_to_bgra(&rgba);
        assert_eq!(bgra, vec![0, 255, 0, 255]); // Green unchanged

        // Single blue pixel
        let rgba = vec![0, 0, 255, 255];
        let bgra = rgba_to_bgra(&rgba);
        assert_eq!(bgra, vec![255, 0, 0, 255]); // Red channel now has 255
    }

    #[test]
    fn test_rgba_to_bgra_multiple_pixels() {
        // Two pixels: red and blue
        let rgba = vec![
            255, 0, 0, 255, // Red
            0, 0, 255, 255, // Blue
        ];
        let bgra = rgba_to_bgra(&rgba);
        assert_eq!(
            bgra,
            vec![
                0, 0, 255, 255, // Red in BGRA
                255, 0, 0, 255, // Blue in BGRA
            ]
        );
    }

    #[test]
    fn test_rgba_to_bgra_transparency() {
        // Semi-transparent red pixel
        let rgba = vec![255, 0, 0, 128];
        let bgra = rgba_to_bgra(&rgba);
        assert_eq!(bgra, vec![0, 0, 255, 128]); // Alpha preserved
    }

    #[test]
    fn test_create_hbitmap_1x1() {
        // Create a 1x1 white pixel
        let bgra = vec![255, 255, 255, 255]; // White in BGRA
        let result = create_hbitmap_from_bgra(&bgra, 1, 1);

        assert!(result.is_ok(), "Failed to create 1x1 HBITMAP");

        // Clean up
        if let Ok(hbitmap) = result {
            unsafe {
                assert!(hbitmap.0 != 0);
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_hbitmap_invalid_dimensions() {
        let bgra = vec![255, 255, 255, 255];

        // Zero width
        let result = create_hbitmap_from_bgra(&bgra, 0, 1);
        assert!(result.is_err());

        // Zero height
        let result = create_hbitmap_from_bgra(&bgra, 1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_hbitmap_size_mismatch() {
        // Data for 2x2 image but claim it's 1x1
        let bgra = vec![
            255, 255, 255, 255, // Pixel 1
            0, 0, 0, 255, // Pixel 2
            255, 0, 0, 255, // Pixel 3
            0, 255, 0, 255, // Pixel 4
        ];

        let result = create_hbitmap_from_bgra(&bgra, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_hbitmap_4x4() {
        // Create a 4x4 checkerboard pattern
        let mut bgra = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    bgra.extend_from_slice(&[255, 255, 255, 255]); // White
                } else {
                    bgra.extend_from_slice(&[0, 0, 0, 255]); // Black
                }
            }
        }

        let result = create_hbitmap_from_bgra(&bgra, 4, 4);
        assert!(result.is_ok(), "Failed to create 4x4 HBITMAP");

        // Clean up
        if let Ok(hbitmap) = result {
            unsafe {
                assert!(hbitmap.0 != 0);
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_hbitmap_from_rgba_convenience() {
        // Test the convenience function
        let rgba = vec![
            255, 0, 0, 255, // Red
            0, 255, 0, 255, // Green
            0, 0, 255, 255, // Blue
            255, 255, 255, 255, // White
        ];

        let result = create_hbitmap_from_rgba(&rgba, 2, 2);
        assert!(result.is_ok(), "Failed to create HBITMAP from RGBA");

        // Clean up
        if let Ok(hbitmap) = result {
            unsafe {
                assert!(hbitmap.0 != 0);
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_create_hbitmap_large() {
        // Create a 256x256 gradient
        let mut bgra = Vec::new();
        for y in 0..256 {
            for x in 0..256 {
                bgra.push(x as u8); // B
                bgra.push(y as u8); // G
                bgra.push(0); // R
                bgra.push(255); // A
            }
        }

        let result = create_hbitmap_from_bgra(&bgra, 256, 256);
        assert!(result.is_ok(), "Failed to create 256x256 HBITMAP");

        // Clean up
        if let Ok(hbitmap) = result {
            unsafe {
                assert!(hbitmap.0 != 0);
                DeleteObject(hbitmap);
            }
        }
    }

    #[test]
    fn test_hbitmap_handle_not_null() {
        let bgra = vec![128, 128, 128, 255]; // Gray pixel
        let result = create_hbitmap_from_bgra(&bgra, 1, 1);

        assert!(result.is_ok());
        let hbitmap = result.unwrap();
        assert_ne!(hbitmap.0, 0, "HBITMAP handle should not be null");

        // Clean up
        unsafe {
            DeleteObject(hbitmap);
        }
    }
}
