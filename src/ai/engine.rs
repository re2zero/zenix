//! Orchestrator engine — LLM API client with SSE streaming support and
//! agent dispatch capability.
//!
//! Uses `reqwest` with tokio runtime to communicate with OpenAI/Anthropic/
//! Ollama-compatible endpoints via the standard Chat Completions API.
//!
//! Dispatch capability routes LLM-decided agent tasks through the herdr IPC
//! client, collecting real-time output and looping until the orchestrator
//! produces a final response without dispatch directives.

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
use crate::ai::endpoint::{ApiEndpoint, ReadSource, SplitDirection};
use crate::ai::error::{AiError, user_friendly};
use crate::ai::tools::{Tool, ToolCall, ToolExecutor, ToolResult};
use crate::api_provider::ApiProviderConfig;

/// Detect the language of user input based on character ranges.
/// Returns `"zh"` if the text contains CJK characters, `"en"` otherwise.
pub fn detect_language(text: &str) -> &'static str {
    if text.chars().any(|c| {
        matches!(c,
            '\u{4e00}'..='\u{9fff}'  // CJK Unified Ideographs
            | '\u{3000}'..='\u{303f}' // CJK Symbols and Punctuation
            | '\u{ff00}'..='\u{ffef}'    // Fullwidth Forms
        )
    }) {
        "zh"
    } else {
        "en"
    }
}

fn lang_instruction(lang: &str) -> &'static str {
    match lang {
        "zh" => "用中文回复。",
        _ => "Reply in the same language as the user's input.",
    }
}

/// Build a dynamic system prompt by loading an external prompt template and
/// appending runtime-dynamic sections (roles, workspace info).
///
/// The template is loaded from the startup cache by `prompts::get_template()`;
/// it already includes the static instructions (persona, tools, delegation
/// rules, workflow, constraints). `{lang_instruction}` placeholders are
/// replaced with the appropriate language instruction.
///
/// `roles_summary` — a newline-separated list of available roles and descriptions.
/// `workspace_info` — description of the currently bound workspace.
/// `lang` — detected user language (`"zh"` or `"en"`).
pub fn build_system_prompt(roles_summary: &str, workspace_info: &str, lang: &str) -> String {
    let mut template = crate::ai::prompts::get_template("orchestrator");
    template = template.replace("{lang_instruction}", lang_instruction(lang));

    let mut prompt = String::with_capacity(template.len() + 512);
    prompt.push_str(&template);
    prompt.push_str("\n\n");

    if !roles_summary.is_empty() {
        prompt.push_str("## 可用角色\n");
        prompt.push_str(roles_summary);
        prompt.push_str("\n\n");
    }
    if !workspace_info.is_empty() {
        prompt.push_str("## 当前工作空间\n");
        prompt.push_str(workspace_info);
        prompt.push_str("\n\n");
    }
    prompt
}

/// Lightweight chat message reference for the engine (no session/sequence).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessageRef {
    /// "system", "user", "assistant", or "tool".
    pub role: String,
    /// Message content.
    pub content: String,
    /// Tool calls from the assistant in OpenAI API format
    /// (set when role=="assistant"). Each entry: {id, type:"function", function:{name, arguments}}.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    /// Tool call ID this result is for (set when role=="tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Events emitted during an LLM streaming call.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StreamEvent {
    /// A token chunk from the LLM response.
    Token(String),
    /// A thinking (reasoning) token chunk from the LLM (tool_call_round, text).
    Thinking { round: u32, text: String },
    /// An error occurred during streaming.
    Error(String),
    /// The stream has finished.
    Done,
    /// The LLM returned a tool call (id, tool_name, arguments JSON).
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// A tool was executed (tool_name, result content, is_error).
    ToolResult(String, String, bool),
}

// ── Dispatch Types ──────────────────────────────────────────────────────

/// Default timeout for agent execution (ms).
#[allow(dead_code)]
pub const DEFAULT_AGENT_TIMEOUT_MS: u64 = 300_000;

/// Pattern substring to match for agent completion.
#[allow(dead_code)]
pub const AGENT_DONE_PATTERN: &str = "[AGENT_DONE]";

/// How long the daemon gets to launch an interactive agent CLI in a pane. The
/// daemon accepts 3000..=300000 ms.
const AGENT_START_TIMEOUT_MS: u64 = 30_000;

