//! AI Chat panel UI — message list, input bar, bottom bar, config bar,
//! history panel, agent blocks, and agent monitoring.
//!
//! Renders as an inline panel (same position as SystemInfo) in the right
//! sidebar area. Users type messages, the orchestrator engine streams replies,
//! and dispatched agents show as inline collapsible blocks.

use gpui::{
    AnyElement, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    div, px,
};
use gpui_component::{
    Theme, button::{Button, ButtonVariants as _}, h_flex, input::{Input, InputState},
    scroll::ScrollableElement as _, v_flex,
};

use crate::ai::engine::AgentResult;
use crate::ai::herdr::AgentStatus as HerdrAgentStatus;
use crate::ai::markdown;
use crate::ai::session::{ChatMessage, SearchHit, Session};
use crate::api_provider::ThinkingLevel;
use crate::i18n::t;

/// Width of the AI chat panel.
pub const AI_PANEL_WIDTH: f32 = 400.0;

// ── Data types ─────────────────────────────────────────────────────────

/// Target for config bar dropdown menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigDropdownTarget {
    Model,
    Workspace,
}

/// A collapsible agent output block rendered inline in the chat stream.
#[derive(Debug, Clone)]
pub struct AgentBlock {
    pub agent_name: String,
    pub status: String,       // "running" | "completed" | "failed"
    pub output: String,       // Full output
    pub summary: Option<String>,
    pub pane_id: String,
    pub is_expanded: bool,
    /// Unique key to track collapse state across renders.
    pub block_id: usize,
    /// Whether the agent is currently running.
    pub is_running: bool,
    /// Accumulated output history for real-time updates.
    pub output_history: String,
}

/// A single entry in the agent monitoring floating layer.
#[derive(Debug, Clone)]
pub struct AgentMonitorEntry {
    pub agent_name: String,
    pub pane_id: String,
    pub status: String,       // "idle" | "working" | "blocked" | "done"
    pub recent_output: String,
    pub is_expanded: bool,
}

impl AgentMonitorEntry {
    /// Convert a herdr `AgentStatus` into a monitor entry (requires recent output).
    pub fn from_herdr(
        agent_name: String,
        pane_id: String,
        status: HerdrAgentStatus,
        recent_output: String,
    ) -> Self {
        Self {
            agent_name,
            pane_id,
            status: status.as_str().to_string(),
            recent_output,
            is_expanded: false,
        }
    }
}

/// The main AI chat panel state.
///
/// Owned by `ZenixApp.ai_chat_panel` as `Option<AiChatPanelData>`.
pub struct AiChatPanelData {
    // ── Messages ──────────────────────────────────────────────────
    /// Display messages (user + orchestrator).
    pub messages: Vec<ChatMessage>,
    /// Whether the orchestrator is currently streaming a reply.
    pub is_streaming: bool,
    /// Input text value (synced with the InputState entity).
    pub input_value: String,

    // ── Session management ────────────────────────────────────────
    /// Currently active session ID (None = no session yet).
    pub current_session_id: Option<String>,
    /// Currently active session title.
    pub current_session_title: Option<String>,

    // ── Config bar ────────────────────────────────────────────────
    /// Whether the config bar is visible.
    pub config_bar_visible: bool,
    /// Selected API provider ID.
    pub selected_provider_id: String,
    /// Selected model name.
    pub selected_model: String,
    /// Available models for the current provider.
    pub available_models: Vec<String>,
    /// Selected thinking level.
    pub thinking_level: ThinkingLevel,
    /// Selected workspace ID (herdr workspace).
    pub selected_workspace: Option<String>,
    /// Available workspaces.
    pub workspaces: Vec<WorkspaceOption>,

    // ── Dropdown state ────────────────────────────────────────────
    /// Currently expanded dropdown target (if any).
    pub config_dropdown: Option<ConfigDropdownTarget>,

    // ── History panel ─────────────────────────────────────────────
    /// Whether the history sidebar is shown.
    pub show_history: bool,
    /// List of past sessions (most recent first).
    pub history_sessions: Vec<Session>,

    // ── Agent blocks (inline in chat) ─────────────────────────────
    /// Agent output blocks rendered inline.
    pub agent_blocks: Vec<AgentBlock>,
    /// Counter for generating unique block IDs.
    pub next_block_id: usize,

