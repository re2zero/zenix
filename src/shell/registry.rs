//! The contribution registry: every part of the shell's own surface is
//! mounted from here, so a built-in capability and a plugin contribution take
//! the same path and the shell never grows a second, hardcoded list beside
//! them (PLAN.md §5.4).
//!
//! Ported in shape from herdr-gpui's `usage/registry.rs` (Apache-2.0), which
//! lists its providers the same way; upstream has no UI-layer equivalent.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::Theme;
use gpui_kit::component::setting::SettingPage;
use gpui_kit::*;
use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame};

use super::commands::Command;
use crate::herdr::ConnectionStatus;
use crate::plugins::host::PluginState;
use crate::plugins::model::{Placement, PluginAction};
use crate::plugins::ui_data::UiData;
use crate::plugins::ui_manifest::{
    Declared, PanelSpec, SidebarItemSpec, StatusItemSpec, UiContributions,
};
use crate::plugins::views;

/// One thing the shell mounts in a slot: a sidebar block, a block of the
/// right-hand panel, a status-bar segment, and later an MCP tool.
#[derive(Clone)]
pub(crate) struct Contribution {
    /// Stable id, unique within its slot. A built-in's is a constant; a
    /// plugin's is the idempotent key of its declaration.
    pub(crate) id: String,
    /// What it is made of. The shell's own blocks and a plugin's declarations
    /// differ only here, never in how they are mounted or dispatched.
    pub(crate) body: ContributionBody,
}

/// What a contribution is made of.
#[derive(Clone)]
pub(crate) enum ContributionBody {
    /// A function of the shell's own: the blocks the shell is born with.
    Builtin(fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement),
    /// A panel a plugin declared in its `zenix-ui.toml`.
    Panel(Arc<PanelSpec>),
    /// A sidebar line that opens one of those panels.
    SidebarItem(Arc<SidebarItemSpec>),
    /// A status-bar segment formatted from a declaration's data.
    StatusItem(Arc<StatusItemSpec>),
}

/// A built-in block, as a constant: what the shell mounts before any plugin is
/// read.
pub(crate) struct Builtin {
    pub(crate) id: &'static str,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

impl Contribution {
    fn builtin(block: &'static Builtin) -> Self {
        Self {
            id: block.id.to_owned(),
            body: ContributionBody::Builtin(block.render),
        }
    }

    /// Build this contribution's element for one frame. Both kinds come through
    /// here, which is what keeps "a plugin panel mounts the same way as the
    /// shell's own" a fact about the code rather than a promise.
    pub(crate) fn render(
        &self,
        invocation: &Invocation<'_>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match &self.body {
            ContributionBody::Builtin(render) => render(invocation, window, cx),
            ContributionBody::Panel(spec) => views::panel(spec, invocation, window, cx),
            ContributionBody::SidebarItem(spec) => {
                views::sidebar_item(spec, invocation, window, cx)
            }
            ContributionBody::StatusItem(spec) => views::status_item(spec, invocation, window, cx),
        }
    }
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
/// contribution's element. Cloning one is the normal case: an element that
/// dispatches a request has to own it for as long as the frame's tree lives.
#[derive(Clone)]
pub(crate) enum ShellRequest {
    /// A command from the directory.
    Command(Command),
    /// Bring a workspace to the front.
    FocusWorkspace(String),
    /// Bring a tab to the front.
    FocusTab(String),
    /// Focus a pane, which also brings its workspace and tab forward.
    FocusPane(String),
    /// Run one plugin action. `source` names where the click came from, which
    /// is the one part of the invocation context the daemon cannot know.
    PluginAction {
        action: PluginAction,
        source: &'static str,
    },
    /// Open a pane a plugin declared, at the placement the manifest asks for.
    OpenPluginPane {
        plugin_id: String,
        entrypoint: String,
        title: String,
        placement: Placement,
    },
    /// Close a plugin pane this client opened.
    ClosePluginPane(String),
    /// Show the right-hand panel, where a plugin's drawer panels mount.
    ShowPluginPanels,
    /// Re-read the plugin registry, which nothing pushes, and re-read the data
    /// its declarations print.
    RefreshPlugins,
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
    /// The plugin registry as of the last refresh. Nothing pushes a plugin
    /// change, so unlike the snapshot this is read, not painted from a stream.
    pub(crate) plugins: &'a PluginState,
    /// What the declarations' commands last printed, by contribution id.
    pub(crate) ui_data: &'a UiData,
    pub(crate) theme: &'a Theme,
    pub(crate) dispatch: &'a Dispatch,
}

/// Where the shell mounts its contributions.
pub(crate) struct Registry {
    panels: Vec<Contribution>,
    aside: Vec<Contribution>,
    status: Vec<Contribution>,
    settings: &'static [SettingsSection],
}

impl Registry {
    /// The shell's own capabilities and every plugin's declared UI, in one
    /// order: the shell's blocks first, then the plugins', in the order the
    /// daemon lists them. Rebuilt whenever the plugin registry is re-read,
    /// because a plugin's contributions appear and vanish with the plugin.
    pub(crate) fn new(ui: &UiContributions) -> Self {
        let mut panels = builtins(super::panels::PANELS);
        // The plugin host's own blocks are the shell's blocks: they report what
        // is installed and what it ran, and they mount before anything a plugin
        // declares about itself.
        panels.extend(builtins(crate::plugins::panels::PANELS));
        panels.extend(ui.sidebar_items.iter().map(|item| Contribution {
            id: item.qualified_id(),
            body: ContributionBody::SidebarItem(item.clone()),
        }));
        let mut aside = builtins(super::chat::ASIDE);
        aside.extend(ui.panels.iter().map(|panel| Contribution {
            id: panel.qualified_id(),
            body: ContributionBody::Panel(panel.clone()),
        }));
        let mut status = builtins(super::panels::STATUS_SEGMENTS);
        status.extend(ui.status_items.iter().map(|item| Contribution {
            id: item.qualified_id(),
            body: ContributionBody::StatusItem(item.clone()),
        }));
        Self {
            panels,
            aside,
            status,
            settings: super::settings::SETTINGS_SECTIONS,
        }
    }

