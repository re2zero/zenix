//! The contribution registry: every part of the shell's own surface is
//! mounted from here, so a built-in capability and a plugin contribution take
//! the same path and the shell never grows a second, hardcoded list beside
//! them (PLAN.md §5.4).
//!
//! Ported in shape from herdr-gpui's `usage/registry.rs` (Apache-2.0), which
//! lists its providers the same way; upstream has no UI-layer equivalent.

use std::rc::Rc;

use gpui_kit::component::Theme;
use gpui_kit::component::setting::SettingPage;
use gpui_kit::*;
use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame};

use super::commands::Command;
use crate::herdr::ConnectionStatus;

/// One thing the shell mounts in a slot: a sidebar block, a status-bar
/// segment, and later a panel or an MCP tool.
pub(crate) struct Contribution {
    /// Stable id, unique within its slot.
    pub(crate) id: &'static str,
    /// Builds the element, once per frame, from the invocation below.
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

/// One page of the settings surface. A page is built from the app rather than
/// from an [`Invocation`]: its fields read and write the config global
/// (`crate::config`), which is where a settings value lives, not the daemon's
/// snapshot.
pub(crate) struct SettingsSection {
    /// Stable id, unique among the sections.
    pub(crate) id: &'static str,
    /// Builds the page, from the config as it is now.
    pub(crate) page: fn(&App) -> SettingPage,
}

/// What the shell can be asked to do, by a chord or by a click on a
/// contribution's element.
pub(crate) enum ShellRequest {
    /// A command from the directory.
    Command(Command),
    /// Bring a workspace to the front.
    FocusWorkspace(String),
    /// Bring a tab to the front.
    FocusTab(String),
    /// Focus a pane, which also brings its workspace and tab forward.
    FocusPane(String),
}

/// A request's way back into the shell. Shared rather than borrowed because
/// every element that dispatches one has to own it for as long as the frame's
/// tree lives.
pub(crate) type Dispatch = Rc<dyn Fn(ShellRequest, &mut Window, &mut App)>;

/// Everything a contribution may read while it builds its element. A built-in
/// and a plugin contribution receive this same shape, so neither can depend on
/// being the first one mounted (PLAN.md §5.9).
pub(crate) struct Invocation<'a> {
    pub(crate) snapshot: Option<&'a ClientShellSnapshot>,
    pub(crate) surface: Option<&'a PaneSurfaceFrame>,
    pub(crate) status: ConnectionStatus,
    /// Why the daemon is unreachable, when it is.
    pub(crate) status_error: Option<&'a str>,
    /// Why the daemon refused the last command, until one is accepted.
    pub(crate) rejection: Option<&'a str>,
    /// The prefix's label while it is armed.
    pub(crate) prefix: Option<&'a str>,
    pub(crate) theme: &'a Theme,
    pub(crate) dispatch: &'a Dispatch,
}

/// Where the shell mounts its contributions.
pub(crate) struct Registry {
    panels: &'static [Contribution],
    status: &'static [Contribution],
    settings: &'static [SettingsSection],
}

impl Registry {
    /// The shell's own capabilities, registered exactly as a plugin's would
    /// be. Nothing here is reached by name from the shell's render.
    pub(crate) fn builtin() -> Self {
        Self {
            panels: super::panels::PANELS,
            status: super::panels::STATUS_SEGMENTS,
            settings: super::settings::SETTINGS_SECTIONS,
        }
    }

    /// The sidebar's blocks, top to bottom.
    pub(crate) fn panels(&self) -> &'static [Contribution] {
        self.panels
    }

    /// The status bar's segments, left to right.
    pub(crate) fn status_segments(&self) -> &'static [Contribution] {
        self.status
    }

    /// The settings surface's pages, in the order it lists them.
    pub(crate) fn settings_sections(&self) -> &'static [SettingsSection] {
        self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use std::prelude::v1::test;

    #[test]
    fn built_in_contribution_ids_are_unique_within_their_slot() {
        let registry = Registry::builtin();
        for slot in [registry.panels(), registry.status_segments()] {
            let mut ids: Vec<&str> = slot.iter().map(|item| item.id).collect();
            let total = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), total);
        }
        let mut ids: Vec<&str> = registry
            .settings_sections()
            .iter()
            .map(|section| section.id)
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "settings section ids are unique");
    }

    #[test]
    fn the_shell_mounts_at_least_one_panel_and_one_status_segment() {
        let registry = Registry::builtin();
        assert!(!registry.panels().is_empty());
        assert!(!registry.status_segments().is_empty());
    }

    #[test]
    fn the_settings_surface_mounts_at_least_one_page() {
        assert!(!Registry::builtin().settings_sections().is_empty());
    }
}