    // ── Agent monitoring ──────────────────────────────────────────
    /// Whether the agent monitoring floating layer is shown.
    pub show_agents: bool,
    /// Active agent statuses (from herdr).
    pub agent_statuses: Vec<AgentMonitorEntry>,
    /// Count of agents that are NOT in "done" state.
    pub active_agent_count: usize,

    // ── Search ────────────────────────────────────────────────────
    /// Current search query text.
    pub search_query: String,
    /// FTS5 search results.
    pub search_results: Vec<SearchHit>,
}

/// A workspace option for the config bar dropdown.
#[derive(Debug, Clone)]
pub struct WorkspaceOption {
    pub id: String,
    pub label: String,
}

impl Default for AiChatPanelData {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            is_streaming: false,
            input_value: String::new(),
            current_session_id: None,
            current_session_title: None,
            config_bar_visible: true,
            selected_provider_id: String::new(),
            selected_model: String::new(),
            available_models: Vec::new(),
            thinking_level: ThinkingLevel::default(),
            selected_workspace: None,
            workspaces: Vec::new(),
            config_dropdown: None,
            show_history: false,
            history_sessions: Vec::new(),
            agent_blocks: Vec::new(),
            next_block_id: 0,
            show_agents: false,
            agent_statuses: Vec::new(),
            active_agent_count: 0,
            search_query: String::new(),
            search_results: Vec::new(),
        }
    }
}

impl AiChatPanelData {
    pub fn new() -> Self {
        let mut data = Self::default();
        // Auto-detect and load zenix provider config if available.
        // Model fetching is handled in app.rs where cx.spawn() is available.
        if let Some(zp) = crate::api_provider::zenix_provider() {
            data.selected_provider_id = zp.name.clone();
            data.selected_model = zp.model.clone();
            data.thinking_level = zp.thinking_level;
        }
        data
    }

    /// Add an agent block from an agent result.
    pub fn add_agent_block(&mut self, result: &AgentResult) {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.agent_blocks.push(AgentBlock {
            agent_name: result.agent_name.clone(),
            status: result.exit_status.clone(),
            output: result.output.clone(),
            summary: result.summary.clone(),
            pane_id: result.pane_id.clone(),
            is_expanded: false,
            block_id: id,
            is_running: false,
            output_history: String::new(),
        });
    }

    /// Mark an agent block as running.
    pub fn mark_agent_running(&mut self, block_id: usize) {
        if let Some(block) = self.agent_blocks.iter_mut().find(|b| b.block_id == block_id) {
            block.is_running = true;
            block.status = "running".to_string();
        }
    }

    /// Toggle the expanded state of an agent block by ID.
    pub fn toggle_agent_block(&mut self, block_id: usize) {
        if let Some(block) = self.agent_blocks.iter_mut().find(|b| b.block_id == block_id) {
            block.is_expanded = !block.is_expanded;
        }
    }

    /// Get the count of non-done agents for the 👤 button display.
    pub fn count_active_agents(&self) -> usize {
        self.agent_statuses
            .iter()
            .filter(|a| a.status != "done" && a.status != "idle")
            .count()
    }

    /// Cycle the thinking level: Low → Medium → High → X-High → Low.
    pub fn cycle_thinking_level(&mut self) {
        self.thinking_level = match self.thinking_level {
            ThinkingLevel::Low => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High => ThinkingLevel::XHigh,
            ThinkingLevel::XHigh => ThinkingLevel::Low,
        };
    }
}

// ── Render ──────────────────────────────────────────────────────────────

