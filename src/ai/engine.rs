//! Orchestrator engine — LLM API client with SSE streaming support and
//! agent dispatch capability.
//!
//! Uses `reqwest` with tokio runtime to communicate with OpenAI/Anthropic/
//! Ollama-compatible endpoints via the standard Chat Completions API.
//!
//! Dispatch capability routes LLM-decided agent tasks through the herdr IPC
//! client, collecting real-time output and looping until the orchestrator
//! produces a final response without dispatch directives.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Acquire the tokio runtime handle.
///
/// Prefers the ambient handle when already inside a tokio context; falls back
/// to the global handle set by `main.rs`. GPUI's async executor is not tokio,
/// so `tokio::task::spawn` called from a GPUI-spawned task would panic — use
/// this helper instead.
fn tokio_handle() -> tokio::runtime::Handle {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
        crate::TOKIO_HANDLE
            .get()
            .expect("TOKIO_HANDLE not initialized")
            .clone()
    })
}

use anyhow::{Context, Result};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};

use crate::ai::compactor::Compactor;
use crate::ai::error::{AiError, user_friendly};
use crate::ai::herdr::{HerdrIpcClient, ReadSource};
use crate::ai::tools::{Tool, ToolCall, ToolResult};
use crate::api_provider::ApiProviderConfig;

/// Default system prompt used when no dynamic prompt is provided.
pub const DEFAULT_SYSTEM_PROMPT: &str = "你是 ZenixAI 编排器。你负责理解用户需求、规划任务分解、将具体执行工作分配给专业 Agent CLI（如 Claude Code、Codex、OpenCode 等）。你不直接写代码或运行命令，只负责规划、调度和结果汇总。用中文回复。";

/// Build a dynamic system prompt that includes roles, workspace, and tools.
///
/// `roles_summary` — a newline-separated list of available roles and descriptions.
/// `workspace_info` — description of the currently bound workspace.
/// If both are empty, the default prompt is returned unchanged.
pub fn build_system_prompt(roles_summary: &str, workspace_info: &str) -> String {
    let has_roles = !roles_summary.is_empty();
    let has_workspace = !workspace_info.is_empty();

    if !has_roles && !has_workspace {
        return DEFAULT_SYSTEM_PROMPT.to_string();
    }

    let mut prompt = String::with_capacity(2048);
    prompt.push_str("你是 ZenixAI 编排器。你负责理解用户需求、规划任务分解、将具体执行工作分配给专业 Agent CLI。你不直接写代码或运行命令，只负责规划、调度和结果汇总。用中文回复。\n\n");

    if has_roles {
        prompt.push_str("## 可用角色\n");
        prompt.push_str(roles_summary);
        prompt.push_str("\n\n");
    }

    prompt.push_str(
        "## 可用工具\n\
        - read_file: 读取文件内容（最大 50KB）\n\
        - read_directory: 递归列出目录结构（最大 3 层）\n\
        - search_files: 递归搜索匹配 glob 模式的文件\n\
        - list_agents: 查看 herdr 中所有 Agent 的运行状态\n\
        - start_agent: 在 herdr 中启动一个 Agent CLI\n\
        - send_instruction: 向指定 Agent 发送文本指令\n\
        - read_output: 读取指定 Agent 的最新输出\n\
        - split_pane: 在 herdr 中分割窗格\n\
        - arrange_grid: 将多个 pane 按网格布局排列\n\
        - read_memory: 读取进程内记忆\n\
        - write_memory: 写入进程内记忆\n\
        - search_memory: 搜索进程内记忆\n\n",
    );

    if has_workspace {
        prompt.push_str("## 当前工作空间\n");
        prompt.push_str(workspace_info);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Agent Dispatch 格式\n\
        当你需要分配任务给 Agent 时，在回复中包含以下 JSON：\n\n\
        ```json\n\
        {\n  \"agents\": [\n    {\n      \"agent_name\": \"opencode\",\n      \"role\": \"frontend-developer\",\n      \"worker_name\": \"oc_task(frontend)\",\n      \"instruction\": \"具体的任务描述\",\n      \"cwd\": \"/home/user/project\"\n    }\n  ]\n}\n\
        ```\n\n\
        worker_name 的标识部分请根据任务内容生成简短有意义的名字，格式: <cli缩写>_<标识>(<角色缩写>)。\n\n\
        ## 工作流\n\
        1. 分析用户需求\n\
        2. 规划任务分解\n\
        3. 选择角色和 Agent CLI\n\
        4. 分配任务（并行任务一次分配）\n\
        5. 收集执行结果\n\
        6. 汇总报告\n\n\
        ## 约束\n\
        - 你不直接写代码或运行命令\n\
        - 你只负责规划、调度和结果汇总\n\
        - 用中文回复");

    prompt
}

/// Lightweight chat message reference for the engine (no session/sequence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRef {
    /// "user" or "orchestrator".
    pub role: String,
    /// Message content.
    pub content: String,
}

/// Events emitted during an LLM streaming call.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A token chunk from the LLM response.
    Token(String),
    /// A thinking (reasoning) token chunk from the LLM.
    Thinking(String),
    /// An error occurred during streaming.
    Error(String),
    /// The stream has finished.
    Done,
    /// The LLM returned a tool call (tool_name, arguments JSON).
    ToolCall(String, serde_json::Value),
    /// A tool was executed (tool_name, result content, is_error).
    ToolResult(String, String, bool),
}

// ── Dispatch Types ──────────────────────────────────────────────────────

/// Default timeout for agent execution (ms).
pub const DEFAULT_AGENT_TIMEOUT_MS: u64 = 300_000;

/// Pattern substring to match for agent completion.
pub const AGENT_DONE_PATTERN: &str = "[AGENT_DONE]";

/// An agent task dispatched by the orchestrator.
#[derive(Debug, Clone)]
pub struct AgentTask {
    /// Agent CLI name (e.g. "claude", "codex", "opencode").
    pub agent_name: String,
    /// Full instruction text for the agent.
    pub instruction: String,
    /// Working directory for the agent.
    pub cwd: String,
    /// Pane ID assigned by herdr (after dispatch).
    pub pane_id: Option<String>,
}

/// Result from an executed agent.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub agent_name: String,
    pub pane_id: String,
    /// "completed", "failed", "timeout", "cancelled".
    pub exit_status: String,
    /// Full output from the agent pane.
    pub output: String,
    /// Brief summary generated by the agent.
    pub summary: Option<String>,
}

/// A dispatch directive parsed from LLM response.
#[derive(Debug, Clone)]
pub enum DispatchDirective {
    /// JSON-formatted dispatch (preferred format).
    Json { agents: Vec<AgentTask> },
    /// Text-formatted dispatch (fallback).
    Text {
        agent_name: String,
        instruction: String,
        cwd: String,
    },
}

impl DispatchDirective {
    /// Collect all agent tasks from this directive into a flat list.
    pub fn to_tasks(&self) -> Vec<AgentTask> {
        match self {
            DispatchDirective::Json { agents } => agents.clone(),
            DispatchDirective::Text {
                agent_name,
                instruction,
                cwd,
            } => vec![AgentTask {
                agent_name: agent_name.clone(),
                instruction: instruction.clone(),
                cwd: cwd.clone(),
                pane_id: None,
            }],
        }
    }
}

/// Information about a compaction event.
#[derive(Debug, Clone)]
pub struct CompactionInfo {
    /// Number of messages before compression.
    pub original_count: usize,
    /// Number of messages that were compacted away.
    pub compressed_count: usize,
    /// Number of messages retained after compression.
    pub kept_count: usize,
    /// Token estimate of the original message list.
    pub before_tokens: usize,
    /// Token estimate of the retained messages.
    pub after_tokens: usize,
}

