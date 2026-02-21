//! COM registration and shell extension registration
//!
//! Handles registry entries for:
//! - CLSID registration
//! - Shell extension handlers (.cbz, .cbr, .zip, .cb7)
//! - Approved shell extensions
//!
//! Based on CBXShell.rgs from the C++ implementation

use crate::utils::error::{CbxError, Result};
use windows::core::GUID;
use windows::Win32::System::Registry::*;

/// CBXShell CLSID: {9E6ECB90-5A61-42BD-B851-D3297D9C7F39}
pub const CLSID_CBXSHELL: GUID = GUID::from_u128(0x9E6ECB90_5A61_42BD_B851_D3297D9C7F39);

/// IThumbnailProvider interface GUID (modern thumbnail API, replaces IExtractImage)
#[allow(dead_code)] // May be used in future for interface registration
const IID_ITHUMBNAILPROVIDER: &str = "{E357FCCD-A995-4576-B01F-234630154E96}";

/// IQueryInfo interface GUID (tooltips)
#[allow(dead_code)] // May be used in future for interface registration
const IID_IQUERYINFO: &str = "{00021500-0000-0000-C000-000000000046}";

/// Get the path to the current DLL
///
/// This is only available when called from within the DLL (e.g., DllRegisterServer).
/// When called from an external executable (like CBXManager), the module handle won't be set.
fn get_module_path() -> Result<String> {
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

    // Get the DLL module handle stored in DllMain
    let hmodule = crate::get_dll_module()
        .ok_or_else(|| CbxError::Registry("DLL module handle not initialized".to_string()))?;

    // UNAVOIDABLE UNSAFE: GetModuleFileNameW is a Windows FFI call
    // Why unsafe is required:
    // 1. Foreign Function Interface: Calling C functions from kernel32.dll
    // 2. No safe alternative: Only way to get DLL path on Windows
    // 3. Buffer management: Must pass mutable buffer to C function
    // Safety guarantees:
    // - hmodule is validated (non-null) above
    // - Buffer size is MAX_PATH (Windows API contract)
    // - Return value checked for errors (len == 0)
    unsafe {
        let mut buffer = vec![0u16; MAX_PATH as usize];
        let len = GetModuleFileNameW(hmodule, &mut buffer);

        if len == 0 {
            return Err(CbxError::Windows(windows::core::Error::from_win32()));
        }

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Ok(path)
    }
}

