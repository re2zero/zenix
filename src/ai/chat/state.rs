//! AI Chat panel state — `AiChatPanelData` and supporting types.
//!
//! Owned by `ZenixApp.ai_chat_panel` as `Option<AiChatPanelData>`.

use std::collections::HashSet;
use std::rc::Rc;

use crate::ai::engine::AgentResult;
use crate::ai::session::{ChatMessage, SearchHit, Session};
use crate::api_provider::ThinkingLevel;
use gpui::{App, ListAlignment, ListState, Window, px};

/// Maximum messages retained in memory. Older messages are FIFO-evicted
/// once this cap is exceeded (status.md Phase 3 requirement).
pub const MAX_MESSAGES: usize = 200;

/// Actions that can be triggered from the chat panel UI.
///
/// All interactive elements (header buttons, model pill, dropdown items,
/// suggestion chips, send button) dispatch through this enum so the panel
/// needs only a single `on_action` callback.
#[derive(Debug, Clone)]
pub enum AiChatAction {
    Send,
    NewChat,
    ToggleModelDropdown,
    SelectModel(String),
    SelectProvider(String),
    SuggestionClick(String),
    OpenMenu,
    OpenSettings,
    SetHoveredMessage(Option<i64>),
    // ── v4 Phase 2 ──
    OpenDrawer(DrawerKind),
    CloseDrawer,
    RetryLast,
    // ── v4 Phase 3 ──
    ResizePanel(f32),
    PersistPanelWidth,
    ResizeInput(f32),
    PersistInputHeight,
    /// Start input-height drag (carries start-y in window coords).
    BeginInputResize(f32, f32),
    ToggleProviderDropdown,
    ToggleThinkingDropdown,
    SelectThinkingLevel(ThinkingLevel),
    // ── Workspace selection ───────────────────────────────────
    /// Open workspace dropdown.
    ToggleWorkspaceDropdown,
    /// Select a workspace by ID.
    SelectWorkspace(String),
    // ── @ Mention ──────────────────────────────────────────────
    /// Show/hide @ mention popup with query filter.
    ShowMentionPopup(String),
    /// Hide the @ mention popup.
    HideMentionPopup,
    /// Select the next item in the mention list.
    MentionNext,
    /// Select the previous item in the mention list.
    MentionPrev,
    /// Confirm the selected mention item.
    MentionConfirm,
    // ── Collapse toggles ────────────────────────────────────────
    /// Toggle collapse state for a tool card by message id.
    ToggleToolCollapse(i64),
    /// Toggle collapse state for a reasoning block by message id.
    ToggleReasoningCollapse(i64),
    // ── Keyboard shortcuts (Phase 7.2) ──────────────────────────
    /// Esc: stop streaming or clear input.
    Escape,
    /// Ctrl+L: clear all messages (keeps session).
    ClearChat,
    /// ↑/↓: navigate input history (Alt+Up/Down already bound at app level).
    HistoryPrev,
    HistoryNext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerKind {
    History,
    Agents,
}

pub type AiChatActionHandler = Rc<dyn Fn(AiChatAction, &mut Window, &mut App) + 'static>;

// ── Data types ─────────────────────────────────────────────────────────

/// Target for config bar dropdown menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigDropdownTarget {
    Model,
    Provider,
    Thinking,
    Workspace,
}

// ── AgentBlock / AgentMonitorEntry re-exports ──────────────────────────
//
// `AgentBlock` and `AgentMonitorEntry` are defined in `super::agent_block`,
// but `AiChatPanelData` references them, so we re-import here for
// convenient field access within this module.
pub use super::agent_block::{AgentBlock, AgentMonitorEntry};

/// The main AI chat panel state.
///
/// Owned by `ZenixApp.ai_chat_panel` as `Option<AiChatPanelData>`.
pub struct AiChatPanelData {
    // ── Messages ──────────────────────────────────────────────────
    /// `Rc<Vec>` rather than `Vec`: the render closure for `gpui::list`
    /// must own its data ('static), and cloning the whole Vec per frame
    /// would copy every `ChatMessage`. `Rc::make_mut` is in-place when
    /// the panel is the sole owner — the only other clones are transient,
    /// held by the in-flight render.
    pub messages: Rc<Vec<ChatMessage>>,
    pub is_streaming: bool,
    pub input_value: String,

