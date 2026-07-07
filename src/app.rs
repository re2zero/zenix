//! Single-terminal app — auto-connects to herdr on startup.

use std::{
    ops::Range,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use alacritty_terminal::{index::Side, selection::SelectionType};
use gpui::{
    App, AppContext, Bounds, ClickEvent, ClipboardItem, Context, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    Styled as _, Window, actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, ElementExt, Theme, ThemeRegistry,
    button::{Button, ButtonVariants},
    h_flex,
    searchable_list::SearchableVec,
    select::SelectState,
    v_flex,
};

use crate::i18n::t;
use crate::{
    api_provider::{ApiFormat, ApiProviderConfig, ThinkingLevel},
    client as herdr,
    config::ConfigStore,
    sys::{self as system_info, CpuSamples, SystemInfo},
    terminal::{
        self, BackendCommand, BackendEvent, BackendTx, ImeState, TerminalElement, TerminalTab,
        encode_key, encode_mouse_drag, encode_mouse_event, encode_mouse_motion,
        encode_mouse_scroll,
    },
    ui::settings::{
        ActionCb, ApiProviderAction, ApiProviderFormState, ContextMenuState, ExtensionSubTab,
        InputDialogMode, McpAction, McpFormState, PendingConfirm, SettingsDialogProps, SkillAction,
        ToastMessage, skill_agent_names,
    },
    ui::{self as sidebar, Panel},
};

// Tab action types to intercept Tab before gpui-component's Root handler.
// ChatHistory{Up,Down} navigate the multi-line input's history via Alt+arrows.
actions!([TabToPty, ShiftTabToPty, ChatHistoryUp, ChatHistoryDown]);

/// Register keybindings at app level. Must be called after gpui_component::init
/// so these bindings take precedence over gpui-component's Root-level bindings.
pub fn init_actions(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", TabToPty, None),
        KeyBinding::new("shift-tab", ShiftTabToPty, None),
        KeyBinding::new("alt-up", ChatHistoryUp, None),
        KeyBinding::new("alt-down", ChatHistoryDown, None),
    ]);
}

const MAX_CONNECT_ATTEMPTS: usize = 200;
const FONT_SIZE_MIN: f32 = 8.0;
const FONT_SIZE_MAX: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HistoryDir {
    Up,
    Down,
}

/// Tracks an in-progress panel-resize drag when gpui's `on_drag` is unreliable.
struct PanelResizeDragState {
    /// Cursor X at drag start (window coords).
    start_x: f32,
    /// Panel width at drag start.
    start_width: f32,
    /// Which panel is being resized.
    target: Panel,
}

/// Tracks an in-progress input-height resize drag.
struct InputResizeDragState {
    /// Cursor Y at drag start (window coords).
    start_y: f32,
    /// Input height at drag start.
    start_height: f32,
}

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
    pub terminal_cursor_style: String,
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
    ai_chat_panel: Option<crate::ai::chat::AiChatPanelData>,
    chat_input_entity: Option<gpui::Entity<gpui_component::input::InputState>>,
    pending_clear_chat_input: bool,
    /// Resizable width for the system-info panel (starts at default).
    system_info_width: f32,
    /// When `Some(start_x)`, a panel-resize drag is in progress.
    /// We track this ourselves because gpui's `on_drag` does not reliably fire
    /// on absolutely-positioned resize handles inside a flex layout.
    panel_resize_drag: Option<PanelResizeDragState>,
    input_resize_drag: Option<InputResizeDragState>,
    agent_polling: bool,
    pub cursor_visible: bool,
    single_instance_rx: Option<std::sync::mpsc::Receiver<crate::single_instance::Signal>>,
}
type FontSelect = SelectState<SearchableVec<String>>;

fn write_to_pty(backend: &Option<BackendTx>, tab: &mut Option<TerminalTab>, bytes: Vec<u8>) {
    if let Some(backend) = backend {
        // Scroll to bottom if there's a display offset
        if let Some(tab) = tab
            && tab.render_snapshot().display_offset > 0
        {
            tab.scroll_to_bottom();
        }
        // Always send input regardless of display_offset
        backend.send(BackendCommand::Input(bytes));
    }
}

/// Return the current UTC timestamp in ISO 8601 format (matching SQLite datetime output).
pub(crate) fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let h = time_of_day / 3600;
            let min = (time_of_day % 3600) / 60;
            let s = time_of_day % 60;
            // Simple date calculation from days since epoch
            // 1970-01-01 + days
            let mut y = 1970i32;
            let mut remaining_days = days as i32;
            loop {
                let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    366
                } else {
                    365
                };
                if remaining_days < diy {
                    break;
                }
                remaining_days -= diy;
                y += 1;
            }
            let month_days = [
                31,
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let mut m = 0usize;
            for (i, &md) in month_days.iter().enumerate() {
                if remaining_days < md {
                    m = i;
                    break;
                }
                remaining_days -= md;
            }
            let d = remaining_days + 1;
            format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}")
        })
        .unwrap_or_default()
}

