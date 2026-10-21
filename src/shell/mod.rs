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

use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, Theme, ThemeMode, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use herdr_client::protocol::{ClientShellSnapshot, ClientShellTab};

use crate::ai::endpoint::ApiEndpoint;
use crate::config::{self, ASIDE_WIDTH_DEFAULT, ConfigStore, SIDEBAR_WIDTH_DEFAULT};
use crate::events::{EventState, Stream};
use crate::fleet::{FleetState, Tails, read_tails};
use crate::herdr::Connection;
use crate::herdr::api::ApiClient;
use crate::layout::{self, LayoutState};
use crate::plugins::actions::Catalog;
use crate::plugins::host::PluginHost;
use crate::plugins::model::Availability;
use crate::plugins::trust_view::{self, TrustReview};
use crate::plugins::ui_data::{self, UiData, UiDataHost};
use crate::plugins::ui_manifest::UiContributions;
use crate::terminal::Theme as TerminalTheme;
use crate::terminal::view::TerminalView;
use commands::{COMMANDS, Command};
use keymap::{Chord, Keymap, RunCommand};
use palette::{Entry, Key as PaletteKey, Palette};
use panel_resize::{ASIDE, AsideDrag, Edge, PanelWidth, SIDEBAR, SidebarDrag};
use registry::{
    AsideTab, Dispatch, Invocation, Registry, ShellRequest, SidebarView, StatusAlign,
    StatusContribution,
};