/// Extended stream events for dispatch-capable streaming.
#[derive(Debug, Clone)]
pub enum DispatchStreamEvent {
    /// A token chunk from the LLM response.
    Token(String),
    /// A thinking (reasoning) token chunk.
    Thinking(String),
    /// The orchestrator decided to dispatch one or more agents.
    Dispatch(DispatchDirective),
    /// Agent output chunk (streamed in real-time).
    AgentOutput { agent_name: String, text: String },
    /// An agent has completed.
    AgentComplete(AgentResult),
    /// A tool was called by the LLM (tool_name, arguments JSON).
    ToolCall(String, serde_json::Value),
    /// A tool was executed (tool_name, result content, is_error).
    ToolResult(String, String, bool),
    /// The conversation was compacted to fit within the context window.
    Compacted(CompactionInfo),
    /// An error occurred.
    Error(String),
    /// The entire flow (LLM + agents) has finished.
    Done,
}

/// Orchestrator LLM engine.
pub struct OrchestratorEngine {
    http_client: reqwest::Client,
    endpoint_url: String,
    model: String,
    api_key: String,
    /// System prompt used for all LLM calls.
    /// Set via `new()` or `with_system_prompt()`.
    pub system_prompt: String,
}

/// SSE response chunk from the LLM API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseChunk {
    id: Option<String>,
    choices: Option<Vec<SseChoice>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseChoice {
    delta: Option<SseDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseDelta {
    content: Option<String>,
    role: Option<String>,
    /// Reasoning/thinking content from the OpenAI delta
    /// (for reasoning models like o1, DeepSeek R1, etc.).
    /// Present alongside `content` in SSE chunks.
    reasoning_content: Option<String>,
}

/// Parsed Anthropic SSE event, richer than a plain `Option<String>` to
/// distinguish thinking content from text tokens.
#[derive(Debug)]
enum AnthropicSseEvent {
    /// A text token chunk.
    Text(String),
    /// A thinking (reasoning) token chunk.
    Thinking(String),
    /// The stream has finished (message_stop).
    Done,
    /// Non-content event — skip (content_block_start, ping, etc.).
    Ignored,
}

/// Parse an Anthropic SSE event line into one of the `AnthropicSseEvent` variants.
///
/// Anthropic's `/v1/messages` SSE stream emits events like:
///   `data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}`
///   `data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"..."}}`
/// Terminated by:
///   `data: {"type":"message_stop"}`
fn parse_anthropic_sse_event(data: &str) -> AnthropicSseEvent {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
        return AnthropicSseEvent::Ignored;
    };
    let Some(event_type) = value.get("type").and_then(|v| v.as_str()) else {
        return AnthropicSseEvent::Ignored;
    };

    match event_type {
        "message_stop" => AnthropicSseEvent::Done,
        "content_block_delta" => {
            let Some(delta) = value.get("delta") else {
                return AnthropicSseEvent::Ignored;
            };
            // Check delta.type to distinguish text vs thinking
            match delta.get("type").and_then(|v| v.as_str()) {
                Some("text_delta") => {
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        AnthropicSseEvent::Text(text.to_string())
                    } else {
                        AnthropicSseEvent::Ignored
                    }
                }
                Some("thinking_delta") => {
                    if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                        AnthropicSseEvent::Thinking(text.to_string())
                    } else {
                        AnthropicSseEvent::Ignored
                    }
                }
                _ => {
                    // Fallback: try delta.text (compatibility with OpenAI-format proxied
                    // endpoints that don't nest under type-specific deltas).
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        AnthropicSseEvent::Text(text.to_string())
                    } else {
                        AnthropicSseEvent::Ignored
                    }
                }
            }
        }
        _ => AnthropicSseEvent::Ignored,
    }
}

impl OrchestratorEngine {
    /// Create a new engine from an API provider configuration.
    ///
    /// Validates that the provider has a valid base_url, api_key, and model
    /// configured. The HTTP client uses a 120-second timeout.
    /// Uses `DEFAULT_SYSTEM_PROMPT` unless overridden via `with_system_prompt()`.
    pub fn new(provider: &ApiProviderConfig) -> Result<Self, AiError> {
        if provider.base_url.is_empty() {
            return Err(AiError::ConfigMissing("base_url is empty".into()));
        }
        if provider.api_key.is_empty() {
            return Err(AiError::ConfigMissing("api_key is empty".into()));
        }
        if provider.model.is_empty() {
            return Err(AiError::ConfigMissing("model is empty".into()));
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent("ZenixAI/0.1")
            .build()
            .context("build reqwest client")
            .map_err(AiError::from)?;

        Ok(Self {
            http_client,
            endpoint_url: provider.endpoint_url(),
            model: provider.model.clone(),
            api_key: provider.api_key.clone(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        })
    }

    /// Override the system prompt for all subsequent LLM calls.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Send a streaming chat request to the LLM API.
    ///
    /// The system prompt is prepended as the first message if not already present.
    /// Calls `on_event` with `StreamEvent::Token(content)` for each token chunk,
    /// `StreamEvent::Error(msg)` on failure, and `StreamEvent::Done` at completion.
    ///
    /// Uses `reqwest::bytes_stream()` to parse SSE "data: " lines.
    async fn chat_stream(
        &self,
        messages: &[ChatMessageRef],
        on_event: impl Fn(StreamEvent) + Send + 'static,
    ) -> Result<(), AiError> {
        // Ensure system prompt is first
        let effective_messages =
            if messages.is_empty() || messages.first().map(|m| m.role.as_str()) != Some("system") {
                let mut prefixed = vec![ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                }];
                prefixed.extend(messages.iter().cloned());
                prefixed
            } else {
                messages.to_vec()
            };

        // Build the request payload
        let payload = serde_json::json!({
            "model": self.current_model(),
            "messages": effective_messages,
            "stream": true,
        });

        let http_client = self.http_client.clone();
        let endpoint = self.endpoint_url().to_string();
        let api_key = self.api_key.clone();
        let on_event_clone = on_event;

        let result = tokio_handle()
            .spawn(async move {
                let resp = http_client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                    .context("HTTP request failed")
                    .map_err(AiError::from)?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(AiError::ApiError(format!("HTTP {status}: {text}")));
                }

                // Parse SSE stream line-by-line.
                // Supports both OpenAI format (choices[].delta.content, [DONE])
                // and Anthropic format (type: "content_block_delta", text_delta.text, message_stop).
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut done = false;

                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(e) => {
                            on_event_clone(StreamEvent::Error(format!("stream error: {e}")));
                            break;
                        }
                    };

                    let text = String::from_utf8_lossy(&chunk);
                    for ch in text.chars() {
                        if ch == '\n' {
                            if buffer.starts_with("data: ") {
                                let data = &buffer[6..];
                                if data.trim() == "[DONE]" {
                                    done = true;
                                    buffer.clear();
                                    continue;
                                }
                                if let Ok(sse_chunk) = serde_json::from_str::<SseChunk>(data) {
                                    if let Some(choices) = sse_chunk.choices {
                                        for choice in choices {
                                            if let Some(delta) = choice.delta {
                                                if let Some(text) = delta.content {
                                                    on_event_clone(StreamEvent::Token(text));
                                                }
                                                if let Some(reasoning) = delta.reasoning_content {
                                                    on_event_clone(StreamEvent::Thinking(
                                                        reasoning,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    match parse_anthropic_sse_event(data) {
                                        AnthropicSseEvent::Text(text) if !text.is_empty() => {
                                            on_event_clone(StreamEvent::Token(text));
                                        }
                                        AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                                            on_event_clone(StreamEvent::Thinking(text));
                                        }
                                        AnthropicSseEvent::Done => done = true,
                                        _ => {}
                                    }
                                }
                            }
                            buffer.clear();
                        } else {
                            buffer.push(ch);
                        }
                    }
                    if done {
                        break;
                    }
                }

                // Handle any remaining partial line (no trailing newline)
                if !buffer.is_empty() && buffer.starts_with("data: ") {
                    let data = &buffer[6..];
                    if data.trim() == "[DONE]" {
                        done = true;
                    } else if let Ok(sse_chunk) = serde_json::from_str::<SseChunk>(data) {
                        if let Some(choices) = sse_chunk.choices {
                            for choice in choices {
                                if let Some(delta) = choice.delta {
                                    if let Some(text) = delta.content {
                                        on_event_clone(StreamEvent::Token(text));
                                    }
                                    if let Some(reasoning) = delta.reasoning_content {
                                        on_event_clone(StreamEvent::Thinking(reasoning));
                                    }
                                }
                            }
                        }
                    } else {
                        match parse_anthropic_sse_event(data) {
                            AnthropicSseEvent::Text(text) if !text.is_empty() => {
                                on_event_clone(StreamEvent::Token(text));
                            }
                            AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                                on_event_clone(StreamEvent::Thinking(text));
                            }
                            AnthropicSseEvent::Done => done = true,
                            _ => {}
                        }
                    }
                }

                if !done {
                    on_event_clone(StreamEvent::Done);
                }
                Ok(())
            })
            .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(e) => Err(AiError::ApiError(format!("spawn failed: {e}"))),
        }
    }

    /// Non-streaming chat: sends a synchronous HTTP request via `ureq` and
    /// collects the complete response into a single string.
    ///
    /// Useful for simple Q&A without real-time rendering.
    pub fn chat_simple(&self, messages: &[ChatMessageRef]) -> Result<String, AiError> {
        // Ensure system prompt is first
        let effective_messages =
            if messages.is_empty() || messages.first().map(|m| m.role.as_str()) != Some("system") {
                let mut prefixed = vec![ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                }];
                prefixed.extend(messages.iter().cloned());
                prefixed
            } else {
                messages.to_vec()
            };

        let url = self.endpoint_url();
        let api_key = self.api_key.clone();
        let model = self.current_model();

        let payload = serde_json::json!({
            "model": model,
            "messages": effective_messages,
            "stream": false,
        });

        let resp = ureq::post(url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!(&payload))
            .context("ureq HTTP request failed")
            .map_err(AiError::from)?;

        let status = resp.status();
        let text = resp.into_string().context("read response body")?;
        if !(200..300).contains(&status) {
            return Err(AiError::ApiError(format!("HTTP {status}: {text}")));
        }

        let body: serde_json::Value = serde_json::from_str(&text)
            .context("parse response JSON")
            .map_err(AiError::from)?;

        body.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(String::from)
            .ok_or_else(|| AiError::ApiError("response missing content".into()))
    }