    pub list_state: ListState,
    pub follow_tail: bool,
    pub scrolled_off_count: usize,

    // ── Session management ────────────────────────────────────────
    pub current_session_id: Option<String>,
    pub current_session_title: Option<String>,

    // ── Config ────────────────────────────────────────────────────
    pub config_bar_visible: bool,
    pub selected_provider_id: String,
    pub selected_model: String,
    pub available_models: Vec<String>,
    pub thinking_level: ThinkingLevel,
    pub input_height: f32,
    pub selected_workspace: Option<String>,
    pub workspace_bound: bool,
    pub workspaces: Vec<WorkspaceOption>,

    // ── Dropdown state ────────────────────────────────────────────
    pub config_dropdown: Option<ConfigDropdownTarget>,

    // ── History panel ─────────────────────────────────────────────
    pub drawer: Option<DrawerKind>,
    pub retry_last_prompt: Option<String>,
    pub history_sessions: Vec<Session>,

    // ── Agent blocks (inline in chat) ─────────────────────────────
    pub agent_blocks: Vec<AgentBlock>,
    pub next_block_id: usize,

    // ── Agent monitoring ──────────────────────────────────────────
    pub agent_statuses: Vec<AgentMonitorEntry>,
    pub active_agent_count: usize,

    // ── Search ────────────────────────────────────────────────────
    pub search_query: String,
    pub search_results: Vec<SearchHit>,

    // ── Streaming state (Phase 2 of structural redesign) ─────────
    /// Index into `messages` of the assistant message currently being streamed.
    /// `None` when not streaming or when the stream has finalized.
    pub streaming_msg_idx: Option<usize>,
    /// Most recent user prompt, kept for retry-on-error.
    pub last_user_prompt: Option<String>,
    /// Sender used to abort the in-flight chat stream. `Some` while a stream
    /// is active, taken and sent by `stop_chat_streaming`.
    pub abort_handle: Option<tokio::sync::oneshot::Sender<()>>,

    // ── v4 visual state ──────────────────────────────────────────
    pub ai_panel_width: f32,
    pub header_menu_open: bool,
    // Tracked at panel level (not per-row) because gpui::list rebuilds each
    // row element every frame, so per-row hover state would be discarded.
    pub hovered_msg_id: Option<i64>,

    // ── Model discovery ──────────────────────────────────────────
    /// Whether a `fetch_models` attempt has been made for the current
    /// provider. Prevents render-loop re-fetching after a failure.
    pub models_fetch_attempted: bool,

    // ── Provider list ────────────────────────────────────────────
    /// Names of all configured providers (for the provider dropdown).
    pub available_providers: Vec<String>,

    // ── Input history (Phase 3 R10) ──────────────────────────────
    pub input_history: Vec<String>,
    pub input_history_idx: Option<usize>,

    // ── @ Mention popup ──────────────────────────────────────────
    pub show_mention_popup: bool,
    pub mention_query: String,
    pub mention_selected_idx: usize,

    // ── Collapse state ───────────────────────────────────────────
    /// Set of message ids whose tool card body is collapsed.
    pub collapsed_tool_ids: HashSet<i64>,
    /// Set of message ids whose reasoning block is collapsed.
    pub collapsed_reasoning_ids: HashSet<i64>,
}

/// A workspace option for the config bar dropdown.
#[derive(Debug, Clone)]
pub struct WorkspaceOption {
    pub id: String,
    pub label: String,
    pub focused: bool,
}

