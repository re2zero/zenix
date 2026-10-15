//! Built-in tool system for the ZenixAI orchestrator.
//!
//! Tools are callable functions that the LLM can invoke via function calling.
//! Each tool implements the `Tool` trait with a name, description, JSON Schema
//! parameters, and an execute method.
//!
//! The `ToolExecutor` holds the shared [`ApiEndpoint`] — the daemon's JSON API
//! lane, the only one that carries the agent and pane-read methods these tools
//! are defined on — and is passed to every tool's execute method.
//!
//! Built-in tools (6):
//!   - `spawn_worker`:   Split a pane, start an agent CLI in it, send the task
//!   - `send_text`:      Send text/instruction to a worker pane
//!   - `read_output`:    Read a worker's recent output
//!   - `wait_for_output`: Wait for a completion pattern from a worker
//!   - `list_workers`:   List running workers
//!   - `close_worker`:   Terminate and remove a worker pane
//!
//! The tool names, parameter schemas and result texts are the MCP contract
//! (`crate::mcp::handler`), so their shapes stay put across this rewrite.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::ai::endpoint::{ApiEndpoint, EndpointError, ReadSource, SplitDirection};

/// How long the daemon gets to launch an interactive agent CLI in a pane. The
/// daemon accepts 3000..=300000 ms.
const AGENT_START_TIMEOUT_MS: u64 = 30_000;

/// Lines a pattern wait keeps as its result, so a match arrives with context.
const WAIT_CONTEXT_LINES: u32 = 200;

/// Recent lines a read returns by default.
const DEFAULT_READ_LINES: u32 = 100;

/// How much text a read result may carry before it is cut, in bytes.
const MAX_READ_BYTES: usize = 8_000;

// ── Core Types ─────────────────────────────────────────────────────────

/// A tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique tool call ID from the LLM (e.g. "call_abc123").
    pub id: String,
    /// Tool name (e.g. "spawn_worker").
    pub name: String,
    /// Parsed arguments as JSON.
    pub arguments: serde_json::Value,
}

/// Result from executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Name of the tool that produced this result.
    pub tool_name: String,
    /// Text content to send back to the LLM.
    pub content: String,
    /// Whether this is an error result.
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful result.
    pub fn ok(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error result.
    pub fn error(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            content: content.into(),
            is_error: true,
        }
    }
}

/// Trait that all tools implement.
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "spawn_worker").
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does (in Chinese).
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's parameters.
    fn parameters(&self) -> serde_json::Value;

    /// Execute the tool with the executor's shared state.
    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult;
}

// ── ToolExecutor ───────────────────────────────────────────────────────

/// Shared execution context passed to all tools.
///
/// Holds the daemon's JSON API endpoint and the workspace the orchestrator was
/// started for. An empty workspace id means "whatever the daemon has focused",
/// which is what the pane commands then omit.
pub struct ToolExecutor {
    endpoint: Arc<ApiEndpoint>,
    workspace_id: String,
}

impl ToolExecutor {
    /// Create a new tool executor.
    pub fn new(endpoint: ApiEndpoint, workspace_id: String) -> Self {
        Self {
            endpoint: Arc::new(endpoint),
            workspace_id,
        }
    }

    /// The endpoint every tool reaches the daemon through.
    pub fn endpoint(&self) -> &ApiEndpoint {
        &self.endpoint
    }

    /// The workspace pane commands start from; `None` when the executor follows
    /// the daemon's focused workspace.
    fn workspace(&self) -> Option<&str> {
        (!self.workspace_id.is_empty()).then_some(self.workspace_id.as_str())
    }

    /// Execute a single tool call, dispatching by name.
    pub fn execute(&self, call: &ToolCall) -> ToolResult {
        let all_tools = Self::available_tools();
        for tool in &all_tools {
            if tool.name() == call.name {
                return tool.execute(self, call.arguments.clone());
            }
        }
        ToolResult::error("tool_dispatch", format!("未知工具: {}", call.name))
    }

    /// Return all built-in tools available to the orchestrator.
    pub fn available_tools() -> Vec<Box<dyn Tool>> {
        builtin_tools()
    }
}

