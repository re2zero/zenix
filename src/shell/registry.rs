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
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame};

use super::chat::ChatPanel;
use super::commands::Command;
use crate::events::EventState;
use crate::fleet::{FleetState, Tails};
use crate::herdr::ConnectionStatus;
use crate::layout::LayoutState;
use crate::plugins::host::PluginState;
use crate::plugins::model::{Placement, PluginAction};
use crate::plugins::ui_data::UiData;
use crate::plugins::ui_manifest::{
    Declared, PanelSpec, SettingsSectionSpec, SidebarItemSpec, StatusItemSpec, UiContributions,
};
use crate::plugins::views;

/// One sidebar view: what the activity rail switches between. Every sidebar
/// block names the view it belongs to, so the rail decides what is on screen
/// and no block is always-visible (PLAN.md §M11a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SidebarView {
    Workspaces,
    Agents,
    Fleet,
    Plugins,
    Events,
    Layout,
}

/// One tab of the right-hand panel: the conversation, the fleet, and whatever
/// the plugins declare. A tab is the panel's unit of composition exactly as a
/// [`SidebarView`] is the sidebar's, so the panel shows one tab's blocks and
/// never a column of everything (PLAN.md §M11c).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AsideTab {
    Chat,
    Fleet,
    Panels,
}

impl AsideTab {
    /// The strip's tabs, left to right.
    pub(crate) const ALL: [Self; 3] = [Self::Chat, Self::Fleet, Self::Panels];

    /// The stable id the i18n key and the tab's element id are built from.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Fleet => "fleet",
            Self::Panels => "panels",
        }
    }

    /// What the tab is called, from the `shell.aside` section of the locale
    /// file, which is keyed by the tab's own id.
    pub(crate) fn label(self) -> String {
        crate::i18n::t(&format!("shell.aside.{}", self.id()))
    }

    /// The tab's glyph, inline so the strip needs no asset registration.
    pub(crate) fn icon(self) -> &'static [u8] {
        match self {
            Self::Chat => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H9l-5 4Z"/></svg>"#,
            Self::Fleet => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="8" r="3"/><circle cx="17" cy="9" r="2.4"/><path d="M3 20c0-3.3 2.7-6 6-6s6 2.7 6 6"/><path d="M15.5 14.2A5 5 0 0 1 21 19"/></svg>"#,
            Self::Panels => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#,
        }
    }
}

impl SidebarView {
    /// The rail's buttons, top to bottom.
    pub(crate) const ALL: [Self; 6] = [
        Self::Workspaces,
        Self::Agents,
        Self::Fleet,
        Self::Plugins,
        Self::Events,
        Self::Layout,
    ];

    /// The stable id the i18n keys for the view's head and foot are built from.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Agents => "agents",
            Self::Fleet => "fleet",
            Self::Plugins => "plugins",
            Self::Events => "events",
            Self::Layout => "layout",
        }
    }

    /// What the view is called, from the `shell.view` section of the locale
    /// file, which is keyed by the view's own id.
    pub(crate) fn label(self) -> String {
        crate::i18n::t(&format!("shell.view.{}", self.id()))
    }

    /// The button's glyph, inline so the rail needs no asset registration:
    /// `stroke="currentColor"` takes the button's own colour.
    pub(crate) fn icon(self) -> &'static [u8] {
        match self {
            Self::Workspaces => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M9 9v11"/></svg>"#,
            Self::Agents => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="7" width="16" height="12" rx="3"/><path d="M12 3v4M9 13h.01M15 13h.01"/></svg>"#,
            // The fleet's glyph is the aside tab's own, so the same drawing
            // names the cockpit wherever it opens.
            Self::Fleet => AsideTab::Fleet.icon(),
            Self::Plugins => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#,
            Self::Events => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h4l2.5-6 4 12 2.5-6h5"/></svg>"#,
            Self::Layout => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M12 9v11"/></svg>"#,
        }
    }
}

