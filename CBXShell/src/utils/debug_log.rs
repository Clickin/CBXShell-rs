//! Debug logging utility for diagnosing thumbnail extraction issues
//!
//! Provides file-based logging that persists across DLL loads/unloads
//! to help diagnose why Windows Explorer may not be showing thumbnails.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const DEBUG_LOG_FILENAME: &str = "cbxshell_debug.log";

/// Global mutex to serialize log writes
static LOG_MUTEX: Mutex<()> = Mutex::new(());
static DEBUG_ENABLED: OnceLock<bool> = OnceLock::new();

fn debug_enabled_from_env(get: impl Fn(&str) -> Option<String>) -> bool {
    get("CBXSHELL_DEBUG").as_deref() == Some("1") || get("CBXSHELL_DEBUG_LOG_PATH").is_some()
}

fn debug_enabled() -> bool {
    *DEBUG_ENABLED.get_or_init(|| debug_enabled_from_env(|name| std::env::var(name).ok()))
}

fn debug_log_path() -> PathBuf {
    if let Some(custom_path) = std::env::var_os("CBXSHELL_DEBUG_LOG_PATH") {
        return PathBuf::from(custom_path);
    }

    std::env::temp_dir().join(DEBUG_LOG_FILENAME)
}

fn write_line(path: &Path, msg: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| {
            use std::time::SystemTime;

            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            writeln!(f, "[{}] {}", timestamp, msg)
        });
}

/// Log a debug message to file with timestamp
///
/// This function is safe to call from any thread and will serialize writes.
/// Errors are silently ignored to prevent logging from breaking functionality.
pub fn debug_log(msg: &str) {
    if !debug_enabled() {
        return;
    }

    let Ok(_guard) = LOG_MUTEX.lock() else {
        return;
    };

    write_line(&debug_log_path(), msg);
}

/// Log method entry with parameters
#[macro_export]
macro_rules! log_entry {
    ($method:expr) => {
        $crate::utils::debug_log::debug_log(&format!("[ENTRY] {}", $method));
    };
    ($method:expr, $($arg:tt)*) => {
        $crate::utils::debug_log::debug_log(&format!("[ENTRY] {} - {}", $method, format!($($arg)*)));
    };
}

/// Log method success with result
#[macro_export]
macro_rules! log_success {
    ($method:expr) => {
        $crate::utils::debug_log::debug_log(&format!("[SUCCESS] {}", $method));
    };
    ($method:expr, $($arg:tt)*) => {
        $crate::utils::debug_log::debug_log(&format!("[SUCCESS] {} - {}", $method, format!($($arg)*)));
    };
}

/// Log method failure with error
#[macro_export]
macro_rules! log_error {
    ($method:expr, $error:expr) => {
        $crate::utils::debug_log::debug_log(&format!("[ERROR] {} - {}", $method, $error));
    };
}

/// Clear the debug log file (useful for testing)
#[allow(dead_code)] // Utility function for debugging and testing
pub fn clear_debug_log() {
    if debug_enabled() {
        let _ = std::fs::remove_file(debug_log_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    static TEST_LOG_MUTEX: Mutex<()> = Mutex::new(());
    static TEST_PATH_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_path(name: &str) -> PathBuf {
        let nonce = TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "cbxshell_debug_{}_{}_{}",
            std::process::id(),
            name,
            nonce
        ))
    }

    #[test]
    fn test_debug_gate_requires_explicit_opt_in() {
        assert!(debug_enabled_from_env(|name| match name {
            "CBXSHELL_DEBUG" => Some("1".to_string()),
            _ => None,
        }));
        assert!(!debug_enabled_from_env(|_| None));
        assert!(!debug_enabled_from_env(|name| match name {
            "CBXSHELL_DEBUG" => Some("0".to_string()),
            _ => None,
        }));
    }

    #[test]
    fn test_debug_gate_path_implies_opt_in() {
        assert!(debug_enabled_from_env(|name| match name {
            "CBXSHELL_DEBUG" => Some("0".to_string()),
            "CBXSHELL_DEBUG_LOG_PATH" => Some("custom.log".to_string()),
            _ => None,
        }));
    }

    #[test]
    fn test_debug_log_basic() {
        let _guard = TEST_LOG_MUTEX.lock().unwrap();
        let path = test_path("basic");
        write_line(&path, "Test message");

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Test message"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_debug_log_concurrent() {
        use std::thread;

        let _guard = TEST_LOG_MUTEX.lock().unwrap();
        let path = test_path("concurrent");
        let path_for_threads = path.clone();

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let path = path_for_threads.clone();
                thread::spawn(move || {
                    let _guard = LOG_MUTEX.lock().unwrap();
                    write_line(&path, &format!("Thread {} message", i));
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let matching_lines = contents
            .lines()
            .filter(|line| line.contains("Thread") && line.contains("message"))
            .count();
        assert_eq!(matching_lines, 10);
        let _ = std::fs::remove_file(path);
    }
}
