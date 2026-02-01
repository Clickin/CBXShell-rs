#![windows_subsystem = "windows"]

mod registry_ops;
///! CBXManager - Native configuration utility for CBXShell
///!
///! Built with native-windows-gui for a Windows-native interface
mod state;
mod ui;
mod utils;

use native_windows_gui as nwg;
use native_windows_gui::NativeUi;

fn main() -> Result<(), nwg::NwgError> {
    nwg::init()?;
    nwg::Font::set_global_family("Segoe UI")?;

    let app = ui::CBXManagerApp::build_ui(Default::default())?;
    app.initialize_state();

    nwg::dispatch_thread_events();
    Ok(())
}
