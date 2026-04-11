///! Utility functions for CBXManager
///!
///! Helper functions for Explorer restart and other operations
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
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

pub fn spawn_update_check_now() -> anyhow::Result<()> {
    let exe_path = std::env::current_exe()?;
    std::process::Command::new(exe_path)
        .arg("--check-updates-now")
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch update check: {}", e))?;
    Ok(())
}

/// Open a URL in the default browser
pub fn open_url(url: &str) -> anyhow::Result<()> {
    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = format!("{}\0", url).encode_utf16().collect::<Vec<_>>();

    let result = unsafe {
        ShellExecuteW(
            HWND(0),
            windows::core::PCWSTR(operation.as_ptr()),
            windows::core::PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };

    if result.0 as isize <= 32 {
        return Err(anyhow::anyhow!("Failed to open URL: {}", url));
    }

    Ok(())
}

/// Prompt the user to open a release page
pub fn prompt_open_release(current_version: &str, latest_version: &str) -> bool {
    let title = "Update Available\0".encode_utf16().collect::<Vec<_>>();
    let message = format!(
        "A newer version of CBXShell-rs is available.\n\nCurrent version: {}\nLatest version: {}\n\nOpen the GitHub Release page now?",
        current_version, latest_version
    )
    .replace('\n', "\r\n");
    let message = format!("{}\0", message)
        .encode_utf16()
        .collect::<Vec<_>>();

    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(message.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONINFORMATION,
        ) == IDYES
    }
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