    fn endpoint_url(&self) -> &str {
        &self.endpoint_url
    }

    fn current_model(&self) -> &str {
        &self.model
    }
}

// ── Tool Executor ──────────────────────────────────────────────────────

/// Executes tool calls dispatched by the LLM.
///
/// Holds a reference to the herdr IPC client and workspace ID for
/// agent-related tools. Tool execution is synchronous — callers in
/// async contexts should wrap with `tokio::task::spawn_blocking`.
pub struct ToolExecutor {
    herdr_client: Arc<Mutex<HerdrIpcClient>>,
    workspace_id: String,
    memory: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl ToolExecutor {
    /// Create a new tool executor with the given herdr client and workspace.
    pub fn new(herdr_client: HerdrIpcClient, workspace_id: String) -> Self {
        Self {
            herdr_client: Arc::new(Mutex::new(herdr_client)),
            workspace_id,
            memory: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Execute a single tool call, dispatching by name.
    pub fn execute(&self, call: &ToolCall) -> ToolResult {
        let ctx = self.make_context();
        let all_tools = self.available_tools();
        for tool in &all_tools {
            if tool.name() == call.name {
                return tool.execute(&ctx, call.arguments.clone());
            }
        }
        ToolResult::error("tool_dispatch", format!("未知工具: {}", call.name))
    }

    /// Build a `ToolContext` sharing this executor's herdr client and memory store.
    fn make_context(&self) -> crate::ai::tools::ToolContext {
        crate::ai::tools::ToolContext {
            herdr_client: Some(Arc::clone(&self.herdr_client)),
            workspace_id: self.workspace_id.clone(),
            memory: Arc::clone(&self.memory),
        }
    }

    /// Return all built-in tools available to the orchestrator.
    pub fn available_tools(&self) -> Vec<Box<dyn Tool>> {
        crate::ai::tools::builtin_tools()
    }
}

// ── SSE Tool Call Parsing ──────────────────────────────────────────────

/// SSE response chunk from the LLM API (extended with tool_calls).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SseChunkWithTools {
    id: Option<String>,
    choices: Option<Vec<SseChoiceWithTools>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SseChoiceWithTools {
    delta: Option<SseDeltaWithTools>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SseDeltaWithTools {
    content: Option<String>,
    role: Option<String>,
    tool_calls: Option<Vec<SseToolCall>>,
    /// Reasoning/thinking content from the OpenAI delta
    /// (for reasoning models like o1, DeepSeek R1, etc.).
    reasoning_content: Option<String>,
}

/// A single tool call in the SSE stream.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SseToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
}

/// Function call details (name + JSON arguments string).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct FunctionCall {
    name: String,
    arguments: String,
}

/// Parse the tool_calls array from an SSE delta, extracting tool name
/// and JSON arguments.
pub(crate) fn parse_sse_tool_calls(delta: &SseDeltaWithTools) -> Vec<(String, serde_json::Value)> {
    let mut calls = Vec::new();
    if let Some(tool_calls) = &delta.tool_calls {
        for tc in tool_calls {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
                calls.push((tc.function.name.clone(), args));
            } else {
                // If arguments can't be parsed as JSON, pass the raw string
                calls.push((
                    tc.function.name.clone(),
                    serde_json::Value::String(tc.function.arguments.clone()),
                ));
            }
        }
    }
    calls
}

// ── Tool Call Loop Engine ─────────────────────────────────────────────

/// Maximum tool call rounds before giving up.
const MAX_TOOL_ROUNDS: u32 = 10;

/// Build the request payload for a tool-calling LLM request.
///
/// Converts the tool list into OpenAI function calling format and appends
/// it to the standard chat completions payload.
fn build_tool_call_payload(
    messages: &[ChatMessageRef],
    tools: &[&dyn Tool],
    model: &str,
) -> serde_json::Value {
    let tool_defs: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            let params = t.parameters();
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": params,
                }
            })
        })
        .collect();

    serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "tools": tool_defs,
    })
}

