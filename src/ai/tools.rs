//! Built-in tool system for the ZenixAI orchestrator.
//!
//! Tools are callable functions that the LLM can invoke via function calling.
//! Each tool implements the `Tool` trait with a name, description, JSON Schema
//! parameters, and an execute method.
//!
//! The ToolExecutor holds shared state (herdr IPC) and is passed
//! to every tool's execute method.
//!
//! Built-in tools (6):
//!   - `spawn_worker`:   Create a worker agent in a grid-positioned pane
//!   - `send_text`:      Send text/instruction to a worker
//!   - `read_output`:    Read a worker's recent output
//!   - `wait_for_output`: Wait for a completion pattern from a worker
//!   - `list_workers`:   List running workers
//!   - `close_worker`:   Terminate and remove a worker pane

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::ai::herdr::{HerdrIpcClient, SplitDirection};

// ── Core Types ─────────────────────────────────────────────────────────

/// A tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
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
/// Holds references to the herdr IPC client and the current workspace ID.
pub struct ToolExecutor {
    herdr_client: Arc<Mutex<HerdrIpcClient>>,
    workspace_id: String,
}

impl ToolExecutor {
    /// Create a new tool executor.
    pub fn new(herdr_client: HerdrIpcClient, workspace_id: String) -> Self {
        Self {
            herdr_client: Arc::new(Mutex::new(herdr_client)),
            workspace_id,
        }
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

// ═══════════════════════════════════════════════════════════════════════════
// Tool Implementations
// ═══════════════════════════════════════════════════════════════════════════

// ── spawn_worker ───────────────────────────────────────────────────────

/// Create a worker agent in a new grid-positioned pane.
pub struct SpawnWorker;

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
        let agent = match args.get("agent").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("spawn_worker", "参数 agent 是必需的非空字符串");
            }
        };
        let instruction = match args.get("instruction").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("spawn_worker", "参数 instruction 是必需的非空字符串");
            }
        };
        let cwd = args.get("cwd").and_then(|v| v.as_str());
        let position = args.get("position");

        let mut client = executor.herdr_client.lock().unwrap();
        let workspace_id = &executor.workspace_id;

        // Handle grid layout if position is specified
        if let Some(pos) = position {
            let row = pos.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let col = pos.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let target_cols = col + 1;
            let target_rows = row + 1;
            let target_count = target_rows * target_cols;

            // List current panes
            let current_count = match client.list_panes(Some(workspace_id)) {
                Ok(panes) => panes.len(),
                Err(_) => 0,
            };

            // Create new panes if needed to reach the grid size
            if current_count < target_count {
                let to_create = target_count - current_count;
                let mut dir = SplitDirection::Right;
                for _ in 0..to_create {
                    if let Err(e) = client.split_pane(Some(workspace_id), dir, None) {
                        return ToolResult::error("spawn_worker", format!("创建网格窗格失败: {e}"));
                    }
                    dir = if matches!(dir, SplitDirection::Right) {
                        SplitDirection::Down
                    } else {
                        SplitDirection::Right
                    };
                }
            }

            // After creating, list panes again to get the target pane
            let panes = match client.list_panes(Some(workspace_id)) {
                Ok(p) => p,
                Err(e) => {
                    return ToolResult::error("spawn_worker", format!("获取窗格列表失败: {e}"));
                }
            };

            let target_idx = row * target_cols + col;
            let _pane = panes.into_iter().nth(target_idx);

            // Start the agent in the target pane
            let ws = Some(workspace_id.as_str());
            match client.start_agent(agent, ws, cwd, vec![]) {
                Ok(agent_info) => {
                    // Send the instruction text after agent starts
                    let term_id = &agent_info.terminal_id;
                    let _ = client.send_agent_text(term_id, instruction);

                    ToolResult::ok(
                        "spawn_worker",
                        format!(
                            "Worker '{}' 已启动: pane_id={}, terminal_id={}, 位置=({},{})",
                            agent, agent_info.pane_id, agent_info.terminal_id, row, col
                        ),
                    )
                }
                Err(e) => {
                    ToolResult::error("spawn_worker", format!("启动 Agent '{}' 失败: {e}", agent))
                }
            }
        } else {
            // No position — simple spawn
            let ws = Some(workspace_id.as_str());
            match client.start_agent(agent, ws, cwd, vec![]) {
                Ok(agent_info) => {
                    let term_id = &agent_info.terminal_id;
                    let _ = client.send_agent_text(term_id, instruction);

                    ToolResult::ok(
                        "spawn_worker",
                        format!(
                            "Worker '{}' 已启动: pane_id={}, terminal_id={}",
                            agent, agent_info.pane_id, agent_info.terminal_id
                        ),
                    )
                }
                Err(e) => {
                    ToolResult::error("spawn_worker", format!("启动 Agent '{}' 失败: {e}", agent))
                }
            }
        }
    }
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
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("send_text", "参数 pane_id 是必需的非空字符串");
            }
        };
        let text = match args.get("text").and_then(|v| v.as_str()) {
            Some(s) => s,
            _ => {
                return ToolResult::error("send_text", "参数 text 是必需的字符串");
            }
        };

        let mut client = executor.herdr_client.lock().unwrap();
        match client.send_pane_text(pane_id, text) {
            Ok(()) => ToolResult::ok("send_text", format!("文本已发送到 pane {pane_id}")),
            Err(e) => ToolResult::error("send_text", format!("发送文本失败: {e}")),
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
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("read_output", "参数 pane_id 是必需的非空字符串");
            }
        };
        let lines: u32 = args
            .get("lines")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(100);

        let mut client = executor.herdr_client.lock().unwrap();
        match client.read_pane(pane_id, crate::ai::herdr::ReadSource::Recent, lines) {
            Ok(text) => {
                if text.len() > 8000 {
                    ToolResult::ok(
                        "read_output",
                        format!("{}...[已截断，共 {} 字符]", &text[..8000], text.len()),
                    )
                } else {
                    ToolResult::ok("read_output", text)
                }
            }
            Err(e) => ToolResult::error("read_output", format!("读取 pane {pane_id} 失败: {e}")),
        }
    }
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
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("wait_for_output", "参数 pane_id 是必需的非空字符串");
            }
        };
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("wait_for_output", "参数 pattern 是必需的非空字符串");
            }
        };
        let timeout_ms: u64 = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(300_000);

        let mut client = executor.herdr_client.lock().unwrap();
        match client.wait_for_output(pane_id, pattern, timeout_ms) {
            Ok(matched) => ToolResult::ok("wait_for_output", matched),
            Err(e) => ToolResult::error("wait_for_output", format!("等待超时或失败: {e}")),
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
        let mut client = executor.herdr_client.lock().unwrap();
        match client.list_agents() {
            Ok(agents) => {
                if agents.is_empty() {
                    return ToolResult::ok("list_workers", "当前没有运行中的 worker。");
                }
                let mut lines = Vec::new();
                for (i, agent) in agents.iter().enumerate() {
                    let name = agent.name.as_deref().unwrap_or("(unnamed)");
                    let status = agent.agent_status.as_str();
                    lines.push(format!(
                        "{}. {} | pane={} | status={} | workspace={}",
                        i + 1,
                        name,
                        agent.pane_id,
                        status,
                        agent.workspace_id
                    ));
                }
                ToolResult::ok(
                    "list_workers",
                    format!("找到 {} 个 worker:\n{}", agents.len(), lines.join("\n")),
                )
            }
            Err(e) => ToolResult::error("list_workers", format!("获取 worker 列表失败: {e}")),
        }
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
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("close_worker", "参数 pane_id 是必需的非空字符串");
            }
        };

        let mut client = executor.herdr_client.lock().unwrap();
        match client.close_pane(pane_id) {
            Ok(()) => ToolResult::ok("close_worker", format!("Worker pane {pane_id} 已关闭")),
            Err(e) => ToolResult::error("close_worker", format!("关闭 pane 失败: {e}")),
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
    builtin_tools().into_iter().find(|t| t.name() == name)
}

/// Execute a tool call with the given executor.
#[allow(dead_code)]
pub fn execute_tool_call(call: &ToolCall, executor: &ToolExecutor) -> ToolResult {
    match find_tool(&call.name) {
        Some(tool) => tool.execute(executor, call.arguments.clone()),
        None => ToolResult::error("tool_dispatch", format!("未知工具: {}", call.name)),
    }
}
