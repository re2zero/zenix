//! The workspace shell: a title bar, a sidebar of workspaces, a tab strip,
//! the terminal grid, and a status bar.
//!
//! The daemon owns the layout: the shell draws one composite surface and asks
//! the daemon to create, close and focus workspaces, tabs and panes. Local
//! state is only what the daemon has no opinion about — the sidebar's
//! visibility and the chrome's light/dark mode.
//!
//! Structure follows herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/window.rs`
//! in shape, at skeleton scale by decision (D3): the rich sidebar rows, the
//! worktree grouping, the browser tabs, the menus and the command palette
//! stay upstream.

pub(crate) mod chat;
pub(crate) mod cockpit;
pub(crate) mod commands;
pub(crate) mod empty;
pub(crate) mod keymap;
pub(crate) mod palette;
pub(crate) mod panel_resize;
pub(crate) mod panels;
pub(crate) mod registry;
pub(crate) mod reorder;
pub(crate) mod settings;

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, Theme, ThemeMode, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellTab, ClientShellWorkspace,
};

use crate::ai::endpoint::ApiEndpoint;
use crate::config::{
    self, ASIDE_WIDTH_DEFAULT, Appearance, ConfigStore, Layout, SIDEBAR_WIDTH_DEFAULT,
    TerminalConfig,
};
use crate::events::{EventState, Stream};
use crate::fleet::{FleetState, Tails, read_tails};
use crate::herdr::Connection;
use crate::herdr::api::ApiClient;
use crate::i18n::t;
use crate::layout::{self, LayoutState};
use crate::plugins::actions::Catalog;
use crate::plugins::host::{PluginHost, PluginState};
use crate::plugins::manager::{self, Manager};
use crate::plugins::model::Availability;
use crate::plugins::trust_view::{self, TrustReview};
use crate::plugins::ui_data::{self, UiData, UiDataHost};
use crate::plugins::ui_manifest::UiContributions;
use crate::terminal::view::TerminalView;
use crate::terminal::{CursorShape, Theme as TerminalTheme};
use commands::{COMMANDS, Command};
use keymap::{Chord, Keymap, RunCommand};
use palette::{Entry, Palette, PlanRow};
use panel_resize::{ASIDE, AsideDrag, Edge, PanelWidth, SIDEBAR, SidebarDrag};
use registry::{
    AsideTab, Dispatch, Invocation, Registry, RenameSubject, ShellRequest, SidebarView,
    StatusAlign, StatusContribution,
};

const TITLE_BAR_HEIGHT: f32 = 38.;
const TAB_STRIP_HEIGHT: f32 = 40.;
/// The status bar's height: one row of 11px text with room for its dividers.
const STATUS_BAR_HEIGHT: f32 = 26.;
/// The right-hand panel's head: the tab strip above whatever tab is in front.
const ASIDE_HEAD_HEIGHT: f32 = 34.;
/// The activity rail: one column of view buttons, left of the sidebar.
const RAIL_WIDTH: f32 = 46.;
/// The sidebar's head: the view's identity strip, one row above its blocks.
const SIDEBAR_HEAD_HEIGHT: f32 = 34.;
/// The sidebar's foot: the one line naming where the view's rows come from.
const SIDEBAR_FOOT_HEIGHT: f32 = 24.;

pub(crate) struct ShellView {
    terminal: Entity<TerminalView>,
    connection: Connection,
    registry: Registry,
    /// Which sidebar view is in front. The rail changes it; the daemon has no
    /// opinion about what the client is looking at, so it is the shell's own.
    rail: SidebarView,
    keymap: Keymap,
    /// How wide the sidebar is drawn. Local like its visibility, but saved to
    /// the config when a drag ends, and restored from it when the settings
    /// panel changes it.
    sidebar: PanelWidth,
    /// A drag is in flight: the config must not overwrite the pointer.
    sidebar_dragging: bool,
    /// The right-hand panel's width, under the sidebar's rules but measured
    /// from the other edge.
    aside: PanelWidth,
    /// Which tab of the right-hand panel is in front. The strip changes it; a
    /// request that opens the panel for a plugin's panel names its own tab.
    aside_tab: AsideTab,
    aside_dragging: bool,
    /// The settings surface is over the shell, and has the keyboard.
    settings_open: bool,
    /// Which page of the settings dialog is in front. Local like the rail's
    /// view; the page list writes it through a request.
    settings_page: usize,
    /// The config page's restore confirm, armed by its first press and spent
    /// by its second. Cleared when the dialog closes, so an armed confirm
    /// never waits for a click that comes much later.
    settings_reset_armed: bool,
    /// The keyboard while the settings surface is open, so the pane beside it
    /// stops receiving keystrokes.
    settings_focus: FocusHandle,
    /// The command palette: the shell's own directory plus the daemon's plugin
    /// actions, over the shell while it is open. Its query input holds the
    /// keyboard while it is up.
    palette: Palette,
    /// The pre-link review: a path is typed into it, and what that path would
    /// run is shown before anything is linked.
    trust: TrustReview,
    /// The plugin manager: the registry one plugin at a time.
    manager: Manager,
    /// The keyboard while the review is open.
    trust_focus: FocusHandle,
    manager_focus: FocusHandle,
    /// The installed plugins, and where an action's outcome appears. Nothing
    /// pushes a plugin change, so this is read rather than painted from a
    /// stream, and refreshed on demand — see `crate::plugins::host`.
    plugins: PluginHost,
    /// The reader behind the data a plugin's declared panels and status items
    /// show: one command per declaration, run off the UI thread.
    ui_data_host: UiDataHost,
    /// What those commands last printed, as of the last answer. A field rather
    /// than a per-frame read, so a paint clones nothing.
    ui_data: UiData,
    /// The project the focused pane belongs to and the layout template found in
    /// it. The walk and the read happen when the cwd moves, not every frame.
    layout: LayoutState,
    /// The daemon's event stream, and the last of it this shell read. The worker
    /// reads the socket; a paint clones its ring rather than locking it.
    event_stream: Stream,
    events: EventState,
    /// What each running agent is doing, and the transitions it went through:
    /// one subscription per agent pane, opened and dropped as the fleet grows
    /// and shrinks.
    fleet_stream: crate::fleet::Stream,
    fleet: FleetState,
    /// The tails the cockpit last read, by pane. Read on demand, not pushed.
    tails: Tails,
    /// Why the last reorder on the API lane failed. The endpoint lane reports
    /// its own refusals into the snapshot; `workspace.move` only exists on the
    /// API lane, so its failure needs somewhere to be said.
    move_error: Option<String>,
    /// The inline rename editor, when a tab or a workspace row is being
    /// renamed: the subject and the editor's own state.
    rename: Option<(RenameSubject, Entity<InputState>)>,
    /// The prefix has been typed and the next keystroke is its chord.
    prefix_armed: bool,
    /// The terminal's last drawn revision, so a paint that changed nothing
    /// else does not rebuild the sidebar.
    drawn: u64,
    _observe: Subscription,
    _prefix: Subscription,
    _config: Subscription,
    /// Wakes the shell when the plugin registry moved. Held (not read) so the
    /// drain lives exactly as long as the window.
    _plugin_pulses: Task<()>,
    /// The same, for the data a declaration's command printed.
    _ui_data_pulses: Task<()>,
    /// The same, for the daemon's event stream.
    _event_pulses: Task<()>,
    /// The same, for the fleet's status changes.
    _fleet_pulses: Task<()>,
    /// The in-process MCP orchestrator agents connect to. Held (not read) so it
    /// lives exactly as long as the window: dropping the shell stops it.
    _orchestrator: Option<crate::mcp::server::McpOrchestrationServer>,
}

