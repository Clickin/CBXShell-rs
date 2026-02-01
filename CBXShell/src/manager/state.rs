///! Application state management for CBXManager
///!
///! Defines the configuration state for the CBXShell extension

/// Configuration for a single file extension
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionConfig {
    /// File extension (e.g., ".cbz", ".cbr")
    pub extension: String,
    /// Whether thumbnail handler is enabled for this extension
    pub thumbnail_enabled: bool,
    /// Whether infotip (tooltip) handler is enabled for this extension
    pub infotip_enabled: bool,
}

impl ExtensionConfig {
    /// Create a new extension configuration
    pub fn new(extension: impl Into<String>) -> Self {
        Self {
            extension: extension.into(),
            thumbnail_enabled: false,
            infotip_enabled: false,
        }
    }

    /// Create with both handlers enabled
    #[allow(dead_code)] // Utility function for configuration
    pub fn enabled(extension: impl Into<String>) -> Self {
        Self {
            extension: extension.into(),
            thumbnail_enabled: true,
            infotip_enabled: true,
        }
    }
}

/// Application state for the manager
#[derive(Debug, Clone)]
pub struct AppState {
    /// Supported extensions with their handler states
    pub extensions: Vec<ExtensionConfig>,
    /// Whether alphabetical sorting is enabled (true) or first-found mode (false)
    pub sort_enabled: bool,
    /// Whether preview pages are sorted alphabetically
    pub sort_preview_enabled: bool,
    /// Whether the DLL is registered as a COM server
    pub dll_registered: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            extensions: vec![
                ExtensionConfig::new(".cbz"),
                ExtensionConfig::new(".cbr"),
                ExtensionConfig::new(".zip"),
                ExtensionConfig::new(".rar"),
                ExtensionConfig::new(".7z"),
                ExtensionConfig::new(".cb7"),
            ],
            sort_enabled: false, // Default: sort disabled (NoSort=1) for better performance with large archives
            sort_preview_enabled: false,
            dll_registered: false,
        }
    }
}

impl AppState {
    /// Check if any extension has handlers enabled
    pub fn has_any_handlers_enabled(&self) -> bool {
        self.extensions
            .iter()
            .any(|ext| ext.thumbnail_enabled || ext.infotip_enabled)
    }

    /// Get extension config by extension name
    #[allow(dead_code)] // Part of public API, may be used in future
    pub fn get_extension(&self, extension: &str) -> Option<&ExtensionConfig> {
        self.extensions
            .iter()
            .find(|ext| ext.extension == extension)
    }

    /// Get mutable extension config by extension name
    pub fn get_extension_mut(&mut self, extension: &str) -> Option<&mut ExtensionConfig> {
        self.extensions
            .iter_mut()
            .find(|ext| ext.extension == extension)
    }

    /// Validate state (ensure DLL is registered if handlers are enabled)
    pub fn is_valid(&self) -> bool {
        if self.has_any_handlers_enabled() && !self.dll_registered {
            return false; // Can't have handlers without DLL registration
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_config_new() {
        let config = ExtensionConfig::new(".cbz");
        assert_eq!(config.extension, ".cbz");
        assert!(!config.thumbnail_enabled);
        assert!(!config.infotip_enabled);
    }

    #[test]
    fn test_extension_config_enabled() {
        let config = ExtensionConfig::enabled(".cbr");
        assert_eq!(config.extension, ".cbr");
        assert!(config.thumbnail_enabled);
        assert!(config.infotip_enabled);
    }

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.extensions.len(), 6);
        assert!(!state.sort_enabled); // Default: sort disabled for performance
        assert!(!state.dll_registered);
        assert!(!state.has_any_handlers_enabled());
    }

    #[test]
    fn test_has_any_handlers_enabled() {
        let mut state = AppState::default();
        assert!(!state.has_any_handlers_enabled());

        state.extensions[0].thumbnail_enabled = true;
        assert!(state.has_any_handlers_enabled());
    }

    #[test]
    fn test_get_extension() {
        let state = AppState::default();
        let ext = state.get_extension(".cbz");
        assert!(ext.is_some());
        assert_eq!(ext.unwrap().extension, ".cbz");

        let missing = state.get_extension(".unknown");
        assert!(missing.is_none());
    }

    #[test]
    fn test_get_extension_mut() {
        let mut state = AppState::default();

        if let Some(ext) = state.get_extension_mut(".cbz") {
            ext.thumbnail_enabled = true;
        }

        let ext = state.get_extension(".cbz").unwrap();
        assert!(ext.thumbnail_enabled);
    }

    #[test]
    fn test_is_valid() {
        let mut state = AppState::default();
        assert!(state.is_valid()); // No handlers, no DLL - valid

        state.extensions[0].thumbnail_enabled = true;
        assert!(!state.is_valid()); // Handlers but no DLL - invalid

        state.dll_registered = true;
        assert!(state.is_valid()); // Handlers with DLL - valid
    }
}