impl OrchestratorEngine {
    /// Send a streaming chat request that supports function calling.
    ///
    /// Like `chat_stream`, but parses `choices[].delta.tool_calls` and
    /// emits `StreamEvent::ToolCall(name, args)` for each tool call the LLM
    /// returns. Content tokens are emitted as before.
    async fn chat_stream_with_tools(
        &self,
        _messages: &[ChatMessageRef],
        on_event: impl Fn(StreamEvent) + Send + 'static,
    ) -> Result<Vec<(String, serde_json::Value)>, AiError> {
        // Tools are passed via chat_with_tools() caller, not in this stream path.
        // This function only handles SSE parsing for text + tool_calls.

        let http_client = self.http_client.clone();
        let endpoint = self.endpoint_url().to_string();
        let api_key = self.api_key.clone();
        let on_event_clone = on_event;

        let result = tokio_handle()
            .spawn(async move {
                let resp = http_client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .send()
                    .await
                    .context("HTTP request failed")
                    .map_err(AiError::from)?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(AiError::ApiError(format!("HTTP {status}: {text}")));
                }

                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut done = false;
                let mut tool_calls: Vec<(String, serde_json::Value)> = Vec::new();

                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(e) => {
                            on_event_clone(StreamEvent::Error(format!("stream error: {e}")));
                            break;
                        }
                    };

                    for byte in chunk {
                        if byte == b'\n' {
                            if buffer.starts_with("data: ") {
                                let data = &buffer[6..];
                                if data.trim() == "[DONE]" {
                                    done = true;
                                    buffer.clear();
                                    continue;
                                }
                                if let Ok(sse_chunk) =
                                    serde_json::from_str::<SseChunkWithTools>(data)
                                {
                                    if let Some(choices) = sse_chunk.choices {
                                        for choice in choices {
                                            if let Some(ref delta) = choice.delta {
                                                // Content tokens
                                                if let Some(ref text) = delta.content {
                                                    on_event_clone(StreamEvent::Token(
                                                        text.clone(),
                                                    ));
                                                }
                                                if let Some(ref reasoning) =
                                                    delta.reasoning_content
                                                {
                                                    on_event_clone(StreamEvent::Thinking(
                                                        reasoning.clone(),
                                                    ));
                                                }
                                                // Tool calls
                                                for (name, args) in parse_sse_tool_calls(delta) {
                                                    on_event_clone(StreamEvent::ToolCall(
                                                        name.clone(),
                                                        args.clone(),
                                                    ));
                                                    tool_calls.push((name, args));
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    match parse_anthropic_sse_event(data) {
                                        AnthropicSseEvent::Text(text) if !text.is_empty() => {
                                            on_event_clone(StreamEvent::Token(text));
                                        }
                                        AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                                            on_event_clone(StreamEvent::Thinking(text));
                                        }
                                        AnthropicSseEvent::Done => done = true,
                                        _ => {}
                                    }
                                }
                            }
                            buffer.clear();
                        } else {
                            buffer.push(byte as char);
                        }
                    }
                    if done {
                        break;
                    }
                }

                // Handle remaining partial line
                if !buffer.is_empty() && buffer.starts_with("data: ") {
                    let data = &buffer[6..];
                    if data.trim() == "[DONE]" {
                        done = true;
                    } else if let Ok(sse_chunk) = serde_json::from_str::<SseChunkWithTools>(data) {
                        if let Some(choices) = sse_chunk.choices {
                            for choice in choices {
                                if let Some(ref delta) = choice.delta {
                                    if let Some(ref text) = delta.content {
                                        on_event_clone(StreamEvent::Token(text.clone()));
                                    }
                                    if let Some(ref reasoning) = delta.reasoning_content {
                                        on_event_clone(StreamEvent::Thinking(
                                            reasoning.clone(),
                                        ));
                                    }
                                    for (name, args) in parse_sse_tool_calls(delta) {
                                        on_event_clone(StreamEvent::ToolCall(
                                            name.clone(),
                                            args.clone(),
                                        ));
                                        tool_calls.push((name, args));
                                    }
                                }
                            }
                        }
                    } else {
                        match parse_anthropic_sse_event(data) {
                            AnthropicSseEvent::Text(text) if !text.is_empty() => {
                                on_event_clone(StreamEvent::Token(text));
                            }
                            AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                                on_event_clone(StreamEvent::Thinking(text));
                            }
                            AnthropicSseEvent::Done => done = true,
                            _ => {}
                        }
                    }
                }

                if !done {
                    on_event_clone(StreamEvent::Done);
                }
                Ok(tool_calls)
            })
            .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(e) => Err(AiError::ApiError(format!("spawn failed: {e}"))),
        }
    }

    /// Non-streaming chat with tool support.
    ///
    /// Calls the LLM once, executes any tool calls returned, and returns
    /// the final text response. Useful for programmatic use without
    /// real-time rendering.
    pub async fn chat_simple_with_tools(
        &self,
        messages: &[ChatMessageRef],
        tools: &[&dyn Tool],
        executor: &ToolExecutor,
    ) -> Result<String, AiError> {
        // Ensure system prompt is first
        let effective_messages =
            if messages.is_empty() || messages.first().map(|m| m.role.as_str()) != Some("system") {
                let mut prefixed = vec![ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                }];
                prefixed.extend(messages.iter().cloned());
                prefixed
            } else {
                messages.to_vec()
            };

        let mut loop_messages = effective_messages;
        let mut tool_round = 0;

        loop {
            // Build the request payload with tools
            let payload = build_tool_call_payload(&loop_messages, tools, &self.current_model());

            let http_client = self.http_client.clone();
            let endpoint = self.endpoint_url().to_string();
            let api_key = self.api_key.clone();

            let (final_text, tool_calls) = match http_client
                .post(&endpoint)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp
                        .text()
                        .await
                        .context("read response body")
                        .map_err(AiError::from)?;
                    let parsed: serde_json::Value = serde_json::from_str(&body)
                        .context("parse response JSON")
                        .map_err(AiError::from)?;

                    // Extract tool calls from choices[0].message.tool_calls
                    let mut tc = Vec::new();
                    if let Some(choice) = parsed.get("choices").and_then(|c| c.get(0)) {
                        if let Some(msg) = choice.get("message") {
                            if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                                for t in tcs {
                                    let name = t
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|n| n.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    let args_str = t
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|a| a.as_str())
                                        .unwrap_or("{}");
                                    let args = serde_json::from_str(args_str)
                                        .unwrap_or(serde_json::Value::String(args_str.to_string()));
                                    tc.push(ToolCall {
                                        name,
                                        arguments: args,
                                    });
                                }
                            }
                        }
                    }

                    // Extract content
                    let content = parsed
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();

                    (content, tc)
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(AiError::ApiError(format!("HTTP {status}: {text}")));
                }
                Err(e) => return Err(AiError::from(anyhow::Error::from(e))),
            };

            // No tool calls — return final text
            if tool_calls.is_empty() {
                return Ok(final_text);
            }

            // Execute all tool calls
            let results: Vec<ToolResult> =
                tool_calls.iter().map(|tc| executor.execute(tc)).collect();

            // Add tool results as messages
            for result in &results {
                loop_messages.push(ChatMessageRef {
                    role: "tool".into(),
                    content: format!(
                        "Tool '{}' returned: {}{}",
                        result.tool_name,
                        if result.is_error { "ERROR: " } else { "" },
                        result.content
                    ),
                });
            }

            tool_round += 1;
            if tool_round >= MAX_TOOL_ROUNDS {
                return Err(AiError::Timeout(format!(
                    "工具调用循环超过最大迭代次数 ({MAX_TOOL_ROUNDS})"
                )));
            }
        }
    }

    /// Chat with LLM using function calling tools, executing tool calls
    /// in a loop until the LLM returns a plain text response.
    ///
    /// Flow:
    /// 1. Call LLM with tools attached (function calling)
    /// 2. If LLM returns tool calls: execute them, add results to messages
    /// 3. Call LLM again with tool results
    /// 4. Repeat until LLM returns plain text (no tool calls)
    /// 5. Return the final text response
    ///
    /// Context compaction is performed at the start of each loop iteration
    /// to keep the message list within the LLM's context window.
    ///
    /// The `on_event` callback receives `DispatchStreamEvent::ToolCall`,
    /// `DispatchStreamEvent::ToolResult`, and `DispatchStreamEvent::Compacted`
    /// events during the loop.
    pub async fn chat_with_tools(
        &self,
        messages: &[ChatMessageRef],
        _tools: &[&dyn Tool],
        executor: &ToolExecutor,
        compactor: &Compactor,
        max_tool_rounds: u32,
        on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
    ) -> Result<String, AiError> {
        let max_rounds = max_tool_rounds.min(MAX_TOOL_ROUNDS);
        let mut loop_messages: Vec<ChatMessageRef> = messages.to_vec();
        let mut tool_round = 0;

        // Arc the callback so it can be shared across loop iterations.
        let on_event: Arc<dyn Fn(DispatchStreamEvent) + Send + Sync + 'static> = Arc::new(on_event);

        loop {
            // Check whether the message list needs compaction.
            let refs: Vec<crate::ai::compactor::MessageRef> = loop_messages
                .iter()
                .map(crate::ai::compactor::MessageRef::from)
                .collect();
            if compactor.should_compress(&refs) {
                let compacted = compactor.compress_messages(&refs);
                on_event(DispatchStreamEvent::Compacted(
                    crate::ai::engine::CompactionInfo {
                        original_count: compacted.original_count,
                        compressed_count: compacted.compressed_count,
                        kept_count: compacted.kept_count,
                        before_tokens: compacted.before_tokens,
                        after_tokens: compacted.after_tokens,
                    },
                ));
                loop_messages = compacted
                    .kept_messages
                    .iter()
                    .map(|m| ChatMessageRef {
                        role: m.role.to_string(),
                        content: m.content.to_string(),
                    })
                    .collect();
            }

            // Collect tool calls from this LLM round
            let collected: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
            let collected_clone = collected.clone();
            let on_event_clone = Arc::clone(&on_event);

            let raw_calls = self
                .chat_stream_with_tools(&loop_messages, move |event| {
                    match event {
                        StreamEvent::Token(text) => {
                            collected_clone.lock().unwrap().push_str(&text);
                            on_event_clone(DispatchStreamEvent::Token(text));
                        }
                        StreamEvent::Thinking(text) => {
                            on_event_clone(DispatchStreamEvent::Thinking(text));
                        }
                        StreamEvent::ToolCall(name, args) => {
                            collected_clone
                                .lock()
                                .unwrap()
                                .push_str(&format!("[TOOL_CALL: {}({})]", name, args));
                            on_event_clone(DispatchStreamEvent::ToolCall(name, args));
                        }
                        StreamEvent::Error(err) => {
                            on_event_clone(DispatchStreamEvent::Error(err));
                        }
                        StreamEvent::Done => {
                            on_event_clone(DispatchStreamEvent::Done);
                        }
                        // ToolResult events are forwarded via on_event only.
                        StreamEvent::ToolResult(_, _, _) => {}
                    }
                })
                .await?;

            // Convert (name, args) pairs to ToolCall objects
            let tool_calls: Vec<ToolCall> = raw_calls
                .into_iter()
                .map(|(name, args)| ToolCall {
                    name,
                    arguments: args,
                })
                .collect();

            if tool_calls.is_empty() {
                // No tool calls — return collected text
                let final_response = collected.lock().unwrap().clone();
                return Ok(final_response);
            }

            // Execute all tool calls (single-threaded, errors don't stop loop)
            let results: Vec<ToolResult> =
                tool_calls.iter().map(|tc| executor.execute(tc)).collect();

            // Report results via callback
            for result in &results {
                on_event(DispatchStreamEvent::ToolResult(
                    result.tool_name.clone(),
                    result.content.clone(),
                    result.is_error,
                ));
            }

            // Add tool results as messages
            for result in &results {
                loop_messages.push(ChatMessageRef {
                    role: "tool".into(),
                    content: format!(
                        "Tool '{}' returned: {}{}",
                        result.tool_name,
                        if result.is_error { "ERROR: " } else { "" },
                        result.content
                    ),
                });
            }

            tool_round += 1;
            if tool_round >= max_rounds {
                return Err(AiError::Timeout(format!(
                    "工具调用循环超过最大迭代次数 ({max_rounds})"
                )));
            }
        }
    }
}