impl ShellView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(TerminalView::new);
        let connection = terminal.read(cx).connection();
        let observe = cx.observe(&terminal, |shell, terminal, cx| {
            let revision = terminal.read(cx).live().revision;
            let cwd = terminal
                .read(cx)
                .live()
                .snapshot
                .as_deref()
                .and_then(layout_cwd)
                .map(|cwd| cwd.to_owned());
            let drawn = revision != shell.drawn;
            // The cockpit follows the fleet: the daemon pushes snapshots, so the
            // agent list a new agent appears in is the same push that says which
            // panes to listen to. A set that did not move costs nothing.
            let panes: Vec<String> = terminal
                .read(cx)
                .live()
                .snapshot
                .as_deref()
                .map(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .map(|agent| agent.pane_id.clone())
                        .collect()
                })
                .unwrap_or_default();
            shell.fleet_stream.watch(panes);
            // The palette reads the session too: a workspace or an agent the
            // push brought in (or took away) is a row the open palette does
            // not yet draw, or one its selection may point past.
            if shell.palette.is_open() {
                shell.palette_sync(cx);
            }
            // A scan is a handful of directory checks and one small file, so it
            // runs when the focused pane's cwd moves and not on every revision.
            let mut scanned = false;
            if let Some(cwd) = cwd.as_deref()
                && Some(cwd) != shell.layout.cwd.as_deref()
            {
                shell.layout.scan(Path::new(cwd));
                scanned = true;
            }
            if drawn {
                shell.drawn = revision;
            }
            if drawn || scanned {
                cx.notify();
            }
        });
        let prefix = Self::intercept_prefix(cx);
        let config = Self::follow_config(cx);
        // The plugin registry, read once at start and then on demand: the
        // daemon pushes nothing when a plugin is linked, enabled or run.
        let (plugins, plugin_pulses) = PluginHost::start();
        let plugin_pulses = Self::follow_plugins(cx, plugin_pulses);
        let (ui_data_host, ui_data_pulses) = UiDataHost::start();
        let ui_data_pulses = Self::follow_ui_data(cx, ui_data_pulses);
        // The daemon's events arrive on their own, so this is the one stream the
        // shell paints from rather than reads on demand.
        let (event_stream, event_pulses) = Stream::start();
        let event_pulses = Self::follow_events(cx, event_pulses);
        let (fleet_stream, fleet_pulses) = crate::fleet::Stream::start();
        let fleet_pulses = Self::follow_fleet(cx, fleet_pulses);
        let mut sidebar = SIDEBAR;
        sidebar.restore(config::get(cx).layout.sidebar_width);
        let mut aside = ASIDE;
        aside.restore(config::get(cx).layout.aside_width);
        // The orchestrator's pane commands follow the daemon's focused
        // workspace (an empty id), because the first snapshot has not landed yet.
        let orchestrator = crate::mcp::start_orchestrator(String::new());
        let mut shell = Self {
            terminal,
            connection,
            registry: Registry::new(&plugins.state().ui),
            rail: SidebarView::Workspaces,
            keymap: Keymap::default(),
            sidebar,
            sidebar_dragging: false,
            aside,
            aside_tab: AsideTab::Chat,
            aside_dragging: false,
            settings_open: false,
            settings_page: 0,
            settings_reset_armed: false,
            settings_focus: cx.focus_handle(),
            palette: Palette::default(),
            trust: TrustReview::default(),
            manager: Manager::default(),
            trust_focus: cx.focus_handle(),
            manager_focus: cx.focus_handle(),
            plugins,
            ui_data_host,
            ui_data: UiData::default(),
            layout: LayoutState::default(),
            event_stream,
            events: EventState::default(),
            fleet_stream,
            fleet: FleetState::default(),
            tails: Tails::new(),
            move_error: None,
            rename: None,
            prefix_armed: false,
            drawn: 0,
            _observe: observe,
            _prefix: prefix,
            _config: config,
            _plugin_pulses: plugin_pulses,
            _ui_data_pulses: ui_data_pulses,
            _event_pulses: event_pulses,
            _fleet_pulses: fleet_pulses,
            _orchestrator: orchestrator,
        };
        shell.sync_terminal_theme(cx);
        shell.sync_terminal_metrics(cx);
        shell
    }

    /// Keep the shell in step with the config: the settings panel can change
    /// the sidebar's width, the palettes and the locale while the shell is
    /// drawn beside it. A drag in flight owns the width until it ends.
    fn follow_config(cx: &mut Context<Self>) -> Subscription {
        cx.observe_global::<ConfigStore>(|shell, cx| {
            if !shell.sidebar_dragging {
                shell.sidebar.restore(config::get(cx).layout.sidebar_width);
            }
            if !shell.aside_dragging {
                shell.aside.restore(config::get(cx).layout.aside_width);
            }
            shell.sync_terminal_theme(cx);
            shell.sync_terminal_metrics(cx);
            cx.notify();
        })
    }

    /// Repaint when the plugin registry moved, and remount what it declares:
    /// the shell's contributions include a plugin's, so the registry is rebuilt
    /// in this event, never during a paint. The worker thread already read the
    /// registry; this only wakes the shell, which reads the cache itself.
    fn follow_plugins(cx: &mut Context<Self>, pulses: crossbeam_channel::Receiver<()>) -> Task<()> {
        cx.spawn(async move |shell, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                if shell
                    .update(cx, |shell, cx| shell.read_plugins(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// Take the plugin registry the worker read, mount the UI it declares, and
    /// ask for the data those declarations name — a declaration that has just
    /// appeared has no answer yet.
    fn read_plugins(&mut self, cx: &mut Context<Self>) {
        let state = self.plugins.state();
        self.registry = Registry::new(&state.ui);
        // A chord a plugin declares is this client's to bind: the catalog is the
        // same list the palette draws and the MCP bridge exposes, so a plugin
        // that left the registry takes its chord with it.
        let catalog = Catalog::build(&state.actions, &state.ui).runnable(&state.plugins);
        self.keymap.set_plugins(&catalog);
        self.request_ui_data(&state.ui, false, cx);
        // The review is where a link's answer belongs while it is open: the
        // sidebar's log block is behind it, and a refused link leaves no log
        // entry at all.
        if self.trust.is_open()
            && let Some(last) = state.last.clone()
        {
            self.trust.note(last);
        }
        cx.notify();
    }

    /// Repaint when a declaration's command answered. The answer itself is read
    /// from the reader's cache, which is the only copy of it there is.
    fn follow_ui_data(cx: &mut Context<Self>, pulses: crossbeam_channel::Receiver<()>) -> Task<()> {
        cx.spawn(async move |shell, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                let alive = shell
                    .update(cx, |shell, cx| {
                        shell.ui_data = shell.ui_data_host.state();
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
    }

    /// Repaint when the daemon announced something. The events themselves are
    /// read from the stream's ring, which is the only copy of them there is.
    fn follow_events(cx: &mut Context<Self>, pulses: crossbeam_channel::Receiver<()>) -> Task<()> {
        cx.spawn(async move |shell, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                if shell.update(cx, |shell, cx| shell.read_events(cx)).is_err() {
                    break;
                }
            }
        })
    }

    /// Take what the stream has read and paint it. An event a plugin hooks is
    /// the one moment the log ring has to be read again: the daemon has just run
    /// something for it, and the ring's own fallback tick is a slow way to find
    /// out what happened.
    fn read_events(&mut self, cx: &mut Context<Self>) {
        self.events = self.event_stream.state();
        if let Some(event) = self.events.events.front()
            && self.plugins.hooks_on(&event.kind)
        {
            self.plugins.refresh_logs();
        }
        cx.notify();
    }

    /// Repaint when an agent's status moved. The transitions are read from the
    /// stream's own cache, which is the only copy of them there is.
    fn follow_fleet(cx: &mut Context<Self>, pulses: crossbeam_channel::Receiver<()>) -> Task<()> {
        cx.spawn(async move |shell, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                let alive = shell
                    .update(cx, |shell, cx| {
                        shell.fleet = shell.fleet_stream.state();
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
    }

    /// Read every agent's tail, so the fleet can be compared rather than
    /// visited. The reads are a blocking call each on the API lane, so they run
    /// off the UI thread; the panel shows what came back until it is asked
    /// again.
    fn compare_fleet(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.terminal.read(cx).live().snapshot.clone() else {
            return;
        };
        let panes: Vec<String> = snapshot
            .agents
            .iter()
            .map(|agent| agent.pane_id.clone())
            .collect();
        if panes.is_empty() {
            return;
        }
        let endpoint = ApiEndpoint::local();
        cx.spawn(async move |shell, cx| {
            let tails = cx
                .background_executor()
                .spawn(async move { read_tails(&endpoint, &panes) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.tails = tails;
                cx.notify();
            });
        })
        .detach();
    }

    /// Move a workspace on the API lane. The endpoint protocol the grid paints
    /// through carries `workspace.move_block` but not `workspace.move`, and the
    /// sidebar's order is the daemon's, so the request goes where the method
    /// is. A refusal is kept for the status bar: a drag that silently did
    /// nothing would look like a gesture the shell dropped.
    fn move_workspace(
        &mut self,
        workspace_id: String,
        insert_index: usize,
        cx: &mut Context<Self>,
    ) {
        let api = ApiClient::local();
        self.move_error = None;
        cx.spawn(async move |shell, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    api.call(
                        "workspace.move",
                        serde_json::json!({
                            "workspace_id": workspace_id,
                            "insert_index": insert_index,
                        }),
                    )
                })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.move_error = outcome.err().map(|error| error.to_string());
                cx.notify();
            });
        })
        .detach();
    }

    /// Tell the reader what is on screen and how often each declaration wants
    /// to be read. A panel is asked for only while the aside shows the tab it
    /// mounts in — a closed panel is not worth a process every ten seconds —
    /// and a status segment always, because the bar is never hidden. `force`
    /// re-reads what is on screen now, which is what a refresh click means.
    fn request_ui_data(&self, ui: &UiContributions, force: bool, cx: &App) {
        let live = self.terminal.read(cx).live().clone();
        let context = Arc::new(ui_data::context(live.snapshot.as_deref()));
        let screen = ui_data::OnScreen {
            panels: config::get(cx).layout.aside_visible && self.aside_tab == AsideTab::Panels,
        };
        self.ui_data_host
            .sync(ui_data::loads(ui, screen), context, force);
    }

    /// The terminal keeps the keyboard: the shell has no focus of its own.
    pub(crate) fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.terminal.read(cx).focus_handle().clone()
    }

    /// GPUI calls keystroke interceptors before any binding or key handler, so
    /// an armed prefix claims the next key wherever focus is. Interceptors are
    /// app-wide; with one window (M4) there is nothing to filter by, and a
    /// second window must claim only its own keystrokes when it arrives.
    fn intercept_prefix(cx: &mut Context<Self>) -> Subscription {
        let view = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
            let _ = view.update(cx, |shell, cx| {
                shell.prefix_keystroke(&event.keystroke, window, cx);
            });
        })
    }

    fn prefix_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The settings surface, the palette and the review own the keyboard
        // while they are open: their inputs must see every key, including the
        // prefix.
        if self.overlay_owns_keyboard() {
            return;
        }
        // A plugin's chord that carries a modifier of its own is claimed outright,
        // before the prefix is even considered: that is what declaring
        // `ctrl-shift-l` asks for, and it is the only case where a chord can be
        // pressed without disarming anything.
        if let Some(chord) = self.keymap.claimed(keystroke) {
            cx.stop_propagation();
            self.run_chord(chord, window, cx);
            return;
        }
        let is_prefix = self.keymap.is_prefix(keystroke);
        if !self.prefix_armed {
            if is_prefix {
                self.prefix_armed = true;
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let chord = self.keymap.chord(keystroke);
        self.prefix_armed = false;
        cx.notify();
        // Typing the prefix twice sends it on as ordinary input rather than
        // swallowing it, so a pane that binds the prefix still receives it.
        if is_prefix {
            return;
        }
        cx.stop_propagation();
        if let Some(chord) = chord {
            self.run_chord(chord, window, cx);
        }
        // A chord with nothing bound to it is swallowed, as in herdr.
    }

    /// Run what a chord points at. A plugin action goes out over the same
    /// dispatch a palette row uses, so the two cannot behave differently; a
    /// chord only reaches an action the current focus offers, and is swallowed
    /// otherwise, exactly as an unbound chord is.
    fn run_chord(&mut self, chord: Chord, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(command) = chord.command() {
            self.run(command, window, cx);
            return;
        }
        let Some(action) = chord.plugin().cloned() else {
            return;
        };
        let availability = Availability::of(self.terminal.read(cx).live().snapshot.as_deref());
        if !action.is_available(availability) {
            return;
        }
        self.dispatch(
            ShellRequest::PluginAction {
                action,
                source: "key",
            },
            window,
            cx,
        );
    }

    /// Run one directory command: locally when the shell owns it, otherwise as
    /// the daemon method the snapshot resolves it to.
    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::ToggleSidebar => {
                let visible = !config::get(cx).layout.sidebar_visible;
                config::update(cx, |config| config.layout.sidebar_visible = visible);
            }
            // The right-hand panel: shown or hidden, width remembered, exactly
            // as the sidebar's own toggle works.
            Command::ToggleAside => {
                let visible = !config::get(cx).layout.aside_visible;
                config::update(cx, |config| config.layout.aside_visible = visible);
                // Opening or closing the panel changes what can be seen, and
                // with it what the reader is responsible for.
                self.request_ui_data(&self.plugins.state().ui, false, cx);
            }
            // The mode the chrome is drawn in is a setting, so the title bar's
            // button and the settings panel's item are the same write; the
            // palette pair follows through the config observer.
            Command::ToggleTheme => {
                let mode = if cx.theme().is_dark() {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                };
                config::update(cx, |config| config.appearance.theme_mode = mode);
            }
            Command::ToggleSettings => self.toggle_settings(window, cx),
            Command::TogglePalette => self.toggle_palette(window, cx),
            // The plugin surface is read, not pushed: both blocks a user can
            // see ask for the same refresh, and it is the registry and the log
            // ring together, because the reason to look is the same.
            Command::RefreshPlugins => self.refresh_plugins(cx),
            // Linking runs a directory's commands as the user, so the review
            // comes first, from wherever the command was reached.
            Command::LinkPlugin => self.dispatch(ShellRequest::OpenPluginTrust, window, cx),
            Command::PluginsManager => {
                self.dispatch(ShellRequest::OpenPluginManager(None), window, cx)
            }
            Command::ClosePopup => self.plugins.close_popup(),
            // The terminal's own scrollback chrome: the find field over the
            // focused pane, and keyboard copy mode. Both own the keyboard
            // while they are open, so the view is the only place that can
            // start them.
            Command::Find => self
                .terminal
                .update(cx, |terminal, cx| terminal.open_find(window, cx)),
            Command::CopyMode => self
                .terminal
                .update(cx, |terminal, cx| terminal.enter_copy_mode(window, cx)),
            // The template is the project's, not the daemon's: saving writes a
            // file, and restoring stays a command because `layout.apply` always
            // makes a new tab.
            Command::SaveLayout => self.save_layout(cx),
            Command::RestoreLayout => self.restore_layout(cx),
            // The cockpit's two local commands: read every tail, or drop the
            // timeline read so far.
            Command::CompareFleet => self.compare_fleet(cx),
            Command::ClearFleet => self.clear_fleet(cx),
            // Every other command is the daemon's. A command marked local with
            // no arm above would silently do nothing, so say so in tests.
            command => {
                debug_assert!(
                    !command.is_local(),
                    "{command:?} is local but has no arm in `run`"
                );
                self.send_command(command, cx);
            }
        }
    }

    /// Ask the daemon for one directory command: the method and its params come
    /// from the snapshot, so a closed tab or workspace cannot be addressed twice.
    fn send_command(&mut self, command: Command, cx: &mut Context<Self>) {
        let snapshot = self.terminal.read(cx).live().snapshot.clone();
        if let Some((method, params)) = snapshot
            .as_ref()
            .and_then(|snapshot| commands::request(command, snapshot))
        {
            self.connection.request(method, params);
        }
    }

    /// Show the settings surface, or hide it and give the keyboard back to the
    /// pane. Which one has the keyboard is the whole of the shell's interest in
    /// it: the panel keeps its own page selection.
    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        self.settings_reset_armed = false;
        if self.settings_open {
            let focus = self.settings_focus.clone();
            window.focus(&focus, cx);
        } else {
            let focus = self.terminal.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Show the palette, or hide it and give the keyboard back to the pane. The
    /// settings surface keeps the keyboard while it is open, so the palette
    /// does not open over it. Opening hands the keyboard to the query input —
    /// it is a real input, so IME and the caret are its own — and the first
    /// open subscribes to it once.
    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            return;
        }
        if self.palette.is_open() {
            self.palette.close();
            let focus = self.terminal.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        } else {
            let input = self.palette.open(window, cx);
            if self.palette.input_subscribed().is_none() {
                let subscription = cx.subscribe_in(
                    &input,
                    window,
                    |shell, _, event: &InputEvent, window, cx| {
                        shell.palette_input_event(event, window, cx);
                    },
                );
                self.palette.hold_subscription(subscription);
            }
            self.palette_sync(cx);
            let focus = input.read(cx).focus_handle(cx).clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Read the plugin surface again: what is installed, and what last ran.
    fn refresh_plugins(&self, cx: &mut Context<Self>) {
        self.plugins.refresh();
        self.plugins.refresh_logs();
        // A refresh is "read it again": a declaration's data is read again too,
        // which is the only way the numbers in a panel move.
        self.request_ui_data(&self.plugins.state().ui, true, cx);
        cx.notify();
    }

    /// Everything the palette offers: the whole command directory, then every
    /// plugin action the current focus can run. The directory is the same table
    /// the keymap and the sidebar read, so the palette cannot offer a command
    /// the shell has no way to run.
    ///
    /// The plugin rows are the catalog's entries offered in this focus — the
    /// same list the keymap binds chords from and the MCP bridge turns into
    /// tools. A plugin that left the registry leaves none of the three.
    fn palette_entries(&self, cx: &App) -> Vec<Entry> {
        let availability = Availability::of(self.terminal.read(cx).live().snapshot.as_deref());
        let plugins = self.plugins.state();
        let mut entries: Vec<Entry> = COMMANDS
            .iter()
            .map(|info| Entry::Command(info.command))
            .collect();
        // The session's workspaces come between the two: the section order is
        // the entry order, and both the shell's directory and the plugins'
        // actions bracket the session.
        if let Some(snapshot) = self.terminal.read(cx).live().snapshot.as_deref() {
            entries.extend(snapshot.workspaces.iter().map(|workspace| {
                Entry::Workspace {
                    workspace_id: workspace.workspace_id.clone(),
                    label: workspace.label.clone(),
                    branch: workspace.branch.clone(),
                    tabs: snapshot
                        .tabs
                        .iter()
                        .filter(|tab| tab.workspace_id == workspace.workspace_id)
                        .count(),
                }
            }));
        }
        entries.extend(
            Catalog::build(&plugins.actions, &plugins.ui)
                .runnable(&plugins.plugins)
                .offered(availability)
                .map(|entry| Entry::Plugin(entry.clone())),
        );
        entries
    }

    /// Re-derive the rows the open palette draws: the query or the entries
    /// moved under it. This runs from events — a keystroke in the query, a
    /// snapshot push — never from render, where the rows are only drawn.
    fn palette_sync(&mut self, cx: &mut Context<Self>) {
        if !self.palette.is_open() {
            return;
        }
        let entries = self.palette_entries(cx);
        let matches = self.palette.matches(&entries, cx);
        self.palette.sync_rows(palette::plan(&matches).len());
    }

    /// The query input's events. A change re-derives the rows and returns the
    /// selection to the first one — a selection is a view of the rows, not a
    /// fact of its own. Enter arrives here as `PressEnter` too, but it is
    /// already answered by the palette's own action (the input's single-line
    /// Enter propagates), so answering it here as well would run one row twice.
    fn palette_input_event(
        &mut self,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.palette.reset_selection();
                self.palette_sync(cx);
            }
            InputEvent::PressEnter { .. } => {}
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        }
    }

    /// Move the palette's selection a row, taking the scroll with it: the
    /// selected row is always one the palette has drawn.
    fn palette_step(&mut self, step: isize, cx: &mut Context<Self>) {
        let entries = self.palette_entries(cx);
        let matches = self.palette.matches(&entries, cx);
        if matches.is_empty() {
            return;
        }
        let rows = palette::plan(&matches);
        self.palette.step(step, rows.len());
        let selected = self.palette.selected();
        let list = self.palette.list();
        if let (Some(list), Some(row)) = (
            list,
            rows.iter()
                .position(|row| matches!(row, PlanRow::Item(index) if *index == selected)),
        ) {
            list.scroll_to_reveal_item(row);
        }
        cx.notify();
    }

    /// Up and down in the palette: one row at a time, stopping at the ends.
    fn palette_select_prev(
        &mut self,
        _: &palette::PaletteSelectPrev,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_step(-1, cx);
    }

    fn palette_select_next(
        &mut self,
        _: &palette::PaletteSelectNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_step(1, cx);
    }

    /// Enter in the palette: hand the row's request to the same dispatch a
    /// click uses, so the two cannot behave differently.
    fn palette_run(
        &mut self,
        _: &palette::PaletteRun,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.palette_entries(cx);
        let matches = self.palette.matches(&entries, cx);
        let Some(entry) = self.palette.selected_entry(&matches) else {
            return;
        };
        let request = entry.request();
        self.palette.note_run(entry);
        self.palette.close();
        self.dispatch(request, window, cx);
        // A request that opened an overlay — the settings surface, the
        // review, or the palette itself — claimed the keyboard itself
        // while it ran; handing it back to the pane here would take it
        // away again before the overlay has been used.
        if !self.overlay_owns_keyboard() {
            let focus = self.terminal.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Escape in the palette: a non-empty query clears first — the same
    /// two-step the command palettes the user knows speak — and an empty one
    /// hands the keyboard back to the pane.
    fn palette_dismiss(
        &mut self,
        _: &palette::PaletteDismiss,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.palette.input() else {
            return;
        };
        if !input.read(cx).value().trim().is_empty() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
            return;
        }
        self.toggle_palette(window, cx);
    }

    /// A keystroke GPUI bound directly, dispatched as its action. The prefix
    /// chords arrive through the interceptor instead, and both end in [`Self::run`].
    fn run_command(&mut self, action: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.run(action.command, window, cx);
    }

    /// Open the rename editor on a tab or a workspace, prefilled with the name
    /// the snapshot carries. A subject the snapshot no longer names is ignored:
    /// the row it sat in is gone.
    fn start_rename(
        &mut self,
        subject: RenameSubject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self
            .terminal
            .read(cx)
            .live()
            .snapshot
            .as_deref()
            .and_then(|snapshot| match &subject {
                RenameSubject::Tab(tab_id) => snapshot
                    .tabs
                    .iter()
                    .find(|tab| &tab.tab_id == tab_id)
                    .map(|tab| tab.label.clone()),
                RenameSubject::Workspace(workspace_id) => snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| &workspace.workspace_id == workspace_id)
                    .map(|workspace| workspace.label.clone()),
            });
        let Some(label) = label else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| input.set_value(label, window, cx));
        let focus = input.read(cx).focus_handle(cx).clone();
        self.rename = Some((subject, input));
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The editor's text becomes the subject's name over the endpoint. An
    /// empty editor cancels rather than wiping the name: the daemon keeps an
    /// empty custom name as it is given, so a cleared field would stick.
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((subject, input)) = self.rename.take() else {
            return;
        };
        let label = input.read(cx).value().trim().to_owned();
        if !label.is_empty() {
            match &subject {
                RenameSubject::Tab(tab_id) => {
                    self.connection.request(
                        herdr_client::Method::TabRename,
                        serde_json::json!({"tab_id": tab_id, "label": label}),
                    );
                }
                RenameSubject::Workspace(workspace_id) => {
                    self.connection.request(
                        herdr_client::Method::WorkspaceRename,
                        serde_json::json!({"workspace_id": workspace_id, "label": label}),
                    );
                }
            }
        }
        self.end_rename(window, cx);
    }

    /// Leave the rename editor without writing anything.
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rename = None;
        self.end_rename(window, cx);
    }

    /// The editor is gone; the pane gets the keyboard back.
    fn end_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.terminal.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Hand a request to the daemon, or run it locally. Both the keymap and a
    /// contribution's click arrive here, so the two cannot behave differently.
    fn dispatch(&mut self, request: ShellRequest, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            // The rail is the shell's own surface: showing another view moves
            // nothing on the daemon, it only changes what the sidebar mounts.
            ShellRequest::ShowView(view) => {
                self.rail = view;
                cx.notify();
            }
            // The same for the panel's tabs, except that a tab click also opens
            // a closed panel: the click named a tab, so hiding it would be a
            // gesture answered by nothing.
            ShellRequest::ShowAsideTab(tab) => {
                self.aside_tab = tab;
                config::update(cx, |config| config.layout.aside_visible = true);
                // The tab that just came forward may hold declarations nobody
                // has read yet: ask for them now rather than on the next refresh.
                self.request_ui_data(&self.plugins.state().ui, false, cx);
                cx.notify();
            }
            // A settings page is the dialog's own state; the surface stays
            // open, because the click named a page, not an exit.
            ShellRequest::ShowSettingsPage(page) => {
                self.settings_page = page;
                cx.notify();
            }
            ShellRequest::FocusWorkspace(workspace_id) => {
                self.connection.focus_workspace(&workspace_id)
            }
            ShellRequest::FocusTab(tab_id) => self.connection.focus_tab(&tab_id),
            // An inline rename is the shell's own editor: the daemon hears the
            // committed name, nothing while it is typed.
            ShellRequest::StartRename(subject) => self.start_rename(subject, window, cx),
            ShellRequest::CommitRename => self.commit_rename(window, cx),
            ShellRequest::CancelRename => self.cancel_rename(window, cx),
            // A drag lands on a gap; the daemon owns the order, so the request
            // is the same `tab.move` the move chords send.
            ShellRequest::MoveTab {
                tab_id,
                insert_index,
            } => {
                self.connection.request(
                    herdr_client::Method::TabMove,
                    serde_json::json!({"tab_id": tab_id, "insert_index": insert_index}),
                );
            }
            // `workspace.move` is not on the endpoint protocol; the API lane is
            // where the method exists.
            ShellRequest::MoveWorkspace {
                workspace_id,
                insert_index,
            } => self.move_workspace(workspace_id, insert_index, cx),
            ShellRequest::FocusPane(pane_id) => self.connection.focus_pane(&pane_id),
            // Clearing the unread count is the terminal's to do: it owns the
            // counter the status bar paints.
            ShellRequest::AcknowledgeAlerts => {
                self.terminal
                    .update(cx, |terminal, cx| terminal.acknowledge_alerts(cx));
            }
            // The restore is two presses on purpose: the first arms, the
            // second writes. The window's geometry is left alone — the
            // closing window owns it, and a reset that changed what the next
            // launch opens into would be the surface overstepping.
            ShellRequest::ResetConfigDefaults => {
                if self.settings_reset_armed {
                    self.settings_reset_armed = false;
                    config::update(cx, |config| {
                        config.appearance = Appearance::default();
                        config.terminal = TerminalConfig::default();
                        config.layout = Layout::default();
                    });
                } else {
                    self.settings_reset_armed = true;
                    cx.notify();
                }
            }
            // A local command is the shell's own, so it goes back through
            // [`Self::run`]: a palette row and a chord must end in the same
            // place, and the directory is the only thing that knows which
            // commands the daemon has no method for.
            ShellRequest::Command(command) if command.is_local() => self.run(command, window, cx),
            ShellRequest::Command(command) => self.send_command(command, cx),
            // Everything a plugin asks for leaves over the API lane, which is
            // this host's to own; the endpoint connection below never sees it.
            ShellRequest::PluginAction { action, source } => self.plugins.invoke(action, source),
            ShellRequest::OpenPluginPane {
                plugin_id,
                entrypoint,
                title,
                placement,
            } => self
                .plugins
                .open_pane(plugin_id, entrypoint, title, placement),
            ShellRequest::ClosePluginPane(pane_id) => self.plugins.close_pane(pane_id),
            // A declared sidebar line opens the panel its plugin declared, which
            // mounts in the right-hand panel: showing that panel is the whole
            // request.
            ShellRequest::ShowPluginPanels => {
                self.aside_tab = AsideTab::Panels;
                config::update(cx, |config| config.layout.aside_visible = true);
                self.request_ui_data(&self.plugins.state().ui, false, cx);
                cx.notify();
            }
            ShellRequest::RefreshPlugins => self.refresh_plugins(cx),
            ShellRequest::OpenPluginManager(start_at) => {
                self.manager.open(start_at.as_deref());
                let focus = self.manager_focus.clone();
                window.focus(&focus, cx);
                cx.notify();
            }
            ShellRequest::ClosePluginManager => {
                self.manager.close();
                let focus = self.terminal.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
                cx.notify();
            }
            ShellRequest::SelectPlugin(plugin_id) => {
                self.manager.select(&plugin_id);
                cx.notify();
            }
            ShellRequest::SetPluginEnabled { plugin_id, enabled } => {
                self.plugins.set_enabled(plugin_id, enabled);
                cx.notify();
            }
            ShellRequest::UnlinkPlugin(plugin_id) => {
                self.plugins.unlink(plugin_id);
                cx.notify();
            }
            ShellRequest::OpenPluginTrust => {
                self.trust.open();
                let focus = self.trust_focus.clone();
                window.focus(&focus, cx);
                cx.notify();
            }
            // Reading is the shell's own: it is one small file, and the answer
            // is what the review shows.
            ShellRequest::ReviewPlugin => {
                self.trust.review();
                cx.notify();
            }
            ShellRequest::OpenPluginTrustAt(dir) => {
                self.trust.open_at(std::path::Path::new(&dir));
                let focus = self.trust_focus.clone();
                window.focus(&focus, cx);
                cx.notify();
            }
            ShellRequest::LinkPlugin(path) => {
                self.trust
                    .note(rust_i18n::t!("shell.trust.linking", path = path).to_string());
                self.plugins.link(path);
                cx.notify();
            }
            ShellRequest::ClosePluginTrust => {
                self.trust.close();
                let focus = self.terminal.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
                cx.notify();
            }
            ShellRequest::SaveLayout => self.save_layout(cx),
            ShellRequest::RestoreLayout => self.restore_layout(cx),
            ShellRequest::CompareFleet => self.compare_fleet(cx),
            ShellRequest::ReadTail(pane_id) => self.read_tail(pane_id, cx),
            // Clearing is local, like the events ring's: the subscription keeps
            // running and the next status change arrives into an empty list.
            ShellRequest::ClearFleet => self.clear_fleet(cx),
            // Clearing is local: the subscription keeps running, and the next
            // event arrives into an empty ring.
            ShellRequest::ClearEvents => {
                self.event_stream.clear();
                self.events = self.event_stream.state();
                cx.notify();
            }
            // Filtering is local the same way: the stream reads everything and
            // the panel decides what of it to show.
            ShellRequest::SetEventFilter(filter) => {
                self.event_stream.set_filter(filter);
                self.events = self.event_stream.state();
                cx.notify();
            }
        }
    }

    /// Write the focused tab's layout into the project's file. The daemon answers
    /// with the tree and the ids it owns; the file keeps the same tree without
    /// them, so the template outlives this run.
    fn save_layout(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.terminal.read(cx).live().snapshot.clone();
        let Some(tab_id) = snapshot.and_then(|snapshot| snapshot.focused_tab_id.clone()) else {
            return;
        };
        let Some(root) = self.layout.root.clone() else {
            return;
        };
        let path = root.join(layout::DIR).join(layout::FILE_NAME);
        let client = ApiClient::local();
        // The API lane is a blocking socket call, so it runs off the UI thread as
        // every other call on it does.
        cx.spawn(async move |shell, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let template = layout::export(&client, &tab_id)?;
                    layout::write(&path, &template)?;
                    Ok(path)
                })
                .await;
            let outcome = outcome.map(|path| {
                rust_i18n::t!("layout.saved", path = path.display().to_string()).to_string()
            });
            let _ = shell.update(cx, |shell, cx| {
                shell.layout.record(outcome);
                // The file may not have existed before this save, so the block
                // shows what the project holds now.
                if shell.layout.error.is_none()
                    && let Some(cwd) = shell.layout.cwd.clone()
                {
                    shell.layout.scan(Path::new(&cwd));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Restore the project's template as a new tab in the focused workspace. The
    /// daemon never replaces a tab on an apply, so this only ever runs when a
    /// person asked for it.
    fn restore_layout(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.layout.template.clone() else {
            return;
        };
        let snapshot = self.terminal.read(cx).live().snapshot.clone();
        let Some(workspace_id) =
            snapshot.and_then(|snapshot| snapshot.focused_workspace_id.clone())
        else {
            return;
        };
        let client = ApiClient::local();
        cx.spawn(async move |shell, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { layout::apply(&client, &workspace_id, &template) })
                .await;
            let outcome = outcome.map(|description| {
                rust_i18n::t!("layout.restored", tab = description.tab_id).to_string()
            });
            let _ = shell.update(cx, |shell, cx| {
                shell.layout.record(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    /// Forget the transitions read so far. The subscriptions are untouched: the
    /// next status change still arrives, which is what makes this a view rather
    /// than a stop.
    fn clear_fleet(&mut self, cx: &mut Context<Self>) {
        self.fleet_stream.clear();
        self.fleet = self.fleet_stream.state();
        self.tails.clear();
        cx.notify();
    }

    /// Read one agent's tail into the comparison: a fleet card's tail button
    /// asks about that agent alone, where the head's compare reads them all.
    /// The answer lands in the same map, so a card and the compare cannot show
    /// two different truths about one pane.
    fn read_tail(&mut self, pane_id: String, cx: &mut Context<Self>) {
        let endpoint = ApiEndpoint::local();
        cx.spawn(async move |shell, cx| {
            let tails = cx
                .background_executor()
                .spawn(async move { read_tails(&endpoint, &[pane_id]) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.tails.extend(tails);
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether an overlay of the shell's holds the keyboard right now. Each one
    /// claims it while it opens, so nothing may hand it back to the pane while
    /// any of them is up — the shell's own chord interceptor defers to them the
    /// same way.
    fn overlay_owns_keyboard(&self) -> bool {
        self.settings_open
            || self.palette.is_open()
            || self.trust.is_open()
            || self.manager.is_open()
    }

    /// A keystroke while the manager is open: Escape closes it, and nothing else
    /// is typed into it — every command it offers is a click.
    fn manager_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.dispatch(ShellRequest::ClosePluginManager, window, cx);
        }
    }

    /// A keystroke while the review is open: Escape closes it, Enter reads the
    /// path or links it, and everything else is typed into the input.
    fn trust_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.dispatch(ShellRequest::ClosePluginTrust, window, cx);
            }
            "enter" => {
                let request = self.trust.enter();
                self.dispatch(request, window, cx);
            }
            _ => {
                if self.trust.keystroke(&event.keystroke) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }
        }
    }

    /// The terminal palette that matches the chrome's mode, named by the
    /// config so a mode's palette can be chosen in the settings panel. The
    /// gpui-kit theme is the only source of truth for the mode: a second local
    /// flag could disagree with it.
    fn sync_terminal_theme(&mut self, cx: &mut Context<Self>) {
        let config = config::get(cx);
        // The config's mode, not `cx.theme().is_dark()`: at construction the
        // global theme's change is still queued to the next frame, so reading
        // the theme here would pair a light chrome with the dark palette.
        let name = if config.appearance.theme_mode == ThemeMode::Dark {
            config.terminal.palette_dark.as_str()
        } else {
            config.terminal.palette_light.as_str()
        };
        let theme = TerminalTheme::builtin(name).unwrap_or_default();
        self.terminal
            .update(cx, |view, cx| view.set_theme(theme, cx));
    }

    /// Draw the terminal at the config's metrics and cursor shape, the same
    /// way the palette follows the mode: a settings change reaches the next
    /// frame, and the next frame re-sends the surface size the new cell asks
    /// for.
    fn sync_terminal_metrics(&mut self, cx: &mut Context<Self>) {
        let terminal = &config::get(cx).terminal;
        let (font_size, line_height) = (terminal.font_size, terminal.line_height);
        let cursor = CursorShape::from_name(&terminal.cursor);
        self.terminal.update(cx, |view, cx| {
            view.set_metrics(font_size, line_height, cursor, cx)
        });
    }

    /// Follow the pointer with the sidebar's edge, resolved against the
    /// sidebar's own bounds: the drag reports where the pointer is, not how far
    /// the panel should grow. The config is written once, when the drag ends.
    fn drag_sidebar(
        &mut self,
        event: &DragMoveEvent<SidebarDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_dragging = true;
        if self.sidebar.drag(event.bounds, event.event.position.x) {
            cx.notify();
        }
    }

    /// A drag is over: save the width it left, once, however many frames it
    /// took. The release can also land outside the shell's edge, so both
    /// `on_mouse_up` and `on_mouse_up_out` come here.
    fn settle_sidebar(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_dragging = false;
        if self.sidebar.take_unsaved() {
            let width = self.sidebar.chosen().unwrap_or(SIDEBAR_WIDTH_DEFAULT);
            config::update(cx, |config| config.layout.set_sidebar_width(width));
        }
    }

    /// A double-click on the grip puts the sidebar back to its default width,
    /// and the config with it.
    fn reset_sidebar(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.reset();
        self.sidebar.take_unsaved();
        config::update(cx, |config| {
            config.layout.set_sidebar_width(SIDEBAR_WIDTH_DEFAULT)
        });
    }

    /// The right-hand panel's edge, dragged the other way: its width runs from
    /// the pointer to the panel's own right edge.
    fn drag_aside(
        &mut self,
        event: &DragMoveEvent<AsideDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.aside_dragging = true;
        if self.aside.drag(event.bounds, event.event.position.x) {
            cx.notify();
        }
    }

    fn settle_aside(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.aside_dragging = false;
        if self.aside.take_unsaved() {
            let width = self.aside.chosen().unwrap_or(ASIDE_WIDTH_DEFAULT);
            config::update(cx, |config| config.layout.set_aside_width(width));
        }
    }

    fn reset_aside(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.aside.reset();
        self.aside.take_unsaved();
        config::update(cx, |config| {
            config.layout.set_aside_width(ASIDE_WIDTH_DEFAULT)
        });
    }
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live = self.terminal.read(cx).live().clone();
        let error = self.connection.footer();
        let theme = cx.theme().clone();
        let plugins = self.plugins.state();
        let sidebar_visible = config::get(cx).layout.sidebar_visible;
        let aside_visible = config::get(cx).layout.aside_visible;
        let dispatch: Dispatch = Rc::new({
            let connection = self.connection.clone();
            let snapshot = live.snapshot.clone();
            let entity = cx.entity();
            move |request, window, cx| match request {
                ShellRequest::FocusWorkspace(workspace_id) => {
                    connection.focus_workspace(&workspace_id)
                }
                ShellRequest::FocusTab(tab_id) => connection.focus_tab(&tab_id),
                ShellRequest::FocusPane(pane_id) => connection.focus_pane(&pane_id),
                // A command the shell runs itself goes through `run` whether it
                // arrived as a chord or as a click, so the two cannot part ways.
                ShellRequest::Command(command) if command.is_local() => {
                    entity.update(cx, |shell, cx| shell.run(command, window, cx));
                }
                ShellRequest::Command(command) => {
                    if let Some((method, params)) = snapshot
                        .as_ref()
                        .and_then(|snapshot| commands::request(command, snapshot))
                    {
                        connection.request(method, params);
                    }
                }
                // A plugin request leaves through the host, which owns the API
                // lane; the dispatch is the shell's, so the two paths cannot
                // part ways.
                request @ (ShellRequest::ShowView(_)
                | ShellRequest::ShowAsideTab(_)
                | ShellRequest::ShowSettingsPage(_)
                | ShellRequest::AcknowledgeAlerts
                | ShellRequest::ResetConfigDefaults
                | ShellRequest::StartRename(_)
                | ShellRequest::CommitRename
                | ShellRequest::CancelRename
                | ShellRequest::MoveTab { .. }
                | ShellRequest::MoveWorkspace { .. }
                | ShellRequest::PluginAction { .. }
                | ShellRequest::OpenPluginPane { .. }
                | ShellRequest::ClosePluginPane(_)
                | ShellRequest::ShowPluginPanels
                | ShellRequest::RefreshPlugins
                | ShellRequest::OpenPluginTrust
                | ShellRequest::ReviewPlugin
                | ShellRequest::OpenPluginTrustAt(_)
                | ShellRequest::LinkPlugin(_)
                | ShellRequest::ClosePluginTrust
                | ShellRequest::OpenPluginManager(_)
                | ShellRequest::ClosePluginManager
                | ShellRequest::SelectPlugin(_)
                | ShellRequest::SetPluginEnabled { .. }
                | ShellRequest::UnlinkPlugin(_)
                | ShellRequest::SaveLayout
                | ShellRequest::RestoreLayout
                | ShellRequest::ClearEvents
                | ShellRequest::SetEventFilter(_)
                | ShellRequest::CompareFleet
                | ShellRequest::ReadTail(_)
                | ShellRequest::ClearFleet) => {
                    entity.update(cx, |shell, cx| {
                        shell.dispatch(request, window, cx);
                        cx.notify();
                    });
                }
            }
        });
        // The prefix is always named, the way the prototype's status bar reads;
        // it only changes clothes while it is armed and claiming the next key.
        let prefix = self.keymap.prefix_label();
        let invocation = Invocation {
            snapshot: live.snapshot.as_deref(),
            surface: live.surface.as_deref(),
            status: live.status,
            status_error: error.as_deref(),
            rejection: self.move_error.as_deref().or(live.rejected.as_deref()),
            prefix: prefix.as_str(),
            prefix_armed: self.prefix_armed,
            plugins: &plugins,
            ui_data: &self.ui_data,
            layout: &self.layout,
            events: &self.events,
            fleet: &self.fleet,
            tails: &self.tails,
            bells: live.bells,
            attention: live.attention,
            rename: self.rename.as_ref(),
            theme: &theme,
            dispatch: &dispatch,
            row_height: config::get(cx).appearance.row_height(),
        };

        // Not `h_flex()`: that centres its children on the cross axis, which
        // would leave the grid column at its content height instead of filling
        // the window.
        let sidebar_width = self
            .sidebar
            .width(window.viewport_size().width.to_f64() as f32);
        let mut body = div().flex().flex_row().flex_1().min_h_0().w_full();
        // The rail owns the sidebar's composition, so it comes first and the
        // sidebar below mounts only the view it names.
        body = body.child(rail(self.rail, &invocation));
        if sidebar_visible {
            body = body.child(
                v_flex()
                    .relative()
                    .w(px(sidebar_width))
                    .h_full()
                    .flex_none()
                    .overflow_hidden()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.sidebar_border)
                    // The edge facing the grid is the sidebar's own handle, so
                    // the drag is resolved inside the panel it resizes, and the
                    // release that ends it is saved from here.
                    .on_drag_move(cx.listener(Self::drag_sidebar))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::settle_sidebar))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::settle_sidebar))
                    .child(panel_resize::grip(
                        "sidebar-grip",
                        Edge::Right,
                        SidebarDrag,
                        cx.listener(Self::reset_sidebar),
                    ))
                    .child(sidebar_head(self.rail, &invocation))
                    // The blocks scroll under the head; the foot stays on the
                    // floor. A workspace list that outgrows the panel used to
                    // be clipped with no way to reach the rest.
                    .child(
                        v_flex()
                            .w_full()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .children(
                                self.registry
                                    .panels_in(self.rail)
                                    .map(|panel| panel.render(&invocation, window, cx)),
                            ),
                    )
                    .child(sidebar_foot(self.rail, &invocation)),
            );
        }
        // The grid takes what the two panels leave, so it is added between
        // them: the sidebar on the left, the right-hand panel last.
        body = body.child(
            v_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(tab_strip(&invocation))
                .child(
                    // The grid needs the remaining height: inside a flex
                    // column a childless `size_full` collapses to the height
                    // its own content asks for, which for a canvas is one row.
                    // With no pane to paint, the area says what can be done
                    // instead of showing an empty surface.
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(if grid_is_empty(live.snapshot.as_deref()) {
                            empty::grid(&invocation)
                        } else {
                            self.terminal.clone().into_any_element()
                        }),
                ),
        );
        if aside_visible {
            let aside_width = self
                .aside
                .width(window.viewport_size().width.to_f64() as f32);
            // Every tab stays mounted and only the one in front is laid out:
            // a block's state lives in the element that renders it, so
            // unmounting a tab would drop what was typed into it (M11c).
            let bodies: Vec<AnyElement> = AsideTab::ALL
                .iter()
                .map(|tab| {
                    let active = *tab == self.aside_tab;
                    v_flex()
                        .id(("aside-body", *tab as usize))
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .when(!active, |body| body.hidden())
                        .child(aside_body(*tab, &self.registry, &invocation, window, cx))
                        .into_any_element()
                })
                .collect();
            body = body.child(
                v_flex()
                    .relative()
                    .w(px(aside_width))
                    .h_full()
                    .flex_none()
                    .overflow_hidden()
                    .bg(theme.sidebar)
                    .border_l_1()
                    .border_color(theme.sidebar_border)
                    // The same arrangement as the sidebar's, on the other side:
                    // the edge facing the grid is this panel's own handle.
                    .on_drag_move(cx.listener(Self::drag_aside))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::settle_aside))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::settle_aside))
                    .child(panel_resize::grip(
                        "aside-grip",
                        Edge::Left,
                        AsideDrag,
                        cx.listener(Self::reset_aside),
                    ))
                    .child(aside_strip(self.aside_tab, &self.registry, &invocation))
                    .children(bodies),
            );
        }
        let mut root = v_flex()
            .size_full()
            // The settings surface is laid over the shell, so the shell is what
            // it is laid over.
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            // Every directly bound keystroke dispatches its action here: a
            // binding with no handler matches and does nothing.
            .on_action(cx.listener(Self::run_command))
            // The key cap shows the palette's own chord, the way the palette's
            // rows would spell it.
            .child({
                let palette_hint = COMMANDS
                    .iter()
                    .find(|info| info.command == Command::TogglePalette)
                    .and_then(|info| info.direct.first())
                    .map(|chord| self.keymap.chord_label(chord));
                title_bar(&invocation, sidebar_visible, aside_visible, palette_hint)
            })
            .child(body)
            .child(status_bar(
                self.registry.status_segments(),
                &invocation,
                window,
                cx,
            ));
        if self.palette.is_open() {
            let entries = self.palette_entries(cx);
            root = root.child(palette::overlay(
                &self.palette,
                &entries,
                &self.keymap,
                &dispatch,
                palette::PaletteActions {
                    select_prev: Rc::new(cx.listener(Self::palette_select_prev)),
                    select_next: Rc::new(cx.listener(Self::palette_select_next)),
                    run: Rc::new(cx.listener(Self::palette_run)),
                    dismiss: Rc::new(cx.listener(Self::palette_dismiss)),
                },
                cx,
            ));
        }
        if self.settings_open {
            root = root.child(settings::overlay(
                self.registry.settings_sections(),
                self.settings_page,
                &self.settings_focus,
                &dispatch,
                self.settings_reset_armed,
                cx,
            ));
        }
        // Over the settings and the palette: the manager is opened from the
        // plugin view and from the review, and holding the keyboard is what
        // makes Escape mean what it says.
        if self.manager.is_open() {
            root = root.child(manager::overlay(
                &self.manager,
                &plugins,
                Availability::of(live.snapshot.as_deref()),
                &self.manager_focus,
                &dispatch,
                cx.listener(Self::manager_key),
                cx,
            ));
        }
        // Last, so it is the surface on top: the review is opened from the
        // palette as well as from the sidebar, and the keyboard follows it.
        if self.trust.is_open() {
            root = root.child(trust_view::overlay(
                &self.trust,
                &self.trust_focus,
                &dispatch,
                cx.listener(Self::trust_key),
                cx,
            ));
        }
        root
    }
}

