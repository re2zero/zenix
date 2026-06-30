//! Herdr IPC client — JSON-line protocol over Unix socket.
//!
//! Communicates with the running herdr daemon to manage workspaces, tabs,
//! panes, and agents. All types are self-contained to avoid importing herdr
//! as a Rust crate (herdr is an external process).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::error::AiError;

// ── Wire Protocol Types ──────────────────────────────────────────────

/// A request sent to herdr (JSON-line format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HerdrRequest {
    pub id: String,
    #[serde(flatten)]
    pub method: HerdrMethod,
}

/// Supported herdr IPC methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum HerdrMethod {
    #[serde(rename = "server.ping")]
    ServerPing,

    #[serde(rename = "workspace.list")]
    WorkspaceList,

    #[serde(rename = "workspace.get")]
    WorkspaceGet(WorkspaceTarget),

    #[serde(rename = "workspace.create")]
    WorkspaceCreate(WorkspaceCreateParams),

    #[serde(rename = "workspace.close")]
    WorkspaceClose(WorkspaceTarget),

    #[serde(rename = "tab.list")]
    TabList(TabListParams),

    #[serde(rename = "tab.get")]
    TabGet(TabTarget),

    #[serde(rename = "tab.create")]
    TabCreate(TabCreateParams),

    #[serde(rename = "agent.list")]
    AgentList,

    #[serde(rename = "agent.start")]
    AgentStart(AgentStartParams),

    #[serde(rename = "agent.send")]
    AgentSend(AgentSendParams),

    #[serde(rename = "agent.read")]
    AgentRead(AgentReadParams),

    #[serde(rename = "pane.list")]
    PaneList(PaneListParams),

    #[serde(rename = "pane.get")]
    PaneGet(PaneTarget),

    #[serde(rename = "pane.split")]
    PaneSplit(PaneSplitParams),

    #[serde(rename = "pane.send_text")]
    PaneSendText(PaneSendTextParams),

    #[serde(rename = "pane.read")]
    PaneRead(PaneReadParams),

    #[serde(rename = "pane.wait_for_output")]
    PaneWaitForOutput(PaneWaitForOutputParams),

    #[serde(rename = "events.subscribe")]
    EventsSubscribe(EventsSubscribeParams),
}

