//! The terminal view: one daemon surface painted natively, with GPUI input
//! mapped onto the daemon's semantic input events.
//!
//! Ported in shape from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/{window.rs,
//! window/input.rs, window/mouse.rs, window/render.rs, input.rs}`. Selection, links, copy mode,
//! scrollbar dragging, split dragging and the chrome around the pane are not
//! part of M3; the daemon draws the pane borders and focus separators as cells.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::*;
// `when_some` and its siblings ride in on this trait, which the glob above does
// not bring along.
use gpui_kit::prelude::FluentBuilder as _;
// The chrome's accent, which a focused pane's card takes: the terminal palette
// has no accent of its own that matches the shell around it.
use gpui_kit::component::ActiveTheme as _;
use herdr_client::ConnectOptions;
use herdr_client::ConnectTarget;
use herdr_client::protocol::{
    ClientMouseButton, ClientMouseKind, ClientPaneInputEvent, PaneSurfaceFrame,
};

use crate::herdr::{Connection, LiveState, decode};
use crate::terminal::cards;
use crate::terminal::painter::TerminalPainter;
use crate::terminal::regions::{Look, Regions};
use crate::terminal::split_drag::SplitDrag;
use crate::terminal::splits;
use crate::terminal::theme::Theme;
use crate::terminal::{
    CELL_HEIGHT, FONT_SIZE, InputTarget, WheelAccumulator, WheelTarget, input_area,
    input_cursor_bounds, key_input, pane_at, viewport, wheel_target,
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
        let (pulse, pulses) = crossbeam_channel::unbounded::<()>();
        let connection = Connection::start(ConnectTarget::Local, move || {
            // A full inbox already means "there is something to read".
            let _ = pulse.try_send(());
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
            theme: Theme::default(),
            live: LiveState::default(),
            drawn_revision: 0,
            bounds: Rc::new(Cell::new(Bounds::default())),
            cell_width: Rc::new(Cell::new(FONT_SIZE / 2.)),
            sent_options: Rc::new(Cell::new(None)),
            marked: String::new(),
            wheel: WheelAccumulator::default(),
            mouse_gesture: None,
            split_drag: None,
            split_cursor: None,
            _events: events,
            regions: Regions::new(),
        }
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
        let next = self.connection.state();
        if next.revision != self.drawn_revision || next.status != self.live.status {
            self.drawn_revision = next.revision;
            self.live = next;
            cx.notify();
        }
        // A split drag waiting for its answer continues here: an answer does not
        // change the revision, and it is exactly what frees the next ratio.
        self.flush_split();
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
        let Some(target) = wheel_target(&surface, x, y, cell_width, CELL_HEIGHT) else {
            self.wheel = WheelAccumulator::default();
            return;
        };
        let steps = self.wheel.steps(&target, event, cell_width, CELL_HEIGHT);
        cx.stop_propagation();
        for input in target.wheel_events(steps, event.modifiers) {
            self.send_to(&target.target, vec![input]);
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A new press retires a forwarded one whose release was never observed.
        self.cancel_mouse_gesture();
        // The border between panes belongs to the shell, before the pane below
        // it has any say: upstream's order is the scrollbar, the split, then
        // the pane's own application.
        if event.button == MouseButton::Left && self.press_split(event.position, cx) {
            return;
        }
        let reporting = self
            .mouse_target_at(event.position)
            .filter(|hit| hit.mouse_reporting)
            // Shift bypasses the application's mouse mode, as in a real
            // terminal; a local Shift action (selection) lands in a later
            // milestone.
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
        // No application reported the mouse under the press: keep the M3
        // behaviour, where only the left button moves pane focus.
        window.focus(&self.focus, cx);
        if event.button != MouseButton::Left {
            return;
        }
        let Some(surface) = self.live.surface.clone() else {
            return;
        };
        let Some(pane) = pane_at(
            &surface,
            self.bounds.get(),
            event.position,
            self.cell_width.get(),
            CELL_HEIGHT,
        ) else {
            return;
        };
        if self.focused_pane().as_deref() == Some(pane) {
            return;
        }
        // The daemon owns focus: ask, and let the next snapshot say what changed.
        self.connection.focus_pane(pane);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag_split(event, cx) {
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
            self.track_split_cursor(event.position, cx);
            if let Some(hit) = self
                .mouse_target_at(event.position)
                .filter(|hit| hit.mouse_reporting)
            {
                self.send_mouse(&hit, ClientMouseKind::Moved, event.modifiers);
            }
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.release_split(event, cx) {
            cx.stop_propagation();
            return;
        }
        if self
            .mouse_gesture
            .as_ref()
            .is_none_or(|gesture| gesture.button != event.button)
        {
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
        wheel_target(surface, x, y, self.cell_width.get(), CELL_HEIGHT)
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
        let hit = wheel_target(surface, x, y, self.cell_width.get(), CELL_HEIGHT)?;
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
            CELL_HEIGHT,
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
        let (cell_width, cell_height) = (self.cell_width.get(), CELL_HEIGHT);
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
            CELL_HEIGHT,
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
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let look = Look {
            font: self.font.clone(),
            font_size: FONT_SIZE,
            cell_width,
            cell_height: CELL_HEIGHT,
            theme: theme.clone(),
        };
        let regions = self
            .regions
            .views(regions_painter, live.surface.as_ref(), &look, cx);
        // An empty set means the grid must paint as a whole: a surface with
        // graphics placements (whose textures a replayed region would outlive)
        // or no surface at all.
        let whole = regions.is_empty();

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
                    // The pane border is draggable, so it says so before a
                    // press: a handle nobody can see is a handle nobody uses.
                    .when_some(self.split_cursor, |grid, cursor| grid.cursor(cursor))
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
                            .bg(rgb(theme.surface))
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
                                                        CELL_HEIGHT,
                                                    ),
                                                    cell_width_px: cell_width.round().max(1.)
                                                        as u32,
                                                    cell_height_px: CELL_HEIGHT.round() as u32,
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
                                                    FONT_SIZE,
                                                    CELL_HEIGHT,
                                                    theme.clone(),
                                                );
                                                if whole {
                                                    painter.paint_frame(
                                                        &surface.frame,
                                                        bounds.origin,
                                                        Some(bounds.size),
                                                        cell_width,
                                                        &paint_font,
                                                        &[],
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
                                                        CELL_HEIGHT,
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
                                                // Painted after the frame so the composition
                                                // sits above the cells and the cursor.
                                                if !marked.is_empty() {
                                                    painter.paint_composition(
                                                        &marked,
                                                        input_cursor_bounds(
                                                            Some(&surface),
                                                            bounds.origin,
                                                            cell_width,
                                                            CELL_HEIGHT,
                                                        )
                                                        .origin,
                                                        input_area(
                                                            Some(&surface),
                                                            bounds,
                                                            cell_width,
                                                            CELL_HEIGHT,
                                                        ),
                                                        &paint_font,
                                                        window,
                                                    );
                                                }
                                            },
                                        )
                                        .size_full(),
                                    ),
                            ),
                    ),
            )
    }
}

