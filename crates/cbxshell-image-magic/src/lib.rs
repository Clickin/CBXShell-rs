pub use cbxshell_core::utils;
use cbxshell_core::{CbxError, Result};

#[path = "../../../CBXShell/src/image_processor/magic.rs"]
mod magic_impl;

pub mod magic {
    pub use super::magic_impl::*;
}

pub use magic::*;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp", "avif", "tif", "tiff", "ico",
];

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

pub fn verify_image_data(data: &[u8], filename: &str) -> Result<()> {
    match verify_image_format(data) {
        Ok(format) => {
            tracing::debug!(
                "Verified image format for {}: {} (magic header check passed)",
                filename,
                format.as_str()
            );
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                "File {} has image extension but failed magic header verification: {}",
                filename,
                e
            );
            Err(CbxError::Image(format!(
                "File '{}' appears to have wrong extension (not a valid image)",
                filename
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_image_file, verify_image_data};

    #[test]
    fn verify_image_data_accepts_jpeg_signature() {
        let jpeg_data = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        assert!(verify_image_data(jpeg_data, "test.jpg").is_ok());
    }

    #[test]
    fn verify_image_data_rejects_non_image_bytes() {
        assert!(verify_image_data(b"not-an-image", "fake.jpg").is_err());
    }

    #[test]
    fn is_image_file_matches_supported_extensions() {
        assert!(is_image_file("cover.avif"));
        assert!(is_image_file("page01.webp"));
        assert!(is_image_file("page01.JPG"));
        assert!(!is_image_file("notes.txt"));
    }
}