/// The one place a tool turns an API failure into the text the LLM sees.
fn report(tool: &str, action: &str, error: EndpointError) -> ToolResult {
    ToolResult::error(tool, format!("{action}: {error}"))
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool Implementations
// ═══════════════════════════════════════════════════════════════════════════

// ── spawn_worker ───────────────────────────────────────────────────────

/// Split a pane, start an agent CLI in it and send the task.
pub struct SpawnWorker;

impl SpawnWorker {
    /// Split once, in the direction that moves toward the requested grid cell.
    fn direction(position: Option<&serde_json::Value>) -> SplitDirection {
        let col = position
            .and_then(|pos| pos.get("col"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        if col > 0 {
            SplitDirection::Right
        } else {
            SplitDirection::Down
        }
    }
}

impl Tool for SpawnWorker {
    fn name(&self) -> &str {
        "spawn_worker"
    }

    fn description(&self) -> &str {
        "在 herdr 中创建一个新的 worker pane 并启动指定的 Agent CLI。支持通过 position 参数指定在网格中的位置（row, col）。返回分配的 pane_id 和 terminal_id。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Agent CLI 名称，例如 \"claude\"、\"codex\"、\"opencode\""
                },
                "instruction": {
                    "type": "string",
                    "description": "要发送给 Agent 的完整任务指令"
                },
                "cwd": {
                    "type": "string",
                    "description": "工作目录（可选，默认为 workspace 根目录）"
                },
                "worker_name": {
                    "type": "string",
                    "description": "Worker 名称（可选），用于 pane 标签显示，例如 oc_checkgit"
                },
                "position": {
                    "type": "object",
                    "description": "网格位置（可选）。例如 {\"row\": 0, \"col\": 0} 表示左上角",
                    "properties": {
                        "row": { "type": "integer", "description": "行号（从 0 开始）" },
                        "col": { "type": "integer", "description": "列号（从 0 开始）" }
                    }
                }
            },
            "required": ["agent", "instruction"]
        })
    }

    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
        let agent = match args.get("agent").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("spawn_worker", "参数 agent 是必需的非空字符串"),
        };
        let instruction = match args.get("instruction").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("spawn_worker", "参数 instruction 是必需的非空字符串"),
        };
        let cwd = args.get("cwd").and_then(|value| value.as_str());
        let worker_name = args.get("worker_name").and_then(|value| value.as_str());
        let position = args.get("position");
        let direction = Self::direction(position);

        // The daemon names agents itself and requires a lowercase name it can put
        // in a pane label, so an unusable `worker_name` is cleaned rather than
        // rejected.
        let name = agent_name(worker_name, agent);

        let endpoint = executor.endpoint();
        let pane = match endpoint.split_pane(executor.workspace(), cwd, direction) {
            Ok(pane) => pane,
            Err(error) => return report("spawn_worker", "创建 worker pane 失败", error),
        };

        let started = match endpoint.start_agent(
            &name,
            agent,
            &pane.pane_id,
            Vec::new(),
            AGENT_START_TIMEOUT_MS,
        ) {
            Ok(started) => started,
            Err(error) => {
                // The pane is already there; leaving it behind would silently change
                // the layout the caller asked for, so it is closed again.
                let _ = endpoint.close_pane(&pane.pane_id);
                return report("spawn_worker", &format!("启动 Agent '{agent}' 失败"), error);
            }
        };

        // `agent.start` returns once the launch command is written, not once the
        // CLI takes prompts; a prompt sent in between is refused with
        // `agent_not_ready`.
        if let Err(error) =
            endpoint.wait_for_agent_ready(&name, Duration::from_millis(AGENT_START_TIMEOUT_MS))
        {
            return ToolResult::error(
                "spawn_worker",
                format!(
                    "Worker '{agent}' 的 pane 已创建（pane_id={}），但 Agent 未就绪: {error}",
                    started.pane_id
                ),
            );
        }

        if let Err(error) = endpoint.prompt_agent(&name, instruction) {
            return ToolResult::error(
                "spawn_worker",
                format!(
                    "Worker '{agent}' 已启动（pane_id={}, name={}），但发送指令失败: {error}",
                    started.pane_id, name
                ),
            );
        }

        let label = worker_name.map(str::to_owned).unwrap_or_else(|| {
            let short: String = instruction.chars().take(16).collect();
            format!("{agent}: {short}...")
        });
        let _ = endpoint.rename_pane(&started.pane_id, &label);

        let terminal_id = started.terminal_id.as_deref().unwrap_or("-");
        let position = match position {
            Some(_) => {
                let row = position_value(position, "row");
                let col = position_value(position, "col");
                format!(", 位置=({row},{col})")
            }
            None => String::new(),
        };
        ToolResult::ok(
            "spawn_worker",
            format!(
                "Worker '{agent}' 已启动: pane_id={}, terminal_id={terminal_id}{position}",
                started.pane_id
            ),
        )
    }
}