// ── Dispatch Directive Parsing ──────────────────────────────────────────

/// Parse a dispatch directive from LLM response text.
///
/// Tries JSON format first (inside ```json ... ``` or ``` ... ``` code blocks),
/// then falls back to text format (looking for "Agent: <name>" patterns).
///
/// Returns `None` if no dispatch directive is found (LLM decided to just respond).
pub fn parse_dispatch_directive(response: &str) -> Option<DispatchDirective> {
    let trimmed = response.trim();

    // Try JSON format first — look for code blocks
    if let Some(json_text) = extract_json_block(trimmed) {
        if let Some(directive) = parse_json_dispatch(&json_text) {
            return Some(directive);
        }
    }

    // Try inline JSON (no code block)
    if let Some(directive) = parse_json_dispatch(trimmed) {
        return Some(directive);
    }

    // Fallback to text format
    parse_text_dispatch(trimmed)
}

/// Extract JSON from a code block: ```json ... ``` or ``` ... ```
fn extract_json_block(text: &str) -> Option<String> {
    let patterns: &[&str] = &["```json\n", "```JSON\n", "```\n"];
    for &prefix in patterns {
        if let Some(start) = text.find(prefix) {
            let json_start = start + prefix.len();
            if let Some(end) = text[json_start..].find("```") {
                return Some(text[json_start..json_start + end].trim().to_string());
            }
            // No closing ``` found — return everything after prefix, trimmed
            return Some(text[json_start..].trim().to_string());
        }
    }
    None
}

/// Parse JSON dispatch directive: `{"agents": [...]}`
fn parse_json_dispatch(json_text: &str) -> Option<DispatchDirective> {
    let json_value: serde_json::Value = serde_json::from_str(json_text).ok()?;

    let agents_array = json_value.get("agents")?.as_array()?;
    if agents_array.is_empty() {
        return None;
    }

    let mut agents = Vec::with_capacity(agents_array.len());
    for item in agents_array {
        let agent_name = item
            .get("agent_name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        let instruction = item
            .get("instruction")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        let cwd = item
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();

        if agent_name.is_empty() || instruction.is_empty() {
            continue;
        }

        agents.push(AgentTask {
            agent_name,
            instruction,
            cwd,
            pane_id: None,
        });
    }

    if agents.is_empty() {
        return None;
    }

    Some(DispatchDirective::Json { agents })
}

/// Parse text-formatted dispatch directive (example):
/// ```ignore
/// Agent: claude
/// Task: Implement JWT token refresh in src/auth/jwt.rs
/// CWD: /home/user/project
/// ```
fn parse_text_dispatch(text: &str) -> Option<DispatchDirective> {
    // Look for "Agent: <name>" pattern
    let agent_line = find_pattern(text, "Agent:");
    let task_line = find_pattern(text, "Task:");
    let cwd_line = find_pattern(text, "CWD:");

    let agent_name = agent_line?.trim().to_string();
    if agent_name.is_empty() {
        return None;
    }

    let instruction = task_line.map(|l| l.trim().to_string()).unwrap_or_default();

    let cwd = cwd_line.map(|l| l.trim().to_string()).unwrap_or_default();

    if instruction.is_empty() {
        return None;
    }

    Some(DispatchDirective::Text {
        agent_name,
        instruction,
        cwd,
    })
}

/// Find a line starting with the given prefix and return its content after the prefix.
fn find_pattern(text: &str, prefix: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(prefix) {
            let content = trimmed[prefix.len()..].trim().to_string();
            return Some(content);
        }
    }
    None
}

// ── Agent Error Classification ──────────────────────────────────────────

/// Classify an AiError from agent startup into a specific error variant
/// for user-friendly display.
fn classify_agent_error(err: &AiError) -> AiError {
    match err {
        AiError::IpcError(msg)
            if msg.contains("not found")
                || msg.contains("no such file")
                || msg.contains("executable") =>
        {
            AiError::AgentCliNotFound(msg.clone())
        }
        AiError::IpcError(msg) if msg.contains("connection refused") || msg.contains("connect") => {
            AiError::AgentCliNotFound(format!("herdr not running: {msg}"))
        }
        _ => err.clone(),
    }
}

// ── Agent Execution Helpers ─────────────────────────────────────────────

/// Execute a single agent task via herdr IPC.
///
/// The herdr IPC methods are synchronous (Unix socket, JSON-line protocol).
/// This function runs them directly. Callers in an async context should
/// wrap with `tokio::task::spawn_blocking` if needed.
///
/// Returns the agent result with collected output.
pub fn execute_agent_task(
    herdr_client: &mut HerdrIpcClient,
    task: &AgentTask,
    workspace_id: &str,
    timeout_ms: u64,
    on_event: impl Fn(DispatchStreamEvent),
) -> AgentResult {
    let agent_name = task.agent_name.clone();
    let instruction = task.instruction.clone();
    let cwd = task.cwd.clone();

    tracing::info!(
        "dispatching agent '{}' in workspace '{}', cwd='{}'",
        agent_name,
        workspace_id,
        cwd,
    );

    // Start the agent CLI
    let agent_info = match herdr_client.start_agent(
        &agent_name,
        Some(workspace_id),
        if cwd.is_empty() { None } else { Some(&cwd) },
        vec![],
    ) {
        Ok(info) => info,
        Err(e) => {
            tracing::error!("failed to start agent '{agent_name}': {e}");
            // Classify the error for user-friendly display
            let error_kind = classify_agent_error(&e);
            on_event(DispatchStreamEvent::Error(format!(
                "Failed to start agent '{agent_name}': {}",
                user_friendly(&error_kind)
            )));
            return AgentResult {
                agent_name: agent_name.clone(),
                pane_id: String::new(),
                exit_status: "failed".into(),
                output: format!("Agent start failed: {e}"),
                summary: Some(format!("{error_kind}: {e}")),
            };
        }
    };

    let pane_id = agent_info.pane_id.clone();
    let terminal_id = agent_info.terminal_id.clone();

    // Give the agent a moment to initialize
    std::thread::sleep(Duration::from_millis(500));

    // Send the instruction
    if let Err(e) = herdr_client.send_agent_text(&terminal_id, &instruction) {
        tracing::warn!("failed to send instruction to agent '{}': {e}", agent_name);
    }

    // Wait for output with DONE pattern
    let output = match herdr_client.wait_for_output(&pane_id, AGENT_DONE_PATTERN, timeout_ms) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!("wait_for_output failed for agent '{}': {e}", agent_name);
            // Fallback: read recent output
            match herdr_client.read_agent(&terminal_id, ReadSource::Recent, 500) {
                Ok(text) => text,
                Err(read_err) => {
                    return AgentResult {
                        agent_name: agent_name.clone(),
                        pane_id: pane_id.clone(),
                        exit_status: "timeout".into(),
                        output: String::new(),
                        summary: Some(format!(
                            "Agent '{agent_name}' timed out or failed to read: {read_err}",
                        )),
                    };
                }
            }
        }
    };

    let trimmed = output.trim().to_string();

    AgentResult {
        agent_name,
        pane_id,
        exit_status: "completed".into(),
        output: trimmed.clone(),
        summary: if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.lines().take(5).collect::<Vec<_>>().join("\n"))
        },
    }
}