/// Which slot a contribution belongs to, and which part of that slot shows it:
/// a sidebar block names its view, a right-hand panel block names its tab. The
/// scope is what the shell filters on, so no block decides for itself whether it
/// is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ContributionScope {
    View(SidebarView),
    Tab(AsideTab),
}

/// One thing the shell mounts in a slot: a sidebar block, a block of the
/// right-hand panel, a status-bar segment. Mounting is positional — a slot is
/// iterated in order, filtered by scope — so a contribution carries no name
/// of its own; a plugin declaration's identity lives on its spec.
#[derive(Clone)]
pub(crate) struct Contribution {
    /// Which part of its slot shows it. Every contribution has one; the slot
    /// itself is decided by the collection it is built into.
    pub(crate) scope: ContributionScope,
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
}

/// A sidebar block, as a constant: what the shell mounts before any plugin is
/// read. A block names its view because the rail, not the block, decides what
/// is on screen.
pub(crate) struct SidebarBlock {
    pub(crate) view: SidebarView,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

/// One block of the right-hand panel, as a constant: what the panel mounts
/// before any plugin is read. A block names its tab because the strip, not the
/// block, decides what is on screen.
pub(crate) struct AsideBlock {
    pub(crate) tab: AsideTab,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> AnyElement,
}

/// Which end of the status bar a segment sits at. A segment that reports what
/// the session is (connection, counts, plugins, agents) opens the bar; one that
/// reports what the client thinks (diagnostics, refusal, branch, key prefix)
/// closes it, past the free space (PLAN.md §M11a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StatusAlign {
    Start,
    End,
}

/// One status-bar segment the bar can lay out: where it goes, and how it is
/// built. `None` is a segment with nothing to say right now — the bar skips it,
/// padding and divider included, so a quiet segment leaves no gap behind.
pub(crate) struct StatusSegment {
    pub(crate) align: StatusAlign,
    pub(crate) render: fn(&Invocation<'_>, &mut Window, &mut App) -> Option<AnyElement>,
}

/// One segment the bar mounts, the shell's own or a plugin's.
pub(crate) struct StatusContribution {
    align: StatusAlign,
    body: StatusBody,
}

enum StatusBody {
    Builtin(&'static StatusSegment),
    Plugin(Arc<StatusItemSpec>),
}

impl StatusContribution {
    fn builtin(segment: &'static StatusSegment) -> Self {
        Self {
            align: segment.align,
            body: StatusBody::Builtin(segment),
        }
    }

    /// A plugin's segment reports what the plugin is doing, which is part of
    /// the session the bar opens with.
    fn plugin(spec: &Arc<StatusItemSpec>) -> Self {
        Self {
            align: StatusAlign::Start,
            body: StatusBody::Plugin(spec.clone()),
        }
    }

    pub(crate) fn align(&self) -> StatusAlign {
        self.align
    }

    pub(crate) fn render(
        &self,
        invocation: &Invocation<'_>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        match &self.body {
            StatusBody::Builtin(segment) => (segment.render)(invocation, window, cx),
            StatusBody::Plugin(spec) => Some(views::status_item(spec, invocation, window, cx)),
        }
    }
}

impl Contribution {
    fn sidebar_block(block: &'static SidebarBlock) -> Self {
        Self {
            scope: ContributionScope::View(block.view),
            body: ContributionBody::Builtin(block.render),
        }
    }

