//! UI components: sidebar, icon bar, settings panel, system info panel.

mod sidebar;
pub mod settings;

pub use settings::settings_dialog;
pub use settings::SkillAction;
pub use settings::SKILL_AGENTS;
pub use sidebar::{
    sidebar, system_info_panel,
    Panel, ThemeCallback, SIDEBAR_WIDTH, PANEL_WIDTH,
};