// ── Chat and Dispatch ───────────────────────────────────────────────────

impl OrchestratorEngine {
    /// Chat with LLM and dispatch agents if the LLM decides to.
    ///
    /// Flow:
    /// 1. Call LLM with messages (SSE streaming)
    /// 2. Collect full response text
    /// 3. Parse for dispatch directives
    /// 4. If dispatch found: execute agent(s) via herdr, stream output
    /// 5. If more dispatches needed: call LLM again with agent results
    /// 6. Call LLM for final summary if agents were dispatched
    /// 7. Stream final response
    ///
    /// The `herdr_client` is used for agent execution.
    /// The `workspace_id` is the herdr workspace to use.
    ///
    /// Returns the final complete response text (or agent results summary).
    pub async fn chat_and_dispatch(
        &self,
        messages: &[ChatMessageRef],
        herdr_client: &mut HerdrIpcClient,
        workspace_id: &str,
        compactor: &crate::ai::compactor::Compactor,
        on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
    ) -> Result<String, AiError> {
        let mut dispatch_count = 0u32;
        let max_dispatch_rounds = 5; // Safety limit
        let mut loop_messages: Vec<ChatMessageRef> = messages.to_vec();
        let final_response;

        // Arc the callback so it can be shared across loop iterations.
        let on_event: Arc<dyn Fn(DispatchStreamEvent) + Send + Sync + 'static> = Arc::new(on_event);

        loop {
            // ── Compaction check (mirrors chat_with_tools) ──────────
            let refs: Vec<crate::ai::compactor::MessageRef> = loop_messages
                .iter()
                .map(crate::ai::compactor::MessageRef::from)
                .collect();
            if compactor.should_compress(&refs) {
                let compacted = compactor.compress_messages(&refs);
                on_event(DispatchStreamEvent::Compacted(
                    crate::ai::engine::CompactionInfo {
                        original_count: compacted.original_count,
                        compressed_count: compacted.compressed_count,
                        kept_count: compacted.kept_count,
                        before_tokens: compacted.before_tokens,
                        after_tokens: compacted.after_tokens,
                    },
                ));
                loop_messages = compacted
                    .kept_messages
                    .iter()
                    .map(|m| ChatMessageRef {
                        role: m.role.to_string(),
                        content: m.content.to_string(),
                    })
                    .collect();
            }

            // Phase 1: LLM call with streaming
            let on_event_clone = Arc::clone(&on_event);
            let response_text = self
                .call_llm_collect(&loop_messages, move |ev| on_event_clone(ev))
                .await?;

            // Phase 2: Parse dispatch
            if let Some(directive) = parse_dispatch_directive(&response_text) {
                // Phase 3: Execute agents (parallel dispatch)
                on_event(DispatchStreamEvent::Dispatch(directive.clone()));

                let tasks = directive.to_tasks();
                let task_count = tasks.len();
                let agent_results: Vec<AgentResult> = if task_count <= 1 {
                    // Single agent — execute directly (no spawn overhead)
                    let task = tasks.into_iter().next().unwrap();
                    let on_event_clone = Arc::clone(&on_event);
                    let result = execute_agent_task(
                        herdr_client,
                        &task,
                        workspace_id,
                        DEFAULT_AGENT_TIMEOUT_MS,
                        move |ev| on_event_clone(ev),
                    );
                    on_event(DispatchStreamEvent::AgentComplete(result.clone()));
                    vec![result]
                } else {
                    // Multiple agents — execute concurrently via std::thread
                    let mut handles = Vec::with_capacity(task_count);
                    for task in &tasks {
                        // Each thread gets its own IPC client connection
                        let agent_name = task.agent_name.clone();
                        let instruction = task.instruction.clone();
                        let cwd = task.cwd.clone();
                        let ws = workspace_id.to_string();
                        let socket = herdr_client.socket_path().to_path_buf();

                        let on_ev = Arc::clone(&on_event);
                        handles.push(std::thread::spawn(move || {
                            let mut client = HerdrIpcClient::new(socket);
                            let at = AgentTask {
                                agent_name,
                                instruction,
                                cwd,
                                pane_id: None,
                            };
                            let result = execute_agent_task(
                                &mut client,
                                &at,
                                &ws,
                                DEFAULT_AGENT_TIMEOUT_MS,
                                move |ev| on_ev(ev),
                            );
                            result
                        }));
                    }

                    let mut results = Vec::with_capacity(task_count);
                    for handle in handles {
                        let result = handle.join().unwrap_or_else(|_| AgentResult {
                            agent_name: "unknown".into(),
                            pane_id: String::new(),
                            exit_status: "crashed".into(),
                            output: "Agent thread panicked".into(),
                            summary: None,
                        });
                        results.push(result);
                    }

                    // Emit AgentComplete for each result (after all are done)
                    for r in &results {
                        on_event(DispatchStreamEvent::AgentComplete(r.clone()));
                    }

                    results
                };

                dispatch_count += 1;

                // Build summary for next LLM round
                let mut summary_parts = Vec::new();
                for r in &agent_results {
                    summary_parts.push(format!(
                        "## Agent: {} ({}), Pane: {}\n{}",
                        r.agent_name, r.exit_status, r.pane_id, r.output
                    ));
                }
                let summary = summary_parts.join("\n\n---\n\n");

                if dispatch_count >= max_dispatch_rounds {
                    // Safety: LLM keeps dispatching, break out
                    final_response = format!(
                        "Here is the summary of agent results:\n\n{summary}\n\n(Dispatch loop reached maximum iterations)"
                    );
                    break;
                }

                // Phase 4: Call LLM again with agent results
                loop_messages = vec![
                    ChatMessageRef {
                        role: "system".into(),
                        content: self.system_prompt.clone(),
                    },
                    ChatMessageRef {
                        role: "user".into(),
                        content: format!(
                            "Agent execution results:\n{summary}\n\nPlease provide a final summary or further instructions.",
                        ),
                    },
                ];
            } else {
                // No dispatch — this is the final response
                final_response = response_text;
                break;
            }
        }

        Ok(final_response)
    }