    fn aside_block(block: &'static AsideBlock) -> Self {
        Self {
            scope: ContributionScope::Tab(block.tab),
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
        }
    }
}

/// One page of the settings surface. A page is built from the app rather than
/// from an [`Invocation`]: its fields read and write the config global
/// (`crate::config`), which is where a settings value lives, not the daemon's
/// snapshot.
pub(crate) struct SettingsSection {
    /// The page's stable name, so a caller that is not the nav (the status
    /// bar's diagnostic count, a future plugin contribution) can ask for it
    /// without knowing its position in the list.
    pub(crate) id: &'static str,
    /// Builds the page, from the config as it is now. `reset_armed` is the
    /// two-step confirm's state for pages that can put the file back (the
    /// config page's restore): the first press arms, the second writes, and
    /// `dispatch` is how a page's button reaches the shell.
    pub(crate) page: fn(&App, bool, &Dispatch) -> super::settings::Page,
}

/// One page the settings surface mounts, the shell's own or a plugin's
/// declared one. Like the status bar's slot, both kinds come through one type:
/// the dialog's nav lists them the same way, and a plugin's page is a page of
/// *this* dialog, not a second surface beside it.
pub(crate) struct SettingsContribution {
    body: SettingsBody,
}

enum SettingsBody {
    Builtin(&'static SettingsSection),
    Plugin(Arc<SettingsSectionSpec>),
}

impl SettingsContribution {
    fn builtin(section: &'static SettingsSection) -> Self {
        Self {
            body: SettingsBody::Builtin(section),
        }
    }

    fn plugin(spec: &Arc<SettingsSectionSpec>) -> Self {
        Self {
            body: SettingsBody::Plugin(spec.clone()),
        }
    }

    /// Builds the page as it is right now. A built-in page reads the config
    /// global; a plugin's reads the last answer of the command its declaration
    /// named, through the same data reader a panel's does.
    pub(crate) fn build_page(
        &self,
        invocation: &Invocation,
        _window: &mut Window,
        cx: &mut App,
        reset_armed: bool,
        dispatch: &Dispatch,
    ) -> super::settings::Page {
        match &self.body {
            SettingsBody::Builtin(section) => (section.page)(cx, reset_armed, dispatch),
            SettingsBody::Plugin(spec) => super::settings::plugin_page(spec, invocation),
        }
    }