/// One tab's blocks. The chat and the fleet bring their own scrolling, the
/// plugin panels are plain columns and need the shell to scroll them — so the
/// scroll wrapper belongs to the tab that needs it, inside the container that
/// decides whether the tab is laid out at all.
fn aside_body(
    tab: AsideTab,
    registry: &Registry,
    invocation: &Invocation,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut column = v_flex().flex_1().min_h_0().w_full();
    if tab == AsideTab::Panels {
        if registry.aside_in(tab).next().is_none() {
            // The third tab is only what the plugins declare, so a client with
            // no such declaration says where they come from rather than showing
            // an empty column.
            column = column.child(empty::plugin_panels(invocation));
        }
        return column
            .children(
                registry
                    .aside_in(tab)
                    .map(|panel| panel.render(invocation, window, cx)),
            )
            .overflow_y_scrollbar()
            .into_any_element();
    }
    column
        .children(
            registry
                .aside_in(tab)
                .map(|panel| panel.render(invocation, window, cx)),
        )
        .into_any_element()
}

/// The right-hand panel's strip: one button per tab, then the button that
/// closes the panel. A tab with something to count shows how many, so the strip
/// answers "is anything running / declared?" without being opened.
fn aside_strip(current: AsideTab, registry: &Registry, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let fleet = invocation.fleet.count("running") + invocation.fleet.count("blocked");
    let panels = registry.aside_in(AsideTab::Panels).count();
    let strip = h_flex()
        .flex_none()
        .w_full()
        .h(px(ASIDE_HEAD_HEIGHT))
        .px(px(6.))
        .gap(px(2.))
        .items_center()
        .bg(theme.sidebar)
        .border_b_1()
        .border_color(theme.sidebar_border)
        .children(AsideTab::ALL.iter().map(|tab| {
            let count = match tab {
                AsideTab::Fleet => fleet,
                AsideTab::Panels => panels,
                AsideTab::Chat => 0,
            };
            aside_tab_button(*tab, *tab == current, count, invocation)
        }));
    strip
        .child(div().flex_1())
        .child(aside_close(invocation))
        .into_any_element()
}

