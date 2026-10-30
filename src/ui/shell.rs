//! The window's root: one daemon connection, the pushed state it owns, and the
//! chrome [`super::chrome`] composes from it.
//!
//! The connection used to belong to the terminal view (the old shell read the
//! state through it). U2-1 returns the terminal as a component of this root,
//! not as its owner: the shell starts the connection, keeps the last pulled
//! [`LiveState`], and repaints when the reader says something changed. Nothing
//! polls; the reader thread pulses a channel after every applied event and a
//! foreground task pulls the state and notifies.
//!
//! U2-3 mounts the pane grid (`crate::terminal::view::TerminalView`) as that
//! component: [`Root::pull`] hands every pulse's state to it — including the
//! pulses that do not move the revision, because a find answer or a split
//! ratio's acknowledgement is what the next request waits on — and the config
//! and appearance observers hand it the terminal theme and metrics. The grid
//! starts with the keyboard focus; a press in it keeps it there.
//!
//! The window's close path saves its geometry here (`save_window_bounds`),
//! registered as the library's close guard by [`crate::run`], so the next
//! launch opens the shape this one closed with — from the client-decorated
//! strip's close button and from a window manager's close alike.

use ely_core::forms::{InputEvent, TextInput};
use ely_core::layout::AppShell;
use ely_core::shell::{ResizeBorder, guard_close};
use ely_core::theme::ActiveTheme as _;
use gpui::{
    App, ClipboardItem, Context, DragMoveEvent, Entity, FocusHandle, IntoElement, MouseButton,
    MouseUpEvent, ParentElement, Render, Subscription, SystemNotification, Task, Window,
    WindowBounds, div, prelude::*,
};
use herdr_client::ConnectTarget;

use super::{
    aside, chat, chrome, commands, confirm, empty, fleet, inspector, keymap, layout_sheet, manager,
    palette, review, settings, sidebar, theme, toast, trust, worktree,
};
use crate::api_provider::ApiFormat;
use crate::config;
use crate::events::{EventFilter, EventState};
use crate::fleet::FleetState;
use crate::herdr::{Connection, ConnectionStatus, LiveState};
use crate::i18n::t;
use crate::layout::LayoutState;
use crate::plugins::actions::Catalog;
use crate::plugins::host::{PluginHost, PluginState};
use crate::plugins::model::Availability;
use crate::review::{DiffState, ReviewHost, ReviewState};
use crate::terminal::view::TerminalView;
use crate::worktrees::{WorktreeHost, WorktreeState, WorktreeTarget};

/// Below this width the sidebar folds away and the rail is left alone; the
/// design's breakpoint, and a client-side decision that is never written back
/// into the config.
const SIDEBAR_BREAKPOINT: f32 = 900.;
/// Below this width the aside hides: the column needs room beside the grid, and
/// a folded rail would leave the grid too little. `layout.aside_visible` says
/// whether the reader wants it; this is whether the window can hold it.
const ASIDE_BREAKPOINT: f32 = 1180.;

/// What the right edge draws at a width: the open panel, the rail of a panel
/// the reader has closed, or nothing when the window is too narrow to hold the
/// panel the reader asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsideState {
    Open,
    Rail,
    Hidden,
}

/// The window's content: the shell frame over the daemon's pushed state.
pub struct Root {
    /// The one endpoint connection. Cloning it shares the reader and its
    /// state; the shell hands clones to the chrome's controls.
    connection: Connection,
    /// The last state the pulse task pulled. A paint clones nothing.
    live: LiveState,
    /// The pane grid: the painter, the cards, split dragging and the terminal's
    /// input. It is a component of this root, not a second owner of the
    /// connection — every pulse hands it the state pulled here.
    panes: Entity<TerminalView>,
    /// The aside's conversation: its own turn, composer and history. The shell
    /// decides only whether it is on screen.
    chat: Entity<chat::ChatView>,
    /// Which aside tab is in front, and the fleet tab's own state — the mode,
    /// the panes picked for TAIL and the tails read for them. Both belong to
    /// the client, not to the snapshot.
    aside_tab: aside::Tab,
    /// The panel's width, dragged by its edge. The config stores what a
    /// gesture leaves; this is the live value the column is drawn at, capped
    /// by the window's share.
    aside_width: aside::Width,
    fleet_panel: fleet::State,
    /// The event stream's ring and filter, the fleet's watched status, the
    /// plugin registry and the focused repo's worktrees: the sidebar's four
    /// sources beside the snapshot. Each is a copy taken on a pulse, so a
    /// paint reads one value instead of locking a cache per row.
    events: EventState,
    fleet: FleetState,
    plugins: PluginState,
    worktrees: WorktreeState,
    /// The focused workspace the worktree list is scoped to. A snapshot that
    /// moves the focus is the one moment the repo behind the list can change.
    worktrees_scope: Option<String>,
    /// The new-worktree form: its two fields and the daemon's answer.
    worktree_form: worktree::State,
    /// The two fields' submission handlers, alive exactly while the form is.
    _worktree_inputs: Vec<Subscription>,
    event_stream: Option<crate::events::Stream>,
    fleet_stream: Option<crate::fleet::Stream>,
    plugin_host: Option<PluginHost>,
    worktree_host: Option<WorktreeHost>,
    /// Which sidebar view is in front. The daemon has no opinion about what
    /// the client is looking at, so the rail's choice is the shell's own.
    rail: chrome::View,
    /// The command directory's chords, resolved for this host.
    keymap: keymap::Keymap,
    /// Whether the prefix has been typed and the next key is its chord.
    prefix_armed: bool,
    /// The palette's own state: open, and what was run recently. The query
    /// and the cursor inside one opening belong to the library.
    palette: palette::State,
    /// The settings dialog: which page is open, and the provider file's
    /// contents the AI page reads. Its controls write the config themselves.
    settings: settings::State,
    /// The raised surfaces U3-4 adds: the plugin manager drawer, the pre-link
    /// trust review, the event inspector (and its contract) and the layout
    /// inspector. Each owns only its own page state; the data they read is the
    /// host state above.
    manager: manager::State,
    trust: trust::State,
    inspector: inspector::State,
    layout_sheet: layout_sheet::State,
    /// The review sheet: whether it is open, and the file the right column
    /// shows. What it draws comes from the two caches below it.
    review: review::State,
    review_state: ReviewState,
    review_diff: DiffState,
    review_host: ReviewHost,
    /// The two drawers hold the keyboard through these handles while they are
    /// open; the modal surfaces take their own. Focusing and handing back are
    /// the shell's because it owns the grid's handle.
    manager_focus: FocusHandle,
    inspector_focus: FocusHandle,
    /// The pending confirmation (a worktree removal or a plugin unlink). Only
    /// the card's confirm button reaches the daemon.
    confirm: Option<confirm::Confirm>,
    /// The focused pane's project template, re-read when the pane's cwd moves.
    /// Its scan is a handful of directory checks, never a file watcher.
    layout_state: LayoutState,
    /// The cwd the scan above was taken for, so a pulse that did not move it
    /// does not read the file again.
    layout_cwd: Option<String>,
    /// The first-run block's daemon card: its log tail, the last start's
    /// answer, and whether one is running (U3-5). Nothing here polls; the card
    /// reads the log once when asked.
    empty: empty::State,
    /// A daemon start running off the UI thread, because seeding the binary is
    /// file work.
    _daemon_task: Option<Task<()>>,
    /// The layout inspector's directory field, alive while the sheet offers it,
    /// and the field's submission handler.
    _layout_input: Option<Subscription>,
    /// The window's short-lived messages (U3-5). The viewport is the library's;
    /// this is the one stack the shell pushes to.
    toasts: Entity<ely_core::feedback::Toaster>,
    /// The daemon target this window's launch named, and the JSON lane beside
    /// its endpoint connection — `None` on a remote window, whose lane surfaces
    /// (events, fleet, plugins, worktrees, layout, the chat's daemon tools)
    /// stay dark rather than read this machine's daemon.
    target: ConnectTarget,
    lane: Option<crate::herdr::api::ApiClient>,
    /// The last values toasted about, so a pulse that did not change them does
    /// not say the same thing twice: the daemon's last refusal and the two
    /// hosts' last failure reasons.
    rejected_seen: Option<String>,
    worktree_error_seen: Option<String>,
    plugin_error_seen: Option<String>,
    /// The trust field's submission handler, alive exactly while the dialog's
    /// field is.
    _trust_input: Option<Subscription>,
    /// A layout export or apply running off the UI thread. Replacing it on the
    /// next ask cancels the request it replaced.
    _layout_task: Option<Task<()>>,
    /// The app-wide keystroke interceptor: the prefix has to claim the next
    /// key wherever focus is, which a bound action could not do.
    _keys: Subscription,
    /// The revision [`Root::live`] was taken at, so a pulse that changed
    /// nothing the chrome draws does not repaint.
    drawn: u64,
    /// The pulse drain: held (not read) so it lives exactly as long as the
    /// window, and dropped — cancelling the task and its inbox — with it.
    _pulses: Task<()>,
    /// One wake-up task per host. Each reads its cache and repaints; the
    /// worker thread that wrote it never touches GPUI.
    _plugin_pulses: Option<Task<()>>,
    _worktree_pulses: Option<Task<()>>,
    _event_pulses: Option<Task<()>>,
    _fleet_pulses: Option<Task<()>>,
    _review_pulses: Task<()>,
    /// A config write can change the mode, locale or metrics the chrome reads.
    _config: Subscription,
    /// A system appearance change has to move a `mode: system` chrome.
    _appearance: Subscription,
    /// The fleet tab's tail read, while one is in flight. Replacing it on the
    /// next ask cancels the read it replaced; dropping it with the window does
    /// the same.
    _fleet_read: Option<Task<()>>,
}