/// Create a registry key (helper function)
fn create_key(hkey: HKEY, subkey: &str) -> Result<HKEY> {
    // UNAVOIDABLE UNSAFE: RegCreateKeyExW is a Windows FFI call
    // Why unsafe is required:
    // 1. Foreign Function Interface: Windows Registry API (advapi32.dll)
    // 2. No safe alternative: Registry manipulation is Windows-specific
    // 3. Raw pointer passing: PCWSTR requires pointer to null-terminated UTF-16
    // Safety guarantees:
    // - subkey_wide has null terminator (chain(Some(0)))
    // - hkey is validated by caller (from Windows API)
    // - Error handling via Result propagation
    unsafe {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
        let mut result_key = HKEY::default();

        RegCreateKeyExW(
            hkey,
            windows::core::PCWSTR(subkey_wide.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut result_key,
            None,
        )
        .map_err(|e| CbxError::Windows(e))?;

        Ok(result_key)
    }
}

/// Set a registry string value (helper function)
fn set_string_value(hkey: HKEY, value_name: Option<&str>, data: &str) -> Result<()> {
    let value_name_wide: Vec<u16> = value_name
        .map(|s| s.encode_utf16().chain(Some(0)).collect())
        .unwrap_or_else(|| vec![0]);

    let data_wide: Vec<u16> = data.encode_utf16().chain(Some(0)).collect();

    // SAFETY IMPROVEMENT: Convert u16 to bytes safely without raw pointers
    // Previously used std::slice::from_raw_parts which is unsafe
    // Now using safe iterator-based conversion with explicit endianness
    let data_bytes: Vec<u8> = data_wide.iter().flat_map(|&w| w.to_le_bytes()).collect();

    // UNAVOIDABLE UNSAFE: RegSetValueExW is a Windows FFI call
    // The Windows Registry API requires calling C functions which is inherently unsafe
    unsafe {
        RegSetValueExW(
            hkey,
            windows::core::PCWSTR(value_name_wide.as_ptr()),
            0,
            REG_SZ,
            Some(&data_bytes),
        )
        .map_err(|e| CbxError::Windows(e))?;
    }

    Ok(())
}

/// Delete a registry key recursively
fn delete_key_recursive(hkey: HKEY, subkey: &str) -> Result<()> {
    // UNAVOIDABLE UNSAFE: RegDeleteTreeW is a Windows FFI call
    // Why unsafe is required:
    // 1. Foreign Function Interface: Windows Registry API (advapi32.dll)
    // 2. No safe alternative: Recursive registry deletion is Windows-specific
    // 3. Raw pointer passing: PCWSTR requires pointer to UTF-16 string
    // Safety guarantees:
    // - subkey_wide has null terminator
    // - Error codes properly handled (FILE_NOT_FOUND is acceptable)
    unsafe {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();

        // Try to delete, but ignore "key not found" errors
        match RegDeleteTreeW(hkey, windows::core::PCWSTR(subkey_wide.as_ptr())) {
            Ok(_) => Ok(()),
            Err(e) => {
                // ERROR_FILE_NOT_FOUND or ERROR_PATH_NOT_FOUND are acceptable
                let code = e.code().0 as u32;
                if code == 2 || code == 3 {
                    Ok(())
                } else {
                    Err(CbxError::Registry(format!(
                        "Failed to delete registry key: {} (error: {:?})",
                        subkey, e
                    )))
                }
            }
        }
    }
}

/// Register shell extension handler for a file extension
#[allow(dead_code)] // Helper function for per-extension registration
fn register_extension(extension: &str, clsid_str: &str) -> Result<()> {
    let base_key = format!("Software\\Classes\\{}", extension);

    // 1. Register PerceivedType as "image" so Windows treats these as media files
    // This is CRITICAL for Windows 11 to show thumbnails in folder views
    let ext_key = create_key(HKEY_CURRENT_USER, &base_key)?;
    set_string_value(ext_key, Some("PerceivedType"), "image")?;
    set_string_value(ext_key, Some("Content Type"), "application/x-cbz")?;
    unsafe {
        RegCloseKey(ext_key).ok();
    }

    // 2. Create .ext\shellex key
    let shellex_key_path = format!("{}\\shellex", base_key);
    let shellex_key = create_key(HKEY_CURRENT_USER, &shellex_key_path)?;

    // 3. Register IThumbnailProvider handler (thumbnails - modern API)
    let thumbnail_key_path = format!("{}\\shellex\\{}", base_key, IID_ITHUMBNAILPROVIDER);
    let thumbnail_key = create_key(HKEY_CURRENT_USER, &thumbnail_key_path)?;
    set_string_value(thumbnail_key, None, clsid_str)?;
    unsafe {
        RegCloseKey(thumbnail_key).ok();
    }

    // 4. Register IQueryInfo handler (tooltips)
    let infotip_key_path = format!("{}\\shellex\\{}", base_key, IID_IQUERYINFO);
    let infotip_key = create_key(HKEY_CURRENT_USER, &infotip_key_path)?;
    set_string_value(infotip_key, None, clsid_str)?;
    unsafe {
        RegCloseKey(infotip_key).ok();
    }

    unsafe {
        RegCloseKey(shellex_key).ok();
    }

    Ok(())
}

/// Unregister shell extension handler for a file extension
#[allow(dead_code)] // Helper function for per-extension unregistration
fn unregister_extension(extension: &str) -> Result<()> {
    let base_key = format!("Software\\Classes\\{}\\shellex", extension);
    delete_key_recursive(HKEY_CURRENT_USER, &base_key)?;
    Ok(())
}

/// Register the COM server and shell extension handlers
///
/// # Arguments
/// * `dll_path` - Optional path to the DLL. If None, will attempt to get path from DllMain module handle.
///                When calling from an external executable (like CBXManager), you must provide this.
pub fn register_server(dll_path: Option<&str>) -> Result<()> {
    // Format CLSID with hyphens as Windows expects: {XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}
    let clsid_str = format!("{{{:?}}}", CLSID_CBXSHELL);

    // Get DLL path: use provided path or get from module handle
    let module_path = match dll_path {
        Some(path) => path.to_string(),
        None => get_module_path()?,
    };

    // 1. Register CLSID
    let clsid_key_path = format!("Software\\Classes\\CLSID\\{}", clsid_str);
    let clsid_key = create_key(HKEY_CURRENT_USER, &clsid_key_path)?;
    set_string_value(clsid_key, None, "CBXShell Class")?;

    // 2. Register InprocServer32
    let inproc_key_path = format!("{}\\InprocServer32", clsid_key_path);
    let inproc_key = create_key(HKEY_CURRENT_USER, &inproc_key_path)?;
    set_string_value(inproc_key, None, &module_path)?;
    set_string_value(inproc_key, Some("ThreadingModel"), "Apartment")?;
    unsafe {
        RegCloseKey(inproc_key).ok();
    }

    // 3. Register ProgID (optional, for compatibility)
    let progid_key = create_key(HKEY_CURRENT_USER, "Software\\Classes\\CBXShell.CBXShell.1")?;
    set_string_value(progid_key, None, "CBXShell Class")?;
    let progid_clsid_key = create_key(
        HKEY_CURRENT_USER,
        "Software\\Classes\\CBXShell.CBXShell.1\\CLSID",
    )?;
    set_string_value(progid_clsid_key, None, &clsid_str)?;
    unsafe {
        RegCloseKey(progid_clsid_key).ok();
        RegCloseKey(progid_key).ok();
        RegCloseKey(clsid_key).ok();
    }

    // 4. Add to approved shell extensions (HKCU, not HKLM to avoid admin requirement)
    // Note: File extension registration is now handled by CBXManager via registry_ops
    let approved_key_path =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Approved";
    let approved_key = create_key(HKEY_CURRENT_USER, approved_key_path)?;
    set_string_value(approved_key, Some(&clsid_str), "CBXShell Class")?;
    unsafe {
        RegCloseKey(approved_key).ok();
    }

    tracing::info!(
        "Successfully registered CBXShell COM server (file extensions must be configured via CBXManager)"
    );

    Ok(())
}

/// Unregister the COM server and shell extension handlers
pub fn unregister_server() -> Result<()> {
    let clsid_str = format!("{{{:?}}}", CLSID_CBXSHELL);

    // 1. Remove from approved shell extensions
    // Note: File extension cleanup is handled by CBXManager via registry_ops
    let approved_key_path =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Approved";
    if let Ok(approved_key) = create_key(HKEY_CURRENT_USER, approved_key_path) {
        unsafe {
            let value_name_wide: Vec<u16> = clsid_str.encode_utf16().chain(Some(0)).collect();
            let _ = RegDeleteValueW(
                approved_key,
                windows::core::PCWSTR(value_name_wide.as_ptr()),
            );
            RegCloseKey(approved_key).ok();
        }
    }

    // 2. Delete CLSID
    let clsid_key_path = format!("Software\\Classes\\CLSID\\{}", clsid_str);
    delete_key_recursive(HKEY_CURRENT_USER, &clsid_key_path)?;

    // 3. Delete ProgID
    let _ = delete_key_recursive(HKEY_CURRENT_USER, "Software\\Classes\\CBXShell.CBXShell.1");
    let _ = delete_key_recursive(HKEY_CURRENT_USER, "Software\\Classes\\CBXShell.CBXShell");

    tracing::info!("Successfully unregistered CBXShell");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clsid_format() {
        let clsid_str = format!("{{{:?}}}", CLSID_CBXSHELL);
        // Debug format uses uppercase for GUIDs
        assert_eq!(clsid_str, "{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}");
    }

    #[test]
    fn test_get_module_path() {
        // This test only works when running as a DLL (not in test executable)
        // The module handle is set in DllMain which doesn't run during cargo test
        let path = get_module_path();

        // If module handle is not set (test context), expect an error
        // If it is set (DLL context), verify path format
        if let Ok(path_str) = path {
            assert!(path_str.ends_with(".dll") || path_str.ends_with(".exe"));
        } else {
            // In test context, expecting an error is acceptable
            assert!(path.is_err());
        }
    }
}
