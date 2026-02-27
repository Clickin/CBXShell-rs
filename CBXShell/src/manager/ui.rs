///! Native-windows-gui based UI for CBXManager
///!
///! Compact, professional interface with proper alignment and spacing
use super::{registry_ops, state::AppState, utils};
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use std::cell::{Cell, RefCell};

const WINDOW_WIDTH: i32 = 360;
const WINDOW_HEIGHT: i32 = 370;

const MARGIN_X: i32 = 10;
const STATUS_Y: i32 = 12;

const GROUP_WIDTH: i32 = 320;
const FILE_GROUP_Y: i32 = 44;
const FILE_GROUP_HEIGHT: i32 = 138;
const ADVANCED_GROUP_Y: i32 = FILE_GROUP_Y + FILE_GROUP_HEIGHT + 10;
const ADVANCED_GROUP_HEIGHT: i32 = 120;

const BUTTON_WIDTH: i32 = 80;
const BUTTON_HEIGHT: i32 = 24;
const BUTTON_SPACING: i32 = 8;
const BUTTON_Y: i32 = 320;
const BUTTON_ROW_X: i32 = WINDOW_WIDTH - MARGIN_X - (BUTTON_WIDTH * 3 + BUTTON_SPACING * 2);

const CHECKBOX_X: i32 = 12;
const CHECKBOX_Y_START: i32 = 18;
const CHECKBOX_STEP: i32 = 17;

thread_local! {
    static APP_STATE: RefCell<AppState> = RefCell::new(AppState::default());
    static NEEDS_RESTART: Cell<bool> = Cell::new(false);
}

#[derive(Default, NwgUi)]
pub struct CBXManagerApp {
    #[nwg_control(
        size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        position: (300, 300),
        title: "CBXShell Manager",
        flags: "WINDOW|VISIBLE"
    )]
    #[nwg_events(OnWindowClose: [CBXManagerApp::exit])]
    window: nwg::Window,

    #[nwg_control(parent: window, text: "&Tools")]
    tools_menu: nwg::Menu,

    #[nwg_control(parent: tools_menu, text: "Register DLL")]
    #[nwg_events(OnMenuItemSelected: [CBXManagerApp::on_register_dll])]
    register_menu: nwg::MenuItem,

    #[nwg_control(parent: tools_menu, text: "Unregister DLL")]
    #[nwg_events(OnMenuItemSelected: [CBXManagerApp::on_unregister_dll])]
    unregister_menu: nwg::MenuItem,

    #[nwg_control(parent: tools_menu)]
    tools_separator: nwg::MenuSeparator,

    #[nwg_control(parent: tools_menu, text: "About")]
    #[nwg_events(OnMenuItemSelected: [CBXManagerApp::on_about])]
    about_menu: nwg::MenuItem,

    #[nwg_resource(family: "Segoe UI", size: 16)]
    ui_font: nwg::Font,

    #[nwg_control(
        parent: window,
        text: "",
        position: (MARGIN_X, STATUS_Y),
        size: (16, 22)
    )]
    status_icon: nwg::Label,

    #[nwg_control(
        parent: window,
        text: "",
        position: (MARGIN_X + 20, STATUS_Y),
        size: (300, 22)
    )]
    status_text: nwg::Label,

    #[nwg_control(
        parent: window,
        position: (MARGIN_X, FILE_GROUP_Y),
        size: (GROUP_WIDTH, FILE_GROUP_HEIGHT),
        flags: "BORDER|VISIBLE"
    )]
    file_group_frame: nwg::Frame,

    #[nwg_control(
        parent: window,
        text: "File types",
        position: (MARGIN_X + 8, FILE_GROUP_Y - 6),
        size: (120, 24)
    )]
    file_group_label: nwg::Label,

    #[nwg_control(
        parent: file_group_frame,
        text: "CBZ Image Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 0)),
        size: (260, 18)
    )]
    cbz_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: file_group_frame,
        text: "ZIP Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 1)),
        size: (260, 18)
    )]
    zip_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: file_group_frame,
        text: "CBR Image Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 2)),
        size: (260, 18)
    )]
    cbr_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: file_group_frame,
        text: "RAR Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 3)),
        size: (260, 18)
    )]
    rar_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: file_group_frame,
        text: "CB7 Image Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 4)),
        size: (260, 18)
    )]
    cb7_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: file_group_frame,
        text: "7Z Archives",
        position: (CHECKBOX_X, CHECKBOX_Y_START + (CHECKBOX_STEP * 5)),
        size: (260, 18)
    )]
    sevenz_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: window,
        position: (MARGIN_X, ADVANCED_GROUP_Y),
        size: (GROUP_WIDTH, ADVANCED_GROUP_HEIGHT),
        flags: "BORDER|VISIBLE"
    )]
    advanced_group_frame: nwg::Frame,

    #[nwg_control(
        parent: window,
        text: "Advanced",
        position: (MARGIN_X + 8, ADVANCED_GROUP_Y - 6),
        size: (120, 24)
    )]
    advanced_group_label: nwg::Label,

    #[nwg_control(
        parent: advanced_group_frame,
        text: "Sort images alphabetically",
        position: (CHECKBOX_X, 16),
        size: (260, 18)
    )]
    sort_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: window,
        text: "Uncheck to sort images by archive order.\r\nRequired to display custom thumbnail.",
        position: (MARGIN_X + CHECKBOX_X, ADVANCED_GROUP_Y + 34),
        size: (300, 28)
    )]
    sort_help_label: nwg::Label,

    #[nwg_control(
        parent: advanced_group_frame,
        text: "Sort preview pages alphabetically",
        position: (CHECKBOX_X, 68),
        size: (300, 18)
    )]
    sort_preview_checkbox: nwg::CheckBox,

    #[nwg_control(
        parent: window,
        text: "Uncheck to use archive order in the preview pane.",
        position: (MARGIN_X + CHECKBOX_X, ADVANCED_GROUP_Y + 86),
        size: (300, 20)
    )]
    sort_preview_help_label: nwg::Label,

    #[nwg_control(
        parent: window,
        text: "OK",
        position: (BUTTON_ROW_X, BUTTON_Y),
        size: (BUTTON_WIDTH, BUTTON_HEIGHT)
    )]
    #[nwg_events(OnButtonClick: [CBXManagerApp::on_ok])]
    ok_button: nwg::Button,

    #[nwg_control(
        parent: window,
        text: "Cancel",
        position: (BUTTON_ROW_X + BUTTON_WIDTH + BUTTON_SPACING, BUTTON_Y),
        size: (BUTTON_WIDTH, BUTTON_HEIGHT)
    )]
    #[nwg_events(OnButtonClick: [CBXManagerApp::on_cancel])]
    cancel_button: nwg::Button,

    #[nwg_control(
        parent: window,
        text: "Apply",
        position: (BUTTON_ROW_X + (BUTTON_WIDTH + BUTTON_SPACING) * 2, BUTTON_Y),
        size: (BUTTON_WIDTH, BUTTON_HEIGHT)
    )]
    #[nwg_events(OnButtonClick: [CBXManagerApp::on_apply])]
    apply_button: nwg::Button,
}

