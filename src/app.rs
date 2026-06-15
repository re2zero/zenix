//! Single-terminal app — auto-connects to herdr on startup.

use std::{ops::Range, sync::mpsc::{self, Receiver, Sender}, time::Duration};

use alacritty_terminal::{index::Side, selection::SelectionType};
use gpui::{
    AppContext, Bounds, ClipboardItem, ClickEvent, Context, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Render, ScrollWheelEvent, Styled as _, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, ElementExt, Theme, ThemeRegistry,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::{
    config::ConfigStore,
    client as herdr,
    ui::{self as sidebar, Panel},
    ui::settings::{ActionCb, SkillAction, McpAction, CliAction, McpFormState, InputDialogMode, SKILL_AGENTS},
    sys::{self as system_info, CpuSamples, SystemInfo},
    terminal::{
        self, BackendCommand, BackendEvent, BackendTx, ImeState, TerminalTab, TerminalElement,
        encode_key, encode_mouse_drag, encode_mouse_event, encode_mouse_motion, encode_mouse_scroll,
    },
};
use crate::i18n::t;

const FONT_FAMILY: &str = "Lilex";
const MAX_CONNECT_ATTEMPTS: usize = 200;
const FONT_SIZE_MIN: f32 = 8.0;
const FONT_SIZE_MAX: f32 = 24.0;

pub struct ZenixApp {
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
    active_panel: Panel,
    system_info: SystemInfo,
    sysinfo_cpu_sample: CpuSamples,
    show_settings: bool,
    active_settings_tab: usize,
    agent_statuses: Vec<crate::agent::AgentCliInfo>,
    mcp_zenix_servers: Vec<crate::mcp::ZenixMcpServer>,
    mcp_per_agent: Vec<(String, Vec<crate::mcp::McpServerEntry>)>,
    skills: std::collections::HashMap<String, crate::skills::SkillInfo>,
    mcp_form: Option<McpFormState>,
    mcp_available_agents: Vec<String>,
    dialog_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    dialog_input_mode: InputDialogMode,
    sysinfo_polling: bool,
    terminal_cell_width: Pixels,
    terminal_line_height: Pixels,
    terminal_ascent: Pixels,
    terminal_descent: Pixels,
    last_mouse_cell: Option<(usize, usize)>,
    last_selection_cell: Option<(usize, usize)>,
    ime_state: Option<ImeState>,
}
fn write_to_pty(backend: &Option<BackendTx>, tab: &mut Option<TerminalTab>, bytes: Vec<u8>) {
    if let Some(backend) = backend {
        if let Some(tab) = tab
            && tab.render_snapshot().display_offset > 0
        {
            tab.scroll_to_bottom();
        }
        backend.send(BackendCommand::Input(bytes));
    }
}

impl ZenixApp {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, config: ConfigStore) -> Self {
        let focus_handle = cx.focus_handle();
        let (events_tx, events_rx) = mpsc::channel();
        let font_size = config.terminal_font_size();
        Self {
            focus_handle, backend: None, tab: None,
            status: t("status.initializing"),
            events_rx, events_tx,
            launch_attempts: 0, connected: false,
            connect_attempted: false, connect_ready: false,
            terminal_font_size: font_size,
            terminal_selecting: false,
            terminal_bounds: None,
            config,
            active_panel: Panel::None,
            system_info: SystemInfo::default(),
            sysinfo_cpu_sample: CpuSamples::default(),
            show_settings: false,
            active_settings_tab: 0,
            agent_statuses: Vec::new(),
            mcp_zenix_servers: Vec::new(),
            mcp_per_agent: Vec::new(),
            skills: std::collections::HashMap::new(),
            mcp_form: None,
            mcp_available_agents: crate::mcp::claude_format_agent_names(),
            dialog_input: None,
            dialog_input_mode: InputDialogMode::Hidden,
            sysinfo_polling: false,
            terminal_cell_width: px(14.0 * 0.6),
            terminal_line_height: px(14.0 * 1.15),
            terminal_ascent: px(14.0 * 0.8),
            terminal_descent: px(14.0 * 0.2),
            last_mouse_cell: None, last_selection_cell: None,
            ime_state: None,
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
                    if let Some(tab) = &mut self.tab && tab.id == tab_id { tab.feed(&bytes); }
                }
                BackendEvent::TitleChanged { title, .. } => {
                    tracing::info!("terminal title: {title}");
                }
                BackendEvent::Closed { reason, .. } => {
                    tracing::warn!("pty closed: {reason}");
                    self.tab = None; self.connected = false;
                    self.connect_attempted = false;
                    self.launch_attempts = 0;
                    self.status = t("status.disconnected").replace("%{reason}", &reason);
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
                self.status = t("status.waiting").replace("%{n}", &self.launch_attempts.to_string());
                changed = true;
            }
        }
    if self.launch_attempts >= MAX_CONNECT_ATTEMPTS {
        self.status = t("status.timed_out");
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
                self.status = t("status.connected");
                cx.notify();
            }
            Err(err) => {
                tracing::error!("spawn_pty failed: {err}");
                self.status = t("status.failed").replace("%{err}", &err.to_string());
            }
        }
    }

    fn connect_herdr(&mut self, cx: &mut Context<Self>) {
        self.connect_attempted = true;
        let binary = match herdr::find_herdr_binary() {
            Some(p) => p.to_string_lossy().to_string(),
            None => { self.status = t("status.not_found"); return; }
        };
        let socket = herdr::herdr_socket_path();
        if herdr::is_socket_ready(&socket) {
            tracing::info!("socket ready, spawning PTY");
            self.spawn_pty(&binary, &["client"], cx);
        } else {
            tracing::info!("socket not ready, starting server");
            if self.launch_attempts == 0 { herdr::start_herdr_server(); }
            self.launch_attempts = 1;
            self.status = t("status.waiting").replace("%{n}", "1");
        }
    }

    fn change_font_size(&mut self, size: f32) {
        self.terminal_font_size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.config.set_terminal_font_size(self.terminal_font_size);
        let _ = self.config.save();
    }

    fn toggle_panel(&mut self, panel: Panel) {
        self.active_panel = if self.active_panel == panel { Panel::None } else { panel };
    }

    fn switch_theme(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Clone the config first to release the immutable borrow on cx
        let config = ThemeRegistry::global(cx).themes().get(name).cloned();
        if let Some(config) = config {
            let theme = Theme::global_mut(cx);
            theme.apply_config(&config);
            // Force embedded Lilex font — theme configs may override font_family.
            theme.font_family = "Lilex".into();
            window.refresh();
            self.config.set_theme_name(name.to_string());
            if let Err(e) = self.config.save() {
                eprintln!("failed to save config: {e:#}");
            }
        }
    }


    // ── IME (Input Method Editor) support ────────────────────────────────

    /// Sets the marked (pre-edit) text from the IME composition.
    pub(crate) fn set_marked_text(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() {
            self.clear_marked_text(cx);
            return;
        }
        self.ime_state = Some(ImeState { marked_text: text });
        cx.notify();
    }

    /// Returns the current marked text range (UTF-16) for IME cursor placement.
    pub(crate) fn marked_text_range(&self) -> Option<Range<usize>> {
        self.ime_state
            .as_ref()
            .map(|state| 0..state.marked_text.encode_utf16().count())
    }

    /// Clears the marked text state, ending IME composition.
    pub(crate) fn clear_marked_text(&mut self, cx: &mut Context<Self>) {
        if self.ime_state.is_some() {
            self.ime_state = None;
            cx.notify();
        }
    }

    /// Commits finalized text from the IME to the PTY.
    pub(crate) fn commit_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if !text.is_empty() {
            write_to_pty(&self.backend, &mut self.tab, text.as_bytes().to_vec());
            cx.notify();
        }
    }

    fn start_sysinfo_poll(&mut self, cx: &mut Context<Self>) {
        if self.sysinfo_polling { return; }
        self.sysinfo_polling = true;
        cx.spawn(async move |this, cx| {
            let mut samples = CpuSamples::default();
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let mut info = SystemInfo::default();
                let new_samples = system_info::collect(&mut info, &samples);
                samples = new_samples;
                if this.update(cx, |this, cx| {
                    this.system_info = info;
                    this.sysinfo_cpu_sample = samples.clone();
                    cx.notify();
                }).is_err() { break; }
            }
        }).detach();
    }
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let key = &event.keystroke.key;

        // Escape closes settings dialog
        if self.show_settings && key.eq_ignore_ascii_case("escape") {
            self.show_settings = false;
            cx.notify();
            return;
        }
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

        // All printable characters without ctrl/alt/platform modifiers are handled by
        // the InputHandler's replace_text_in_range. We must NOT send
        // them here to avoid double input (including shift+letter).
        if (event.keystroke.key.len() == 1 || event.keystroke.key.eq_ignore_ascii_case("space"))
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.platform
        {
            return;
        }

        // Terminal keystrokes (named keys, modified keys): encode and send.
        if let Some(tab) = &self.tab
            && let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false)
        {
            write_to_pty(&self.backend, &mut self.tab, bytes);
        }
    }
    // ── Mouse dispatch ──────────────────────────────────────────────────

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_settings { return; }
        window.focus(&self.focus_handle, cx);
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, true, cx);
        } else if event.button == MouseButton::Left {
            self.begin_terminal_selection(event, cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.show_settings { return; }
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
        if self.show_settings { return; }
        if self.terminal_selecting {
            self.terminal_selecting = false;
            cx.notify();
            return;
        }
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, false, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if self.show_settings { return; }
        let lh = f32::from(self.terminal_line_height);
        let delta = match event.delta {
            gpui::ScrollDelta::Lines(lines) => px(lines.y * lh),
            gpui::ScrollDelta::Pixels(px) => px.y,
        };
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            if let Some((row, col)) = self.terminal_grid_point(event.position) {
                let up = delta > px(0.0);
                let steps = (f32::from(delta).abs() / lh).ceil() as u32;
                for _ in 0..steps {
                    write_to_pty(&self.backend, &mut self.tab,
                        encode_mouse_scroll(row as u16, col as u16, up, false, false, false));
                }
            }
        } else if let Some(tab) = &mut self.tab {
            tab.scroll_history(f32::from(delta) as i32);
        }
    }

    // ── Mouse → PTY helpers ─────────────────────────────────────────────
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
        let cw = self.terminal_cell_width;
        let lh = self.terminal_line_height;
        let snap = self.tab.as_ref()?.render_snapshot();
        let row = ((f32::from(local_y) / f32::from(lh)).floor() as usize).min(snap.rows.saturating_sub(1));
        let col = ((f32::from(local_x) / f32::from(cw)).floor() as usize).min(snap.cols.saturating_sub(1));
        Some((row, col))
    }

    fn terminal_grid_point_and_side(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let bounds = self.terminal_bounds?;
        if !bounds.contains(&position) { return None; }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cw = self.terminal_cell_width;
        let lh = self.terminal_line_height;
        let snap = self.tab.as_ref()?.render_snapshot();
        let col = ((f32::from(local_x) / f32::from(cw)).floor() as usize).min(snap.cols.saturating_sub(1));
        let row = ((f32::from(local_y) / f32::from(lh)).floor() as usize).min(snap.rows.saturating_sub(1));
        let side = if f32::from(local_x) % f32::from(cw) >= f32::from(cw) / 2.0 { Side::Right } else { Side::Left };
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

    fn refresh_mcp_data(&mut self, cx: &mut gpui::Context<Self>) {
        self.mcp_zenix_servers = crate::mcp::load_zenix_mcp_servers();
        self.mcp_per_agent = crate::mcp::scan_per_agent_mcp();
        cx.notify();
    }
}