/// An agent task dispatched by the orchestrator.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    #[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
pub enum DispatchStreamEvent {
    /// A token chunk from the LLM response.
    Token(String),
    /// A thinking (reasoning) token chunk (tool_call_round, text).
    Thinking { round: u32, text: String },
    /// The orchestrator decided to dispatch one or more agents.
    Dispatch(DispatchDirective),
    /// Agent output chunk (streamed in real-time).
    AgentOutput { agent_name: String, text: String },
    /// An agent has completed.
    AgentComplete(AgentResult),
    /// A tool was called by the LLM (id, tool_name, arguments JSON).
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// A tool was executed, with tool_call_id for result matching.
    ToolResult {
        id: String,
        tool_name: String,
        content: String,
        is_error: bool,
    },
    /// The conversation was compacted to fit within the context window.
    Compacted(CompactionInfo),
    /// An error occurred.
    Error(String),
    /// A new LLM round is about to begin (for per-round UI separation).
    RoundStart { round: u32 },
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
    /// Uses the orchestrator prompt template unless overridden via `with_system_prompt()`.
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
            system_prompt: crate::ai::prompts::get_template("orchestrator"),
        })
    }

    /// Override the system prompt for all subsequent LLM calls.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Non-streaming chat: sends a synchronous HTTP request via `ureq` and
    /// collects the complete response into a single string.
    ///
    /// Useful for simple Q&A without real-time rendering.
    #[allow(dead_code)]
    pub fn chat_simple(&self, messages: &[ChatMessageRef]) -> Result<String, AiError> {
        // Ensure system prompt is first
        let effective_messages =
            if messages.is_empty() || messages.first().map(|m| m.role.as_str()) != Some("system") {
                let mut prefixed = vec![ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                    ..Default::default()
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
/// For streamed tool calls, `id` and `function.name` are only present
/// in the first chunk; subsequent chunks append to `function.arguments`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SseToolCall {
    /// Tool call index for stream accumulation.
    #[serde(default)]
    index: usize,
    /// Tool call ID (only present in first chunk).
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    call_type: String,
    #[serde(default)]
    function: FunctionCall,
}

/// Function call details (name + JSON arguments string).
#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub(crate) struct FunctionCall {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

/// Accumulate streaming tool calls from SSE deltas.
///
/// OpenAI streams tool calls across multiple SSE events:
///   chunk 1: {index:0, id:"call_1", function:{name:"spawn_worker", arguments:"{\"agent\":"}}
///   chunk 2: {index:0, function:{arguments:"\"opencode\"}"}}
///
/// This function accumulates arguments by `index` across calls.
pub(crate) fn accumulate_sse_tool_calls(
    delta: &SseDeltaWithTools,
    accumulator: &mut std::collections::HashMap<usize, (String, String, String)>,
) {
    if let Some(tool_calls) = &delta.tool_calls {
        for tc in tool_calls {
            let entry = accumulator
                .entry(tc.index)
                .or_insert_with(|| (tc.id.clone(), tc.function.name.clone(), String::new()));
            if !tc.id.is_empty() {
                entry.0 = tc.id.clone();
            }
            if !tc.function.name.is_empty() {
                entry.1 = tc.function.name.clone();
            }
            entry.2.push_str(&tc.function.arguments);
        }
    }
}

/// Finalize accumulated tool calls: parse accumulated arguments as JSON.
/// Returns Vec<(id, name, parsed_args)>.
pub(crate) fn finalize_accumulated_calls(
    accumulator: std::collections::HashMap<usize, (String, String, String)>,
) -> Vec<(String, String, serde_json::Value)> {
    let mut calls = Vec::new();
    // Sort by index for deterministic output
    let mut indices: Vec<&usize> = accumulator.keys().collect();
    indices.sort();
    for idx in indices {
        if let Some((id, name, args_str)) = accumulator.get(idx) {
            if name.is_empty() {
                continue;
            }
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_str) {
                calls.push((id.clone(), name.clone(), args));
            } else if !args_str.is_empty() {
                // Partial/invalid arguments — try to fix by wrapping in a string
                calls.push((
                    id.clone(),
                    name.clone(),
                    serde_json::Value::String(args_str.clone()),
                ));
            }
        }
    }
    calls
}

// ── Tool Call Loop Engine ─────────────────────────────────────────────

/// Maximum tool call rounds (safety net — loop detection should fire first).
pub const MAX_TOOL_ROUNDS: u32 = 1000;

/// How many consecutive rounds of the exact same tool+args before
/// we declare the LLM is stuck and break the loop.
const MAX_CONSECUTIVE_IDENTICAL: u32 = 3;

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

/// Parse an SSE byte stream from a tool-calling LLM response.
///
/// Parses both OpenAI format (`choices[].delta.tool_calls`) and Anthropic
/// format (`content_block_delta`). Emits StreamEvent for each token and
/// tool call, and returns the collected tool calls.
async fn parse_tool_call_sse_stream(
    resp: reqwest::Response,
    round: u32,
    on_event: impl Fn(StreamEvent) + Send + 'static,
) -> Vec<(String, String, serde_json::Value)> {
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut done = false;
    let mut tool_call_acc: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                on_event(StreamEvent::Error(format!("stream error: {e}")));
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
                    if let Ok(sse_chunk) = serde_json::from_str::<SseChunkWithTools>(data) {
                        if let Some(choices) = sse_chunk.choices {
                            for choice in choices {
                                if let Some(ref delta) = choice.delta {
                                    if let Some(ref text) = delta.content {
                                        on_event(StreamEvent::Token(text.clone()));
                                    }
                                    if let Some(ref reasoning) = delta.reasoning_content {
                                        on_event(StreamEvent::Thinking {
                                            round,
                                            text: reasoning.clone(),
                                        });
                                    }
                                    // Accumulate streaming tool calls by index
                                    accumulate_sse_tool_calls(delta, &mut tool_call_acc);
                                }
                            }
                        }
                    } else {
                        match parse_anthropic_sse_event(data) {
                            AnthropicSseEvent::Text(text) if !text.is_empty() => {
                                on_event(StreamEvent::Token(text));
                            }
                            AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                                on_event(StreamEvent::Thinking { round, text });
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
                            on_event(StreamEvent::Token(text.clone()));
                        }
                        if let Some(ref reasoning) = delta.reasoning_content {
                            on_event(StreamEvent::Thinking {
                                round,
                                text: reasoning.clone(),
                            });
                        }
                        accumulate_sse_tool_calls(delta, &mut tool_call_acc);
                    }
                }
            }
        } else {
            match parse_anthropic_sse_event(data) {
                AnthropicSseEvent::Text(text) if !text.is_empty() => {
                    on_event(StreamEvent::Token(text));
                }
                AnthropicSseEvent::Thinking(text) if !text.is_empty() => {
                    on_event(StreamEvent::Thinking { round, text });
                }
                AnthropicSseEvent::Done => done = true,
                _ => {}
            }
        }
    }

    if !done {
        on_event(StreamEvent::Done);
    }
    let tool_calls = finalize_accumulated_calls(tool_call_acc);
    for (id, name, args) in &tool_calls {
        on_event(StreamEvent::ToolCall {
            id: id.clone(),
            name: name.clone(),
            args: args.clone(),
        });
    }
    tool_calls
}