impl Default for AiChatPanelData {
    fn default() -> Self {
        let list_state = ListState::new(0, ListAlignment::Bottom, px(400.));
        list_state.set_follow_mode(gpui::FollowMode::Tail);
        Self {
            messages: Rc::new(Vec::new()),
            is_streaming: false,
            input_value: String::new(),
            list_state,
            follow_tail: true,
            scrolled_off_count: 0,
            current_session_id: None,
            current_session_title: None,
            config_bar_visible: true,
            selected_provider_id: String::new(),
            selected_model: String::new(),
            available_models: Vec::new(),
            thinking_level: ThinkingLevel::default(),
            input_height: 160.0,
            selected_workspace: None,
            workspace_bound: false,
            workspaces: Vec::new(),
            config_dropdown: None,
            drawer: None,
            history_sessions: Vec::new(),
            agent_blocks: Vec::new(),
            next_block_id: 0,
            agent_statuses: Vec::new(),
            active_agent_count: 0,
            search_query: String::new(),
            search_results: Vec::new(),
            streaming_msg_idx: None,
            last_user_prompt: None,
            abort_handle: None,
            retry_last_prompt: None,
            ai_panel_width: 420.0,
            header_menu_open: false,
            hovered_msg_id: None,
            models_fetch_attempted: false,
            available_providers: Vec::new(),
            input_history: Vec::new(),
            input_history_idx: None,
            show_mention_popup: false,
            mention_query: String::new(),
            mention_selected_idx: 0,
            collapsed_tool_ids: HashSet::new(),
            collapsed_reasoning_ids: HashSet::new(),
        }
    }
}