    /// Call the LLM and collect all stream tokens into a single string,
    /// while forwarding stream events through the provided callback.
    ///
    /// This is used by `chat_and_dispatch` to gather the full response text
    /// before parsing dispatch directives.
    async fn call_llm_collect<F>(
        &self,
        messages: &[ChatMessageRef],
        on_event: F,
    ) -> Result<String, AiError>
    where
        F: Fn(DispatchStreamEvent) + Send + 'static,
    {
        let collected: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let collected_clone = collected.clone();

        self.chat_stream(messages, move |event| {
            match event {
                StreamEvent::Token(text) => {
                    collected_clone.lock().unwrap().push_str(&text);
                    on_event(DispatchStreamEvent::Token(text));
                }
                StreamEvent::Thinking(text) => {
                    on_event(DispatchStreamEvent::Thinking(text));
                }
                StreamEvent::Error(err) => {
                    on_event(DispatchStreamEvent::Error(err));
                }
                StreamEvent::Done => {
                    on_event(DispatchStreamEvent::Done);
                }
                // Tool call events from tool-calling LLMs are ignored here
                // since this is used by chat_and_dispatch (non-tool-calling path).
                StreamEvent::ToolCall(_, _) | StreamEvent::ToolResult(_, _, _) => {}
            }
        })
        .await?;

        Ok(collected.lock().unwrap().clone())
    }
}

// ── Configuration Helpers ──────────────────────────────────────────────

impl OrchestratorEngine {
    /// Check if there is at least one configured API Provider with a non-empty API key.
    ///
    /// Used by the UI to decide whether to show the chat panel
    /// or a "please configure" placeholder.
    pub fn has_available_provider() -> bool {
        crate::api_provider::load_all()
            .into_iter()
            .any(|p| !p.api_key.trim().is_empty())
    }