/// Render the AI chat panel as a GPUI element.
///
/// The `input_entity` parameter provides the shared `InputState` entity managed
/// by `ZenixApp` (for subscription to `PressEnter` events). `None` renders a
/// placeholder-only input bar.
pub fn render_ai_chat_panel(
    data: &AiChatPanelData,
    theme: &Theme,
    input_entity: Option<&Entity<InputState>>,
) -> impl IntoElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let bg = th.background;
    let border = th.border;
    let _fg = th.foreground;

    // Message area
    let message_elements: Vec<AnyElement> = data
        .messages
        .iter()
        .map(|msg| render_message(msg, theme))
        .collect();

    let mut panel = h_flex()
        .w(px(AI_PANEL_WIDTH))
        .h_full()
        .flex_col()
        .bg(bg)
        .border_l_1()
        .border_color(border)
        .overflow_y_scrollbar();

    // ── Header ──────────────────────────────────────────────────────
    panel = panel.child(render_header(data, theme));

    // ── Config bar ──────────────────────────────────────────────────
    if data.config_bar_visible {
        panel = panel.child(render_config_bar(data, theme));
    }

    // ── Dropdown overlay ────────────────────────────────────────────
    if let Some(target) = data.config_dropdown {
        panel = panel.child(render_dropdown(target, data, theme));
    }

    // ── Search bar ──────────────────────────────────────────────────
    if !data.search_query.is_empty() {
        panel = panel.child(render_search_bar(data, theme));
    }

    // ── History panel ───────────────────────────────────────────────
    if data.show_history {
        panel = panel.child(render_history_panel(data, theme));
    }

    // ── Agent monitoring ────────────────────────────────────────────
    if data.show_agents {
        panel = panel.child(render_agent_monitor(data, theme));
    }

    // ── Messages (scrollable) ───────────────────────────────────────
    panel = panel.child(
        div()
            .flex_grow(1.0)
            .min_h_0()
            .overflow_y_scrollbar()
            .p(px(12.))
            .gap(px(10.))
            .children(message_elements),
    );

    // ── Agent blocks ────────────────────────────────────────────────
    if !data.agent_blocks.is_empty() {
        let block_elements: Vec<AnyElement> = data
            .agent_blocks
            .iter()
            .map(|block| render_agent_block(block, theme))
            .collect();
        panel = panel.child(v_flex().gap(px(8.)).children(block_elements));
    }

    // ── Streaming indicator ─────────────────────────────────────────
    if data.is_streaming {
        panel = panel.child(
            h_flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(6.))
                .child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(gpui::hsla(0.55, 0.8, 0.6, 1.0)),
                )
                .child(
                    txt(px(11.))
                        .text_color(mu)
                        .child(t("ai.chat.streaming")),
                ),
        );
    }

    // ── Input area ──────────────────────────────────────────────────
    panel = panel.child(render_input_area(data, theme, input_entity));

    // ── Bottom bar ──────────────────────────────────────────────────
    panel = panel.child(render_bottom_bar(data, theme));

    panel
}

// ── Header ──────────────────────────────────────────────────────────────

fn render_header(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let border = th.border;

    let title = if let Some(ref title) = data.current_session_title {
        title.clone()
    } else {
        t("ai.chat.new_chat").to_string()
    };

    v_flex()
        .gap(px(2.))
        .px(px(12.))
        .py(px(10.))
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    txt(px(14.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(title),
                )
                .child(
                    h_flex()
                        .gap(px(2.))
                        .child(
                            Button::new("ai-chat-search")
                                .ghost()
                                .label(t("ai.chat.search_btn"))
                                .compact()
                                .w(px(52.)),
                        )
                        .child(
                            Button::new("ai-chat-config")
                                .ghost()
                                .label(t("ai.chat.config_btn"))
                                .compact()
                                .w(px(52.)),
                        ),
                ),
        )
        .into_any_element()
}

// ── Config bar ──────────────────────────────────────────────────────────

fn render_config_bar(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let _bg = th.background;
    let border = th.border;
    let _fg = th.foreground;

    let pill = |text: &str| -> AnyElement {
        txt(px(11.))
            .text_color(mu)
            .bg(gpui::rgba(0xFFFFFF08))
            .rounded_full()
            .px(px(8.))
            .py(px(3.))
            .cursor_pointer()
            .hover(|st| st.bg(gpui::rgba(0xFFFFFF12)))
            .child(text.to_string())
            .into_any_element()
    };

    let sep1 = txt(px(10.))
        .text_color(gpui::rgba(0x555555FF))
        .child("▏");
    let sep2 = txt(px(10.))
        .text_color(gpui::rgba(0x555555FF))
        .child("▏");

    let ws_label = data
        .selected_workspace
        .as_ref()
        .map(|wid| {
            data.workspaces
                .iter()
                .find(|w| w.id == *wid)
                .map(|w| w.label.clone())
                .unwrap_or_else(|| wid.clone())
        })
        .unwrap_or_else(|| t("ai.config.no_workspace").to_string());

    h_flex()
        .gap(px(6.))
        .px(px(12.))
        .py(px(5.))
        .border_b_1()
        .border_color(border)
        .items_center()
        .child(pill(&data.selected_model))
        .child(sep1)
        .child(pill(data.thinking_level.display_label()))
        .child(sep2)
        .child(pill(&ws_label))
        .into_any_element()
}