impl AiChatPanelData {
    pub fn new() -> Self {
        let mut data = Self::default();
        // Prefer "Zenix" provider, fall back to first provider with api_key.
        let provider = crate::api_provider::zenix_provider().or_else(|| {
            crate::api_provider::load_all()
                .into_iter()
                .find(|p| !p.api_key.trim().is_empty())
        });
        if let Some(p) = provider {
            data.selected_provider_id = p.name.clone();
            data.selected_model = p.model.clone();
            data.thinking_level = p.thinking_level;
        }
        data
    }

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
        // Dual-track: agent_blocks keeps runtime lifecycle state, while this
        // role="tool" message enters the unified render flow (R5).
        let activities = serde_json::json!({
            "agent_name": result.agent_name,
            "status": result.exit_status,
            "summary": result.summary,
            "is_running": false,
            "group_id": id,
        })
        .to_string();
        let session_id = self.current_session_id.clone().unwrap_or_default();
        let tool_msg = ChatMessage {
            id: 0,
            session_id,
            role: "tool".into(),
            content: result.output.clone(),
            agent_activities: Some(activities),
            reasoning_text: None,
            timestamp: crate::app::timestamp_now(),
            sequence: self.messages.len() as i64,
        };
        self.push_message(tool_msg);
    }

    pub fn mark_agent_running(&mut self, block_id: usize) {
        if let Some(block) = self
            .agent_blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
        {
            block.is_running = true;
            block.status = "running".to_string();
        }
    }

    pub fn toggle_agent_block(&mut self, block_id: usize) {
        if let Some(block) = self
            .agent_blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
        {
            block.is_expanded = !block.is_expanded;
        }
    }

    pub fn count_active_agents(&self) -> usize {
        self.agent_statuses
            .iter()
            .filter(|a| a.status != "done" && a.status != "idle")
            .count()
    }

    /// Append a message and notify the virtualized list. FIFO-evicts the
    /// oldest message when `MAX_MESSAGES` is exceeded.
    /// Returns the index of the newly-pushed message.
    pub fn push_message(&mut self, msg: ChatMessage) -> usize {
        let msgs = Rc::make_mut(&mut self.messages);
        let old_len = msgs.len();
        msgs.push(msg);
        if msgs.len() > MAX_MESSAGES {
            msgs.remove(0);
            self.scrolled_off_count = self.scrolled_off_count.saturating_add(1);
            self.list_state.splice(0..1, 0);
        } else {
            self.list_state.splice(old_len..old_len, 1);
        }
        if self.follow_tail {
            self.list_state.scroll_to_end();
        }
        msgs.len() - 1
    }

    /// Append text to message `idx` and remeasure that list item so its
    /// rendered height tracks the new content.
    pub fn append_to_message(&mut self, idx: usize, text: &str) {
        let msgs = Rc::make_mut(&mut self.messages);
        if let Some(msg) = msgs.get_mut(idx) {
            msg.content.push_str(text);
            self.list_state.remeasure_items(idx..idx + 1);
            if self.follow_tail {
                self.list_state.scroll_to_end();
            }
        }
    }

    pub fn set_message_content(&mut self, idx: usize, content: impl Into<String>) {
        let msgs = Rc::make_mut(&mut self.messages);
        if let Some(msg) = msgs.get_mut(idx) {
            msg.content = content.into();
            self.list_state.remeasure_items(idx..idx + 1);
            if self.follow_tail {
                self.list_state.scroll_to_end();
            }
        }
    }

    pub fn cycle_thinking_level(&mut self) {
        self.thinking_level = match self.thinking_level {
            ThinkingLevel::Low => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High => ThinkingLevel::XHigh,
            ThinkingLevel::XHigh => ThinkingLevel::Low,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::engine::AgentResult;

    #[test]
    fn test_ai_chat_panel_data_default() {
        let data = AiChatPanelData::default();
        assert!(data.messages.is_empty());
        assert!(!data.is_streaming);
        assert_eq!(data.selected_model, "");
        assert!(data.config_dropdown.is_none());
    }

    #[test]
    fn test_add_agent_block() {
        let mut data = AiChatPanelData::default();
        let result = AgentResult {
            agent_name: "test-agent".into(),
            exit_status: "completed".into(),
            output: "test output".into(),
            summary: Some("test summary".into()),
            pane_id: "pane-1".into(),
        };
        data.add_agent_block(&result);
        assert_eq!(data.agent_blocks.len(), 1);
        assert_eq!(data.agent_blocks[0].agent_name, "test-agent");
    }

    #[test]
    fn test_toggle_agent_block() {
        let mut data = AiChatPanelData::default();
        let result = AgentResult {
            agent_name: "a".into(),
            exit_status: "ok".into(),
            output: "o".into(),
            summary: None,
            pane_id: "p".into(),
        };
        data.add_agent_block(&result);
        assert!(!data.agent_blocks[0].is_expanded);
        data.toggle_agent_block(0);
        assert!(data.agent_blocks[0].is_expanded);
    }

    #[test]
    fn test_cycle_thinking_level() {
        let mut data = AiChatPanelData::default();
        assert_eq!(data.thinking_level, ThinkingLevel::Low);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::Medium);
    }

    #[test]
    fn test_count_active_agents() {
        let mut data = AiChatPanelData::default();
        data.agent_statuses = vec![
            AgentMonitorEntry {
                agent_name: "a".into(),
                pane_id: "p1".into(),
                status: "working".into(),
                recent_output: String::new(),
                is_expanded: false,
            },
            AgentMonitorEntry {
                agent_name: "b".into(),
                pane_id: "p2".into(),
                status: "done".into(),
                recent_output: String::new(),
                is_expanded: false,
            },
            AgentMonitorEntry {
                agent_name: "c".into(),
                pane_id: "p3".into(),
                status: "blocked".into(),
                recent_output: String::new(),
                is_expanded: false,
            },
        ];
        assert_eq!(data.count_active_agents(), 2);
    }

    #[test]
    fn test_workspace_option_creation() {
        let opt = WorkspaceOption {
            id: "ws-1".into(),
            label: "Main Project".into(),
            focused: false,
        };
        assert_eq!(opt.id, "ws-1");
        assert_eq!(opt.label, "Main Project");
    }

    #[test]
    fn test_mark_agent_running() {
        let mut data = AiChatPanelData::default();
        let result = AgentResult {
            agent_name: "test".into(),
            exit_status: "idle".into(),
            output: String::new(),
            summary: None,
            pane_id: "p".into(),
        };
        data.add_agent_block(&result);
        assert!(!data.agent_blocks[0].is_running);
        data.mark_agent_running(0);
        assert!(data.agent_blocks[0].is_running);
        assert_eq!(data.agent_blocks[0].status, "running");
    }

    #[test]
    fn test_new_has_zenix_provider() {
        let data = AiChatPanelData::new();
        assert!(data.messages.is_empty());
    }

    #[test]
    fn test_cycle_thinking_level_loops() {
        let mut data = AiChatPanelData::default();
        assert_eq!(data.thinking_level, ThinkingLevel::Low);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::Medium);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::High);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::XHigh);
        data.cycle_thinking_level();
        assert_eq!(data.thinking_level, ThinkingLevel::Low);
    }
}