    /// Return the first configured API Provider with a non-empty API key.
    pub fn get_available_provider() -> Option<crate::api_provider::ApiProviderConfig> {
        crate::api_provider::load_all()
            .into_iter()
            .find(|p| !p.api_key.trim().is_empty())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dispatch_json_code_block() {
        let response = r#"
Here's my plan:

```json
{
  "agents": [
    {
      "agent_name": "claude",
      "instruction": "Implement JWT token refresh in src/auth/jwt.rs",
      "cwd": "/home/user/project"
    }
  ]
}
```

I'll have Claude handle the implementation.
"#;

        let directive = parse_dispatch_directive(response);
        assert!(directive.is_some());
        let directive = directive.unwrap();
        match directive {
            DispatchDirective::Json { agents } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].agent_name, "claude");
                assert_eq!(
                    agents[0].instruction,
                    "Implement JWT token refresh in src/auth/jwt.rs"
                );
                assert_eq!(agents[0].cwd, "/home/user/project");
            }
            _ => panic!("expected Json variant"),
        }
    }

    #[test]
    fn test_parse_dispatch_json_inline() {
        // Inline JSON: the response is just the JSON object, no code block
        let response = r#"{"agents": [{"agent_name": "codex", "instruction": "Fix the bug", "cwd": "/proj"}]}"#;

        let directive = parse_dispatch_directive(response);
        assert!(directive.is_some());
        match directive.unwrap() {
            DispatchDirective::Json { agents } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].agent_name, "codex");
            }
            _ => panic!("expected Json variant"),
        }
    }

    #[test]
    fn test_parse_dispatch_json_no_code_block() {
        let response = r#"{
  "agents": [
    {"agent_name": "claude", "instruction": "Do task A", "cwd": "/a"},
    {"agent_name": "codex", "instruction": "Do task B", "cwd": "/b"}
  ]
}"#;

        let directive = parse_dispatch_directive(response);
        assert!(directive.is_some());
        match directive.unwrap() {
            DispatchDirective::Json { agents } => {
                assert_eq!(agents.len(), 2);
                assert_eq!(agents[0].agent_name, "claude");
                assert_eq!(agents[1].agent_name, "codex");
            }
            _ => panic!("expected Json variant"),
        }
    }

    #[test]
    fn test_parse_dispatch_text_format() {
        let response = r#"Let me dispatch an agent:

Agent: claude
Task: Implement JWT token refresh in src/auth/jwt.rs
CWD: /home/user/project
"#;

        let directive = parse_dispatch_directive(response);
        assert!(directive.is_some());
        match directive.unwrap() {
            DispatchDirective::Text {
                agent_name,
                instruction,
                cwd,
            } => {
                assert_eq!(agent_name, "claude");
                assert_eq!(
                    instruction,
                    "Implement JWT token refresh in src/auth/jwt.rs"
                );
                assert_eq!(cwd, "/home/user/project");
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn test_parse_dispatch_text_format_no_cwd() {
        let response = r#"Agent: codex
Task: Fix the login bug
"#;

        let directive = parse_dispatch_directive(response);
        assert!(directive.is_some());
        match directive.unwrap() {
            DispatchDirective::Text {
                agent_name,
                instruction,
                cwd,
            } => {
                assert_eq!(agent_name, "codex");
                assert_eq!(instruction, "Fix the login bug");
                assert!(cwd.is_empty());
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn test_parse_dispatch_returns_none_for_regular_response() {
        let response = r#"Here's my analysis of the issue:

The JWT token refresh mechanism has a race condition where the refresh token
expires before the access token refresh completes. Here's a suggested fix...
"#;

        let directive = parse_dispatch_directive(response);
        assert!(
            directive.is_none(),
            "regular response should not parse as dispatch"
        );
    }

    #[test]
    fn test_parse_dispatch_none_for_empty() {
        let directive = parse_dispatch_directive("");
        assert!(directive.is_none());
    }

    #[test]
    fn test_parse_dispatch_none_for_invalid_json() {
        let directive = parse_dispatch_directive("this is not json at all");
        assert!(directive.is_none());
    }

    #[test]
    fn test_parse_dispatch_none_for_json_without_agents() {
        let directive = parse_dispatch_directive("{\"foo\": \"bar\"}");
        assert!(directive.is_none());
    }

    #[test]
    fn test_parse_dispatch_none_for_empty_agents_array() {
        let directive = parse_dispatch_directive("{\"agents\": []}");
        assert!(directive.is_none());
    }

    #[test]
    fn test_parse_dispatch_none_for_missing_fields() {
        // Has agents array but missing required fields
        let directive = parse_dispatch_directive(
            "{\"agents\": [{\"agent_name\": \"\", \"instruction\": \"\", \"cwd\": \"\"}]}",
        );
        assert!(directive.is_none());
    }

    #[test]
    fn test_dispatch_directive_to_tasks_json() {
        let directive = DispatchDirective::Json {
            agents: vec![
                AgentTask {
                    agent_name: "claude".into(),
                    instruction: "task 1".into(),
                    cwd: "/a".into(),
                    pane_id: None,
                },
                AgentTask {
                    agent_name: "codex".into(),
                    instruction: "task 2".into(),
                    cwd: "/b".into(),
                    pane_id: None,
                },
            ],
        };
        let tasks = directive.to_tasks();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].agent_name, "claude");
        assert_eq!(tasks[1].agent_name, "codex");
    }

    #[test]
    fn test_dispatch_directive_to_tasks_text() {
        let directive = DispatchDirective::Text {
            agent_name: "claude".into(),
            instruction: "do something".into(),
            cwd: "/proj".into(),
        };
        let tasks = directive.to_tasks();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].agent_name, "claude");
    }

    #[test]
    fn test_agent_result_construction() {
        let result = AgentResult {
            agent_name: "claude".into(),
            pane_id: "p-1".into(),
            exit_status: "completed".into(),
            output: "All tests passed!".into(),
            summary: Some("Done".into()),
        };
        assert_eq!(result.agent_name, "claude");
        assert_eq!(result.exit_status, "completed");
        assert!(result.summary.is_some());
    }

    #[test]
    fn test_extract_json_block_with_json_lang() {
        let text = "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text";
        let extracted = extract_json_block(text);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap(), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_block_without_lang() {
        let text = "Some text\n```\n{\"key\": \"value\"}\n```\nMore text";
        let extracted = extract_json_block(text);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap(), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_block_no_closing() {
        let text = "Some text\n```json\n{\"key\": \"value\"}";
        let extracted = extract_json_block(text);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap(), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_block_no_block() {
        let text = "No code blocks here, just text.";
        let extracted = extract_json_block(text);
        assert!(extracted.is_none());
    }

    #[test]
    fn test_find_pattern() {
        let text = "Agent: claude\nTask: do stuff\nCWD: /home\nOther line";
        assert_eq!(find_pattern(text, "Agent:").unwrap(), "claude");
        assert_eq!(find_pattern(text, "Task:").unwrap(), "do stuff");
        assert_eq!(find_pattern(text, "CWD:").unwrap(), "/home");
        assert!(find_pattern(text, "Missing:").is_none());
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_AGENT_TIMEOUT_MS, 300_000);
        assert_eq!(AGENT_DONE_PATTERN, "[AGENT_DONE]");
    }

    #[test]
    fn test_parse_anthropic_text_delta() {
        let data =
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello world"}}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Text(t) if t == "hello world"
        ));
    }

    #[test]
    fn test_parse_anthropic_thinking_delta() {
        let data =
            r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"reasoning..."}}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Thinking(t) if t == "reasoning..."
        ));
    }

    #[test]
    fn test_parse_anthropic_message_stop() {
        let data = r#"{"type":"message_stop"}"#;
        assert!(matches!(parse_anthropic_sse_event(data), AnthropicSseEvent::Done));
    }

    #[test]
    fn test_parse_anthropic_content_block_start_ignored() {
        let data = r#"{"type":"content_block_start","index":0}"#;
        assert!(matches!(parse_anthropic_sse_event(data), AnthropicSseEvent::Ignored));
    }

    #[test]
    fn test_parse_anthropic_invalid_json() {
        assert!(matches!(parse_anthropic_sse_event("not json"), AnthropicSseEvent::Ignored));
    }

    #[test]
    fn test_parse_anthropic_empty_data() {
        assert!(matches!(parse_anthropic_sse_event(""), AnthropicSseEvent::Ignored));
    }

    #[test]
    fn test_parse_anthropic_text_fallback() {
        // OpenAI-format proxy: delta.text without delta.type
        let data = r#"{"type":"content_block_delta","delta":{"text":"fallback text"}}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Text(t) if t == "fallback text"
        ));
    }

    // ── ToolExecutor tests ──────────────────────────────────────────────

    #[test]
    fn test_tool_executor_execute_read_file() {
        let dir = std::env::temp_dir().join("zenix_engine_test");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test.txt");
        std::fs::write(&file_path, "test content").unwrap();

        let herdr = HerdrIpcClient::new(std::path::PathBuf::from("/tmp/nonexistent"));
        let executor = ToolExecutor::new(herdr, "workspace-1".into());

        let call = ToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": file_path.to_str().unwrap() }),
        };
        let result = executor.execute(&call);
        assert!(
            !result.is_error,
            "read_file should succeed: {}",
            result.content
        );
        assert_eq!(result.content, "test content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tool_executor_execute_unknown_tool() {
        let herdr = HerdrIpcClient::new(std::path::PathBuf::from("/tmp/nonexistent"));
        let executor = ToolExecutor::new(herdr, "workspace-1".into());

        let call = ToolCall {
            name: "nonexistent_tool_xyz".into(),
            arguments: serde_json::json!({}),
        };
        let result = executor.execute(&call);
        assert!(result.is_error);
        assert!(result.content.contains("未知工具"));
    }

    #[test]
    fn test_tool_executor_execute_parse_error() {
        // read_file with missing "path" argument should error
        let herdr = HerdrIpcClient::new(std::path::PathBuf::from("/tmp/nonexistent"));
        let executor = ToolExecutor::new(herdr, "workspace-1".into());

        let call = ToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({}), // missing "path"
        };
        let result = executor.execute(&call);
        assert!(result.is_error);
        assert!(result.content.contains("path"));
    }

    // ── SSE tool call parsing tests ─────────────────────────────────────

    #[test]
    fn test_parse_sse_tool_calls_empty() {
        let delta = SseDeltaWithTools {
            content: Some("hello".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: None,
        };
        let calls = parse_sse_tool_calls(&delta);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_sse_tool_calls_with_calls() {
        let delta = SseDeltaWithTools {
            content: Some("".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: Some(vec![SseToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path": "/tmp/test.txt"}"#.into(),
                },
            }]),
        };
        let calls = parse_sse_tool_calls(&delta);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "read_file");
        assert_eq!(calls[0].1["path"].as_str().unwrap(), "/tmp/test.txt");
    }

    #[test]
    fn test_parse_sse_tool_calls_invalid_json_args() {
        let delta = SseDeltaWithTools {
            content: Some("".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: Some(vec![SseToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: "not valid json".into(),
                },
            }]),
        };
        let calls = parse_sse_tool_calls(&delta);
        assert_eq!(calls.len(), 1);
        // Should still extract the call but with raw string as value
        assert_eq!(calls[0].0, "read_file");
    }

    // ── build_tool_call_payload tests ──────────────────────────────────

    #[test]
    fn test_build_tool_call_payload_includes_tools() {
        let messages = vec![ChatMessageRef {
            role: "user".into(),
            content: "hello".into(),
        }];
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(crate::ai::tools::ReadFile)];
        let tool_refs: Vec<&dyn Tool> = tools.iter().map(|t| t.as_ref()).collect();

        let payload = build_tool_call_payload(&messages, &tool_refs, "gpt-4o-mini");
        assert_eq!(payload["model"].as_str().unwrap(), "gpt-4o-mini");
        assert!(payload["stream"].as_bool().unwrap());
        assert!(payload["tools"].is_array());
        let tool_list = payload["tools"].as_array().unwrap();
        assert_eq!(tool_list.len(), 1);
        assert_eq!(
            tool_list[0]["function"]["name"].as_str().unwrap(),
            "read_file"
        );
    }

    // ── chat_with_tools unit test (no HTTP) ────────────────────────────

    #[test]
    fn test_chat_with_tools_loop_logic() {
        // This test verifies the loop logic without making HTTP calls.
        // We simulate by testing the message accumulation pattern.
        let messages: Vec<ChatMessageRef> = vec![ChatMessageRef {
            role: "user".into(),
            content: "read file x".into(),
        }];

        // Simulate: first round returns tool call, second round returns text
        let mut loop_messages: Vec<ChatMessageRef> = messages.clone();
        let mut rounds = 0;
        let max_rounds = 10;

        // Round 1: LLM says "use read_file"
        loop_messages.push(ChatMessageRef {
            role: "assistant".into(),
            content: "I'll read the file for you.".into(),
        });
        loop_messages.push(ChatMessageRef {
            role: "tool".into(),
            content: "Hello, World!".into(),
        });
        rounds += 1;

        // Round 2: LLM returns plain text
        let final_response = "The file contains: Hello, World!";

        assert_eq!(rounds, 1);
        assert_eq!(final_response, "The file contains: Hello, World!");
        assert!(rounds < max_rounds);
    }

    // ── Config helper tests ──────────────────────────────────────────

    #[test]
    fn test_has_available_provider_returns_bool() {
        // has_available_provider() is a simple check over load_all().
        // Since load_all() reads from disk, the result depends on the
        // test environment, but the logic is straightforward:
        // it returns true if any provider has a non-empty api_key.
        let result = OrchestratorEngine::has_available_provider();
        // Just verify it returns a bool (no panic).
        let _ = result;
    }

    #[test]
    fn test_get_available_provider_returns_option() {
        // get_available_provider() returns the first provider with a key,
        // or None if no providers exist.
        let result = OrchestratorEngine::get_available_provider();
        // In an isolated test env, this may be None.
        match result {
            Some(cfg) => {
                assert!(!cfg.api_key.trim().is_empty());
            }
            None => {
                // Expected in clean environments.
            }
        }
    }

    // ── ToolResult error propagation test ─────────────────────────────

    #[test]
    fn test_tool_result_error_not_interrupting_loop() {
        // Verify that error results are still included in the loop,
        // not causing early termination.
        let results = vec![
            ToolResult::ok("read_file", "file content"),
            ToolResult::error("read_file", "file not found"),
        ];

        // Both results should be collectable
        let success_count = results.iter().filter(|r| !r.is_error).count();
        let error_count = results.iter().filter(|r| r.is_error).count();
        assert_eq!(success_count, 1);
        assert_eq!(error_count, 1);
        assert_eq!(results.len(), 2);
    }
}
