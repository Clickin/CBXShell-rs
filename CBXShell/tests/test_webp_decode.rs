//! Integration test for WebP image decoding
//! Verifies that WebP images can be decoded and converted to thumbnails

use cbxshell::create_thumbnail_with_size;

/// Minimal valid WebP file (1x1 red pixel, lossy VP8 format)
const MINIMAL_WEBP: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x40, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x20,
    0x34, 0x00, 0x00, 0x00, 0xF0, 0x01, 0x00, 0x9D, 0x01, 0x2A, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00,
    0x1C, 0x25, 0xA0, 0x02, 0x74, 0xBA, 0x01, 0xF8, 0x00, 0x04, 0x4C, 0x00, 0x00, 0xFE, 0xF5, 0xB8,
    0x7F, 0xFE, 0x9A, 0x47, 0x8D, 0x23, 0xC6, 0x91, 0xF1, 0x70, 0xFF, 0xEE, 0x81, 0x3F, 0x74, 0x09,
    0xFB, 0xA0, 0x4F, 0xFD, 0xCD, 0xA0, 0x00, 0x00,
];

#[test]
fn test_webp_decoding() {
    println!(
        "Testing WebP decoding with minimal WebP file ({} bytes)",
        MINIMAL_WEBP.len()
    );

    // Attempt to create thumbnail from WebP
    let result = create_thumbnail_with_size(MINIMAL_WEBP, 256, 256);

    match &result {
        Ok(hbitmap) => {
            println!("SUCCESS: WebP decoded and HBITMAP created: {:?}", hbitmap);
            // Clean up
            unsafe {
                use windows::Win32::Graphics::Gdi::DeleteObject;
                let _ = DeleteObject(*hbitmap);
            }
        }
        Err(e) => {
            println!("FAILED: WebP decoding error: {}", e);
            println!("Error type: {:?}", e);
        }
    }

    assert!(
        result.is_ok(),
        "WebP decoding should succeed, but got: {:?}",
        result.err()
    );
}

#[test]
fn test_image_crate_webp_support() {
    use image::ImageReader;
    use std::io::Cursor;

    println!("Testing image crate WebP support directly...");

    let reader = ImageReader::new(Cursor::new(MINIMAL_WEBP)).with_guessed_format();

    match reader {
        Ok(reader) => {
            println!("Format detection: {:?}", reader.format());

            match reader.decode() {
                Ok(img) => {
                    println!(
                        "SUCCESS: Decoded WebP image: {}x{}",
                        img.width(),
                        img.height()
                    );
                    assert!(img.width() > 0 && img.height() > 0);
                }
                Err(e) => {
                    panic!("Failed to decode WebP: {}", e);
                }
            }
        }
        Err(e) => {
            panic!("Failed to create image reader: {}", e);
        }
    }
}
