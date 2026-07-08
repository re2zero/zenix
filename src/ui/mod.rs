//! UI components: sidebar, icon bar, settings panel, system info panel.

pub mod settings;
mod sidebar;

pub use settings::settings_dialog;
pub use sidebar::{
    PANEL_WIDTH, Panel, SIDEBAR_WIDTH, ThemeCallback, sidebar, system_info_panel,
};