    /// The qualified id of the plugin page, when one is: what the data reader
    /// is told is on screen while the dialog is open here. The shell's own
    /// pages read the config global and ask for no data.
    pub(crate) fn plugin_page_id(&self) -> Option<String> {
        match &self.body {
            SettingsBody::Builtin(_) => None,
            SettingsBody::Plugin(spec) => Some(spec.qualified_id()),
        }
    }
}

/// What the shell can be asked to do, by a chord or by a click on a
/// contribution's element. Cloning one is the normal case: an element that
/// dispatches a request has to own it for as long as the frame's tree lives.
#[derive(Clone)]
pub(crate) enum ShellRequest {
    /// Bring one sidebar view to the front. The shell's own state, so the
    /// daemon never hears it.
    ShowView(SidebarView),
    /// Bring one tab of the right-hand panel to the front, and open the panel
    /// if it is closed: a tab nobody can see is not what a click asked for.
    ShowAsideTab(AsideTab),
    /// Bring one page of the settings dialog to the front.
    ShowSettingsPage(usize),
    /// A command from the directory.
    Command(Command),
    /// Bring a workspace to the front.
    FocusWorkspace(String),
    /// Bring a tab to the front.
    FocusTab(String),
    /// Put a tab or a workspace into its inline rename editor. The shell owns
    /// the editor; the daemon hears the committed name, nothing while it is
    /// typed.
    StartRename(RenameSubject),
    /// Write the rename editor's text as the subject's new name. An empty text
    /// cancels: the daemon keeps an empty custom name as it is given, so
    /// clearing a name must not be one keystroke away.
    CommitRename,
    /// Leave the rename editor without writing anything.
    CancelRename,
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
    /// Clear the status bar's unread count for terminal bells and
    /// notifications that asked for attention.
    AcknowledgeAlerts,
    /// The config page's restore, two-step: the first press arms the confirm,
    /// the second writes the defaults (the window's own geometry is not the
    /// settings surface's to throw away).
    ResetConfigDefaults,
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
    /// Show the plugin manager, on one plugin's page when the click named one.
    OpenPluginManager(Option<String>),
    /// Close the plugin manager.
    ClosePluginManager,
    /// Bring one plugin's page to the front in the manager.
    SelectPlugin(String),
    /// Turn a linked plugin on or off, which the daemon decides.
    SetPluginEnabled { plugin_id: String, enabled: bool },
    /// Forget a linked plugin. Its directory is untouched.
    UnlinkPlugin(String),
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
    /// Show one slice of the event stream behind its chip. The stream reads
    /// everything either way; this is the view, not the subscription.
    SetEventFilter(crate::events::EventFilter),
    /// Read the tail of every running agent's pane, for the cockpit's
    /// side-by-side view.
    CompareFleet,
    /// Read one agent's tail into the comparison: a fleet card's own tail
    /// button, which asks about that agent alone.
    ReadTail(String),
    /// Drop the fleet's transitions. The subscriptions themselves keep running.
    ClearFleet,
}

/// What an inline rename is editing. The shell holds at most one editor at a
/// time; the subject only names which row of which surface it sits in.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RenameSubject {
    Tab(String),
    Workspace(String),
}

/// A request's way back into the shell. Shared rather than borrowed because
/// every element that dispatches one has to own it for as long as the frame's
/// tree lives.
pub(crate) type Dispatch = Rc<dyn Fn(ShellRequest, &mut Window, &mut App)>;

/// Everything a contribution may read while it builds its element. A built-in
/// and a plugin contribution receive this same shape, so neither can depend on
/// being the first one mounted (PLAN.md §5.9).
pub(crate) struct Invocation<'a> {
    /// The conversation panel, which the shell owns and the panel's first tab
    /// mounts. An entity, not a frame-built block: what the composer holds
    /// outlives the panel's closing, and opening the panel can focus it.
    pub(crate) chat: &'a Entity<ChatPanel>,
    pub(crate) snapshot: Option<&'a ClientShellSnapshot>,
    pub(crate) surface: Option<&'a PaneSurfaceFrame>,
    pub(crate) status: ConnectionStatus,
    /// Why the daemon is unreachable, when it is.
    pub(crate) status_error: Option<&'a str>,
    /// Why the daemon refused the last command, until one is accepted.
    pub(crate) rejection: Option<&'a str>,
    /// The prefix key's label, named by the keymap; always present.
    pub(crate) prefix: &'a str,
    /// Whether the prefix has been typed and the next key is its chord.
    pub(crate) prefix_armed: bool,
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
    /// Terminal bells and notifications that asked for attention, until the
    /// status bar's alert segment is clicked.
    pub(crate) bells: u64,
    pub(crate) attention: u64,
    /// The inline rename editor, when a tab or a workspace row is being
    /// renamed: the subject and the editor's own state.
    pub(crate) rename: Option<&'a (RenameSubject, Entity<InputState>)>,
    pub(crate) theme: &'a Theme,
    pub(crate) dispatch: &'a Dispatch,
    /// The height the sidebar's list rows are drawn at, from the config's
    /// density: the tree rows, the agent rows and their tab sub-rows.
    pub(crate) row_height: f32,
}

/// Where the shell mounts its contributions. The surface has six slots — the
/// sidebar's blocks, the right-hand panel's blocks, the status bar's segments,
/// the settings surface's pages, the command directory the palette and keymap
/// read, and the MCP tools — and this is the one place that knows what fills
/// them. A plugin's half of a slot is read off the same declarations for every
/// slot; the actions' one derivation (`crate::plugins::actions::Catalog`) is
/// what the command and MCP slots' plugin halves come from, and the settings
/// slot's is declared here.
pub(crate) struct Registry {
    panels: Vec<Contribution>,
    aside: Vec<Contribution>,
    status: Vec<StatusContribution>,
    settings: Vec<SettingsContribution>,
}

