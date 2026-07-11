use std::sync::Arc;

use rmcp::{
    handler::server::ServerHandler,
    model::*,
};

use serde_json::Value;

use crate::ai::session::SessionManager;
use crate::ai::tools::{ToolCall, ToolExecutor};
use crate::ai::workers::WorkerRegistry;

use super::types::WorkerMessage;

#[derive(Clone)]
pub struct McpToolHandler {
    executor: Arc<ToolExecutor>,
    worker_registry: Arc<WorkerRegistry>,
    session_manager: Arc<SessionManager>,
    workspace_id: String,
}

impl McpToolHandler {
    pub fn new(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
    ) -> Self {
        Self { executor, worker_registry, session_manager, workspace_id }
    }

    fn exec_orch(&self, name: &str, args: Value) -> CallToolResult {
        let call = ToolCall { id: String::new(), name: name.to_string(), arguments: args };
        let result = self.executor.execute(&call);
        let text = if result.is_error { format!("ERROR: {}", result.content) } else { result.content };
        Self::ok(text)
    }

    fn ok(text: String) -> CallToolResult {
        serde_json::from_value(serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": false
        })).unwrap_or_else(|_| Self::err("failed to serialize ok response".into()))
    }

    fn err(text: String) -> CallToolResult {
        serde_json::from_value(serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": true
        })).unwrap_or_else(|_| {
            serde_json::from_value(serde_json::json!({
                "content": [{"type": "text", "text": "internal error"}],
                "isError": true
            })).unwrap()
        })
    }

    fn tool_def(name: &str, desc: &str, schema: Value) -> Tool {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "description": desc,
            "inputSchema": schema
        })).unwrap()
    }

    pub fn build_tools() -> Vec<Tool> {
        vec![
            Self::tool_def("spawn_worker", "Create a worker pane and start an Agent CLI.", serde_json::json!({
                "type": "object", "properties": {
                    "agent": {"type": "string"},
                    "instruction": {"type": "string"},
                    "cwd": {"type": "string"},
                    "position": {"type": "object", "properties": {
                        "row": {"type": "integer"},
                        "col": {"type": "integer"}
                    }}
                }, "required": ["agent", "instruction"]
            })),
            Self::tool_def("send_text", "Send text to a worker pane.", serde_json::json!({
                "type": "object", "properties": {
                    "pane_id": {"type": "string"},
                    "text": {"type": "string"}
                }, "required": ["pane_id", "text"]
            })),
            Self::tool_def("read_output", "Read recent output from a worker pane.", serde_json::json!({
                "type": "object", "properties": {
                    "pane_id": {"type": "string"},
                    "lines": {"type": "integer"}
                }, "required": ["pane_id"]
            })),
            Self::tool_def("wait_for_output", "Wait for worker output to contain a pattern.", serde_json::json!({
                "type": "object", "properties": {
                    "pane_id": {"type": "string"},
                    "pattern": {"type": "string"},
                    "timeout_ms": {"type": "integer"}
                }, "required": ["pane_id", "pattern"]
            })),
            Self::tool_def("list_workers", "List all running workers.", serde_json::json!({
                "type": "object", "properties": {}, "required": []
            })),
            Self::tool_def("close_worker", "Close and terminate a worker pane.", serde_json::json!({
                "type": "object", "properties": {
                    "pane_id": {"type": "string"}
                }, "required": ["pane_id"]
            })),
            Self::tool_def("worker_list", "List all known workers with status.", serde_json::json!({
                "type": "object", "properties": {}, "required": []
            })),
            Self::tool_def("worker_send", "Send a message to another worker.", serde_json::json!({
                "type": "object", "properties": {
                    "worker_name": {"type": "string"},
                    "text": {"type": "string"}
                }, "required": ["worker_name", "text"]
            })),
            Self::tool_def("worker_inbox", "Read unread messages for a worker.", serde_json::json!({
                "type": "object", "properties": {
                    "worker_name": {"type": "string"}
                }, "required": ["worker_name"]
            })),
            Self::tool_def("worker_broadcast", "Send a message to all active workers.", serde_json::json!({
                "type": "object", "properties": {
                    "text": {"type": "string"}
                }, "required": ["text"]
            })),
        ]
    }
}

// Worker comm helpers
fn worker_list_text(h: &McpToolHandler) -> CallToolResult {
    let w = h.worker_registry.all_workers();
    if w.is_empty() { return McpToolHandler::ok("No active workers.".into()); }
    let l: Vec<String> = w.iter().map(|w|
        format!("- {} | id={} | pane={} | status={} | role={}", w.worker_name, w.id, w.pane_id, w.status, w.role_id)
    ).collect();
    McpToolHandler::ok(format!("Active workers ({}):\n{}", l.len(), l.join("\n")))
}