impl CBXManagerApp {
    pub fn initialize_state(&self) {
        let state = registry_ops::read_app_state().unwrap_or_default();
        APP_STATE.with(|stored| {
            *stored.borrow_mut() = state;
        });
        self.apply_font();
        self.sync_controls_from_state();
    }

    fn sync_controls_from_state(&self) {
        let state = self.get_state();

        self.status_icon
            .set_text(if state.dll_registered { "✓" } else { "⚠" });
        self.status_text.set_text(if state.dll_registered {
            "DLL Registered"
        } else {
            "DLL Not Registered"
        });

        let zip_family_enabled =
            self.extension_enabled(&state, ".zip") || self.extension_enabled(&state, ".cbz");
        self.set_checkbox(&self.cbz_checkbox, zip_family_enabled);
        self.set_checkbox(&self.zip_checkbox, zip_family_enabled);
        let rar_family_enabled =
            self.extension_enabled(&state, ".rar") || self.extension_enabled(&state, ".cbr");
        self.set_checkbox(&self.cbr_checkbox, rar_family_enabled);
        self.set_checkbox(&self.rar_checkbox, rar_family_enabled);
        let sevenz_family_enabled =
            self.extension_enabled(&state, ".7z") || self.extension_enabled(&state, ".cb7");
        self.set_checkbox(&self.cb7_checkbox, sevenz_family_enabled);
        self.set_checkbox(&self.sevenz_checkbox, sevenz_family_enabled);
        self.set_checkbox(&self.sort_checkbox, state.sort_enabled);
        self.set_checkbox(&self.sort_preview_checkbox, state.sort_preview_enabled);
    }

    fn extension_enabled(&self, state: &AppState, extension: &str) -> bool {
        state
            .get_extension(extension)
            .map(|ext| ext.thumbnail_enabled)
            .unwrap_or(false)
    }

