//! The terminal view: one daemon surface painted natively, with GPUI input
//! mapped onto the daemon's semantic input events.
//!
//! Ported in shape from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/{window.rs,
//! window/input.rs, window/mouse.rs, window/render.rs, input.rs}`. The view
//! owns local pointer selection and copy, link hover and activation, scrollbar
//! dragging, reconnection chrome, split dragging, the find field over
//! `pane.copy_search` and keyboard copy mode over `pane.copy_motion` (M16
//! Wave 1).

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::*;
// `when_some` and its siblings ride in on this trait, which the glob above does
// not bring along.
use gpui_kit::prelude::FluentBuilder as _;
// The chrome's accent, which a focused pane's card takes: the terminal palette
// has no accent of its own that matches the shell around it.
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use herdr_client::ConnectOptions;
use herdr_client::ConnectTarget;
use herdr_client::Method;
use herdr_client::protocol::{
    ClientMouseButton, ClientMouseKind, ClientPaneInputEvent, PaneSurfaceFrame, PaneSurfacePane,
};
use herdr_client::scrollback::{
    CopyMotionParams, ScrollbackResponse, SelectionReadParams, TextRange, decode_response,
};
use serde_json::json;

use crate::herdr::{Connection, LiveState, decode};
use crate::terminal::cards;
use crate::terminal::copy_mode::{Command as CopyCommand, CopyMode, Outcome as CopyOutcome};
use crate::terminal::find::{self, Count, Search, Step};
use crate::terminal::links::{LinkHit, activation_fallback, link_at};
use crate::terminal::painter::TerminalPainter;
use crate::terminal::regions::{Look, Regions};
use crate::terminal::selection::Selection;
use crate::terminal::split_drag::SplitDrag;
use crate::terminal::splits;
use crate::terminal::theme::Theme;
use crate::terminal::{
    CursorShape, InputTarget, Scrollbar, WheelAccumulator, WheelTarget, cell_height, input_area,
    input_cursor_bounds, key_input, pane_at, popup_origin, viewport, wheel_target,
};

#[cfg(target_os = "linux")]
use crate::terminal::input::WeakInputHandler as ViewInputHandler;
#[cfg(not(target_os = "linux"))]
use gpui_kit::ElementInputHandler as ViewInputHandler;

/// A press forwarded to a pane's application: the drag and release that answer
/// it must reach the same target, on the daemon boot it was pressed against.
struct MouseGesture {
    hit: WheelTarget,
    button: MouseButton,
    boot: String,
}

/// A drag of a pane's native scrollbar. `grab` keeps the pointer's place on
/// the thumb; `want` is the latest offset not yet sent to the daemon.
struct ScrollbarDrag {
    pane: String,
    boot: String,
    grab: f32,
    held: bool,
    want: Option<u64>,
}

/// A link activation waiting for the daemon's answer, and the row-local
/// address to open when the daemon no longer reads one there.
struct PendingActivation {
    request: String,
    fallback: Option<String>,
}

/// The find field over one pane: its text input, the search it drives, and
/// the daemon boot both were opened on. Dropped when either goes away.
struct FindBar {
    input: Entity<InputState>,
    search: Search,
    boot: String,
    /// Re-reads the query whenever the field's text changes.
    _changed: Subscription,
}

/// Keyboard copy mode over one pane, on the daemon boot it began on.
struct CopyModeState {
    mode: CopyMode,
    boot: String,
}

/// A GPUI button onto the daemon's wire form; Navigate buttons have none.
fn mouse_button(button: MouseButton) -> Option<ClientMouseButton> {
    match button {
        MouseButton::Left => Some(ClientMouseButton::Left),
        MouseButton::Right => Some(ClientMouseButton::Right),
        MouseButton::Middle => Some(ClientMouseButton::Middle),
        _ => None,
    }
}

/// The terminal id of the popup a surface reports, if one is open.
///
/// A popup is the daemon's modal layer rather than a pane of the composite
/// grid: it never appears in the snapshot's pane list, and it is named by
/// terminal id rather than pane id. Every input path that resolves its own
/// target — the keys and paste through `send`, the mouse and the wheel through
/// their hit — therefore asks this one question instead of repeating the test.
fn popup_target(surface: Option<&PaneSurfaceFrame>) -> Option<&str> {
    Some(surface?.popup.as_deref()?.terminal_id.as_str())
}

pub(crate) struct TerminalView {
    focus: FocusHandle,
    connection: Connection,
    painter: Rc<RefCell<TerminalPainter>>,
    font: Font,
    /// The font's size and the cell's height the grid draws at, from the
    /// config (`crate::config::TerminalConfig`). Hit tests, the region views
    /// and the resize all measure with these, so a settings change re-sends
    /// the surface size by itself.
    font_size: f32,
    cell_height: f32,
    theme: Theme,
    /// The state the last event brought in; painted as-is.
    live: LiveState,
    drawn_revision: u64,
    /// The grid the pointer's coordinates are relative to, shared with the
    /// canvas because only the paint phase knows it.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The advance the font resolved to, shared with the canvas because only the
    /// frame-build phase can measure it. Every hit test, the region views and
    /// the IME cursor measure in this cell, so it must be the same number the
    /// painter lays cells out with; it starts from an estimate for the one frame
    /// before the font has answered.
    cell_width: Rc<Cell<f32>>,
    /// The geometry the daemon was last asked to lay out for.
    sent_options: Rc<Cell<Option<ConnectOptions>>>,
    /// An uncommitted IME composition, painted at the input cursor.
    marked: String,
    /// Whole-line wheel motion kept between scroll events on one axis.
    wheel: WheelAccumulator,
    /// The forwarded press a drag or release must answer, if any.
    mouse_gesture: Option<MouseGesture>,
    /// A drag of the border between panes, if one is armed.
    split_drag: Option<SplitDrag>,
    /// A drag of a pane's scrollbar, if one is armed.
    scrollbar_drag: Option<ScrollbarDrag>,
    /// The selection the pointer made, kept after release so a copy still
    /// knows what it covers.
    selection: Option<Selection>,
    /// The link under the pointer while the link modifier is held.
    hover_link: Option<LinkHit>,
    /// The link a press landed on, which its release must match.
    pressed_link: Option<LinkHit>,
    /// A link activation the daemon has not answered yet.
    pending_activation: Option<PendingActivation>,
    /// The find field, while it is open over a pane.
    find: Option<FindBar>,
    /// Keyboard copy mode, while it is on over a pane.
    copy_mode: Option<CopyModeState>,
    /// The `pane.selection.read` a copy-mode yank waits on.
    pending_copy: Option<String>,
    /// The cursor the border under the pointer asks for, so a press there is
    /// visible before it happens.
    split_cursor: Option<CursorStyle>,
    /// Keeps the event relay alive for as long as the view is.
    _events: Task<()>,
    /// The per-pane cached views the grid paints through, so one busy pane
    /// stops reshaping every other one. The same instance must be used every
    /// frame, or no region is ever adopted.
    regions: Regions,
}