    /// The sidebar's blocks, top to bottom.
    pub(crate) fn panels(&self) -> &[Contribution] {
        &self.panels
    }

    /// The right-hand panel's blocks, top to bottom. Unlike the sidebar's,
    /// these are only built while the panel is open.
    pub(crate) fn aside_panels(&self) -> &[Contribution] {
        &self.aside
    }

    /// The status bar's segments, left to right.
    pub(crate) fn status_segments(&self) -> &[Contribution] {
        &self.status
    }

    /// The settings surface's pages, in the order it lists them.
    pub(crate) fn settings_sections(&self) -> &'static [SettingsSection] {
        self.settings
    }
}

fn builtins(blocks: &'static [Builtin]) -> Vec<Contribution> {
    blocks.iter().map(Contribution::builtin).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn registry() -> Registry {
        Registry::new(&UiContributions::default())
    }

    fn with_ui(text: &str) -> Registry {
        Registry::new(&UiContributions::parse(
            "a.b",
            std::path::Path::new("/tmp/zenix-ui-root"),
            text,
        ))
    }

    #[test]
    fn built_in_contribution_ids_are_unique_within_their_slot() {
        let registry = registry();
        for slot in [
            registry.panels(),
            registry.aside_panels(),
            registry.status_segments(),
        ] {
            let mut ids: Vec<&str> = slot.iter().map(|item| item.id.as_str()).collect();
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
        let registry = registry();
        assert!(!registry.panels().is_empty());
        assert!(!registry.aside_panels().is_empty());
        assert!(!registry.status_segments().is_empty());
    }

    #[test]
    fn the_settings_surface_mounts_at_least_one_page() {
        assert!(!registry().settings_sections().is_empty());
    }

    /// The one thing a plugin's declaration cannot do is pick its slot freely:
    /// a sidebar item goes to the sidebar, a panel to the right-hand panel, and
    /// a status item to the status bar, each behind the shell's own blocks.
    #[test]
    fn a_plugins_contributions_mount_in_the_slots_their_kind_names() {
        let registry = with_ui(
            r#"
[[sidebar_item]]
id = "board"
title = "Board"
panel = "board"

[[panel]]
id = "board"
title = "Board"

[[status_item]]
id = "wip"
format = "{count} WIP"
"#,
        );
        let sidebar: Vec<&str> = registry
            .panels()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        let aside: Vec<&str> = registry
            .aside_panels()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        let status: Vec<&str> = registry
            .status_segments()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(sidebar.last().copied(), Some("a.b:sidebar:board"));
        assert_eq!(aside.last().copied(), Some("a.b:panel:board"));
        assert_eq!(status.last().copied(), Some("a.b:status:wip"));
        // The shell's own blocks stay in front of them, the plugin host's among
        // them: a plugin's own row must not push the host's blocks out.
        assert_eq!(sidebar.first().copied(), Some("workspaces"));
        assert_eq!(aside.first().copied(), Some("chat"));
        assert_eq!(status.first().copied(), Some("connection"));
        let ids: Vec<&str> = registry
            .panels()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert!(ids.contains(&"plugins"), "{ids:?}");
        assert!(ids.contains(&"plugin-logs"), "{ids:?}");
    }
}