const TITLE_BAR_HEIGHT: f32 = 34.;
const TAB_STRIP_HEIGHT: f32 = 28.;
/// The status bar's height: one row of 11px text with room for its dividers.
const STATUS_BAR_HEIGHT: f32 = 26.;
/// The activity rail: one column of view buttons, left of the sidebar.
const RAIL_WIDTH: f32 = 44.;

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
    /// The keyboard while the settings surface is open, so the pane beside it
    /// stops receiving keystrokes.
    settings_focus: FocusHandle,
    /// The command palette: the shell's own directory plus the daemon's plugin
    /// actions, over the shell while it is open.
    palette: Palette,
    /// The keyboard while the palette is open.
    palette_focus: FocusHandle,
    /// The pre-link review: a path is typed into it, and what that path would
    /// run is shown before anything is linked.
    trust: TrustReview,
    /// The keyboard while the review is open.
    trust_focus: FocusHandle,
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
            settings_focus: cx.focus_handle(),
            palette: Palette::default(),
            palette_focus: cx.focus_handle(),
            trust: TrustReview::default(),
            trust_focus: cx.focus_handle(),
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
        let catalog = Catalog::build(&state.actions, &state.ui);
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

    /// Ask the reader for the data these declarations name. `force` re-reads
    /// every one of them, which is what asking for a refresh means; otherwise
    /// only what has no answer yet is read.
    fn request_ui_data(&self, ui: &UiContributions, force: bool, cx: &App) {
        let live = self.terminal.read(cx).live().clone();
        let context = Arc::new(ui_data::context(live.snapshot.as_deref()));
        self.ui_data_host.sync(ui_data::loads(ui), context, force);
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
        if self.settings_open || self.palette.is_open() || self.trust.is_open() {
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
            Command::ClosePopup => self.plugins.close_popup(),
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
    /// does not open over it.
    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            return;
        }
        if self.palette.is_open() {
            self.palette.close();
            let focus = self.terminal.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        } else {
            self.palette.open();
            let focus = self.palette_focus.clone();
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
        entries.extend(
            Catalog::build(&plugins.actions, &plugins.ui)
                .offered(availability)
                .map(|entry| Entry::Plugin(entry.clone())),
        );
        entries
    }

    /// The palette holds the keyboard while it is open: a keystroke moves its
    /// selection, and Enter hands the row's request to the same dispatch a
    /// click uses, so the two cannot behave differently.
    fn palette_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.palette_entries(cx);
        let matches = self.palette.matches(&entries);
        match self.palette.keystroke(&event.keystroke, matches.len()) {
            PaletteKey::Close => self.toggle_palette(window, cx),
            PaletteKey::Run(index) => {
                let Some(entry) = matches.get(index) else {
                    return;
                };
                let request = entry.request();
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
            PaletteKey::Handled => cx.notify(),
        }
    }

    /// A keystroke GPUI bound directly, dispatched as its action. The prefix
    /// chords arrive through the interceptor instead, and both end in [`Self::run`].
    fn run_command(&mut self, action: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.run(action.command, window, cx);
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
                cx.notify();
            }
            ShellRequest::FocusWorkspace(workspace_id) => {
                self.connection.focus_workspace(&workspace_id)
            }
            ShellRequest::FocusTab(tab_id) => self.connection.focus_tab(&tab_id),
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
                cx.notify();
            }
            ShellRequest::RefreshPlugins => self.refresh_plugins(cx),
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

    /// Whether an overlay of the shell's holds the keyboard right now. Each one
    /// claims it while it opens, so nothing may hand it back to the pane while
    /// any of them is up — the shell's own chord interceptor defers to them the
    /// same way.
    fn overlay_owns_keyboard(&self) -> bool {
        self.settings_open || self.palette.is_open() || self.trust.is_open()
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
        let name = if cx.theme().is_dark() {
            config.terminal.palette_dark.as_str()
        } else {
            config.terminal.palette_light.as_str()
        };
        let theme = TerminalTheme::builtin(name).unwrap_or_default();
        self.terminal
            .update(cx, |view, cx| view.set_theme(theme, cx));
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
                | ShellRequest::SaveLayout
                | ShellRequest::RestoreLayout
                | ShellRequest::ClearEvents
                | ShellRequest::CompareFleet
                | ShellRequest::ClearFleet) => {
                    entity.update(cx, |shell, cx| {
                        shell.dispatch(request, window, cx);
                        cx.notify();
                    });
                }
            }
        });
        let prefix = self.prefix_armed.then(|| self.keymap.prefix_label());
        let invocation = Invocation {
            snapshot: live.snapshot.as_deref(),
            surface: live.surface.as_deref(),
            status: live.status,
            status_error: error.as_deref(),
            rejection: self.move_error.as_deref().or(live.rejected.as_deref()),
            prefix: prefix.as_deref(),
            plugins: &plugins,
            ui_data: &self.ui_data,
            layout: &self.layout,
            events: &self.events,
            fleet: &self.fleet,
            tails: &self.tails,
            theme: &theme,
            dispatch: &dispatch,
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
                    .children(
                        self.registry
                            .panels_in(self.rail)
                            .map(|panel| panel.render(&invocation, window, cx)),
                    ),
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
            .child(title_bar(&invocation, sidebar_visible))
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
                &self.palette_focus,
                &dispatch,
                cx.listener(Self::palette_key),
                cx,
            ));
        }
        if self.settings_open {
            root = root.child(settings::overlay(
                self.registry.settings_sections(),
                &self.settings_focus,
                &dispatch,
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
        .h(px(TAB_STRIP_HEIGHT))
        .px_1()
        .gap_1()
        .items_center()
        .bg(theme.tab_bar)
        .border_b_1()
        .border_color(theme.border)
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
        .rounded_sm()
        .cursor_pointer()
        .text_xs()
        .text_color(theme.muted_foreground)
        .hover(|style| style.bg(theme.muted))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(Command::ToggleAside), window, cx);
        })
        .child("x")
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
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .bg(if selected {
            theme.tab_active
        } else {
            theme.tab
        })
        .text_color(if selected {
            theme.tab_active_foreground
        } else {
            theme.tab_foreground
        })
        .hover(|style| style.bg(theme.tab_active))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::ShowAsideTab(tab), window, cx);
        })
        .child(Icon::default().data(tab.icon()).w(px(13.)).h(px(13.)))
        .child(
            div()
                .text_xs()
                .when(selected, |label| label.font_weight(FontWeight::SEMIBOLD))
                .child(tab.label()),
        )
        .children((count > 0).then(|| {
            div()
                .text_xs()
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
    bar.into_any_element()
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

/// The activity rail: one button per sidebar view, the current one picked out.
/// A click is a local request — the daemon has no opinion about what the client
/// is looking at.
fn rail(current: SidebarView, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    v_flex()
        .w(px(RAIL_WIDTH))
        .h_full()
        .flex_none()
        .items_center()
        .py_1()
        .gap_0p5()
        .bg(theme.sidebar)
        .border_r_1()
        .border_color(theme.sidebar_border)
        .children(SidebarView::ALL.iter().map(|view| {
            let view = *view;
            let selected = view == current;
            let dispatch = invocation.dispatch.clone();
            div()
                .id(("rail", view as usize))
                .w(px(30.))
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .bg(if selected {
                    theme.sidebar_accent
                } else {
                    theme.sidebar
                })
                .text_color(if selected {
                    theme.sidebar_primary
                } else {
                    theme.muted_foreground
                })
                .hover(|style| style.bg(theme.sidebar_accent))
                .on_click(move |_, window, cx| {
                    dispatch(ShellRequest::ShowView(view), window, cx);
                })
                .child(Icon::default().data(view.icon()).w(px(16.)).h(px(16.)))
        }))
        .into_any_element()
}

/// The cwd the layout block speaks for: the focused pane's, preferring the cwd the
/// daemon carries for its foreground process. A snapshot with no focused pane has
/// nothing to walk up from.
fn layout_cwd(snapshot: &ClientShellSnapshot) -> Option<&str> {
    let id = snapshot.focused_pane_id.as_deref()?;
    let pane = snapshot.panes.iter().find(|pane| pane.pane_id == id)?;
    pane.foreground_cwd.as_deref().or(pane.cwd.as_deref())
}

/// The chrome above the grid: which workspace is in front, and the local
/// toggles. Window controls stay the platform's until M7 has a frameless
/// window to draw them on.
fn title_bar(invocation: &Invocation, sidebar_visible: bool) -> AnyElement {
    let theme = invocation.theme;
    let title = invocation
        .snapshot
        .and_then(|snapshot| {
            let id = snapshot.focused_workspace_id.as_ref()?;
            snapshot
                .workspaces
                .iter()
                .find(|workspace| &workspace.workspace_id == id)
                .map(|workspace| workspace.label.clone())
        })
        .unwrap_or_default();
    h_flex()
        .flex_none()
        .w_full()
        .h(px(TITLE_BAR_HEIGHT))
        .px_2()
        .gap_2()
        .items_center()
        .bg(theme.title_bar)
        .border_b_1()
        .border_color(theme.title_bar_border)
        .child(toggle_button(
            "toggle-sidebar",
            if sidebar_visible { "<" } else { ">" },
            Command::ToggleSidebar,
            invocation,
        ))
        .child(div().flex_1().text_sm().child(title))
        .child(toggle_button(
            "toggle-aside",
            crate::i18n::t("ai.chat.title"),
            Command::ToggleAside,
            invocation,
        ))
        .child(toggle_button(
            "settings",
            crate::i18n::t("settings.title"),
            Command::ToggleSettings,
            invocation,
        ))
        .child(toggle_button(
            "toggle-theme",
            // The button names the mode it would switch to, so the label is the
            // other mode's name.
            if theme.is_dark() {
                crate::i18n::t("settings.appearance.theme_light")
            } else {
                crate::i18n::t("settings.appearance.theme_dark")
            },
            Command::ToggleTheme,
            invocation,
        ))
        .into_any_element()
}

fn toggle_button(
    id: &'static str,
    label: impl Into<SharedString>,
    command: Command,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .text_xs()
        .bg(theme.secondary)
        .text_color(theme.secondary_foreground)
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(command), window, cx);
        })
        .child(label.into())
        .into_any_element()
}