impl Root {
    /// Built for every window [`crate::run`] or `view.new-window` opens; each
    /// owns its endpoint connection, hosts and streams, on the daemon target
    /// its launch named.
    pub fn new(window: &mut Window, target: ConnectTarget, cx: &mut Context<Self>) -> Self {
        let (pulse, pulses) = crossbeam_channel::unbounded::<()>();
        let connection = Connection::start(target.clone(), move || {
            // A closed inbox means the window that owns this connection is
            // gone; the reader stops instead of retrying forever.
            pulse.try_send(()).is_ok()
        });

        // The reader thread never touches GPUI: it pulses this foreground
        // task, which pulls the state and repaints. Nothing polls the daemon.
        let pulses = cx.spawn(async move |root, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                if root.update(cx, |root, cx| root.pull(cx)).is_err() {
                    break;
                }
            }
        });

        // The JSON lane beside the endpoint: a local target has one (its API
        // socket sits beside the endpoint socket), and the sidebar's four
        // sources read it — each host owns its worker thread and pulses this
        // side when its cache moved; none of them is polled by the paint, and
        // the plugin registry's own fallback tick is the host's, not the
        // shell's. A remote target has no lane here — its API socket lives on
        // the other machine — so those sources stay dark rather than quietly
        // reading this machine's daemon.
        let lane = crate::herdr::api::ApiClient::for_target(&target);
        let (plugin_host, plugin_pulses) = match lane.as_ref() {
            Some(client) => {
                let (host, pulses) = PluginHost::start(client.clone());
                (
                    Some(host),
                    Some(Self::follow(cx, pulses, Self::read_plugins)),
                )
            }
            None => (None, None),
        };
        let (worktree_host, worktree_pulses) = match lane.as_ref() {
            Some(client) => {
                let (host, pulses) = WorktreeHost::start(client.clone());
                (
                    Some(host),
                    Some(Self::follow(cx, pulses, Self::read_worktrees)),
                )
            }
            None => (None, None),
        };
        let (event_stream, event_pulses) = match lane.as_ref() {
            Some(client) => {
                let (stream, pulses) = crate::events::Stream::start(client.clone());
                (
                    Some(stream),
                    Some(Self::follow(cx, pulses, Self::read_events)),
                )
            }
            None => (None, None),
        };
        let (fleet_stream, fleet_pulses) = match lane.as_ref() {
            Some(client) => {
                let (stream, pulses) = crate::fleet::Stream::start(client.clone());
                (
                    Some(stream),
                    Some(Self::follow(cx, pulses, Self::read_fleet)),
                )
            }
            None => (None, None),
        };
        let (review_host, review_pulses) = ReviewHost::start();
        let review_pulses = Self::follow(cx, review_pulses, Self::read_review);

        // The pane grid shares this connection; it owns no reader of its own.
        // It starts dressed for the config's mode and metrics, and the keyboard
        // belongs to the grid until something else asks for it.
        let starting = connection.state();
        let panes = cx.new(|cx| TerminalView::new(connection.clone(), starting, cx));
        panes.update(cx, |panes, cx| {
            panes.set_theme(terminal_theme(cx), cx);
            let terminal = &config::get(cx).terminal;
            panes.set_metrics(
                terminal.font_size,
                terminal.line_height,
                crate::terminal::CursorShape::from_name(&terminal.cursor),
                cx,
            );
        });
        let panes_focus = panes.read(cx).focus_handle().clone();
        panes_focus.focus(window, cx);

        // The aside's conversation. It is built closed: `layout.aside_visible`
        // is the reader's intent, and the config's default is off until the
        // panel is asked for. On a window without a JSON lane the model
        // conversation runs but the daemon tools are not offered.
        let chat = cx.new(|cx| chat::ChatView::new(window, lane.is_some(), cx));
        let aside_wanted = config::get(cx).layout.aside_visible;
        chat.update(cx, |chat, cx| chat.set_on_screen(aside_wanted, cx));
        // The panel opens at the width the config remembers; a drag moves the
        // value this shell owns and the release writes it back.
        let mut aside_width = aside::Width::new();
        aside_width.restore(config::get(cx).layout.aside_width);

        // The MCP surface: one orchestrator socket per process, in the config
        // directory, started at the first window and serving until the
        // process ends. Agents have reached it there since M6; the settings
        // dialog's MCP page states what it is serving.
        crate::mcp::ensure_orchestrator();

        // The config has already dressed the theme in `config::init`; a later
        // write has done the same in its apply, so this only repaints and
        // passes the new layers to the grid.
        let config = cx.observe_global::<config::ConfigStore>(|root, cx| {
            root.panes.update(cx, |panes, cx| {
                panes.set_theme(terminal_theme(cx), cx);
                let terminal = &config::get(cx).terminal;
                panes.set_metrics(
                    terminal.font_size,
                    terminal.line_height,
                    crate::terminal::CursorShape::from_name(&terminal.cursor),
                    cx,
                );
            });
            // The panel's own visibility is a config value: the toggle writes
            // it, and the conversation's unread rule follows it here.
            let wanted = config::get(cx).layout.aside_visible;
            root.chat
                .update(cx, |chat, cx| chat.set_on_screen(wanted, cx));
            cx.notify();
        });
        // A `mode: system` chrome has to follow the system while the window is
        // open. `theme::sync` re-resolves the mode and starts its fade; the
        // refresh draws the frames the fade needs, and the grid takes the new
        // terminal tone beside it.
        let follow_panes = panes.clone();
        let appearance = window.observe_window_appearance(move |window, cx| {
            theme::sync(cx);
            follow_panes.update(cx, |panes, cx| panes.set_theme(terminal_theme(cx), cx));
            window.refresh();
        });

        // The command directory's chords are resolved once, for the host this
        // window runs on; the prefix interceptor claims them ahead of focus.
        let keymap = keymap::Keymap::new(ely_core::typography::keys::os(cx.theme().platform));
        let keys = Self::intercept_prefix(cx);
        let manager_focus = cx.focus_handle();
        let inspector_focus = cx.focus_handle();
        // The one stack of short-lived messages this window shows (U3-5).
        let toasts = cx.new(|_| ely_core::feedback::Toaster::default());

        Self {
            connection,
            live: LiveState::default(),
            panes,
            chat,
            aside_tab: aside::Tab::Chat,
            aside_width,
            fleet_panel: fleet::State::default(),
            events: EventState::default(),
            fleet: FleetState::default(),
            plugins: PluginState::default(),
            worktrees: WorktreeState::default(),
            worktrees_scope: None,
            worktree_form: worktree::State::default(),
            _worktree_inputs: Vec::new(),
            event_stream,
            fleet_stream,
            plugin_host,
            worktree_host,
            rail: chrome::View::Workspaces,
            keymap,
            prefix_armed: false,
            palette: palette::State::default(),
            settings: settings::State::default(),
            manager: manager::State::default(),
            trust: trust::State::default(),
            inspector: inspector::State::default(),
            layout_sheet: layout_sheet::State::default(),
            review: review::State::default(),
            review_state: ReviewState::default(),
            review_diff: DiffState::default(),
            review_host,
            manager_focus,
            inspector_focus,
            confirm: None,
            layout_state: LayoutState::default(),
            layout_cwd: None,
            empty: empty::State::default(),
            _daemon_task: None,
            _layout_input: None,
            toasts,
            target: target.clone(),
            lane: lane.clone(),
            rejected_seen: None,
            worktree_error_seen: None,
            plugin_error_seen: None,
            _trust_input: None,
            _layout_task: None,
            _keys: keys,
            drawn: 0,
            _pulses: pulses,
            _plugin_pulses: plugin_pulses,
            _worktree_pulses: worktree_pulses,
            _event_pulses: event_pulses,
            _fleet_pulses: fleet_pulses,
            _review_pulses: review_pulses,
            _config: config,
            _appearance: appearance,
            _fleet_read: None,
        }
    }

    /// Wake on a host's pulse and read its cache, without polling: the same
    /// shape the daemon's own reader takes.
    fn follow(
        cx: &mut Context<Self>,
        pulses: crossbeam_channel::Receiver<()>,
        read: fn(&mut Self, &mut Context<Self>),
    ) -> Task<()> {
        cx.spawn(async move |root, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                if root.update(cx, read).is_err() {
                    break;
                }
            }
        })
    }

    fn read_plugins(&mut self, cx: &mut Context<Self>) {
        self.plugins = self
            .plugin_host
            .as_ref()
            .map(|host| host.state())
            .unwrap_or_default();
        // A link asked for from the trust dialog is answered in the registry's
        // own words: the daemon pushes nothing, so the next read is where the
        // outcome appears.
        if self.trust.awaiting()
            && let Some(last) = self.plugins.last.clone()
        {
            self.trust.note(last);
        }
        // A refusal has no log entry and no page of its own when the action
        // came from a chord or the palette; the toast is where it lands. The
        // value is compared so a pulse that changed nothing says nothing.
        if self.plugins.last_error != self.plugin_error_seen {
            self.plugin_error_seen = self.plugins.last_error.clone();
            if let Some(reason) = self.plugins.last_error.as_deref() {
                toast::host_failed(&self.toasts, t("shell.toast.plugin_failed"), reason, cx);
            }
        }
        // The plugin half of the keymap is read from the registry, so it is
        // rebuilt here rather than at construction. Disabled plugins are
        // filtered first — the daemon lists their actions but refuses them —
        // and a directory chord wins a collision.
        let catalog =
            Catalog::build(&self.plugins.actions, &self.plugins.ui).runnable(&self.plugins.plugins);
        self.keymap.set_plugins(&catalog);
        cx.notify();
    }

    fn read_worktrees(&mut self, cx: &mut Context<Self>) {
        self.worktrees = self
            .worktree_host
            .as_ref()
            .map(|host| host.state())
            .unwrap_or_default();
        // A create asked from the form is answered in the host's own words;
        // the daemon pushes `worktree.created`, so the list beside the form
        // updates in the same pulse.
        if self.worktree_form.awaiting()
            && let Some(last) = self.worktrees.last.clone()
        {
            self.worktree_form.note(last);
        }
        // A removal's refusal is the design's "show the daemon's reason": the
        // card that asked is gone by now, and the row that lost the request
        // has no room for a sentence.
        if self.worktrees.last_error != self.worktree_error_seen {
            self.worktree_error_seen = self.worktrees.last_error.clone();
            if let Some(reason) = self.worktrees.last_error.as_deref() {
                toast::host_failed(&self.toasts, t("shell.toast.worktree_failed"), reason, cx);
            }
        }
        cx.notify();
    }

    fn read_fleet(&mut self, cx: &mut Context<Self>) {
        self.fleet = self
            .fleet_stream
            .as_ref()
            .map(|stream| stream.state())
            .unwrap_or_default();
        cx.notify();
    }

    fn read_review(&mut self, cx: &mut Context<Self>) {
        self.review_state = self.review_host.state();
        self.review_diff = self.review_host.diff();
        // A refresh can take the picked file out of the listing; the sheet
        // goes back to the pick hint rather than holding a stale diff.
        self.review.prune_selection(&self.review_state.files);
        cx.notify();
    }

    /// The repository the focused workspace sits in, as the worktree host last
    /// reported it. Empty until a `worktree.list` has answered.
    fn review_root(&self) -> Option<String> {
        self.worktrees
            .source
            .as_ref()
            .map(|source| source.repo_root.clone())
            .filter(|root| !root.is_empty())
    }

    /// The focused repository's changes. With no repository to read the sheet
    /// still opens and says so, rather than refusing silently.
    pub(crate) fn open_review(&mut self, cx: &mut Context<Self>) {
        if self.lane.is_none() {
            self.review.open(Some(t("shell.remote.no_lane")));
            cx.notify();
            return;
        }
        match self.review_root() {
            Some(root) => {
                self.review.open(None);
                self.review_host.refresh(root);
            }
            None => self.review.open(Some(t("shell.review.no_repo"))),
        }
        cx.notify();
    }

    pub(crate) fn close_review(&mut self, cx: &mut Context<Self>) {
        self.review.close();
        cx.notify();
    }

    pub(crate) fn refresh_review(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.review_root() {
            self.review_host.refresh(root);
        }
        cx.notify();
    }

    /// Pick a file: the diff is read off the UI thread and lands on the
    /// generation this select advanced to.
    pub(crate) fn select_review_file(
        &mut self,
        path: String,
        staged: bool,
        untracked: bool,
        cx: &mut Context<Self>,
    ) {
        let generation = self.review.select(path.clone(), staged, untracked);
        if let Some(root) = self.review_root() {
            self.review_host
                .read_diff(root, path, staged, untracked, generation);
        }
        cx.notify();
    }

    /// A streamed event is also the one moment a worktree can have changed: the
    /// daemon announces it there, and the list is re-read now rather than
    /// waiting for the next focus change. A removal asks for the daemon's
    /// active workspace rather than a scope the removal may just have deleted.
    fn read_events(&mut self, cx: &mut Context<Self>) {
        self.events = self
            .event_stream
            .as_ref()
            .map(|stream| stream.state())
            .unwrap_or_default();
        if let Some(event) = self.events.events.front()
            && event.kind.starts_with("worktree.")
        {
            let scope = if event.kind == "worktree.removed" {
                None
            } else {
                self.worktrees_scope.clone()
            };
            if let Some(host) = self.worktree_host.as_ref() {
                host.refresh(scope);
            }
        }
        cx.notify();
    }

    /// The events view's chips: the ring keeps reading every kind; this only
    /// decides which slice the rows show.
    pub(crate) fn set_event_filter(&mut self, filter: EventFilter, cx: &mut Context<Self>) {
        if let Some(stream) = self.event_stream.as_ref() {
            stream.set_filter(filter);
        }
        self.events.filter = filter;
        cx.notify();
    }

    /// A worktree row was pressed and it is not open: ask the daemon to open
    /// it. The focused workspace names the repo the request is scoped to.
    pub(crate) fn open_worktree(&mut self, branch: Option<String>, path: String) {
        let Some(scope) = self.worktrees_scope.clone() else {
            return;
        };
        let target = match branch {
            Some(branch) => WorktreeTarget::Branch(branch),
            None => WorktreeTarget::Path(path),
        };
        if let Some(host) = self.worktree_host.as_ref() {
            host.open(scope, target);
        }
    }

    /// The worktrees section's `+` and the palette row: ask for a branch and
    /// a base before the daemon creates anything. The form's base starts at
    /// the focused workspace's own branch, which is the one a worktree is
    /// usually cut from.
    pub(crate) fn open_worktree_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.lane.is_none() {
            toast::not_sent(&self.toasts, &t("shell.remote.no_lane").to_string(), cx);
            return;
        }
        let branch = cx.new(|cx| {
            TextInput::new(window, cx).placeholder(t("shell.worktrees.form_branch_placeholder"))
        });
        let starting = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| {
                let id = snapshot.focused_workspace_id.as_ref()?;
                snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| &workspace.workspace_id == id)
                    .and_then(|workspace| workspace.branch.clone())
            })
            .unwrap_or_else(|| "main".to_owned());
        let base = cx.new(|cx| TextInput::new(window, cx).placeholder(starting.clone()));
        base.update(cx, |input, cx| input.set_text(starting, cx));
        let mut handlers = Vec::new();
        for input in [&branch, &base] {
            handlers.push(cx.subscribe_in(
                input,
                window,
                |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Submit) {
                        this.create_worktree(cx);
                    }
                },
            ));
        }
        self._worktree_inputs = handlers;
        self.worktree_form.open(branch.clone(), base);
        self.palette.open = false;
        self.prefix_armed = false;
        let focus = branch.read(cx).focus().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn close_worktree_form(&mut self, cx: &mut Context<Self>) {
        self.worktree_form.close();
        self._worktree_inputs.clear();
        cx.notify();
    }

    /// Create what the form names. An empty branch is the daemon's to generate
    /// only when nothing was typed; the form disables its button in that case,
    /// and Enter says why instead of sending a request it cannot name.
    pub(crate) fn create_worktree(&mut self, cx: &mut Context<Self>) {
        let (branch, base) = self.worktree_form.text(cx);
        if branch.is_empty() {
            self.worktree_form
                .note(t("shell.worktrees.form_need_branch"));
            cx.notify();
            return;
        }
        let Some(scope) = self.worktrees_scope.clone() else {
            self.worktree_form.note(t("shell.worktrees.form_no_repo"));
            cx.notify();
            return;
        };
        self.worktree_form.creating(
            rust_i18n::t!("shell.worktrees.form_creating", branch = branch.clone()).to_string(),
        );
        if let Some(host) = self.worktree_host.as_ref() {
            host.create(
                scope,
                Some(branch),
                Some(base).filter(|base| !base.is_empty()),
            );
        }
        cx.notify();
    }

    /// Put one of the rail's views in front. The rail and the workspace pill
    /// both come through here, so the two cannot disagree about what the
    /// sidebar shows.
    /// The short identity the chrome draws beside the brand: which daemon
    /// this window is a face of.
    pub(crate) fn target_label(&self) -> Option<String> {
        super::windows::target_label(&self.target)
    }

    /// Whether this window's daemon lane is this machine's socket. A remote
    /// window draws the honest degraded faces instead of local actions.
    pub(crate) fn lane_up(&self) -> bool {
        self.lane.is_some()
    }

    pub(crate) fn show_view(&mut self, view: chrome::View, cx: &mut Context<Self>) {
        if self.rail != view {
            self.rail = view;
            cx.notify();
        }
    }

    /// The prefix is decided before focus dispatch, so an armed shell claims
    /// the next key wherever the keyboard is — including inside a pane, whose
    /// application would otherwise type it. The interceptor is app-wide; the
    /// shell is one window, and a second window must claim only its own keys
    /// when multi-window arrives.
    fn intercept_prefix(cx: &mut Context<Self>) -> Subscription {
        let root = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
            let _ = root.update(cx, |root, cx| {
                root.prefix_keystroke(&event.keystroke, window, cx);
            });
        })
    }

    /// One keystroke through the keymap: a claimed chord runs at once, the
    /// prefix arms, and the key after an armed prefix is its chord.
    fn prefix_keystroke(
        &mut self,
        typed: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A raised surface owns the keyboard while it is open: the settings
        // dialog traps Tab, the palette's field must see every key, and the
        // drawers and modals answer Escape themselves. A shell chord must not
        // fire behind any of them, so nothing below runs until they close.
        if self.raised() {
            return;
        }
        // A chord carrying its own modifier is claimed outright, before the
        // prefix is even considered. An armed prefix does not survive it.
        if let Some(chord) = self.keymap.claimed(typed) {
            if self.prefix_armed {
                self.prefix_armed = false;
                cx.notify();
            }
            cx.stop_propagation();
            self.run_chord(chord, window, cx);
            return;
        }
        let is_prefix = self.keymap.is_prefix(typed);
        if !self.prefix_armed {
            if is_prefix {
                self.prefix_armed = true;
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let chord = self.keymap.chord(typed);
        self.prefix_armed = false;
        cx.notify();
        // Typing the prefix twice sends it on as ordinary input rather than
        // swallowing it, so a pane that binds Ctrl-B still receives it.
        if is_prefix {
            return;
        }
        cx.stop_propagation();
        if let Some(chord) = chord {
            self.run_chord(chord, window, cx);
        }
        // A chord with nothing bound to it is swallowed, as in herdr.
    }

    /// Run what a chord points at. A plugin action and a palette row go
    /// through the same dispatch, so the two cannot behave differently.
    fn run_chord(&mut self, chord: keymap::Chord, window: &mut Window, cx: &mut Context<Self>) {
        match chord {
            keymap::Chord::Command(command) => self.run_command(command, window, cx),
            keymap::Chord::Plugin(action) => self.run_plugin_action(action),
        }
    }

    /// Run one directory command: locally when the shell owns it, otherwise as
    /// the daemon method the snapshot resolves it to.
    pub(crate) fn run_command(
        &mut self,
        command: commands::Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            commands::Command::TogglePalette => self.toggle_palette(cx),
            // Both of these own the keyboard while open, so the view is the
            // only place that can start them.
            commands::Command::Find => {
                self.panes
                    .update(cx, |panes, cx| panes.open_find(window, cx));
            }
            commands::Command::CopyMode => {
                self.panes
                    .update(cx, |panes, cx| panes.enter_copy_mode(window, cx));
            }
            commands::Command::NewWorktree => self.open_worktree_form(window, cx),
            // The first workspace exists before any snapshot does, so this one
            // is not resolved against one.
            commands::Command::NewWorkspace => self.new_workspace(cx),
            commands::Command::OpenSettings => self.toggle_settings(cx),
            commands::Command::ToggleAside => self.toggle_aside(window, cx),
            commands::Command::LinkPlugin => self.open_trust_at(None, window, cx),
            commands::Command::InspectLayout => self.open_layout(window, cx),
            commands::Command::OpenReview => self.open_review(cx),
            // One more window, one more connection face — this window's own
            // target, so a chord on a remote window opens another window
            // there.
            commands::Command::NewWindow => {
                let target = self.target.clone();
                drop(super::windows::open_shell_window(cx, target));
            }
            command => {
                if let Some((method, params)) = self
                    .live
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| commands::request(command, snapshot))
                {
                    // A request the connection cannot take is a command that
                    // silently did nothing until U3-5; it says so now.
                    if self.connection.request(method, params).is_none() {
                        let what = command.info().map(|info| info.label()).unwrap_or_default();
                        toast::not_sent(&self.toasts, &what, cx);
                    }
                }
            }
        }
    }

    /// A new workspace from a control with no snapshot to resolve against —
    /// the first-run block and the sidebar's empty state. The request itself
    /// stays the directory's; this owns only what a lost send says.
    pub(crate) fn new_workspace(&mut self, cx: &mut Context<Self>) {
        let (method, params) = commands::new_workspace(self.live.snapshot.as_deref());
        if self.connection.request(method, params).is_none() {
            let what = commands::Command::NewWorkspace
                .info()
                .map(|info| info.label())
                .unwrap_or_default();
            toast::not_sent(&self.toasts, &what, cx);
        }
        cx.notify();
    }

    /// Show the palette, or hide it. The library keys its query field to the
    /// palette's element, so the field exists only while the palette is drawn:
    /// closing drops it, and the next opening starts with an empty query.
    pub(crate) fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.open {
            self.close_palette(cx);
            return;
        }
        self.prefix_armed = false;
        self.palette.open = true;
        cx.notify();
    }

    /// The palette closed, through Escape, a click outside, or a run. Focus
    /// was already handed back by the library's overlay.
    pub(crate) fn close_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.open {
            self.palette.open = false;
            cx.notify();
        }
    }

    /// Show the aside, or hide it. The panel's visibility is a config value
    /// (`layout.aside_visible`), so the toggle's choice survives the window —
    /// the title strip's button, the rail's tabs and the `view.toggle-chat`
    /// chord all land here, and the config observer tells the conversation.
    pub(crate) fn toggle_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = !config::get(cx).layout.aside_visible;
        config::update(cx, |config| config.layout.aside_visible = visible);
        // Only a panel the window can actually draw takes the keyboard: a
        // composer that is not on screen must not hold the keys. A panel
        // opened on the fleet has no field to focus, so the terminal keeps
        // them.
        if visible
            && self.aside_tab == aside::Tab::Chat
            && aside_state(true, window.viewport_size().width.as_f32()) == AsideState::Open
        {
            self.focus_chat(window, cx);
        }
        cx.notify();
    }

    /// Put one aside tab in front. The strip's press; the chat tab also gives
    /// the composer the keys, which is what a press on it is for.
    pub(crate) fn select_aside_tab(
        &mut self,
        tab: aside::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.aside_tab != tab {
            self.aside_tab = tab;
            cx.notify();
        }
        if tab == aside::Tab::Chat {
            self.focus_chat(window, cx);
        }
    }

    /// The rail's press: open the panel if the reader has it closed, then put
    /// `tab` in front. Pressing the tab that is already in front is a no-op
    /// rather than a toggle — the rail names tabs, and the strip's close
    /// button is the way to fold the panel.
    pub(crate) fn open_aside_tab(
        &mut self,
        tab: aside::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !config::get(cx).layout.aside_visible {
            config::update(cx, |config| config.layout.aside_visible = true);
        }
        self.select_aside_tab(tab, window, cx);
        cx.notify();
    }

    /// The aside's edge follows the pointer: the width runs from the pointer
    /// to the panel's right edge, which is the bounds of the column this
    /// listener sits on.
    fn drag_aside(
        &mut self,
        event: &DragMoveEvent<aside::AsideDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.aside_width.drag(
            f32::from(event.bounds.right()),
            f32::from(event.event.position.x),
        ) {
            cx.notify();
        }
    }

    /// A drag is over: save the width it left, once, however many frames it
    /// took. The release can also land outside the panel's edge, so both
    /// `on_mouse_up` and `on_mouse_up_out` come here.
    fn settle_aside(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.aside_width.take_unsaved() {
            let width = self
                .aside_width
                .chosen()
                .unwrap_or(config::ASIDE_WIDTH_DEFAULT);
            config::update(cx, |config| config.layout.set_aside_width(width));
        }
    }

    /// A double-click on the grip puts the panel back to the default width,
    /// and the config with it.
    pub(crate) fn reset_aside_width(&mut self, cx: &mut Context<Self>) {
        self.aside_width.reset();
        self.aside_width.take_unsaved();
        config::update(cx, |config| {
            config.layout.set_aside_width(config::ASIDE_WIDTH_DEFAULT)
        });
        cx.notify();
    }

    /// A card or a selection row was pressed: pick it for the TAIL comparison,
    /// or unpick it. A comparison that just reached two panes is the moment
    /// its tails are asked for; one pick is still a selection being made.
    pub(crate) fn fleet_toggle_pick(&mut self, pane: &str, cx: &mut Context<Self>) {
        self.fleet_panel.toggle_pick(pane);
        if self.fleet_panel.mode() == fleet::Mode::Compare && self.fleet_panel.picked().len() == 2 {
            self.fleet_read(cx);
        }
        cx.notify();
    }

    /// A card's terminal button: open the comparison on that pane — plus the
    /// one it was paired with, if the picks already hold two — and read them.
    pub(crate) fn fleet_open_tail(&mut self, pane: &str, cx: &mut Context<Self>) {
        if self.lane.is_none() {
            return;
        }
        self.fleet_panel.open_tail(pane);
        self.fleet_read(cx);
        cx.notify();
    }

    /// The roster / TAIL chips. Switching to the comparison reads a pair that
    /// is already picked; switching back keeps the picks and their tails, so
    /// the two modes are one state seen twice.
    pub(crate) fn fleet_set_mode(&mut self, mode: fleet::Mode, cx: &mut Context<Self>) {
        if self.fleet_panel.set_mode(mode) {
            if mode == fleet::Mode::Compare {
                self.fleet_read(cx);
            }
            cx.notify();
        }
    }

    /// The comparison's "pick again": drop the picks and the tails read for
    /// them, leaving the comparison's selection list in their place.
    pub(crate) fn fleet_clear_picks(&mut self, cx: &mut Context<Self>) {
        self.fleet_panel.clear_picks();
        cx.notify();
    }

    /// Read the picked panes' tails, on demand.
    ///
    /// `pane.read` is a socket call, so the read runs on the background
    /// executor and its answer comes back to this entity; a paint never waits
    /// on the daemon. The panel's `begin_read` refuses a second ask while one
    /// is in flight, and a generation number drops an answer that a newer ask
    /// superseded.
    pub(crate) fn fleet_read(&mut self, cx: &mut Context<Self>) {
        let Some((generation, panes)) = self.fleet_panel.begin_read() else {
            return;
        };
        cx.notify();
        let task = cx.spawn(async move |root, cx| {
            let tails = cx
                .background_executor()
                .spawn(async move {
                    let endpoint = crate::ai::endpoint::ApiEndpoint::local();
                    let mut tails = crate::fleet::Tails::new();
                    for pane in panes {
                        let tail = crate::fleet::read_tail(
                            &endpoint,
                            &pane,
                            fleet::READ_ROWS,
                            fleet::READ_LINES,
                        );
                        tails.insert(pane, tail);
                    }
                    tails
                })
                .await;
            let _ = root.update(cx, |root, cx| {
                if root.fleet_panel.finish_read(generation, tails) {
                    cx.notify();
                }
            });
        });
        self._fleet_read = Some(task);
    }

    /// The composer takes the keyboard: what the toggle, the strip's tab and
    /// the rail's chat button all mean when the panel is asked for.
    pub(crate) fn focus_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.chat.read(cx).composer_focus(cx);
        window.focus(&focus, cx);
    }

    /// Show the settings dialog, or hide it: the rail's gear and the settings
    /// chord both land here, so the two cannot disagree about the dialog.
    pub(crate) fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings.open {
            self.close_settings(cx);
        } else {
            self.open_settings(cx);
        }
    }

    /// Opening reads the provider file once — the AI page's facts — and takes
    /// the palette down: one raised surface at a time, and focus has one place
    /// to come back to.
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings.open {
            return;
        }
        self.settings.open = true;
        self.settings.reload();
        self.palette.open = false;
        self.prefix_armed = false;
        cx.notify();
    }

    /// The dialog closed: through its own close controls, a press on the
    /// scrim, or Escape. Focus was already handed back by the overlay.
    pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings.open {
            self.settings.open = false;
            self.prefix_armed = false;
            cx.notify();
        }
    }

    /// A page picked in the nav.
    pub(crate) fn set_settings_page(&mut self, page: settings::Page, cx: &mut Context<Self>) {
        if self.settings.page != page {
            self.settings.page = page;
            cx.notify();
        }
    }

    /// Put back the open page's own settings. Only the pages that own settings
    /// offer it; the reference pages have nothing to write.
    pub(crate) fn reset_settings_page(&mut self, cx: &mut Context<Self>) {
        let page = self.settings.page;
        config::update(cx, |config| settings::reset_page(page, config));
    }

    /// The AI page's provider: a page-local choice, not a second authority
    /// over what the chat panel runs on.
    pub(crate) fn choose_ai_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        self.settings.choose_provider(name);
        cx.notify();
    }

    /// The AI page's reasoning effort, persisted to the provider file the chat
    /// panel reads when it starts.
    pub(crate) fn set_ai_level(&mut self, token: &str, cx: &mut Context<Self>) {
        if let Some(name) = self
            .settings
            .selected()
            .map(|provider| provider.name.clone())
        {
            self.settings.set_level(token, &name);
        }
        cx.notify();
    }

    /// The AI page's provider form, adding one (`None`) or editing a row.
    /// Fields are built here because only a window can make a text input.
    pub(crate) fn open_ai_provider_form(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = index
            .and_then(|index| self.settings.provider_at(index))
            .map(|provider| {
                (
                    provider.name.clone(),
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    provider.format,
                    provider.url_suffix.clone(),
                )
            });
        let (name, base_url, api_key, format, suffix) = existing.unwrap_or_else(|| {
            (
                String::new(),
                ApiFormat::OpenAI.default_base_url().to_owned(),
                String::new(),
                ApiFormat::OpenAI,
                ApiFormat::OpenAI.default_suffix().to_owned(),
            )
        });
        let form = settings::ProviderForm::new(
            index,
            settings::text_field(
                window,
                cx,
                &name,
                t("settings.ai.provider_name_hint"),
                false,
            ),
            settings::text_field(window, cx, &base_url, String::new(), false),
            settings::text_field(window, cx, &api_key, "sk-…".to_owned(), true),
            format,
            suffix,
        );
        let focus = form.first();
        self.settings.begin_provider(form);
        let handle = focus.read(cx).focus().clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    pub(crate) fn cancel_ai_provider_form(&mut self, cx: &mut Context<Self>) {
        self.settings.cancel_provider();
        cx.notify();
    }

    pub(crate) fn save_ai_provider_form(&mut self, cx: &mut Context<Self>) {
        self.settings.save_provider(cx);
        cx.notify();
    }

    pub(crate) fn set_ai_form_format(&mut self, token: &str, cx: &mut Context<Self>) {
        self.settings.set_form_format(token, cx);
        cx.notify();
    }

    pub(crate) fn set_ai_form_suffix(&mut self, suffix: &str, cx: &mut Context<Self>) {
        self.settings.set_form_suffix(suffix);
        cx.notify();
    }

    pub(crate) fn arm_ai_provider_delete(&mut self, name: &str, cx: &mut Context<Self>) {
        self.settings.arm_provider_delete(name);
        cx.notify();
    }

    pub(crate) fn cancel_ai_provider_delete(&mut self, cx: &mut Context<Self>) {
        self.settings.cancel_provider_delete();
        cx.notify();
    }

    pub(crate) fn confirm_ai_provider_delete(&mut self, cx: &mut Context<Self>) {
        self.settings.confirm_provider_delete();
        cx.notify();
    }

    /// The model form inside the shown provider, adding one (`None`) or
    /// editing a row.
    pub(crate) fn open_ai_model_form(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.settings.selected().cloned() else {
            return;
        };
        let (id, label, vision) = index
            .and_then(|index| provider.models.get(index))
            .map(|model| (model.id.clone(), model.label.clone(), model.vision))
            .unwrap_or_else(|| (String::new(), String::new(), false));
        let form = settings::ModelForm::new(
            provider.name.clone(),
            index,
            settings::text_field(window, cx, &id, t("settings.ai.model_id_hint"), false),
            settings::text_field(window, cx, &label, t("settings.ai.model_label_hint"), false),
            vision,
        );
        let focus = form.first();
        self.settings.begin_model(form);
        let handle = focus.read(cx).focus().clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    pub(crate) fn cancel_ai_model_form(&mut self, cx: &mut Context<Self>) {
        self.settings.cancel_model();
        cx.notify();
    }

    pub(crate) fn save_ai_model_form(&mut self, cx: &mut Context<Self>) {
        self.settings.save_model(cx);
        cx.notify();
    }

    pub(crate) fn set_ai_model_vision(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.set_model_vision(on);
        cx.notify();
    }

    pub(crate) fn arm_ai_model_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.settings.arm_model_delete(id);
        cx.notify();
    }

    pub(crate) fn cancel_ai_model_delete(&mut self, cx: &mut Context<Self>) {
        self.settings.cancel_model_delete();
        cx.notify();
    }

    pub(crate) fn confirm_ai_model_delete(&mut self, cx: &mut Context<Self>) {
        self.settings.confirm_model_delete();
        cx.notify();
    }

    /// The model a turn runs on: written to the provider file, so the chat
    /// panel's next turn picks it up.
    pub(crate) fn select_ai_model(&mut self, id: &str, cx: &mut Context<Self>) {
        self.settings.select_model(id);
        cx.notify();
    }

    /// A palette row was picked: remember it, then run what its value names.
    /// A value that no longer resolves (a closed workspace, an action the
    /// daemon stopped offering) does nothing rather than guessing.
    pub(crate) fn run_palette_value(
        &mut self,
        value: &gpui::SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = palette::decode(value) else {
            return;
        };
        self.palette.note_run(value);
        match target {
            palette::Target::Command(command) => self.run_command(command, window, cx),
            palette::Target::Workspace(workspace_id) => {
                self.connection.focus_workspace(&workspace_id);
            }
            palette::Target::Plugin(tool_name) => {
                if let Some(action) = self.plugin_action(&tool_name)
                    && let Some(host) = self.plugin_host.as_ref()
                {
                    host.invoke(action, "palette");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // The plugin manager, the trust review and the shared confirmation
    // -----------------------------------------------------------------------

    /// A plugin row was pressed: put that plugin's page in front. The drawer
    /// is a drawer, not a mode — it keeps the shell visible — but it takes the
    /// keyboard while it is up so Escape reaches it. The event inspector and
    /// the manager are the only two surfaces that share this edge, so opening
    /// one puts the other away.
    pub(crate) fn open_plugin_manager(
        &mut self,
        plugin_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.manager.open(plugin_id);
        self.inspector.close();
        self.palette.open = false;
        self.prefix_armed = false;
        // The page is about one plugin's contract, and the log ring is part of
        // it; opening the drawer asks for it once instead of waiting for the
        // host's fallback tick or for something else to change.
        if let Some(host) = self.plugin_host.as_ref() {
            host.refresh_logs();
        }
        window.focus(&self.manager_focus, cx);
        cx.notify();
    }

    pub(crate) fn close_plugin_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.manager.is_open() {
            self.manager.close();
            self.focus_panes(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn set_manager_tab(&mut self, tab: manager::Tab, cx: &mut Context<Self>) {
        if self.manager.tab() != tab {
            self.manager.set_tab(tab);
            if tab == manager::Tab::Logs
                && let Some(host) = self.plugin_host.as_ref()
            {
                host.refresh_logs();
            }
            cx.notify();
        }
    }

    /// A row on the actions page ran its action through the host — the same
    /// dispatch the palette and the chord take, with the manager named as the
    /// source.
    pub(crate) fn run_manifest_action(
        &mut self,
        action: crate::plugins::model::PluginAction,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.plugin_host.as_ref() {
            host.invoke(action, "manager");
        }
        cx.notify();
    }

    /// A pane a plugin declares was pressed: the daemon opens it as a pane,
    /// and the drawer goes away so the new pane is visible.
    pub(crate) fn open_plugin_pane(
        &mut self,
        plugin_id: String,
        entrypoint: String,
        title: String,
        placement: crate::plugins::model::Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.plugin_host.as_ref() {
            host.open_pane(plugin_id, entrypoint, title, placement);
        }
        self.close_plugin_manager(window, cx);
    }

    /// The footer's switch. Enable and disable are the daemon's calls, and the
    /// registry is re-read by the host; no confirmation stands in front of a
    /// change that is its own undo.
    pub(crate) fn set_plugin_enabled(
        &mut self,
        plugin_id: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.plugin_host.as_ref() {
            host.set_enabled(plugin_id, enabled);
        }
        cx.notify();
    }

    /// The unlink button: ask before forgetting a plugin. The card names the
    /// plugin, and only its confirm button reaches the daemon.
    pub(crate) fn ask_unlink_plugin(
        &mut self,
        plugin_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.confirm = Some(confirm::Confirm::from(confirm::Kind::UnlinkPlugin {
            plugin_id,
            name,
        }));
        cx.notify();
    }

    /// A worktree row's remove button: ask first, with the force row for a
    /// checkout the daemon would otherwise refuse.
    pub(crate) fn ask_remove_worktree(
        &mut self,
        workspace_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.confirm = Some(confirm::Confirm::from(confirm::Kind::RemoveWorktree {
            workspace_id,
            name,
        }));
        cx.notify();
    }

    pub(crate) fn dismiss_confirm(&mut self, cx: &mut Context<Self>) {
        if self.confirm.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn set_confirm_force(&mut self, force: bool, cx: &mut Context<Self>) {
        if let Some(confirm) = self.confirm.as_mut() {
            confirm.force = force;
            cx.notify();
        }
    }

    /// The card's confirm button, and the only path to either request. A
    /// plugin unlink also puts away the page it was asked from: the plugin no
    /// longer exists to show.
    pub(crate) fn confirm_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        match confirm.kind {
            confirm::Kind::RemoveWorktree { workspace_id, name } => {
                if let Some(host) = self.worktree_host.as_ref() {
                    host.remove(workspace_id, name, confirm.force)
                }
            }
            confirm::Kind::UnlinkPlugin { plugin_id, .. } => {
                if let Some(host) = self.plugin_host.as_ref() {
                    host.unlink(plugin_id.clone());
                }
                if self.manager.shows(&plugin_id) {
                    self.manager.close();
                }
                self.focus_panes(window, cx);
            }
        }
        cx.notify();
    }

    /// Show the trust review, optionally already holding a directory (a
    /// plugin's own page asks for its commands to be read back). The field is
    /// a real input entity, built here because an entity needs the window.
    pub(crate) fn open_trust_at(
        &mut self,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lane.is_none() {
            toast::not_sent(&self.toasts, &t("shell.remote.no_lane").to_string(), cx);
            return;
        }
        let placeholder = t("shell.trust.placeholder");
        let input = cx.new(|cx| TextInput::new(window, cx).placeholder(placeholder));
        if let Some(path) = path.clone() {
            input.update(cx, |input, cx| input.set_text(path, cx));
        }
        let submitted = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Submit) {
                    this.trust_enter(window, cx);
                }
            },
        );
        self.trust.open(input);
        self._trust_input = Some(submitted);
        self.palette.open = false;
        self.prefix_armed = false;
        cx.notify();
        if path.is_some() {
            self.review_trust(cx);
        }
    }

    pub(crate) fn close_trust(&mut self, cx: &mut Context<Self>) {
        self.trust.close();
        self._trust_input = None;
        cx.notify();
    }

    /// Read the directory the field names and say what it would run. One small
    /// TOML file, on this thread: it starts no process and reads nothing the
    /// plugin did not declare.
    pub(crate) fn review_trust(&mut self, cx: &mut Context<Self>) {
        let typed = self.trust.typed(cx);
        match trust::directory_of(&typed) {
            None => self.trust.reviewed(
                std::path::PathBuf::new(),
                Err(t("shell.trust.type_directory")),
            ),
            Some(dir) => {
                let preview = crate::plugins::trust::preview(&dir);
                self.trust.reviewed(dir, preview);
            }
        }
        cx.notify();
    }

    pub(crate) fn set_trust_read(&mut self, read: bool, cx: &mut Context<Self>) {
        self.trust.set_read(read);
        cx.notify();
    }

    /// What the dialog's Link button and Enter do: hand the reviewed directory
    /// to the daemon. The acknowledgement is the gate, and the answer arrives
    /// on the next registry read.
    pub(crate) fn trust_link(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.trust.linkable() else {
            return;
        };
        let text = path.to_string_lossy().into_owned();
        self.trust
            .linking(rust_i18n::t!("shell.trust.linking", path = text.clone()).to_string());
        if let Some(host) = self.plugin_host.as_ref() {
            host.link(text);
        }
        cx.notify();
    }

    /// Enter in the path field: read it the first time, link it once it has
    /// been read and acknowledged. A field that has changed since the read is
    /// read again rather than linking a directory that is not on screen.
    fn trust_enter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.trust.typed(cx);
        let same = self
            .trust
            .reviewed_path()
            .is_some_and(|path| path.to_string_lossy() == typed);
        if same && self.trust.preview_ok() {
            if self.trust.is_read() {
                self.trust_link(cx);
            }
        } else {
            self.review_trust(cx);
        }
    }

    // -----------------------------------------------------------------------
    // The event inspector
    // -----------------------------------------------------------------------

    /// A stream row was pressed: capture that event and show it. The copy is
    /// taken now because the ring keeps moving under the row.
    pub(crate) fn open_event(
        &mut self,
        event: crate::events::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector.open(event);
        self.manager.close();
        window.focus(&self.inspector_focus, cx);
        cx.notify();
    }

    pub(crate) fn close_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inspector.is_open() {
            self.inspector.close();
            self.focus_panes(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn set_inspector_tab(&mut self, tab: inspector::Tab, cx: &mut Context<Self>) {
        if self.inspector.tab() != tab {
            self.inspector.set_tab(tab);
            cx.notify();
        }
    }

    /// The 21-name contract, from the inspector or the events view's foot.
    pub(crate) fn open_contract(&mut self, cx: &mut Context<Self>) {
        self.inspector.open_contract();
        self.prefix_armed = false;
        cx.notify();
    }

    pub(crate) fn close_contract(&mut self, cx: &mut Context<Self>) {
        self.inspector.close_contract();
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // The layout inspector
    // -----------------------------------------------------------------------

    /// Open the inspector. When no project has been read yet — the first-run
    /// entry is exactly that case — the sheet offers a directory to read, so
    /// "open layout" is a destination rather than a card over an empty pane.
    pub(crate) fn open_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = if self.layout_state.path.is_none() {
            let field = cx.new(|cx| {
                TextInput::new(window, cx).placeholder(t("shell.layout_sheet.placeholder"))
            });
            self._layout_input = Some(cx.subscribe_in(
                &field,
                window,
                |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Submit) {
                        this.scan_layout_from_input(cx);
                    }
                },
            ));
            Some(field)
        } else {
            self._layout_input = None;
            None
        };
        self.layout_sheet.open(input);
        self.palette.open = false;
        self.prefix_armed = false;
        cx.notify();
    }

    /// Read the directory the inspector's field names: the same scan the
    /// focused pane's cwd gets, aimed where the reader pointed it. It is a
    /// handful of directory checks and one small file, so it runs in place.
    pub(crate) fn scan_layout_from_input(&mut self, cx: &mut Context<Self>) {
        let typed = self.layout_sheet.typed(cx);
        if typed.is_empty() {
            return;
        }
        self.layout_state.scan(std::path::Path::new(&typed));
        cx.notify();
    }

    pub(crate) fn close_layout(&mut self, cx: &mut Context<Self>) {
        self.layout_sheet.close();
        self._layout_input = None;
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // The first-run daemon card (U3-5)
    // -----------------------------------------------------------------------

    /// Show or hide the daemon's last output. Reading the file happens once per
    /// press, never on a pulse: nothing here polls.
    pub(crate) fn toggle_daemon_log(&mut self, cx: &mut Context<Self>) {
        if self.empty.log.take().is_some() {
            cx.notify();
            return;
        }
        self.empty.log = Some(
            crate::client::daemon_log_tail(14)
                .unwrap_or_else(|| t("shell.first_run.daemon.no_log")),
        );
        cx.notify();
    }

    /// Start the daemon in place: the same bootstrap the connection runs on its
    /// own, off the UI thread because seeding a binary is file work. The
    /// connection is asked to dial at once when it returns, so the first
    /// snapshot arrives on the existing pulse path and nothing here waits.
    pub(crate) fn start_daemon(&mut self, cx: &mut Context<Self>) {
        if self.empty.starting {
            return;
        }
        self.empty.starting = true;
        self.empty.note = None;
        cx.notify();
        let task = cx.spawn(async move |root, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { crate::client::start_herdr_server() })
                .await;
            let _ = root.update(cx, |root, cx| {
                root.empty.starting = false;
                root.connection.retry_now();
                if started {
                    root.empty.note = Some(t("shell.first_run.daemon.started"));
                    root.empty.log = None;
                } else {
                    // The failed start's own words are the only witness there
                    // is; the card opens them rather than a second dialog.
                    root.empty.note = Some(t("shell.first_run.daemon.failed"));
                    root.empty.log = Some(
                        crate::client::daemon_log_tail(14)
                            .unwrap_or_else(|| t("shell.first_run.daemon.no_log")),
                    );
                }
                cx.notify();
            });
        });
        self._daemon_task = Some(task);
    }

    /// Export the focused tab's layout and write it to the project's file. The
    /// daemon call and the file write run off the UI thread; the answer lands
    /// in the inspector's own note line and the file is read back.
    pub(crate) fn export_layout(&mut self, cx: &mut Context<Self>) {
        let Some(tab_id) = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_tab_id.clone())
        else {
            return;
        };
        let Some(project) = self.layout_state.root.clone() else {
            return;
        };
        let path = self.layout_state.path.clone().unwrap_or_else(|| {
            project
                .join(crate::layout::DIR)
                .join(crate::layout::FILE_NAME)
        });
        let task = cx.spawn(async move |root, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let client = crate::herdr::api::ApiClient::local();
                    crate::layout::export(&client, &tab_id).and_then(
                        |template| -> Result<std::path::PathBuf, crate::layout::Error> {
                            crate::layout::write(&path, &template)?;
                            Ok(path)
                        },
                    )
                })
                .await;
            let _ = root.update(cx, |root, cx| {
                match outcome {
                    Ok(path) => {
                        let note = rust_i18n::t!("layout.saved", path = path.display().to_string())
                            .to_string();
                        root.layout_state.record(Ok(note));
                        if let Some(cwd) = root.layout_cwd.clone() {
                            root.layout_state.scan(std::path::Path::new(&cwd));
                        }
                    }
                    Err(error) => root.layout_state.record(Err(error)),
                }
                cx.notify();
            });
        });
        self._layout_task = Some(task);
        cx.notify();
    }

    /// Restore the project's template into the focused workspace. The daemon
    /// always creates a new tab, which the inspector says in words; the new
    /// tab arrives on the endpoint lane like any other.
    pub(crate) fn apply_layout(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.layout_state.template.clone() else {
            return;
        };
        let Some(workspace) = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.clone())
        else {
            return;
        };
        let task = cx.spawn(async move |root, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let client = crate::herdr::api::ApiClient::local();
                    crate::layout::apply(&client, &workspace, &template).map(|layout| layout.tab_id)
                })
                .await;
            let _ = root.update(cx, |root, cx| match outcome {
                Ok(tab) => {
                    root.layout_state
                        .record(Ok(rust_i18n::t!("layout.restored", tab = tab).to_string()));
                    cx.notify();
                }
                Err(error) => {
                    root.layout_state.record(Err(error));
                    cx.notify();
                }
            });
        });
        self._layout_task = Some(task);
        cx.notify();
    }

    /// Hand the keyboard back to the grid. Every drawer and dialog does this on
    /// the way out; the grid's handle is the shell's to know.
    fn focus_panes(&self, window: &mut Window, cx: &mut App) {
        let focus = self.panes.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }

    /// Whether a raised surface owns the keyboard. The prefix and every shell
    /// chord stay out of the way while one does — and a manager whose plugin
    /// was unlinked elsewhere is not up at all, so its stale state does not
    /// swallow the keys.
    fn raised(&self) -> bool {
        self.settings.open
            || self.palette.open
            || self.trust.is_open()
            || self.layout_sheet.is_open()
            || self.review.is_open()
            || self.inspector.is_open()
            || self.inspector.contract_open()
            || self.worktree_form.is_open()
            || self.confirm.is_some()
            || self
                .plugins
                .plugins
                .iter()
                .any(|plugin| self.manager.shows(&plugin.plugin_id))
    }

    /// The runnable, currently offered action behind a palette row's tool
    /// name, rebuilt from the registry the way the row was offered. A plugin
    /// that a later read dropped or disabled resolves to nothing.
    fn plugin_action(&self, tool_name: &str) -> Option<crate::plugins::model::PluginAction> {
        let catalog =
            Catalog::build(&self.plugins.actions, &self.plugins.ui).runnable(&self.plugins.plugins);
        let availability = Availability::of(self.live.snapshot.as_deref());
        catalog
            .named(tool_name)
            .filter(|entry| entry.is_offered(availability))
            .map(|entry| entry.action.clone())
    }

    /// A plugin action reached by its declared chord. The action came from the
    /// catalog the chord was bound from, but the focus may have moved since:
    /// the daemon's contexts decide, so a chord only reaches an action the
    /// current focus offers.
    fn run_plugin_action(&mut self, action: crate::plugins::model::PluginAction) {
        let availability = Availability::of(self.live.snapshot.as_deref());
        if !action.is_available(availability) {
            return;
        }
        if let Some(host) = self.plugin_host.as_ref() {
            host.invoke(action, "key");
        }
    }

    /// Take the reader's latest state. Clipboard writes and platform
    /// notifications are drained here, off the shared state, because the shell
    /// owns the one connection; a paint never drains anything.
    fn pull(&mut self, cx: &mut Context<Self>) {
        for data in self.connection.take_clipboard() {
            if let Some(text) = crate::herdr::decode(&data) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            } else {
                tracing::warn!(
                    length = data.len(),
                    "undecodable OSC 52 clipboard payload dropped"
                );
            }
        }
        for notice in self.connection.take_notices() {
            cx.show_system_notification(SystemNotification {
                tag: notice.tag.into(),
                title: notice.title.into(),
                body: notice.body.into(),
                actions: Vec::new(),
            });
        }

        let next = self.connection.state();
        // Every pulse reaches the grid, not only the state changes the chrome
        // draws: a find answer or a split ratio's acknowledgement moves no
        // revision, and those are exactly what the next request waits on.
        self.panes
            .update(cx, |panes, cx| panes.sync(next.clone(), cx));
        // A refusal is news after the surface that asked has closed (a chord's
        // tab close, an action run from the palette). The value is compared
        // rather than read every pulse: the daemon keeps the last refusal until
        // the next request clears it, and one refusal is one message.
        if next.rejected != self.rejected_seen {
            self.rejected_seen = next.rejected.clone();
            if let Some(reason) = next.rejected.as_deref() {
                toast::refused(&self.toasts, reason, cx);
            }
        }
        // A daemon that came back makes the first-run card's last start answer
        // stale; the card is gone while this is true, and a later drop starts
        // it clean.
        if next.status == ConnectionStatus::Connected
            && self.live.status != ConnectionStatus::Connected
        {
            self.empty = empty::State::default();
        }
        // A drop and a welcome carry no revision bump of their own, so the
        // status is compared beside it: the chrome says which state the
        // connection is in and must move when that changes.
        if next.revision != self.drawn || next.status != self.live.status {
            self.drawn = next.revision;
            self.live = next;
            self.follow_focus();
            cx.notify();
        }
    }

    /// The two lists that follow the snapshot rather than a stream: the focused
    /// workspace scopes the worktrees, and the agents scope the fleet. Both
    /// calls are no-ops when their input did not move.
    fn follow_focus(&mut self) {
        let focused = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.clone());
        if focused != self.worktrees_scope {
            self.worktrees_scope = focused.clone();
            if let Some(host) = self.worktree_host.as_ref() {
                host.refresh(focused);
            }
        }
        let panes: Vec<String> = self
            .live
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
        if let Some(stream) = self.fleet_stream.as_ref() {
            stream.watch(panes);
        }
        self.scan_layout();
    }

    /// Re-read the project template when the focused pane's directory moves.
    /// The scan is a handful of directory checks and one small file; a pulse
    /// that did not move the cwd reads nothing.
    fn scan_layout(&mut self) {
        let cwd = self.live.snapshot.as_deref().and_then(focused_pane_cwd);
        if cwd == self.layout_cwd {
            return;
        }
        let had_pane = self.layout_cwd.is_some();
        self.layout_cwd = cwd.clone();
        match cwd {
            Some(cwd) => self.layout_state.scan(std::path::Path::new(&cwd)),
            // A pane that named a project went away: forget the template it
            // named. A directory the reader picked in the inspector is not a
            // pane's, and stays until a pane names its own.
            None if had_pane => self.layout_state = LayoutState::default(),
            None => {}
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let root = &entity;
        // The sidebar reads one copy of each source; the paint never locks a
        // host's cache. The width decides whether the column is drawn at all
        // (below 900 the rail is the whole sidebar) and is never written back
        // into the config.
        let data = sidebar::Data {
            live: &self.live,
            events: &self.events,
            fleet: &self.fleet,
            plugins: &self.plugins,
            worktrees: &self.worktrees,
            layout: &self.layout_state,
            selected: self.inspector.selected(),
            manager: self.manager.plugin_id(),
            lane: self.lane.is_some(),
        };
        let roomy = sidebar_visible(window.viewport_size().width.as_f32());
        // The aside's state is the config's intent, narrowed by what the
        // window can hold. The rail stands in only for a panel the reader has
        // closed; a window too narrow for the open panel leaves the grid the
        // room and draws no rail — the strip's button is still the way back.
        let aside = aside_state(
            config::get(cx).layout.aside_visible,
            window.viewport_size().width.as_f32(),
        );
        let unread = self.chat.read(cx).unread();
        let mut body = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(chrome::main_area(
                chrome::Main {
                    live: &self.live,
                    connection: &self.connection,
                    panes: &self.panes,
                    keymap: &self.keymap,
                    prefix_armed: self.prefix_armed,
                    empty: &self.empty,
                    root,
                },
                cx,
            ));
        match aside {
            AsideState::Open => {
                // The column is the drag listener's element, so its bounds are
                // the panel's right edge; the grip sits over the border it
                // drags. The width the config stores is capped by the window's
                // share here rather than written back, so a window narrowed
                // for a moment does not forget a wide panel.
                let width = self
                    .aside_width
                    .width(window.viewport_size().width.as_f32());
                body = body.child(
                    div()
                        .relative()
                        .h_full()
                        .flex_none()
                        .on_drag_move(cx.listener(Self::drag_aside))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::settle_aside))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::settle_aside))
                        .child(aside::grip(root, cx))
                        .child(aside::column(
                            &self.chat,
                            self.aside_tab,
                            unread,
                            width,
                            fleet::Sources {
                                state: &self.fleet_panel,
                                live: &self.live,
                                stream: &self.fleet,
                                connection: &self.connection,
                            },
                            root,
                            cx,
                        )),
                );
            }
            AsideState::Rail => {
                body = body.child(aside::rail(unread, root, cx));
            }
            AsideState::Hidden => {}
        }
        // The frame is the library's: a column with a title strip and a row of
        // sidebar and main area. The status bar lives at the main column's
        // foot (the design's layout), so it is composed there rather than in
        // the shell's own slot. The resize margin and hairline are
        // `ResizeBorder`'s, and it is what knows whether this window is
        // client-decorated.
        // Text that inherits — markdown bodies, source feet, a plugin's
        // strings — otherwise falls back to the platform's own UI face, which
        // is a third font beside the library's two. Naming the library's face
        // once at the root gives the whole window one voice; the terminal
        // still names the mono face because its cells are laid on a fixed
        // advance.
        let frame = div()
            .relative()
            .size_full()
            .font_family(cx.theme().font_family.clone())
            .child(
                ResizeBorder::new().child(
                    AppShell::new()
                        .title_bar(chrome::title_bar(
                            window,
                            &self.live,
                            &self.keymap,
                            root,
                            cx,
                        ))
                        .sidebar(
                            div()
                                .flex()
                                .h_full()
                                .child(chrome::rail(self.rail, &self.live, root))
                                .when(roomy, |row| {
                                    row.child(chrome::sidebar(
                                        self.rail,
                                        &data,
                                        &self.connection,
                                        root,
                                        cx,
                                    ))
                                }),
                        )
                        .child(body),
                ),
            );
        // The short-lived messages float over the shell. The library's
        // viewport is the topmost raised layer by its own priority (1), so a
        // message is never hidden behind a dialog it was caused by (U3-5).
        let frame = frame.child(toast::viewport(&self.toasts));
        // The palette is a raised layer over the whole window, not a child of
        // the main column: the scrim must cover the rail and the strip too.
        let frame = if self.palette.open {
            frame.child(palette::overlay(
                &self.palette,
                &self.live,
                &self.plugins,
                &self.keymap,
                root,
            ))
        } else {
            frame
        };
        // The settings dialog is the topmost of the two exclusive full-window
        // surfaces. Then the two right-edge drawers (they never stack), the
        // layout inspector, the trust review, the event contract and — last,
        // because it asks about an action underneath — the confirmation.
        let frame = if self.settings.open {
            frame.child(settings::overlay(
                window,
                &self.settings,
                settings::Sources {
                    live: &self.live,
                    plugins: &self.plugins,
                    keymap: &self.keymap,
                    orchestrator: crate::mcp::orchestrator_running(),
                },
                root,
                cx,
            ))
        } else {
            frame
        };
        // A modal above a drawer must not let the drawer's outside-press fire:
        // a click inside the trust review or the contract is not a click on the
        // shell, and closing the drawer under its own dialog loses the page the
        // reader returns to. Drawers never stack, so the manager only has to
        // know about the modal surfaces and the palette.
        let modal_above_manager = self.settings.open
            || self.palette.open
            || self.trust.is_open()
            || self.layout_sheet.is_open()
            || self.worktree_form.is_open()
            || self.inspector.contract_open()
            || self.confirm.is_some();
        let frame = match manager::overlay(
            manager::Sources {
                state: &self.manager,
                plugins: &self.plugins,
                availability: Availability::of(self.live.snapshot.as_deref()),
                keymap: &self.keymap,
                focus: &self.manager_focus,
                blocked: modal_above_manager,
            },
            root,
            window,
            cx,
        ) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let frame = match inspector::overlay(
            inspector::Sources {
                state: &self.inspector,
                plugins: &self.plugins,
                stream: &self.events,
                focus: &self.inspector_focus,
                blocked: self.settings.open || self.palette.open || self.inspector.contract_open(),
            },
            root,
            window,
            cx,
        ) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let workspace = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.clone());
        let tab = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_tab_id.clone());
        let frame = match layout_sheet::overlay(
            &self.layout_sheet,
            &self.layout_state,
            workspace.as_deref(),
            tab.as_deref(),
            root,
            window,
            cx,
        ) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let frame = match review::overlay(
            &self.review,
            &self.review_state,
            &self.review_diff,
            root,
            window,
            cx,
        ) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let repo = self
            .worktrees
            .source
            .as_ref()
            .map(|source| source.repo_name.clone());
        let frame = match worktree::overlay(&self.worktree_form, repo.as_deref(), root, window, cx)
        {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let frame = match trust::overlay(&self.trust, root, window, cx) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        let frame = match inspector::contract_overlay(&self.inspector, root, window, cx) {
            Some(overlay) => frame.child(overlay),
            None => frame,
        };
        if let Some(confirm) = self.confirm.as_ref() {
            frame.child(confirm::overlay(confirm, root, window, cx))
        } else {
            frame
        }
    }
}

