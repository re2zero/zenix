//! Single-terminal app — auto-connects to herdr on startup.

use std::{sync::mpsc::{self, Receiver, Sender}, time::Duration};

use alacritty_terminal::{index::Side, selection::SelectionType};
use gpui::{
    Bounds, ClipboardItem, Context, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Render, ScrollWheelEvent, Styled as _, Window, div, px,
};
use gpui_component::{ActiveTheme as _, ElementExt, v_flex, h_flex, button::{Button, ButtonVariants}};

use crate::{
    herdr, config::ConfigStore,
    terminal::{
        self, BackendCommand, BackendEvent, BackendTx, TerminalTab,
        encode_key, encode_mouse_event, encode_mouse_drag, encode_mouse_motion, encode_mouse_scroll,
    },
    terminal_element::TerminalElement,
};

const FONT_FAMILY: &str = "Noto Mono";
const MAX_CONNECT_ATTEMPTS: usize = 200;
const FONT_SIZE_MIN: f32 = 8.0;
const FONT_SIZE_MAX: f32 = 24.0;

pub struct DeepinHerdr {
    focus_handle: FocusHandle,
    backend: Option<BackendTx>,
    tab: Option<TerminalTab>,
    status: String,
    events_rx: Receiver<BackendEvent>,
    events_tx: Sender<BackendEvent>,
    launch_attempts: usize,
    connected: bool,
    connect_attempted: bool,
    connect_ready: bool,
    terminal_font_size: f32,
    terminal_selecting: bool,
    terminal_bounds: Option<Bounds<Pixels>>,
    config: ConfigStore,
    show_settings: bool,
    last_mouse_cell: Option<(usize, usize)>,
    last_selection_cell: Option<(usize, usize)>,
}

fn cell_width(font_size: f32) -> f32 { (font_size * 0.6).max(6.0) }
fn line_height(font_size: f32) -> f32 { (font_size * 1.3).max(font_size + 2.0) }

fn write_to_pty(backend: &Option<BackendTx>, tab: &mut Option<TerminalTab>, bytes: Vec<u8>) {
    if let Some(backend) = backend {
        if let Some(tab) = tab {
            if tab.render_snapshot().display_offset > 0 { tab.scroll_to_bottom(); }
        }
        backend.send(BackendCommand::Input(bytes));
    }
}