/// The strip's own close: a glyph button, not a labelled one — the strip has no
/// room for a word here and the locale file says nothing about symbols.
fn aside_close(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id("close-aside")
        .w(px(22.))
        .h(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|style| style.bg(theme.list_hover))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(Command::ToggleAside), window, cx);
        })
        .child(Icon::default().data(icons::CLOSE).w(px(13.)).h(px(13.)))
        .into_any_element()
}

/// One tab of the panel's strip, in the same pill the workspace tabs use.
fn aside_tab_button(
    tab: AsideTab,
    selected: bool,
    count: usize,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .id(("aside-tab", tab as usize))
        .h(px(24.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(px(4.))
        .cursor_pointer()
        // Opaque, like the prototype's active pill: `list_active` is a 20%
        // tint by the theme's own clamp, too faint to mark the open tab.
        .bg(if selected {
            theme.secondary
        } else {
            transparent_black()
        })
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted_foreground
        })
        .hover(|style| style.bg(theme.list_hover).text_color(theme.foreground))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::ShowAsideTab(tab), window, cx);
        })
        .child(Icon::default().data(tab.icon()).w(px(13.)).h(px(13.)))
        .child(div().text_size(px(11.5)).child(tab.label()))
        .children((count > 0).then(|| {
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(9.5))
                .text_color(theme.muted_foreground)
                .child(count.to_string())
        }))
        .into_any_element()
}

