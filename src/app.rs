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
    select::SelectState,
    searchable_list::SearchableVec,
};

use crate::{
    config::ConfigStore,
    client as herdr,
    ui::{self as sidebar, Panel},
    ui::settings::{ActionCb, ExtensionSubTab, ToastMessage, ContextMenuState, PendingConfirm, SkillAction, McpAction, McpFormState, InputDialogMode, SettingsDialogProps, skill_agent_names,
        ApiProviderAction, ApiProviderFormState},
    api_provider::{ApiProviderConfig, ApiFormat, ThinkingLevel},
    sys::{self as system_info, CpuSamples, SystemInfo},
    terminal::{
        self, BackendCommand, BackendEvent, BackendTx, ImeState, TerminalTab, TerminalElement,
        encode_key, encode_mouse_drag, encode_mouse_event, encode_mouse_motion, encode_mouse_scroll,
    },
};
use crate::i18n::t;

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
    active_extension_subtab: ExtensionSubTab,
    agent_statuses: Vec<crate::agent::AgentCliInfo>,
    selected_agent: Option<usize>,
    mcp_zenix_servers: Vec<crate::mcp::ZenixMcpServer>,
    mcp_per_agent: Vec<(String, Vec<crate::mcp::McpServerEntry>)>,
    skills: std::collections::HashMap<String, crate::skills::SkillInfo>,
    mcp_form: Option<McpFormState>,
    mcp_available_agents: Vec<String>,
    dialog_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    dialog_input_mode: InputDialogMode,
    font_names: Vec<String>,
    font_select: Option<gpui::Entity<FontSelect>>,
    api_providers: Vec<ApiProviderConfig>,
    api_provider_form: Option<ApiProviderFormState>,
    sysinfo_polling: bool,
    terminal_cell_width: Pixels,
    mcp_test_results: std::collections::HashMap<String, String>,
    api_test_results: std::collections::HashMap<String, String>,
    agent_search: String,
    agent_search_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    agent_installed_only: bool,
    mcp_search: String,
    mcp_search_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    api_search: String,
    api_search_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    skill_search_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    active_skills_subtab: crate::ui::settings::SkillsSubTab,
    skill_search: String,
    market_skills: Vec<crate::skills::ApiSkill>,
    market_loading: bool,
    market_error: Option<String>,
    terminal_cursor_blink: bool,
    terminal_cursor_style: String,
    terminal_opacity: f32,
    terminal_scrollback: u32,
    terminal_line_height: Pixels,
    terminal_ascent: Pixels,
    terminal_descent: Pixels,
    last_mouse_cell: Option<(usize, usize)>,
    last_selection_cell: Option<(usize, usize)>,
    ime_state: Option<ImeState>,
    toast: Option<ToastMessage>,
    context_menu: Option<ContextMenuState>,
    pending_confirm: Option<crate::ui::settings::PendingConfirm>,
    single_instance_rx: Option<std::sync::mpsc::Receiver<crate::single_instance::Signal>>,
}
type FontSelect = SelectState<SearchableVec<String>>;

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
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, config: ConfigStore, single_instance_rx: Option<std::sync::mpsc::Receiver<crate::single_instance::Signal>>) -> Self {
        let focus_handle = cx.focus_handle();
        let (events_tx, events_rx) = mpsc::channel();
        let font_size = config.terminal_font_size();
        let cursor_blink = config.terminal_cursor_blink();
        let cursor_style = config.terminal_cursor_style().to_string();
        let opacity = config.terminal_opacity();
        let scrollback = config.terminal_scrollback();
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
            active_extension_subtab: ExtensionSubTab::Api,
            agent_statuses: Vec::new(),
            selected_agent: None,
            mcp_zenix_servers: Vec::new(),
            mcp_per_agent: Vec::new(),
            mcp_test_results: std::collections::HashMap::new(),
            api_test_results: std::collections::HashMap::new(),
            agent_search: String::new(),
            agent_search_input: None,
            agent_installed_only: false,
            active_skills_subtab: crate::ui::settings::SkillsSubTab::Installed,
            skill_search: String::new(),
            market_skills: Vec::new(),
            market_loading: false,
            market_error: None,
            mcp_search: String::new(),
            mcp_search_input: None,
            api_search: String::new(),
            api_search_input: None,
            skill_search_input: None,
            terminal_cursor_blink: cursor_blink,
            terminal_cursor_style: cursor_style,
            terminal_opacity: opacity,
            terminal_scrollback: scrollback,
            skills: std::collections::HashMap::new(),
            mcp_form: None,
            mcp_available_agents: crate::mcp::claude_format_agent_names(),
            dialog_input: None,
            dialog_input_mode: InputDialogMode::Hidden,
            font_names: Vec::new(),
            font_select: None,
            api_providers: Vec::new(),
            api_provider_form: None,
            sysinfo_polling: false,
            terminal_cell_width: px(14.0 * 0.6),
            terminal_line_height: px(14.0 * 1.15),
            terminal_ascent: px(14.0 * 0.8),
            terminal_descent: px(14.0 * 0.2),
            last_mouse_cell: None, last_selection_cell: None,
            ime_state: None,
            toast: None,
            context_menu: None,
            pending_confirm: None,
            single_instance_rx,
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
    fn start_signal_polling(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = self.single_instance_rx.take();
        let Some(rx) = rx else { return };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                while let Ok(signal) = rx.try_recv() {
                    match signal {
                        crate::single_instance::Signal::Focus => {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |_, cx| {
                                    if let Some(handle) = cx.active_window() {
                                        cx.update_window(handle, |_, window, _| {
                                            window.activate_window();
                                        }).ok();
                                    }
                                });
                            }
                        }
                    }
                }
                if this.upgrade().is_none() { break; }
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
            // Force config font — theme configs may override font_family.
            let font_family = self.config.font_family().to_string();
            theme.font_family = font_family.into();
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

    fn create_agent_search_input(w: &mut Window, cx: &mut gpui::Context<Self>) -> gpui::Entity<gpui_component::input::InputState> {
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(w, cx)
                .placeholder("Search agents...")
        });
        cx.subscribe(&input, |this: &mut ZenixApp, input_state, event: &gpui_component::input::InputEvent, cx| {
            if let gpui_component::input::InputEvent::Change = event {
                let val = input_state.read(cx).value().to_string();
                this.agent_search = val;
                cx.notify();
            }
        }).detach();
        input
    }

    fn create_skill_search_input(w: &mut Window, cx: &mut gpui::Context<Self>) -> gpui::Entity<gpui_component::input::InputState> {
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(w, cx)
                .placeholder("Search skills...")
        });
        cx.subscribe(&input, |this: &mut ZenixApp, input_state, event: &gpui_component::input::InputEvent, cx| {
            if let gpui_component::input::InputEvent::Change = event {
                let val = input_state.read(cx).value().to_string();
                this.skill_search = val;
                cx.notify();
            }
        }).detach();
        input
    }
}