impl TerminalView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let (font_size, line_height) = {
            let config = &crate::config::get(cx).terminal;
            (config.font_size, config.line_height)
        };
        let (pulse, pulses) = crossbeam_channel::unbounded::<()>();
        let connection = Connection::start(ConnectTarget::Local, move || {
            // A closed inbox means the view that owns this connection is gone;
            // the reader stops instead of retrying forever.
            pulse.try_send(()).is_ok()
        });

        // The reader thread never touches GPUI: it pulses this foreground task,
        // which pulls the state and repaints. Nothing polls the daemon.
        let events = cx.spawn(async move |view, cx| {
            loop {
                let pulses = pulses.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move { pulses.recv().is_ok() })
                    .await;
                if !alive {
                    break;
                }
                if view.update(cx, |view, cx| view.pull(cx)).is_err() {
                    break;
                }
            }
        });

        Self {
            focus: cx.focus_handle(),
            connection,
            painter: Rc::new(RefCell::new(TerminalPainter::default())),
            font: font("Lilex"),
            font_size,
            cell_height: cell_height(font_size, line_height),
            theme: Theme::default(),
            live: LiveState::default(),
            drawn_revision: 0,
            bounds: Rc::new(Cell::new(Bounds::default())),
            cell_width: Rc::new(Cell::new(font_size / 2.)),
            sent_options: Rc::new(Cell::new(None)),
            marked: String::new(),
            wheel: WheelAccumulator::default(),
            mouse_gesture: None,
            split_drag: None,
            scrollbar_drag: None,
            selection: None,
            hover_link: None,
            pressed_link: None,
            pending_activation: None,
            find: None,
            copy_mode: None,
            pending_copy: None,
            split_cursor: None,
            _events: events,
            regions: Regions::new(),
        }
    }

    /// Clear the unread count for terminal bells and attention notifications,
    /// which the status bar shows until they are acknowledged. Pulls at once
    /// so both this view and the shell see the zeroed count.
    pub(crate) fn acknowledge_alerts(&mut self, cx: &mut Context<Self>) {
        self.connection.acknowledge_alerts();
        self.pull(cx);
    }

    pub(crate) fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The one connection, so the shell dispatches its own commands through
    /// the client that already owns it.
    pub(crate) fn connection(&self) -> Connection {
        self.connection.clone()
    }

    /// What the last event brought in, for the shell's sidebar and tab strip.
    pub(crate) fn live(&self) -> &LiveState {
        &self.live
    }

    /// Paint the grid with another palette. The shell changes it when the
    /// chrome's light/dark mode does.
    pub(crate) fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        if self.theme != theme {
            self.theme = theme;
            cx.notify();
        }
    }

    /// Draw at the config's metrics and cursor. A changed size or line height
    /// moves every cell, so the painter reshapes its glyphs and the next paint
    /// asks the daemon for a different grid; the cursor shape only changes
    /// what the cursor quad looks like.
    pub(crate) fn set_metrics(
        &mut self,
        font_size: f32,
        line_height: f32,
        cursor: CursorShape,
        cx: &mut Context<Self>,
    ) {
        let height = cell_height(font_size, line_height);
        if self.font_size != font_size || self.cell_height != height {
            self.font_size = font_size;
            self.cell_height = height;
        }
        self.painter.borrow_mut().set_cursor(cursor);
        cx.notify();
    }

    /// Take whatever the connection has applied since the last paint.
    fn pull(&mut self, cx: &mut Context<Self>) {
        // OSC 52 writes land on the system clipboard here, before the paint
        // comparison: the drain is unconditional and on the shared state, so a
        // cloned `LiveState` can never re-deliver a payload and no repaint is
        // needed for the write to happen. The daemon base64-encodes before
        // sending, so a payload that fails to decode is a protocol defect, not
        // hostile input; it is dropped with a warning.
        for data in self.connection.take_clipboard() {
            if let Some(text) = decode(&data) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            } else {
                tracing::warn!(
                    length = data.len(),
                    "undecodable OSC 52 clipboard payload dropped"
                );
            }
        }
        // Notifications the daemon asked the platform to show. Draining is
        // unconditional, like the clipboard: the queue lives on the shared
        // state, so a cloned `LiveState` cannot deliver one twice.
        for notice in self.connection.take_notices() {
            cx.show_system_notification(SystemNotification {
                tag: notice.tag.into(),
                title: notice.title.into(),
                body: notice.body.into(),
                actions: Vec::new(),
            });
        }
        self.poll_activation(cx);
        let next = self.connection.state();
        // A reconnect may reach a daemon with another boot: its surface has the
        // geometry the *old* daemon was told, so the next paint must report the
        // window's size again or the grid stays at the daemon's default.
        let boot_changed = next.boot_id() != self.live.boot_id();
        if next.revision != self.drawn_revision || next.status != self.live.status || boot_changed {
            self.drawn_revision = next.revision;
            self.live = next;
            if boot_changed {
                self.sent_options.set(None);
                // Everything below names cells of the old boot's content; a
                // selection or a link from it would read the new daemon's
                // grid at the same coordinates.
                self.selection = None;
                self.hover_link = None;
                self.pressed_link = None;
                self.scrollbar_drag = None;
                self.mouse_gesture = None;
                self.find = None;
                self.copy_mode = None;
                self.pending_copy = None;
            }
            cx.notify();
        }
        // A drop retires a pending activation: its answer can never arrive,
        // and the link it named is stale content now.
        if self.live.status == crate::herdr::ConnectionStatus::Disconnected {
            self.pending_activation = None;
        }
        // A split drag waiting for its answer continues here: an answer does not
        // change the revision, and it is exactly what frees the next ratio.
        self.flush_split();
        // The same answer frees the scrollbar's next offset.
        self.flush_scrollbar();
        // The scrollback features read the state applied above: a find answer
        // maps onto the pane revision it was asked against, and copy mode
        // retries a stale motion only once the surface shows settled content.
        self.poll_copy_read(cx);
        self.poll_find(cx);
        self.poll_copy_mode(cx);
    }

    /// Fold a link activation's answer into the window: open the address the
    /// daemon names (or the one the row read) unless a plugin handled it. A
    /// drop retires the activation: its answer can never arrive.
    fn poll_activation(&mut self, cx: &mut Context<Self>) {
        if self.live.status == crate::herdr::ConnectionStatus::Disconnected {
            self.pending_activation = None;
            return;
        }
        let Some(pending) = self.pending_activation.as_ref() else {
            return;
        };
        let Some(response) = self.connection.take_response(&pending.request) else {
            // A rejection retires the request without a payload: open the
            // row-local address rather than leaving the click unanswered.
            if !self.connection.in_flight(&pending.request)
                && let Some(pending) = self.pending_activation.take()
                && let Some(url) = pending.fallback
            {
                cx.open_url(&url);
            }
            return;
        };
        let Some(pending) = self.pending_activation.take() else {
            return;
        };
        if let Some(url) = activation_fallback(&response, pending.fallback.as_deref()) {
            cx.open_url(&url);
        }
    }

    /// Copy the selection to the system clipboard. `false` when there is
    /// nothing to copy, so Ctrl-C still reaches the pane as an interrupt.
    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(surface) = self.live.surface.as_deref() else {
            return false;
        };
        let Some(selection) = self.selection.as_ref() else {
            return false;
        };
        let Some(text) = selection.text(surface, self.cell_width.get()) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// The focused pane as the live surface paints it, when the daemon names
    /// one and no popup covers the grid.
    fn focused_surface_pane(&self) -> Option<&PaneSurfacePane> {
        let surface = self.live.surface.as_deref()?;
        if surface.popup.is_some() {
            return None;
        }
        let focused = self.live.snapshot.as_ref()?.focused_pane_id.as_deref()?;
        surface.panes.iter().find(|pane| pane.pane_id == focused)
    }

    /// One pane of the live surface by id, when no popup covers it.
    fn surface_pane(&self, pane_id: &str) -> Option<&PaneSurfacePane> {
        let surface = self.live.surface.as_deref()?;
        if surface.popup.is_some() {
            return None;
        }
        surface.panes.iter().find(|pane| pane.pane_id == pane_id)
    }

    /// Open the find field over the focused pane, or return to it with its
    /// query selected when it is already open there. The field is the client's
    /// chrome and the search is the daemon's: what is typed reaches
    /// `pane.copy_search`, never the pane's application.
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.live.supports_copy_search {
            tracing::warn!("the daemon does not advertise pane.copy_search");
            return;
        }
        let Some(pane) = self.focused_surface_pane() else {
            return;
        };
        let pane_id = pane.pane_id.clone();
        if let Some(bar) = self
            .find
            .as_ref()
            .filter(|bar| bar.search.pane_id() == pane_id)
        {
            let input = bar.input.clone();
            let focus = input.read(cx).focus_handle(cx).clone();
            window.focus(&focus, cx);
            input.update(cx, |input, cx| input.select_all(window, cx));
            cx.notify();
            return;
        }
        let search = Search::new(pane);
        let boot = self.live.boot_id().map(str::to_owned).unwrap_or_default();
        let placeholder = rust_i18n::t!("terminal.find.placeholder").to_string();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let changed = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    if let Some(bar) = this.find.as_mut() {
                        bar.search.set_query(&query);
                    }
                    this.flush_find(cx);
                    cx.notify();
                }
            },
        );
        let focus = input.read(cx).focus_handle(cx).clone();
        self.find = Some(FindBar {
            input,
            search,
            boot,
            _changed: changed,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Close the find field and hand the keyboard back to the terminal. A
    /// search still on its way is answered into nothing: its answer is the
    /// oldest, bounded entry in the connection's response queue.
    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.find.take() else {
            return;
        };
        if bar.input.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Step the open search toward older or newer output.
    fn find_step(&mut self, step: Step, cx: &mut Context<Self>) {
        if let Some(bar) = self.find.as_mut() {
            bar.search.step(step);
        }
        self.flush_find(cx);
        cx.notify();
    }

    /// Send the search that is due, if any. At most one is in flight, so the
    /// daemon never scans the scrollback twice for the same field.
    fn flush_find(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let Some(pane) = self.live.surface.as_deref().and_then(|surface| {
            if surface.popup.is_some() {
                return None;
            }
            surface
                .panes
                .iter()
                .find(|pane| pane.pane_id == bar.search.pane_id())
        }) else {
            return;
        };
        let Some((step, params)) = bar.search.next_request(pane) else {
            return;
        };
        let Ok(value) = serde_json::to_value(&params) else {
            return;
        };
        match self.connection.request(Method::PaneCopySearch, value) {
            Some(request) => bar.search.sent(request, &params, step, Instant::now()),
            None => bar
                .search
                .abandoned(rust_i18n::t!("terminal.find.not_sent").as_ref()),
        }
        cx.notify();
    }

    /// Fold the search's answer into the bar, refresh it when the pane's
    /// content moved on, and send whatever is due. A request the daemon
    /// refused never gets a response, so an unanswered one is retired here
    /// rather than leaving the field waiting forever.
    fn poll_find(&mut self, cx: &mut Context<Self>) {
        let current = self
            .find
            .as_ref()
            .is_some_and(|bar| self.live.boot_id() == Some(bar.boot.as_str()));
        if !current {
            if self.find.take().is_some() {
                cx.notify();
            }
            return;
        }
        let mut reveal = None;
        {
            let Some(bar) = self.find.as_mut() else {
                return;
            };
            if let Some(request) = bar.search.in_flight().map(str::to_owned) {
                if let Some(response) = self.connection.take_response(&request) {
                    let answer = decode_response(&response).and_then(|answer| match answer {
                        ScrollbackResponse::PaneCopySearch(result) => Ok(result),
                        _ => Err(herdr_client::Error::ResponseType),
                    });
                    reveal = bar.search.answer(&request, answer);
                } else if !self.connection.in_flight(&request) {
                    let message = self.live.rejected.clone().unwrap_or_default();
                    bar.search.abandoned(&message);
                }
            }
        }
        if let Some(range) = reveal {
            self.reveal_find(range, cx);
            cx.notify();
        }
        if let Some(bar) = self.find.as_mut()
            && let Some(pane) = self.live.surface.as_deref().and_then(|surface| {
                if surface.popup.is_some() {
                    return None;
                }
                surface
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == bar.search.pane_id())
            })
        {
            bar.search.content_changed(pane, Instant::now());
        }
        self.flush_find(cx);
    }

    /// Scroll the find field's pane so `range` shows, when it does not
    /// already. Centering a match is a `pane.scroll`, like the scrollbar's.
    fn reveal_find(&mut self, range: TextRange, cx: &mut Context<Self>) {
        let Some(pane_id) = self
            .find
            .as_ref()
            .map(|bar| bar.search.pane_id().to_owned())
        else {
            return;
        };
        let Some(offset) = self
            .surface_pane(&pane_id)
            .and_then(|pane| find::reveal_offset(pane, range))
        else {
            return;
        };
        self.scroll_pane(pane_id, offset, cx);
    }

    /// Enter keyboard copy mode on the focused pane, at the terminal's cursor
    /// when it is visible. The mode takes every key until it is left.
    pub(crate) fn enter_copy_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.live.supports_copy_motion {
            tracing::warn!("the daemon does not advertise pane.copy_motion");
            return;
        }
        let Some(pane) = self.focused_surface_pane() else {
            return;
        };
        let Some(boot) = self.live.boot_id().map(str::to_owned) else {
            return;
        };
        let cursor = self
            .live
            .surface
            .as_deref()
            .and_then(|surface| surface.frame.cursor.as_ref())
            .filter(|cursor| cursor.visible)
            .map(|cursor| (cursor.x, cursor.y));
        let mode = CopyMode::new(pane, cursor);
        // The find field and the mode cannot both hold the keyboard.
        self.find = None;
        self.selection = None;
        self.marked.clear();
        self.copy_mode = Some(CopyModeState { mode, boot });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Leave copy mode, scrolling the pane back to where it was when the mode
    /// began.
    pub(crate) fn leave_copy_mode(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.copy_mode.take() else {
            return;
        };
        if let Some(offset) = state.mode.entry_offset() {
            self.scroll_pane(state.mode.pane_id().to_owned(), offset, cx);
        }
        cx.notify();
    }

    /// A keystroke while copy mode is on. Printable text does not arrive here:
    /// the platform commits it through [`Self::replace_text_in_range`], and
    /// running it in both places would move twice.
    fn copy_mode_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.alt || modifiers.function {
            return;
        }
        if let Some(command) =
            CopyCommand::from_key(&keystroke.key, modifiers.shift, modifiers.control)
        {
            self.copy_mode_command(command, cx);
        }
    }

    /// Text a layout or an input method committed while copy mode is on, read
    /// as the copy-mode keys they spell.
    fn copy_mode_text(&mut self, text: &str, cx: &mut Context<Self>) {
        for command in text.chars().filter_map(CopyCommand::from_char) {
            self.copy_mode_command(command, cx);
        }
    }

    fn copy_mode_command(&mut self, command: CopyCommand, cx: &mut Context<Self>) {
        let Some(pane_id) = self
            .copy_mode
            .as_ref()
            .map(|state| state.mode.pane_id().to_owned())
        else {
            return;
        };
        let Some(pane) = self.surface_pane(&pane_id).cloned() else {
            return;
        };
        let Some(state) = self.copy_mode.as_mut() else {
            return;
        };
        let outcome = state.mode.command(command, &pane);
        match outcome {
            CopyOutcome::Nothing => {}
            CopyOutcome::Moved => self.reveal_copy_cursor(cx),
            CopyOutcome::Motion(params) => self.send_copy_motion(params, cx),
            CopyOutcome::Copy(range) => {
                self.read_copy_range(range, cx);
                self.leave_copy_mode(cx);
            }
            CopyOutcome::Exit => self.leave_copy_mode(cx),
        }
        cx.notify();
    }

    fn send_copy_motion(&mut self, params: CopyMotionParams, cx: &mut Context<Self>) {
        let Ok(value) = serde_json::to_value(&params) else {
            if let Some(state) = self.copy_mode.as_mut() {
                state.mode.send_failed();
            }
            return;
        };
        match self.connection.request(Method::PaneCopyMotion, value) {
            Some(request) => {
                if let Some(state) = self.copy_mode.as_mut() {
                    state.mode.sent(request, &params);
                }
            }
            None => {
                if let Some(state) = self.copy_mode.as_mut() {
                    state.mode.send_failed();
                }
                tracing::warn!("copy mode motion was not sent");
            }
        }
        cx.notify();
    }

    /// Read the marked range from the daemon. The daemon owns the text, and a
    /// copy-mode selection may run through rows the view does not paint, so
    /// the read is `pane.selection.read` rather than a walk over cells.
    fn read_copy_range(&mut self, range: TextRange, cx: &mut Context<Self>) {
        let Some(pane_id) = self
            .copy_mode
            .as_ref()
            .map(|state| state.mode.pane_id().to_owned())
        else {
            return;
        };
        let params = SelectionReadParams {
            pane_id,
            anchor: range.start,
            cursor: range.end,
            content_revision: None,
        };
        let Ok(value) = serde_json::to_value(&params) else {
            return;
        };
        if let Some(request) = self.connection.request(Method::PaneSelectionRead, value) {
            self.pending_copy = Some(request);
        }
        cx.notify();
    }

    /// Write a copy-mode read's text to the system clipboard. A refused read
    /// leaves the clipboard alone; its reason is on the status bar.
    fn poll_copy_read(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.pending_copy.clone() else {
            return;
        };
        if let Some(response) = self.connection.take_response(&request) {
            self.pending_copy = None;
            match decode_response(&response) {
                Ok(ScrollbackResponse::PaneSelection(result)) if !result.text.is_empty() => {
                    cx.write_to_clipboard(ClipboardItem::new_string(result.text));
                }
                Ok(_) => tracing::warn!("a copy-mode read answered no text"),
                Err(error) => tracing::warn!(%error, "a copy-mode read failed"),
            }
            cx.notify();
        } else if !self.connection.in_flight(&request) {
            self.pending_copy = None;
            cx.notify();
        }
    }

    /// Run every tick: end a mode whose boot is gone, apply a motion's answer,
    /// retry a stale one, and run the keys that waited behind it.
    fn poll_copy_mode(&mut self, cx: &mut Context<Self>) {
        let current = self
            .copy_mode
            .as_ref()
            .is_some_and(|state| self.live.boot_id() == Some(state.boot.as_str()));
        if !current {
            if self.copy_mode.take().is_some() {
                cx.notify();
            }
            return;
        }
        let request = self
            .copy_mode
            .as_ref()
            .and_then(|state| state.mode.in_flight().map(str::to_owned));
        if let Some(request) = request {
            if let Some(response) = self.connection.take_response(&request) {
                let answer = decode_response(&response).and_then(|answer| match answer {
                    ScrollbackResponse::PaneCopyMotion(result) => Ok(result),
                    _ => Err(herdr_client::Error::ResponseType),
                });
                let moved = self
                    .copy_mode
                    .as_mut()
                    .map(|state| state.mode.answer(&request, answer));
                match moved {
                    Some(Ok(true)) => self.reveal_copy_cursor(cx),
                    Some(Ok(false)) => {}
                    Some(Err(error)) => tracing::warn!(%error, "copy mode motion failed"),
                    None => {}
                }
                cx.notify();
            } else if !self.connection.in_flight(&request) {
                if let Some(state) = self.copy_mode.as_mut() {
                    state.mode.send_failed();
                }
                cx.notify();
            }
        }
        // A motion refused as stale goes again once the surface settles.
        let retry = self.copy_mode.as_ref().and_then(|state| {
            self.surface_pane(state.mode.pane_id())
                .and_then(|pane| state.mode.due_retry(pane))
        });
        if let Some(params) = retry {
            self.send_copy_motion(params, cx);
        }
        // Keys typed behind a motion run now, in order.
        loop {
            let next = self
                .copy_mode
                .as_mut()
                .and_then(|state| state.mode.next_queued());
            let Some(command) = next else {
                break;
            };
            self.copy_mode_command(command, cx);
        }
    }

    /// Scroll a pane's copy-mode cursor into view, as little as possible.
    fn reveal_copy_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(pane_id) = self
            .copy_mode
            .as_ref()
            .map(|state| state.mode.pane_id().to_owned())
        else {
            return;
        };
        let Some(pane) = self.live.surface.as_deref().and_then(|surface| {
            if surface.popup.is_some() {
                return None;
            }
            surface.panes.iter().find(|pane| pane.pane_id == pane_id)
        }) else {
            return;
        };
        let Some(state) = self.copy_mode.as_mut() else {
            return;
        };
        let Some(offset) = state.mode.reveal(pane) else {
            return;
        };
        self.scroll_pane(pane_id, offset, cx);
    }

    /// Ask the daemon to scroll a pane; one request is enough, the surface
    /// that answers it repaints.
    fn scroll_pane(&self, pane_id: String, offset: u64, cx: &mut Context<Self>) {
        if self
            .connection
            .request(
                Method::PaneScroll,
                json!({"pane_id": pane_id, "offset_from_bottom": offset}),
            )
            .is_none()
        {
            tracing::warn!(pane = %pane_id, offset, "scroll request was not sent");
        }
        cx.notify();
    }

    /// Ask the connection to dial now rather than wait out its backoff.
    fn retry_connection(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.connection.retry_now();
        cx.notify();
    }

    /// The scrollbar of `pane`, in paint-origin pixels.
    fn scrollbar(&self, pane: &str) -> Option<Scrollbar> {
        let pane = self
            .live
            .surface
            .as_deref()?
            .panes
            .iter()
            .find(|candidate| candidate.pane_id == pane)?;
        Scrollbar::new(pane, self.cell_width.get(), self.cell_height)
    }

    /// Grab the thumb where pressed, or jump it under the pointer when the
    /// press lands on the track. `false` where no scrollbar owns the point so
    /// the pane below it keeps the press.
    fn press_scrollbar(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(surface) = self.live.surface.as_deref().filter(|s| s.popup.is_none()) else {
            return false;
        };
        let Some(boot) = self.live.boot_id().map(str::to_owned) else {
            return false;
        };
        let local = position - self.bounds.get().origin;
        let (pane, bar) = match surface.panes.iter().find_map(|pane| {
            Scrollbar::new(pane, self.cell_width.get(), self.cell_height)
                .filter(|bar| bar.track.contains(&local))
                .map(|bar| (pane.pane_id.clone(), bar))
        }) {
            Some(hit) => hit,
            None => return false,
        };
        let grab = if bar.thumb.contains(&local) {
            f32::from(local.y - bar.thumb.top())
        } else {
            f32::from(bar.thumb.size.height) / 2.
        };
        self.scrollbar_drag = Some(ScrollbarDrag {
            pane,
            boot,
            grab,
            held: true,
            want: None,
        });
        self.drag_scrollbar(position);
        cx.stop_propagation();
        cx.notify();
        true
    }

    /// Follow the pointer with an armed scrollbar drag. `true` while the drag
    /// owns the move event, so it never reaches an application.
    fn drag_scrollbar(&mut self, position: Point<Pixels>) -> bool {
        let Some(drag) = self.scrollbar_drag.as_ref().filter(|drag| drag.held) else {
            return false;
        };
        let Some(bar) = self.scrollbar(&drag.pane) else {
            self.scrollbar_drag = None;
            return true;
        };
        let local = position - self.bounds.get().origin;
        let top = f32::from(local.y) - drag.grab;
        if let Some(drag) = self.scrollbar_drag.as_mut() {
            drag.want = Some(bar.offset_at(top));
        }
        self.flush_scrollbar();
        true
    }

    /// End an armed scrollbar drag. `true` while the release belongs to it.
    fn release_scrollbar(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) -> bool {
        if event.button != MouseButton::Left
            || !self.scrollbar_drag.as_ref().is_some_and(|drag| drag.held)
        {
            return false;
        }
        if let Some(drag) = self.scrollbar_drag.as_mut() {
            drag.held = false;
        }
        self.flush_scrollbar();
        cx.notify();
        true
    }

    /// Send the latest wanted offset once the previous request has answered,
    /// and retire a released drag when nothing is left to send.
    fn flush_scrollbar(&mut self) {
        let Some(drag) = self.scrollbar_drag.as_mut() else {
            return;
        };
        if self.live.boot_id() != Some(drag.boot.as_str()) {
            self.scrollbar_drag = None;
            return;
        }
        if self.connection.has_in_flight() {
            return;
        }
        let Some(offset) = drag.want.take() else {
            if !drag.held {
                self.scrollbar_drag = None;
            }
            return;
        };
        let pane = drag.pane.clone();
        let request = self.connection.request(
            Method::PaneScroll,
            json!({"pane_id": pane, "offset_from_bottom": offset}),
        );
        if request.is_none() {
            self.scrollbar_drag = None;
        }
    }

    /// The pane the daemon reports as focused. Input and focus requests name it.
    fn focused_pane(&self) -> Option<String> {
        self.live
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.focused_pane_id.clone())
    }

    /// The one funnel keys, paste and the IME commits all reach a pane through.
    /// While a popup is open it is the popup that takes them: the daemon made
    /// the popup the modal layer, and the focused pane sits behind it, so a
    /// keystroke meant for the popup must not land in the pane under it.
    fn send(&self, events: Vec<ClientPaneInputEvent>) {
        if let Some(terminal_id) = popup_target(self.live.surface.as_deref()) {
            self.connection.send_popup_input(terminal_id, events);
            return;
        }
        let Some(pane) = self.focused_pane() else {
            return;
        };
        self.connection.send_input(&pane, events);
    }

    /// Send one event to the target a hit names. The mouse and the wheel resolve
    /// their own target from the pointer rather than from focus, so a popup is
    /// named here by a hit instead of by `send`'s question.
    fn send_to(&self, target: &InputTarget, events: Vec<ClientPaneInputEvent>) -> bool {
        match target {
            InputTarget::Pane(pane) => self.connection.send_input(pane, events),
            InputTarget::Popup(terminal_id) => {
                self.connection.send_popup_input(terminal_id, events)
            }
        }
    }

    /// Super-V, Ctrl-Shift-V and Shift-Insert paste the system clipboard into
    /// the focused pane — the open popup instead, where one takes the input —
    /// as one bracketed paste, so a multi-line paste is not
    /// executed line by line. An empty clipboard sends nothing: a `Paste("")`
    /// would still be a bracketed-paste sequence wrapping nothing.
    /// `EntityInputHandler::paste` is deliberately not overridden: GPUI's
    /// default routes it through `replace_text_in_range` into a
    /// `TextCommit`, and only this keystroke path sends the true `Paste`
    /// event.
    fn paste(&mut self, cx: &mut Context<Self>) {
        // Copy mode reads keys, not text: a paste would become motions.
        if self.copy_mode.is_some() {
            return;
        }
        if let Some(text) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .filter(|text| !text.is_empty())
        {
            self.send(vec![ClientPaneInputEvent::Paste(text)]);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        // Copy mode owns the keyboard: a key it names runs as a copy-mode
        // command, and one it does not is swallowed rather than typed into the
        // pane. Printable text is committed separately and read there.
        if self.copy_mode.is_some() {
            self.copy_mode_key(event, cx);
            cx.stop_propagation();
            return;
        }
        // Find is the terminal's own chord. It is decided here rather than as a
        // global binding so a text field elsewhere keeps its Ctrl-F.
        let find_chord = keystroke.modifiers.platform
            || (!cfg!(target_os = "macos") && keystroke.modifiers.control);
        if find_chord && keystroke.key.eq_ignore_ascii_case("f") && !keystroke.modifiers.shift {
            self.open_find(window, cx);
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        // Copy first: with a selection, Ctrl-C (the platform's copy chord) and
        // Cmd-C copy it; without one Ctrl-C still reaches the pane as SIGINT.
        // On macOS Ctrl is the terminal's own modifier, so only Cmd copies.
        let copy_chord = keystroke.modifiers.platform
            || (!cfg!(target_os = "macos") && keystroke.modifiers.control);
        if copy_chord && keystroke.key.eq_ignore_ascii_case("c") && self.copy_selection(cx) {
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        // Paste is decided before the key mapping: Ctrl-Shift-V would
        // otherwise map to a literal `v` and type it into the pane. On Linux
        // `platform` is the Super key, so plain Ctrl-V stays a pane keystroke.
        if (keystroke.modifiers.platform
            || (keystroke.modifiers.control && keystroke.modifiers.shift)
            || (keystroke.modifiers.shift && keystroke.key == "insert"))
            && (keystroke.key.eq_ignore_ascii_case("v") || keystroke.key == "insert")
        {
            self.paste(cx);
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        // Printable text belongs to the input handler: a key-down must not
        // double-send what the IME or the layout already committed.
        // Alt-modified keys reach the pane as Alt chords: on non-macOS every
        // Alt belongs to the application, which is how upstream's Option-as-Alt
        // policy resolves there unconditionally. The config plumbing is M5.
        let alt_keys = cfg!(not(target_os = "macos"));
        if let Some(input) = key_input(event, alt_keys) {
            self.send(vec![input]);
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    /// Wheel motion belongs to the target under the pointer, not the focused
    /// one: the target the hit names is the target the input is sent to, and an
    /// open popup is that target anywhere over its own grid.
    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(surface) = self.live.surface.clone() else {
            return;
        };
        let origin = self.bounds.get().origin;
        let x = (event.position.x - origin.x).to_f64() as f32;
        let y = (event.position.y - origin.y).to_f64() as f32;
        let cell_width = self.cell_width.get();
        let Some(target) = wheel_target(&surface, x, y, cell_width, self.cell_height) else {
            self.wheel = WheelAccumulator::default();
            return;
        };
        let steps = self
            .wheel
            .steps(&target, event, cell_width, self.cell_height);
        cx.stop_propagation();
        for input in target.wheel_events(steps, event.modifiers) {
            self.send_to(&target.target, vec![input]);
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A new press retires a forwarded one whose release was never observed.
        self.cancel_mouse_gesture();
        // Upstream's order: the scrollbar, the split, then the pane's own
        // application. A link is client chrome too, so the link modifier takes
        // a press before an application that reports the mouse.
        if event.button == MouseButton::Left && self.press_scrollbar(event.position, cx) {
            return;
        }
        if event.button == MouseButton::Left
            && event.modifiers.secondary()
            && self.press_link(event.position, window, cx)
        {
            return;
        }
        if event.button == MouseButton::Left && self.press_split(event.position, cx) {
            return;
        }
        let reporting = self
            .mouse_target_at(event.position)
            .filter(|hit| hit.mouse_reporting)
            // Shift bypasses the application's mouse mode, as in a real
            // terminal, and starts a local selection instead.
            .filter(|_| !event.modifiers.shift)
            .filter(|hit| {
                event.button != MouseButton::Right
                    || self.right_click_to_application(&hit.target, event.modifiers)
            });
        // The application owns this press: forward it, and remember the gesture
        // its drag and release must answer. The pane's focus is asked for on
        // release, so the click itself reaches the application first.
        if let Some(hit) = reporting {
            window.focus(&self.focus, cx);
            cx.stop_propagation();
            if let Some(button) = mouse_button(event.button)
                && let Some(boot) = self.live.boot_id().map(str::to_owned)
            {
                // Arm the gesture only for a press the application actually
                // received, popup or pane: `send_to` already warns when the
                // send failed, and a drag or release must never answer a
                // press the application never saw.
                if self.send_mouse(&hit, ClientMouseKind::Down(button), event.modifiers) {
                    self.mouse_gesture = Some(MouseGesture {
                        hit,
                        button: event.button,
                        boot,
                    });
                }
            }
            return;
        }
        // No application reported the mouse under the press: the left button
        // selects locally and still moves pane focus.
        window.focus(&self.focus, cx);
        if event.button != MouseButton::Left {
            return;
        }
        self.begin_selection(event.position, event.click_count, cx);
        let Some(surface) = self.live.surface.clone() else {
            return;
        };
        let Some(pane) = pane_at(
            &surface,
            self.bounds.get(),
            event.position,
            self.cell_width.get(),
            self.cell_height,
        ) else {
            return;
        };
        if self.focused_pane().as_deref() == Some(pane) {
            return;
        }
        // The daemon owns focus: ask, and let the next snapshot say what changed.
        self.connection.focus_pane(pane);
    }

    /// Start a local selection at the pointer. `false` where no pane or popup
    /// paints under it; the previous selection is dropped either way.
    fn begin_selection(
        &mut self,
        position: Point<Pixels>,
        clicks: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(surface) = self.live.surface.clone() else {
            self.selection = None;
            return false;
        };
        let local = position - self.bounds.get().origin;
        let selection = Selection::begin(
            &surface,
            f32::from(local.x),
            f32::from(local.y),
            self.cell_width.get(),
            self.cell_height,
            clicks,
        );
        let begun = selection.is_some();
        self.selection = selection;
        if begun {
            cx.stop_propagation();
        }
        cx.notify();
        begun
    }

    /// The link under a point, in the paint's own coordinates.
    fn link_at(&self, position: Point<Pixels>) -> Option<LinkHit> {
        let surface = self.live.surface.as_deref()?;
        let local = position - self.bounds.get().origin;
        link_at(
            surface,
            f32::from(local.x),
            f32::from(local.y),
            self.cell_width.get(),
            self.cell_height,
        )
    }

    /// Take the press on a link: it must not reach the application, and its
    /// release is what opens the address.
    fn press_link(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(hit) = self.link_at(position) else {
            return false;
        };
        window.focus(&self.focus, cx);
        self.pressed_link = Some(hit);
        cx.stop_propagation();
        cx.notify();
        true
    }

    /// Open the link a release landed on when it is the one its press took.
    /// `true` while the release belongs to the link press.
    fn release_link(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) -> bool {
        if self.pressed_link.is_none() || event.button != MouseButton::Left {
            return false;
        }
        let pressed = self.pressed_link.take();
        let matched = pressed.as_ref().and_then(|pressed| {
            self.link_at(event.position)
                .filter(|hit| hit.same_link(pressed))
        });
        if let Some(hit) = matched {
            self.activate_link(hit, cx);
        }
        cx.stop_propagation();
        cx.notify();
        true
    }

    /// Ask the daemon to activate the link a plugin may claim, opening the
    /// row-local address when nothing does.
    fn activate_link(&mut self, hit: LinkHit, cx: &mut Context<Self>) {
        let fallback = Some(hit.label().to_owned());
        if let Some(cell) = &hit.cell {
            let (method, params) = cell.activate_request();
            if let Some(request) = self.connection.request(method, params) {
                self.pending_activation = Some(PendingActivation { request, fallback });
                return;
            }
        }
        if let Some(url) = fallback {
            cx.open_url(&url);
        }
    }

    /// Follow the link modifier over a pane: the link under the pointer is
    /// underlined and takes a pointing hand.
    fn track_hover(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let next = modifiers
            .secondary()
            .then(|| self.link_at(position))
            .flatten();
        if next != self.hover_link {
            self.hover_link = next;
            cx.notify();
        }
        if self.hover_link.is_none() {
            self.track_split_cursor(position, cx);
        }
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        // A held scrollbar owns the pointer before the split border under it.
        if self.scrollbar_drag.as_ref().is_some_and(|drag| drag.held) {
            if event.pressed_button == Some(MouseButton::Left) {
                self.drag_scrollbar(event.position);
            } else {
                // The release was never seen: end the drag on the first move
                // that says the button is up.
                if let Some(drag) = self.scrollbar_drag.as_mut() {
                    drag.held = false;
                }
                self.flush_scrollbar();
            }
            cx.stop_propagation();
            return;
        }
        if self.drag_split(event, cx) {
            cx.stop_propagation();
            return;
        }
        if self.selection.as_ref().is_some_and(Selection::dragging) {
            if event.pressed_button != Some(MouseButton::Left) {
                if let Some(selection) = self.selection.as_mut() {
                    selection.release();
                }
            } else if let Some(surface) = self.live.surface.clone() {
                let local = event.position - self.bounds.get().origin;
                let cell_width = self.cell_width.get();
                let changed = self.selection.as_mut().is_some_and(|selection| {
                    selection.extend(
                        &surface,
                        f32::from(local.x),
                        f32::from(local.y),
                        cell_width,
                        self.cell_height,
                    )
                });
                if changed {
                    cx.notify();
                }
            }
            cx.stop_propagation();
            return;
        }
        let held = self.mouse_gesture.as_ref().map(|gesture| gesture.button);
        if let Some(button) = held {
            if event.pressed_button != Some(button) {
                // The release was never seen: retire the press, not the event.
                self.cancel_mouse_gesture();
                return;
            }
            let kind = mouse_button(button).map(ClientMouseKind::Drag);
            let hit = self
                .mouse_gesture
                .as_ref()
                .and_then(|gesture| self.gesture_hit(gesture, event.position));
            match (hit, kind) {
                (Some(hit), Some(kind)) => {
                    self.send_mouse(&hit, kind, event.modifiers);
                    if let Some(gesture) = self.mouse_gesture.as_mut() {
                        gesture.hit = hit;
                    }
                }
                _ => self.cancel_mouse_gesture(),
            }
            cx.stop_propagation();
            return;
        }
        // Hover belongs to the hit pane, unlike an owned drag. Shift keeps the
        // pointer local, as it does in other terminals with no app gesture.
        if event.pressed_button.is_none() && !event.modifiers.shift {
            self.track_hover(event.position, event.modifiers, cx);
            if self.hover_link.is_none()
                && let Some(hit) = self
                    .mouse_target_at(event.position)
                    .filter(|hit| hit.mouse_reporting)
            {
                self.send_mouse(&hit, ClientMouseKind::Moved, event.modifiers);
            }
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.release_scrollbar(event, cx) {
            return;
        }
        if self.release_link(event, cx) {
            return;
        }
        if self.release_split(event, cx) {
            cx.stop_propagation();
            return;
        }
        if self
            .mouse_gesture
            .as_ref()
            .is_none_or(|gesture| gesture.button != event.button)
        {
            let released = self.selection.as_mut().is_some_and(Selection::release);
            if released {
                // A click that never left its half-cell selects nothing; keep
                // no empty selection behind.
                let empty = self
                    .selection
                    .as_ref()
                    .zip(self.live.surface.as_deref())
                    .is_some_and(|(selection, surface)| {
                        selection.is_empty(surface, self.cell_width.get())
                    });
                if empty {
                    self.selection = None;
                }
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        cx.stop_propagation();
        let hit = self
            .mouse_gesture
            .as_ref()
            .and_then(|gesture| self.gesture_hit(gesture, event.position));
        let Some(hit) = hit else {
            // The release left the target it was pressed on: retire the press.
            self.cancel_mouse_gesture();
            return;
        };
        self.mouse_gesture = None;
        if let Some(button) = mouse_button(event.button) {
            self.send_mouse(&hit, ClientMouseKind::Up(button), event.modifiers);
        }
        // Finish the application's input before its pane takes the focus.
        if let InputTarget::Pane(pane) = &hit.target {
            self.focus_clicked_pane(pane);
        }
    }

    /// The wheel and mouse target under `position`, in paint-origin coordinates.
    fn mouse_target_at(&self, position: Point<Pixels>) -> Option<WheelTarget> {
        let surface = self.live.surface.as_deref()?;
        let origin = self.bounds.get().origin;
        let x = (position.x - origin.x).to_f64() as f32;
        let y = (position.y - origin.y).to_f64() as f32;
        wheel_target(surface, x, y, self.cell_width.get(), self.cell_height)
    }

    /// Re-resolve a drag against the target it was pressed on: the pointer is
    /// clamped to the pressed bounds, and only the same target, bounds and
    /// daemon boot answer it.
    fn gesture_hit(&self, gesture: &MouseGesture, position: Point<Pixels>) -> Option<WheelTarget> {
        if self.live.boot_id() != Some(gesture.boot.as_str()) {
            return None;
        }
        let surface = self.live.surface.as_deref()?;
        let bounds = gesture.hit.bounds;
        let local = position - self.bounds.get().origin;
        let x = f32::from(local.x).clamp(
            f32::from(bounds.left()),
            f32::from(bounds.right()).next_down(),
        );
        let y = f32::from(local.y).clamp(
            f32::from(bounds.top()),
            f32::from(bounds.bottom()).next_down(),
        );
        let hit = wheel_target(surface, x, y, self.cell_width.get(), self.cell_height)?;
        (hit.target == gesture.hit.target && hit.bounds == bounds && hit.mouse_reporting)
            .then_some(hit)
    }

    /// Send one mouse event to the target its hit names, popup or pane.
    /// `false` when nothing reached the application.
    fn send_mouse(&self, hit: &WheelTarget, kind: ClientMouseKind, modifiers: Modifiers) -> bool {
        self.send_to(&hit.target, vec![hit.mouse_event(kind, modifiers)])
    }

    /// Retire a forwarded press whose release was never observed, so the
    /// application is not left dragging. Only on the boot it was pressed on.
    fn cancel_mouse_gesture(&mut self) {
        let Some(gesture) = self.mouse_gesture.take() else {
            return;
        };
        if self.live.boot_id() != Some(gesture.boot.as_str()) {
            return;
        }
        if let Some(button) = mouse_button(gesture.button) {
            self.send_mouse(
                &gesture.hit,
                ClientMouseKind::Up(button),
                Modifiers::default(),
            );
        }
    }

    /// Whether the daemon routes this pane's right-clicks to its application;
    /// a popup keeps its right-clicks, as it has no client chrome over it.
    fn right_click_to_application(&self, target: &InputTarget, modifiers: Modifiers) -> bool {
        let InputTarget::Pane(id) = target else {
            return true;
        };
        !modifiers.modified()
            && self.live.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .panes
                    .iter()
                    .any(|pane| pane.pane_id == *id && pane.right_click_passthrough)
            })
    }

    /// Ask the daemon to focus a pane, unless it already reports it focused.
    fn focus_clicked_pane(&self, pane: &str) {
        if self.focused_pane().as_deref() != Some(pane) {
            self.connection.focus_pane(pane);
        }
    }

    /// Arm a drag of the pane border under `position`, in window coordinates.
    /// `false` where no border is hit, so the pane below it keeps the press.
    fn press_split(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let (Some(snapshot), Some(surface)) = (&self.live.snapshot, self.live.surface.as_deref())
        else {
            return false;
        };
        let local = position - self.bounds.get().origin;
        let Some(drag) = SplitDrag::press(
            surface,
            snapshot,
            f32::from(local.x),
            f32::from(local.y),
            self.cell_width.get(),
            self.cell_height,
        ) else {
            return false;
        };
        self.split_drag = Some(drag);
        cx.stop_propagation();
        cx.notify();
        true
    }

    /// Follow the pointer with an armed drag. `true` while the drag owns the
    /// move event, so it never reaches an application.
    fn drag_split(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        if !self.split_drag.as_ref().is_some_and(SplitDrag::held) {
            return false;
        }
        // A release outside the grid never reaches its own handler: the drag
        // ends on the first move that says the button is up.
        if event.pressed_button != Some(MouseButton::Left) {
            if let Some(drag) = self.split_drag.as_mut() {
                drag.release();
            }
            self.flush_split();
            cx.notify();
            return false;
        }
        let local = event.position - self.bounds.get().origin;
        let (cell_width, cell_height) = (self.cell_width.get(), self.cell_height);
        if let Some(drag) = self.split_drag.as_mut() {
            drag.drag(
                f32::from(local.x),
                f32::from(local.y),
                cell_width,
                cell_height,
            );
        }
        self.track_split_cursor(event.position, cx);
        self.flush_split();
        true
    }

    /// End an armed drag on release, sending the ratio it last asked for.
    /// `true` while the release belongs to the drag.
    fn release_split(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) -> bool {
        if event.button != MouseButton::Left
            || !self.split_drag.as_ref().is_some_and(SplitDrag::held)
        {
            return false;
        }
        if let Some(drag) = self.split_drag.as_mut() {
            drag.release();
        }
        self.flush_split();
        cx.notify();
        true
    }

    /// Hand the drag's latest ratio to the daemon, drop a drag that can no
    /// longer name its border, and keep a released one until its last ratio has
    /// been sent.
    fn flush_split(&mut self) {
        let Some(drag) = self.split_drag.as_mut() else {
            return;
        };
        if drag.flush(&self.live, &self.connection) {
            self.split_drag = None;
        }
    }

    /// The cursor the border under `position` asks for, if any.
    fn split_cursor_at(&self, position: Point<Pixels>) -> Option<CursorStyle> {
        if let Some(drag) = self.split_drag.as_ref().filter(|drag| drag.held()) {
            return Some(drag.cursor());
        }
        let surface = self.live.surface.as_deref()?;
        let local = position - self.bounds.get().origin;
        splits::split_at(
            surface,
            f32::from(local.x),
            f32::from(local.y),
            self.cell_width.get(),
            self.cell_height,
        )
        .map(|split| splits::cursor(split.direction))
    }

    /// Show the resize cursor while the pointer is on a border a press would
    /// pick up, so the handle is visible before it is grabbed.
    fn track_split_cursor(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let cursor = self.split_cursor_at(position);
        if cursor != self.split_cursor {
            self.split_cursor = cursor;
            cx.notify();
        }
    }

    /// The find field, laid over the top right of the grid while it is open.
    /// It is chrome rather than terminal cells, so it wears the shell's tokens
    /// and a press on it never reaches the pane below.
    fn find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.find.as_ref()?;
        let chrome = cx.theme();
        let (bg, border, fg, muted, danger) = (
            chrome.secondary,
            chrome.border,
            chrome.foreground,
            chrome.muted_foreground,
            chrome.danger,
        );
        let label = match bar.search.error() {
            Some(error) => error.to_owned(),
            None => match bar.search.count() {
                Count::Unknown => String::new(),
                Count::None => rust_i18n::t!("terminal.find.none").to_string(),
                Count::Current { index, total } => {
                    rust_i18n::t!("terminal.find.count", index = index, total = total).to_string()
                }
                Count::Found(total) => {
                    rust_i18n::t!("terminal.find.found", total = total).to_string()
                }
            },
        };
        let failed = bar.search.error().is_some();
        let can_step = bar.search.has_matches();
        let input = bar.input.clone();
        let focus_input = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if let Some(bar) = this.find.as_ref() {
                let focus = bar.input.read(cx).focus_handle(cx).clone();
                window.focus(&focus, cx);
            }
        };
        let button = |id: &'static str, glyph: &'static str, step: Option<Step>, enabled: bool| {
            div()
                .id(id)
                .flex_none()
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .text_size(px(13.))
                .text_color(if enabled { fg } else { muted })
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(|button| button.bg(fg.opacity(0.12)))
                })
                .child(glyph)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    match step {
                        Some(step) => this.find_step(step, cx),
                        None => this.close_find(window, cx),
                    }
                    focus_input(this, window, cx);
                }))
        };
        Some(
            div()
                .id("find-bar")
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .w(px(340.))
                .flex()
                .items_center()
                .gap_1()
                .p(px(4.))
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(bg)
                .text_color(fg)
                // The field is not a pane: a press in it must not start a
                // selection or move focus to the grid under it.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if keystroke.key == "escape" && !keystroke.modifiers.modified() {
                        this.close_find(window, cx);
                        cx.stop_propagation();
                        window.prevent_default();
                        return;
                    }
                    let step = match keystroke.key.as_str() {
                        "enter" if keystroke.modifiers.shift => Some(Step::Newer),
                        "enter" => Some(Step::Older),
                        "up" if !keystroke.modifiers.modified() => Some(Step::Older),
                        "down" if !keystroke.modifiers.modified() => Some(Step::Newer),
                        _ => None,
                    };
                    if let Some(step) = step {
                        this.find_step(step, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&input).appearance(false).bordered(false)),
                )
                .child(
                    div()
                        .flex_none()
                        .px(px(4.))
                        .text_size(px(11.5))
                        .text_color(if failed { danger } else { muted })
                        .child(label),
                )
                .child(button("find-older", "↑", Some(Step::Older), can_step))
                .child(button("find-newer", "↓", Some(Step::Newer), can_step))
                .child(button("find-close", "×", None, true))
                .into_any_element(),
        )
    }

    /// A small marker in the bottom right of the grid while copy mode is on:
    /// the mode swallows keys, and this is the only sign of why.
    fn copy_mode_badge(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.copy_mode.as_ref()?;
        let chrome = cx.theme();
        Some(
            div()
                .absolute()
                .bottom(px(8.))
                .right(px(24.))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(chrome.primary)
                .text_color(chrome.primary_foreground)
                .text_size(px(11.))
                .child(rust_i18n::t!("terminal.copy_mode.badge").to_string())
                .into_any_element(),
        )
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The daemon's window title while this view is the focused one; the
        // app's own name when it reports none.
        let title = self
            .live
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| rust_i18n::t!("terminal.title.fallback").to_string());
        window.set_window_title(&title);
        let live = self.live.clone();
        // Each canvas closure takes its own handle: both outlive this render.
        let prepaint_painter = self.painter.clone();
        let prepaint_font = self.font.clone();
        let paint_painter = self.painter.clone();
        let paint_font = self.font.clone();
        // `self.marked` cannot be read from the `move` closure, so the
        // uncommitted composition is captured here like the painter and font.
        let marked = self.marked.clone();
        let view_bounds = self.bounds.clone();
        let theme = self.theme.clone();
        let connection = self.connection.clone();
        let sent_options = self.sent_options.clone();
        let focus = self.focus.clone();
        let entity = cx.entity();
        // The cell the font resolves to is measured while the frame is built
        // (only the paint phase has the text system), and every hit test and
        // region view measures in it on the next frame.
        let cell = self.cell_width.clone();
        let cell_width = self.cell_width.get();
        // The grid paints through one cached view per pane where it can, so a
        // busy pane stops reshaping every other one. `Regions::views` creates
        // and updates entities, which GPUI only allows while the frame is
        // being built, and this is the only place the frame exists. That makes
        // this call the one deliberate exception to the project's "no state
        // changes in render" rule; see src/AGENTS.md.
        let regions_painter = self.painter.clone();
        let font_size = self.font_size;
        let cell_height = self.cell_height;
        let look = Look {
            font: self.font.clone(),
            font_size,
            cell_width,
            cell_height,
            theme: theme.clone(),
        };
        let regions = self
            .regions
            .views(regions_painter, live.surface.as_ref(), &look, cx);
        // An empty set means the grid must paint as a whole: a surface with
        // graphics placements (whose textures a replayed region would outlive)
        // or no surface at all.
        let whole = regions.is_empty();
        // The over-paint chrome the canvas draws: what the pointer selected,
        // the find matches, the copy-mode cursor, the link it hovers, and which
        // cursor those ask for. A popup covers the panes, so nothing under it
        // is tinted; the find field and copy mode never open over one.
        let selection_highlights = live
            .surface
            .as_deref()
            .and_then(|surface| {
                self.selection
                    .as_ref()
                    .map(|selection| selection.highlights(surface, cell_width))
            })
            .unwrap_or_default();
        let find_hits = self
            .find
            .as_ref()
            .zip(live.surface.as_deref())
            .filter(|(_, surface)| surface.popup.is_none())
            .and_then(|(bar, surface)| {
                surface
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == bar.search.pane_id())
                    .map(|pane| bar.search.hits(pane))
            })
            .unwrap_or_default();
        let copy_hits = self
            .copy_mode
            .as_ref()
            .zip(live.surface.as_deref())
            .filter(|(_, surface)| surface.popup.is_none())
            .and_then(|(state, surface)| {
                surface
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == state.mode.pane_id())
                    .map(|pane| state.mode.hits(pane))
            })
            .unwrap_or_default();
        let hover_link = self.hover_link.clone();
        let link_in_popup = live.surface.as_ref().is_some_and(|s| s.popup.is_some());
        let cursor = if hover_link.is_some() {
            Some(CursorStyle::PointingHand)
        } else {
            self.split_cursor
        };
        let disconnected = self.live.status == crate::herdr::ConnectionStatus::Disconnected;
        let stale = self.live.stale();
        // A handshake refused because one side is older names the command that
        // fixes it; nothing is run for the user, because stopping the daemon
        // would take the running agents with it.
        let update_hint = self.live.version_mismatch.as_ref().map(|mismatch| {
            use herdr_client::VersionMismatch;
            match mismatch {
                VersionMismatch::DaemonOutdated { .. } => {
                    rust_i18n::t!("terminal.reconnect.update_daemon").to_string()
                }
                VersionMismatch::ClientOutdated { .. } => {
                    rust_i18n::t!("terminal.reconnect.update_client").to_string()
                }
            }
        });
        let reason = self.live.error.clone().unwrap_or_default();
        // The reconnect card is chrome, not terminal cells, so it wears the
        // shell's tokens. Copied out because `cx.listener` borrows the context
        // the theme was read from.
        let (scrim, card_bg, card_border, card_fg, card_muted, card_error, accent, accent_fg) = {
            let chrome = cx.theme();
            (
                chrome.background.opacity(0.82),
                chrome.secondary,
                chrome.border,
                chrome.foreground,
                chrome.muted_foreground,
                chrome.danger,
                chrome.primary,
                chrome.primary_foreground,
            )
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .on_scroll_wheel(cx.listener(Self::scroll_wheel))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
                    .on_mouse_move(cx.listener(Self::mouse_move))
                    // The pane border and a hovered link both change the
                    // pointer, so the grid wears whichever is under it.
                    .when_some(cursor, |grid, cursor| grid.cursor(cursor))
                    .child(
                        // Every card keeps this much background from the pane
                        // area's edges, so the grid is laid out in what is
                        // left: the padding is on the grid's own box, which is
                        // what the viewport, the region views and every hit
                        // test measure from. It is painted as the same channel
                        // the gutters are, so a card sits on one surface
                        // whether it is the only pane or one of four.
                        div()
                            .size_full()
                            .p(px(cards::INSET))
                            .bg(rgb(cards::channel(&theme)))
                            .child(
                                div()
                                    .relative()
                                    .size_full()
                                    // The cached per-pane views paint the cells;
                                    // the canvas below them owns input, the
                                    // whole-grid fallback, and the IME
                                    // composition.
                                    .children(regions)
                                    .child(
                                        canvas(
                                            move |bounds, window, cx| {
                                                let cell_width = prepaint_painter
                                                    .borrow_mut()
                                                    .cell_width(&prepaint_font, window, cx);
                                                if view_bounds.get() != bounds {
                                                    view_bounds.set(bounds);
                                                }
                                                // The number the painter lays cells out
                                                // with is the one every hit test and
                                                // region view must use, so it is kept
                                                // for the frames after this one.
                                                cell.set(cell_width);
                                                let options = ConnectOptions {
                                                    surface_size: viewport(
                                                        bounds.size.width.to_f64() as f32,
                                                        bounds.size.height.to_f64() as f32,
                                                        cell_width,
                                                        cell_height,
                                                    ),
                                                    cell_width_px: cell_width.round().max(1.)
                                                        as u32,
                                                    cell_height_px: cell_height.round() as u32,
                                                };
                                                if sent_options.get() != Some(options)
                                                    && connection.resize(options)
                                                {
                                                    sent_options.set(Some(options));
                                                }
                                                cell_width
                                            },
                                            move |bounds, cell_width, window, cx| {
                                                window.handle_input(
                                                    &focus,
                                                    ViewInputHandler::new(bounds, entity.clone()),
                                                    cx,
                                                );
                                                let Some(surface) = live.surface.clone() else {
                                                    return;
                                                };
                                                let mut painter = paint_painter.borrow_mut();
                                                painter.set_appearance(
                                                    font_size,
                                                    cell_height,
                                                    theme.clone(),
                                                );
                                                if whole {
                                                    painter.paint_frame(
                                                        &surface.frame,
                                                        bounds.origin,
                                                        Some(bounds.size),
                                                        cell_width,
                                                        &paint_font,
                                                        &surface.panes,
                                                        None,
                                                        None,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                                // Over the cells, under the popup: the
                                                // pane cards are chrome the client owns,
                                                // and they paint over cells the daemon
                                                // spent on borders.
                                                paint_cards(
                                                    &surface,
                                                    bounds.origin,
                                                    cell_width,
                                                    cell_height,
                                                    &theme,
                                                    cx,
                                                    window,
                                                );
                                                // The daemon's popup carries its own grid and is
                                                // not a pane of the composite one, so it paints
                                                // as a floating layer over the cells, below the
                                                // IME composition that closes this closure.
                                                // `input_area` is its rectangle — input lands and
                                                // the IME clips there — so the paint reuses that
                                                // geometry rather than centring the popup a second
                                                // time. The daemon's `width`/`height` only describe
                                                // the size it asked for; the grid painted is the
                                                // one the popup reports.
                                                if let Some(popup) =
                                                    surface.popup.as_deref().filter(|popup| {
                                                        popup.frame.width > 0
                                                            && popup.frame.height > 0
                                                    })
                                                {
                                                    let rect = input_area(
                                                        Some(&surface),
                                                        bounds,
                                                        cell_width,
                                                        cell_height,
                                                    );
                                                    // In paint order: an opaque backdrop, so no
                                                    // pane cell reads through; the popup's own
                                                    // cells; then a one-pixel border the cells
                                                    // would otherwise cover. Each quad and layer
                                                    // takes an order above what it overlaps, so
                                                    // this sequence is what stacks them.
                                                    window.paint_quad(fill(
                                                        rect,
                                                        rgb(theme.background),
                                                    ));
                                                    painter.paint_frame(
                                                        &popup.frame,
                                                        rect.origin,
                                                        Some(rect.size),
                                                        cell_width,
                                                        &paint_font,
                                                        &[],
                                                        None,
                                                        None,
                                                        window,
                                                        cx,
                                                    );
                                                    window.paint_quad(outline(
                                                        rect,
                                                        rgb(theme.muted),
                                                        BorderStyle::Solid,
                                                    ));
                                                }
                                                // Selection and link chrome sit over the cells
                                                // they were made in, popup included. A terminal
                                                // tint keeps the glyphs readable on every theme;
                                                // the hovered link gets a hairline, which is how
                                                // it says "click me" without moving any text.
                                                let popup_origin =
                                                    surface.popup.as_deref().map(|popup| {
                                                        bounds.origin
                                                            + popup_origin(
                                                                &surface.frame,
                                                                &popup.frame,
                                                                cell_width,
                                                                cell_height,
                                                            )
                                                    });
                                                let chrome_origin = |popup: bool| {
                                                    if popup {
                                                        popup_origin.unwrap_or(bounds.origin)
                                                    } else {
                                                        bounds.origin
                                                    }
                                                };
                                                for highlight in &selection_highlights {
                                                    let columns = &highlight.columns;
                                                    if columns.is_empty() {
                                                        continue;
                                                    }
                                                    let rect = Bounds::new(
                                                        chrome_origin(highlight.popup)
                                                            + point(
                                                                px(f32::from(columns.start)
                                                                    * cell_width),
                                                                px(f32::from(highlight.row)
                                                                    * cell_height),
                                                            ),
                                                        size(
                                                            px(f32::from(
                                                                columns.end - columns.start,
                                                            ) * cell_width),
                                                            px(cell_height),
                                                        ),
                                                    );
                                                    window.paint_quad(fill(
                                                        rect,
                                                        rgba((theme.foreground << 8) | 0x59),
                                                    ));
                                                }
                                                // Find matches are tinted, the one the
                                                // field is on more strongly. Both sit
                                                // above the cells so a dark theme and a
                                                // light one read the same.
                                                for hit in &find_hits {
                                                    let columns = &hit.columns;
                                                    if columns.is_empty() {
                                                        continue;
                                                    }
                                                    let rect = Bounds::new(
                                                        bounds.origin
                                                            + point(
                                                                px(f32::from(columns.start)
                                                                    * cell_width),
                                                                px(f32::from(hit.row)
                                                                    * cell_height),
                                                            ),
                                                        size(
                                                            px(f32::from(
                                                                columns.end - columns.start,
                                                            ) * cell_width),
                                                            px(cell_height),
                                                        ),
                                                    );
                                                    let tint = if hit.current {
                                                        (theme.cursor << 8) | 0x66
                                                    } else {
                                                        (theme.foreground << 8) | 0x2e
                                                    };
                                                    window.paint_quad(fill(rect, rgba(tint)));
                                                }
                                                // Copy mode paints what it marked, and
                                                // its cursor cell solidly so it is legible
                                                // against any selection tint under it.
                                                for hit in &copy_hits {
                                                    let columns = &hit.columns;
                                                    if columns.is_empty() {
                                                        continue;
                                                    }
                                                    let rect = Bounds::new(
                                                        bounds.origin
                                                            + point(
                                                                px(f32::from(columns.start)
                                                                    * cell_width),
                                                                px(f32::from(hit.row)
                                                                    * cell_height),
                                                            ),
                                                        size(
                                                            px(f32::from(
                                                                columns.end - columns.start,
                                                            ) * cell_width),
                                                            px(cell_height),
                                                        ),
                                                    );
                                                    let tint = if hit.cursor {
                                                        (theme.cursor << 8) | 0xa8
                                                    } else {
                                                        (theme.foreground << 8) | 0x59
                                                    };
                                                    window.paint_quad(fill(rect, rgba(tint)));
                                                }
                                                if let Some(link) = &hover_link {
                                                    let columns = &link.columns;
                                                    if !columns.is_empty() {
                                                        let start =
                                                            f32::from(columns.start) * cell_width;
                                                        let width =
                                                            f32::from(columns.end - columns.start)
                                                                * cell_width;
                                                        let top = f32::from(link.row) * cell_height
                                                            + cell_height
                                                            - 2.;
                                                        window.paint_quad(fill(
                                                            Bounds::new(
                                                                chrome_origin(link_in_popup)
                                                                    + point(px(start), px(top)),
                                                                size(px(width), px(1.5)),
                                                            ),
                                                            rgb(theme.foreground),
                                                        ));
                                                    }
                                                }
                                                // Painted after the frame so the composition
                                                // sits above the cells and the cursor.
                                                if !marked.is_empty() {
                                                    painter.paint_composition(
                                                        &marked,
                                                        input_cursor_bounds(
                                                            Some(&surface),
                                                            bounds.origin,
                                                            cell_width,
                                                            cell_height,
                                                        )
                                                        .origin,
                                                        input_area(
                                                            Some(&surface),
                                                            bounds,
                                                            cell_width,
                                                            cell_height,
                                                        ),
                                                        &paint_font,
                                                        window,
                                                    );
                                                }
                                            },
                                        )
                                        .size_full(),
                                    )
                                    // The two scrollback features' chrome, over
                                    // the cells they address: the find field and
                                    // the copy-mode badge. `Option` children keep
                                    // the tree empty while neither is on.
                                    .children(self.find_bar(cx))
                                    .children(self.copy_mode_badge(cx)),
                            ),
                    )
                    // A dropped endpoint leaves the last picture up, dimmed,
                    // with the reason and a way to dial now: a blank window
                    // would hide the fact that a reconnect is under way.
                    .children(disconnected.then(|| {
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(px(16.))
                            .when(stale, |overlay| overlay.bg(scrim))
                            .child(
                                div()
                                    .id("reconnect-card")
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .p(px(16.))
                                    .w(px(440.))
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(card_border)
                                    .bg(card_bg)
                                    .text_color(card_fg)
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(
                                        rust_i18n::t!("terminal.reconnect.title").to_string(),
                                    ))
                                    .child(div().text_size(px(12.5)).text_color(card_muted).child(
                                        rust_i18n::t!("terminal.reconnect.body").to_string(),
                                    ))
                                    .children((!reason.is_empty()).then(|| {
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(card_error)
                                            .child(reason.clone())
                                    }))
                                    .children(update_hint.clone().map(|hint| {
                                        div().text_size(px(11.5)).text_color(card_muted).child(hint)
                                    }))
                                    .child(
                                        div()
                                            .id("reconnect-now")
                                            .self_start()
                                            .px(px(12.))
                                            .py(px(5.))
                                            .rounded_md()
                                            .bg(accent)
                                            .text_color(accent_fg)
                                            .cursor_pointer()
                                            .hover(|style| style.opacity(0.9))
                                            .on_click(cx.listener(Self::retry_connection))
                                            .child(
                                                rust_i18n::t!("terminal.reconnect.retry")
                                                    .to_string(),
                                            ),
                                    ),
                            )
                    })),
            )
    }
}