// ── Request Parameters ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTarget {
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTarget {
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTarget {
    pub tab_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TabCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TabListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSendParams {
    pub target: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReadParams {
    pub target: String,
    pub source: ReadSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(default)]
    pub format: ReadFormat,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStartParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<SplitDirection>,
    #[serde(default)]
    pub focus: bool,
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PaneListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSplitParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pane_id: Option<String>,
    pub direction: SplitDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSendTextParams {
    pub pane_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneReadParams {
    pub pane_id: String,
    pub source: ReadSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(default)]
    pub format: ReadFormat,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneWaitForOutputParams {
    pub pane_id: String,
    pub source: ReadSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    pub r#match: OutputMatch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsSubscribeParams {
    pub subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Subscription {
    #[serde(rename = "workspace.created")]
    WorkspaceCreated {},

    #[serde(rename = "workspace.updated")]
    WorkspaceUpdated {},

    #[serde(rename = "workspace.renamed")]
    WorkspaceRenamed {},

    #[serde(rename = "workspace.closed")]
    WorkspaceClosed {},

    #[serde(rename = "workspace.focused")]
    WorkspaceFocused {},

    #[serde(rename = "tab.created")]
    TabCreated {},

    #[serde(rename = "tab.closed")]
    TabClosed {},

    #[serde(rename = "tab.focused")]
    TabFocused {},

    #[serde(rename = "tab.renamed")]
    TabRenamed {},

    #[serde(rename = "pane.created")]
    PaneCreated {},

    #[serde(rename = "pane.closed")]
    PaneClosed {},

    #[serde(rename = "pane.focused")]
    PaneFocused {},

    #[serde(rename = "pane.exited")]
    PaneExited {},

    #[serde(rename = "pane.agent_detected")]
    PaneAgentDetected {},

    #[serde(rename = "pane.output_matched")]
    PaneOutputMatched {
        pane_id: String,
        source: ReadSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lines: Option<u32>,
        r#match: OutputMatch,
        #[serde(default = "default_true")]
        strip_ansi: bool,
    },

    #[serde(rename = "pane.agent_status_changed")]
    PaneAgentStatusChanged {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_status: Option<AgentStatus>,
    },
}

// ── Response Types ───────────────────────────────────────────────────

/// A success response from herdr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessResponse {
    pub id: String,
    pub result: ResponseResult,
}

/// An error response from herdr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub id: String,
    pub error: ErrorBody,
}

/// Error body returned by herdr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// Parsed response from herdr.
#[derive(Debug, Clone)]
pub enum HerdrResponse {
    Success(SuccessResponse),
    Error(ErrorResponse),
}

/// Typed response payloads from herdr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Pong {
        version: String,
        protocol: u32,
        #[serde(default)]
        capabilities: Option<ServerCapabilities>,
    },

    WorkspaceInfo { workspace: WorkspaceInfo },

    WorkspaceCreated {
        workspace: WorkspaceInfo,
        tab: TabInfo,
        root_pane: PaneInfo,
    },

    WorkspaceList { workspaces: Vec<WorkspaceInfo> },

    TabInfo { tab: TabInfo },

    TabCreated { tab: TabInfo, root_pane: PaneInfo },

    TabList { tabs: Vec<TabInfo> },

    AgentInfo { agent: AgentInfo },

    AgentStarted { agent: AgentInfo, argv: Vec<String> },

    AgentList { agents: Vec<AgentInfo> },

    PaneInfo { pane: PaneInfo },

    PaneList { panes: Vec<PaneInfo> },

    PaneRead { read: PaneReadResult },

    SubscriptionStarted {},

    OutputMatched {
        pane_id: String,
        revision: u64,
        matched_line: Option<String>,
        read: PaneReadResult,
    },

    Ok {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub live_handoff: bool,
}

// ── Data Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub workspace_id: String,
    pub number: usize,
    pub label: String,
    pub focused: bool,
    pub pane_count: usize,
    pub tab_count: usize,
    pub active_tab_id: String,
    pub agent_status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorkspaceWorktreeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceWorktreeInfo {
    pub repo_key: String,
    pub repo_name: String,
    pub repo_root: String,
    pub checkout_path: String,
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: usize,
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    pub pane_count: usize,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub terminal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_agent: Option<String>,
    pub agent_status: AgentStatus,
    #[serde(default, skip_serializing_if = "is_false")]
    pub screen_detection_skipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_status: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub state_labels: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSessionInfo>,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_cwd: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionInfo {
    pub source: String,
    pub agent: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub focused: bool,
    pub cwd: Option<String>,
    pub foreground_cwd: Option<String>,
    pub label: Option<String>,
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_agent: Option<String>,
    pub agent_status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_status: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub state_labels: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSessionInfo>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneReadResult {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub source: ReadSource,
    pub format: ReadFormat,
    pub text: String,
    pub revision: u64,
    pub truncated: bool,
}

/// Agent status values matching herdr's AgentStatus enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl AgentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Option<AgentStatus> {
        match s {
            "idle" => Some(AgentStatus::Idle),
            "working" => Some(AgentStatus::Working),
            "blocked" => Some(AgentStatus::Blocked),
            "done" => Some(AgentStatus::Done),
            "unknown" => Some(AgentStatus::Unknown),
            _ => None,
        }
    }
}

/// Read source for pane/agent output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadSource {
    Visible,
    Recent,
    RecentUnwrapped,
    Detection,
}

/// Read format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat {
    #[default]
    Text,
    Ansi,
}

/// Output match criteria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputMatch {
    Substring { value: String },
    Regex { value: String },
}

/// Pane split direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Right,
    Down,
}