impl Render for ZenixApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.connected && !self.connect_attempted {
            self.start_event_pump(cx);
            self.start_signal_polling(window, cx);
            self.connect_herdr(cx);
        }
        if self.connect_ready {
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
        let font = gpui::Font { family: self.config.font_family().into(), ..gpui::Font::default() };
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
        let settings_cb: sidebar::ThemeCallback = Box::new(cx.listener(|this, _: &ClickEvent, window, cx| {
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
            if this.api_providers.is_empty() {
                this.api_providers = crate::api_provider::load_all();
            }
            if this.font_select.is_none() {
                let mut all = window.text_system().all_font_names();
                let embedded = "Lilex".to_string();
                all.retain(|n| n != &embedded && !n.starts_with('.'));
                let mut names = vec![embedded];
                names.extend(all);
                this.font_names = names;

                // Find current font index
                let current = this.config.font_family().to_string();
                let selected_idx = this.font_names.iter().position(|n| *n == current)
                    .map(|i| gpui_component::IndexPath::new(i));
                let delegate = gpui_component::searchable_list::SearchableVec::new(this.font_names.clone());
                let font_select = cx.new(|cx| {
                    FontSelect::new(delegate, selected_idx, window, cx)
                        .searchable(true)
                });
                // Subscribe to font selection confirm
                cx.subscribe_in(&font_select, window, |this, _font_sel, event: &gpui_component::select::SelectEvent<gpui_component::searchable_list::SearchableVec<String>>, _window, cx| {
                    if let gpui_component::select::SelectEvent::Confirm(Some(font_name)) = event {
                        this.config.set_font_family(font_name.clone());
                        let _ = this.config.save();
                        let theme = gpui_component::Theme::global_mut(cx);
                        theme.font_family = font_name.into();
                    }
                    cx.notify();
                }).detach();
                this.font_select = Some(font_select);
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
                .child(TerminalElement::new(snapshot, app_entity, focus2, self.config.font_family(), px(fs), lh_px, cw_px, self.terminal_ascent, self.terminal_descent, ime))
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
            .font_family(self.config.font_family())
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
                        this.config.set_active_settings_tab(idx);
                        let _ = this.config.save();
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
                            SkillAction::SetSubTab(sub) => {
                                this.active_skills_subtab = sub;
                                cx.notify();
                            }
                            SkillAction::Link { skill_name } => {
                                // Link to all supported agents
                                for agent in &skill_agent_names() {
                                    let _ = crate::skills::link_skill(&skill_name, agent);
                                }
                                this.skills = crate::skills::scan_all_skills();
                                cx.notify();
                            }
                            SkillAction::Unlink { skill_name } => {
                                // Unlink from all agents
                                for agent in &skill_agent_names() {
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
                                this.pending_confirm = Some(PendingConfirm::RemoveSkill(skill_name));
                                cx.notify();
                            }
                            SkillAction::FetchMarket => {
                                this.market_loading = true;
                                this.market_error = None;
                                cx.notify();
                                let entity = entity.clone();
                                cx.spawn(async move |_, cx| {
                                    let result = std::thread::spawn(|| crate::skills::fetch_leaderboard()).join();
                                    entity.update(cx, |this, cx| {
                                        this.market_loading = false;
                                        match result {
                                            Ok(Ok(skills)) => {
                                                this.market_skills = skills;
                                            }
                                            Ok(Err(e)) => {
                                                this.market_error = Some(e);
                                            }
                                            Err(_) => {
                                                this.market_error = Some("thread panic".into());
                                            }
                                        }
                                        cx.notify();
                                    });
                                }).detach();
                            }
                            SkillAction::InstallApi { skill_id } => {
                                let entity = entity.clone();
                                cx.spawn(async move |_, cx| {
                                    let result = std::thread::spawn(move || {
                                        let install_result = crate::skills::install_from_api(&skill_id);
                                        (install_result, crate::skills::scan_all_skills())
                                    }).join();
                                    entity.update(cx, |this, cx| {
                                        match result {
                                            Ok((Ok(name), skills)) => {
                                                this.skills = skills;
                                                this.toast = Some(ToastMessage {
                                                    text: format!("Installed: {name}"),
                                                    msg_type: "success".into(),
                                                });
                                            }
                                            Ok((Err(e), skills)) => {
                                                this.skills = skills;
                                                this.toast = Some(ToastMessage {
                                                    text: format!("Install failed: {e}"),
                                                    msg_type: "error".into(),
                                                });
                                            }
                                            Err(_) => {
                                                this.toast = Some(ToastMessage {
                                                    text: "Install thread panicked".into(),
                                                    msg_type: "error".into(),
                                                });
                                            }
                                        }
                                        cx.notify();
                                    });
                                }).detach();
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
                                    url: None,
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
                                    let url_val = form.url.as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let mcp_type = if form.mcp_type == "sse" {
                                        crate::mcp::McpType::Sse
                                    } else {
                                        crate::mcp::McpType::Stdio
                                    };
                                    // For SSE, use URL as command; for stdio, use command field
                                    let effective_cmd = if mcp_type == crate::mcp::McpType::Sse {
                                        url_val.trim().to_string()
                                    } else {
                                        cmd_val.trim().to_string()
                                    };
                                    if !name_val.trim().is_empty() && !effective_cmd.is_empty() {
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
                                        let (server, old_enabled) = if let Some(old_name) = &form.editing_name {
                                            let old = this.mcp_zenix_servers.iter().find(|s| s.name == *old_name);
                                            let old_agents = old.map(|s| s.enabled_agents.clone()).unwrap_or_default();
                                            let _ = crate::mcp::remove_zenix_mcp_server(old_name);
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
                                                command: effective_cmd,
                                                args,
                                                env,
                                                enabled_agents: old_agents,
                                            }, true)
                                        } else {
                                            (crate::mcp::ZenixMcpServer {
                                                name: name_val.trim().to_string(),
                                                mcp_type,
                                                command: effective_cmd,
                                                args,
                                                env,
                                                enabled_agents: vec![],
                                            }, false)
                                        };
                                        if let Err(e) = crate::mcp::add_zenix_mcp_server(&server) {
                                            this.toast = Some(ToastMessage {
                                                text: format!("Failed to add MCP server: {e}"),
                                                msg_type: "error".into(),
                                            });
                                        }
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
                                    form.mcp_type = mcp_type.clone();
                                    // Create URL input when switching to SSE
                                    if mcp_type == "sse" && form.url.is_none() {
                                        form.url = Some(cx.new(|cx| {
                                            gpui_component::input::InputState::new(w, cx).placeholder("http://localhost:3000/sse")
                                        }));
                                    }
                                    // Clear URL input when switching to stdio
                                    if mcp_type == "stdio" {
                                        form.url = None;
                                    }
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
                                    let url_input = if s.mcp_type == crate::mcp::McpType::Sse {
                                        Some(cx.new(|cx| {
                                            let mut st = gpui_component::input::InputState::new(w, cx).placeholder("http://localhost:3000/sse");
                                            st.set_value(&s.command, w, cx);
                                            st
                                        }))
                                    } else {
                                        None
                                    };
                                    let form = McpFormState {
                                        editing_name: Some(s.name.clone()),
                                        name: Some(name_input),
                                        command: Some(command_input),
                                        args: Some(args_input),
                                        env: Some(env_input),
                                        url: url_input,
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
                                this.pending_confirm = Some(PendingConfirm::RemoveMcp(name));
                                cx.notify();
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
                            McpAction::TestServer { name } => {
                                let server = this.mcp_zenix_servers.iter().find(|s| s.name == name).cloned();
                                if let Some(srv) = server {
                                    let result = match crate::mcp::test_mcp_server(&srv) {
                                        Ok(msg) => format!("OK: {msg}"),
                                        Err(e) => format!("ERR: {e}"),
                                    };
                                    this.mcp_test_results.insert(name.clone(), result);
                                } else {
                                    this.mcp_test_results.insert(name.clone(), "ERR: Server not found".into());
                                }
                                let toast_msg = this.mcp_test_results.get(&name).cloned().unwrap_or_default();
                                let toast_type = if toast_msg.starts_with("OK:") { "success" } else { "error" };
                                this.toast = Some(ToastMessage { text: toast_msg, msg_type: toast_type.into() });
                                cx.notify();
                            }
                            McpAction::TestAgentServer { name, agent } => {
                                let entry = this.mcp_per_agent.iter()
                                    .find(|(a, _)| a == &agent)
                                    .and_then(|(_, entries)| entries.iter().find(|e| e.name == name))
                                    .cloned();
                                let result = match entry {
                                    Some(e) => match crate::mcp::test_agent_mcp_server(&e) {
                                        Ok(msg) => format!("OK: {msg}"),
                                        Err(e) => format!("ERR: {e}"),
                                    },
                                    None => "ERR: Server entry not found".into(),
                                };
                                let key = format!("{name}@{agent}");
                                this.mcp_test_results.insert(key, result.clone());
                                let toast_type = if result.starts_with("OK:") { "success" } else { "error" };
                                this.toast = Some(ToastMessage { text: result, msg_type: toast_type.into() });
                                cx.notify();
                            }
                        }
                    });
                }
            });


            let on_api_action: ActionCb<ApiProviderAction> = Box::new({
                let entity = entity.clone();
                move |action: ApiProviderAction, w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| {
                        match action {
                            ApiProviderAction::ShowAddForm => {
                                let name_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("my-provider")
                                });
                                let base_url_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("https://api.openai.com")
                                });
                                let api_key_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("sk-...")
                                });
                                let suffix_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx).placeholder("/v1/chat/completions")
                                });
                                this.api_provider_form = Some(ApiProviderFormState {
                                    editing_name: None,
                                    name: Some(name_input),
                                    base_url: Some(base_url_input),
                                    api_key: Some(api_key_input),
                                    url_suffix: Some(suffix_input),
                                    format: ApiFormat::OpenAI,
                                    thinking_level: ThinkingLevel::default(),
                                    available_models: vec![],
                                    selected_model: String::new(),
                                    model_search: String::new(),
                                });
                                cx.notify();
                            }
                            ApiProviderAction::CancelForm => {
                                this.api_provider_form = None;
                                cx.notify();
                            }
                            ApiProviderAction::SubmitForm => {
                                let form = this.api_provider_form.take();
                                if let Some(form) = form {
                                    let name_val = form.name.as_ref()
                                        .map(|e| e.read(cx).value().to_string()).unwrap_or_default();
                                    let base_val = form.base_url.as_ref()
                                        .map(|e| e.read(cx).value().to_string()).unwrap_or_default();
                                    let key_val = form.api_key.as_ref()
                                        .map(|e| e.read(cx).value().to_string()).unwrap_or_default();
                                    let suffix_val = form.url_suffix.as_ref()
                                        .map(|e| e.read(cx).value().to_string()).unwrap_or_default();
                                    if name_val.trim().is_empty() {
                                        this.toast = Some(ToastMessage {
                                            text: "Provider name is required".into(),
                                            msg_type: "error".into(),
                                        });
                                        this.api_provider_form = Some(form);
                                        cx.notify();
                                        return;
                                    }
                                    if !base_val.trim().starts_with("http://") && !base_val.trim().starts_with("https://") {
                                        this.toast = Some(ToastMessage {
                                            text: "Base URL must start with http:// or https://".into(),
                                            msg_type: "error".into(),
                                        });
                                        this.api_provider_form = Some(form);
                                        cx.notify();
                                        return;
                                    }
                                    if key_val.trim().is_empty() {
                                        this.toast = Some(ToastMessage {
                                            text: "API key is required".into(),
                                            msg_type: "error".into(),
                                        });
                                        this.api_provider_form = Some(form);
                                        cx.notify();
                                        return;
                                    }
                                    let existing_agents = if form.editing_name.is_some() {
                                        this.api_providers.iter()
                                            .find(|p| p.name == form.editing_name.as_deref().unwrap_or(""))
                                            .map(|p| p.enabled_agents.clone())
                                            .unwrap_or_default()
                                    } else {
                                        vec![]
                                    };
                                    let cfg = ApiProviderConfig {
                                        name: name_val.trim().to_string(),
                                        format: form.format,
                                        base_url: base_val.trim().to_string(),
                                        api_key: key_val.trim().to_string(),
                                        model: form.selected_model,
                                        thinking_level: form.thinking_level,
                                        url_suffix: suffix_val.trim().to_string(),
                                        enabled_agents: existing_agents,
                                    };
                                    let _ = crate::api_provider::upsert(&cfg);
                                    this.api_providers = crate::api_provider::load_all();
                                }
                                cx.notify();
                            }
                            ApiProviderAction::SetThinkingLevel { level } => {
                                if let Some(form) = &mut this.api_provider_form {
                                    form.thinking_level = level;
                                }
                                cx.notify();
                            }
                            ApiProviderAction::SetSuffix { suffix } => {
                                if let Some(form) = &mut this.api_provider_form {
                                    if let Some(suffix_input) = &form.url_suffix {
                                        suffix_input.update(cx, |state, cx| {
                                            state.set_value(&suffix, w, cx);
                                        });
                                    }
                                }
                                cx.notify();
                            }
                            ApiProviderAction::SetFormat { format } => {
                                if let Some(form) = &mut this.api_provider_form {
                                    form.format = format;
                                    if let Some(suffix_input) = &form.url_suffix {
                                        suffix_input.update(cx, |state, cx| {
                                            state.set_value(format.default_suffix(), w, cx);
                                        });
                                    }
                                }
                                cx.notify();
                            }
                            ApiProviderAction::ApplyPreset { preset } => {
                                if let Some(form) = &mut this.api_provider_form {
                                    if let Some(name_input) = &form.name {
                                        name_input.update(cx, |state, cx| {
                                            state.set_value(preset.name.to_string(), w, cx);
                                        });
                                    }
                                    if let Some(url_input) = &form.base_url {
                                        url_input.update(cx, |state, cx| {
                                            state.set_value(preset.base_url.to_string(), w, cx);
                                        });
                                    }
                                    if let Some(suffix_input) = &form.url_suffix {
                                        suffix_input.update(cx, |state, cx| {
                                            state.set_value(preset.suffix, w, cx);
                                        });
                                    }
                                }
                                cx.notify();
                            }
                            ApiProviderAction::Edit { name } => {
                                if let Some(cfg) = this.api_providers.iter().find(|p| p.name == name).cloned() {
                                    let name_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("name");
                                        st.set_value(&cfg.name, w, cx);
                                        st
                                    });
                                    let base_url_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("url");
                                        st.set_value(&cfg.base_url, w, cx);
                                        st
                                    });
                                    let api_key_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("key");
                                        st.set_value(&cfg.api_key, w, cx);
                                        st
                                    });
                                    let suffix_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx).placeholder("suffix");
                                        st.set_value(&cfg.url_suffix, w, cx);
                                        st
                                    });
                                    this.api_provider_form = Some(ApiProviderFormState {
                                        editing_name: Some(cfg.name.clone()),
                                        name: Some(name_input),
                                        base_url: Some(base_url_input),
                                        api_key: Some(api_key_input),
                                        url_suffix: Some(suffix_input),
                                        format: cfg.format,
                                        thinking_level: cfg.thinking_level,
                                        available_models: vec![],
                                        selected_model: cfg.model.clone(),
                                        model_search: String::new(),
                                    });
                                }
                                cx.notify();
                            }
                            ApiProviderAction::Remove { name } => {
                                this.pending_confirm = Some(PendingConfirm::RemoveApi(name));
                                cx.notify();
                            }
                            ApiProviderAction::FetchModels { name } => {
                                if let Some(cfg) = this.api_providers.iter().find(|p| p.name == name).cloned() {
                                    let models = std::thread::spawn(move || {
                                        crate::api_provider::fetch_models(&cfg)
                                    }).join().unwrap_or_else(|_| Err("thread panic".into()));
                                    match models {
                                        Ok(list) => {
                                            if let Some(form) = &mut this.api_provider_form {
                                                form.available_models = list;
                                                if form.selected_model.is_empty() {
                                                    form.selected_model = form.available_models.first().cloned().unwrap_or_default();
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!("fetch models failed for {name}: {e}");
                                        }
                                    }
                                }
                                cx.notify();
                            }
                            ApiProviderAction::SelectModel { model } => {
                                if let Some(form) = &mut this.api_provider_form {
                                    form.selected_model = model;
                                }
                                cx.notify();
                            }
                            ApiProviderAction::ToggleAgent { name, agent } => {
                                if let Some(prov) = this.api_providers.iter_mut().find(|p| p.name == name) {
                                    let _ = crate::api_provider::toggle_api_agent(prov, &agent);
                                    this.api_providers = crate::api_provider::load_all();
                                }
                                cx.notify();
                            }
                            ApiProviderAction::TestConnection { name } => {
                                let cfg = this.api_providers.iter().find(|p| p.name == name).cloned();
                                if let Some(config) = cfg {
                                    let result = match crate::api_provider::test_connection(&config) {
                                        Ok(msg) => format!("OK: {msg}"),
                                        Err(e) => format!("ERR: {e}"),
                                    };
                                    this.api_test_results.insert(name.clone(), result);
                                } else {
                                    this.api_test_results.insert(name.clone(), "ERR: Provider not found".into());
                                }
                                let toast_msg = this.api_test_results.get(&name).cloned().unwrap_or_default();
                                let toast_type = if toast_msg.starts_with("OK:") { "success" } else { "error" };
                                this.toast = Some(ToastMessage { text: toast_msg, msg_type: toast_type.into() });
                                cx.notify();
                            }
                        }
                    });
                }
            });

            let api_ref = &self.api_providers;
            let api_form_ref = self.api_provider_form.as_ref();

            if self.agent_search_input.is_none() {
                let input_state = Self::create_agent_search_input(window, cx);
                self.agent_search_input = Some(input_state);
            }
            if self.skill_search_input.is_none() {
                let input_state = Self::create_skill_search_input(window, cx);
                self.skill_search_input = Some(input_state);
            }
            let font_select_ref = self.font_select.as_ref();

            div().size_full()
                .child(main_ui)
                .child(
                    super::ui::settings_dialog(SettingsDialogProps {
                        active_tab: tab,
                        on_tab_change,
                        on_extension_tab_change: Box::new({
                            let entity = entity.clone();
                            move |sub: ExtensionSubTab, _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.active_extension_subtab = sub;
                                    cx.notify();
                                });
                            }
                        }),
                        active_extension_subtab: self.active_extension_subtab,
                        on_close: {
                            let entity = entity.clone();
                            Box::new(move |_w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.show_settings = false; cx.notify();
                                });
                            })
                        },
                        font_size: fs,
                        current_theme_name: &cur_theme,
                        theme_buttons: tbs,
                        font_select: font_select_ref,
                        on_font_down: font_down,
                        on_font_up: font_up,
                        locale: &locale,
                        on_locale_change,
                        agents: agents_ref,
                        selected_agent: self.selected_agent,
                        on_agent_select: Box::new({
                            let entity = entity.clone();
                            move |idx: Option<usize>, _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.selected_agent = idx;
                                    cx.notify();
                                });
                            }
                        }),
                        on_agent_refresh: Box::new({
                            let entity = entity.clone();
                            move |_v: (), _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.agent_statuses = crate::agent::detect_all_agents();
                                    cx.notify();
                                });
                            }
                        }),
                        mcp_servers: mcp_ref,
                        mcp_test_results: &self.mcp_test_results,
                        api_test_results: &self.api_test_results,
                        agent_search: &self.agent_search,
                        agent_search_input: self.agent_search_input.as_ref(),
                        on_agent_toggle_installed: Box::new({
                            let entity = entity.clone();
                            move |_val: bool, _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.agent_installed_only = !this.agent_installed_only;
                                    cx.notify();
                                });
                            }
                        }),
                        agent_installed_only: self.agent_installed_only,
                        active_skills_subtab: self.active_skills_subtab,
                        skill_search: &self.skill_search,
                        mcp_search: &self.mcp_search,
                        mcp_search_input: self.mcp_search_input.as_ref(),
                        api_search: &self.api_search,
                        api_search_input: self.api_search_input.as_ref(),
                        skill_search_input: self.skill_search_input.as_ref(),
                        market_skills: &self.market_skills,
                        market_loading: self.market_loading,
                        market_error: self.market_error.as_deref(),
                        terminal_cursor_blink: self.terminal_cursor_blink,
                        terminal_cursor_style: &self.terminal_cursor_style,
                        terminal_opacity: self.terminal_opacity,
                        terminal_scrollback: self.terminal_scrollback,
                        on_terminal_action: Box::new({
                            let entity = entity.clone();
                            move |action: crate::ui::settings::TerminalAction, _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    match action {
                                        crate::ui::settings::TerminalAction::SetCursorStyle(s) => {
                                            this.terminal_cursor_style = s.clone();
                                            this.config.set_terminal_cursor_style(s);
                                        }
                                        crate::ui::settings::TerminalAction::ToggleCursorBlink => {
                                            this.terminal_cursor_blink = !this.terminal_cursor_blink;
                                            this.config.set_terminal_cursor_blink(this.terminal_cursor_blink);
                                        }
                                        crate::ui::settings::TerminalAction::SetOpacity(op) => {
                                            this.terminal_opacity = op;
                                            this.config.set_terminal_opacity(op);
                                        }
                                        crate::ui::settings::TerminalAction::SetScrollback(sb) => {
                                            this.terminal_scrollback = sb;
                                            this.config.set_terminal_scrollback(sb);
                                        }
                                    }
                                    let _ = this.config.save();
                                    cx.notify();
                                });
                            }
                        }),
                        mcp_per_agent: mcp_per_agent_ref,
                        mcp_form: mcp_form_ref,
                        mcp_available_agents: mcp_agents_ref,
                        skills: skills_ref,
                        skill_dialog_input: self.dialog_input.as_ref(),
                        skill_dialog_mode: &self.dialog_input_mode,
                        on_skill_action,
                        on_mcp_action,
                        api_providers: api_ref,
                        api_form: api_form_ref,
                        on_api_action,
                        toast: self.toast.as_ref(),
                        context_menu: self.context_menu.as_ref(),
                        on_context_menu_dismiss: Box::new({
                            let entity = entity.clone();
                            move |_: (), _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    this.context_menu = None;
                                    cx.notify();
                                });
                            }
                        }),
                        pending_confirm: self.pending_confirm.as_ref(),
                        on_confirm_action: Box::new({
                            let entity = entity.clone();
                            move |confirmed: bool, _w: &mut Window, app: &mut gpui::App| {
                                entity.update(app, |this: &mut ZenixApp, cx| {
                                    if confirmed {
                                        if let Some(pc) = this.pending_confirm.take() {
                                            match pc {
                                                PendingConfirm::RemoveMcp(name) => {
                                                    let _ = crate::mcp::remove_zenix_mcp_server(&name);
                                                    this.refresh_mcp_data(cx);
                                                    this.toast = Some(ToastMessage { text: format!("Removed {name}"), msg_type: "success".into() });
                                                }
                                                PendingConfirm::RemoveSkill(name) => {
                                                    let _ = crate::skills::remove_zenix_skill(&name);
                                                    this.skills = crate::skills::scan_all_skills();
                                                    this.toast = Some(ToastMessage { text: format!("Removed {name}"), msg_type: "success".into() });
                                                }
                                                PendingConfirm::RemoveApi(name) => {
                                                    let _ = crate::api_provider::remove(&name);
                                                    this.api_providers = crate::api_provider::load_all();
                                                    this.toast = Some(ToastMessage { text: format!("Removed {name}"), msg_type: "success".into() });
                                                }
                                            }
                                        }
                                    } else {
                                        this.pending_confirm = None;
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                        font_family: self.config.font_family(),
                        win_w: f32::from(w),
                        win_h: f32::from(h),
                    })
                )
                .into_any_element()
        } else {
            main_ui.into_any_element()
        }
    }
}