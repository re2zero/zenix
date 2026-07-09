//! Agent block — data types for agent output cards + monitor entries.
//!
//! Rendering of role="tool" messages lives in `tool_card.rs` (v4 Phase 2).
//! This module retains the data types (`AgentBlock`, `AgentMonitorEntry`)
//! used by `AiChatPanelData` runtime state and the drawer agent monitor.

use crate::ai::herdr::AgentStatus as HerdrAgentStatus;

// ── Data types ─────────────────────────────────────────────────────────

/// Runtime lifecycle state for an agent output block.
///
/// Tracked in `AiChatPanelData::agent_blocks`; a parallel `role="tool"`
/// ChatMessage enters the render flow (see `tool_card.rs`).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentBlock {
    pub agent_name: String,
    pub status: String,
    pub output: String,
    pub summary: Option<String>,
    pub pane_id: String,
    pub is_expanded: bool,
    pub block_id: usize,
    pub is_running: bool,
    pub output_history: String,
}

/// A single entry in the agent monitoring drawer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentMonitorEntry {
    pub agent_name: String,
    pub pane_id: String,
    pub status: String,
    pub recent_output: String,
    pub is_expanded: bool,
}

impl AgentMonitorEntry {
    /// Convert a herdr `AgentStatus` into a monitor entry.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_monitor_entry_from_herdr() {
        let entry = AgentMonitorEntry::from_herdr(
            "agent-1".into(),
            "pane-1".into(),
            HerdrAgentStatus::Working,
            "recent output".into(),
        );
        assert_eq!(entry.agent_name, "agent-1");
        assert_eq!(entry.status, "working");
    }
}
