use native_windows_gui as nwg;
use native_windows_gui::NativeUi;

#[path = "../../../CBXShell/src/manager/mod.rs"]
mod manager;

pub type AppResult = anyhow::Result<()>;

pub use manager::ui::CBXManagerApp;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppCommand {
    Gui,
    BackgroundCheck,
    ManualCheck,
    OpenReleaseUrl(String),
}

fn parse_command<I, S>(args: I) -> anyhow::Result<AppCommand>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    match args.next().as_deref() {
        None => Ok(AppCommand::Gui),
        Some("--check-updates-background") => Ok(AppCommand::BackgroundCheck),
        Some("--check-updates-now") => Ok(AppCommand::ManualCheck),
        Some("--open-release-url") => {
            let url = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing URL for --open-release-url"))?;
            Ok(AppCommand::OpenReleaseUrl(url))
        }
        Some(other) => Err(anyhow::anyhow!("unknown command line mode: {}", other)),
    }
}

pub fn run() -> AppResult {
    let command = parse_command(std::env::args())?;
    match command {
        AppCommand::Gui => run_gui(),
        AppCommand::BackgroundCheck => manager::update::run_background_check(),
        AppCommand::ManualCheck => manager::update::run_manual_check(),
        AppCommand::OpenReleaseUrl(url) => manager::utils::open_url(&url),
    }
}

fn run_gui() -> AppResult {
    nwg::init()?;
    nwg::Font::set_global_family("Segoe UI")?;

    let app = CBXManagerApp::build_ui(Default::default())?;
    app.initialize_state();

    nwg::dispatch_thread_events();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_command, AppCommand};

    #[test]
    fn parses_gui_mode_without_extra_args() {
        let command = parse_command(["CBXManager.exe"]).expect("gui mode should parse");
        assert_eq!(command, AppCommand::Gui);
    }

    #[test]
    fn parses_background_mode() {
        let command = parse_command(["CBXManager.exe", "--check-updates-background"])
            .expect("background mode should parse");
        assert_eq!(command, AppCommand::BackgroundCheck);
    }

    #[test]
    fn parses_manual_mode() {
        let command =
            parse_command(["CBXManager.exe", "--check-updates-now"]).expect("manual mode should parse");
        assert_eq!(command, AppCommand::ManualCheck);
    }

    #[test]
    fn parses_open_release_url_mode() {
        let command = parse_command([
            "CBXManager.exe",
            "--open-release-url",
            "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.0",
        ])
        .expect("open release mode should parse");
        assert_eq!(
            command,
            AppCommand::OpenReleaseUrl(
                "https://github.com/Clickin/CBXShell-rs/releases/tag/v5.2.0".to_string()
            )
        );
    }
}
