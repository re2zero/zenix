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
use crate::events::EventState;
use crate::fleet::{FleetState, Tails};
use crate::herdr::ConnectionStatus;
use crate::layout::LayoutState;
use crate::plugins::host::PluginState;
use crate::plugins::model::{Placement, PluginAction};
use crate::plugins::ui_data::UiData;
use crate::plugins::ui_manifest::{
    Declared, PanelSpec, SidebarItemSpec, StatusItemSpec, UiContributions,
};
use crate::plugins::views;

/// One sidebar view: what the activity rail switches between. Every sidebar
/// block names the view it belongs to, so the rail decides what is on screen
/// and no block is always-visible (PLAN.md §M11a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SidebarView {
    Workspaces,
    Agents,
    Plugins,
    Events,
    Layout,
}

impl SidebarView {
    /// The rail's buttons, top to bottom.
    pub(crate) const ALL: [Self; 5] = [
        Self::Workspaces,
        Self::Agents,
        Self::Plugins,
        Self::Events,
        Self::Layout,
    ];

    /// The button's glyph, inline so the rail needs no asset registration:
    /// `stroke="currentColor"` takes the button's own colour.
    pub(crate) fn icon(self) -> &'static [u8] {
        match self {
            Self::Workspaces => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M9 9v11"/></svg>"#,
            Self::Agents => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="7" width="16" height="12" rx="3"/><path d="M12 3v4M9 13h.01M15 13h.01"/></svg>"#,
            Self::Plugins => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#,
            Self::Events => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h4l2.5-6 4 12 2.5-6h5"/></svg>"#,
            Self::Layout => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M12 9v11"/></svg>"#,
        }
    }
}

/// One thing the shell mounts in a slot: a sidebar block, a block of the
/// right-hand panel, a status-bar segment, and later an MCP tool.
#[derive(Clone)]
pub(crate) struct Contribution {
    /// Stable id, unique within its slot. A built-in's is a constant; a
    /// plugin's is the idempotent key of its declaration.
    pub(crate) id: String,
    /// Which sidebar view it belongs to. Only meaningful in the sidebar slot;
    /// the other slots carry a view and never read it.
    pub(crate) view: SidebarView,
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

/// A sidebar block, as a constant: what the shell mounts before any plugin is
/// read. A block names its view because the rail, not the block, decides what
/// is on screen.
pub(crate) struct SidebarBlock {
    pub(crate) id: &'static str,
    pub(crate) view: SidebarView,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

/// A status-bar segment, as a constant. Segments are not view-scoped: the bar
/// is one row, always visible.
pub(crate) struct Builtin {
    pub(crate) id: &'static str,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

impl Contribution {
    fn sidebar_block(block: &'static SidebarBlock) -> Self {
        Self {
            id: block.id.to_owned(),
            view: block.view,
            body: ContributionBody::Builtin(block.render),
        }
    }