fn position_value(position: Option<&serde_json::Value>, key: &str) -> u64 {
    position
        .and_then(|pos| pos.get(key))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

/// A pane label that fits the daemon's agent-name rule: lowercase letters,
/// digits, `-` and `_`, starting with a letter, at most 32 bytes.
pub(crate) fn agent_name(requested: Option<&str>, agent: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    let candidate = requested.filter(|name| !name.is_empty()).unwrap_or(agent);
    let mut cleaned: String = candidate
        .chars()
        .map(|ch| {
            let lowered = ch.to_ascii_lowercase();
            if lowered.is_ascii_lowercase()
                || lowered.is_ascii_digit()
                || matches!(lowered, '-' | '_')
            {
                lowered
            } else {
                '-'
            }
        })
        .collect();
    if !cleaned
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase())
    {
        cleaned.insert(0, 'w');
    }
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let tail = format!("-{suffix}");
    cleaned.truncate(32usize.saturating_sub(tail.len()));
    cleaned.push_str(&tail);
    cleaned
}

// ── send_text ──────────────────────────────────────────────────────────

/// Send text/instruction to a worker pane.
pub struct SendText;

impl Tool for SendText {
    fn name(&self) -> &str {
        "send_text"
    }

    fn description(&self) -> &str {
        "向指定 worker pane 发送文本指令或消息。Worker 会接收并处理该文本。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "目标 worker 的 pane ID"
                },
                "text": {
                    "type": "string",
                    "description": "要发送的文本内容"
                }
            },
            "required": ["pane_id", "text"]
        })
    }

    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("send_text", "参数 pane_id 是必需的非空字符串"),
        };
        let text = match args.get("text").and_then(|value| value.as_str()) {
            Some(value) => value,
            _ => return ToolResult::error("send_text", "参数 text 是必需的字符串"),
        };

        match executor
            .endpoint()
            .send_pane_text(pane_id, &format!("{text}\r"))
        {
            Ok(()) => ToolResult::ok("send_text", format!("文本已发送到 pane {pane_id}")),
            Err(error) => report("send_text", "发送文本失败", error),
        }
    }
}

// ── read_output ────────────────────────────────────────────────────────

/// Read recent output from a worker pane.
pub struct ReadOutput;

impl Tool for ReadOutput {
    fn name(&self) -> &str {
        "read_output"
    }

    fn description(&self) -> &str {
        "读取指定 worker pane 的最新输出内容。可用于检查 Agent 的执行进度和结果。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "目标 worker 的 pane ID"
                },
                "lines": {
                    "type": "integer",
                    "description": "读取最近 N 行输出，默认 100",
                    "default": 100
                }
            },
            "required": ["pane_id"]
        })
    }

    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("read_output", "参数 pane_id 是必需的非空字符串"),
        };
        let lines = args
            .get("lines")
            .and_then(|value| value.as_u64())
            .map(|lines| lines as u32)
            .unwrap_or(DEFAULT_READ_LINES);

        match executor
            .endpoint()
            .read_pane(pane_id, ReadSource::Recent, lines)
        {
            Ok(read) => ToolResult::ok("read_output", clipped(read.text)),
            Err(error) => report("read_output", &format!("读取 pane {pane_id} 失败"), error),
        }
    }
}

/// Keep a read result inside the model's budget, on a character boundary.
fn clipped(text: String) -> String {
    if text.len() <= MAX_READ_BYTES {
        return text;
    }
    let mut end = MAX_READ_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[已截断，共 {} 字符]", &text[..end], text.len())
}