fn worker_send(h: &McpToolHandler, wn: &str, text: &str) -> CallToolResult {
    if wn.is_empty() || text.is_empty() { return McpToolHandler::err("worker_name and text are required.".into()); }
    if !h.worker_registry.all_workers().iter().any(|w| w.worker_name == wn) {
        return McpToolHandler::err(format!("worker '{wn}' not found"));
    }
    let mid = uuid::Uuid::new_v4().to_string();
    let key = WorkerMessage::inbox_key(wn, &mid);
    let now = crate::app::timestamp_now();
    let val = serde_json::json!({"from":"orchestrator","to":wn,"text":text,"timestamp":now});
    match h.session_manager.write_memory(&key, &val.to_string(), &h.workspace_id) {
        Ok(_) => McpToolHandler::ok(format!("Message sent to worker '{wn}'. id={mid}")),
        Err(e) => McpToolHandler::err(format!("store failed: {e}")),
    }
}

fn worker_inbox(h: &McpToolHandler, wn: &str) -> CallToolResult {
    if wn.is_empty() { return McpToolHandler::err("worker_name is required.".into()); }
    let prefix = format!("worker_msg:{}:", wn);
    let results = match h.session_manager.search_memory(&prefix, 50) {
        Ok(r) => r, Err(e) => return McpToolHandler::err(format!("search failed: {e}")),
    };
    if results.is_empty() { return McpToolHandler::ok(format!("No messages for worker '{wn}'.")); }
    let mut msgs: Vec<String> = Vec::new();
    for (key, value) in &results {
        if let Ok(p) = serde_json::from_str::<Value>(value) {
            msgs.push(format!("[{}] {}: {}",
                p.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?"),
                p.get("from").and_then(|v| v.as_str()).unwrap_or("?"),
                p.get("text").and_then(|v| v.as_str()).unwrap_or(""),
            ));
        }
        let _ = h.session_manager.delete_memory(key);
    }
    McpToolHandler::ok(format!("Inbox for '{}' ({} msgs):\n{}", wn, msgs.len(), msgs.join("\n---\n")))
}

fn worker_broadcast(h: &McpToolHandler, text: &str) -> CallToolResult {
    if text.is_empty() { return McpToolHandler::err("text is required.".into()); }
    let workers = h.worker_registry.active_workers();
    let now = crate::app::timestamp_now();
    let mut sent = 0u32;
    for w in &workers {
        let mid = uuid::Uuid::new_v4().to_string();
        let key = WorkerMessage::inbox_key(&w.worker_name, &mid);
        let val = serde_json::json!({"from":"broadcast","to":w.worker_name,"text":text,"timestamp":now});
        if h.session_manager.write_memory(&key, &val.to_string(), &h.workspace_id).is_ok() { sent += 1; }
    }
    McpToolHandler::ok(format!("Broadcast sent to {sent} active worker(s)."))
}

impl McpToolHandler {
    /// Synchronous tool dispatch for SSE handler (no RequestContext needed).
    pub fn handle_tool_call(&self, name: &str, args: serde_json::Map<String, Value>) -> CallToolResult {
        match name {
            "spawn_worker" | "send_text" | "read_output"
            | "wait_for_output" | "list_workers" | "close_worker" => {
                self.exec_orch(name, Value::Object(args))
            }
            "worker_list" => worker_list_text(self),
            "worker_send" => {
                let wn = args.get("worker_name").and_then(|v| v.as_str()).unwrap_or("");
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                worker_send(self, wn, text)
            }
            "worker_inbox" => {
                let wn = args.get("worker_name").and_then(|v| v.as_str()).unwrap_or("");
                worker_inbox(self, wn)
            }
            "worker_broadcast" => {
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                worker_broadcast(self, text)
            }
            _ => Self::err(format!("Unknown tool: {name}")),
        }
    }
}

// ── ServerHandler Implementation ──────────────────────────────

impl ServerHandler for McpToolHandler {
    fn get_info(&self) -> ServerInfo {
        serde_json::from_value(serde_json::json!({
            "protocolVersion": "2025-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "zenix-orchestrator",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Zenix orchestration MCP server."
        })).expect("valid server info")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(serde_json::from_value(serde_json::json!({
            "tools": Self::build_tools()
        })).unwrap())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self.handle_tool_call(&request.name, request.arguments.unwrap_or_default()))
    }
}
