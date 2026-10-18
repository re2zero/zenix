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
pub(crate) mod commands;
pub(crate) mod keymap;
pub(crate) mod palette;
pub(crate) mod panel_resize;
pub(crate) mod panels;
pub(crate) mod registry;
pub(crate) mod settings;

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::{ActiveTheme, ThemeMode, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::ClientShellTab;

use crate::config::{self, ASIDE_WIDTH_DEFAULT, ConfigStore, SIDEBAR_WIDTH_DEFAULT};
use crate::herdr::Connection;
use crate::plugins::host::PluginHost;
use crate::plugins::model::Availability;
use crate::plugins::trust_view::{self, TrustReview};
use crate::plugins::ui_data::{self, UiData, UiDataHost};
use crate::plugins::ui_manifest::UiContributions;
use crate::terminal::Theme as TerminalTheme;
use crate::terminal::view::TerminalView;
use commands::{COMMANDS, Command};
use keymap::{Keymap, RunCommand};
use palette::{Entry, Key as PaletteKey, Palette};
use panel_resize::{ASIDE, AsideDrag, Edge, PanelWidth, SIDEBAR, SidebarDrag};
use registry::{Dispatch, Invocation, Registry, ShellRequest};

const TITLE_BAR_HEIGHT: f32 = 34.;
const TAB_STRIP_HEIGHT: f32 = 28.;

pub(crate) struct ShellView {
    terminal: Entity<TerminalView>,
    connection: Connection,
    registry: Registry,
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
            if revision != shell.drawn {
                shell.drawn = revision;
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
            keymap: Keymap::default(),
            sidebar,
            sidebar_dragging: false,
            aside,
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
            prefix_armed: false,
            drawn: 0,
            _observe: observe,
            _prefix: prefix,
            _config: config,
            _plugin_pulses: plugin_pulses,
            _ui_data_pulses: ui_data_pulses,
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
        if let Some(command) = chord {
            self.run(command, window, cx);
        }
        // A chord with nothing bound to it is swallowed, as in herdr.
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
    /// A plugin's rows come from the daemon's action list, because that is what
    /// can actually run; a declaration adds only what it knows and the daemon
    /// does not — the chord to show beside the row, and a narrower `when` scope.
    fn palette_entries(&self, cx: &App) -> Vec<Entry> {
        let availability = Availability::of(self.terminal.read(cx).live().snapshot.as_deref());
        let mut entries: Vec<Entry> = COMMANDS
            .iter()
            .map(|info| Entry::Command(info.command))
            .collect();
        let plugins = self.plugins.state();
        for action in plugins
            .actions
            .iter()
            .filter(|action| action.is_available(availability))
        {
            let declared = plugins.ui.commands.iter().find(|command| {
                command.plugin_id == action.plugin_id && command.action_id == action.action_id
            });
            if let Some(declared) = declared
                && !availability.satisfies(&declared.when)
            {
                continue;
            }
            entries.push(Entry::Plugin {
                action: action.clone(),
                declared: declared.cloned(),
            });
        }
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
            ShellRequest::FocusWorkspace(workspace_id) => {
                self.connection.focus_workspace(&workspace_id)
            }
            ShellRequest::FocusTab(tab_id) => self.connection.focus_tab(&tab_id),
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
        }
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
                request @ (ShellRequest::PluginAction { .. }
                | ShellRequest::OpenPluginPane { .. }
                | ShellRequest::ClosePluginPane(_)
                | ShellRequest::ShowPluginPanels
                | ShellRequest::RefreshPlugins
                | ShellRequest::OpenPluginTrust
                | ShellRequest::ReviewPlugin
                | ShellRequest::OpenPluginTrustAt(_)
                | ShellRequest::LinkPlugin(_)
                | ShellRequest::ClosePluginTrust) => {
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
            rejection: live.rejected.as_deref(),
            prefix: prefix.as_deref(),
            plugins: &plugins,
            ui_data: &self.ui_data,
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
                            .panels()
                            .iter()
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
                // The grid needs the remaining height: inside a flex
                // column a childless `size_full` collapses to the height
                // its own content asks for, which for a canvas is one row.
                .child(div().flex_1().min_h_0().child(self.terminal.clone())),
        );
        if aside_visible {
            let aside_width = self
                .aside
                .width(window.viewport_size().width.to_f64() as f32);
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
                    .children(
                        self.registry
                            .aside_panels()
                            .iter()
                            .map(|panel| panel.render(&invocation, window, cx)),
                    ),
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
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .py_0p5()
                    .gap_3()
                    .items_center()
                    .bg(theme.status_bar)
                    .border_t_1()
                    .border_color(theme.status_bar_border)
                    .children(
                        self.registry
                            .status_segments()
                            .iter()
                            .map(|segment| segment.render(&invocation, window, cx)),
                    ),
            );
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
    strip = strip.children(tabs.into_iter().map(|tab| tab_button(tab, invocation)));
    strip
        .child(toggle_button("new-tab", "+", Command::NewTab, invocation))
        .into_any_element()
}

fn tab_button(tab: &ClientShellTab, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let open = invocation.dispatch.clone();
    let close = invocation.dispatch.clone();
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
