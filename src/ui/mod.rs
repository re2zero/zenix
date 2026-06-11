//! UI components: sidebar, icon bar, settings panel, system info panel.

mod sidebar;

pub use sidebar::{
    sidebar, settings_panel, system_info_panel,
    Panel, ThemeCallback, SIDEBAR_WIDTH, PANEL_WIDTH,
};
