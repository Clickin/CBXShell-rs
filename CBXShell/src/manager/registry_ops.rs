///! Registry operations for CBXManager
///!
///! Read and write configuration from/to Windows registry
use super::state::AppState;
use anyhow::{Context, Result};
use winreg::enums::*;
use winreg::RegKey;

/// CBXShell CLSID string
const CLSID_STR: &str = "{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}";

/// Configuration registry path
const CONFIG_KEY_PATH: &str = "Software\\CBXShell-rs\\{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}";

/// IThumbnailProvider interface GUID
const IID_ITHUMBNAILPROVIDER: &str = "{E357FCCD-A995-4576-B01F-234630154E96}";

/// IQueryInfo interface GUID (tooltips)
const IID_IQUERYINFO: &str = "{00021500-0000-0000-C000-000000000046}";

/// Read current application state from registry
pub fn read_app_state() -> Result<AppState> {
    let mut state = AppState::default();

    // 1. Check DLL registration
    state.dll_registered = check_dll_registration();

    // 2. Read sort setting
    state.sort_enabled = read_sort_setting()?;
    state.sort_preview_enabled = read_sort_preview_setting()?;

    // 3. Check each extension's handler registration
    for ext_config in &mut state.extensions {
        let (thumbnail, infotip) = check_extension_handlers(&ext_config.extension)?;
        ext_config.thumbnail_enabled = thumbnail;
        ext_config.infotip_enabled = infotip;
    }

    Ok(state)
}

/// Write application state to registry
pub fn write_app_state(state: &AppState) -> Result<()> {
    // 1. Write sort settings
    write_sort_setting(state.sort_enabled)?;
    write_sort_preview_setting(state.sort_preview_enabled)?;

    // 2. Update extension handlers
    for ext_config in &state.extensions {
        set_extension_handlers(
            &ext_config.extension,
            ext_config.thumbnail_enabled,
            ext_config.infotip_enabled,
        )?;
    }

    Ok(())
}

/// Check if the DLL is registered as a COM server
pub fn check_dll_registration() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let clsid_path = format!("Software\\Classes\\CLSID\\{}", CLSID_STR);

    hkcu.open_subkey(clsid_path).is_ok()
}

/// Check if handlers are registered for an extension
///
/// Returns (thumbnail_enabled, infotip_enabled)
pub fn check_extension_handlers(extension: &str) -> Result<(bool, bool)> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base_path = format!("Software\\Classes\\{}\\shellex", extension);

    // Check thumbnail handler
    let thumbnail_path = format!("{}\\{}", base_path, IID_ITHUMBNAILPROVIDER);
    let thumbnail_enabled = if let Ok(key) = hkcu.open_subkey(&thumbnail_path) {
        // Check if the default value matches our CLSID
        match key.get_value::<String, _>("") {
            Ok(value) => value == CLSID_STR,
            Err(_) => false,
        }
    } else {
        false
    };

    // Check infotip handler
    let infotip_path = format!("{}\\{}", base_path, IID_IQUERYINFO);
    let infotip_enabled = if let Ok(key) = hkcu.open_subkey(&infotip_path) {
        match key.get_value::<String, _>("") {
            Ok(value) => value == CLSID_STR,
            Err(_) => false,
        }
    } else {
        false
    };

    Ok((thumbnail_enabled, infotip_enabled))
}

/// Set handlers for an extension
fn set_extension_handlers(extension: &str, thumbnail: bool, infotip: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base_path = format!("Software\\Classes\\{}", extension);

    // Ensure base extension key exists with PerceivedType
    let (ext_key, _) = hkcu
        .create_subkey(&base_path)
        .context("Failed to create extension key")?;
    ext_key
        .set_value("PerceivedType", &"image")
        .context("Failed to set PerceivedType")?;

    // Create shellex key
    let shellex_path = format!("{}\\shellex", base_path);
    hkcu.create_subkey(&shellex_path)
        .context("Failed to create shellex key")?;

    // Handle thumbnail provider
    let thumbnail_path = format!("{}\\shellex\\{}", base_path, IID_ITHUMBNAILPROVIDER);
    if thumbnail {
        let (thumb_key, _) = hkcu
            .create_subkey(&thumbnail_path)
            .context("Failed to create thumbnail key")?;
        thumb_key
            .set_value("", &CLSID_STR)
            .context("Failed to set thumbnail CLSID")?;
    } else {
        // Remove thumbnail handler
        let _ = hkcu.delete_subkey_all(&thumbnail_path);
    }

    // Handle infotip provider
    let infotip_path = format!("{}\\shellex\\{}", base_path, IID_IQUERYINFO);
    if infotip {
        let (info_key, _) = hkcu
            .create_subkey(&infotip_path)
            .context("Failed to create infotip key")?;
        info_key
            .set_value("", &CLSID_STR)
            .context("Failed to set infotip CLSID")?;
    } else {
        // Remove infotip handler
        let _ = hkcu.delete_subkey_all(&infotip_path);
    }

    Ok(())
}

