use crate::archive::ArchiveType;
use crate::utils::error::{CbxError, Result};
///! File system utility functions
use std::path::Path;
use widestring::U16CString;
use windows::core::PCWSTR;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileTime, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
};

/// Get the last modified time of a file
///
/// # Arguments
/// * `path` - Path to the file
///
/// # Returns
/// * `Ok(FILETIME)` - File modification time
/// * `Err(CbxError)` - Failed to retrieve time
///
/// # Windows API
/// Uses GetFileTime to retrieve the file's last write time.
/// This matches the C++ behavior for thumbnail cache validation.
#[allow(dead_code)] // Part of public API, may be used in future
pub fn get_file_modified_time(path: &Path) -> Result<FILETIME> {
    // UNAVOIDABLE UNSAFE: Windows File API operations
    // Why unsafe is required:
    // 1. CreateFileW, GetFileTime, CloseHandle are Windows FFI (kernel32.dll)
    // 2. No safe alternative: FILETIME structure is Windows-specific
    // 3. Handle management: Must explicitly close file handle
    //
    // Why std::fs::metadata is not sufficient:
    // - Rust's metadata returns SystemTime, not FILETIME
    // - Windows Shell Extensions require exact FILETIME format
    // - No safe conversion between SystemTime and FILETIME
    //
    // Safety guarantees:
    // - Path validated and converted to wide string
    // - Handle validity checked (INVALID_HANDLE_VALUE)
    // - CloseHandle called in all code paths
    // - Error propagation via Result
    unsafe {
        // Convert path to wide string for Windows API
        let wide_path =
            U16CString::from_os_str(path.as_os_str()).map_err(|_| CbxError::InvalidPath)?;

        // Open file handle (read-only, shared read access)
        let handle = CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            0, // No access needed, just getting metadata
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| CbxError::Windows(e))?;

        if handle == INVALID_HANDLE_VALUE {
            return Err(CbxError::Io(std::io::Error::last_os_error()));
        }

        // Get file times
        let mut creation_time = FILETIME::default();
        let mut last_access_time = FILETIME::default();
        let mut last_write_time = FILETIME::default();

        let result = GetFileTime(
            handle,
            Some(&mut creation_time),
            Some(&mut last_access_time),
            Some(&mut last_write_time),
        );

        // Always close the handle
        CloseHandle(handle).ok();

        if result.is_err() {
            return Err(CbxError::Windows(result.unwrap_err()));
        }

        Ok(last_write_time)
    }
}

/// Detect archive type from file extension
///
/// # Arguments
/// * `path` - Path to the archive file
///
/// # Returns
/// * `Ok(ArchiveType)` - Detected archive type
/// * `Err(CbxError)` - Unsupported or invalid extension
///
/// # Supported Extensions
/// - ZIP: .zip, .cbz, .epub, .phz
/// - RAR: .rar, .cbr
/// - 7-Zip: .7z, .cb7
#[allow(dead_code)] // Part of public API, may be used in future
pub fn detect_archive_type(path: &Path) -> Result<ArchiveType> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or(CbxError::InvalidPath)?;

    ArchiveType::from_extension(extension)
        .ok_or_else(|| CbxError::UnsupportedFormat(extension.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_get_file_modified_time() {
        // Create a temporary file
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"test content").unwrap();
        file.sync_all().unwrap();
        drop(file);

        // Get modified time
        let result = get_file_modified_time(&file_path);
        assert!(
            result.is_ok(),
            "Failed to get file time: {:?}",
            result.err()
        );

        let filetime = result.unwrap();
        // FILETIME should be non-zero for a real file
        assert!(filetime.dwLowDateTime != 0 || filetime.dwHighDateTime != 0);
    }

    #[test]
    fn test_get_file_modified_time_nonexistent() {
        let path = Path::new("G:\\nonexistent_file_12345.txt");
        let result = get_file_modified_time(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_archive_type_zip() {
        assert_eq!(
            detect_archive_type(Path::new("test.zip")).unwrap(),
            ArchiveType::Zip
        );
        assert_eq!(
            detect_archive_type(Path::new("comic.cbz")).unwrap(),
            ArchiveType::Zip
        );
        assert_eq!(
            detect_archive_type(Path::new("book.epub")).unwrap(),
            ArchiveType::Zip
        );
        assert_eq!(
            detect_archive_type(Path::new("archive.phz")).unwrap(),
            ArchiveType::Zip
        );
    }

    #[test]
    fn test_detect_archive_type_rar() {
        assert_eq!(
            detect_archive_type(Path::new("test.rar")).unwrap(),
            ArchiveType::Rar
        );
        assert_eq!(
            detect_archive_type(Path::new("comic.cbr")).unwrap(),
            ArchiveType::Rar
        );
    }

    #[test]
    fn test_detect_archive_type_7z() {
        assert_eq!(
            detect_archive_type(Path::new("test.7z")).unwrap(),
            ArchiveType::SevenZip
        );
        assert_eq!(
            detect_archive_type(Path::new("comic.cb7")).unwrap(),
            ArchiveType::SevenZip
        );
    }

    #[test]
    fn test_detect_archive_type_case_insensitive() {
        assert_eq!(
            detect_archive_type(Path::new("test.ZIP")).unwrap(),
            ArchiveType::Zip
        );
        assert_eq!(
            detect_archive_type(Path::new("test.Cbz")).unwrap(),
            ArchiveType::Zip
        );
    }

    #[test]
    fn test_detect_archive_type_unsupported() {
        let result = detect_archive_type(Path::new("test.txt"));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CbxError::UnsupportedFormat(_)
        ));
    }

    #[test]
    fn test_detect_archive_type_no_extension() {
        let result = detect_archive_type(Path::new("test"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CbxError::InvalidPath));
    }

    #[test]
    fn test_detect_archive_type_with_path() {
        assert_eq!(
            detect_archive_type(Path::new("G:\\comics\\issue1.cbz")).unwrap(),
            ArchiveType::Zip
        );
        assert_eq!(
            detect_archive_type(Path::new("C:\\temp\\archive.7z")).unwrap(),
            ArchiveType::SevenZip
        );
    }
}