/// Whether the grid has no pane to paint. The daemon always gives a tab one
/// pane, so this is also the state before the first snapshot lands — the same
/// question either way: is there a surface for a focused tab?
fn grid_is_empty(snapshot: Option<&ClientShellSnapshot>) -> bool {
    let Some(snapshot) = snapshot else {
        return true;
    };
    let Some(tab_id) = snapshot.focused_tab_id.as_deref() else {
        return true;
    };
    !snapshot.panes.iter().any(|pane| pane.tab_id == tab_id)
}

/// The status bar: the facts about the session open it, the client's own state
/// closes it, and the free space between keeps the two apart. A segment with
/// nothing to say mounts nothing — its padding and divider go with it — so the
/// bar never shows a divider around an empty cell.
fn status_bar(
    segments: &[StatusContribution],
    invocation: &Invocation<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = invocation.theme;
    let bar = h_flex()
        .flex_none()
        .w_full()
        .h(px(STATUS_BAR_HEIGHT))
        .overflow_hidden()
        .items_center()
        .bg(theme.status_bar)
        .border_t_1()
        .border_color(theme.status_bar_border);
    let bar = status_side(
        bar,
        segments,
        StatusAlign::Start,
        invocation,
        window,
        cx,
        theme,
    );
    // The bar's free space: everything after it is the client's own state.
    let bar = bar.child(div().flex_1());
    let bar = status_side(
        bar,
        segments,
        StatusAlign::End,
        invocation,
        window,
        cx,
        theme,
    );
    // The client's own unread count comes last: bells and notifications that
    // asked for attention, until the user acknowledges them here.
    let unread = invocation.bells.saturating_add(invocation.attention);
    bar.when(unread > 0, |bar| {
        bar.child(status_segment(
            alert_badge(unread, invocation),
            false,
            theme,
        ))
    })
    .into_any_element()
}

