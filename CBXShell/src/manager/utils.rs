///! Utility functions for CBXManager
///!
///! Helper functions for Explorer restart and other operations
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
};

/// Prompt user to restart Explorer to apply changes
pub fn prompt_restart_explorer() -> bool {
    let title = "Restart Explorer?\0".encode_utf16().collect::<Vec<_>>();
    let message = "Changes have been saved. Would you like to restart Windows Explorer now to apply them?\n\n(You can also restart manually later)\0"
        .encode_utf16()
        .collect::<Vec<_>>();

    // UNAVOIDABLE UNSAFE: MessageBoxW is a Windows UI FFI call
    // Why unsafe is required:
    // 1. FFI call to user32.dll (Windows User Interface API)
    // 2. No safe alternative: Native modal dialogs are Windows-specific
    // 3. Raw pointer to UTF-16 string required by Windows API
    //
    // Safety guarantees:
    // - Strings are null-terminated (\0)
    // - Pointers are valid (from Vec we own)
    // - MessageBoxW doesn't modify the strings
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(message.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

/// Restart Windows Explorer process
pub fn restart_explorer() -> anyhow::Result<()> {
    use std::process::Command;

    // Kill explorer.exe
    Command::new("taskkill")
        .args(&["/f", "/im", "explorer.exe"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to kill Explorer: {}", e))?;

    // Wait a moment
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Restart explorer.exe
    Command::new("explorer.exe")
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start Explorer: {}", e))?;

    Ok(())
}

/// Show success message
#[allow(dead_code)]
pub fn show_success(title: &str, message: &str) {
    let title_wide = format!("{}\0", title).encode_utf16().collect::<Vec<_>>();
    let message_wide = format!("{}\0", message).encode_utf16().collect::<Vec<_>>();

    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(message_wide.as_ptr()),
            windows::core::PCWSTR(title_wide.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Show error message
#[allow(dead_code)]
pub fn show_error(title: &str, message: &str) {
    let title_wide = format!("{}\0", title).encode_utf16().collect::<Vec<_>>();
    let message_wide = format!("{}\0", message).encode_utf16().collect::<Vec<_>>();

    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(message_wide.as_ptr()),
            windows::core::PCWSTR(title_wide.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}