impl Registry {
    /// The shell's own capabilities and every plugin's declared UI, in one
    /// order: the shell's blocks first, then the plugins', in the order the
    /// daemon lists them. Rebuilt whenever the plugin registry is re-read,
    /// because a plugin's contributions appear and vanish with the plugin.
    pub(crate) fn new(ui: &UiContributions) -> Self {
        let mut panels = sidebar_blocks(super::panels::PANELS);
        // The cockpit's own view is the rail's sixth: the roster with the
        // transitions beside it, where the aside's tab holds the tails.
        panels.extend(sidebar_blocks(super::cockpit::SIDEBAR));
        // The plugin host's own blocks are the shell's blocks: they report what
        // is installed and what it ran, and they mount before anything a plugin
        // declares about itself.
        panels.extend(sidebar_blocks(crate::plugins::panels::PANELS));
        panels.extend(ui.sidebar_items.iter().map(|item| Contribution {
            // A plugin's sidebar item belongs to the plugin view: the shell's
            // own views are about the daemon, this one is about what was added.
            scope: ContributionScope::View(SidebarView::Plugins),
            body: ContributionBody::SidebarItem(item.clone()),
        }));
        let mut aside = aside_blocks(super::chat::ASIDE);
        aside.extend(aside_blocks(super::cockpit::ASIDE));
        aside.extend(ui.panels.iter().map(|panel| Contribution {
            // A panel a plugin declared is what the panel's third tab is for.
            scope: ContributionScope::Tab(AsideTab::Panels),
            body: ContributionBody::Panel(panel.clone()),
        }));
        let mut status: Vec<StatusContribution> = super::panels::STATUS_SEGMENTS
            .iter()
            .map(StatusContribution::builtin)
            .collect();
        status.extend(ui.status_items.iter().map(StatusContribution::plugin));
        // The settings surface's own pages first, so a page's position among
        // the built-ins never moves when a plugin links: the status bar asks
        // for the config page by index, and that ask stays true.
        let mut settings: Vec<SettingsContribution> = super::settings::SETTINGS_SECTIONS
            .iter()
            .map(SettingsContribution::builtin)
            .collect();
        settings.extend(
            ui.settings_sections
                .iter()
                .map(SettingsContribution::plugin),
        );
        Self {
            panels,
            aside,
            status,
            settings,
        }
    }

    /// The sidebar's blocks, top to bottom, for one view. Filtering here is
    /// what lets the rail own the sidebar's composition: the shell's render
    /// never decides which block is visible.
    pub(crate) fn panels_in(&self, view: SidebarView) -> impl Iterator<Item = &Contribution> {
        self.panels
            .iter()
            .filter(move |panel| panel.scope == ContributionScope::View(view))
    }

    /// The right-hand panel's blocks for one tab, top to bottom. Unlike the
    /// sidebar's, these are only built while their tab is in front.
    pub(crate) fn aside_in(&self, tab: AsideTab) -> impl Iterator<Item = &Contribution> {
        self.aside
            .iter()
            .filter(move |panel| panel.scope == ContributionScope::Tab(tab))
    }

    /// The status bar's segments, left to right. The bar splits them at
    /// [`StatusAlign`], so this is the composition order on each side.
    pub(crate) fn status_segments(&self) -> &[StatusContribution] {
        &self.status
    }

    /// The settings surface's pages, in the order it lists them: the shell's
    /// own first, then whatever the plugins declare.
    pub(crate) fn settings_sections(&self) -> &[SettingsContribution] {
        &self.settings
    }
}

fn sidebar_blocks(blocks: &'static [SidebarBlock]) -> Vec<Contribution> {
    blocks.iter().map(Contribution::sidebar_block).collect()
}