impl ZenixApp {
    pub fn new(
        _window: &mut Window,
        cx: &mut Context<Self>,
        config: ConfigStore,
        single_instance_rx: Option<std::sync::mpsc::Receiver<crate::single_instance::Signal>>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let (events_tx, events_rx) = mpsc::channel();
        let font_size = config.terminal_font_size();
        let cursor_blink = config.terminal_cursor_blink();
        let cursor_style = config.terminal_cursor_style().to_string();
        let opacity = config.terminal_opacity();
        let scrollback = config.terminal_scrollback();
        let result = Self {
            focus_handle,
            backend: None,
            tab: None,
            status: t("status.initializing"),
            events_rx,
            events_tx,
            launch_attempts: 0,
            connected: false,
            connect_attempted: false,
            connect_ready: false,
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
            last_mouse_cell: None,
            last_selection_cell: None,
            ime_state: None,
            cursor_visible: true,
            toast: None,
            context_menu: None,
            pending_confirm: None,
            ai_chat_panel: None,
            chat_input_entity: None,
            pending_clear_chat_input: false,
            system_info_width: sidebar::PANEL_WIDTH,
            panel_resize_drag: None,
            input_resize_drag: None,
            agent_polling: false,
            single_instance_rx,
        };
        // Start cursor blink timer if enabled
        if cursor_blink {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            // Stop blinking if cursor_blink is disabled
                            if !this.terminal_cursor_blink {
                                this.cursor_visible = true;
                                return true;
                            }
                            this.cursor_visible = !this.cursor_visible;
                            cx.notify();
                            false
                        })
                        .is_err()
                        || this
                            .update(cx, |this, _| this.terminal_cursor_blink)
                            .unwrap_or(true)
                            == false
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        result
    }

    fn start_event_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.drain_pty() {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
    fn start_signal_polling(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = self.single_instance_rx.take();
        let Some(rx) = rx else { return };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                while let Ok(signal) = rx.try_recv() {
                    match signal {
                        crate::single_instance::Signal::Focus => {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |_, cx| {
                                    if let Some(handle) = cx.active_window() {
                                        cx.update_window(handle, |_, window, _| {
                                            window.activate_window();
                                        })
                                        .ok();
                                    }
                                });
                            }
                        }
                    }
                }
                if this.upgrade().is_none() {
                    break;
                }
            }
        })
        .detach();
    }

    fn drain_pty(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events_rx.try_recv() {
            changed = true;
            match event {
                BackendEvent::Output { tab_id, bytes } => {
                    if let Some(tab) = &mut self.tab
                        && tab.id == tab_id
                    {
                        tab.feed(&bytes);
                    }
                }
                BackendEvent::TitleChanged { title, .. } => {
                    tracing::debug!("terminal title: {title}");
                }
                BackendEvent::Closed { reason, .. } => {
                    tracing::warn!("pty closed: {reason}");
                    self.tab = None;
                    self.connected = false;
                    self.connect_attempted = false;
                    self.launch_attempts = 0;
                    self.status = t("status.disconnected").replace("%{reason}", &reason);
                }
                _ => {}
            }
        }
        if self.connect_attempted && !self.connected && self.launch_attempts < MAX_CONNECT_ATTEMPTS
        {
            if self.launch_attempts == 0 {
                return changed;
            }
            let socket = herdr::herdr_socket_path();
            if herdr::is_socket_ready(&socket) {
                self.connect_ready = true;
                changed = true;
            } else {
                self.launch_attempts += 1;
                self.status =
                    t("status.waiting").replace("%{n}", &self.launch_attempts.to_string());
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
        unsafe {
            std::env::remove_var("HERDR_ENV");
            std::env::remove_var("HERDR_PANE_ID");
            std::env::remove_var("HERDR_SOCKET_PATH");
        }
        let id = uuid::Uuid::new_v4().to_string();
        tracing::info!("spawning PTY: {program} {args:?}");
        let cols = 120u16;
        let rows = 40u16;
        match terminal::spawn_command_in_pty(
            id.clone(),
            program,
            args,
            cols,
            rows,
            self.events_tx.clone(),
        ) {
            Ok(backend) => {
                let mut tab = TerminalTab::new_local(
                    id,
                    "terminal".into(),
                    backend.clone(),
                    self.events_tx.clone(),
                );
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
            None => {
                self.status = t("status.not_found");
                return;
            }
        };
        let socket = herdr::herdr_socket_path();
        if herdr::is_socket_ready(&socket) {
            tracing::info!("socket ready, spawning PTY");
            self.spawn_pty(&binary, &["client"], cx);
        } else {
            tracing::info!("socket not ready, starting server");
            if self.launch_attempts == 0 {
                herdr::start_herdr_server();
            }
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
        self.active_panel = if self.active_panel == panel {
            Panel::None
        } else {
            panel
        };
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
    pub(crate) fn marked_text_range(&self) -> Option<Range<usize>> {
        self.ime_state
            .as_ref()
            .filter(|s| !s.marked_text.is_empty())
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
        if self.sysinfo_polling {
            return;
        }
        self.sysinfo_polling = true;
        cx.spawn(async move |this, cx| {
            let mut samples = CpuSamples::default();
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let mut info = SystemInfo::default();
                let new_samples = system_info::collect(&mut info, &samples);
                samples = new_samples;
                if this
                    .update(cx, |this, cx| {
                        this.system_info = info;
                        this.sysinfo_cpu_sample = samples.clone();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
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
        if (modifiers.secondary() && key.eq_ignore_ascii_case(","))
            || key.eq_ignore_ascii_case("f1")
        {
            self.show_settings = !self.show_settings;
            cx.notify();
            return;
        }
        if modifiers.secondary()
            && (key.eq_ignore_ascii_case("=") || key.eq_ignore_ascii_case("numpadadd"))
        {
            self.change_font_size(self.terminal_font_size + 1.0);
            cx.notify();
            return;
        }
        if modifiers.secondary()
            && (key.eq_ignore_ascii_case("-") || key.eq_ignore_ascii_case("numpadsubtract"))
        {
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

        // Tab key penetration: when terminal has focus and settings is not shown,
        // send Tab to PTY. Use stop_propagation to prevent GPUI's built-in
        // Tab/Shift-Tab focus navigation from moving focus away.
        if key.eq_ignore_ascii_case("tab") && !self.show_settings {
            if let Some(tab) = &self.tab
                && let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false)
            {
                write_to_pty(&self.backend, &mut self.tab, bytes);
            }
            cx.stop_propagation();
            return;
        }

        // All printable characters without ctrl/alt/platform modifiers are handled by
        // the InputHandler's replace_text_in_range. We must NOT send
        // them here to avoid double input (including shift+letter).
        // Linux reports Tab as a named key, so encode it explicitly instead of
        // treating it as printable text.
        if !key.eq_ignore_ascii_case("tab")
            && (event.keystroke.key.len() == 1 || event.keystroke.key.eq_ignore_ascii_case("space"))
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.platform
        {
            return;
        }

        // Terminal keystrokes (named keys, modified keys): encode and send.
        // Stop propagation to prevent GPUI's InputHandler from also dispatching
        // the key_char, which would cause double-send (e.g., Ctrl+V sent twice).
        if let Some(tab) = &self.tab
            && let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false)
        {
            write_to_pty(&self.backend, &mut self.tab, bytes);
            cx.stop_propagation();
            return;
        }
    }

    // Tab action handler — catches Tab before gpui-component's Root handler.
    // Writes Tab byte to PTY. In the action dispatch bubble phase, propagation
    // stops by default, preventing Root's focus_next from running.
    fn on_tab_to_pty(&mut self, _: &TabToPty, _window: &mut Window, _cx: &mut Context<Self>) {
        write_to_pty(&self.backend, &mut self.tab, vec![0x09]);
    }

    fn on_shift_tab_to_pty(
        &mut self,
        _: &ShiftTabToPty,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        write_to_pty(&self.backend, &mut self.tab, vec![0x09]);
    }

    fn on_chat_history_up(&mut self, _: &ChatHistoryUp, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_chat_history(HistoryDir::Up, window, cx);
    }

    fn on_chat_history_down(
        &mut self,
        _: &ChatHistoryDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_chat_history(HistoryDir::Down, window, cx);
    }

    fn navigate_chat_history(
        &mut self,
        dir: HistoryDir,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.chat_input_entity.as_ref() else {
            return;
        };
        let Some(panel) = self.ai_chat_panel.as_mut() else {
            return;
        };
        if panel.input_history.is_empty() {
            return;
        }
        let len = panel.input_history.len();
        let new_idx = match (panel.input_history_idx, dir) {
            (None, HistoryDir::Up) => Some(len - 1),
            (None, HistoryDir::Down) => None,
            (Some(i), HistoryDir::Up) => Some(i.saturating_sub(1)),
            (Some(i), HistoryDir::Down) => {
                if i + 1 >= len {
                    None
                } else {
                    Some(i + 1)
                }
            }
        };
        panel.input_history_idx = new_idx;
        let value = match new_idx {
            Some(i) => panel.input_history[i].clone(),
            None => String::new(),
        };
        input.update(cx, |state, cx| state.set_value(value, window, cx));
        cx.notify();
    }

    // ── Mouse dispatch ──────────────────────────────────────────────────

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.show_settings {
            return;
        }

        // Check if click starts an input-height resize inside the AI panel.
        if self.active_panel == Panel::AiChat && event.button == MouseButton::Left {
            let bounds = window.bounds();
            let ih = self
                .ai_chat_panel
                .as_ref()
                .map(|d| d.input_height)
                .unwrap_or(160.0);
            let handle_y: f32 = f32::from(bounds.size.height) - ih - 3.0;
            let click_y: f32 = f32::from(event.position.y);
            if (click_y - handle_y).abs() < 6.0 {
                self.input_resize_drag = Some(InputResizeDragState {
                    start_y: click_y,
                    start_height: ih,
                });
                self.panel_resize_drag = None;
                cx.stop_propagation();
                return;
            }
        }

        // Check if this click starts a panel resize (left edge of active panel).
        if self.active_panel != Panel::None && event.button == MouseButton::Left {
            let bounds = window.bounds();
            let pw: f32 = match self.active_panel {
                Panel::AiChat => self
                    .ai_chat_panel
                    .as_ref()
                    .map(|d| d.ai_panel_width)
                    .unwrap_or(sidebar::AI_PANEL_WIDTH),
                Panel::SystemInfo => self.system_info_width,
                Panel::None => 0.0,
            };
            let panel_left_edge: f32 =
                f32::from(bounds.size.width) - sidebar::SIDEBAR_WIDTH - pw;
            let click_x: f32 = f32::from(event.position.x);
            if (click_x - panel_left_edge).abs() < 4.0 {
                self.panel_resize_drag = Some(PanelResizeDragState {
                    start_x: click_x,
                    start_width: pw,
                    target: self.active_panel,
                });
                cx.stop_propagation();
                return;
            }
        }

        // Only route clicks to the terminal when they actually land inside the
        // terminal area. Without this guard, every click anywhere in the
        // window (including the chat panel) calls `window.focus(&self.focus_handle)`,
        // which steals focus from chat input / sidebar search fields and prevents
        // them from ever receiving keyboard input.
        let in_terminal = self
            .terminal_bounds
            .is_some_and(|b| b.contains(&event.position));
        if !in_terminal {
            return;
        }
        window.focus(&self.focus_handle, cx);
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, true, cx);
        } else if event.button == MouseButton::Left {
            self.begin_terminal_selection(event, cx);
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.show_settings {
            return;
        }

        // Handle input height resize drag.
        if let Some(ref drag) = self.input_resize_drag {
            let cursor_y: f32 = f32::from(event.position.y);
            let delta = cursor_y - drag.start_y;
            let new_height = (drag.start_height - delta)
                .clamp(crate::ai::chat::resize::MIN_INPUT_HEIGHT, crate::ai::chat::resize::MAX_INPUT_HEIGHT);
            if let Some(ref mut data) = self.ai_chat_panel {
                data.input_height = new_height;
            }
            cx.notify();
            return;
        }

        // Handle active panel resize drag.
        if let Some(ref drag) = self.panel_resize_drag {
            let cursor_x: f32 = f32::from(event.position.x);
            let delta = cursor_x - drag.start_x;
            use crate::ai::chat::resize::{
                MAX_INPUT_HEIGHT, MAX_PANEL_WIDTH, MIN_INPUT_HEIGHT, MIN_PANEL_WIDTH,
            };
            let new_width = (drag.start_width - delta).clamp(MIN_PANEL_WIDTH, MAX_PANEL_WIDTH);
            match drag.target {
                Panel::AiChat => {
                    if let Some(ref mut data) = self.ai_chat_panel {
                        data.ai_panel_width = new_width;
                    }
                }
                Panel::SystemInfo => {
                    self.system_info_width = new_width;
                }
                Panel::None => {}
            }
            cx.notify();
            return;
        }

        if self.terminal_selecting {
            if event.pressed_button == Some(MouseButton::Left) {
                if self.update_terminal_selection(event.position) {
                    cx.notify();
                }
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
        if self.show_settings {
            return;
        }

        // End input height resize drag.
        if self.input_resize_drag.take().is_some() {
            if let Some(ref data) = self.ai_chat_panel {
                self.config.set_ai_input_height(data.input_height);
            }
            cx.notify();
            return;
        }

        // End panel resize drag.
        if let Some(drag) = self.panel_resize_drag.take() {
            match drag.target {
                Panel::AiChat => {
                    if let Some(ref data) = self.ai_chat_panel {
                        self.config.set_ai_panel_width(data.ai_panel_width);
                    }
                }
                Panel::SystemInfo => {
                    // system_info_width persists in memory for the session;
                    // config persistence added later if needed.
                }
                Panel::None => {}
            }
            cx.notify();
            return;
        }

        if self.terminal_selecting {
            self.terminal_selecting = false;
            cx.notify();
            return;
        }
        if self.tab.as_ref().is_some_and(|t| t.mouse_mode()) {
            self.send_mouse_to_pty(event.position, event.button, false, cx);
        }
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if self.show_settings {
            return;
        }
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
                    write_to_pty(
                        &self.backend,
                        &mut self.tab,
                        encode_mouse_scroll(row as u16, col as u16, up, false, false, false),
                    );
                }
            }
        } else if let Some(tab) = &mut self.tab {
            tab.scroll_history(f32::from(delta) as i32);
        }
    }

    // ── Mouse → PTY helpers ─────────────────────────────────────────────
    fn send_mouse_to_pty(
        &mut self,
        position: Point<Pixels>,
        button: MouseButton,
        pressed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((row, col)) = self.terminal_grid_point(position) else {
            return;
        };
        let btn = match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return,
        };
        write_to_pty(
            &self.backend,
            &mut self.tab,
            encode_mouse_event(row as u16, col as u16, btn, pressed, false, false, false),
        );
        cx.notify();
    }

    /// Drag event (button held during motion mode → SGR cb = 32 | btn_code).
    fn send_mouse_drag_to_pty(
        &mut self,
        position: Point<Pixels>,
        button: MouseButton,
        cx: &mut Context<Self>,
    ) {
        let Some(cell) = self.terminal_grid_point(position) else {
            return;
        };
        if self.last_mouse_cell == Some(cell) {
            return;
        }
        self.last_mouse_cell = Some(cell);
        let btn = match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return,
        };
        write_to_pty(
            &self.backend,
            &mut self.tab,
            encode_mouse_drag(cell.0 as u16, cell.1 as u16, btn, false, false, false),
        );
        cx.notify();
    }

    /// Pure motion event (no button → SGR cb = 35 = motion | release).
    fn send_mouse_motion_to_pty(&mut self, position: Point<Pixels>, _cx: &mut Context<Self>) {
        let Some(cell) = self.terminal_grid_point(position) else {
            return;
        };
        if self.last_mouse_cell == Some(cell) {
            return;
        }
        self.last_mouse_cell = Some(cell);
        write_to_pty(
            &self.backend,
            &mut self.tab,
            encode_mouse_motion(cell.0 as u16, cell.1 as u16, false, false, false),
        );
    }

    // ── Coordinate helpers ──────────────────────────────────────────────

    fn terminal_grid_point(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let bounds = self.terminal_bounds?;
        if !bounds.contains(&position) {
            return None;
        }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cw = self.terminal_cell_width;
        let lh = self.terminal_line_height;
        let snap = self.tab.as_ref()?.render_snapshot();
        let row = ((f32::from(local_y) / f32::from(lh)).floor() as usize)
            .min(snap.rows.saturating_sub(1));
        let col = ((f32::from(local_x) / f32::from(cw)).floor() as usize)
            .min(snap.cols.saturating_sub(1));
        Some((row, col))
    }

    fn terminal_grid_point_and_side(
        &self,
        position: Point<Pixels>,
    ) -> Option<(usize, usize, Side)> {
        let bounds = self.terminal_bounds?;
        if !bounds.contains(&position) {
            return None;
        }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cw = self.terminal_cell_width;
        let lh = self.terminal_line_height;
        let snap = self.tab.as_ref()?.render_snapshot();
        let col = ((f32::from(local_x) / f32::from(cw)).floor() as usize)
            .min(snap.cols.saturating_sub(1));
        let row = ((f32::from(local_y) / f32::from(lh)).floor() as usize)
            .min(snap.rows.saturating_sub(1));
        let side = if f32::from(local_x) % f32::from(cw) >= f32::from(cw) / 2.0 {
            Side::Right
        } else {
            Side::Left
        };
        Some((row, col, side))
    }

    // ── Text selection ──────────────────────────────────────────────────

    fn begin_terminal_selection(&mut self, event: &MouseDownEvent, _cx: &mut Context<Self>) {
        let stype = match event.click_count.max(1) {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        let Some((row, col, side)) = self.terminal_grid_point_and_side(event.position) else {
            return;
        };
        if let Some(tab) = &mut self.tab {
            tab.begin_selection(row, col, side, stype);
            self.terminal_selecting = true;
            self.last_selection_cell = None;
        }
    }

    fn update_terminal_selection(&mut self, position: Point<Pixels>) -> bool {
        let Some((row, col, side)) = self.terminal_grid_point_and_side(position) else {
            return false;
        };
        let cell = (row, col);
        if self.last_selection_cell == Some(cell) {
            return false;
        }
        self.last_selection_cell = Some(cell);
        if let Some(tab) = &mut self.tab {
            tab.update_selection(row, col, side);
        }
        true
    }

    fn refresh_mcp_data(&mut self, cx: &mut gpui::Context<Self>) {
        self.mcp_zenix_servers = crate::mcp::load_zenix_mcp_servers();
        self.mcp_per_agent = crate::mcp::scan_per_agent_mcp();
        cx.notify();
    }

    fn create_agent_search_input(
        w: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Entity<gpui_component::input::InputState> {
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(w, cx).placeholder("Search agents...")
        });
        cx.subscribe(
            &input,
            |this: &mut ZenixApp, input_state, event: &gpui_component::input::InputEvent, cx| {
                if let gpui_component::input::InputEvent::Change = event {
                    let val = input_state.read(cx).value().to_string();
                    this.agent_search = val;
                    cx.notify();
                }
            },
        )
        .detach();
        input
    }

    fn create_skill_search_input(
        w: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Entity<gpui_component::input::InputState> {
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(w, cx).placeholder("Search skills...")
        });
        cx.subscribe(
            &input,
            |this: &mut ZenixApp, input_state, event: &gpui_component::input::InputEvent, cx| {
                if let gpui_component::input::InputEvent::Change = event {
                    let val = input_state.read(cx).value().to_string();
                    this.skill_search = val;
                    cx.notify();
                }
            },
        )
        .detach();
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
            if let Some(binary) =
                herdr::find_herdr_binary().map(|p| p.to_string_lossy().to_string())
            {
                self.spawn_pty(&binary, &["client"], cx);
            }
        }
        if self.pending_clear_chat_input {
            if let Some(input) = self.chat_input_entity.as_ref() {
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            self.pending_clear_chat_input = false;
        }

        // Start sysinfo polling when system info panel is first opened
        if self.active_panel == Panel::SystemInfo && !self.sysinfo_polling {
            self.start_sysinfo_poll(cx);
        }

        let fs = self.terminal_font_size;
        let fs_px = px(fs);

        // Cell metrics from font, line height = Zed Standard (1.3x)
        let text_system = window.text_system();
        let font = gpui::Font {
            family: self.config.font_family().into(),
            ..gpui::Font::default()
        };
        let font_id = text_system.resolve_font(&font);
        let cw_px = text_system
            .advance(font_id, fs_px, 'm')
            .map(|s| s.width)
            .unwrap_or(fs_px * 0.6);
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
        let panel_w = match self.active_panel {
            Panel::AiChat => self
                .ai_chat_panel
                .as_ref()
                .map(|d| d.ai_panel_width.clamp(
                    crate::ai::chat::resize::MIN_PANEL_WIDTH,
                    crate::ai::chat::resize::MAX_PANEL_WIDTH,
                ))
                .unwrap_or(sidebar::AI_PANEL_WIDTH),
            Panel::SystemInfo => self.system_info_width,
            Panel::None => 0.0,
        };
        let sidebar_w = sidebar::SIDEBAR_WIDTH + panel_w;

        if let Some(tab) = &mut self.tab {
            let b = window.bounds();
            let avail_w = (f32::from(b.size.width) - sidebar_w).max(200.0);
            let cw = f32::from(cw_px);
            let lh = f32::from(lh_px);
            let cols = (avail_w / cw.max(1.0)).max(20.0) as u16;
            let rows = (f32::from(b.size.height) / lh.max(1.0)).max(10.0) as u16;
            if tab.cols != cols || tab.rows != rows {
                tab.resize(cols, rows);
            }
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
        let _light_themes: Vec<gpui::AnyElement> = registry
            .sorted_themes()
            .iter()
            .filter(|tc| tc.mode == gpui_component::ThemeMode::Light)
            .map(|tc| {
                let name = tc.name.to_string();
                let is_current = name == current_theme_name;
                let prefix = if is_current { "\u{2713} " } else { "  " };
                let cb: sidebar::ThemeCallback = {
                    let n = name.clone();
                    Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.switch_theme(&n, window, cx);
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
        let _dark_themes: Vec<gpui::AnyElement> = registry
            .sorted_themes()
            .iter()
            .filter(|tc| tc.mode == gpui_component::ThemeMode::Dark)
            .map(|tc| {
                let name = tc.name.to_string();
                let is_current = name == current_theme_name;
                let prefix = if is_current { "\u{2713} " } else { "  " };
                let cb: sidebar::ThemeCallback = {
                    let n = name.clone();
                    Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.switch_theme(&n, window, cx);
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

        // Callbacks for sidebar buttons (Box<dyn Fn> works with .on_click)
        let sysinfo_cb: sidebar::ThemeCallback =
            Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.toggle_panel(Panel::SystemInfo);
                cx.notify();
            }));
        let ai_cb: sidebar::ThemeCallback =
            Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.toggle_panel(Panel::AiChat);
                cx.notify();
            }));
        let settings_cb: sidebar::ThemeCallback =
            Box::new(cx.listener(|this, _: &ClickEvent, window, cx| {
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
                    let selected_idx = this
                        .font_names
                        .iter()
                        .position(|n| *n == current)
                        .map(|i| gpui_component::IndexPath::new(i));
                    let delegate = gpui_component::searchable_list::SearchableVec::new(
                        this.font_names.clone(),
                    );
                    let font_select = cx.new(|cx| {
                        FontSelect::new(delegate, selected_idx, window, cx).searchable(true)
                    });
                    // Subscribe to font selection confirm
                    cx.subscribe_in(
                        &font_select,
                        window,
                        |this,
                         _font_sel,
                         event: &gpui_component::select::SelectEvent<
                            gpui_component::searchable_list::SearchableVec<String>,
                        >,
                         _window,
                         cx| {
                            if let gpui_component::select::SelectEvent::Confirm(Some(font_name)) =
                                event
                            {
                                this.config.set_font_family(font_name.clone());
                                let _ = this.config.save();
                                let theme = gpui_component::Theme::global_mut(cx);
                                theme.font_family = font_name.into();
                            }
                            cx.notify();
                        },
                    )
                    .detach();
                    this.font_select = Some(font_select);
                }
                this.show_settings = !this.show_settings;
                cx.notify();
            }));
        let font_down_cb: sidebar::ThemeCallback =
            Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.change_font_size(this.terminal_font_size - 1.0);
                cx.notify();
            }));
        let font_up_cb: sidebar::ThemeCallback =
            Box::new(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.change_font_size(this.terminal_font_size + 1.0);
                cx.notify();
            }));
        // Expanded panel (conditionally shown) — SystemInfo or AiChat, Settings is a dialog
        let panel: Option<gpui::AnyElement> = match self.active_panel {
            Panel::SystemInfo => {
                Some(
                    sidebar::system_info_panel(&theme, &self.system_info, self.system_info_width)
                        .into_any_element(),
                )
            }
            Panel::AiChat => {
                // Check if there is at least one configured API Provider.
                // If none, show a friendly placeholder instead of a blank panel.
                if !crate::ai::engine::OrchestratorEngine::has_available_provider() {
                    Some(render_ai_config_empty(&theme).into_any_element())
                } else {
                    let data = self
                        .ai_chat_panel
                        .get_or_insert_with(|| {
                            let mut d = crate::ai::chat::AiChatPanelData::new();
                            d.ai_panel_width = self.config.ai_panel_width().clamp(360.0, 720.0);
                            d.input_height = self.config.ai_input_height().clamp(60.0, 400.0);
                            d
                        });
                    // Lazy-create the InputState entity for the chat input.
                    if self.chat_input_entity.is_none() {
                        let input = cx.new(|cx| {
                            gpui_component::input::InputState::new(window, cx)
                                .placeholder("Ask anything, dispatch agents, or type / for commands")
                                .multi_line(true)
                                .auto_grow(1, 12)
                        });
                        // Subscribe to input events: Enter sends, Escape clears focus.
                        cx.subscribe(&input, {
                            let _entity = cx.entity().clone();
                            move |this: &mut ZenixApp,
                                  _input: gpui::Entity<gpui_component::input::InputState>,
                                  event: &gpui_component::input::InputEvent,
                                  cx| {
                                match event {
                                    gpui_component::input::InputEvent::PressEnter {
                                        shift: false,
                                        ..
                                    } => {
                                        let text = _input.read(cx).value().to_string();
                                        if !text.trim().is_empty() {
                                            if let Some(ref mut data) = this.ai_chat_panel {
                                                if data.input_history.last() != Some(&text) {
                                                    data.input_history.push(text.clone());
                                                }
                                                data.input_history_idx = None;
                                            }
                                            start_chat_streaming(this, text, cx);
                                            // Input is cleared from render() via
                                            // pending_clear_chat_input, where the
                                            // window handle is available.
                                            this.pending_clear_chat_input = true;
                                        }
                                        cx.notify();
                                    }
                                    gpui_component::input::InputEvent::PressEnter {
                                        shift: true,
                                        ..
                                    } => {
                                        // Shift+Enter: newline (handled by multi_line mode).
                                    }
                                    gpui_component::input::InputEvent::Change => {
                                        if let Some(ref mut data) = this.ai_chat_panel {
                                            data.input_value = _input.read(cx).value().to_string();
                                        }
                                        cx.notify();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .detach();
                        self.chat_input_entity = Some(input);
                    }
                    if data.available_providers.is_empty() {
                        data.available_providers = crate::api_provider::load_all()
                            .into_iter()
                            .map(|p| p.name)
                            .collect();
                    }
                    // Fetch available models from the selected provider (or first
                    // provider with an api_key) when the list is empty and we
                    // haven't already attempted a fetch this session.
                    if data.available_models.is_empty() && !data.models_fetch_attempted {
                        data.models_fetch_attempted = true;
                        let providers = crate::api_provider::load_all();
                        let provider = providers
                            .iter()
                            .find(|p| {
                                !data.selected_provider_id.is_empty()
                                    && p.name == data.selected_provider_id
                            })
                            .or_else(|| providers.iter().find(|p| !p.api_key.trim().is_empty()))
                            .filter(|p| !p.api_key.trim().is_empty())
                            .cloned();
                        if let Some(provider) = provider {
                            let provider_name = provider.name.clone();
                            let entity = cx.entity().clone();
                            cx.spawn(async move |_this, cx| {
                                let provider_for_fetch = provider.clone();
                                let models = std::thread::spawn(move || {
                                    crate::api_provider::fetch_models(&provider_for_fetch)
                                })
                                .join()
                                .unwrap_or_else(|_| Err("thread panic".into()));
                                match &models {
                                    Ok(list) => tracing::info!(
                                        target: "zenixai",
                                        provider = %provider.name,
                                        count = list.len(),
                                        "fetched models"
                                    ),
                                    Err(e) => tracing::warn!(
                                        target: "zenixai",
                                        provider = %provider.name,
                                        error = %e,
                                        "fetch models failed"
                                    ),
                                }
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(ref mut data) = this.ai_chat_panel {
                                        if data.selected_provider_id.is_empty() {
                                            data.selected_provider_id = provider_name;
                                        }
                                        if let Ok(list) = &models {
                                            data.available_models = list.clone();
                                            if data.selected_model.is_empty() {
                                                data.selected_model = data
                                                    .available_models
                                                    .first()
                                                    .cloned()
                                                    .unwrap_or_default();
                                            }
                                        }
                                    }
                                    cx.notify();
                                });
                            })
                            .detach();
                        }
                    }
                    // Start agent polling if not already running.
                    if !self.agent_polling {
                        self.agent_polling = true;
                        cx.spawn(async move |this, cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_secs(2)).await;
                                // Try to connect to herdr and list agents.
                                let result = std::thread::spawn(|| {
                                    let mut client = crate::ai::herdr::HerdrIpcClient::new(
                                        crate::client::herdr_socket_path(),
                                    );
                                    client.list_agents().ok()
                                })
                                .join();
                                if let Ok(Some(agents)) = result {
                                    let _ = this.update(cx, |this, cx| {
                                        if let Some(ref mut data) = this.ai_chat_panel {
                                            data.agent_statuses = agents
                                                .iter()
                                                .map(|a| {
                                                    let output =
                                                        crate::ai::herdr::HerdrIpcClient::new(
                                                            crate::client::herdr_socket_path(),
                                                        )
                                                        .read_agent(
                                                            &a.terminal_id,
                                                            crate::ai::herdr::ReadSource::Recent,
                                                            3,
                                                        )
                                                        .unwrap_or_default();
                                                    crate::ai::chat::AgentMonitorEntry::from_herdr(
                                                        a.agent
                                                            .clone()
                                                            .unwrap_or_else(|| "agent".into()),
                                                        a.pane_id.clone(),
                                                        a.agent_status,
                                                        output,
                                                    )
                                                })
                                                .collect();
                                            data.active_agent_count = data
                                                .agent_statuses
                                                .iter()
                                                .filter(|a| {
                                                    a.status != "done" && a.status != "idle"
                                                })
                                                .count();
                                        }
                                        cx.notify();
                                    });
                                } else {
                                    // Connection failed — stop polling, will restart when panel reopens.
                                    let _ = this.update(cx, |this, _| {
                                        this.agent_polling = false;
                                    });
                                    break;
                                }
                            }
                        })
                        .detach();
                    }
                    let input_ref = self.chat_input_entity.as_ref();
                    let entity = cx.entity();
                    let on_action: crate::ai::chat::AiChatActionHandler =
                        std::rc::Rc::new(move |action, window, cx| {
                            let entity = entity.clone();
                            let _ = entity.update(cx, |this, cx| {
                                handle_ai_chat_action(this, action, window, cx);
                            });
                        });
                    Some(
                        crate::ai::chat::render_ai_chat_panel(data, &theme, input_ref, on_action)
                            .into_any_element(),
                    )
                }
            }
            _ => None,
        };

        // Terminal area content
        let terminal_area = match (snapshot, backend) {
            (Some(snapshot), Some(_backend)) => {
                let app_entity = cx.entity().clone();
                let ime = self.ime_state.clone();
                div()
                    .size_full()
                    .relative()
                    .on_prepaint({
                        let view = cx.entity().clone();
                        move |bounds, _window, cx| {
                            view.update(cx, |this, _| {
                                this.terminal_bounds = Some(bounds);
                            });
                        }
                    })
                    .child(TerminalElement::new(
                        snapshot,
                        app_entity,
                        focus2,
                        self.config.font_family(),
                        px(fs),
                        lh_px,
                        cw_px,
                        self.terminal_ascent,
                        self.terminal_descent,
                        ime,
                        self.terminal_opacity,
                    ))
                    .into_any_element()
            }
            _ => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_6()
                .child(div().text_size(px(24.)).child(t("landing.title")))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme.muted_foreground)
                        .child(self.status.clone()),
                )
                .child(
                    Button::new("term")
                        .primary()
                        .label(t("landing.terminal_btn"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.spawn_pty(crate::platform::default_shell(), &[], cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("herdr")
                        .ghost()
                        .label(t("landing.herdr_btn"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.launch_attempts = 0;
                            this.connect_attempted = false;
                            this.connect_herdr(cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        };
        let main_ui = h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(self.config.font_family())
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_tab_to_pty))
            .on_action(cx.listener(Self::on_shift_tab_to_pty))
            .on_action(cx.listener(Self::on_chat_history_up))
            .on_action(cx.listener(Self::on_chat_history_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(div().flex_grow(1.0).h_full().child(terminal_area))
            .children(panel)
            .child(sidebar::sidebar(
                &theme,
                self.active_panel,
                self.show_settings,
                sysinfo_cb,
                ai_cb,
                settings_cb,
            ));

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
                                    cx.subscribe(
                                        &input,
                                        |this,
                                         _input,
                                         event: &gpui_component::input::InputEvent,
                                         cx| {
                                            if let gpui_component::input::InputEvent::PressEnter {
                                                shift: false,
                                                ..
                                            } = event
                                            {
                                                let url = _input.read(cx).value().to_string();
                                                if !url.trim().is_empty() {
                                                    match crate::skills::install_from_git(
                                                        url.trim(),
                                                    ) {
                                                        Ok(name) => tracing::info!(
                                                            "installed skill: {name}"
                                                        ),
                                                        Err(e) => {
                                                            tracing::warn!("install failed: {e}")
                                                        }
                                                    }
                                                    this.skills = crate::skills::scan_all_skills();
                                                }
                                                this.dialog_input = None;
                                                this.dialog_input_mode = InputDialogMode::Hidden;
                                                cx.notify();
                                            }
                                        },
                                    )
                                    .detach();
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
                                    cx.subscribe(
                                        &input,
                                        |this,
                                         _input,
                                         event: &gpui_component::input::InputEvent,
                                         cx| {
                                            if let gpui_component::input::InputEvent::PressEnter {
                                                shift: false,
                                                ..
                                            } = event
                                            {
                                                let path = _input.read(cx).value().to_string();
                                                if !path.trim().is_empty() {
                                                    match crate::skills::install_from_local(
                                                        path.trim(),
                                                    ) {
                                                        Ok(name) => tracing::info!(
                                                            "installed skill: {name}"
                                                        ),
                                                        Err(e) => {
                                                            tracing::warn!("install failed: {e}")
                                                        }
                                                    }
                                                    this.skills = crate::skills::scan_all_skills();
                                                }
                                                this.dialog_input = None;
                                                this.dialog_input_mode = InputDialogMode::Hidden;
                                                cx.notify();
                                            }
                                        },
                                    )
                                    .detach();
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
                                this.pending_confirm =
                                    Some(PendingConfirm::RemoveSkill(skill_name));
                                cx.notify();
                            }
                            SkillAction::FetchMarket => {
                                this.market_loading = true;
                                this.market_error = None;
                                cx.notify();
                                let entity = entity.clone();
                                cx.spawn(async move |_, cx| {
                                    let result =
                                        std::thread::spawn(|| crate::skills::fetch_leaderboard())
                                            .join();
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
                                })
                                .detach();
                            }
                            SkillAction::InstallApi { skill_id } => {
                                let entity = entity.clone();
                                cx.spawn(async move |_, cx| {
                                    let result = std::thread::spawn(move || {
                                        let install_result =
                                            crate::skills::install_from_api(&skill_id);
                                        (install_result, crate::skills::scan_all_skills())
                                    })
                                    .join();
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
                                })
                                .detach();
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
                                    gpui_component::input::InputState::new(w, cx)
                                        .placeholder("server-name")
                                });
                                let command_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx)
                                        .placeholder("npx -y @server/name")
                                });
                                let args_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx)
                                        .placeholder("--port 3000")
                                });
                                let env_input = cx.new(|cx| {
                                    gpui_component::input::InputState::new(w, cx)
                                        .placeholder("KEY=value")
                                });
                                let form = McpFormState {
                                    editing_name: None,
                                    name: Some(name_input),
                                    command: Some(command_input),
                                    args: Some(args_input),
                                    env: Some(env_input),
                                    url: None,
                                    mcp_type: "stdio".to_string(),
                                    original_command: None,
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
                                    let name_val = form
                                        .name
                                        .as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let cmd_val = form
                                        .command
                                        .as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let args_val = form
                                        .args
                                        .as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let env_val = form
                                        .env
                                        .as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let url_val = form
                                        .url
                                        .as_ref()
                                        .map(|e| e.read(cx).value().to_string())
                                        .unwrap_or_default();
                                    let mcp_type = if form.mcp_type == "sse" {
                                        crate::mcp::McpType::Sse
                                    } else {
                                        crate::mcp::McpType::Stdio
                                    };
                                    // For SSE, use URL as command; for stdio, use command field
                                    // If editing and switching from SSE to stdio, restore original command
                                    let effective_cmd = if mcp_type == crate::mcp::McpType::Sse {
                                        url_val.trim().to_string()
                                    } else if mcp_type == crate::mcp::McpType::Stdio
                                        && form.original_command.is_some()
                                    {
                                        // User switched from SSE back to stdio — restore original command
                                        form.original_command.unwrap()
                                    } else {
                                        cmd_val.trim().to_string()
                                    };
                                    if !name_val.trim().is_empty() && !effective_cmd.is_empty() {
                                        let args: Vec<String> = if args_val.trim().is_empty() {
                                            Vec::new()
                                        } else {
                                            args_val.split_whitespace().map(String::from).collect()
                                        };
                                        let env: std::collections::HashMap<String, String> =
                                            if env_val.trim().is_empty() {
                                                std::collections::HashMap::new()
                                            } else {
                                                env_val
                                                    .split_whitespace()
                                                    .filter_map(|pair| {
                                                        let mut parts = pair.splitn(2, '=');
                                                        let k = parts.next()?.trim().to_string();
                                                        let v = parts.next()?.trim().to_string();
                                                        if k.is_empty() {
                                                            None
                                                        } else {
                                                            Some((k, v))
                                                        }
                                                    })
                                                    .collect()
                                            };
                                        let (server, old_enabled) = if let Some(old_name) =
                                            &form.editing_name
                                        {
                                            let old = this
                                                .mcp_zenix_servers
                                                .iter()
                                                .find(|s| s.name == *old_name);
                                            let old_agents = old
                                                .map(|s| s.enabled_agents.clone())
                                                .unwrap_or_default();
                                            let _ = crate::mcp::remove_zenix_mcp_server(old_name);
                                            // Create new server with preserved enabled agents
                                            (
                                                crate::mcp::ZenixMcpServer {
                                                    name: name_val.trim().to_string(),
                                                    mcp_type,
                                                    command: effective_cmd,
                                                    args,
                                                    env,
                                                    enabled_agents: old_agents.clone(),
                                                },
                                                true,
                                            )
                                        } else {
                                            (
                                                crate::mcp::ZenixMcpServer {
                                                    name: name_val.trim().to_string(),
                                                    mcp_type,
                                                    command: effective_cmd,
                                                    args,
                                                    env,
                                                    enabled_agents: vec![],
                                                },
                                                false,
                                            )
                                        };
                                        if let Err(e) = crate::mcp::add_zenix_mcp_server(&server) {
                                            this.toast = Some(ToastMessage {
                                                text: format!("Failed to add MCP server: {e}"),
                                                msg_type: "error".into(),
                                            });
                                        }
                                        if old_enabled {
                                            for agent in &server.enabled_agents {
                                                let _ = crate::mcp::sync_zenix_server_to_agent(
                                                    &server, agent, true,
                                                );
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
                                            gpui_component::input::InputState::new(w, cx)
                                                .placeholder("http://localhost:3000/sse")
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
                                let server = this
                                    .mcp_zenix_servers
                                    .iter()
                                    .find(|s| s.name == *name)
                                    .cloned();
                                if let Some(s) = server {
                                    let name_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx)
                                            .placeholder("server-name");
                                        st.set_value(&s.name, w, cx);
                                        st
                                    });
                                    let command_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx)
                                            .placeholder("npx -y @server/name");
                                        st.set_value(&s.command, w, cx);
                                        st
                                    });
                                    let args_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx)
                                            .placeholder("--port 3000");
                                        st.set_value(s.args.join(" "), w, cx);
                                        st
                                    });
                                    let env_input = cx.new(|cx| {
                                        let mut st = gpui_component::input::InputState::new(w, cx)
                                            .placeholder("KEY=value");
                                        st.set_value(
                                            s.env
                                                .iter()
                                                .map(|(k, v)| format!("{k}={v}"))
                                                .collect::<Vec<_>>()
                                                .join(" "),
                                            w,
                                            cx,
                                        );
                                        st
                                    });
                                    let url_input = if s.mcp_type == crate::mcp::McpType::Sse {
                                        Some(cx.new(|cx| {
                                            let mut st =
                                                gpui_component::input::InputState::new(w, cx)
                                                    .placeholder("http://localhost:3000/sse");
                                            st.set_value(&s.command, w, cx);
                                            st
                                        }))
                                    } else {
                                        None
                                    };
                                    // Preserve original command when switching to SSE mode
                                    let original_command = if s.mcp_type == crate::mcp::McpType::Sse
                                    {
                                        Some(s.command.clone())
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
                                        original_command,
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
                                let server = this
                                    .mcp_zenix_servers
                                    .iter()
                                    .find(|s| s.name == name)
                                    .cloned();
                                if let Some(srv) = server {
                                    let result = match crate::mcp::test_mcp_server(&srv) {
                                        Ok(msg) => format!("OK: {msg}"),
                                        Err(e) => format!("ERR: {e}"),
                                    };
                                    this.mcp_test_results.insert(name.clone(), result);
                                } else {
                                    this.mcp_test_results
                                        .insert(name.clone(), "ERR: Server not found".into());
                                }
                                let toast_msg = this
                                    .mcp_test_results
                                    .get(&name)
                                    .cloned()
                                    .unwrap_or_default();
                                let toast_type = if toast_msg.starts_with("OK:") {
                                    "success"
                                } else {
                                    "error"
                                };
                                this.toast = Some(ToastMessage {
                                    text: toast_msg,
                                    msg_type: toast_type.into(),
                                });
                                cx.notify();
                            }
                            McpAction::TestAgentServer { name, agent } => {
                                let entry = this
                                    .mcp_per_agent
                                    .iter()
                                    .find(|(a, _)| a == &agent)
                                    .and_then(|(_, entries)| {
                                        entries.iter().find(|e| e.name == name)
                                    })
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
                                let toast_type = if result.starts_with("OK:") {
                                    "success"
                                } else {
                                    "error"
                                };
                                this.toast = Some(ToastMessage {
                                    text: result,
                                    msg_type: toast_type.into(),
                                });
                                cx.notify();
                            }
                        }
                    });
                }
            });

            let on_api_action: ActionCb<ApiProviderAction> = Box::new({
                let entity = entity.clone();
                move |action: ApiProviderAction, w: &mut Window, app: &mut gpui::App| {
                    entity.update(app, |this: &mut ZenixApp, cx| match action {
                        ApiProviderAction::ShowAddForm => {
                            let name_input = cx.new(|cx| {
                                gpui_component::input::InputState::new(w, cx)
                                    .placeholder("my-provider")
                            });
                            let base_url_input = cx.new(|cx| {
                                gpui_component::input::InputState::new(w, cx)
                                    .placeholder("https://api.openai.com")
                            });
                            let api_key_input = cx.new(|cx| {
                                gpui_component::input::InputState::new(w, cx).placeholder("sk-...")
                            });
                            let suffix_input = cx.new(|cx| {
                                gpui_component::input::InputState::new(w, cx)
                                    .placeholder("/v1/chat/completions")
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
                                locked_name: None,
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
                                let name_val = form
                                    .name
                                    .as_ref()
                                    .map(|e| e.read(cx).value().to_string())
                                    .unwrap_or_default();
                                let base_val = form
                                    .base_url
                                    .as_ref()
                                    .map(|e| e.read(cx).value().to_string())
                                    .unwrap_or_default();
                                let key_val = form
                                    .api_key
                                    .as_ref()
                                    .map(|e| e.read(cx).value().to_string())
                                    .unwrap_or_default();
                                let suffix_val = form
                                    .url_suffix
                                    .as_ref()
                                    .map(|e| e.read(cx).value().to_string())
                                    .unwrap_or_default();
                                if name_val.trim().is_empty() {
                                    this.toast = Some(ToastMessage {
                                        text: "Provider name is required".into(),
                                        msg_type: "error".into(),
                                    });
                                    this.api_provider_form = Some(form);
                                    cx.notify();
                                    return;
                                }
                                if !base_val.trim().starts_with("http://")
                                    && !base_val.trim().starts_with("https://")
                                {
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
                                    this.api_providers
                                        .iter()
                                        .find(|p| {
                                            p.name == form.editing_name.as_deref().unwrap_or("")
                                        })
                                        .map(|p| p.enabled_agents.clone())
                                        .unwrap_or_default()
                                } else {
                                    vec![]
                                };
                                // If editing, remove the old provider first (in case name changed)
                                if let Some(old_name) = &form.editing_name {
                                    if old_name != &name_val {
                                        let _ = crate::api_provider::remove(old_name);
                                    }
                                }
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
                                // Lock name + URL fields for built-in presets (e.g. Zenix)
                                // so users can only fill in the API key.
                                if preset.name == "Zenix" {
                                    form.locked_name = Some("Zenix".to_string());
                                } else {
                                    form.locked_name = None;
                                }
                            }
                            cx.notify();
                        }
                        ApiProviderAction::Edit { name } => {
                            if let Some(cfg) =
                                this.api_providers.iter().find(|p| p.name == name).cloned()
                            {
                                let name_input = cx.new(|cx| {
                                    let mut st = gpui_component::input::InputState::new(w, cx)
                                        .placeholder("name");
                                    st.set_value(&cfg.name, w, cx);
                                    st
                                });
                                let base_url_input = cx.new(|cx| {
                                    let mut st = gpui_component::input::InputState::new(w, cx)
                                        .placeholder("url");
                                    st.set_value(&cfg.base_url, w, cx);
                                    st
                                });
                                let api_key_input = cx.new(|cx| {
                                    let mut st = gpui_component::input::InputState::new(w, cx)
                                        .placeholder("key");
                                    st.set_value(&cfg.api_key, w, cx);
                                    st
                                });
                                let suffix_input = cx.new(|cx| {
                                    let mut st = gpui_component::input::InputState::new(w, cx)
                                        .placeholder("suffix");
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
                                    locked_name: None,
                                });
                            }
                            cx.notify();
                        }
                        ApiProviderAction::Remove { name } => {
                            this.pending_confirm = Some(PendingConfirm::RemoveApi(name));
                            cx.notify();
                        }
                        ApiProviderAction::FetchModels { name } => {
                            if let Some(cfg) =
                                this.api_providers.iter().find(|p| p.name == name).cloned()
                            {
                                let models = std::thread::spawn(move || {
                                    crate::api_provider::fetch_models(&cfg)
                                })
                                .join()
                                .unwrap_or_else(|_| Err("thread panic".into()));
                                match models {
                                    Ok(list) => {
                                        if let Some(form) = &mut this.api_provider_form {
                                            form.available_models = list;
                                            if form.selected_model.is_empty() {
                                                form.selected_model = form
                                                    .available_models
                                                    .first()
                                                    .cloned()
                                                    .unwrap_or_default();
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
                            if let Some(prov) =
                                this.api_providers.iter_mut().find(|p| p.name == name)
                            {
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
                                this.api_test_results
                                    .insert(name.clone(), "ERR: Provider not found".into());
                            }
                            let toast_msg = this
                                .api_test_results
                                .get(&name)
                                .cloned()
                                .unwrap_or_default();
                            let toast_type = if toast_msg.starts_with("OK:") {
                                "success"
                            } else {
                                "error"
                            };
                            this.toast = Some(ToastMessage {
                                text: toast_msg,
                                msg_type: toast_type.into(),
                            });
                            cx.notify();
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
            let scrollback_input = cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .default_value(self.terminal_scrollback.to_string())
            });
            // Subscribe to scrollback input for Enter/blur handling
            cx.subscribe(&scrollback_input, {
                let _entity = entity.clone();
                move |this: &mut ZenixApp,
                      _input: gpui::Entity<gpui_component::input::InputState>,
                      event: &gpui_component::input::InputEvent,
                      cx| {
                    let save_value = match event {
                        gpui_component::input::InputEvent::PressEnter { shift: false, .. } => true,
                        gpui_component::input::InputEvent::Blur => true,
                        _ => false,
                    };
                    if save_value {
                        let val = _input.read(cx).value().to_string();
                        if let Ok(n) = val.parse::<u32>() {
                            let bounded = n.min(100000).max(100);
                            let aligned = (bounded / 100) * 100;
                            this.terminal_scrollback = aligned;
                            this.config.set_terminal_scrollback(aligned);
                            let _ = this.config.save();
                            cx.notify();
                        }
                    }
                }
            })
            .detach();

            div()
                .relative()
                .size_full()
                .child(main_ui)
                .child(super::ui::settings_dialog(SettingsDialogProps {
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
                                this.show_settings = false;
                                this.toast = None; // Clear toast when settings dialog closes
                                this.pending_confirm = None; // Also clear pending confirm
                                cx.notify();
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
                    active_skills_subtab: self.active_skills_subtab,
                    skill_search: &self.skill_search,
                    skill_search_input: self.skill_search_input.as_ref(),
                    market_skills: &self.market_skills,
                    market_loading: self.market_loading,
                    market_error: self.market_error.as_deref(),
                    terminal_cursor_blink: self.terminal_cursor_blink,
                    terminal_cursor_style: &self.terminal_cursor_style,
                    terminal_opacity: self.terminal_opacity,
                    terminal_scrollback: self.terminal_scrollback,
                    scrollback_input: Some(&scrollback_input),
                    on_terminal_action: Box::new({
                        let entity = entity.clone();
                        move |action: crate::ui::settings::TerminalAction,
                              _w: &mut Window,
                              app: &mut gpui::App| {
                            entity.update(app, |this: &mut ZenixApp, cx| {
                                match action {
                                    crate::ui::settings::TerminalAction::SetCursorStyle(s) => {
                                        this.terminal_cursor_style = s.clone();
                                        this.config.set_terminal_cursor_style(s);
                                    }
                                    crate::ui::settings::TerminalAction::ToggleCursorBlink => {
                                        this.terminal_cursor_blink = !this.terminal_cursor_blink;
                                        this.config
                                            .set_terminal_cursor_blink(this.terminal_cursor_blink);
                                        // Restart blink timer if enabled
                                        if this.terminal_cursor_blink {
                                            cx.spawn(async move |this, cx| {
                                                loop {
                                                    cx.background_executor()
                                                        .timer(Duration::from_millis(500))
                                                        .await;
                                                    if this
                                                        .update(cx, |this, cx| {
                                                            if !this.terminal_cursor_blink {
                                                                this.cursor_visible = true;
                                                                return true;
                                                            }
                                                            this.cursor_visible =
                                                                !this.cursor_visible;
                                                            cx.notify();
                                                            false
                                                        })
                                                        .is_err()
                                                        || this
                                                            .update(cx, |this, _| {
                                                                this.terminal_cursor_blink
                                                            })
                                                            .unwrap_or(true)
                                                            == false
                                                    {
                                                        break;
                                                    }
                                                }
                                            })
                                            .detach();
                                        }
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
                                                this.toast = Some(ToastMessage {
                                                    text: format!("Removed {name}"),
                                                    msg_type: "success".into(),
                                                });
                                            }
                                            PendingConfirm::RemoveSkill(name) => {
                                                let _ = crate::skills::remove_zenix_skill(&name);
                                                this.skills = crate::skills::scan_all_skills();
                                                this.toast = Some(ToastMessage {
                                                    text: format!("Removed {name}"),
                                                    msg_type: "success".into(),
                                                });
                                            }
                                            PendingConfirm::RemoveApi(name) => {
                                                let _ = crate::api_provider::remove(&name);
                                                this.api_providers =
                                                    crate::api_provider::load_all();
                                                this.toast = Some(ToastMessage {
                                                    text: format!("Removed {name}"),
                                                    msg_type: "success".into(),
                                                });
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
                    theme: &theme,
                }))
                .into_any_element()
        } else {
            main_ui.into_any_element()
        }
    }
}

// ── AI Panel: Config Empty Placeholder ──────────────────────────────────

// ── Chat streaming control (Phase 2 of ZenixAI structural redesign) ────────
//
// These three free functions are wired into the chat panel from
// `ZenixApp::render` (send_click_cb) and the InputState subscribe closure
// (PressEnter). They own the entire chat-stream lifecycle: pushing the user
// message, spawning the engine task, processing stream events, and aborting.

/// Begin a streaming chat turn.
///
/// Synchronously pushes the user message + an empty assistant placeholder into
/// `data.messages`, sets streaming state, then spawns an async task that calls
/// `OrchestratorEngine::chat_and_dispatch`. Stream events arrive via a tokio
/// mpsc channel and are folded back into `data.messages[streaming_msg_idx]`.
fn start_chat_streaming(this: &mut ZenixApp, text: String, cx: &mut gpui::Context<'_, ZenixApp>) {
    use crate::ai::engine::{ChatMessageRef, DispatchStreamEvent, OrchestratorEngine};
    use crate::ai::herdr::HerdrIpcClient;

    if this
        .ai_chat_panel
        .as_ref()
        .map_or(false, |d| d.is_streaming)
    {
        return;
    }

    // 1. Push user message + empty assistant placeholder.
    let streaming_msg_idx = {
        let Some(ref mut data) = this.ai_chat_panel else {
            return;
        };

        // Auto-create a session on first message — current_session_id is None
        // on panel open and was never set, so without this the entire user
        // message push (including the visible `data.messages.push`) was
        // skipped, leaving the user with a blank panel after pressing Enter.
        if data.current_session_id.is_none() {
            if let Ok(mgr) = crate::ai::session::SessionManager::open() {
                let now = timestamp_now();
                let session_id = uuid::Uuid::new_v4().to_string();
                let title: String = text.chars().take(40).collect();
                let session = crate::ai::session::Session {
                    id: session_id.clone(),
                    title: title.clone(),
                    created_at: now.clone(),
                    updated_at: now,
                    workspace: data.selected_workspace.clone(),
                    provider_id: data.selected_provider_id.clone(),
                    model: data.selected_model.clone(),
                    thinking_level: format!("{:?}", data.thinking_level).to_lowercase(),
                };
                if mgr.create_session(&session).is_ok() {
                    tracing::info!(
                        target: "zenixai",
                        session_id = %session_id,
                        "auto-created session"
                    );
                    data.current_session_id = Some(session_id);
                    data.current_session_title = Some(title);
                    data.history_sessions.insert(0, session);
                }
            }
        }

        let session_id = data.current_session_id.clone().unwrap_or_default();
        let next_seq: i64 = {
            crate::ai::session::SessionManager::open()
                .ok()
                .and_then(|mgr| mgr.get_messages(&session_id).ok())
                .map(|msgs| msgs.len() as i64 + 1)
                .unwrap_or(0)
        };

        let user_msg = crate::ai::session::ChatMessage {
            id: 0,
            session_id: session_id.clone(),
            role: "user".into(),
            content: text.clone(),
            agent_activities: None,
            timestamp: timestamp_now(),
            sequence: next_seq,
        };
        if !session_id.is_empty() {
            if let Ok(mgr) = crate::ai::session::SessionManager::open() {
                let _ = mgr.add_message(&user_msg);
            }
        }
        data.push_message(user_msg);

        data.input_value = text;
        data.last_user_prompt = Some(data.input_value.clone());
        data.is_streaming = true;

        // Empty assistant message — tokens will append into `content`.
        let assistant_msg = crate::ai::session::ChatMessage {
            id: 0,
            session_id,
            role: "assistant".into(),
            content: String::new(),
            agent_activities: None,
            timestamp: timestamp_now(),
            sequence: next_seq + 1,
        };
        let streaming_idx = data.push_message(assistant_msg);
        streaming_idx
    };

    // 2. Snapshot data needed by the async task before yielding.
    let selected_provider_id = this
        .ai_chat_panel
        .as_ref()
        .map(|d| d.selected_provider_id.clone())
        .unwrap_or_default();
    let selected_model = this
        .ai_chat_panel
        .as_ref()
        .map(|d| d.selected_model.clone())
        .unwrap_or_default();
    let providers = crate::api_provider::load_all();
    let provider = providers
        .iter()
        .find(|p| !selected_provider_id.is_empty() && p.name == selected_provider_id)
        .or_else(|| providers.first())
        .cloned()
        .map(|mut p| {
            if !selected_model.is_empty() {
                p.model = selected_model;
            }
            p
        });
    let provider = match provider {
        Some(p) => p,
        None => {
            // No provider — append error to the empty assistant msg.
            if let Some(ref mut data) = this.ai_chat_panel {
                if let Some(idx) = data.streaming_msg_idx.or(Some(streaming_msg_idx)) {
                    data.set_message_content(
                        idx,
                        "⚠️ No API provider configured. Open Settings → API Providers.",
                    );
                }
                data.is_streaming = false;
                data.streaming_msg_idx = None;
            }
            cx.notify();
            return;
        }
    };
    let workspace_id = this
        .ai_chat_panel
        .as_ref()
        .and_then(|d| d.selected_workspace.clone())
        .unwrap_or_else(|| "default".to_string());
    let socket_path = crate::client::herdr_socket_path();
    let messages_for_engine: Vec<ChatMessageRef> = this
        .ai_chat_panel
        .as_ref()
        .map(|d| {
            d.messages
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .map(|m| ChatMessageRef {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    // 3. Persist streaming_msg_idx into the panel data so handle_dispatch_event
    //    knows which message to append to.
    if let Some(ref mut data) = this.ai_chat_panel {
        data.streaming_msg_idx = Some(streaming_msg_idx);
    }

    // 4. Spawn the streaming task. Uses mpsc to bridge tokio-side callback →
    //    GPUI-side entity updates, and tokio::select! for cooperative abort.
    cx.spawn(async move |this, cx| {
        // 4a. Abort channel — sender goes to entity state for stop_chat_streaming.
        let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
        let _ = this.update(cx, |this, _| {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.abort_handle = Some(abort_tx);
            }
        });

        // 4b. Event channel — chat_and_dispatch's callback runs on a tokio
        //     worker thread; we forward events here and process them on the
        //     GPUI async task so they can call `this.update(cx, ...)`.
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchStreamEvent>();

        // 4c. Build the engine + IPC client. Every failure path forwards an
        //     Error event into the streaming message so the user sees a
        //     visible error instead of a silently-empty bubble.
        let event_tx_for_cb = event_tx.clone();
        let chat_fut = async move {
            tracing::info!(
                target: "zenixai",
                provider = %provider.name,
                model = %provider.model,
                endpoint = %provider.endpoint_url(),
                msg_count = messages_for_engine.len(),
                workspace = %workspace_id,
                "chat_fut start"
            );

            let engine = match OrchestratorEngine::new(&provider) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(target: "zenixai", error = %e, "engine init failed");
                    let _ = event_tx.send(DispatchStreamEvent::Error(format!(
                        "Engine init failed: {e}"
                    )));
                    let _ = event_tx.send(DispatchStreamEvent::Done);
                    return Err(e);
                }
            };

            // NOTE: Do NOT call herdr_client.connect() upfront — it pings the
            // daemon and fails if herdr isn't running. For pure chat (no
            // dispatch directive in the LLM response), herdr is never used.
            // Defer connection to chat_and_dispatch's dispatch branch; if
            // dispatch fires and herdr is down, execute_agent_task will
            // surface the error via AgentComplete at that point.
            let mut herdr_client = HerdrIpcClient::new(socket_path);

            tracing::info!(target: "zenixai", "calling chat_and_dispatch");
            let result = engine
                .chat_and_dispatch(
                    &messages_for_engine,
                    &mut herdr_client,
                    &workspace_id,
                    move |ev| {
                        let _ = event_tx_for_cb.send(ev);
                    },
                )
                .await;

            match &result {
                Ok(text) => tracing::info!(
                    target: "zenixai",
                    bytes = text.len(),
                    "chat_and_dispatch completed"
                ),
                Err(e) => {
                    tracing::warn!(target: "zenixai", error = %e, "chat_and_dispatch failed");
                    let _ = event_tx.send(DispatchStreamEvent::Error(format!("Chat failed: {e}")));
                }
            }

            // Always emit Done so the consumer loop unblocks.
            let _ = event_tx.send(DispatchStreamEvent::Done);
            result
        };

        // 4d. Run chat_fut on the background executor with abort support.
        //     When abort_rx fires, chat_fut is dropped (HTTP stream + on_event
        //     callback are dropped together) — partial content is preserved.
        let chat_task = cx.background_executor().spawn(async move {
            tokio::select! {
                _ = abort_rx => Ok::<String, crate::ai::error::AiError>(String::new()),
                result = chat_fut => result,
            }
        });

        // 4e. Consume events. Each event triggers an entity update that
        //     appends to the streaming message's content.
        while let Some(event) = event_rx.recv().await {
            let _ = this.update(cx, |this, cx| {
                handle_dispatch_event(this, event, cx);
            });
        }

        // 4f. chat_task has resolved (event_tx dropped → recv returned None).
        let _ = chat_task.await;

        // 4g. Clear streaming state. Partial content is already in place on
        //     the assistant message via Token events.
        let _ = this.update(cx, |this, cx| {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.is_streaming = false;
                data.streaming_msg_idx = None;
                data.abort_handle = None;
            }
            cx.notify();
        });
    })
    .detach();
}

/// Fold a `DispatchStreamEvent` into the chat panel state.
///
/// Called from the spawn task's consumer loop via `this.update(cx, ...)`.
fn handle_dispatch_event(
    this: &mut ZenixApp,
    event: crate::ai::engine::DispatchStreamEvent,
    cx: &mut gpui::Context<'_, ZenixApp>,
) {
    use crate::ai::engine::DispatchStreamEvent;
    let Some(ref mut data) = this.ai_chat_panel else {
        return;
    };
    match event {
        DispatchStreamEvent::Token(text) => {
            tracing::trace!(
                target: "zenixai",
                len = text.len(),
                streaming_idx = ?data.streaming_msg_idx,
                current_msgs = data.messages.len(),
                "Token event"
            );
            if let Some(idx) = data.streaming_msg_idx {
                data.append_to_message(idx, &text);
                cx.notify();
            }
        }
        DispatchStreamEvent::Error(err) => {
            data.is_streaming = false;
            data.streaming_msg_idx = None;
            data.retry_last_prompt = data.last_user_prompt.clone();
            let session_id = data.current_session_id.clone().unwrap_or_default();
            let err_msg = crate::ai::session::ChatMessage {
                id: 0,
                session_id,
                role: "error".into(),
                content: err.clone(),
                agent_activities: None,
                timestamp: timestamp_now(),
                sequence: data.messages.len() as i64,
            };
            data.push_message(err_msg);
            cx.notify();
        }
        DispatchStreamEvent::AgentComplete(result) => {
            data.add_agent_block(&result);
            cx.notify();
        }
        // Done, Dispatch, AgentOutput, ToolCall, ToolResult, Compacted —
        // handled (or deferred to later phases). Done is consumed by the
        // spawn loop's event_rx.recv() returning None after channel close.
        other => {
            tracing::debug!(
                target: "zenixai",
                event = ?std::mem::discriminant(&other),
                "other dispatch event"
            );
        }
    }
}

/// Abort the in-flight chat stream.
///
/// Sends the abort signal; the spawn task's `tokio::select!` resolves the
/// abort branch, drops `chat_fut` (closing the HTTP stream), and the cleanup
/// in step 4g clears streaming state. Partial content already on the message
/// is preserved.
fn stop_chat_streaming(this: &mut ZenixApp, cx: &mut gpui::Context<'_, ZenixApp>) {
    if let Some(ref mut data) = this.ai_chat_panel {
        if let Some(tx) = data.abort_handle.take() {
            let _ = tx.send(());
        }
    }
    cx.notify();
}

/// Central dispatch for all chat panel UI actions.
fn handle_ai_chat_action(
    this: &mut ZenixApp,
    action: crate::ai::chat::AiChatAction,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<'_, ZenixApp>,
) {
    use crate::ai::chat::AiChatAction;
    use gpui::{FollowMode, ListAlignment, ListState, px};
    match action {
        AiChatAction::Send => {
            let streaming = this
                .ai_chat_panel
                .as_ref()
                .map_or(false, |d| d.is_streaming);
            if streaming {
                stop_chat_streaming(this, cx);
            } else {
                let text = this
                    .chat_input_entity
                    .as_ref()
                    .map(|e| e.read(cx).value().to_string())
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    if let Some(ref mut data) = this.ai_chat_panel {
                        data.input_value = text.clone();
                    }
                    start_chat_streaming(this, text, cx);
                    if let Some(input) = this.chat_input_entity.as_ref() {
                        input.update(cx, |state, cx| state.set_value("", window, cx));
                    }
                }
            }
        }
        AiChatAction::NewChat => {
            if let Some(ref mut data) = this.ai_chat_panel {
                let list_state = ListState::new(0, ListAlignment::Bottom, px(400.));
                list_state.set_follow_mode(FollowMode::Tail);
                data.messages = std::rc::Rc::new(Vec::new());
                data.list_state = list_state;
                data.scrolled_off_count = 0;
                data.current_session_id = None;
                data.current_session_title = None;
                data.streaming_msg_idx = None;
                data.is_streaming = false;
                data.input_value = String::new();
                data.last_user_prompt = None;
                data.config_dropdown = None;
                data.header_menu_open = false;
            }
            cx.notify();
        }
        AiChatAction::OpenDrawer(kind) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.drawer = Some(kind);
                data.config_dropdown = None;
                data.header_menu_open = false;
            }
            cx.notify();
        }
        AiChatAction::CloseDrawer => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.drawer = None;
            }
            cx.notify();
        }
        AiChatAction::RetryLast => {
            let prompt = this
                .ai_chat_panel
                .as_ref()
                .and_then(|d| {
                    d.retry_last_prompt
                        .clone()
                        .or_else(|| d.last_user_prompt.clone())
                });
            if let Some(prompt) = prompt {
                start_chat_streaming(this, prompt, cx);
            } else {
                tracing::warn!(target: "zenixai", "retry requested but no last prompt");
            }
        }
        AiChatAction::ToggleModelDropdown => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.config_dropdown = match data.config_dropdown {
                    Some(crate::ai::chat::ConfigDropdownTarget::Model) => None,
                    _ => {
                        data.drawer = None;
                        Some(crate::ai::chat::ConfigDropdownTarget::Model)
                    }
                };
            }
            cx.notify();
        }
        AiChatAction::SelectProvider(name) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.selected_provider_id = name;
                data.config_dropdown = None;
                // Reset model discovery so the new provider's models are fetched.
                data.available_models.clear();
                data.selected_model.clear();
                data.models_fetch_attempted = false;
            }
            cx.notify();
        }
        AiChatAction::ToggleProviderDropdown => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.config_dropdown = match data.config_dropdown {
                    Some(crate::ai::chat::ConfigDropdownTarget::Provider) => None,
                    _ => {
                        data.drawer = None;
                        Some(crate::ai::chat::ConfigDropdownTarget::Provider)
                    }
                };
            }
            cx.notify();
        }
        AiChatAction::ToggleThinkingDropdown => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.config_dropdown = match data.config_dropdown {
                    Some(crate::ai::chat::ConfigDropdownTarget::Thinking) => None,
                    _ => {
                        data.drawer = None;
                        Some(crate::ai::chat::ConfigDropdownTarget::Thinking)
                    }
                };
            }
            cx.notify();
        }
        AiChatAction::SelectThinkingLevel(level) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.thinking_level = level;
                data.config_dropdown = None;
            }
            cx.notify();
        }
        AiChatAction::SelectModel(model) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.selected_model = model;
                data.config_dropdown = None;
            }
            cx.notify();
        }
        AiChatAction::SuggestionClick(text) => {
            if !text.trim().is_empty() {
                start_chat_streaming(this, text, cx);
            }
        }
        AiChatAction::OpenMenu => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.header_menu_open = !data.header_menu_open;
            }
            cx.notify();
        }
        AiChatAction::OpenSettings => {
            this.show_settings = true;
            if let Some(ref mut data) = this.ai_chat_panel {
                data.header_menu_open = false;
            }
            cx.notify();
        }
        AiChatAction::SetHoveredMessage(id) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.hovered_msg_id = id;
            }
            cx.notify();
        }
        AiChatAction::ResizePanel(new_width) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.ai_panel_width = new_width.clamp(360.0, 720.0);
            }
            cx.notify();
        }
        AiChatAction::PersistPanelWidth => {
            if let Some(ref data) = this.ai_chat_panel {
                this.config.set_ai_panel_width(data.ai_panel_width);
                let _ = this.config.save();
            }
        }
        AiChatAction::ResizeInput(new_height) => {
            if let Some(ref mut data) = this.ai_chat_panel {
                data.input_height = new_height.clamp(60.0, 400.0);
            }
            cx.notify();
        }
        AiChatAction::PersistInputHeight => {
            if let Some(ref data) = this.ai_chat_panel {
                this.config.set_ai_input_height(data.input_height);
                let _ = this.config.save();
            }
        }
    }
}

/// Render a friendly placeholder when no API Provider is configured.
///
/// Shown instead of the chat panel to guide users to Settings → Extensions
/// → API Providers.
fn render_ai_config_empty(theme: &gpui_component::Theme) -> gpui::AnyElement {
    use gpui::{div, px};
    use gpui_component::v_flex;

    let bg = theme.background;
    let muted = theme.muted_foreground;

    v_flex()
        .size_full()
        .bg(bg)
        .items_center()
        .justify_center()
        .gap(px(12.))
        .child(div().text_size(px(36.)).child("🤖"))
        .child(
            div()
                .text_size(px(13.))
                .text_color(muted)
                .px(px(24.))
                .text_center()
                .child(t("ai.config_empty")),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(muted.opacity(0.6))
                .px(px(24.))
                .text_center()
                .child(t("ai.config_empty_hint")),
        )
        .into_any_element()
}
