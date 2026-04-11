use native_windows_gui as nwg;
use native_windows_gui::NativeUi;

#[path = "../../../CBXShell/src/manager/mod.rs"]
mod manager;

pub type AppResult = Result<(), nwg::NwgError>;

pub use manager::ui::CBXManagerApp;

pub fn run() -> AppResult {
    nwg::init()?;
    nwg::Font::set_global_family("Segoe UI")?;

    let app = CBXManagerApp::build_ui(Default::default())?;
    app.initialize_state();

    nwg::dispatch_thread_events();
    Ok(())
}