/// The bell segment: how many bells and attention notifications are unread.
/// A click clears them, which is the only way they go away.
fn alert_badge(unread: u64, invocation: &Invocation<'_>) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .id("status-alerts")
        .gap_1()
        .items_center()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::AcknowledgeAlerts, window, cx);
        })
        .child(
            Icon::default()
                .data(icons::BELL)
                .w(px(12.))
                .h(px(12.))
                .text_color(theme.warning),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.muted_foreground)
                .child(unread.to_string()),
        )
        .into_any_element()
}

/// One side of the bar, left to right in registry order.
fn status_side(
    mut bar: Div,
    segments: &[StatusContribution],
    align: StatusAlign,
    invocation: &Invocation<'_>,
    window: &mut Window,
    cx: &mut App,
    theme: &Theme,
) -> Div {
    let mut first = true;
    for segment in segments.iter().filter(|item| item.align() == align) {
        if let Some(element) = segment.render(invocation, window, cx) {
            bar = bar.child(status_segment(element, first, theme));
            first = false;
        }
    }
    bar
}

/// One segment, with the divider that separates it from the segment before it
/// on the same side of the bar.
fn status_segment(element: AnyElement, first: bool, theme: &Theme) -> AnyElement {
    h_flex()
        .h_full()
        .flex_none()
        .items_center()
        .px_2()
        .when(!first, |row| {
            row.border_l_1().border_color(theme.status_bar_border)
        })
        .child(element)
        .into_any_element()
}

/// The sidebar's head: the view's name, what it holds, and the one action the
/// view carries — the prototype's identity strip above whatever blocks mount
/// under it (PLAN.md §M11j).
fn sidebar_head(view: SidebarView, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .flex_none()
        .w_full()
        .h(px(SIDEBAR_HEAD_HEIGHT))
        .px_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(theme.sidebar_border)
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                // The head is an identity strip, not a title: the prototype
                // keeps it at the muted weight so the rows under it lead.
                .text_color(theme.muted_foreground)
                .child(view.label().to_uppercase()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(view_count(view, invocation)),
        )
        // The manager is the plugins view's own action: what is installed is
        // one registry read, and its page is where that read is shaped.
        .when(view == SidebarView::Plugins, |head| {
            head.child(
                div()
                    .id("sidebar-plugins-manager")
                    .w(px(22.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(|style| style.bg(theme.sidebar_accent))
                    .on_click(move |_, window, cx| {
                        dispatch(ShellRequest::OpenPluginManager(None), window, cx);
                    })
                    .child(
                        Icon::default()
                            .data(SidebarView::Plugins.icon())
                            .w(px(13.))
                            .h(px(13.)),
                    ),
            )
        })
        .into_any_element()
}

/// What the view holds, as one dim count beside its name: how many of the thing
/// the view is about exist right now. A count is the pushed truth, never a
/// second source — each one reads the state its view's blocks read.
fn view_count(view: SidebarView, invocation: &Invocation) -> String {
    match view {
        SidebarView::Workspaces => workspaces_count(invocation.snapshot),
        SidebarView::Agents => agents_count(invocation.snapshot),
        SidebarView::Fleet => rust_i18n::t!(
            "fleet.summary",
            agents = invocation
                .snapshot
                .map_or(0, |snapshot| snapshot.agents.len()),
            blocked = invocation.fleet.count("blocked")
        )
        .to_string(),
        SidebarView::Plugins => plugins_count(invocation.plugins),
        SidebarView::Events => events_count(invocation.events),
        SidebarView::Layout => t("layout.file"),
    }
}

/// The session's size, the way the prototype's head reads it: workspaces, then
/// panes.
fn workspaces_count(snapshot: Option<&ClientShellSnapshot>) -> String {
    let (workspaces, panes) = snapshot
        .map(|snapshot| (snapshot.workspaces.len(), snapshot.panes.len()))
        .unwrap_or_default();
    rust_i18n::t!(
        "shell.head.workspaces",
        workspaces = workspaces,
        panes = panes
    )
    .to_string()
}

/// How many agents there are, and how many are away from idle — the same count
/// the rail's badge wears.
fn agents_count(snapshot: Option<&ClientShellSnapshot>) -> String {
    let (agents, active) = snapshot
        .map(|snapshot| {
            (
                snapshot.agents.len(),
                snapshot
                    .agents
                    .iter()
                    .filter(|agent| {
                        matches!(
                            agent.agent_status,
                            AgentStatus::Working | AgentStatus::Blocked
                        )
                    })
                    .count(),
            )
        })
        .unwrap_or_default();
    rust_i18n::t!("shell.head.agents", agents = agents, active = active).to_string()
}

/// What is linked, and how much of it is running with the daemon's blessing.
fn plugins_count(plugins: &PluginState) -> String {
    let enabled = plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .count();
    rust_i18n::t!(
        "shell.head.plugins",
        plugins = plugins.plugins.len(),
        enabled = enabled
    )
    .to_string()
}

/// Whether the event stream is where it should be, so a silent stream and a
/// broken one do not read the same.
fn events_count(events: &EventState) -> String {
    match events.error.as_deref() {
        Some(_) => t("shell.head.events_down"),
        None => t("shell.head.events_live"),
    }
}

/// The sidebar's foot: where this view's rows come from. One line, fixed per
/// view — a data source is a fact about the view, not a running value.
fn sidebar_foot(view: SidebarView, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    h_flex()
        .flex_none()
        .w_full()
        .h(px(SIDEBAR_FOOT_HEIGHT))
        .px_2()
        .items_center()
        .border_t_1()
        .border_color(theme.sidebar_border)
        .child(
            div()
                .min_w_0()
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(t(&format!("shell.foot.{}", view.id()))),
        )
        .into_any_element()
}

/// The activity rail: one button per sidebar view, the current one marked by a
/// bar on the rail's edge, a view with something away from idle wearing its
/// count. The gear at the bottom is the settings surface's second entry — the
/// palette and `cmd-,` are the first. A click is a local request — the daemon
/// has no opinion about what the client is looking at.
fn rail(current: SidebarView, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    v_flex()
        .w(px(RAIL_WIDTH))
        .h_full()
        .flex_none()
        .py(px(6.))
        .gap(px(2.))
        .bg(theme.sidebar)
        .border_r_1()
        .border_color(theme.sidebar_border)
        .children(SidebarView::ALL.iter().map(|view| {
            let view = *view;
            let selected = view == current;
            let dispatch = invocation.dispatch.clone();
            // The row spans the rail so the active bar can reach its edge; the
            // button stays centered inside it.
            div()
                .w_full()
                .h(px(34.))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(8.))
                            .bottom(px(8.))
                            .w(px(2.))
                            .rounded_full()
                            .bg(theme.primary),
                    )
                })
                .child(
                    div()
                        .id(("rail", view as usize))
                        .w(px(34.))
                        .h(px(34.))
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .bg(if selected {
                            theme.primary.opacity(0.12)
                        } else {
                            theme.sidebar
                        })
                        .text_color(if selected {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        })
                        .hover(|style| style.bg(theme.sidebar_accent))
                        .on_click(move |_, window, cx| {
                            dispatch(ShellRequest::ShowView(view), window, cx);
                        })
                        .child(Icon::default().data(view.icon()).w(px(17.)).h(px(17.)))
                        .children(rail_badge(invocation.snapshot, view).map(|count| {
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .min_w(px(13.))
                                .h(px(13.))
                                .px(px(3.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(theme.info)
                                .text_color(theme.info_foreground)
                                .text_size(px(9.))
                                .child(count.to_string())
                        })),
                )
        }))
        // The rail's floor: the settings surface, reached from here the way the
        // prototype reads — a chrome entry, not a sidebar view.
        .child(div().flex_1())
        .child(rail_gear(invocation))
        .into_any_element()
}

/// The rail's settings button. `active` never applies: it opens an overlay and
/// nothing about the rail says the overlay is in front.
fn rail_gear(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id("rail-settings")
        .w(px(34.))
        .h(px(34.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(Command::ToggleSettings), window, cx);
        })
        .child(Icon::default().data(icons::GEAR).w(px(17.)).h(px(17.)))
        .into_any_element()
}

/// The count a rail button wears: how many agents are away from idle, so the
/// rail answers "is anything running?" without opening the view. The other
/// views have nothing counted yet.
fn rail_badge(snapshot: Option<&ClientShellSnapshot>, view: SidebarView) -> Option<usize> {
    match view {
        SidebarView::Agents => Some(
            snapshot?
                .agents
                .iter()
                .filter(|agent| {
                    matches!(
                        agent.agent_status,
                        AgentStatus::Working | AgentStatus::Blocked
                    )
                })
                .count(),
        )
        .filter(|&count| count > 0),
        _ => None,
    }
}

/// The cwd the layout block speaks for: the focused pane's, preferring the cwd the
/// daemon carries for its foreground process. A snapshot with no focused pane has
/// nothing to walk up from.
fn layout_cwd(snapshot: &ClientShellSnapshot) -> Option<&str> {
    let id = snapshot.focused_pane_id.as_deref()?;
    let pane = snapshot.panes.iter().find(|pane| pane.pane_id == id)?;
    pane.foreground_cwd.as_deref().or(pane.cwd.as_deref())
}