impl Render for ZenixApp {
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

        // Start sysinfo polling when system info panel is first opened
        if self.active_panel == Panel::SystemInfo && !self.sysinfo_polling {
            self.start_sysinfo_poll(cx);
        }

        let fs = self.terminal_font_size;
        let fs_px = px(fs);

        // Cell metrics from font, line height = Zed Standard (1.3x)
        let text_system = window.text_system();
        let font = gpui::Font { family: FONT_FAMILY.into(), ..gpui::Font::default() };
        let font_id = text_system.resolve_font(&font);
        let cw_px = text_system.advance(font_id, fs_px, 'm').map(|s| s.width).unwrap_or(fs_px * 0.6);
        let ascent = text_system.ascent(font_id, fs_px);
        let descent = text_system.descent(font_id, fs_px);
        // Snap line height to device pixels (matches Zed's approach)
        let sf = window.scale_factor();
        let lh_device = (fs * 1.3 * sf).round().max(1.0);
        let lh_px = px(lh_device / sf);
        self.terminal_cell_width = cw_px;
        self.terminal_line_height = lh_px;
        self.terminal_ascent = ascent;
        self.terminal_descent = descent;
        // Calculate available width for terminal (account for sidebar + panel)
        let sidebar_w = sidebar::SIDEBAR_WIDTH
            + if self.active_panel != Panel::None { sidebar::PANEL_WIDTH } else { 0.0 };