/// Event envelope from herdr's events.stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event: EventKind,
    pub data: EventData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    WorkspaceCreated,
    WorkspaceUpdated,
    WorkspaceClosed,
    WorkspaceRenamed,
    WorkspaceFocused,
    TabCreated,
    TabClosed,
    TabRenamed,
    TabFocused,
    PaneCreated,
    PaneClosed,
    PaneFocused,
    PaneOutputChanged,
    PaneExited,
    PaneAgentDetected,
    PaneAgentStatusChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventData {
    WorkspaceCreated { workspace: WorkspaceInfo },
    WorkspaceUpdated { workspace: WorkspaceInfo },
    WorkspaceClosed { workspace_id: String },
    WorkspaceRenamed { workspace_id: String, label: String },
    WorkspaceFocused { workspace_id: String },
    TabCreated { tab: TabInfo },
    TabClosed { tab_id: String, workspace_id: String },
    TabRenamed { tab_id: String, workspace_id: String, label: String },
    TabFocused { tab_id: String, workspace_id: String },
    PaneCreated { pane: PaneInfo },
    PaneClosed { pane_id: String, workspace_id: String },
    PaneFocused { pane_id: String, workspace_id: String },
    PaneOutputChanged {
        pane_id: String,
        workspace_id: String,
        revision: u64,
    },
    PaneExited { pane_id: String, workspace_id: String },
    PaneAgentDetected {
        pane_id: String,
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
    },
    PaneAgentStatusChanged {
        pane_id: String,
        workspace_id: String,
        agent_status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_agent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom_status: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        state_labels: HashMap<String, String>,
    },
}

// ── Result Types ─────────────────────────────────────────────────────

/// Result of creating a workspace via herdr.
pub struct CreateWorkspaceResult {
    pub workspace: WorkspaceInfo,
    pub tab: TabInfo,
    pub pane: PaneInfo,
}

/// Result of creating a tab via herdr.
pub struct CreateTabResult {
    pub tab: TabInfo,
    pub pane: PaneInfo,
}

// ── Helper Functions ─────────────────────────────────────────────────

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
}

// ── HerdrIpcClient ───────────────────────────────────────────────────

/// Client for communicating with the herdr daemon over its Unix socket.
///
/// Uses JSON-line protocol: each request and response is a complete JSON
/// object terminated by a newline character.
pub struct HerdrIpcClient {
    socket_path: PathBuf,
    next_id: u64,
}