/// The chrome's own icons, the same inline svg the rail's views are drawn
/// with: the prototype's stroke shapes, at whatever size the surface asks for.
mod icons {
    pub(crate) const SEARCH: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="6"/><path d="m20 20-4.3-4.3"/></svg>"#;
    pub(crate) const MENU: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M4 12h16M4 17h10"/></svg>"#;
    pub(crate) const CLOSE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m7 7 10 10M17 7 7 17"/></svg>"#;
    pub(crate) const PLUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>"#;
    pub(crate) const CHEVRON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"#;
    pub(crate) const SIDEBAR: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg>"#;
    pub(crate) const ASIDE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></svg>"#;
    pub(crate) const GEAR: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.6"/><circle cx="12" cy="12" r="3.2"/><path d="M12 3.4v2.2M12 18.4v2.2M3.4 12h2.2M18.4 12h2.2"/></svg>"#;
    pub(crate) const SPLIT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/></svg>"#;
    pub(crate) const BELL: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9a6 6 0 0 1 12 0c0 5 2 6 2 6H4s2-1 2-6"/><path d="M10 20a2 2 0 0 0 4 0"/></svg>"#;
}

/// The workspace the chrome speaks for: the one the daemon says is focused.
fn focused_workspace(snapshot: &ClientShellSnapshot) -> Option<&ClientShellWorkspace> {
    let id = snapshot.focused_workspace_id.as_deref()?;
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == id)
}

/// The chrome above the grid, the prototype's five-part shell's first part:
/// who this window is (brand, workspace, daemon), the one search that reaches
/// every command, and the local toggles — with the window controls at the end.
/// The closing window's geometry goes into the config, so the next launch
/// opens the shape this one closes with. Both close paths — the strip's own X
/// (Linux) and the window manager's — come through here.
pub(crate) fn save_window_bounds(window: &Window, cx: &mut App) {
    let size = match window.window_bounds() {
        WindowBounds::Windowed(bounds)
        | WindowBounds::Maximized(bounds)
        | WindowBounds::Fullscreen(bounds) => bounds.size,
    };
    config::update(cx, |config| {
        config.window.set_bounds(
            size.width.as_f32(),
            size.height.as_f32(),
            window.is_maximized(),
        )
    });
}

/// The strip is gpui-kit's `TitleBar`: it owns the dragging, the double-click
/// zoom and the min/max/close cluster, drawing the latter only when the
/// window is client-decorated (a compositor manager's server decorations
/// already bring their own). The settings and theme entries live on the rail
/// and in the settings surface; the M7 note that left controls to the
/// platform is superseded by the frameless window they were waiting for.
fn title_bar(
    invocation: &Invocation,
    sidebar_visible: bool,
    aside_visible: bool,
    palette_hint: Option<String>,
) -> AnyElement {
    let theme = invocation.theme;
    let mut bar = TitleBar::new()
        .h(px(TITLE_BAR_HEIGHT))
        .pl_2()
        .pr_1()
        .gap_2()
        .bg(theme.title_bar)
        .border_color(theme.title_bar_border)
        // The strip's own close button would remove the window outright; the
        // geometry goes into the config first, so the next launch opens the
        // shape this one closes with. The window manager's close path — a
        // server-decorated window's, or a WM's kill — is hooked in `lib.rs`.
        .on_close_window(|_, window, cx| {
            save_window_bounds(window, cx);
            window.remove_window();
        })
        // The window says its name before anything else.
        .child(
            div()
                .w(px(18.))
                .h(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                // The prototype's mark: accent at one corner, brand blue at
                // the other. In the zenix theme `primary` is that accent, so
                // the second stop is the info blue.
                .bg(linear_gradient(
                    140.,
                    linear_color_stop(theme.primary, 0.),
                    linear_color_stop(theme.info, 1.),
                ))
                .text_color(theme.primary_foreground)
                .text_size(px(11.))
                .font_family(theme.mono_font_family.clone())
                .font_weight(FontWeight::BOLD)
                .child("z"),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("zenix"),
        )
        .child(div().w(px(1.)).h(px(16.)).bg(theme.title_bar_border));
    // The workspace pill: what is in front, and the branch it is on. A click
    // asks the sidebar for its workspaces view — the same list a workspace
    // switch is made from — so the pill is an entry to picking, not a picker.
    if let Some(workspace) = invocation.snapshot.and_then(focused_workspace) {
        let dispatch = invocation.dispatch.clone();
        bar = bar.child(
            h_flex()
                .id("workspace-pill")
                .px_2()
                .py_0p5()
                .gap_1p5()
                .items_center()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(theme.list_hover))
                .on_click(move |_, window, cx| {
                    dispatch(ShellRequest::ShowView(SidebarView::Workspaces), window, cx);
                })
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(Icon::default().data(icons::MENU).w(px(13.)).h(px(13.))),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(workspace.label.clone()),
                )
                .children(workspace.branch.as_deref().map(|branch| {
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_size(px(10.5))
                        .text_color(theme.muted_foreground)
                        .child(branch.to_owned())
                }))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(Icon::default().data(icons::CHEVRON).w(px(11.)).h(px(11.))),
                ),
        );
    }
    // The daemon, beside the workspace it serves. The release version is not
    // in the handshake, so the label is the state alone — the probe prints the
    // version when it is worth knowing.
    let (status_label, status_color) =
        panels::connection_indication(invocation.status, invocation.status_error, theme);
    bar = bar.child(
        h_flex()
            .gap_1p5()
            .items_center()
            .child(div().size(px(6.)).rounded_full().bg(status_color))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child(status_label),
            ),
    );
    // The one search: every command, workspace and plugin action behind a
    // single trigger, with the chord that reaches it from the keyboard.
    bar = bar
        .child(div().flex_1())
        .child(command_trigger(palette_hint, invocation))
        .child(div().flex_1());
    bar.child(title_toggle(
        "toggle-sidebar",
        icons::SIDEBAR,
        sidebar_visible,
        Command::ToggleSidebar,
        invocation,
    ))
    .child(title_toggle(
        "toggle-aside",
        icons::ASIDE,
        aside_visible,
        Command::ToggleAside,
        invocation,
    ))
    .into_any_element()
}

/// The title bar's search trigger: a quiet input-shaped button that opens the
/// palette. The key cap shows the chord the keymap actually binds, not a
/// hardcoded one.
fn command_trigger(palette_hint: Option<String>, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .id("palette-trigger")
        .w(px(420.))
        .h(px(24.))
        .px(px(8.))
        .gap_2()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_color(theme.muted_foreground)
        .cursor_pointer()
        .hover(|style| style.border_color(theme.muted_foreground))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(Command::TogglePalette), window, cx);
        })
        .child(Icon::default().data(icons::SEARCH).w(px(13.)).h(px(13.)))
        .child(
            div()
                .flex_1()
                .text_size(px(11.5))
                .child(t("shell.titlebar.search")),
        )
        .children(palette_hint.map(|hint| key_cap(hint, theme)))
        .into_any_element()
}

/// The little key cap at a trigger's right edge: the chord that reaches the
/// same thing from the keyboard.
fn key_cap(label: String, theme: &Theme) -> AnyElement {
    div()
        .px(px(4.))
        .py(px(1.))
        .rounded(px(3.))
        .border_1()
        .border_color(theme.border)
        .font_family(theme.mono_font_family.clone())
        .text_size(px(10.))
        .text_color(theme.muted_foreground)
        .child(label)
        .into_any_element()
}

/// One of the chrome's right-edge toggles: an icon, lit while the thing it
/// opens is in front. A click is the same dispatch a chord runs.
fn title_toggle(
    id: &'static str,
    icon: &'static [u8],
    active: bool,
    command: Command,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id(id)
        .w(px(26.))
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .text_color(if active {
            theme.primary
        } else {
            theme.muted_foreground
        })
        .hover(|style| style.bg(theme.list_hover))
        .when(active, |button| button.bg(theme.primary.opacity(0.12)))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(command), window, cx);
        })
        .child(Icon::default().data(icon).w(px(15.)).h(px(15.)))
        .into_any_element()
}

/// The focused workspace's tabs, then the button that opens another one.
fn tab_strip(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let tabs: Vec<&ClientShellTab> = invocation
        .snapshot
        .map(|snapshot| {
            focused_workspace(snapshot)
                .map(|workspace| {
                    snapshot
                        .tabs
                        .iter()
                        .filter(|tab| tab.workspace_id == workspace.workspace_id)
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let strip = h_flex()
        .flex_none()
        .w_full()
        .h(px(TAB_STRIP_HEIGHT))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        // The strip is the shell's own background, not the tab bar's: only the
        // tabs lift off it, as the prototype draws them.
        .bg(theme.background)
        .border_b_1()
        .border_color(theme.border);
    let count = tabs.len();
    let dispatch = invocation.dispatch.clone();
    // The tabs scroll in their own row; the actions stay pinned to the right
    // edge, so a session with more tabs than width never hides them.
    let tabs_row = h_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .gap(px(4.))
        .items_center()
        .overflow_x_scrollbar()
        .children(
            tabs.into_iter()
                .enumerate()
                .map(|(index, tab)| tab_button(tab, index, invocation)),
        )
        // The row's tail is the gap after the last tab: a drop that no tab
        // claimed lands here and appends.
        .on_drop(move |drag: &reorder::TabDrag, window, cx| {
            if let Some(insert_index) = reorder::gap_at_end(drag.from, count) {
                dispatch(
                    ShellRequest::MoveTab {
                        tab_id: drag.tab_id.clone(),
                        insert_index,
                    },
                    window,
                    cx,
                );
            }
        });
    strip
        // The strip's actions sit at its right edge, where the prototype puts
        // them: new tab, split, palette.
        .child(tabs_row)
        .child(strip_button(
            "strip-new",
            icons::PLUS,
            Command::NewTab,
            invocation,
        ))
        .child(strip_button(
            "strip-split",
            icons::SPLIT,
            Command::SplitRight,
            invocation,
        ))
        .child(strip_button(
            "strip-palette",
            icons::SEARCH,
            Command::TogglePalette,
            invocation,
        ))
        .into_any_element()
}

/// One of the strip's quiet icon buttons — the split and the palette triggers
/// that sit beside the new-tab button. Neither holds a state, so unlike the
/// title bar's toggles there is nothing to light.
fn strip_button(
    id: &'static str,
    icon: &'static [u8],
    command: Command,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id(id)
        .w(px(22.))
        .h(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|style| style.bg(theme.list_hover).text_color(theme.foreground))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(command), window, cx);
        })
        .child(Icon::default().data(icon).w(px(14.)).h(px(14.)))
        .into_any_element()
}

fn tab_button(tab: &ClientShellTab, index: usize, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let open = invocation.dispatch.clone();
    let close = invocation.dispatch.clone();
    let drop = invocation.dispatch.clone();
    let tab_id = tab.tab_id.clone();
    // A tab being renamed is its editor, not its button: no focus click, no
    // drag, no close while the name is on the table.
    if let Some(input) = invocation
        .rename
        .and_then(|(subject, input)| match subject {
            RenameSubject::Tab(renaming) if *renaming == tab.tab_id => Some(input.clone()),
            _ => None,
        })
    {
        let keys = invocation.dispatch.clone();
        return h_flex()
            .id(("tab-rename", tab.number))
            .h(px(26.))
            .min_w(px(160.))
            .px(px(7.))
            .gap(px(7.))
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.tab_active)
            .text_color(theme.tab_active_foreground)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => {
                        cx.stop_propagation();
                        keys(ShellRequest::CommitRename, window, cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        keys(ShellRequest::CancelRename, window, cx);
                    }
                    _ => {}
                }
            })
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .h(px(20.))
                        .appearance(false)
                        .bordered(false),
                ),
            )
            .into_any_element();
    }
    let label = if tab.zoomed {
        format!("{} ^", tab.label)
    } else {
        tab.label.clone()
    };
    // What the strip says without being opened: whether the tab's agent needs
    // an eye (the dot), and whether the tab is more than one pane (the count).
    let dot = invocation
        .snapshot
        .and_then(|snapshot| tab_agent_status(&snapshot.agents, &tab.tab_id));
    let pane_count = invocation
        .snapshot
        .map(|snapshot| {
            snapshot
                .panes
                .iter()
                .filter(|pane| pane.tab_id == tab.tab_id)
                .count()
        })
        .unwrap_or(0);
    h_flex()
        .id(("tab", tab.number))
        .h(px(26.))
        .min_w(px(88.))
        .max_w(px(240.))
        .px(px(7.))
        .pl(px(9.))
        .gap(px(7.))
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(if tab.focused {
            theme.border
        } else {
            transparent_black()
        })
        .bg(if tab.focused {
            theme.tab_active
        } else {
            transparent_black()
        })
        .text_color(if tab.focused {
            theme.tab_active_foreground
        } else {
            theme.tab_foreground
        })
        .cursor_pointer()
        .group("tab")
        // The second click of a double-click is a rename, not another focus.
        .on_click(move |event, window, cx| {
            if let ClickEvent::Mouse(click) = event
                && click.up.click_count >= 2
            {
                cx.stop_propagation();
                open(
                    ShellRequest::StartRename(RenameSubject::Tab(tab_id.clone())),
                    window,
                    cx,
                );
                return;
            }
            open(ShellRequest::FocusTab(tab_id.clone()), window, cx);
        })
        // A tab carried onto another lands in the gap before it; the two gaps
        // around its own place change nothing and keep the drop from reaching
        // the strip's end gap.
        .on_drag(
            reorder::TabDrag {
                tab_id: tab.tab_id.clone(),
                from: index,
            },
            |_, _, _, cx| cx.new(|_| EmptyView),
        )
        .on_drop(move |drag: &reorder::TabDrag, window, cx| {
            if let Some(insert_index) = reorder::gap_before(drag.from, index) {
                drop(
                    ShellRequest::MoveTab {
                        tab_id: drag.tab_id.clone(),
                        insert_index,
                    },
                    window,
                    cx,
                );
            }
        })
        .children(
            dot.filter(|status| !matches!(status, AgentStatus::Idle | AgentStatus::Unknown))
                .map(|status| panels::status_dot(status, theme)),
        )
        .child(div().min_w_0().truncate().text_size(px(12.)).child(label))
        .children((pane_count > 0).then(|| {
            div()
                .flex_none()
                .px(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme.border)
                .font_family(theme.mono_font_family.clone())
                .text_size(px(9.5))
                .text_color(theme.muted_foreground)
                .child(pane_count.to_string())
        }))
        .child(
            div()
                .id(("tab-close", tab.number))
                .flex_none()
                .w(px(15.))
                .h(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .cursor_pointer()
                .text_color(theme.muted_foreground)
                .opacity(if tab.focused { 1. } else { 0. })
                .group_hover("tab", |close| close.opacity(1.))
                .hover(|style| style.bg(theme.muted).text_color(theme.danger))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(ShellRequest::Command(Command::CloseTab), window, cx);
                })
                .child(Icon::default().data(icons::CLOSE).w(px(11.)).h(px(11.))),
        )
        .into_any_element()
}