// ── Dropdown ────────────────────────────────────────────────────────────

fn render_dropdown(
    target: ConfigDropdownTarget,
    data: &AiChatPanelData,
    theme: &Theme,
) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let fg = th.foreground;
    let border = th.border;
    let _bg = th.background;

    match target {
        ConfigDropdownTarget::Model => {
            // Group models by provider if available
            let model_items: Vec<AnyElement> = data
                .available_models
                .iter()
                .map(|model| {
                    let is_selected = model == &data.selected_model;
                    let prefix = if is_selected { "✓ " } else { "  " };
                    let style = if is_selected { fg } else { mu };
                    h_flex()
                        .gap(px(6.))
                        .p(px(6.))
                        .rounded_sm()
                        .hover(|st| st.bg(gpui::rgba(0xFFFFFF08)))
                        .cursor_pointer()
                        .child(txt(px(11.)).text_color(style).child(format!("{prefix}{model}")))
                        .into_any_element()
                })
                .collect();

            v_flex()
                .gap(px(4.))
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            txt(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(fg)
                                .child(t("ai.config.select_model")),
                        )
                        .child(
                            Button::new("ai-dropdown-close")
                                .ghost()
                                .label("✕")
                                .compact()
                                .w(px(18.))
                                .h(px(16.)),
                        ),
                )
                .children(model_items)
                .into_any_element()
        }
        ConfigDropdownTarget::Workspace => {
            let ws_items: Vec<AnyElement> = data
                .workspaces
                .iter()
                .map(|ws| {
                    let is_selected = data.selected_workspace.as_deref() == Some(ws.id.as_str());
                    let prefix = if is_selected { "✓ " } else { "  " };
                    h_flex()
                        .justify_between()
                        .items_center()
                        .gap(px(8.))
                        .p(px(6.))
                        .rounded_sm()
                        .hover(|st| st.bg(gpui::rgba(0xFFFFFF08)))
                        .cursor_pointer()
                        .child(
                            txt(px(11.))
                                .text_color(mu)
                                .child(format!("{prefix}{}", ws.label)),
                        )
                        .into_any_element()
                })
                .collect();

            let empty_msg = if data.workspaces.is_empty() {
                txt(px(11.))
                    .text_color(mu)
                    .child(t("ai.config.no_workspaces"))
                    .into_any_element()
            } else {
                div().into_any_element()
            };

            v_flex()
                .gap(px(4.))
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            txt(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(fg)
                                .child(t("ai.config.select_workspace")),
                        )
                        .child(
                            Button::new("ai-dropdown-close")
                                .ghost()
                                .label("✕")
                                .compact()
                                .w(px(18.))
                                .h(px(16.)),
                        ),
                )
                .children(ws_items)
                .child(empty_msg)
                .into_any_element()
        }
    }
}

// ── Search bar ──────────────────────────────────────────────────────────