impl DeepinHerdr {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, config: ConfigStore) -> Self {
        let focus_handle = cx.focus_handle();
        let (events_tx, events_rx) = mpsc::channel();
        let font_size = config.terminal_font_size();
        Self {
            focus_handle, backend: None, tab: None,
            status: "initializing".into(),
            events_rx, events_tx,
            launch_attempts: 0, connected: false,
            connect_attempted: false, connect_ready: false,
            terminal_font_size: font_size,
            terminal_selecting: false,
            terminal_bounds: None,
            config, show_settings: false,
            last_mouse_cell: None, last_selection_cell: None,
        }
    }

    fn start_event_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(1)).await;
                if this.update(cx, |this, cx| {
                    if this.drain_pty() { cx.notify(); }
                }).is_err() { break; }
            }
        }).detach();
    }

    fn drain_pty(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events_rx.try_recv() {
            changed = true;
            match event {
                BackendEvent::Output { tab_id, bytes } => {
                    if let Some(tab) = &mut self.tab { if tab.id == tab_id { tab.feed(&bytes); } }
                }
                BackendEvent::TitleChanged { title, .. } => {
                    tracing::info!("terminal title: {title}");
                }
                BackendEvent::Closed { reason, .. } => {
                    tracing::warn!("pty closed: {reason}");
                    self.tab = None; self.connected = false;
                    self.connect_attempted = false;
                    self.launch_attempts = 0;
                    self.connect_ready = false;
                    self.status = format!("disconnected: {reason}");
                }
                _ => {}
            }
        }
        if self.connect_attempted && !self.connected && self.launch_attempts < MAX_CONNECT_ATTEMPTS {
            if self.launch_attempts == 0 { return changed; }
            let socket = herdr::herdr_socket_path();
            if herdr::is_socket_ready(&socket) {
                self.connect_ready = true;
                changed = true;
            } else {
                self.launch_attempts += 1;
                if self.launch_attempts % 20 == 0 {
                    self.status = format!("waiting (attempt {})...", self.launch_attempts);
                    changed = true;
                }
            }
        }
        if self.launch_attempts >= MAX_CONNECT_ATTEMPTS {
            self.status = "timed out".into();
            changed = true;
        }
        changed
    }

    fn spawn_pty(&mut self, program: &str, args: &[&str], cx: &mut Context<Self>) {
        unsafe { std::env::remove_var("HERDR_ENV"); std::env::remove_var("HERDR_PANE_ID"); std::env::remove_var("HERDR_SOCKET_PATH"); }
        let id = uuid::Uuid::new_v4().to_string();
        tracing::info!("spawning PTY: {program} {args:?}");
        let cols = 120u16;
        let rows = 40u16;
        match terminal::spawn_command_in_pty(id.clone(), program, args, cols, rows, self.events_tx.clone()) {
            Ok(backend) => {
                let mut tab = TerminalTab::new_local(id, "terminal".into(), backend.clone(), self.events_tx.clone());
                tab.resize(cols, rows);
                self.tab = Some(tab);
                self.backend = Some(backend);
                self.connected = true;
                self.status = "connected".into();
                cx.notify();
            }
            Err(err) => {
                tracing::error!("spawn_pty failed: {err}");
                self.status = format!("failed: {err}");
            }
        }
    }

    fn connect_herdr(&mut self, cx: &mut Context<Self>) {
        self.connect_attempted = true;
        let binary = match herdr::find_herdr_binary() {
            Some(p) => p.to_string_lossy().to_string(),
            None => { self.status = "herdr not found".into(); return; }
        };
        let socket = herdr::herdr_socket_path();
        if herdr::is_socket_ready(&socket) {
            tracing::info!("socket ready, spawning PTY");
            self.spawn_pty(&binary, &["client"], cx);
        } else {
            tracing::info!("socket not ready, starting server");
            if self.launch_attempts == 0 { herdr::start_herdr_server(); }
            self.launch_attempts = 1;
            self.status = format!("waiting (attempt {})...", 1);
        }
    }

    fn change_font_size(&mut self, size: f32) {
        self.terminal_font_size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.config.set_terminal_font_size(self.terminal_font_size);
        let _ = self.config.save();
    }

    // ── Keyboard ────────────────────────────────────────────────────────

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let key = &event.keystroke.key;

        if (modifiers.secondary() && key.eq_ignore_ascii_case(",")) || key.eq_ignore_ascii_case("f1") {
            self.show_settings = !self.show_settings;
            cx.notify();
            return;
        }
        if modifiers.secondary() && (key.eq_ignore_ascii_case("=") || key.eq_ignore_ascii_case("numpadadd")) {
            self.change_font_size(self.terminal_font_size + 1.0);
            cx.notify();
            return;
        }
        if modifiers.secondary() && (key.eq_ignore_ascii_case("-") || key.eq_ignore_ascii_case("numpadsubtract")) {
            self.change_font_size(self.terminal_font_size - 1.0);
            cx.notify();
            return;
        }
        if modifiers.secondary() && modifiers.shift && key.eq_ignore_ascii_case("c") {
            if let Some(text) = self.tab.as_ref().and_then(|t| t.selection_text()) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            return;
        }
        if modifiers.secondary() && modifiers.shift && key.eq_ignore_ascii_case("v") {
            if let Some(cb) = cx.read_from_clipboard().and_then(|c| c.text()) {
                write_to_pty(&self.backend, &mut self.tab, cb.as_bytes().to_vec());
            }
            return;
        }
        if event.prefer_character_input {
            if let Some(text) = event.keystroke.key_char.as_deref() {
                if !text.is_empty() { write_to_pty(&self.backend, &mut self.tab, text.as_bytes().to_vec()); }
            }
            return;
        }
        if let Some(tab) = &self.tab {
            if let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false) {
                write_to_pty(&self.backend, &mut self.tab, bytes);
            }
        }
    }

    // ── Mouse dispatch ──────────────────────────────────────────────────

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, true, cx);
        } else if event.button == MouseButton::Left {
            self.begin_terminal_selection(event, cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_selecting {
            if event.pressed_button == Some(MouseButton::Left) {
                if self.update_terminal_selection(event.position) { cx.notify(); }
            } else {
                self.terminal_selecting = false;
                self.last_selection_cell = None;
            }
            return;
        }
        if self.tab.as_ref().is_some_and(|t| t.mouse_motion_mode()) {
            if let Some(button) = event.pressed_button {
                self.send_mouse_drag_to_pty(event.position, button, cx);
            } else {
                self.send_mouse_motion_to_pty(event.position, cx);
            }
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_selecting {
            self.terminal_selecting = false;
            cx.notify();
            return;
        }
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, false, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let lh = line_height(self.terminal_font_size);
        let delta = match event.delta {
            gpui::ScrollDelta::Lines(p) => p.y.round() as i32,
            gpui::ScrollDelta::Pixels(p) => (f32::from(p.y) / lh).round() as i32,
        };
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            if let Some((row, col)) = self.terminal_grid_point(event.position) {
                let up = delta < 0;
                for _ in 0..delta.unsigned_abs() {
                    write_to_pty(&self.backend, &mut self.tab,
                        encode_mouse_scroll(row as u16, col as u16, up, false, false, false));
                }
                cx.notify();
            }
        } else if let Some(tab) = &mut self.tab {
            tab.scroll_history(delta);
        }
    }

    // ── Mouse → PTY helpers ─────────────────────────────────────────────

    /// Regular button press/release (forwarded verbatim to PTY).
    fn send_mouse_to_pty(&mut self, position: Point<Pixels>, button: MouseButton, pressed: bool, cx: &mut Context<Self>) {
        let Some((row, col)) = self.terminal_grid_point(position) else { return; };
        let btn = match button {
            MouseButton::Left => 0, MouseButton::Middle => 1, MouseButton::Right => 2,
            _ => return,
        };
        write_to_pty(&self.backend, &mut self.tab,
            encode_mouse_event(row as u16, col as u16, btn, pressed, false, false, false));
        cx.notify();
    }

    /// Drag event (button held during motion mode → SGR cb = 32 | btn_code).
    fn send_mouse_drag_to_pty(&mut self, position: Point<Pixels>, button: MouseButton, cx: &mut Context<Self>) {
        let Some(cell) = self.terminal_grid_point(position) else { return; };
        if self.last_mouse_cell == Some(cell) { return; }
        self.last_mouse_cell = Some(cell);
        let btn = match button {
            MouseButton::Left => 0, MouseButton::Middle => 1, MouseButton::Right => 2,
            _ => return,
        };
        write_to_pty(&self.backend, &mut self.tab,
            encode_mouse_drag(cell.0 as u16, cell.1 as u16, btn, false, false, false));
        cx.notify();
    }

    /// Pure motion event (no button → SGR cb = 35 = motion | release).
    fn send_mouse_motion_to_pty(&mut self, position: Point<Pixels>, _cx: &mut Context<Self>) {
        let Some(cell) = self.terminal_grid_point(position) else { return; };
        if self.last_mouse_cell == Some(cell) { return; }
        self.last_mouse_cell = Some(cell);
        write_to_pty(&self.backend, &mut self.tab,
            encode_mouse_motion(cell.0 as u16, cell.1 as u16, false, false, false));
    }

    // ── Coordinate helpers ──────────────────────────────────────────────

    fn terminal_grid_point(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let bounds = self.terminal_bounds?;
        if !bounds.contains(&position) { return None; }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cw = px(cell_width(self.terminal_font_size));
        let lh = px(line_height(self.terminal_font_size));
        let snap = self.tab.as_ref()?.render_snapshot();
        Some((((local_y / lh).floor() as usize).min(snap.rows.saturating_sub(1)),
              ((local_x / cw).floor() as usize).min(snap.cols.saturating_sub(1))))
    }

    fn terminal_grid_point_and_side(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let bounds = self.terminal_bounds?;
        if !bounds.contains(&position) { return None; }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cw = px(cell_width(self.terminal_font_size));
        let lh = px(line_height(self.terminal_font_size));
        let snap = self.tab.as_ref()?.render_snapshot();
        let col = ((local_x / cw).floor() as usize).min(snap.cols.saturating_sub(1));
        let row = ((local_y / lh).floor() as usize).min(snap.rows.saturating_sub(1));
        let side = if px(local_x.as_f32() % cw.as_f32()) >= (cw / 2.) { Side::Right } else { Side::Left };
        Some((row, col, side))
    }

    // ── Text selection ──────────────────────────────────────────────────

    fn begin_terminal_selection(&mut self, event: &MouseDownEvent, _cx: &mut Context<Self>) {
        let stype = match event.click_count.max(1) {
            1 => SelectionType::Simple, 2 => SelectionType::Semantic,
            3 => SelectionType::Lines, _ => SelectionType::Simple,
        };
        let Some((row, col, side)) = self.terminal_grid_point_and_side(event.position) else { return; };
        if let Some(tab) = &mut self.tab {
            tab.begin_selection(row, col, side, stype);
            self.terminal_selecting = true;
            self.last_selection_cell = None;
        }
    }

    fn update_terminal_selection(&mut self, position: Point<Pixels>) -> bool {
        let Some((row, col, side)) = self.terminal_grid_point_and_side(position) else { return false; };
        let cell = (row, col);
        if self.last_selection_cell == Some(cell) { return false; }
        self.last_selection_cell = Some(cell);
        if let Some(tab) = &mut self.tab { tab.update_selection(row, col, side); }
        true
    }
}