/// The status a tab's dot wears: the one among its agents that most needs an
/// eye — blocked, then working, then done; idle and unknown are the floor. A
/// tab without agents wears no dot. The tab strip and the workspaces tree
/// paint the same dot, so a status means one color everywhere it appears.
pub(crate) fn tab_agent_status(agents: &[ClientShellAgent], tab_id: &str) -> Option<AgentStatus> {
    let rank = |status: AgentStatus| match status {
        AgentStatus::Blocked => 0,
        AgentStatus::Working => 1,
        AgentStatus::Done => 2,
        AgentStatus::Idle | AgentStatus::Unknown => 3,
    };
    agents
        .iter()
        .filter(|agent| agent.tab_id == tab_id)
        .map(|agent| agent.agent_status)
        .min_by_key(|status| rank(*status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_client::protocol::ClientShellSnapshot;
    use std::prelude::v1::test;

    fn snapshot(focused_tab: Option<&str>, panes: &[(&str, &str)]) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": "boot",
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "workspaces": [],
            "tabs": [],
            "panes": panes
                .iter()
                .map(|(pane_id, tab_id)| serde_json::json!({
                    "pane_id": pane_id,
                    "workspace_id": "w1",
                    "tab_id": tab_id,
                    "focused": false,
                    "right_click_passthrough": false,
                }))
                .collect::<Vec<_>>(),
            "agents": [],
            "commands": [],
            "focused_tab_id": focused_tab,
        }))
        .expect("a snapshot with panes still deserializes")
    }

    /// The empty state stands in for the surface, so it must be shown exactly
    /// when there is no surface to paint for the focused tab.
    #[test]
    fn the_grid_is_empty_without_a_snapshot_or_without_a_pane_in_the_focused_tab() {
        assert!(grid_is_empty(None));
        assert!(grid_is_empty(Some(&snapshot(None, &[("p1", "t1")]))));
        assert!(grid_is_empty(Some(&snapshot(Some("t1"), &[]))));
        // A pane in another tab is not a pane in front of this one.
        assert!(grid_is_empty(Some(&snapshot(Some("t2"), &[("p1", "t1")]))));
        assert!(!grid_is_empty(Some(&snapshot(
            Some("t1"),
            &[("p1", "t1"), ("p2", "t2")]
        ))));
    }

    /// A snapshot with workspaces in it, so the chrome's lookup has something
    /// to find.
    fn workspace_snapshot(
        focused: Option<&str>,
        labels: &[(&str, &str, Option<&str>)],
    ) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": "boot",
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "focused_workspace_id": focused,
            "workspaces": labels
                .iter()
                .map(|(id, label, branch)| serde_json::json!({
                    "workspace_id": id,
                    "active_tab_id": "t1",
                    "new_workspace_cwd": "",
                    "number": 1,
                    "label": label,
                    "custom_label": false,
                    "branch": branch,
                    "tokens": [],
                    "focused": false,
                    "agent_status": "idle",
                }))
                .collect::<Vec<_>>(),
            "tabs": [],
            "panes": [],
            "agents": [],
            "commands": [],
            "focused_tab_id": null,
        }))
        .expect("a snapshot with workspaces still deserializes")
    }

    /// The title bar's pill speaks for the focused workspace and names its
    /// branch; without a focus there is nothing to speak for.
    #[test]
    fn the_chrome_names_the_focused_workspace_and_its_branch() {
        let snapshot = workspace_snapshot(
            Some("w2"),
            &[("w1", "zenix", Some("main")), ("w2", "BitFun", None)],
        );
        let workspace = focused_workspace(&snapshot).expect("the focused workspace is found");
        assert_eq!(workspace.label, "BitFun");
        assert_eq!(workspace.branch.as_deref(), None);

        let snapshot = workspace_snapshot(None, &[("w1", "zenix", Some("main"))]);
        assert!(focused_workspace(&snapshot).is_none());

        let snapshot = workspace_snapshot(Some("gone"), &[("w1", "zenix", Some("main"))]);
        assert!(focused_workspace(&snapshot).is_none());
    }

    /// A snapshot with agents at the given statuses, so the rail's badge and
    /// the tab strip's dot have something to count.
    fn agent_snapshot(agents: &[(&str, &str)]) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": "boot",
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "workspaces": [],
            "tabs": [],
            "panes": [],
            "agents": agents
                .iter()
                .enumerate()
                .map(|(n, (tab_id, status))| serde_json::json!({
                    "pane_id": format!("p{n}"),
                    "workspace_id": "w1",
                    "tab_id": tab_id,
                    "agent_status": status,
                    "state_change_seq": 1,
                    "state_labels": [],
                    "tokens": [],
                    "focused": false,
                }))
                .collect::<Vec<_>>(),
            "commands": [],
            "focused_tab_id": null,
        }))
        .expect("a snapshot with agents still deserializes")
    }

    /// The badge counts the agents that need attention — working or blocked —
    /// and stays off the rail while everything is idle.
    #[test]
    fn the_agents_rail_badge_counts_the_agents_away_from_idle() {
        let snapshot = agent_snapshot(&[("t1", "working"), ("t1", "blocked"), ("t2", "idle")]);
        assert_eq!(
            rail_badge(Some(&snapshot), SidebarView::Agents),
            Some(2),
            "working and blocked wear the count, idle does not"
        );
        let idle = agent_snapshot(&[("t1", "idle"), ("t1", "unknown")]);
        assert_eq!(rail_badge(Some(&idle), SidebarView::Agents), None);
        // A view without a count wears none, and no snapshot counts nothing.
        assert_eq!(rail_badge(Some(&idle), SidebarView::Events), None);
        assert_eq!(rail_badge(None, SidebarView::Agents), None);
    }

    /// A tab's dot is its most attention-needing agent's status; a tab with no
    /// agent wears no dot, and another tab's agent is not this tab's.
    #[test]
    fn a_tabs_dot_is_its_most_urgent_agent() {
        let snapshot = agent_snapshot(&[("t1", "working"), ("t1", "blocked"), ("t2", "done")]);
        let agents = &snapshot.agents;
        assert_eq!(
            tab_agent_status(agents, "t1"),
            Some(AgentStatus::Blocked),
            "blocked outranks working"
        );
        assert_eq!(tab_agent_status(agents, "t2"), Some(AgentStatus::Done));
        assert_eq!(tab_agent_status(agents, "t3"), None);
    }

    /// Each view's head count is the size of the thing the view is about, read
    /// from the state its own blocks read — and a client with no snapshot yet
    /// counts nothing rather than inventing a zero session.
    #[test]
    fn a_views_head_count_is_what_the_view_is_about() {
        let watched = agent_snapshot(&[("t1", "working"), ("t2", "idle")]);
        assert_eq!(
            agents_count(Some(&watched)),
            rust_i18n::t!("shell.head.agents", agents = 2, active = 1).to_string()
        );
        assert_eq!(
            agents_count(None),
            rust_i18n::t!("shell.head.agents", agents = 0, active = 0).to_string()
        );

        let mut mixed = snapshot(None, &[]);
        mixed.workspaces =
            workspace_snapshot(None, &[("w1", "a", None), ("w2", "b", None)]).workspaces;
        mixed.panes.push(
            serde_json::from_value(serde_json::json!(
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
                 "focused": true, "right_click_passthrough": false}
            ))
            .expect("a pane"),
        );
        assert_eq!(
            workspaces_count(Some(&mixed)),
            rust_i18n::t!("shell.head.workspaces", workspaces = 2, panes = 1).to_string()
        );
        assert_eq!(
            workspaces_count(None),
            rust_i18n::t!("shell.head.workspaces", workspaces = 0, panes = 0).to_string()
        );

        let mut plugins = crate::plugins::host::PluginState::default();
        plugins.plugins = vec![
            crate::plugins::model::InstalledPlugin {
                plugin_id: "a".into(),
                name: "a".into(),
                version: "0.1.0".into(),
                plugin_root: "/tmp/a".into(),
                description: None,
                enabled: true,
                panes: vec![],
                events: vec![],
                warnings: vec![],
            },
            crate::plugins::model::InstalledPlugin {
                plugin_id: "b".into(),
                name: "b".into(),
                version: "0.1.0".into(),
                plugin_root: "/tmp/b".into(),
                description: None,
                enabled: false,
                panes: vec![],
                events: vec![],
                warnings: vec![],
            },
        ];
        assert_eq!(
            plugins_count(&plugins),
            rust_i18n::t!("shell.head.plugins", plugins = 2, enabled = 1).to_string()
        );

        let mut events = EventState::default();
        assert_eq!(events_count(&events), t("shell.head.events_live"));
        events.error = Some("gone".into());
        assert_eq!(events_count(&events), t("shell.head.events_down"));
    }
}