        if let Some(tab) = &mut self.tab {
            let b = window.bounds();
            let avail_w = (f32::from(b.size.width) - sidebar_w).max(200.0);
            let cw = f32::from(cw_px);
            let lh = f32::from(lh_px);
            let cols = (avail_w / cw.max(1.0)).max(20.0) as u16;
            let rows = (f32::from(b.size.height) / lh.max(1.0)).max(10.0) as u16;
            if tab.cols != cols || tab.rows != rows { tab.resize(cols, rows); }
        }

        let theme = cx.theme().clone();
        let snapshot = self.tab.as_ref().map(|t| t.render_snapshot());
        let focus = self.focus_handle.clone();
        let focus2 = self.focus_handle.clone();
        let backend = self.backend.clone();

        // Build theme entries as pre-built button elements (avoids closure capture issues)
        let current_theme_name = theme.theme_name().to_string();
        let registry = ThemeRegistry::global(cx);
        let theme_buttons: Vec<gpui::AnyElement> = registry
            .sorted_themes()
            .iter()
            .map(|tc| {
                let name = tc.name.to_string();
                let is_current = name == current_theme_name;
                let prefix = if is_current { "\u{2713} " } else { "  " };
                let cb: sidebar::ThemeCallback = {
                    let name = name.clone();
                    Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.switch_theme(&name, window, cx);
                    }))
                };
                Button::new(format!("th-{name}"))
                    .ghost()
                    .label(format!("{prefix}{name}"))
                    .w_full()
                    .on_click(cb)
                    .into_any_element()
            })
            .collect();
        // Split into light/dark groups for 2-column layout
        let _light_themes: Vec<gpui::AnyElement> = registry.sorted_themes().iter().filter(|tc| tc.mode == gpui_component::ThemeMode::Light).map(|tc| {
            let name = tc.name.to_string();
            let is_current = name == current_theme_name;
            let prefix = if is_current { "\u{2713} " } else { "  " };
            let cb: sidebar::ThemeCallback = { let n = name.clone(); Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| { this.switch_theme(&n, window, cx); })) };
            Button::new(format!("th-{name}")).ghost().label(format!("{prefix}{name}")).w_full().on_click(cb).into_any_element()
        }).collect();
        let _dark_themes: Vec<gpui::AnyElement> = registry.sorted_themes().iter().filter(|tc| tc.mode == gpui_component::ThemeMode::Dark).map(|tc| {
            let name = tc.name.to_string();
            let is_current = name == current_theme_name;
            let prefix = if is_current { "\u{2713} " } else { "  " };
            let cb: sidebar::ThemeCallback = { let n = name.clone(); Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| { this.switch_theme(&n, window, cx); })) };
            Button::new(format!("th-{name}")).ghost().label(format!("{prefix}{name}")).w_full().on_click(cb).into_any_element()
        }).collect();

        // Callbacks for sidebar buttons (Box<dyn Fn> works with .on_click)
        let sysinfo_cb: sidebar::ThemeCallback = Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
            this.toggle_panel(Panel::SystemInfo);
            cx.notify();
        }));
        let settings_cb: sidebar::ThemeCallback = Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
            // Lazy-init on first open
            if this.agent_statuses.is_empty() {
                this.agent_statuses = crate::agent::detect_all_agents();
            }
            if this.mcp_zenix_servers.is_empty() {
                this.mcp_zenix_servers = crate::mcp::load_zenix_mcp_servers();
                this.mcp_per_agent = crate::mcp::scan_per_agent_mcp();
            }
            if this.skills.is_empty() {
                this.skills = crate::skills::scan_all_skills();
            }
            this.show_settings = !this.show_settings;
            cx.notify();
        }));
        let font_down_cb: sidebar::ThemeCallback = Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
            this.change_font_size(this.terminal_font_size - 1.0);
            cx.notify();
        }));
        let font_up_cb: sidebar::ThemeCallback = Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
            this.change_font_size(this.terminal_font_size + 1.0);
            cx.notify();
        }));
        // Expanded panel (conditionally shown) — only SystemInfo now, Settings is a dialog
        let panel: Option<gpui::AnyElement> = match self.active_panel {
            Panel::SystemInfo => Some(sidebar::system_info_panel(
                &theme, &self.system_info,
            ).into_any_element()),
            _ => None,
        };

        // Terminal area content
        let terminal_area = match (snapshot, backend) {
            (Some(snapshot), Some(_backend)) => {
                let app_entity = cx.entity().clone();
                let ime = self.ime_state.clone();
                div()
                .size_full().relative()
                .on_prepaint({
                    let view = cx.entity().clone();
                    move |bounds, _window, cx| {
                        view.update(cx, |this, _| { this.terminal_bounds = Some(bounds); });
                    }
                })
                .child(TerminalElement::new(snapshot, app_entity, focus2, FONT_FAMILY, px(fs), lh_px, cw_px, self.terminal_ascent, self.terminal_descent, ime))
                .into_any_element()
            },
            _ => v_flex()
                .size_full().items_center().justify_center().gap_6()
                .child(div().text_size(px(24.)).child(t("landing.title")))
                .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(self.status.clone()))
                .child(Button::new("term").primary().label(t("landing.terminal_btn"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.spawn_pty(crate::platform::default_shell(), &[], cx); cx.notify();
                    })))
                .child(Button::new("herdr").ghost().label(t("landing.herdr_btn"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.launch_attempts = 0; this.connect_attempted = false;
                        this.connect_herdr(cx); cx.notify();
                    })))
                .into_any_element(),
        };
        let main_ui = h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(FONT_FAMILY)
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(div().flex_grow(1.0).h_full().child(terminal_area))
            .children(panel)
            .child(sidebar::sidebar(&theme, self.active_panel, self.show_settings, sysinfo_cb, settings_cb));



        if self.show_settings {
            let bounds = window.bounds();
            let w = bounds.size.width;
            let h = bounds.size.height;
            let locale = crate::i18n::locale().to_string();
            let tab = self.active_settings_tab;
            let cur_theme = current_theme_name.clone();
            let tbs = theme_buttons;
            let font_down = font_down_cb;
            let font_up = font_up_cb;
            let agents_ref = &self.agent_statuses;
            let mcp_ref = &self.mcp_zenix_servers;
            let mcp_per_agent_ref = &self.mcp_per_agent;
            let mcp_form_ref = self.mcp_form.as_ref();
            let mcp_agents_ref = &self.mcp_available_agents;
            let skills_ref = &self.skills;
            let entity = cx.entity().clone();

            let on_tab_change: ActionCb<usize> = Box::new({
                let entity = entity.clone();
                move |idx: usize, _w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        this.active_settings_tab = idx;
                        cx.notify();
                    });
                }
            });

            let on_locale_change: ActionCb<String> = Box::new({
                let entity = entity.clone();
                move |new_locale: String, _w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        crate::i18n::set_locale(&new_locale);
                        this.config.set_locale(new_locale);
                        let _ = this.config.save();
                        cx.notify();
                    });
                }
            });

            let on_skill_action: ActionCb<SkillAction> = Box::new({
                let entity = entity.clone();
                move |action: SkillAction, w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        match action {
                            SkillAction::Refresh => {
                                this.skills = crate::skills::scan_all_skills();
                                cx.notify();
                            }
                            SkillAction::Link { skill_name } => {
                                // Link to all supported agents
                                for agent in SKILL_AGENTS {
                                    let _ = crate::skills::link_skill(&skill_name, agent);
                                }
                                this.skills = crate::skills::scan_all_skills();
                                cx.notify();
                            }
                            SkillAction::Unlink { skill_name } => {
                                // Unlink from all agents
                                for agent in SKILL_AGENTS {
                                    let _ = crate::skills::unlink_skill(&skill_name, agent);
                                }
                                this.skills = crate::skills::scan_all_skills();
                                cx.notify();
                            }
                            SkillAction::InstallGit { url } => {
                                if url.is_empty() {
                                    let input = cx.new(|cx| {
                                        gpui_component::input::InputState::new(w, cx)
                                            .placeholder("https://github.com/user/repo")
                                    });
                                    cx.subscribe(&input, |this, _input, event: &gpui_component::input::InputEvent, cx| {
                                        if let gpui_component::input::InputEvent::PressEnter { shift: false, .. } = event {
                                            let url = _input.read(cx).value().to_string();
                                            if !url.trim().is_empty() {
                                                match crate::skills::install_from_git(url.trim()) {
                                                    Ok(name) => tracing::info!("installed skill: {name}"),
                                                    Err(e) => tracing::warn!("install failed: {e}"),
                                                }
                                                this.skills = crate::skills::scan_all_skills();
                                            }
                                            this.dialog_input = None;
                                            this.dialog_input_mode = InputDialogMode::Hidden;
                                            cx.notify();
                                        }
                                    }).detach();
                                    this.dialog_input = Some(input);
                                    this.dialog_input_mode = InputDialogMode::SkillAddGit;
                                    cx.notify();
                                } else {
                                    match crate::skills::install_from_git(&url) {
                                        Ok(name) => tracing::info!("installed skill: {name}"),
                                        Err(e) => tracing::warn!("install failed: {e}"),
                                    }
                                    this.skills = crate::skills::scan_all_skills();
                                    cx.notify();
                                }
                            }
                            SkillAction::InstallLocal { path } => {
                                if path.is_empty() {
                                    let input = cx.new(|cx| {
                                        gpui_component::input::InputState::new(w, cx)
                                            .placeholder("/path/to/skill")
                                    });
                                    cx.subscribe(&input, |this, _input, event: &gpui_component::input::InputEvent, cx| {
                                        if let gpui_component::input::InputEvent::PressEnter { shift: false, .. } = event {
                                            let path = _input.read(cx).value().to_string();
                                            if !path.trim().is_empty() {
                                                match crate::skills::install_from_local(path.trim()) {
                                                    Ok(name) => tracing::info!("installed skill: {name}"),
                                                    Err(e) => tracing::warn!("install failed: {e}"),
                                                }
                                                this.skills = crate::skills::scan_all_skills();
                                            }
                                            this.dialog_input = None;
                                            this.dialog_input_mode = InputDialogMode::Hidden;
                                            cx.notify();
                                        }
                                    }).detach();
                                    this.dialog_input = Some(input);
                                    this.dialog_input_mode = InputDialogMode::SkillAddLocal;
                                    cx.notify();
                                } else {
                                    match crate::skills::install_from_local(&path) {
                                        Ok(name) => tracing::info!("installed skill: {name}"),
                                        Err(e) => tracing::warn!("install failed: {e}"),
                                    }
                                    this.skills = crate::skills::scan_all_skills();
                                    cx.notify();
                                }
                            }
                            SkillAction::Remove { skill_name } => {
                                let _ = crate::skills::remove_zenix_skill(&skill_name);
                                this.skills = crate::skills::scan_all_skills();
                                cx.notify();
                            }
                        }
                    });
                }
            });

            let on_mcp_action: ActionCb<McpAction> = Box::new({
                let entity = entity.clone();
                move |action: McpAction, w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        match action {
                            McpAction::ShowAddForm => {
                                let name_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("server-name")
                                });
                                let command_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("npx -y @server/name")
                                });
                                let args_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("--port 3000")
                                });
                                let env_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("KEY=value")
                                });
                                let form = McpFormState {
                                    editing_name: None,
                                    name: Some(name_input),
                                    command: Some(command_input),
                                    args: Some(args_input),
                                    env: Some(env_input),
                                    mcp_type: "stdio".to_string(),
                                };
                                this.mcp_form = Some(form);
                                cx.notify();
                            }
                            McpAction::CancelAddForm => {
                                this.mcp_form = None;
                                cx.notify();
                            }
                            McpAction::SubmitAddForm => {
                                let form = this.mcp_form.take();
                                if let Some(form) = form {
                                    let name_val = form.name.as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let cmd_val = form.command.as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let args_val = form.args.as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let env_val = form.env.as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    if !name_val.trim().is_empty() && !cmd_val.trim().is_empty() {
                                        let args: Vec<String> = if args_val.trim().is_empty() {
                                            Vec::new()
                                        } else {
                                            args_val.split_whitespace().map(String::from).collect()
                                        };
                                        let env: std::collections::HashMap<String, String> = if env_val.trim().is_empty() {
                                            std::collections::HashMap::new()
                                        } else {
                                            env_val.split_whitespace()
                                                .filter_map(|pair| {
                                                    let mut parts = pair.splitn(2, '=');
                                                    let k = parts.next()?.trim().to_string();
                                                    let v = parts.next()?.trim().to_string();
                                                    if k.is_empty() { None } else { Some((k, v)) }
                                                })
                                                .collect()
                                        };
                                        let mcp_type = if form.mcp_type == "sse" {
                                            crate::mcp::McpType::Sse
                                        } else {
                                            crate::mcp::McpType::Stdio
                                        };
                                        let (server, old_enabled) = if let Some(old_name) = &form.editing_name {
                                            let old = this.mcp_zenix_servers.iter().find(|s| s.name == *old_name);
                                            let old_agents = old.map(|s| s.enabled_agents.clone()).unwrap_or_default();
                                            let _ = crate::mcp::remove_zenix_mcp_server(old_name);
                                            // If name changed, remove old entry from CLI configs
                                            if old_name != name_val.trim() {
                                                for agent in &old_agents {
                                                    let _ = crate::mcp::sync_zenix_server_to_agent(
                                                        &crate::mcp::ZenixMcpServer {
                                                            name: old_name.clone(),
                                                            mcp_type: crate::mcp::McpType::Stdio,
                                                            command: String::new(),
                                                            args: vec![],
                                                            env: std::collections::HashMap::new(),
                                                            enabled_agents: vec![],
                                                        },
                                                        agent,
                                                        false,
                                                    );
                                                }
                                            }
                                            (crate::mcp::ZenixMcpServer {
                                                name: name_val.trim().to_string(),
                                                mcp_type,
                                                command: cmd_val.trim().to_string(),
                                                args,
                                                env,
                                                enabled_agents: old_agents,
                                            }, true)
                                        } else {
                                            (crate::mcp::ZenixMcpServer {
                                                name: name_val.trim().to_string(),
                                                mcp_type,
                                                command: cmd_val.trim().to_string(),
                                                args,
                                                env,
                                                enabled_agents: vec![],
                                            }, false)
                                        };
                                        if let Err(e) = crate::mcp::add_zenix_mcp_server(&server) {
                                            tracing::warn!("add MCP server failed: {e}");
                                        }
                                        // Re-sync to CLI configs if editing
                                        if old_enabled {
                                            for agent in &server.enabled_agents {
                                                let _ = crate::mcp::sync_zenix_server_to_agent(&server, agent, true);
                                            }
                                        }
                                    }
                                    this.refresh_mcp_data(cx);
                                }
                            }
                            McpAction::SetFormType { mcp_type } => {
                                if let Some(form) = &mut this.mcp_form {
                                    form.mcp_type = mcp_type;
                                }
                                cx.notify();
                            }
                            McpAction::EditZenix { name } => {
                                let server = this.mcp_zenix_servers.iter().find(|s| s.name == *name).cloned();
                                if let Some(s) = server {
                                    let name_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("server-name");
                                        st.set_value(&s.name, w, cx);
                                        st
                                    });
                                    let command_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("npx -y @server/name");
                                        st.set_value(&s.command, w, cx);
                                        st
                                    });
                                    let args_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("--port 3000");
                                        st.set_value(s.args.join(" "), w, cx);
                                        st
                                    });
                                    let env_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("KEY=value");
                                        st.set_value(
                                            s.env.iter().map(|(k,v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "),
                                            w, cx
                                        );
                                        st
                                    });
                                    let form = McpFormState {
                                        editing_name: Some(s.name.clone()),
                                        name: Some(name_input),
                                        command: Some(command_input),
                                        args: Some(args_input),
                                        env: Some(env_input),
                                        mcp_type: match s.mcp_type {
                                            crate::mcp::McpType::Sse => "sse".into(),
                                            crate::mcp::McpType::Stdio => "stdio".into(),
                                        },
                                    };
                                    this.mcp_form = Some(form);
                                }
                                cx.notify();
                            }
                            McpAction::RemoveZenix { name } => {
                                let _ = crate::mcp::remove_zenix_mcp_server(&name);
                                this.refresh_mcp_data(cx);
                            }
                            McpAction::ToggleZenixAgent { server_name, agent } => {
                                let _ = crate::mcp::toggle_zenix_mcp_agent(&server_name, &agent);
                                this.refresh_mcp_data(cx);
                            }
                            McpAction::RemoveFromAgent { name, agent } => {
                                let _ = crate::mcp::remove_mcp_server_from_agent(&name, &agent);
                                this.refresh_mcp_data(cx);
                            }
                            McpAction::ToggleForAgent { name, agent } => {
                                let _ = crate::mcp::toggle_mcp_server_for_agent(&name, &agent);
                                this.refresh_mcp_data(cx);
                            }
                        }
                    });
                }
            });

            let on_cli_action: ActionCb<CliAction> = Box::new({
                let entity = entity.clone();
                move |action: CliAction, _w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        match action {
                            CliAction::Install { name } => {
                                tracing::info!("cli install {name}: hook asset bundling needed (TODO)");
                            }
                            CliAction::Update { name } => {
                                tracing::info!("cli update {name}: hook asset bundling needed (TODO)");
                            }
                            CliAction::Uninstall { name } => {
                                let _ = crate::agent::uninstall_hook(&name);
                                this.agent_statuses = crate::agent::detect_all_agents();
                                cx.notify();
                            }
                            CliAction::InstallHook { name } => {
                                tracing::info!("cli install hook {name}: hook asset bundling needed (TODO)");
                            }
                        }
                    });
                }
            });

            div().size_full()
                .child(main_ui)
                .child(
                    super::ui::settings_dialog(
                        tab, on_tab_change,
                        {
                            let entity = entity.clone();
                            Box::new(move |_w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.show_settings = false; cx.notify();
                                });
                            })
                        },
                        fs, &cur_theme, tbs, font_down, font_up,
                        &locale, on_locale_change,
                        agents_ref, mcp_ref, mcp_per_agent_ref, mcp_form_ref, mcp_agents_ref,
                        skills_ref,
                        self.dialog_input.as_ref(), &self.dialog_input_mode,
                        on_skill_action, on_mcp_action, on_cli_action,
                        f32::from(w), f32::from(h),
                    )
                )
                .into_any_element()
        } else {
            main_ui.into_any_element()
        }
    }
}