fn render_search_bar(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let fg = th.foreground;

    let results_children: Vec<AnyElement> = data
        .search_results
        .iter()
        .map(|hit| {
            let snippet = hit.snippet.clone();
            let session_title = hit.session_title.clone();
            let role = hit.message.role.clone();
            h_flex()
                .gap(px(6.))
                .p(px(5.))
                .rounded_sm()
                .hover(|st| st.bg(gpui::rgba(0xFFFFFF08)))
                .cursor_pointer()
                .child(
                    txt(px(10.))
                        .text_color(mu)
                        .w(px(80.))
                        .child(session_title),
                )
                .child(
                    txt(px(10.))
                        .text_color(mu)
                        .child(format!("[{role}] {snippet}",)),
                )
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap(px(4.))
        .px(px(12.))
        .py(px(6.))
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .child(
                    txt(px(12.))
                        .text_color(mu)
                        .child(t("ai.search.icon")),
                )
                .child(
                    txt(px(12.))
                        .text_color(fg)
                        .child(data.search_query.clone()),
                )
                .child(
                    Button::new("ai-search-clear")
                        .ghost()
                        .label("✕")
                        .compact()
                        .w(px(18.))
                        .h(px(16.)),
                ),
        )
        .children(results_children)
        .into_any_element()
}

// ── History panel ───────────────────────────────────────────────────────

fn render_history_panel(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let fg = th.foreground;

    let session_rows: Vec<AnyElement> = data
        .history_sessions
        .iter()
        .map(|s| {
            let title = s.title.clone();
            let time = s.updated_at.clone();
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(8.))
                .p(px(6.))
                .rounded_sm()
                .hover(|st| st.bg(gpui::rgba(0xFFFFFF08)))
                .cursor_pointer()
                .child(
                    v_flex()
                        .gap(px(2.))
                        .child(
                            txt(px(12.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(fg)
                                .child(title),
                        )
                        .child(txt(px(10.)).text_color(mu).child(time)),
                )
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap(px(8.))
        .px(px(12.))
        .py(px(8.))
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(t("ai.history.title")),
                )
                .child(
                    Button::new("ai-history-close")
                        .ghost()
                        .label("✕")
                        .compact()
                        .w(px(18.))
                        .h(px(16.)),
                ),
        )
        .children(session_rows)
        .into_any_element()
}

// ── Agent monitor floating layer ────────────────────────────────────────

fn render_agent_monitor(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let fg = th.foreground;

    let entries: Vec<AnyElement> = data
        .agent_statuses
        .iter()
        .map(|entry| {
            let agent_name = entry.agent_name.clone();
            let status_icon = match entry.status.as_str() {
                "working" => "●",
                "blocked" => "◉",
                "idle" => "○",
                "done" => "✓",
                _ => "○",
            };
            let status_color = match entry.status.as_str() {
                "working" => gpui::hsla(0.33, 0.7, 0.5, 1.0),
                "blocked" => gpui::hsla(0.08, 0.9, 0.55, 1.0),
                "done" => gpui::hsla(0.0, 0.0, 0.5, 1.0),
                _ => gpui::hsla(0.0, 0.0, 0.4, 1.0),
            };

            h_flex()
                .justify_between()
                .items_start()
                .gap(px(8.))
                .p(px(6.))
                .rounded_sm()
                .hover(|st| st.bg(gpui::rgba(0xFFFFFF08)))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(
                            txt(px(12.))
                                .text_color(status_color)
                                .child(status_icon.to_string()),
                        )
                        .child(
                            v_flex()
                                .gap(px(1.))
                                .child(
                                    txt(px(11.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(fg)
                                        .child(agent_name),
                                )
                                .child(txt(px(10.)).text_color(mu).child(entry.status.clone())),
                        ),
                )
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap(px(8.))
        .px(px(12.))
        .py(px(8.))
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(format!(
                            "{} ({})",
                            t("ai.agent.monitor_title"),
                            data.active_agent_count
                        )),
                )
                .child(
                    Button::new("ai-monitor-close")
                        .ghost()
                        .label("✕")
                        .compact()
                        .w(px(18.))
                        .h(px(16.)),
                ),
        )
        .children(entries)
        .into_any_element()
}

// ── Input area ──────────────────────────────────────────────────────────

fn render_input_area(
    data: &AiChatPanelData,
    theme: &Theme,
    input_entity: Option<&Entity<InputState>>,
) -> AnyElement {
    let th = theme.clone();
    let border = th.border;

    // Render the InputState if available, otherwise a plain text area
    let input_child: AnyElement = match input_entity {
        Some(entity) => {
            let input = Input::new(entity).into_any_element();
            h_flex()
                .flex_grow(1.0)
                .child(input)
                .into_any_element()
        }
        None => h_flex()
            .flex_grow(1.0)
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .min_h(px(36.))
                    .child(data.input_value.clone()),
            )
            .into_any_element(),
    };

    let send_label = if data.is_streaming {
        t("ai.chat.stop")
    } else {
        t("ai.chat.send")
    };

    h_flex()
        .gap(px(8.))
        .px(px(10.))
        .py(px(8.))
        .border_t_1()
        .border_color(border)
        .child(input_child)
        .child(
            Button::new("ai-chat-send")
                .ghost()
                .label(send_label)
                .compact()
                .primary(),
        )
        .into_any_element()
}

// ── Bottom bar ──────────────────────────────────────────────────────────

fn render_bottom_bar(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let bg = th.background;
    let _fg = th.foreground;
    let active_count = data.count_active_agents();

    let agent_btn_label = if active_count > 0 {
        format!("👤 {}", t("ai.agent.running").replace("%{n}", &active_count.to_string()))
    } else {
        "👤".to_string()
    };

    h_flex()
        .justify_between()
        .items_center()
        .px(px(8.))
        .py(px(4.))
        .border_t_1()
        .border_color(border)
        .bg(bg)
        .child(
            h_flex()
                .gap(px(4.))
                .child(
                    Button::new("ai-chat-new")
                        .ghost()
                        .label(t("ai.chat.new_session"))
                        .compact()
                        .w(px(72.)),
                )
                .child(
                    Button::new("ai-chat-history")
                        .ghost()
                        .label(t("ai.chat.history"))
                        .compact()
                        .w(px(72.)),
                )
                .child(
                    Button::new("ai-chat-agents")
                        .ghost()
                        .label(agent_btn_label)
                        .compact()
                        .w(px(96.)),
                )
                .child(
                    Button::new("ai-chat-settings")
                        .ghost()
                        .label(t("ai.chat.settings"))
                        .compact()
                        .w(px(48.)),
                ),
        )
        .child(
            txt(px(10.))
                .text_color(mu)
                .child(format!(
                    "{} {}",
                    data.messages.len(),
                    t("ai.chat.messages")
                )),
        )
        .into_any_element()
}

// ── Agent block rendering ───────────────────────────────────────────────

fn render_agent_block(block: &AgentBlock, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let fg = th.foreground;
    let _bg = th.background;

    let status_color = match block.status.as_str() {
        "completed" => gpui::hsla(0.33, 0.7, 0.5, 1.0),
        "failed" => gpui::hsla(0.0, 0.8, 0.55, 1.0),
        "running" => gpui::hsla(0.55, 0.8, 0.6, 1.0),
        _ => gpui::hsla(0.0, 0.0, 0.5, 1.0),
    };

    let status_icon = match block.status.as_str() {
        "completed" => "✓",
        "failed" => "✕",
        "running" => "⟳",
        _ => "○",
    };

    let status_text = if block.is_running {
        format!("{status_icon} running...",)
    } else {
        format!("{status_icon} {}", block.status)
    };

    let summary = if let Some(ref s) = block.summary {
        s.clone()
    } else if block.is_running {
        format!("{}: running...", block.agent_name)
    } else {
        format!("{}: {}", block.agent_name, block.status)
    };

    let header = h_flex()
        .justify_between()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(6.))
        .rounded_t_md()
        .bg(gpui::rgba(0xFFFFFF08))
        .cursor_pointer()
        .hover(|st| st.bg(gpui::rgba(0xFFFFFF12)))
        .child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(
                    txt(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(block.agent_name.clone()),
                )
                .child(
                    txt(px(11.))
                        .text_color(status_color)
                        .child(status_text),
                ),
        )
        .child(
            txt(px(10.))
                .text_color(mu)
                .child(if block.is_expanded {
                    t("ai.agent.collapse")
                } else {
                    t("ai.agent.expand")
                }),
        );

    // Collect markdown segments from output
    let output_segments = if block.is_expanded {
        markdown::render_markdown(&block.output, theme)
    } else {
        Vec::new()
    };

    let body: AnyElement = if block.is_expanded && !output_segments.is_empty() {
        // Show rendered markdown output
        v_flex()
            .rounded_b_md()
            .bg(gpui::rgba(0x00000008))
            .p(px(10.))
            .gap(px(4.))
            .children(output_segments)
            .into_any_element()
    } else if block.is_expanded {
        // Show raw output
        gpui::div()
            .rounded_b_md()
            .bg(gpui::rgba(0x00000008))
            .p(px(10.))
            .text_size(px(11.))
            .font_family("Lilex")
            .line_height(px(16.))
            .child(block.output.clone())
            .into_any_element()
    } else if block.is_running {
        // Running — show accumulated output preview
        let preview: String = block
            .output_history
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n");
        gpui::div()
            .rounded_b_md()
            .bg(gpui::rgba(0x00000008))
            .p(px(10.))
            .child(
                txt(px(11.))
                    .text_color(mu)
                    .child(format!("... {preview}")),
            )
            .into_any_element()
    } else {
        gpui::div()
            .rounded_b_md()
            .bg(gpui::rgba(0x00000008))
            .p(px(10.))
            .child(
                txt(px(11.))
                    .text_color(mu)
                    .child(format!("... {summary}")),
            )
            .into_any_element()
    };

    v_flex()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(gpui::rgba(0xFFFFFF10))
        .overflow_hidden()
        .child(header.into_any_element())
        .child(body)
        .into_any_element()
}

// ── Message rendering ───────────────────────────────────────────────────

fn render_message(msg: &ChatMessage, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let is_user = msg.role == "user";
    let content = msg.content.clone();
    let time = msg.timestamp.clone();

    // Use markdown renderer for user messages, plain text for orchestrator
    let bubble_content: AnyElement = if is_user {
        let segments = markdown::render_markdown(&content, theme);
        v_flex()
            .gap(px(2.))
            .children(segments)
            .into_any_element()
    } else {
        div()
            .text_size(px(13.))
            .text_color(fg)
            .line_height(px(18.))
            .child(content)
            .into_any_element()
    };

    let bubble = if is_user {
        // User message — right-aligned, primary color bg
        h_flex()
            .justify_end()
            .max_w(px(300.))
            .child(
                div()
                    .rounded_lg()
                    .rounded_bl_md()
                    .bg(gpui::rgba(0xFFFFFF12))
                    .p(px(10.))
                    .pr(px(12.))
                    .child(bubble_content),
            )
    } else {
        // Orchestrator message — left-aligned, subtle bg
        h_flex()
            .justify_start()
            .max_w(px(300.))
            .child(
                div()
                    .rounded_lg()
                    .rounded_br_md()
                    .bg(gpui::rgba(0xFFFFFF06))
                    .p(px(10.))
                    .pr(px(12.))
                    .child(bubble_content),
            )
    };

    v_flex()
        .w_full()
        .child(bubble.into_any_element())
        .child(
            txt(px(9.))
                .text_color(gpui::rgba(0x666666FF))
                .px(px(4.))
                .child(time),
        )
        .into_any_element()
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn txt(size: gpui::Pixels) -> gpui::Div {
    div().text_size(size)
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::engine::AgentResult;
    use crate::ai::herdr::AgentStatus as HerdrAgentStatus;
    use crate::ai::session::Session;

    fn make_block(id: usize, name: &str, status: &str) -> AgentBlock {
        AgentBlock {
            agent_name: name.to_string(),
            status: status.to_string(),
            output: format!("{name} output"),
            summary: Some(format!("{name} summary")),
            pane_id: format!("pane-{id}"),
            is_expanded: false,
            block_id: id,
            is_running: false,
            output_history: String::new(),
        }
    }

    fn make_monitor(name: &str, status: &str) -> AgentMonitorEntry {
        AgentMonitorEntry {
            agent_name: name.to_string(),
            pane_id: format!("pane-{name}"),
            status: status.to_string(),
            recent_output: format!("{name} recent output"),
            is_expanded: false,
        }
    }

    fn make_session(id: &str, title: &str) -> Session {
        Session {
            id: id.to_string(),
            title: title.to_string(),
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
            workspace: None,
            provider_id: "test".to_string(),
            model: "test-model".to_string(),
            thinking_level: "medium".to_string(),
        }
    }

    #[test]
    fn test_agent_block_construction() {
        let block = make_block(0, "claude", "completed");
        assert_eq!(block.agent_name, "claude");
        assert_eq!(block.status, "completed");
        assert!(!block.is_expanded);
        assert_eq!(block.block_id, 0);
        assert!(!block.is_running);
        assert!(block.output_history.is_empty());
    }

    #[test]
    fn test_agent_monitor_entry_from_herdr() {
        let entry = AgentMonitorEntry::from_herdr(
            "codex".into(),
            "pane-1".into(),
            HerdrAgentStatus::Working,
            "working on it...".into(),
        );
        assert_eq!(entry.agent_name, "codex");
        assert_eq!(entry.status, "working");
        assert!(!entry.is_expanded);
    }

    #[test]
    fn test_agent_monitor_status_as_str() {
        assert_eq!(HerdrAgentStatus::Working.as_str(), "working");
        assert_eq!(HerdrAgentStatus::Done.as_str(), "done");
        assert_eq!(HerdrAgentStatus::Idle.as_str(), "idle");
        assert_eq!(HerdrAgentStatus::Blocked.as_str(), "blocked");
    }

    #[test]
    fn test_agent_block_toggle() {
        let data = AiChatPanelData {
            agent_blocks: vec![make_block(0, "claude", "completed")],
            next_block_id: 1,
            ..Default::default()
        };
        let mut data = data;
        data.toggle_agent_block(0);
        assert!(data.agent_blocks[0].is_expanded);
        data.toggle_agent_block(0);
        assert!(!data.agent_blocks[0].is_expanded);
    }

    #[test]
    fn test_agent_block_toggle_nonexistent() {
        let data = AiChatPanelData {
            agent_blocks: vec![make_block(0, "claude", "completed")],
            next_block_id: 1,
            ..Default::default()
        };
        let mut data = data;
        // Toggling a non-existent block ID should be a no-op
        data.toggle_agent_block(99);
        assert!(!data.agent_blocks[0].is_expanded);
    }

    #[test]
    fn test_add_agent_block() {
        let mut data = AiChatPanelData::default();
        let result = AgentResult {
            agent_name: "claude".into(),
            pane_id: "p-1".into(),
            exit_status: "completed".into(),
            output: "Done".into(),
            summary: Some("Done task".into()),
        };
        data.add_agent_block(&result);
        assert_eq!(data.agent_blocks.len(), 1);
        assert_eq!(data.agent_blocks[0].agent_name, "claude");
        assert_eq!(data.next_block_id, 1);
        assert!(!data.agent_blocks[0].is_running);
        assert!(data.agent_blocks[0].output_history.is_empty());
    }

    #[test]
    fn test_count_active_agents() {
        let data = AiChatPanelData {
            agent_statuses: vec![
                make_monitor("claude", "working"),
                make_monitor("codex", "working"),
                make_monitor("opencode", "done"),
                make_monitor("idle-agent", "idle"),
            ],
            ..Default::default()
        };
        assert_eq!(data.count_active_agents(), 2);
    }

    #[test]
    fn test_count_active_agents_empty() {
        let data = AiChatPanelData::default();
        assert_eq!(data.count_active_agents(), 0);
    }

    #[test]
    fn test_default_is_empty() {
        let data = AiChatPanelData::default();
        assert!(data.messages.is_empty());
        assert!(data.agent_blocks.is_empty());
        assert!(data.agent_statuses.is_empty());
        assert!(data.history_sessions.is_empty());
        assert!(!data.is_streaming);
        assert!(!data.show_history);
        assert!(!data.show_agents);
        assert!(data.config_dropdown.is_none());
        assert_eq!(data.thinking_level, ThinkingLevel::default());
    }

    #[test]
    fn test_workspace_option_construction() {
        let ws = WorkspaceOption {
            id: "w-1".into(),
            label: "My Project".into(),
        };
        assert_eq!(ws.id, "w-1");
        assert_eq!(ws.label, "My Project");
    }

    #[test]
    fn test_cycle_thinking_level() {
        let mut data = AiChatPanelData::default();
        data.thinking_level = ThinkingLevel::Low;
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::Medium);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::High);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::XHigh);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::Low);
    }

    #[test]
    fn test_mark_agent_running() {
        let mut data = AiChatPanelData {
            agent_blocks: vec![make_block(0, "claude", "completed")],
            next_block_id: 1,
            ..Default::default()
        };
        data.mark_agent_running(0);
        assert!(data.agent_blocks[0].is_running);
        assert_eq!(data.agent_blocks[0].status, "running");
    }

    #[test]
    fn test_mark_agent_running_nonexistent() {
        let mut data = AiChatPanelData {
            agent_blocks: vec![make_block(0, "claude", "completed")],
            next_block_id: 1,
            ..Default::default()
        };
        data.mark_agent_running(99);
        assert!(!data.agent_blocks[0].is_running);
    }
}