impl HerdrIpcClient {
    /// Create a new client targeting the given socket path.
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            next_id: 0,
        }
    }

    /// Connect to herdr and verify it's responsive via a ping.
    pub fn connect(&mut self) -> Result<(), AiError> {
        let stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("connect to herdr socket {:?}", self.socket_path))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| AiError::IpcError(format!("set read timeout: {e}")))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| AiError::IpcError(format!("set write timeout: {e}")))?;

        // Send ping and verify response
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::ServerPing,
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::Pong { version, .. } => {
                    tracing::debug!("herdr connected, version={version}");
                    Ok(())
                }
                _ => Err(AiError::IpcError("unexpected ping response".into())),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// List all workspaces known to herdr.
    pub fn list_workspaces(&mut self) -> Result<Vec<WorkspaceInfo>, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::WorkspaceList,
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::WorkspaceList { workspaces } => Ok(workspaces),
                other => Err(AiError::IpcError(format!(
                    "unexpected workspace.list response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Get a single workspace by ID.
    pub fn get_workspace(&mut self, id: &str) -> Result<WorkspaceInfo, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::WorkspaceGet(WorkspaceTarget {
                workspace_id: id.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::WorkspaceInfo { workspace } => Ok(workspace),
                other => Err(AiError::IpcError(format!(
                    "unexpected workspace.get response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Create a new workspace at the given CWD.
    ///
    /// Returns the created workspace, initial tab, and root pane.
    pub fn create_workspace(
        &mut self,
        cwd: &str,
        label: Option<&str>,
    ) -> Result<CreateWorkspaceResult, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::WorkspaceCreate(WorkspaceCreateParams {
                cwd: Some(cwd.to_string()),
                focus: true,
                label: label.map(String::from),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::WorkspaceCreated {
                    workspace,
                    tab,
                    root_pane,
                } => Ok(CreateWorkspaceResult {
                    workspace,
                    tab,
                    pane: root_pane,
                }),
                other => Err(AiError::IpcError(format!(
                    "unexpected workspace.create response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Close (destroy) a workspace.
    pub fn close_workspace(&mut self, workspace_id: &str) -> Result<(), AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::WorkspaceClose(WorkspaceTarget {
                workspace_id: workspace_id.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::Ok {} => Ok(()),
                _ => Err(AiError::IpcError("unexpected response".into())),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// List tabs, optionally filtered by workspace.
    pub fn list_tabs(
        &mut self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<TabInfo>, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::TabList(TabListParams {
                workspace_id: workspace_id.map(String::from),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::TabList { tabs } => Ok(tabs),
                other => Err(AiError::IpcError(format!(
                    "unexpected tab.list response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Get a single tab by ID.
    pub fn get_tab(&mut self, tab_id: &str) -> Result<TabInfo, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::TabGet(TabTarget {
                tab_id: tab_id.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::TabInfo { tab } => Ok(tab),
                other => Err(AiError::IpcError(format!(
                    "unexpected tab.get response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Create a new tab in a workspace.
    pub fn create_tab(
        &mut self,
        workspace_id: &str,
        cwd: Option<&str>,
        label: Option<&str>,
    ) -> Result<CreateTabResult, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::TabCreate(TabCreateParams {
                workspace_id: Some(workspace_id.to_string()),
                cwd: cwd.map(String::from),
                focus: true,
                label: label.map(String::from),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::TabCreated { tab, root_pane } => Ok(CreateTabResult {
                    tab,
                    pane: root_pane,
                }),
                other => Err(AiError::IpcError(format!(
                    "unexpected tab.create response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// List all agents currently running in herdr.
    pub fn list_agents(&mut self) -> Result<Vec<AgentInfo>, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::AgentList,
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::AgentList { agents } => Ok(agents),
                other => Err(AiError::IpcError(format!(
                    "unexpected agent.list response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Start an agent CLI in a new or existing pane.
    pub fn start_agent(
        &mut self,
        name: &str,
        workspace_id: Option<&str>,
        cwd: Option<&str>,
        argv: Vec<String>,
    ) -> Result<AgentInfo, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::AgentStart(AgentStartParams {
                name: name.to_string(),
                cwd: cwd.map(String::from),
                workspace_id: workspace_id.map(String::from),
                tab_id: None,
                split: None,
                focus: true,
                argv,
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::AgentStarted { agent, .. } => Ok(agent),
                other => Err(AiError::IpcError(format!(
                    "unexpected agent.start response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Send text to an agent via its target identifier.
    pub fn send_agent_text(&mut self, target: &str, text: &str) -> Result<(), AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::AgentSend(AgentSendParams {
                target: target.to_string(),
                text: text.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::Ok {} => Ok(()),
                _ => Err(AiError::IpcError("unexpected response".into())),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Read output from an agent via its target identifier.
    pub fn read_agent(
        &mut self,
        target: &str,
        source: ReadSource,
        lines: u32,
    ) -> Result<String, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::AgentRead(AgentReadParams {
                target: target.to_string(),
                source,
                lines: Some(lines),
                format: ReadFormat::Text,
                strip_ansi: true,
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::PaneRead { read } => Ok(read.text),
                other => Err(AiError::IpcError(format!(
                    "unexpected agent.read response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// List panes in a workspace.
    pub fn list_panes(
        &mut self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<PaneInfo>, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneList(PaneListParams {
                workspace_id: workspace_id.map(String::from),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::PaneList { panes } => Ok(panes),
                other => Err(AiError::IpcError(format!(
                    "unexpected pane.list response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Get a single pane by ID.
    pub fn get_pane(&mut self, pane_id: &str) -> Result<PaneInfo, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneGet(PaneTarget {
                pane_id: pane_id.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::PaneInfo { pane } => Ok(pane),
                other => Err(AiError::IpcError(format!(
                    "unexpected pane.get response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Split a pane in the given direction.
    pub fn split_pane(
        &mut self,
        workspace_id: Option<&str>,
        direction: SplitDirection,
        cwd: Option<&str>,
    ) -> Result<PaneInfo, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneSplit(PaneSplitParams {
                workspace_id: workspace_id.map(String::from),
                target_pane_id: None,
                direction,
                ratio: None,
                cwd: cwd.map(String::from),
                focus: true,
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::PaneInfo { pane } => Ok(pane),
                other => Err(AiError::IpcError(format!(
                    "unexpected pane.split response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Send text to a specific pane.
    pub fn send_pane_text(&mut self, pane_id: &str, text: &str) -> Result<(), AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneSendText(PaneSendTextParams {
                pane_id: pane_id.to_string(),
                text: text.to_string(),
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::Ok {} => Ok(()),
                _ => Err(AiError::IpcError("unexpected response".into())),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Read output from a specific pane.
    pub fn read_pane(
        &mut self,
        pane_id: &str,
        source: ReadSource,
        lines: u32,
    ) -> Result<String, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneRead(PaneReadParams {
                pane_id: pane_id.to_string(),
                source,
                lines: Some(lines),
                format: ReadFormat::Text,
                strip_ansi: true,
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::PaneRead { read } => Ok(read.text),
                other => Err(AiError::IpcError(format!(
                    "unexpected pane.read response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    /// Wait for output from a pane matching the given substring.
    ///
    /// Returns the matched text on success, or `AiError::Timeout` if the
    /// timeout elapses before a match is found.
    pub fn wait_for_output(
        &mut self,
        pane_id: &str,
        match_text: &str,
        timeout_ms: u64,
    ) -> Result<String, AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::PaneWaitForOutput(PaneWaitForOutputParams {
                pane_id: pane_id.to_string(),
                source: ReadSource::Recent,
                lines: Some(200),
                r#match: OutputMatch::Substring {
                    value: match_text.to_string(),
                },
                timeout_ms: Some(timeout_ms),
                strip_ansi: true,
            }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::OutputMatched { matched_line, .. } => {
                    Ok(matched_line.unwrap_or_default())
                }
                other => Err(AiError::IpcError(format!(
                    "unexpected wait_for_output response: {:?}",
                    other
                ))),
            },
            HerdrResponse::Error(err) => {
                // Convert known timeout errors
                if err.error.code == "timeout" || err.error.message.contains("timeout") {
                    Err(AiError::Timeout(format!(
                        "wait_for_output timed out: {}",
                        err.error.message
                    )))
                } else {
                    Err(AiError::IpcError(err.error.message))
                }
            }
        }
    }

    /// Subscribe to herdr events.
    pub fn subscribe_events(
        &mut self,
        subscriptions: Vec<Subscription>,
    ) -> Result<(), AiError> {
        let request = HerdrRequest {
            id: self.next_id(),
            method: HerdrMethod::EventsSubscribe(EventsSubscribeParams { subscriptions }),
        };
        let result = self.send_request_raw(&request)?;
        match result {
            HerdrResponse::Success(resp) => match resp.result {
                ResponseResult::SubscriptionStarted {} => Ok(()),
                _ => Err(AiError::IpcError("unexpected response".into())),
            },
            HerdrResponse::Error(err) => Err(AiError::IpcError(err.error.message)),
        }
    }

    // ── Internal ───────────────────────────────────────────────────

    /// Generate the next unique request ID.
    fn next_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("zenix-req-{id}")
    }

    /// Send a raw request and receive the response.
    ///
    /// Serializes the request as a JSON-line object, writes it to the socket,
    /// then reads a single JSON-line response.
    fn send_request_raw(&mut self, request: &HerdrRequest) -> Result<HerdrResponse, AiError> {
        let request_json = serde_json::to_string(request)
            .context("serialize request")
            .map_err(|e: anyhow::Error| AiError::SerializationError(e.to_string()))?;

        let stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("connect to herdr socket {:?}", self.socket_path))?;

        {
            let mut writer = std::io::BufWriter::new(&stream);
            writeln!(writer, "{request_json}")
                .with_context(|| "write request to socket")
                .map_err(|e| AiError::IpcError(format!("write to socket: {e}")))?;
            writer
                .flush()
                .map_err(|e| AiError::IpcError(format!("flush socket: {e}")))?;
        }

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .with_context(|| "read response from socket")
            .map_err(|e| AiError::IpcError(format!("read from socket: {e}")))?;

        let line = line.trim();
        if line.is_empty() {
            return Err(AiError::IpcError("empty response".into()));
        }

        parse_response(line)
    }
}

/// Parse a JSON-line response string into a typed `HerdrResponse`.
fn parse_response(line: &str) -> Result<HerdrResponse, AiError> {
    // Check for error first: both SuccessResponse and ErrorResponse have an
    // "id" field, so we must inspect the JSON structure before deserializing.
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| AiError::SerializationError(format!("invalid JSON: {e}")))?;

    if value.get("error").is_some() {
        let error: ErrorResponse = serde_json::from_value(value)
            .map_err(|e| AiError::SerializationError(format!("parse error response: {e}")))?;
        return Ok(HerdrResponse::Error(error));
    }

    let success: SuccessResponse = serde_json::from_value(value)
        .map_err(|e| AiError::SerializationError(format!("parse success response: {e}")))?;
    Ok(HerdrResponse::Success(success))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_generation() {
        let mut client = HerdrIpcClient::new(PathBuf::from("/tmp/test.sock"));
        let id1 = client.next_id();
        let id2 = client.next_id();
        assert!(id1.starts_with("zenix-req-"));
        assert!(id2.starts_with("zenix-req-"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ping_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-0".into(),
            method: HerdrMethod::ServerPing,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"server.ping\""));
        assert!(json.contains("\"id\":\"zenix-req-0\""));
    }

    #[test]
    fn test_workspace_list_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-1".into(),
            method: HerdrMethod::WorkspaceList,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"workspace.list\""));
    }

    #[test]
    fn test_workspace_get_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-2".into(),
            method: HerdrMethod::WorkspaceGet(WorkspaceTarget {
                workspace_id: "w-1".into(),
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"workspace.get\""));
        assert!(json.contains("\"workspace_id\":\"w-1\""));
    }

    #[test]
    fn test_workspace_create_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-3".into(),
            method: HerdrMethod::WorkspaceCreate(WorkspaceCreateParams {
                cwd: Some("/tmp/project".into()),
                focus: true,
                label: Some("My Project".into()),
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"workspace.create\""));
        assert!(json.contains("\"cwd\":\"/tmp/project\""));
        assert!(json.contains("\"label\":\"My Project\""));
    }

    #[test]
    fn test_agent_start_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-4".into(),
            method: HerdrMethod::AgentStart(AgentStartParams {
                name: "claude".into(),
                cwd: Some("/tmp/project".into()),
                workspace_id: Some("w-1".into()),
                tab_id: None,
                split: None,
                focus: true,
                argv: vec!["claude".into()],
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"agent.start\""));
        assert!(json.contains("\"name\":\"claude\""));
        assert!(json.contains("\"workspace_id\":\"w-1\""));
    }

    #[test]
    fn test_pane_send_text_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-5".into(),
            method: HerdrMethod::PaneSendText(PaneSendTextParams {
                pane_id: "p-1".into(),
                text: "hello world\n".into(),
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"pane.send_text\""));
        assert!(json.contains("\"pane_id\":\"p-1\""));
        assert!(json.contains("hello world"));
    }

    #[test]
    fn test_pane_read_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-6".into(),
            method: HerdrMethod::PaneRead(PaneReadParams {
                pane_id: "p-1".into(),
                source: ReadSource::Recent,
                lines: Some(100),
                format: ReadFormat::Text,
                strip_ansi: true,
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"pane.read\""));
        assert!(json.contains("\"source\":\"recent\""));
        assert!(json.contains("\"lines\":100"));
    }

    #[test]
    fn test_pane_wait_for_output_request_serializes() {
        let request = HerdrRequest {
            id: "zenix-req-7".into(),
            method: HerdrMethod::PaneWaitForOutput(PaneWaitForOutputParams {
                pane_id: "p-1".into(),
                source: ReadSource::Recent,
                lines: Some(200),
                r#match: OutputMatch::Substring {
                    value: "all tests passed".into(),
                },
                timeout_ms: Some(300000),
                strip_ansi: true,
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"pane.wait_for_output\""));
        assert!(json.contains("\"match\":{\"type\":\"substring\""));
        assert!(json.contains("\"value\":\"all tests passed\""));
    }

    #[test]
    fn test_success_response_round_trips() {
        let json = r#"{"id":"req_1","result":{"type":"pong","version":"0.6.10","protocol":6}}"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Success(s) => {
                assert_eq!(s.id, "req_1");
                match s.result {
                    ResponseResult::Pong { version, protocol, .. } => {
                        assert_eq!(version, "0.6.10");
                        assert_eq!(protocol, 6);
                    }
                    _ => panic!("expected pong"),
                }
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn test_error_response_round_trips() {
        let json = r#"{"id":"req_2","error":{"code":"pane_not_found","message":"pane p-1 not found"}}"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Error(e) => {
                assert_eq!(e.id, "req_2");
                assert_eq!(e.error.code, "pane_not_found");
                assert_eq!(e.error.message, "pane p-1 not found");
            }
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn test_workspace_list_response_round_trips() {
        let json = r#"{
            "id": "req_3",
            "result": {
                "type": "workspace_list",
                "workspaces": [{
                    "workspace_id": "w-1",
                    "number": 1,
                    "label": "herdr",
                    "focused": true,
                    "pane_count": 2,
                    "tab_count": 1,
                    "active_tab_id": "w-1:1",
                    "agent_status": "unknown"
                }]
            }
        }"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Success(s) => {
                assert_eq!(s.id, "req_3");
                match s.result {
                    ResponseResult::WorkspaceList { workspaces } => {
                        assert_eq!(workspaces.len(), 1);
                        assert_eq!(workspaces[0].workspace_id, "w-1");
                        assert_eq!(workspaces[0].label, "herdr");
                    }
                    _ => panic!("expected workspace_list"),
                }
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn test_agent_started_response_round_trips() {
        let json = r#"{
            "id": "req_4",
            "result": {
                "type": "agent_started",
                "agent": {
                    "terminal_id": "t-1",
                    "agent": "claude",
                    "agent_status": "working",
                    "workspace_id": "w-1",
                    "tab_id": "w-1:1",
                    "pane_id": "p-1",
                    "revision": 1
                },
                "argv": ["claude", "-p", "task"]
            }
        }"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Success(s) => {
                match s.result {
                    ResponseResult::AgentStarted { agent, argv } => {
                        assert_eq!(agent.agent.as_deref(), Some("claude"));
                        assert_eq!(agent.agent_status, AgentStatus::Working);
                        assert_eq!(argv, vec!["claude", "-p", "task"]);
                    }
                    _ => panic!("expected agent_started"),
                }
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn test_output_matched_response_round_trips() {
        let json = r#"{
            "id": "req_5",
            "result": {
                "type": "output_matched",
                "pane_id": "p-1",
                "revision": 42,
                "matched_line": "All tests passed!",
                "read": {
                    "pane_id": "p-1",
                    "workspace_id": "w-1",
                    "tab_id": "w-1:1",
                    "source": "recent",
                    "format": "text",
                    "text": "All tests passed!\n",
                    "revision": 42,
                    "truncated": false
                }
            }
        }"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Success(s) => {
                match s.result {
                    ResponseResult::OutputMatched {
                        matched_line,
                        read,
                        ..
                    } => {
                        assert_eq!(matched_line.as_deref(), Some("All tests passed!"));
                        assert_eq!(read.text, "All tests passed!\n");
                    }
                    _ => panic!("expected output_matched"),
                }
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn test_parse_response_empty_line_returns_error() {
        let result = parse_response("");
        assert!(result.is_err());
        match result.unwrap_err() {
            AiError::SerializationError(msg) => {
                assert!(msg.contains("invalid JSON"));
            }
            _ => panic!("expected serialization error"),
        }
    }

    #[test]
    fn test_parse_response_invalid_json_returns_error() {
        let result = parse_response("not json at all");
        assert!(result.is_err());
        match result.unwrap_err() {
            AiError::SerializationError(msg) => {
                assert!(msg.contains("invalid JSON"));
            }
            _ => panic!("expected serialization error"),
        }
    }

    #[test]
    fn test_parse_response_error_response_is_not_success() {
        // Error responses must be classified as Error, not Success.
        // Previously, ErrorResponse was deserialized as SuccessResponse
        // because both have an "id" field and SuccessResponse's result
        // field has #[serde(default)].
        let json = r#"{"id":"req-4","error":{"code":"agent_binary_not_found","message":"claude not found"}}"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Error(e) => {
                assert_eq!(e.id, "req-4");
                assert_eq!(e.error.code, "agent_binary_not_found");
            }
            _ => panic!("expected error response, got success"),
        }
    }

    #[test]
    fn test_parse_response_success_response() {
        let json = r#"{"id":"req-1","result":{"type":"pong","version":"0.6.10","protocol":6}}"#;
        let resp = parse_response(json).unwrap();
        match resp {
            HerdrResponse::Success(s) => {
                assert_eq!(s.id, "req-1");
                match s.result {
                    ResponseResult::Pong { version, protocol, .. } => {
                        assert_eq!(version, "0.6.10");
                        assert_eq!(protocol, 6);
                    }
                    _ => panic!("expected pong"),
                }
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn test_agent_status_round_trips() {
        for status in [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Blocked,
            AgentStatus::Done,
            AgentStatus::Unknown,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let restored: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, restored);
            assert_eq!(status.as_str(), status.as_str());
        }
    }

    #[test]
    fn test_read_source_round_trips() {
        for source in [
            ReadSource::Visible,
            ReadSource::Recent,
            ReadSource::RecentUnwrapped,
            ReadSource::Detection,
        ] {
            let json = serde_json::to_string(&source).unwrap();
            let restored: ReadSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, restored);
        }
    }

    #[test]
    fn test_split_direction_round_trips() {
        for dir in [SplitDirection::Right, SplitDirection::Down] {
            let json = serde_json::to_string(&dir).unwrap();
            let restored: SplitDirection = serde_json::from_str(&json).unwrap();
            assert_eq!(dir, restored);
        }
    }

    #[test]
    fn test_output_match_round_trips() {
        let match_ = OutputMatch::Substring {
            value: "done".into(),
        };
        let json = serde_json::to_string(&match_).unwrap();
        assert!(json.contains("\"type\":\"substring\""));
        let restored: OutputMatch = serde_json::from_str(&json).unwrap();
        assert_eq!(match_, restored);
    }

    #[test]
    fn test_subscription_round_trips() {
        let sub = Subscription::PaneAgentStatusChanged {
            pane_id: "p-1".into(),
            agent_status: Some(AgentStatus::Done),
        };
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains("\"type\":\"pane.agent_status_changed\""));
        let restored: Subscription = serde_json::from_str(&json).unwrap();
        assert_eq!(sub, restored);
    }

    #[test]
    fn test_event_envelope_round_trips() {
        let event = EventEnvelope {
            event: EventKind::WorkspaceCreated,
            data: EventData::WorkspaceCreated {
                workspace: WorkspaceInfo {
                    workspace_id: "w-1".into(),
                    number: 1,
                    label: "test".into(),
                    focused: true,
                    pane_count: 1,
                    tab_count: 1,
                    active_tab_id: "w-1:1".into(),
                    agent_status: AgentStatus::Unknown,
                    worktree: None,
                },
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"workspace_created\""));
        let restored: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(event, restored);
    }

    #[test]
    fn test_create_workspace_result_types() {
        // Verify the result structs compile and can be constructed
        let _ = CreateWorkspaceResult {
            workspace: WorkspaceInfo {
                workspace_id: "w-1".into(),
                number: 1,
                label: "test".into(),
                focused: true,
                pane_count: 1,
                tab_count: 1,
                active_tab_id: "w-1:1".into(),
                agent_status: AgentStatus::Unknown,
                worktree: None,
            },
            tab: TabInfo {
                tab_id: "w-1:1".into(),
                workspace_id: "w-1".into(),
                number: 1,
                label: "test".into(),
                focused: true,
                pane_count: 1,
                agent_status: AgentStatus::Unknown,
            },
            pane: PaneInfo {
                pane_id: "p-1".into(),
                terminal_id: "t-1".into(),
                workspace_id: "w-1".into(),
                tab_id: "w-1:1".into(),
                focused: true,
                cwd: None,
                foreground_cwd: None,
                label: None,
                agent: None,
                title: None,
                display_agent: None,
                agent_status: AgentStatus::Unknown,
                custom_status: None,
                state_labels: HashMap::new(),
                agent_session: None,
                revision: 0,
            },
        };

        let _ = CreateTabResult {
            tab: TabInfo {
                tab_id: "w-1:2".into(),
                workspace_id: "w-1".into(),
                number: 2,
                label: "review".into(),
                focused: false,
                pane_count: 1,
                agent_status: AgentStatus::Unknown,
            },
            pane: PaneInfo {
                pane_id: "p-2".into(),
                terminal_id: "t-2".into(),
                workspace_id: "w-1".into(),
                tab_id: "w-1:2".into(),
                focused: false,
                cwd: None,
                foreground_cwd: None,
                label: None,
                agent: None,
                title: None,
                display_agent: None,
                agent_status: AgentStatus::Unknown,
                custom_status: None,
                state_labels: HashMap::new(),
                agent_session: None,
                revision: 0,
            },
        };
    }
}