/// The pane cards: the channel in the daemon's border cells, then each card
/// grown into its own gutter so the gap between two cards is the same
/// [`cards::GAP`] both ways, whichever way the cells fall. The focused card
/// takes the chrome's accent, so the grid itself says which pane input
/// reaches.
///
/// This paints only chrome: `cards::bands` is the border-and-gutter cells of a
/// pane's rectangle, never a cell the pane's application wrote.
fn paint_cards(
    surface: &PaneSurfaceFrame,
    origin: Point<Pixels>,
    cell_width: f32,
    cell_height: f32,
    theme: &Theme,
    cx: &App,
    window: &mut Window,
) {
    let cells = |rect: herdr_client::protocol::SurfaceRect| {
        Bounds::new(
            origin
                + point(
                    px(f32::from(rect.x) * cell_width),
                    px(f32::from(rect.y) * cell_height),
                ),
            size(
                px(f32::from(rect.width) * cell_width),
                px(f32::from(rect.height) * cell_height),
            ),
        )
    };
    // The channel first: every border cell is painted in it, then the cards
    // cover all but the gap.
    let channel = rgb(cards::channel(theme));
    for pane in &surface.panes {
        for band in cards::bands(pane)
            .into_iter()
            .filter(|band| band.width > 0 && band.height > 0)
        {
            window.paint_quad(fill(cells(band), channel));
        }
    }
    let card_tone = rgb(theme.background);
    let idle: Hsla = cx.theme().border;
    // A pane edge reads as an edge, not as a choice: the focused one takes the
    // chrome's accent, the rest sit a step away from the channel.
    let focused = cx.theme().primary;
    let half_gap = px(cards::GAP / 2.);
    for pane in &surface.panes {
        let content = cells(cards::content(pane));
        let rect = cells(pane.rect);
        // How far the card may grow into each gutter, and whether that gutter
        // is shared with another pane. A shared one gives half the gap back to
        // the neighbour; an outer one grows all the way to the pane's edge, so
        // the only margin left is the grid's own inset.
        let (shared_left, shared_right, shared_top, shared_bottom) =
            shared_edges(pane, &surface.panes);
        let grow = |band: Pixels, shared: bool| {
            if shared {
                (band - half_gap).max(px(0.))
            } else {
                band
            }
        };
        let left_band = (content.origin.x - rect.origin.x).max(px(0.));
        let right_band =
            (rect.origin.x + rect.size.width - (content.origin.x + content.size.width)).max(px(0.));
        let top_band = (content.origin.y - rect.origin.y).max(px(0.));
        let bottom_band = (rect.origin.y + rect.size.height
            - (content.origin.y + content.size.height))
            .max(px(0.));
        let (left, right) = (grow(left_band, shared_left), grow(right_band, shared_right));
        let (top, bottom) = (grow(top_band, shared_top), grow(bottom_band, shared_bottom));
        let card = Bounds::new(
            content.origin - point(left, top),
            size(
                content.size.width + left + right,
                content.size.height + top + bottom,
            ),
        );
        // The band ring in the card's own tone. The content itself is left
        // alone: those cells are the daemon's, already painted.
        for ring in [
            Bounds::new(card.origin, size(card.size.width, top)),
            Bounds::new(
                point(card.origin.x, content.origin.y + content.size.height),
                size(card.size.width, bottom),
            ),
            Bounds::new(
                point(card.origin.x, content.origin.y),
                size(left, content.size.height),
            ),
            Bounds::new(
                point(content.origin.x + content.size.width, content.origin.y),
                size(right, content.size.height),
            ),
        ] {
            if ring.size.width > px(0.) && ring.size.height > px(0.) {
                window.paint_quad(fill(ring, card_tone));
            }
        }
        window.paint_quad(quad(
            card,
            px(cards::RADIUS),
            transparent_black(),
            px(1.),
            if pane.focused { focused } else { idle },
            BorderStyle::Solid,
        ));
    }
}