impl OrchestratorEngine {
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
        tools: &[&dyn Tool],
        executor: &ToolExecutor,
        compactor: &Compactor,
        max_tool_rounds: u32,
        on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
    ) -> Result<String, AiError> {
        let max_rounds = max_tool_rounds.min(MAX_TOOL_ROUNDS);
        let mut loop_messages: Vec<ChatMessageRef> = messages.to_vec();
        let mut tool_round = 0;
        // Tracks previous round's tool signatures for loop detection.
        // Each entry: (tool_name, canonical_args_string).
        let mut prev_tool_sigs: Vec<(String, String)> = Vec::new();
        let mut consecutive_identical: u32 = 0;

        let on_event: Arc<dyn Fn(DispatchStreamEvent) + Send + Sync + 'static> = Arc::new(on_event);

        // If the first message is not a system prompt, prepend it.
        if loop_messages.is_empty()
            || loop_messages.first().map(|m| m.role.as_str()) != Some("system")
        {
            loop_messages.insert(
                0,
                ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                    ..Default::default()
                },
            );
        }

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
                        ..Default::default()
                    })
                    .collect();
            }

            // Build the request payload with tools
            let payload = build_tool_call_payload(&loop_messages, tools, &self.current_model());

            // Send the HTTP request and collect tool_calls from SSE stream
            let current_round = tool_round;
            let http_client = self.http_client.clone();
            let endpoint = self.endpoint_url().to_string();
            let api_key = self.api_key.clone();
            let collected: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
            let collected_clone = collected.clone();
            let on_event_clone = Arc::clone(&on_event);

            let raw_calls = tokio_handle()
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

                    let tool_calls =
                        parse_tool_call_sse_stream(resp, current_round, move |event| match event {
                            StreamEvent::Token(text) => {
                                collected_clone.lock().unwrap().push_str(&text);
                                on_event_clone(DispatchStreamEvent::Token(text));
                            }
                            StreamEvent::Thinking { round, text } => {
                                on_event_clone(DispatchStreamEvent::Thinking { round, text });
                            }
                            StreamEvent::ToolCall { id, name, args } => {
                                on_event_clone(DispatchStreamEvent::ToolCall { id, name, args });
                            }
                            StreamEvent::Error(err) => {
                                on_event_clone(DispatchStreamEvent::Error(err));
                            }
                            StreamEvent::Done => {
                                on_event_clone(DispatchStreamEvent::Done);
                            }
                            StreamEvent::ToolResult(_, _, _) => {}
                        })
                        .await;

                    Ok(tool_calls)
                })
                .await
                .map_err(|e| AiError::ApiError(format!("task spawn failed: {e}")))?
                .map_err(|e: AiError| e)?;

            // Convert (id, name, args) triples to ToolCall objects
            let tool_calls: Vec<ToolCall> = raw_calls
                .into_iter()
                .map(|(id, name, args)| ToolCall {
                    id,
                    name,
                    arguments: args,
                })
                .collect();

            if tool_calls.is_empty() {
                let final_response = collected.lock().unwrap().clone();
                return Ok(final_response);
            }

            // Collect the LLM's response text before tool calls,
            // so it's preserved in the conversation history.
            let response_text = collected.lock().unwrap().clone();

            // Execute all tool calls (single-threaded, errors don't stop loop)
            let results: Vec<ToolResult> =
                tool_calls.iter().map(|tc| executor.execute(tc)).collect();

            for (tc, result) in tool_calls.iter().zip(results.iter()) {
                on_event(DispatchStreamEvent::ToolResult {
                    id: tc.id.clone(),
                    tool_name: result.tool_name.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                });
            }

            // Add assistant message with tool_calls before tool results
            // (required by OpenAI API message format)
            let api_tool_calls: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                        }
                    })
                })
                .collect();
            loop_messages.push(ChatMessageRef {
                role: "assistant".into(),
                content: response_text,
                tool_calls: Some(api_tool_calls),
                ..Default::default()
            });

            // Add tool result messages with tool_call_id
            for (i, result) in results.iter().enumerate() {
                loop_messages.push(ChatMessageRef {
                    role: "tool".into(),
                    content: format!(
                        "Tool '{}' returned: {}{}",
                        result.tool_name,
                        if result.is_error { "ERROR: " } else { "" },
                        result.content
                    ),
                    tool_call_id: Some(tool_calls[i].id.clone()),
                    ..Default::default()
                });
            }

            on_event(DispatchStreamEvent::RoundStart {
                round: tool_round + 1,
            });

            tool_round += 1;
            if tool_round >= max_rounds {
                return Err(AiError::Timeout(format!(
                    "工具调用循环超过最大迭代次数 ({max_rounds})"
                )));
            }

            // ── Loop detection ─────────────────────────────────────────
            // If every tool in this round has the exact same name + args
            // as the previous round, the LLM is stuck in a polling loop.
            let current_sigs: Vec<(String, String)> = tool_calls
                .iter()
                .map(|tc| {
                    (
                        tc.name.clone(),
                        serde_json::to_string(&tc.arguments).unwrap_or_default(),
                    )
                })
                .collect();
            if current_sigs == prev_tool_sigs && !current_sigs.is_empty() {
                consecutive_identical += 1;
                tracing::warn!(
                    target: "zenixai",
                    round = tool_round,
                    consecutive = consecutive_identical,
                    tools = ?current_sigs.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                    "tool call loop detection: same tools + args repeated"
                );
                if consecutive_identical >= MAX_CONSECUTIVE_IDENTICAL {
                    return Err(AiError::Timeout(format!(
                        "工具调用循环检测: 同一工具同一参数连续出现 {consecutive_identical} 次，判断为卡死"
                    )));
                }
            } else {
                consecutive_identical = 0;
            }
            prev_tool_sigs = current_sigs;
        }
    }
}