/// The focused workspace's tabs, then the button that opens another one.
fn tab_strip(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let tabs: Vec<&ClientShellTab> = invocation
        .snapshot
        .map(|snapshot| {
            let workspace = snapshot.focused_workspace_id.as_ref().and_then(|id| {
                snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| &workspace.workspace_id == id)
            });
            match workspace {
                Some(workspace) => snapshot
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace.workspace_id)
                    .collect(),
                None => Vec::new(),
            }
        })
        .unwrap_or_default();
    let mut strip = h_flex()
        .flex_none()
        .w_full()
        .h(px(TAB_STRIP_HEIGHT))
        .px_1()
        .gap_1()
        .items_center()
        .bg(theme.tab_bar)
        .border_b_1()
        .border_color(theme.border);
    let count = tabs.len();
    let dispatch = invocation.dispatch.clone();
    strip = strip
        .children(
            tabs.into_iter()
                .enumerate()
                .map(|(index, tab)| tab_button(tab, index, invocation)),
        )
        // The strip is the gap after the last tab: a drop that no tab claimed
        // lands here and appends.
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
        .child(toggle_button("new-tab", "+", Command::NewTab, invocation))
        .into_any_element()
}

fn tab_button(tab: &ClientShellTab, index: usize, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let open = invocation.dispatch.clone();
    let close = invocation.dispatch.clone();
    let drop = invocation.dispatch.clone();
    let tab_id = tab.tab_id.clone();
    let label = if tab.zoomed {
        format!("{} ^", tab.label)
    } else {
        tab.label.clone()
    };
    h_flex()
        .id(("tab", tab.number))
        .px_2()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .bg(if tab.focused {
            theme.tab_active
        } else {
            theme.tab
        })
        .text_color(if tab.focused {
            theme.tab_active_foreground
        } else {
            theme.tab_foreground
        })
        .cursor_pointer()
        .on_click(move |_, window, cx| {
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
        .child(div().text_sm().child(label))
        .child(
            div()
                .id(("tab-close", tab.number))
                .px_0p5()
                .text_xs()
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(ShellRequest::Command(Command::CloseTab), window, cx);
                })
                .child("x"),
        )
        .into_any_element()
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
}