/// One gap per pane, painted over the cells the daemon spent on borders, then
/// one stroked rectangle per pane. The focused pane takes the chrome's accent,
/// so the grid itself says which pane input reaches.
///
/// This paints only chrome: `cards::bands` is the border-and-gutter cells of a
/// pane's rectangle, never a cell the pane's application wrote.
fn paint_cards(
    surface: &PaneSurfaceFrame,
    origin: Point<Pixels>,
    cell_width: f32,
    theme: &Theme,
    cx: &App,
    window: &mut Window,
) {
    let cells = |rect: herdr_client::protocol::SurfaceRect| {
        Bounds::new(
            origin
                + point(
                    px(f32::from(rect.x) * cell_width),
                    px(f32::from(rect.y) * CELL_HEIGHT),
                ),
            size(
                px(f32::from(rect.width) * cell_width),
                px(f32::from(rect.height) * CELL_HEIGHT),
            ),
        )
    };
    for pane in &surface.panes {
        for band in cards::bands(pane)
            .into_iter()
            .filter(|band| band.width > 0 && band.height > 0)
        {
            // The gap is the palette's own panel tone: a card then sits a step
            // above the channel between cards instead of level with it.
            window.paint_quad(fill(cells(band), rgb(theme.surface)));
        }
    }
    // A pane edge reads as an edge, not as a choice: the focused one takes the
    // chrome's primary, the rest sit a step away from the channel.
    let idle: Hsla = rgb(crate::terminal::theme::mix(
        theme.background,
        theme.foreground,
        22,
    ))
    .into();
    let focused = cx.theme().primary;
    for pane in &surface.panes {
        let bounds = cells(cards::content(pane)).dilate(px(1.));
        window.paint_quad(quad(
            bounds,
            px(cards::RADIUS),
            transparent_black(),
            px(1.),
            if pane.focused { focused } else { idle },
            BorderStyle::Solid,
        ));
    }
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
    /// result, or an IME commit all arrive here, already composed.
    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
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
        let cursor = input_cursor_bounds(surface, bounds.origin, cell_width, CELL_HEIGHT);
        let grid = input_area(surface, bounds, cell_width, CELL_HEIGHT);
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