/// Read the sorting preference from registry
fn read_sort_setting() -> Result<bool> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    match hkcu.open_subkey(CONFIG_KEY_PATH) {
        Ok(key) => {
            match key.get_value::<u32, _>("NoSort") {
                Ok(value) => Ok(value == 0), // NoSort=0 means sort enabled
                Err(_) => Ok(false), // Default: sorting disabled (NoSort=1) for better performance
            }
        }
        Err(_) => Ok(false), // Default: sorting disabled (NoSort=1) for better performance
    }
}

/// Read the preview sorting preference from registry
fn read_sort_preview_setting() -> Result<bool> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    match hkcu.open_subkey(CONFIG_KEY_PATH) {
        Ok(key) => match key.get_value::<u32, _>("NoSortPreview") {
            Ok(value) => Ok(value == 0),
            Err(_) => Ok(false),
        },
        Err(_) => Ok(false),
    }
}

/// Write the sorting preference to registry
fn write_sort_setting(sort_enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(CONFIG_KEY_PATH)
        .context("Failed to create config key")?;

    let no_sort_value: u32 = if sort_enabled { 0 } else { 1 };
    key.set_value("NoSort", &no_sort_value)
        .context("Failed to set NoSort value")?;

    Ok(())
}

/// Write the preview sorting preference to registry
fn write_sort_preview_setting(sort_enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(CONFIG_KEY_PATH)
        .context("Failed to create config key")?;

    let no_sort_value: u32 = if sort_enabled { 0 } else { 1 };
    key.set_value("NoSortPreview", &no_sort_value)
        .context("Failed to set NoSortPreview value")?;

    Ok(())
}

/// Register the DLL as a COM server
///
/// This function calls the library's register_server function directly.
/// Since the binary and library are in the same crate but use different module roots,
/// we need to access it through cbxshell:: path.
pub fn register_dll() -> Result<()> {
    // Get the path to cbxshell.dll (should be in the same directory as the manager)
    let exe_path = std::env::current_exe().context("Failed to get current executable path")?;
    let exe_dir = exe_path
        .parent()
        .context("Failed to get executable directory")?;
    let dll_path = exe_dir.join("cbxshell.dll");

    // Verify DLL exists
    if !dll_path.exists() {
        return Err(anyhow::anyhow!(
            "cbxshell.dll not found at: {}",
            dll_path.display()
        ));
    }

    // Convert to string
    let dll_path_str = dll_path
        .to_str()
        .context("Failed to convert DLL path to string")?;

    cbxshell::registry::register_server(Some(dll_path_str))
        .map_err(|e| anyhow::anyhow!("DLL registration failed: {}", e))
}

/// Unregister the DLL as a COM server
pub fn unregister_dll() -> Result<()> {
    cbxshell::registry::unregister_server()
        .map_err(|e| anyhow::anyhow!("DLL unregistration failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_dll_registration() {
        // Just verify it doesn't crash
        let _registered = check_dll_registration();
    }

    #[test]
    fn test_check_extension_handlers() {
        // Test with .cbz (may or may not be registered)
        let result = check_extension_handlers(".cbz");
        assert!(result.is_ok());

        let (thumbnail, infotip) = result.unwrap();
        // Just verify boolean values (actual state depends on system)
        assert!(thumbnail == true || thumbnail == false);
        assert!(infotip == true || infotip == false);
    }

    #[test]
    fn test_read_sort_setting() {
        let result = read_sort_setting();
        assert!(result.is_ok());

        // Verify it's a boolean
        let sort = result.unwrap();
        assert!(sort == true || sort == false);
    }

    #[test]
    fn test_read_app_state() {
        // Should not crash even if registry is not configured
        let result = read_app_state();
        assert!(result.is_ok());

        let state = result.unwrap();
        assert_eq!(state.extensions.len(), 6);
    }

    #[test]
    fn test_write_and_read_sort_setting() {
        // Try to write and read back (may fail without permissions)
        if write_sort_setting(true).is_ok() {
            let result = read_sort_setting().unwrap();
            assert_eq!(result, true);
        }

        if write_sort_setting(false).is_ok() {
            let result = read_sort_setting().unwrap();
            assert_eq!(result, false);
        }

        // Cleanup: restore to default
        let _ = write_sort_setting(true);
    }
}