fn aside_blocks(blocks: &'static [AsideBlock]) -> Vec<Contribution> {
    blocks.iter().map(Contribution::aside_block).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn registry() -> Registry {
        Registry::new(&UiContributions::default())
    }

    fn with_ui(text: &str) -> Registry {
        Registry::new(&crate::plugins::ui_manifest::parse(
            "a.b",
            std::path::Path::new("/tmp/zenix-ui-root"),
            text,
        ))
    }

    #[test]
    fn every_sidebar_view_has_at_least_one_block() {
        let registry = registry();
        for view in SidebarView::ALL {
            assert!(
                registry.panels_in(view).next().is_some(),
                "{view:?} has no block"
            );
        }
        assert!(registry.aside_in(AsideTab::Chat).next().is_some());
        assert!(registry.aside_in(AsideTab::Fleet).next().is_some());
        assert!(!registry.status_segments().is_empty());
    }

    #[test]
    fn the_settings_surface_mounts_at_least_one_page() {
        assert!(!registry().settings_sections().is_empty());
    }

    /// The one thing a plugin's declaration cannot do is pick its slot freely:
    /// a sidebar item goes to the sidebar, a panel to the right-hand panel, a
    /// status item to the status bar, and a settings section to the settings
    /// surface — each behind the shell's own contributions.
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

[[settings_section]]
id = "cache"
title = "Cache"
"#,
        );
        let declared_board = |item: &Contribution| match &item.body {
            ContributionBody::SidebarItem(spec) => spec.local_id == "board",
            ContributionBody::Panel(spec) => spec.local_id == "board",
            ContributionBody::Builtin(_) => false,
        };
        // The plugin's own row lands behind the host's blocks in the view the
        // host lives in, never in front of them.
        let plugins_view: Vec<Contribution> =
            registry.panels_in(SidebarView::Plugins).cloned().collect();
        assert!(plugins_view.last().is_some_and(&declared_board));
        assert!(
            plugins_view
                .iter()
                .rev()
                .skip(1)
                .any(|item| matches!(item.body, ContributionBody::Builtin(_)))
        );
        // A plugin's panel is what the panel's third tab opens with.
        let panels_tab: Vec<Contribution> = registry.aside_in(AsideTab::Panels).cloned().collect();
        assert!(panels_tab.first().is_some_and(declared_board));
        // A plugin's status segment closes the bar's opening side, behind the
        // segments that report what the session is.
        let status = registry.status_segments();
        let last = status.last().expect("the bar mounts a plugin segment");
        assert!(matches!(&last.body, StatusBody::Plugin(spec) if spec.local_id == "wip"));
        assert!(matches!(
            status.first().map(|segment| &segment.body),
            Some(StatusBody::Builtin(_))
        ));
        // A plugin's settings page closes the surface's list, behind every
        // page the shell is born with — whose order never moves, because the
        // status bar asks for the config page by position in it.
        let settings = registry.settings_sections();
        let last = settings.last().expect("the surface mounts a plugin page");
        assert!(matches!(&last.body, SettingsBody::Plugin(spec) if spec.local_id == "cache"));
        assert!(
            matches!(&last.body, SettingsBody::Plugin(spec) if spec.qualified_id() == "a.b:settings_section:cache")
        );
        for (position, section) in crate::shell::settings::SETTINGS_SECTIONS.iter().enumerate() {
            match &settings[position].body {
                SettingsBody::Builtin(builtin) => assert_eq!(builtin.id, section.id),
                SettingsBody::Plugin(_) => {
                    panic!("a plugin page moved in front of {:?}", section.id)
                }
            }
        }
        // Each view and shell tab opens with the shell's own block.
        for view in [
            SidebarView::Workspaces,
            SidebarView::Agents,
            SidebarView::Events,
        ] {
            let first = registry
                .panels_in(view)
                .next()
                .unwrap_or_else(|| panic!("{view:?} opens with no block"));
            assert!(
                matches!(first.body, ContributionBody::Builtin(_)),
                "{view:?}"
            );
        }
        for tab in [AsideTab::Chat, AsideTab::Fleet] {
            let first = registry
                .aside_in(tab)
                .next()
                .unwrap_or_else(|| panic!("{tab:?} opens with no block"));
            assert!(
                matches!(first.body, ContributionBody::Builtin(_)),
                "{tab:?}"
            );
        }
    }
}
