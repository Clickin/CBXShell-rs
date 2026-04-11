///! Shared utilities for archive processing
///!
///! Provides image detection, natural sorting, and common helpers
use std::path::Path;

/// Maximum uncompressed size for a single entry (32MB)
/// This matches the C++ implementation's CBXMEM_MAXBUFFER_SIZE
pub const MAX_ENTRY_SIZE: u64 = 32 * 1024 * 1024;

/// Supported image extensions
/// Includes modern formats (WebP, AVIF) for Phase 3
const IMAGE_EXTENSIONS: &[&str] = &[
    "bmp", "ico", "gif", "jpg", "jpe", "jfif", "jpeg", "png", "tif", "tiff",
    "webp", // Phase 3
    "avif", // Phase 3
];

/// Check if filename is an image based on extension
pub fn is_image_file(name: &str) -> bool {
    if let Some(ext) = Path::new(name).extension().and_then(|s| s.to_str()) {
        IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    } else {
        false
    }
}

/// Natural sort comparison using natord (matches Windows StrCmpLogicalW)
pub fn natural_sort_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natord::compare(a, b)
}

/// Find first image entry from a list, optionally sorted
///
/// If `sort` is true, returns alphabetically first image (natural order).
/// If `sort` is false, returns first image encountered (early exit optimization).
pub fn find_first_image<'a>(names: impl Iterator<Item = &'a str>, sort: bool) -> Option<String> {
    let mut images: Vec<&str> = names.filter(|name| is_image_file(name)).collect();

    if images.is_empty() {
        return None;
    }

    if sort {
        images.sort_by(|a, b| natural_sort_cmp(a, b));
    }

    images.first().map(|s| (*s).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_file() {
        // Supported formats
        assert!(is_image_file("test.jpg"));
        assert!(is_image_file("TEST.PNG"));
        assert!(is_image_file("image.webp"));
        assert!(is_image_file("photo.JPEG"));
        assert!(is_image_file("icon.ico"));
        assert!(is_image_file("graphic.bmp"));
        assert!(is_image_file("scan.tiff"));

        // Unsupported formats
        assert!(!is_image_file("readme.txt"));
        assert!(!is_image_file("archive.zip"));
        assert!(!is_image_file("video.mp4"));
        assert!(!is_image_file("noextension"));
    }

    #[test]
    fn test_natural_sort_cmp() {
        use std::cmp::Ordering;

        // Natural ordering: 1 < 2 < 10
        assert_eq!(natural_sort_cmp("page1.jpg", "page2.jpg"), Ordering::Less);
        assert_eq!(natural_sort_cmp("page2.jpg", "page10.jpg"), Ordering::Less);
        assert_eq!(
            natural_sort_cmp("page10.jpg", "page2.jpg"),
            Ordering::Greater
        );
        assert_eq!(natural_sort_cmp("page1.jpg", "page1.jpg"), Ordering::Equal);

        // Alphabetic fallback
        assert_eq!(natural_sort_cmp("apple.jpg", "banana.jpg"), Ordering::Less);
    }

    #[test]
    fn test_find_first_image_sorted() {
        let files = vec!["readme.txt", "page10.jpg", "page2.jpg", "page1.jpg"];
        let result = find_first_image(files.iter().copied(), true);
        assert_eq!(result, Some("page1.jpg".to_string()));
    }

    #[test]
    fn test_find_first_image_unsorted() {
        let files = vec!["readme.txt", "page10.jpg", "page2.jpg"];
        let result = find_first_image(files.iter().copied(), false);
        // Should return first encountered image
        assert_eq!(result, Some("page10.jpg".to_string()));
    }

    #[test]
    fn test_find_first_image_no_images() {
        let files = vec!["readme.txt", "license.md", "notes.doc"];
        let result = find_first_image(files.iter().copied(), true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_first_image_empty() {
        let files: Vec<&str> = vec![];
        let result = find_first_image(files.iter().copied(), true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_max_entry_size() {
        assert_eq!(MAX_ENTRY_SIZE, 33_554_432);
        assert_eq!(MAX_ENTRY_SIZE, 32 * 1024 * 1024);
    }
}