    fn apply_font(&self) {
        let font = Some(&self.ui_font);

        self.status_icon.set_font(font);
        self.status_text.set_font(font);
        self.file_group_label.set_font(font);
        self.cbz_checkbox.set_font(font);
        self.zip_checkbox.set_font(font);
        self.cbr_checkbox.set_font(font);
        self.rar_checkbox.set_font(font);
        self.cb7_checkbox.set_font(font);
        self.sevenz_checkbox.set_font(font);
        self.advanced_group_label.set_font(font);
        self.sort_checkbox.set_font(font);
        self.sort_help_label.set_font(font);
        self.sort_preview_checkbox.set_font(font);
        self.sort_preview_help_label.set_font(font);
        self.ok_button.set_font(font);
        self.cancel_button.set_font(font);
        self.apply_button.set_font(font);
    }

    fn set_checkbox(&self, checkbox: &nwg::CheckBox, enabled: bool) {
        let state = if enabled {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        };
        checkbox.set_check_state(state);
    }

    fn checkbox_value(&self, checkbox: &nwg::CheckBox) -> bool {
        checkbox.check_state() == nwg::CheckBoxState::Checked
    }

    fn build_state_from_controls(&self) -> AppState {
        let mut state = self.get_state();

        state.sort_enabled = self.checkbox_value(&self.sort_checkbox);
        state.sort_preview_enabled = self.checkbox_value(&self.sort_preview_checkbox);

        let zip_family_enabled =
            self.checkbox_value(&self.zip_checkbox) || self.checkbox_value(&self.cbz_checkbox);
        if let Some(ext) = state.get_extension_mut(".cbz") {
            ext.thumbnail_enabled = zip_family_enabled;
        }
        if let Some(ext) = state.get_extension_mut(".zip") {
            ext.thumbnail_enabled = zip_family_enabled;
        }
        let rar_family_enabled =
            self.checkbox_value(&self.rar_checkbox) || self.checkbox_value(&self.cbr_checkbox);
        if let Some(ext) = state.get_extension_mut(".cbr") {
            ext.thumbnail_enabled = rar_family_enabled;
        }
        if let Some(ext) = state.get_extension_mut(".rar") {
            ext.thumbnail_enabled = rar_family_enabled;
        }
        let sevenz_family_enabled =
            self.checkbox_value(&self.sevenz_checkbox) || self.checkbox_value(&self.cb7_checkbox);
        if let Some(ext) = state.get_extension_mut(".cb7") {
            ext.thumbnail_enabled = sevenz_family_enabled;
        }
        if let Some(ext) = state.get_extension_mut(".7z") {
            ext.thumbnail_enabled = sevenz_family_enabled;
        }

        state
    }

    fn apply_settings(&self) {
        let state = self.build_state_from_controls();
        if !state.is_valid() {
            return;
        }

        if let Err(e) = registry_ops::write_app_state(&state) {
            eprintln!("Failed to save settings: {}", e);
        } else {
            self.set_needs_restart(true);
            APP_STATE.with(|stored| {
                *stored.borrow_mut() = state;
            });
        }
    }

    fn on_register_dll(&self) {
        match registry_ops::register_dll() {
            Ok(_) => {
                let mut state = self.build_state_from_controls();
                state.dll_registered = true;
                if let Err(e) = registry_ops::write_app_state(&state) {
                    eprintln!("Failed to apply extension handlers after DLL registration: {}", e);
                }
                self.initialize_state();
                self.set_needs_restart(true);
            }
            Err(e) => {
                eprintln!("Failed to register DLL: {}", e);
            }
        }
    }

    fn on_unregister_dll(&self) {
        match registry_ops::unregister_dll() {
            Ok(_) => {
                self.initialize_state();
                self.set_needs_restart(true);
            }
            Err(e) => {
                eprintln!("Failed to unregister DLL: {}", e);
            }
        }
    }

    fn on_ok(&self) {
        self.apply_settings();
        if self.needs_restart() && utils::prompt_restart_explorer() {
            let _ = utils::restart_explorer();
        }
        self.window.close();
    }

    fn on_cancel(&self) {
        self.window.close();
    }

    fn on_apply(&self) {
        self.apply_settings();
    }

    fn on_about(&self) {
        utils::show_success(
            "CBXShell Manager",
            "CBXShell AVIF policy (v5.1.2)\n\nSupported AVIF path:\n- Windows WIC codec pipeline\n\nRequired codecs:\n- HEIF Image Extensions\n- AV1 Video Extension\n\nIf AVIF thumbnails fail, install/update both codecs and restart Explorer.\n\nNote: Software fallback may exist for compatibility, but support triage is based on the WIC path first.",
        );
    }

    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn get_state(&self) -> AppState {
        APP_STATE.with(|stored| stored.borrow().clone())
    }

    fn needs_restart(&self) -> bool {
        NEEDS_RESTART.with(|flag| flag.get())
    }

    fn set_needs_restart(&self, value: bool) {
        NEEDS_RESTART.with(|flag| flag.set(value));
    }
}