/// The focused pane's directory: where the program inside it says it is, or
/// the directory the pane was opened in. The layout block walks up from it to
/// the project's template, so the foreground directory is the better answer.
fn focused_pane_cwd(snapshot: &herdr_client::protocol::ClientShellSnapshot) -> Option<String> {
    let id = snapshot.focused_pane_id.as_ref()?;
    let pane = snapshot.panes.iter().find(|pane| &pane.pane_id == id)?;
    pane.foreground_cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| pane.cwd.clone().filter(|cwd| !cwd.is_empty()))
}

/// Whether the sidebar column is drawn at this width. The rail is the floor:
/// it is never folded, because it is the only way back to the views.
fn sidebar_visible(width: f32) -> bool {
    width >= SIDEBAR_BREAKPOINT
}

/// The grid's palette for the chrome's current mode, built from the config's
/// appearance and the library's own ANSI sixteen (U1-3). Reading the mode off
/// the installed chrome rather than the config keeps a `mode: system` grid in
/// step with the appearance the window actually shows.
fn terminal_theme(cx: &App) -> crate::terminal::Theme {
    theme::ThemeSettings::from_config(&config::get(cx).appearance).terminal_theme(cx.theme().mode())
}

/// Whether the aside is open, folded into its rail, or left out at this width.
/// A closed panel is always the rail; an open one needs the window to be wide
/// enough to hold it beside the grid.
fn aside_state(visible: bool, width: f32) -> AsideState {
    if !visible {
        AsideState::Rail
    } else if width >= ASIDE_BREAKPOINT {
        AsideState::Open
    } else {
        AsideState::Hidden
    }
}