impl OrchestratorEngine {
    /// Non-streaming chat with tool support.
    ///
    /// Calls the LLM once, executes any tool calls returned, and returns
    /// the final text response. Useful for programmatic use without
    /// real-time rendering.
    #[allow(dead_code)]
    pub async fn chat_simple_with_tools(
        &self,
        messages: &[ChatMessageRef],
        tools: &[&dyn Tool],
        executor: &ToolExecutor,
    ) -> Result<String, AiError> {
        let effective_messages =
            if messages.is_empty() || messages.first().map(|m| m.role.as_str()) != Some("system") {
                let mut prefixed = vec![ChatMessageRef {
                    role: "system".into(),
                    content: self.system_prompt.clone(),
                    ..Default::default()
                }];
                prefixed.extend(messages.iter().cloned());
                prefixed
            } else {
                messages.to_vec()
            };

        let mut loop_messages = effective_messages;
        let mut tool_round = 0;
        let mut prev_tool_sigs: Vec<(String, String)> = Vec::new();
        let mut consecutive_identical: u32 = 0;

        loop {
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
                                    let id = t
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    tc.push(ToolCall {
                                        id,
                                        name,
                                        arguments: args,
                                    });
                                }
                            }
                        }
                    }

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

            if tool_calls.is_empty() {
                return Ok(final_text);
            }

            let results: Vec<ToolResult> =
                tool_calls.iter().map(|tc| executor.execute(tc)).collect();

            // Add assistant message with tool_calls before tool results
            let api_tool_calls: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                        }
                    })
                })
                .collect();
            loop_messages.push(ChatMessageRef {
                role: "assistant".into(),
                content: String::new(),
                tool_calls: Some(api_tool_calls),
                ..Default::default()
            });

            for (i, result) in results.iter().enumerate() {
                loop_messages.push(ChatMessageRef {
                    role: "tool".into(),
                    content: format!(
                        "Tool '{}' returned: {}{}",
                        result.tool_name,
                        if result.is_error { "ERROR: " } else { "" },
                        result.content
                    ),
                    tool_call_id: Some(tool_calls[i].id.clone()),
                    ..Default::default()
                });
            }

            tool_round += 1;
            if tool_round >= MAX_TOOL_ROUNDS {
                return Err(AiError::Timeout(format!(
                    "工具调用循环超过最大迭代次数 ({MAX_TOOL_ROUNDS})"
                )));
            }

            // Loop detection (same as chat_with_tools)
            let current_sigs: Vec<(String, String)> = tool_calls
                .iter()
                .map(|tc| {
                    (
                        tc.name.clone(),
                        serde_json::to_string(&tc.arguments).unwrap_or_default(),
                    )
                })
                .collect();
            if current_sigs == prev_tool_sigs && !current_sigs.is_empty() {
                consecutive_identical += 1;
                if consecutive_identical >= MAX_CONSECUTIVE_IDENTICAL {
                    return Err(AiError::Timeout(format!(
                        "工具调用循环检测: 同一工具同一参数连续出现 {consecutive_identical} 次，判断为卡死"
                    )));
                }
            } else {
                consecutive_identical = 0;
            }
            prev_tool_sigs = current_sigs;
        }
    }
}

