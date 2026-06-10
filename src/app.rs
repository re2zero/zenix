//! Single-terminal app — auto-connects to herdr on startup.

use std::{sync::mpsc::{self, Receiver, Sender}, time::Duration};

use gpui::{
    ClipboardItem, Context, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Render, ScrollWheelEvent, Styled as _, Window, div, px,
};
use gpui_component::{ActiveTheme as _, v_flex, button::{Button, ButtonVariants}};

use crate::{
    herdr,
    terminal::{self, BackendCommand, BackendEvent, BackendTx, TerminalTab, encode_key},
    terminal_element::TerminalElement,
};

const FONT_SIZE: f32 = 14.0;
const FONT_FAMILY: &str = "Noto Mono";
const MAX_CONNECT_ATTEMPTS: usize = 200;

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
}

fn cell_width() -> f32 { FONT_SIZE * 0.6 }
fn line_height() -> f32 { FONT_SIZE * 1.3 }

fn send(backend: &Option<BackendTx>, tab: &mut Option<TerminalTab>, bytes: Vec<u8>) {
    if let Some(backend) = backend {
        if let Some(tab) = tab {
            if tab.render_snapshot().display_offset > 0 { tab.scroll_to_bottom(); }
            tab.clear_selection();
        }
        backend.send(BackendCommand::Input(bytes));
    }
}

impl DeepinHerdr {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            focus_handle, backend: None, tab: None,
            status: "initializing".into(),
            events_rx, events_tx,
            launch_attempts: 0, connected: false,
            connect_attempted: false, connect_ready: false,
        }
    }

    fn start_event_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(50)).await;
                if this.update(cx, |this, cx| { this.drain_pty(); cx.notify(); }).is_err() { break; }
            }
        }).detach();
    }

    fn drain_pty(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                BackendEvent::Output { tab_id, bytes } => {
                    if let Some(tab) = &mut self.tab { if tab.id == tab_id { tab.feed(&bytes); } }
                }
                BackendEvent::Closed { reason, .. } => {
                    tracing::warn!("pty closed: {}", reason);
                    self.tab = None; self.connected = false;
                    self.connect_attempted = false; // allow reconnect
                    self.launch_attempts = 0;
                    self.connect_ready = false;
                    self.status = format!("disconnected: {reason}");
                }
                _ => {}
            }
        }
        if self.connect_attempted && !self.connected && self.launch_attempts < MAX_CONNECT_ATTEMPTS {
            if self.launch_attempts == 0 { return; }
            let socket = herdr::herdr_socket_path();
            if herdr::is_socket_ready(&socket) {
                self.connect_ready = true;
            } else {
                self.launch_attempts += 1;
                if self.launch_attempts % 20 == 0 {
                    self.status = format!("waiting (attempt {})...", self.launch_attempts);
                }
            }
        }
        if self.launch_attempts >= MAX_CONNECT_ATTEMPTS {
            self.status = "timed out".into();
        }
    }

    fn spawn_pty(&mut self, program: &str, args: &[&str], cx: &mut Context<Self>) {
        // Clear herdr env vars so client connects as a fresh terminal, not a nested pane
        unsafe { std::env::remove_var("HERDR_ENV"); std::env::remove_var("HERDR_PANE_ID"); std::env::remove_var("HERDR_SOCKET_PATH"); }

        let id = uuid::Uuid::new_v4().to_string();
        tracing::info!("spawning PTY: {} {:?}", program, args);
        match terminal::spawn_command_in_pty(id.clone(), program, args, 100, 30, self.events_tx.clone()) {
            Ok(backend) => {
                let mut tab = TerminalTab::new_local(id, "terminal".into(), backend.clone());
                tab.resize(100, 30);
                self.tab = Some(tab);
                self.backend = Some(backend);
                self.connected = true;
                self.status = "connected".into();
                cx.notify();
            }
            Err(err) => {
                tracing::error!("spawn_pty failed: {}", err);
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

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.secondary() && event.keystroke.key.eq_ignore_ascii_case("c") {
            if let Some(text) = self.tab.as_ref().and_then(|t| t.selection_text()) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            return;
        }
        if event.keystroke.modifiers.secondary() && event.keystroke.key.eq_ignore_ascii_case("v") {
            if let Some(cb) = cx.read_from_clipboard().and_then(|c| c.text()) {
                send(&self.backend, &mut self.tab, cb.as_bytes().to_vec());
            }
            return;
        }
        if event.prefer_character_input {
            if let Some(text) = event.keystroke.key_char.as_deref() {
                if !text.is_empty() { send(&self.backend, &mut self.tab, text.as_bytes().to_vec()); }
            }
            return;
        }
        if let Some(tab) = &self.tab {
            if let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false) {
                send(&self.backend, &mut self.tab, bytes);
            }
        }
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }
    fn on_mouse_move(&mut self, _: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {}
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {}
    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, _: &mut Context<Self>) {
        let delta = match event.delta {
            gpui::ScrollDelta::Lines(p) => p.y.round() as i32,
            gpui::ScrollDelta::Pixels(p) => (f32::from(p.y) / line_height()).round() as i32,
        };
        if let Some(tab) = &mut self.tab { tab.scroll_history(delta); }
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
            let binary = herdr::find_herdr_binary().map(|p| p.to_string_lossy().to_string());
            if let Some(binary) = binary { self.spawn_pty(&binary, &["client"], cx); }
        }

        let cw_px = px(cell_width());
        let lh_px = px(line_height());
        if let Some(tab) = &mut self.tab {
            let b = window.bounds();
            let cols = (f32::from(b.size.width) / cell_width()).max(20.0) as u16;
            let rows = (f32::from(b.size.height) / line_height()).max(10.0) as u16;
            if tab.cols != cols || tab.rows != rows { tab.resize(cols, rows); }
        }

        let theme = cx.theme();
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
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(match (snapshot, backend) {
                (Some(snapshot), Some(backend)) =>
                    TerminalElement::new(snapshot, backend, focus, FONT_FAMILY, px(FONT_SIZE), lh_px, cw_px)
                        .into_any_element(),
                _ => v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_6()
                    .child(div().text_size(px(24.)).child("deepin-herdr"))
                    .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(self.status.clone()))
                    .child(
                        Button::new("launch-terminal")
                            .primary()
                            .label("Terminal")
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                                this.spawn_pty("bash", &[], cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("launch-herdr")
                            .ghost()
                            .label("Launch herdr")
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                                this.launch_attempts = 0;
                                this.connect_attempted = false;
                                this.connect_herdr(cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            })
    }
}