/// Put the window's shape into the config, so the next launch opens it.
///
/// The size and the maximized state are what the config carries; the position
/// is the compositor's to choose. Both close paths run through this: the
/// strip's own button through the library's close check, and a window
/// manager's close through the same check (`guard_close`, registered by
/// [`crate::run`]).
///
/// The size is the *inner* one on purpose. `WindowOptions::window_bounds` is
/// the content area, while X11 (and Wayland) report the outer window from
/// `window_bounds()` — the client decorations' 10px resize margin per side is
/// part of it. Saving the outer size made every restart re-add that margin,
/// so the window grew 20px a launch; `inner_window_bounds()` subtracts the
/// insets and the round trip is stable. Elsewhere the two are the same.
pub(crate) fn save_window_bounds(window: &Window, cx: &mut App) {
    let size = match window.inner_window_bounds() {
        WindowBounds::Windowed(bounds)
        | WindowBounds::Maximized(bounds)
        | WindowBounds::Fullscreen(bounds) => bounds.size,
    };
    let maximized = window.is_maximized();
    config::update(cx, |config| {
        config
            .window
            .set_bounds(size.width.as_f32(), size.height.as_f32(), maximized)
    });
}

/// Register the close guard the shell's two close paths share, and open the
/// window's content. [`crate::run`] calls this from inside the window's
/// construction.
pub(crate) fn install_close_guard(window: &mut Window, cx: &mut App) {
    guard_close(window, cx, |window, cx| {
        save_window_bounds(window, cx);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two breakpoints are inclusive on the drawn side: 900 draws the
    /// column, 899 folds it; 1180 opens the aside, 1179 hides it. A closed
    /// aside is the rail at any width.
    #[test]
    fn the_column_folds_below_its_breakpoints() {
        assert!(sidebar_visible(900.));
        assert!(!sidebar_visible(899.9));
        assert_eq!(aside_state(true, 1180.), AsideState::Open);
        assert_eq!(aside_state(true, 1179.9), AsideState::Hidden);
        assert_eq!(aside_state(false, 1440.), AsideState::Rail);
        assert_eq!(aside_state(false, 480.), AsideState::Rail);
        // A phone-width window still draws the rail; only the column folds.
        assert!(!sidebar_visible(480.));
    }
}