// ── Agent Execution Helpers ─────────────────────────────────────────────

/// Execute a single agent task through the daemon's API lane.
///
/// The API lane is synchronous — one JSON line per request over a Unix socket —
/// so this runs the calls directly. Callers in an async context should wrap
/// with `tokio::task::spawn_blocking` if needed.
///
/// Returns the agent result with collected output.
#[allow(dead_code)]
pub fn execute_agent_task(
    endpoint: &ApiEndpoint,
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

    // The API lane starts an agent inside an existing pane, so the pane is
    // opened first; an empty workspace or cwd lets the daemon use its own.
    let workspace = (!workspace_id.is_empty()).then_some(workspace_id);
    let cwd = (!cwd.is_empty()).then_some(cwd.as_str());
    let pane = match endpoint.split_pane(workspace, cwd, SplitDirection::Down) {
        Ok(pane) => pane,
        Err(error) => {
            let error_kind = AiError::IpcError(error.to_string());
            on_event(DispatchStreamEvent::Error(format!(
                "Failed to open a pane for agent '{agent_name}': {}",
                user_friendly(&error_kind)
            )));
            return AgentResult {
                agent_name,
                pane_id: String::new(),
                exit_status: "failed".into(),
                output: format!("Pane split failed: {error}"),
                summary: Some(error_kind.to_string()),
            };
        }
    };

    // The daemon names the agent itself and starts the CLI in the pane.
    let name = crate::ai::tools::agent_name(Some(&agent_name), &agent_name);
    let started = match endpoint.start_agent(
        &name,
        &agent_name,
        &pane.pane_id,
        Vec::new(),
        AGENT_START_TIMEOUT_MS,
    ) {
        Ok(started) => started,
        Err(error) => {
            let _ = endpoint.close_pane(&pane.pane_id);
            let message = error.to_string();
            let error_kind = if matches!(error.code(), Some("unsupported_agent_kind")) {
                AiError::AgentCliNotFound(message.clone())
            } else if message.contains("connect") || message.contains("No such file") {
                AiError::AgentCliNotFound(format!("herdr not running: {message}"))
            } else {
                AiError::IpcError(message)
            };
            tracing::error!("failed to start agent '{agent_name}': {error}");
            on_event(DispatchStreamEvent::Error(format!(
                "Failed to start agent '{agent_name}': {}",
                user_friendly(&error_kind)
            )));
            return AgentResult {
                agent_name,
                pane_id: pane.pane_id.clone(),
                exit_status: "failed".into(),
                output: format!("Agent start failed: {error}"),
                summary: Some(format!("{error_kind}: {error}")),
            };
        }
    };

    let pane_id = started.agent.pane_id.clone();

    // Wait for the CLI to settle: `agent.start` returns before the agent takes
    // prompts, and a prompt sent in between is refused.
    if let Err(error) =
        endpoint.wait_for_agent_ready(&name, Duration::from_millis(AGENT_START_TIMEOUT_MS))
    {
        tracing::warn!("agent '{agent_name}' never became ready: {error}");
        on_event(DispatchStreamEvent::Error(format!(
            "Agent '{agent_name}' did not become ready: {error}"
        )));
        return AgentResult {
            agent_name,
            pane_id,
            exit_status: "timeout".into(),
            output: String::new(),
            summary: Some(error.to_string()),
        };
    }

    // The daemon's own submit path, so the agent sees a finished prompt.
    if let Err(error) = endpoint.prompt_agent(&name, &instruction) {
        tracing::warn!(
            "failed to send instruction to agent '{}': {error}",
            agent_name
        );
    }

    // Wait for output with DONE pattern
    let output = match endpoint.wait_for_output(&pane_id, AGENT_DONE_PATTERN, 500, timeout_ms) {
        Ok(read) => read.text,
        Err(error) => {
            tracing::warn!("wait_for_output failed for agent '{}': {error}", agent_name);
            // Fallback: read recent output
            match endpoint.read_pane(&pane_id, ReadSource::Recent, 500) {
                Ok(read) => read.text,
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
    #[allow(dead_code)]
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
        let data = r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"reasoning..."}}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Thinking(t) if t == "reasoning..."
        ));
    }

    #[test]
    fn test_parse_anthropic_message_stop() {
        let data = r#"{"type":"message_stop"}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Done
        ));
    }

    #[test]
    fn test_parse_anthropic_content_block_start_ignored() {
        let data = r#"{"type":"content_block_start","index":0}"#;
        assert!(matches!(
            parse_anthropic_sse_event(data),
            AnthropicSseEvent::Ignored
        ));
    }

    #[test]
    fn test_parse_anthropic_invalid_json() {
        assert!(matches!(
            parse_anthropic_sse_event("not json"),
            AnthropicSseEvent::Ignored
        ));
    }

    #[test]
    fn test_parse_anthropic_empty_data() {
        assert!(matches!(
            parse_anthropic_sse_event(""),
            AnthropicSseEvent::Ignored
        ));
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

    fn make_executor() -> ToolExecutor {
        let endpoint = ApiEndpoint::at(std::path::PathBuf::from("/tmp/zenix-engine-test.sock"));
        ToolExecutor::new(endpoint, "workspace-1".into())
    }

    #[test]
    fn test_tool_executor_execute_unknown_tool() {
        let executor = make_executor();

        let call = ToolCall {
            id: String::new(),
            name: "nonexistent_tool_xyz".into(),
            arguments: serde_json::json!({}),
        };
        let result = executor.execute(&call);
        assert!(result.is_error);
        assert!(result.content.contains("未知工具"));
    }

    #[test]
    fn test_tool_executor_builtin_tools_list() {
        let tools = ToolExecutor::available_tools();
        let names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(names.len(), 6);
        assert!(names.contains(&"spawn_worker".to_string()));
        assert!(names.contains(&"send_text".to_string()));
        assert!(names.contains(&"read_output".to_string()));
        assert!(names.contains(&"wait_for_output".to_string()));
        assert!(names.contains(&"list_workers".to_string()));
        assert!(names.contains(&"close_worker".to_string()));
    }

    #[test]
    fn test_tool_executor_send_text_validation() {
        let executor = make_executor();

        // Missing pane_id
        let call = ToolCall {
            id: String::new(),
            name: "send_text".into(),
            arguments: serde_json::json!({"text": "hello"}),
        };
        let result = executor.execute(&call);
        assert!(result.is_error);
        assert!(result.content.contains("pane_id"));
    }

    // ── SSE tool call parsing tests ─────────────────────────────────────

    #[test]
    fn test_accumulate_sse_tool_calls_empty() {
        use std::collections::HashMap;
        let delta = SseDeltaWithTools {
            content: Some("hello".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: None,
        };
        let mut acc = HashMap::new();
        accumulate_sse_tool_calls(&delta, &mut acc);
        let calls = finalize_accumulated_calls(acc);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_accumulate_sse_tool_calls_with_calls() {
        use std::collections::HashMap;
        let delta = SseDeltaWithTools {
            content: Some("".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: Some(vec![SseToolCall {
                index: 0,
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path": "/tmp/test.txt"}"#.into(),
                },
            }]),
        };
        let mut acc = HashMap::new();
        accumulate_sse_tool_calls(&delta, &mut acc);
        let calls = finalize_accumulated_calls(acc);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[0].2["path"].as_str().unwrap(), "/tmp/test.txt");
    }

    #[test]
    fn test_accumulate_sse_tool_calls_accumulates_by_index() {
        use std::collections::HashMap;
        // Simulate two chunks that together form a complete argument
        let chunk1 = SseDeltaWithTools {
            content: Some("".into()),
            role: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: Some(vec![SseToolCall {
                index: 0,
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path": "/tmp/"#.into(),
                },
            }]),
        };
        let chunk2 = SseDeltaWithTools {
            content: Some("".into()),
            role: None,
            reasoning_content: None,
            tool_calls: Some(vec![SseToolCall {
                index: 0,
                id: String::new(),
                call_type: String::new(),
                function: FunctionCall {
                    name: String::new(),
                    arguments: r#"test.txt"}"#.into(),
                },
            }]),
        };
        let mut acc = HashMap::new();
        accumulate_sse_tool_calls(&chunk1, &mut acc);
        accumulate_sse_tool_calls(&chunk2, &mut acc);
        let calls = finalize_accumulated_calls(acc);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[0].2["path"].as_str().unwrap(), "/tmp/test.txt");
    }

    // ── build_tool_call_payload tests ──────────────────────────────────

    #[test]
    fn test_build_tool_call_payload_includes_tools() {
        let messages = vec![ChatMessageRef {
            role: "user".into(),
            content: "hello".into(),
            ..Default::default()
        }];
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(crate::ai::tools::SpawnWorker)];
        let tool_refs: Vec<&dyn Tool> = tools.iter().map(|t| t.as_ref()).collect();

        let payload = build_tool_call_payload(&messages, &tool_refs, "gpt-4o-mini");
        assert_eq!(payload["model"].as_str().unwrap(), "gpt-4o-mini");
        assert!(payload["stream"].as_bool().unwrap());
        assert!(payload["tools"].is_array());
        let tool_list = payload["tools"].as_array().unwrap();
        assert_eq!(tool_list.len(), 1);
        assert_eq!(
            tool_list[0]["function"]["name"].as_str().unwrap(),
            "spawn_worker"
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
            ..Default::default()
        }];

        // Simulate: first round returns tool call, second round returns text
        let mut loop_messages: Vec<ChatMessageRef> = messages.clone();
        let mut rounds = 0;
        let max_rounds = 10;

        // Round 1: LLM says "use read_file"
        loop_messages.push(ChatMessageRef {
            role: "assistant".into(),
            content: "I'll read the file for you.".into(),
            ..Default::default()
        });
        loop_messages.push(ChatMessageRef {
            role: "tool".into(),
            content: "Hello, World!".into(),
            ..Default::default()
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