impl Render for DeepinHerdr {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.connected && !self.connect_attempted {
            self.start_event_pump(cx);
            self.connect_herdr(cx);
        }
        if self.connect_ready {
            self.connect_ready = false;
            if let Some(binary) = herdr::find_herdr_binary().map(|p| p.to_string_lossy().to_string()) {
                self.spawn_pty(&binary, &["client"], cx);
            }
        }

        let fs = self.terminal_font_size;
        let cw_px = px(cell_width(fs));
        let lh_px = px(line_height(fs));
        if let Some(tab) = &mut self.tab {
            let b = window.bounds();
            let cols = (f32::from(b.size.width) / cell_width(fs)).max(20.0) as u16;
            let rows = (f32::from(b.size.height) / line_height(fs)).max(10.0) as u16;
            if tab.cols != cols || tab.rows != rows { tab.resize(cols, rows); }
        }

        let settings_overlay: Option<gpui::AnyElement> = if self.show_settings {
            let t = cx.theme().clone();
            Some(settings_panel(t, self.terminal_font_size, cx).into_any_element())
        } else { None };

        let theme = cx.theme().clone();
        let snapshot = self.tab.as_ref().map(|t| t.render_snapshot());
        let backend = self.backend.clone();
        let focus = self.focus_handle.clone();