    fn builtin(block: &'static Builtin) -> Self {
        Self {
            id: block.id.to_owned(),
            view: SidebarView::Workspaces,
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
    /// Bring one sidebar view to the front. The shell's own state, so the
    /// daemon never hears it.
    ShowView(SidebarView),
    /// A command from the directory.
    Command(Command),
    /// Bring a workspace to the front.
    FocusWorkspace(String),
    /// Bring a tab to the front.
    FocusTab(String),
    /// Move a tab to a gap in its workspace's order, as `tab.move` counts it:
    /// with the moving tab still in place.
    MoveTab { tab_id: String, insert_index: usize },
    /// Move a workspace to a gap in the sidebar's order, as `workspace.move`
    /// counts it: with the moving workspace still in place.
    MoveWorkspace {
        workspace_id: String,
        insert_index: usize,
    },
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
    /// Open the pre-link review: a plugin directory is read, and what it would
    /// run is shown, before anything is linked.
    OpenPluginTrust,
    /// Read the path in the review's input and show what it declares.
    ReviewPlugin,
    /// Open the review already reading one directory: what an installed
    /// plugin's row asks for, so its commands can be read back after the fact.
    OpenPluginTrustAt(String),
    /// Link a plugin directory. The daemon reads the manifest itself; this is
    /// the one request in the directory that hands it a path it has not seen.
    LinkPlugin(String),
    /// Close the review.
    ClosePluginTrust,
    /// Write the focused tab's layout to the project's template file.
    SaveLayout,
    /// Restore the project's template as a new tab in the focused workspace.
    RestoreLayout,
    /// Drop the events read so far. The subscription itself keeps running.
    ClearEvents,
    /// Read the tail of every running agent's pane, for the cockpit's
    /// side-by-side view.
    CompareFleet,
    /// Drop the fleet's transitions. The subscriptions themselves keep running.
    ClearFleet,
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
    /// The project the focused pane's cwd belongs to, and the template found in it.
    pub(crate) layout: &'a LayoutState,
    /// What the daemon has announced, newest first. Arrives on its own, unlike
    /// the plugin registry, so this is painted from a stream.
    pub(crate) events: &'a EventState,
    /// What each running agent is doing, and the transitions it went through,
    /// as far as this client has watched. Painted from a stream like `events`.
    pub(crate) fleet: &'a FleetState,
    /// The tails the cockpit last read, by pane. Read on demand, so this is a
    /// snapshot of the last ask rather than a live view.
    pub(crate) tails: &'a Tails,
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
        let mut panels = sidebar_blocks(super::panels::PANELS);
        // The plugin host's own blocks are the shell's blocks: they report what
        // is installed and what it ran, and they mount before anything a plugin
        // declares about itself.
        panels.extend(sidebar_blocks(crate::plugins::panels::PANELS));
        panels.extend(ui.sidebar_items.iter().map(|item| Contribution {
            id: item.qualified_id(),
            // A plugin's sidebar item belongs to the plugin view: the shell's
            // own views are about the daemon, this one is about what was added.
            view: SidebarView::Plugins,
            body: ContributionBody::SidebarItem(item.clone()),
        }));
        let mut aside = builtins(super::chat::ASIDE);
        // The cockpit sits under the chat panel: the conversation is what the
        // panel is opened for, and the fleet is what it is read against.
        aside.extend(builtins(super::cockpit::ASIDE));
        aside.extend(ui.panels.iter().map(|panel| Contribution {
            id: panel.qualified_id(),
            // Not a sidebar slot: the field is unread there.
            view: SidebarView::Workspaces,
            body: ContributionBody::Panel(panel.clone()),
        }));
        let mut status = builtins(super::panels::STATUS_SEGMENTS);
        status.extend(ui.status_items.iter().map(|item| Contribution {
            id: item.qualified_id(),
            view: SidebarView::Workspaces,
            body: ContributionBody::StatusItem(item.clone()),
        }));
        Self {
            panels,
            aside,
            status,
            settings: super::settings::SETTINGS_SECTIONS,
        }
    }

    /// The sidebar's blocks, top to bottom, for one view. Filtering here is
    /// what lets the rail own the sidebar's composition: the shell's render
    /// never decides which block is visible.
    pub(crate) fn panels_in(&self, view: SidebarView) -> impl Iterator<Item = &Contribution> {
        self.panels.iter().filter(move |panel| panel.view == view)
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

fn sidebar_blocks(blocks: &'static [SidebarBlock]) -> Vec<Contribution> {
    blocks.iter().map(Contribution::sidebar_block).collect()
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

    /// Every sidebar block the registry holds, in rail order: the slots are
    /// view-scoped now, so a test that wants "the sidebar" has to name which.
    fn sidebar(registry: &Registry) -> Vec<Contribution> {
        SidebarView::ALL
            .iter()
            .flat_map(|view| registry.panels_in(*view))
            .cloned()
            .collect()
    }

    #[test]
    fn built_in_contribution_ids_are_unique_within_their_slot() {
        let registry = registry();
        let panels = sidebar(&registry);
        for slot in [
            panels.as_slice(),
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

    /// The rail switches between views, so a view with no block would be a
    /// button that shows an empty sidebar.
    #[test]
    fn every_sidebar_view_has_at_least_one_block() {
        let registry = registry();
        for view in SidebarView::ALL {
            assert!(
                registry.panels_in(view).next().is_some(),
                "{view:?} has no block"
            );
        }
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
        let plugins_view: Vec<&str> = registry
            .panels_in(SidebarView::Plugins)
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
        // The plugin's own row lands behind the host's blocks in the view the
        // host lives in, never in front of them.
        assert_eq!(plugins_view.last().copied(), Some("a.b:sidebar:board"));
        assert_eq!(plugins_view.first().copied(), Some("plugins"));
        assert_eq!(aside.last().copied(), Some("a.b:panel:board"));
        assert_eq!(status.last().copied(), Some("a.b:status:wip"));
        // Each view opens with the shell's own block for it.
        assert_eq!(
            registry
                .panels_in(SidebarView::Workspaces)
                .next()
                .map(|item| item.id.as_str()),
            Some("workspaces")
        );
        assert_eq!(
            registry
                .panels_in(SidebarView::Agents)
                .next()
                .map(|item| item.id.as_str()),
            Some("agents")
        );
        assert_eq!(
            registry
                .panels_in(SidebarView::Events)
                .next()
                .map(|item| item.id.as_str()),
            Some("plugin-events")
        );
        assert_eq!(aside.first().copied(), Some("chat"));
        assert_eq!(status.first().copied(), Some("connection"));
        assert!(plugins_view.contains(&"plugin-logs"), "{plugins_view:?}");
    }
}