/// Whether a pane shares each edge with another pane, as left, right, top and
/// bottom. The daemon's rectangles touch on a shared edge and overlap the
/// other pane's border cell on it; an outer edge touches nothing.
fn shared_edges(
    pane: &herdr_client::protocol::PaneSurfacePane,
    panes: &[herdr_client::protocol::PaneSurfacePane],
) -> (bool, bool, bool, bool) {
    let rect = pane.rect;
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    let vertical_overlap = |other: &herdr_client::protocol::SurfaceRect| {
        rect.y < other.y.saturating_add(other.height) && other.y < bottom
    };
    let horizontal_overlap = |other: &herdr_client::protocol::SurfaceRect| {
        rect.x < other.x.saturating_add(other.width) && other.x < right
    };
    let mut shared = (false, false, false, false);
    for other in panes.iter().filter(|other| other.pane_id != pane.pane_id) {
        let other_right = other.rect.x.saturating_add(other.rect.width);
        let other_bottom = other.rect.y.saturating_add(other.rect.height);
        shared.0 |= other_right == rect.x && vertical_overlap(&other.rect);
        shared.1 |= other.rect.x == right && vertical_overlap(&other.rect);
        shared.2 |= other_bottom == rect.y && horizontal_overlap(&other.rect);
        shared.3 |= other.rect.y == bottom && horizontal_overlap(&other.rect);
    }
    shared
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text: Vec<u16> = self.marked.encode_utf16().collect();
        let range = range.start.min(text.len())..range.end.min(text.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&text[range]))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| 0..self.marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked.clear();
        cx.notify();
    }

    /// The one place text reaches a pane: a layout's character, a dead key's
    /// result, or an IME commit all arrive here, already composed. While copy
    /// mode is on, the same text is read as its copy-mode keys instead.
    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        if self.copy_mode.is_some() {
            self.copy_mode_text(text, cx);
            cx.notify();
            return;
        }
        if !text.is_empty() {
            self.send(vec![ClientPaneInputEvent::TextCommit(text.into())]);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = text.into();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let surface = self.live.surface.as_deref();
        let cell_width = self.cell_width.get();
        let bounds = self.bounds.get();
        let cursor = input_cursor_bounds(surface, bounds.origin, cell_width, self.cell_height);
        let grid = input_area(surface, bounds, cell_width, self.cell_height);
        Some(self.painter.borrow().composition_bounds(
            &self.marked,
            range,
            cursor,
            grid,
            &self.font,
            window,
        ))
    }

    /// A terminal has no text cursor to place one in.
    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    // Named imports only: the glob above drags gpui's own `test` attribute
    // macro in, which shadows the builtin one and cannot expand.
    use super::{mouse_button, popup_target};
    use gpui_kit::{MouseButton, NavigationDirection};
    use herdr_client::protocol::{
        ClientMouseButton, ClientShellPopupSurface, FrameData, PaneSurfaceFrame,
    };

    /// A surface with one empty grid, optionally carrying a popup of the same
    /// shape. Nothing here reads the cells: the decision under test is which
    /// name input goes to.
    fn surface(popup: Option<&str>) -> PaneSurfaceFrame {
        let frame = FrameData {
            width: 80,
            height: 24,
            cells: vec![],
            cursor: None,
            hyperlinks: vec![],
            graphics: vec![],
        };
        PaneSurfaceFrame {
            boot_id: "boot".into(),
            projection_revision: 1,
            surface_revision: 1,
            frame: frame.clone(),
            panes: vec![],
            splits: vec![],
            popup: popup.map(|terminal_id| {
                Box::new(ClientShellPopupSurface {
                    terminal_id: terminal_id.into(),
                    title: "Probe Popup".into(),
                    width: None,
                    height: None,
                    frame: frame.clone(),
                    mouse_reporting: true,
                    sgr_pixel_mouse: false,
                    pixel_width: 0,
                    pixel_height: 0,
                })
            }),
            graphics: Default::default(),
        }
    }

    #[test]
    fn input_belongs_to_the_popup_exactly_while_one_is_open() {
        assert_eq!(popup_target(None), None);
        assert_eq!(popup_target(Some(&surface(None))), None);
        let open = surface(Some("popup-1"));
        assert_eq!(popup_target(Some(&open)), Some("popup-1"));
    }

    #[test]
    fn mouse_button_maps_the_wire_buttons_and_drops_the_rest() {
        assert_eq!(
            mouse_button(MouseButton::Left),
            Some(ClientMouseButton::Left)
        );
        assert_eq!(
            mouse_button(MouseButton::Right),
            Some(ClientMouseButton::Right)
        );
        assert_eq!(
            mouse_button(MouseButton::Middle),
            Some(ClientMouseButton::Middle)
        );
        assert_eq!(
            mouse_button(MouseButton::Navigate(NavigationDirection::Back)),
            None
        );
    }
}