// ── wait_for_output ────────────────────────────────────────────────────

/// Wait for a completion pattern from a worker pane.
pub struct WaitForOutput;

impl Tool for WaitForOutput {
    fn name(&self) -> &str {
        "wait_for_output"
    }

    fn description(&self) -> &str {
        "等待指定 worker pane 的输出中包含指定的模式（如 '[DONE]'），返回匹配时的输出内容。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "目标 worker 的 pane ID"
                },
                "pattern": {
                    "type": "string",
                    "description": "要匹配的输出模式字符串（substring 匹配）"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "超时时间（毫秒），默认 300000（5 分钟）",
                    "default": 300000
                }
            },
            "required": ["pane_id", "pattern"]
        })
    }

    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("wait_for_output", "参数 pane_id 是必需的非空字符串"),
        };
        let pattern = match args.get("pattern").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("wait_for_output", "参数 pattern 是必需的非空字符串"),
        };
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|value| value.as_u64())
            .unwrap_or(300_000);

        match executor
            .endpoint()
            .wait_for_output(pane_id, pattern, WAIT_CONTEXT_LINES, timeout_ms)
        {
            Ok(read) => ToolResult::ok("wait_for_output", clipped(read.text)),
            Err(error) => report("wait_for_output", "等待超时或失败", error),
        }
    }
}

// ── list_workers ───────────────────────────────────────────────────────

/// List running workers.
pub struct ListWorkers;

impl Tool for ListWorkers {
    fn name(&self) -> &str {
        "list_workers"
    }

    fn description(&self) -> &str {
        "查看当前 herdr 中所有运行中的 worker（Agent），包括名称、pane_id、状态等信息。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn execute(&self, executor: &ToolExecutor, _args: serde_json::Value) -> ToolResult {
        let agents = match executor.endpoint().list_agents() {
            Ok(agents) => agents,
            Err(error) => return report("list_workers", "获取 worker 列表失败", error),
        };
        if agents.is_empty() {
            return ToolResult::ok("list_workers", "当前没有运行中的 worker。");
        }
        let lines: Vec<String> = agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                format!(
                    "{}. {} | pane={} | status={} | workspace={}",
                    index + 1,
                    agent.name.as_deref().unwrap_or("(unnamed)"),
                    agent.pane_id,
                    agent.agent_status.as_deref().unwrap_or("unknown"),
                    agent.workspace_id.as_deref().unwrap_or("-"),
                )
            })
            .collect();
        ToolResult::ok(
            "list_workers",
            format!("找到 {} 个 worker:\n{}", agents.len(), lines.join("\n")),
        )
    }
}

// ── close_worker ───────────────────────────────────────────────────────

/// Terminate and close a worker pane.
pub struct CloseWorker;

impl Tool for CloseWorker {
    fn name(&self) -> &str {
        "close_worker"
    }

    fn description(&self) -> &str {
        "关闭指定 worker 的 pane，终止其运行。已完成的 worker 建议及时清理以释放资源。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "要关闭的 worker pane ID"
                }
            },
            "required": ["pane_id"]
        })
    }

    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|value| value.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::error("close_worker", "参数 pane_id 是必需的非空字符串"),
        };

        match executor.endpoint().close_pane(pane_id) {
            Ok(()) => ToolResult::ok("close_worker", format!("Worker pane {pane_id} 已关闭")),
            Err(error) => report("close_worker", "关闭 pane 失败", error),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool Registry
// ═══════════════════════════════════════════════════════════════════════════

/// Return all built-in tools registered with the orchestrator.
pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(SpawnWorker),
        Box::new(SendText),
        Box::new(ReadOutput),
        Box::new(WaitForOutput),
        Box::new(ListWorkers),
        Box::new(CloseWorker),
    ]
}

/// Find a tool by name.
#[allow(dead_code)]
pub fn find_tool(name: &str) -> Option<Box<dyn Tool>> {
    builtin_tools().into_iter().find(|tool| tool.name() == name)
}