        div()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(FONT_FAMILY)
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(match (snapshot, backend) {
                (Some(snapshot), Some(backend)) => div()
                    .size_full().relative()
                    .on_prepaint({
                        let view = cx.entity().clone();
                        move |bounds, _window, cx| {
                            let _ = view.update(cx, |this, _| { this.terminal_bounds = Some(bounds); });
                        }
                    })
                    .child(TerminalElement::new(snapshot, backend, focus, FONT_FAMILY, px(fs), lh_px, cw_px))
                    .into_any_element(),
                _ => v_flex()
                    .size_full().items_center().justify_center().gap_6()
                    .child(div().text_size(px(24.)).child("deepin-herdr"))
                    .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(self.status.clone()))
                    .child(Button::new("term").primary().label("Terminal")
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                            this.spawn_pty("bash", &[], cx); cx.notify();
                        })))
                    .child(Button::new("herdr").ghost().label("Launch herdr")
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                            this.launch_attempts = 0; this.connect_attempted = false;
                            this.connect_herdr(cx); cx.notify();
                        })))
                    .into_any_element(),
            })
            .children(settings_overlay)
    }
}

fn settings_panel(theme: gpui_component::Theme, font_size: f32, cx: &mut Context<DeepinHerdr>) -> impl IntoElement {
    let bg = if theme.is_dark() { gpui::rgba(0x18181BFF) } else { gpui::rgba(0xFAFAFAFF) };
    div()
        .absolute().top(px(60.)).right(px(16.)).w(px(280.))
        .bg(bg).border_1().border_color(theme.border).rounded_lg().shadow_md().p_4()
        .child(v_flex().gap_4()
            .child(div().text_size(px(15.)).font_weight(gpui::FontWeight::BOLD).child("Settings"))
            .child(v_flex().gap_2()
                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Font size"))
                .child(h_flex().gap_2().items_center()
                    .child(Button::new("f-dn").ghost().label("\u{2212}").on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                        this.change_font_size(this.terminal_font_size - 1.0); cx.notify();
                    })))
                    .child(div().text_size(px(24.)).child(format!("{:.0}px", font_size)))
                    .child(Button::new("f-up").ghost().label("+").on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                        this.change_font_size(this.terminal_font_size + 1.0); cx.notify();
                    })))))
            .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child("F1 / Ctrl+,  :  settings"))
            .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child("Ctrl+= / Ctrl+minus  :  zoom"))
            .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child("Ctrl+Shift+C/V  :  copy/paste"))
        )
}