/// Execute a tool call with the given executor.
#[allow(dead_code)]
pub fn execute_tool_call(call: &ToolCall, executor: &ToolExecutor) -> ToolResult {
    match find_tool(&call.name) {
        Some(tool) => tool.execute(executor, call.arguments.clone()),
        None => ToolResult::error("tool_dispatch", format!("未知工具: {}", call.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executor() -> ToolExecutor {
        ToolExecutor::new(
            ApiEndpoint::at("/tmp/zenix-tools-test.sock".into()),
            String::new(),
        )
    }

    #[test]
    fn the_six_tool_names_are_stable() {
        let names: Vec<String> = builtin_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "spawn_worker",
                "send_text",
                "read_output",
                "wait_for_output",
                "list_workers",
                "close_worker"
            ]
        );
    }

    #[test]
    fn every_tool_declares_an_object_schema() {
        for tool in builtin_tools() {
            let schema = tool.parameters();
            assert_eq!(
                schema["type"],
                "object",
                "{} declares an object",
                tool.name()
            );
            assert!(schema["properties"].is_object(), "{}", tool.name());
        }
    }

    #[test]
    fn an_unreachable_daemon_becomes_an_error_result_not_a_panic() {
        let result = executor().execute(&ToolCall {
            id: "1".into(),
            name: "list_workers".into(),
            arguments: serde_json::json!({}),
        });
        assert!(result.is_error);
        assert!(result.content.contains("获取 worker 列表失败"));
    }

    #[test]
    fn an_unknown_tool_is_reported_by_name() {
        let result = executor().execute(&ToolCall {
            id: "1".into(),
            name: "nope".into(),
            arguments: serde_json::json!({}),
        });
        assert!(result.is_error);
        assert!(result.content.contains("nope"));
    }

    #[test]
    fn missing_required_arguments_are_reported_before_any_call() {
        for (tool, args) in [
            ("spawn_worker", serde_json::json!({})),
            ("send_text", serde_json::json!({"pane_id": "w1:p1"})),
            ("read_output", serde_json::json!({})),
            ("wait_for_output", serde_json::json!({"pane_id": "w1:p1"})),
            ("close_worker", serde_json::json!({})),
        ] {
            let result = executor().execute(&ToolCall {
                id: "1".into(),
                name: tool.into(),
                arguments: args,
            });
            assert!(result.is_error, "{tool} rejects its missing argument");
        }
    }

    #[test]
    fn a_worker_name_becomes_a_legal_agent_name() {
        let name = agent_name(Some("OC CheckGit"), "claude");
        assert!(
            name.starts_with("oc-checkgit-"),
            "cleaned and letter-initial: {name}"
        );
        assert!(name.len() <= 32);
        assert!(
            name.chars().all(|ch| ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || matches!(ch, '-' | '_'))
        );
    }

    #[test]
    fn a_name_that_cannot_start_an_agent_name_gains_a_letter_prefix() {
        let name = agent_name(Some("9lives"), "codex");
        assert!(name.starts_with("w9lives-"), "{name}");
    }

    #[test]
    fn two_workers_never_share_a_name() {
        let first = agent_name(Some("check"), "claude");
        let second = agent_name(Some("check"), "claude");
        assert_ne!(first, second);
    }

    #[test]
    fn a_long_requested_name_is_cut_to_the_daemons_limit() {
        let long = "a".repeat(80);
        let name = agent_name(Some(&long), "claude");
        assert!(name.len() <= 32, "{name}");
    }

    #[test]
    fn a_split_aims_at_the_requested_grid_cell() {
        assert_eq!(
            SpawnWorker::direction(Some(&serde_json::json!({"row": 0, "col": 1}))),
            SplitDirection::Right
        );
        assert_eq!(
            SpawnWorker::direction(Some(&serde_json::json!({"row": 1, "col": 0}))),
            SplitDirection::Down
        );
        assert_eq!(SpawnWorker::direction(None), SplitDirection::Down);
    }

    #[test]
    fn a_long_read_is_cut_on_a_character_boundary() {
        let text = "汉".repeat(MAX_READ_BYTES);
        let clipped = clipped(text);
        assert!(clipped.starts_with('汉'));
        assert!(clipped.contains("已截断"));
    }

    #[test]
    fn a_short_read_is_returned_untouched() {
        assert_eq!(clipped("hello".into()), "hello");
    }
}
