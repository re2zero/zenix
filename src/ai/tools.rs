//! Built-in tool system for the ZenixAI orchestrator.
//!
//! Tools are callable functions that the LLM can invoke via function calling.
//! Each tool implements the `Tool` trait with a name, description, JSON Schema
//! parameters, and an execute method.
//!
//! Current built-in tools:
//!   - File operations: `read_file`, `read_directory`, `search_files`
//!   - Agent management: `list_agents`, `start_agent`, `send_instruction`, `read_output`
//!   - Layout control: `split_pane`, `arrange_grid`
//!   - Memory management: `read_memory`, `write_memory`, `search_memory`

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Maximum file read size (50 KB).
const MAX_FILE_READ_BYTES: usize = 50 * 1024;

/// Maximum directory listing depth.
const MAX_DIR_DEPTH: usize = 3;

/// Maximum search results.
const DEFAULT_MAX_SEARCH_RESULTS: u64 = 50;

// ── Core Types ─────────────────────────────────────────────────────────

/// A tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name (e.g. "read_file").
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

/// Shared execution context passed to all tools.
///
/// Provides access to the herdr IPC client, workspace ID, and in-memory
/// storage for the memory management tools.
pub struct ToolContext {
    /// Optional herdr IPC client (None when running without herdr).
    pub herdr_client: Option<Arc<Mutex<crate::ai::herdr::HerdrIpcClient>>>,
    /// Workspace ID for agent operations.
    pub workspace_id: String,
    /// In-memory key/value store for memory tools (temporary until SQLite integration).
    pub memory: Arc<Mutex<HashMap<String, String>>>,
}

impl ToolContext {
    /// Create a new context without a herdr client.
    pub fn new(workspace_id: String) -> Self {
        Self {
            herdr_client: None,
            workspace_id,
            memory: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new context with a herdr client.
    pub fn with_herdr(
        workspace_id: String,
        herdr_client: crate::ai::herdr::HerdrIpcClient,
    ) -> Self {
        Self {
            herdr_client: Some(Arc::new(Mutex::new(herdr_client))),
            workspace_id,
            memory: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Trait that all tools implement.
///
/// Tools receive a context (for herdr client access, memory store) and
/// arguments, then produce a result.
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "read_file").
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does (in Chinese).
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's parameters.
    ///
    /// Format follows the OpenAI function calling schema:
    /// ```json
    /// {
    ///   "type": "object",
    ///   "properties": {
    ///     "path": {
    ///       "type": "string",
    ///       "description": "文件路径"
    ///     }
    ///   },
    ///   "required": ["path"]
    /// }
    /// ```
    fn parameters(&self) -> serde_json::Value;

    /// Execute the tool with the given context and arguments.
    ///
    /// Returns a `ToolResult` — successful or error result.
    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult;
}

// ═══════════════════════════════════════════════════════════════════════════
// File Operation Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Read a file's contents (up to 50 KB).
pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "读取文件内容。最大读取 50KB，超过时会截断并标注 \u{201C}TRUNCATED\u{201D}。文件不存在会返回错误提示。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "要读取的文件路径（绝对路径或相对于工作目录的路径）"
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, _ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error(
                    "read_file",
                    "参数 \u{201C}path\u{201D} 必须是字符串类型",
                )
            }
        };

        let path = std::path::Path::new(path_str);
        if !path.exists() {
            return ToolResult::error(
                "read_file",
                format!("文件不存在: {}", path_str),
            );
        }

        if !path.is_file() {
            return ToolResult::error(
                "read_file",
                format!("路径不是文件: {}", path_str),
            );
        }

        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() > MAX_FILE_READ_BYTES {
                    let truncated = String::from_utf8_lossy(&bytes[..MAX_FILE_READ_BYTES]);
                    let remaining = bytes.len() - MAX_FILE_READ_BYTES;
                    ToolResult::ok(
                        "read_file",
                        format!(
                            "{}\n\n[TRUNCATED: 内容已截断，剩余 {} 字节未显示]",
                            truncated, remaining
                        ),
                    )
                } else {
                    match String::from_utf8(bytes) {
                        Ok(text) => ToolResult::ok("read_file", text),
                        Err(e) => ToolResult::error(
                            "read_file",
                            format!("文件包含无效的 UTF-8 数据: {e}"),
                        ),
                    }
                }
            }
            Err(e) => ToolResult::error(
                "read_file",
                format!("读取文件失败: {e}"),
            ),
        }
    }
}

/// List directory contents recursively up to 3 levels deep.
pub struct ReadDirectory;

impl Tool for ReadDirectory {
    fn name(&self) -> &str {
        "read_directory"
    }

    fn description(&self) -> &str {
        "递归列出目录结构，最大深度 3 层，类似 tree 命令的输出格式。自动跳过隐藏文件（以 '.' 开头的文件或目录）。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "要列出的目录路径"
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, _ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error(
                    "read_directory",
                    "参数 \u{201C}path\u{201D} 必须是字符串类型",
                )
            }
        };

        let path = std::path::Path::new(path_str);
        if !path.exists() {
            return ToolResult::error(
                "read_directory",
                format!("目录不存在: {}", path_str),
            );
        }

        if !path.is_dir() {
            return ToolResult::error(
                "read_directory",
                format!("路径不是目录: {}", path_str),
            );
        }

        let mut output = String::new();
        list_dir_recursive(path, &mut output, "", 0, MAX_DIR_DEPTH);

        if output.is_empty() {
            return ToolResult::ok("read_directory", "(空目录)");
        }

        // Remove trailing newline
        let trimmed = output.trim_end_matches('\n').to_string();
        ToolResult::ok("read_directory", trimmed)
    }
}

/// Recursively list directory contents.
fn list_dir_recursive(
    dir: &std::path::Path,
    output: &mut String,
    prefix: &str,
    depth: usize,
    max_depth: usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            output.push_str(&format!("{prefix}\u{26A0} 无法读取: {e}\n"));
            return;
        }
    };

    let mut dirs: Vec<_> = Vec::new();
    let mut files: Vec<_> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files/directories
        if name.starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => dirs.push((name, entry.path())),
            _ => files.push((name, entry.path())),
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    for (i, (name, path)) in dirs.iter().enumerate() {
        let is_last = i == dirs.len() - 1 && files.is_empty();
        let connector = if is_last && !files.is_empty() {
            // Dirs come first, so use ├─
            "├─"
        } else if is_last {
            "└─"
        } else {
            "├─"
        };
        output.push_str(&format!("{prefix}{connector} {name}/\n"));

        if depth + 1 < max_depth {
            let child_prefix = format!("{prefix}{}  ", if is_last { "  " } else { "\u{2502} " });
            list_dir_recursive(path, output, &child_prefix, depth + 1, max_depth);
        } else {
            // At max depth, just show children without deeper recursion
            let child_prefix = format!("{prefix}{}  ", if is_last { "  " } else { "\u{2502} " });
            if let Ok(child_entries) = std::fs::read_dir(path) {
                let mut children: Vec<_> = child_entries
                    .flatten()
                    .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
                    .filter(|(n, _)| !n.starts_with('.'))
                    .collect();
                children.sort_by(|a, b| a.0.cmp(&b.0));
                for (j, (cname, cpath)) in children.iter().enumerate() {
                    let c_last = j == children.len() - 1;
                    let c_connector = if c_last { "└─" } else { "├─" };
                    let is_dir = std::fs::symlink_metadata(cpath)
                        .ok()
                        .map(|m| m.file_type().is_dir())
                        .unwrap_or(false);
                    if is_dir {
                        output.push_str(&format!("{child_prefix}{c_connector} {cname}/\n"));
                    } else {
                        output.push_str(&format!("{child_prefix}{c_connector} {cname}\n"));
                    }
                }
            }
        }
    }

    for (i, (name, path)) in files.iter().enumerate() {
        let is_last = i == files.len() - 1;
        let connector = if is_last { "└─" } else { "├─" };
        let is_dir = std::fs::symlink_metadata(path)
            .ok()
            .map(|m| m.file_type().is_dir())
            .unwrap_or(false);
        if is_dir {
            output.push_str(&format!("{prefix}{connector} {name}/\n"));
        } else {
            if let Ok(meta) = std::fs::metadata(path) {
                let size = meta.len();
                output.push_str(&format!(
                    "{prefix}{connector} {name} ({size} bytes)\n"
                ));
            } else {
                output.push_str(&format!("{prefix}{connector} {name}\n"));
            }
        }
    }
}

/// Search files matching a glob pattern (simple suffix matching, no external deps).
pub struct SearchFiles;

impl Tool for SearchFiles {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "递归搜索文件中匹配指定 glob 模式的文件。支持通配符 *（匹配任意字符）和 **（匹配任意路径）。默认最多返回 50 个结果。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "搜索起始目录路径"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob 模式，例如 \"*.rs\"、\"**/*.toml\"、\"src/**/*\""
                },
                "max_results": {
                    "type": "integer",
                    "description": "最大返回结果数量，默认 50",
                    "default": 50
                }
            },
            "required": ["path", "pattern"]
        })
    }

    fn execute(&self, _ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error(
                    "search_files",
                    "参数 \u{201C}path\u{201D} 必须是字符串类型",
                )
            }
        };

        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error(
                    "search_files",
                    "参数 \u{201C}pattern\u{201D} 必须是字符串类型",
                )
            }
        };

        let max_results: u64 = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_SEARCH_RESULTS);

        let search_path = std::path::Path::new(path_str);
        if !search_path.exists() {
            return ToolResult::error(
                "search_files",
                format!("搜索路径不存在: {}", path_str),
            );
        }
        if !search_path.is_dir() {
            return ToolResult::error(
                "search_files",
                format!("路径不是目录: {}", path_str),
            );
        }

        let mut results: Vec<String> = Vec::new();
        search_files_recursive(search_path, pattern, &mut results, &mut 0, max_results as usize);

        if results.is_empty() {
            ToolResult::ok(
                "search_files",
                format!("未找到匹配 \u{201C}{}\u{201D} 的文件", pattern),
            )
        } else {
            ToolResult::ok(
                "search_files",
                format!("找到 {} 个匹配文件:\n\n{}", results.len(), results.join("\n")),
            )
        }
    }
}

/// Recursively search for files matching a glob pattern.
fn search_files_recursive(
    dir: &std::path::Path,
    pattern: &str,
    results: &mut Vec<String>,
    count: &mut usize,
    max_results: usize,
) {
    if *count >= max_results {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if *count >= max_results {
            return;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files
        if name.starts_with('.') {
            continue;
        }

        let full_path = entry.path();
        let file_name = full_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Check if the file name matches the pattern
        if matches_glob(&file_name, pattern) {
            results.push(full_path.to_string_lossy().to_string());
            *count += 1;
        }

        // Recurse into subdirectories
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir()
                && (pattern == "**" || pattern == "**/*")
            {
                results.push(full_path.to_string_lossy().to_string());
                *count += 1;
            }
            if ft.is_dir() {
                search_files_recursive(&full_path, pattern, results, count, max_results);
            }
        }
    }
}

/// Simple glob matching supporting `*` (any chars in one segment) and `**` (any path).
///
/// This is a simplified glob implementation without external dependencies.
/// Supports: `*` matches within a single path segment, `**` matches across segments.
fn matches_glob(file_name: &str, pattern: &str) -> bool {
    // Handle ** patterns (match across path separators)
    if pattern.contains("**") {
        // Split pattern on **
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim_end_matches('/');
            let suffix = parts[1].trim_start_matches('/');

            if prefix.is_empty() && suffix.is_empty() {
                return true;
            }

            if prefix.is_empty() {
                // Pattern: **/something or **/something/**
                if suffix.is_empty() {
                    return true;
                }
                // Match suffix against the file name.
                // Since search_files_recursive already recurses into all
                // subdirectories, we only need to match the filename itself.
                return matches_glob(file_name, suffix);
            }

            if suffix.is_empty() {
                // Pattern: something/**
                return file_name.starts_with(prefix);
            }

            // Pattern: something/**/something_else
            return file_name.starts_with(prefix)
                && matches_glob(
                    file_name.strip_prefix(prefix).unwrap_or(""),
                    suffix,
                );
        }
    }

    // Handle simple patterns without **
    glob_match(file_name, pattern)
}

/// Match a single-segment glob pattern against a file name.
/// Supports `*` (matches any characters) and `?` (matches single char).
fn glob_match(s: &str, pattern: &str) -> bool {
    let s_bytes = s.as_bytes();
    let pat_bytes = pattern.as_bytes();

    // Build DP table
    let sn = s_bytes.len();
    let pn = pat_bytes.len();
    let mut dp = vec![vec![false; pn + 1]; sn + 1];
    dp[0][0] = true;

    // Handle leading * patterns
    for j in 1..=pn {
        if pat_bytes[j - 1] == b'*' {
            dp[0][j] = dp[0][j - 1];
        }
    }

    for i in 1..=sn {
        for j in 1..=pn {
            if pat_bytes[j - 1] == b'*' {
                // * matches zero chars or one more char
                dp[i][j] = dp[i][j - 1] || dp[i - 1][j];
            } else if pat_bytes[j - 1] == b'?' || s_bytes[i - 1] == pat_bytes[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[sn][pn]
}

// ═══════════════════════════════════════════════════════════════════════════
// Agent Management Tools (Stubs)
// ═══════════════════════════════════════════════════════════════════════════

/// List active agents via herdr IPC. (TODO: implement with HerdrIpcClient)
pub struct ListAgents;

impl Tool for ListAgents {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "查看当前 herdr 中所有 Agent 的运行状态，包括 Agent 名称、所属工作区和当前状态。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn execute(&self, ctx: &ToolContext, _args: serde_json::Value) -> ToolResult {
        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "list_agents",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };
        let mut client = client.lock().unwrap();
        match client.list_agents() {
            Ok(agents) => {
                if agents.is_empty() {
                    return ToolResult::ok("list_agents", "当前没有运行中的 Agent。");
                }
                let mut lines = Vec::new();
                for (i, agent) in agents.iter().enumerate() {
                    let name = agent.name.as_deref().unwrap_or("(unnamed)");
                    let status = agent.agent_status.as_str();
                    let pane = &agent.pane_id;
                    let workspace = &agent.workspace_id;
                    lines.push(format!(
                        "{}. {} | status={} | pane={} | workspace={}",
                        i + 1, name, status, pane, workspace
                    ));
                }
                ToolResult::ok("list_agents", format!("找到 {} 个 Agent:\n{}", agents.len(), lines.join("\n")))
            }
            Err(e) => ToolResult::error("list_agents", format!("获取 Agent 列表失败: {e}")),
        }
    }
}

/// Start an Agent CLI in a herdr pane.
pub struct StartAgent;

impl Tool for StartAgent {
    fn name(&self) -> &str {
        "start_agent"
    }

    fn description(&self) -> &str {
        "在 herdr 中启动一个 Agent CLI（如 claude、codex、opencode 等），返回分配的 pane ID。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Agent CLI 名称，例如 \"claude\"、\"codex\"、\"opencode\""
                },
                "workspace_id": {
                    "type": "string",
                    "description": "herdr 工作区 ID，Agent 将在该工作区中运行"
                },
                "cwd": {
                    "type": "string",
                    "description": "Agent 的工作目录（可选，默认为工作区根目录）"
                }
            },
            "required": ["agent", "workspace_id"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let agent_name = match args.get("agent").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("start_agent", "参数 agent 是必需的字符串");
            }
        };
        let workspace_id = match args.get("workspace_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("start_agent", "参数 workspace_id 是必需的字符串");
            }
        };
        let cwd = args.get("cwd").and_then(|v| v.as_str());

        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "start_agent",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };
        let mut client = client.lock().unwrap();
        match client.start_agent(agent_name, Some(workspace_id), cwd, vec![]) {
            Ok(agent_info) => {
                ToolResult::ok("start_agent", format!(
                    "Agent '{}' 已启动：terminal_id={}, pane_id={}, workspace_id={}",
                    agent_name, agent_info.terminal_id, agent_info.pane_id, agent_info.workspace_id
                ))
            }
            Err(e) => ToolResult::error("start_agent", format!("启动 Agent '{}' 失败: {e}", agent_name)),
        }
    }
}

/// Send a text instruction to an agent pane.
pub struct SendInstruction;

impl Tool for SendInstruction {
    fn name(&self) -> &str {
        "send_instruction"
    }

    fn description(&self) -> &str {
        "向指定 Agent 的 pane 发送文本指令。Agent CLI 会接收并执行该指令。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "目标 pane 的 ID"
                },
                "text": {
                    "type": "string",
                    "description": "要发送的指令文本"
                }
            },
            "required": ["pane_id", "text"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("send_instruction", "参数 pane_id 是必需的字符串");
            }
        };
        let text = match args.get("text").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("send_instruction", "参数 text 是必需的字符串");
            }
        };

        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "send_instruction",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };
        let mut client = client.lock().unwrap();
        match client.send_pane_text(pane_id, text) {
            Ok(()) => ToolResult::ok("send_instruction", format!("指令已发送给 pane {pane_id}")),
            Err(e) => ToolResult::error("send_instruction", format!("发送指令失败: {e}")),
        }
    }
}

/// Read the recent output from an agent pane.
pub struct ReadOutput;

impl Tool for ReadOutput {
    fn name(&self) -> &str {
        "read_output"
    }

    fn description(&self) -> &str {
        "读取指定 Agent pane 的最新输出内容，可用于检查 Agent 的执行结果。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "目标 pane 的 ID"
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

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("read_output", "参数 pane_id 是必需的字符串");
            }
        };
        let lines: u32 = args.get("lines").and_then(|v| v.as_u64()).map(|n| n as u32).unwrap_or(100);

        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "read_output",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };
        let mut client = client.lock().unwrap();
        match client.read_pane(pane_id, crate::ai::herdr::ReadSource::Recent, lines) {
            Ok(text) => {
                let truncated = if text.len() > 4000 {
                    format!("{}...[已截断，共 {} 字符]", &text[..4000], text.len())
                } else {
                    text
                };
                ToolResult::ok("read_output", truncated)
            }
            Err(e) => ToolResult::error("read_output", format!("读取 pane {pane_id} 失败: {e}")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Layout Control Tools (Stubs)
// ═══════════════════════════════════════════════════════════════════════════

/// Split a herdr pane. (TODO: implement with HerdrIpcClient)
pub struct SplitPane;

impl Tool for SplitPane {
    fn name(&self) -> &str {
        "split_pane"
    }

    fn description(&self) -> &str {
        "在 herdr 中分割窗格，创建一个新的子 pane。可用于并排查看多个 Agent 的输出。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pane_id": {
                    "type": "string",
                    "description": "要分割的源 pane ID"
                },
                "direction": {
                    "type": "string",
                    "description": "分割方向：\"horizontal\"（水平）或 \"vertical\"（垂直）",
                    "enum": ["horizontal", "vertical"]
                }
            },
            "required": ["pane_id", "direction"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let pane_id = match args.get("pane_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("split_pane", "参数 pane_id 是必需的字符串");
            }
        };
        let direction = match args.get("direction").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("split_pane", "参数 direction 是必需的字符串");
            }
        };

        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "split_pane",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };

        let _pane_id = pane_id;
        let split_dir = match direction {
            "horizontal" | "right" => crate::ai::herdr::SplitDirection::Right,
            "vertical" | "down" => crate::ai::herdr::SplitDirection::Down,
            _ => {
                return ToolResult::error("split_pane", format!("无效的方向: {direction}，应为 \"horizontal\"/\"vertical\"/\"right\"/\"down\""));
            }
        };

        let mut client = client.lock().unwrap();
        // split_pane takes workspace_id, direction, and cwd
        let workspace_id = if ctx.workspace_id.is_empty() {
            None
        } else {
            Some(ctx.workspace_id.as_str())
        };
        match client.split_pane(workspace_id, split_dir, None) {
            Ok(pane) => {
                ToolResult::ok("split_pane", format!(
                    "窗格已分割：新 pane_id={}, terminal_id={}, direction={}",
                    pane.pane_id, pane.terminal_id, direction
                ))
            }
            Err(e) => ToolResult::error("split_pane", format!("分割窗格失败: {e}")),
        }
    }
}

/// Arrange panes in a grid layout.
pub struct ArrangeGrid;

impl Tool for ArrangeGrid {
    fn name(&self) -> &str {
        "arrange_grid"
    }

    fn description(&self) -> &str {
        "将多个 pane 按网格布局排列。支持指定行数和列数，自动分配可用 pane。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tab_id": {
                    "type": "string",
                    "description": "目标标签页 ID"
                },
                "rows": {
                    "type": "integer",
                    "description": "网格行数，默认 1",
                    "default": 1
                },
                "cols": {
                    "type": "integer",
                    "description": "网格列数，默认 1",
                    "default": 1
                }
            },
            "required": ["tab_id"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let tab_id = match args.get("tab_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("arrange_grid", "参数 tab_id 是必需的字符串");
            }
        };
        let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

        if rows == 0 || cols == 0 {
            return ToolResult::error("arrange_grid", "行数和列数必须大于 0");
        }

        let client = match &ctx.herdr_client {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "arrange_grid",
                    "herdr IPC 客户端未连接。请先启动 herdr 并连接。",
                )
            }
        };

        let target_count = rows * cols;
        let mut client = client.lock().unwrap();

        // List current panes in the tab
        let panes = match client.list_panes(None) {
            Ok(p) => p,
            Err(e) => return ToolResult::error("arrange_grid", format!("获取窗格列表失败: {e}")),
        };

        let tab_panes: Vec<_> = panes.iter().filter(|p| p.tab_id == tab_id).collect();
        let current_count = tab_panes.len();

        if current_count >= target_count {
            // Already enough panes — just report
            return ToolResult::ok("arrange_grid", format!(
                "标签页 {} 当前有 {} 个窗格，目标 {}×{} = {} 个。窗格数量已足够。",
                tab_id, current_count, rows, cols, target_count
            ));
        }

        // Need to create more panes by splitting
        let workspace_id = if ctx.workspace_id.is_empty() {
            None
        } else {
            Some(ctx.workspace_id.as_str())
        };

        // Get the first pane in this tab (to verify it exists)
        let _root_pane = match tab_panes.first() {
            Some(p) => p.pane_id.clone(),
            None => {
                return ToolResult::error("arrange_grid", format!("标签页 {} 中没有窗格", tab_id));
            }
        };

        let mut created = 0;
        let mut direction = crate::ai::herdr::SplitDirection::Right;

        while created < target_count.saturating_sub(1) {
            match client.split_pane(workspace_id, direction, None) {
                Ok(pane) => {
                    let _new_pane = &pane.pane_id;
                    created += 1;
                    // Alternate direction for grid layout
                    if direction == crate::ai::herdr::SplitDirection::Right {
                        direction = crate::ai::herdr::SplitDirection::Down;
                    } else {
                        direction = crate::ai::herdr::SplitDirection::Right;
                    }
                }
                Err(e) => {
                    return ToolResult::error("arrange_grid", format!("创建窗格失败 (第 {} 个): {e}", created + 1));
                }
            }
        }

        ToolResult::ok("arrange_grid", format!(
            "已将标签页 {} 的窗格重排为 {}×{} 网格（共 {} 个窗格），新创建了 {} 个窗格。",
            tab_id, rows, cols, current_count + created, created
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Memory Management Tools (Stubs)
// ═══════════════════════════════════════════════════════════════════════════

/// Read a memory entry from the session context. (TODO: implement with session.rs)
pub struct ReadMemory;

impl Tool for ReadMemory {
    fn name(&self) -> &str {
        "read_memory"
    }

    fn description(&self) -> &str {
        "从当前会话的记忆中读取指定条目的内容。记忆用于保存重要的上下文信息、决策和约定。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "记忆条目的键名"
                }
            },
            "required": ["key"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let key = match args.get("key").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("read_memory", "参数 \"key\" 是必需的非空字符串");
            }
        };

        let memory = ctx.memory.lock().unwrap();
        match memory.get(key) {
            Some(value) => ToolResult::ok("read_memory", value.clone()),
            None => ToolResult::ok("read_memory", format!("记忆键 \"{}\" 不存在", key)),
        }
    }
}

/// Write a memory entry to the session context.
pub struct WriteMemory;

impl Tool for WriteMemory {
    fn name(&self) -> &str {
        "write_memory"
    }

    fn description(&self) -> &str {
        "向当前会话的记忆中写入一条条目。用于保存重要的上下文信息、决策和约定，方便后续 Agent 参考。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "记忆条目的键名"
                },
                "value": {
                    "type": "string",
                    "description": "记忆条目的内容"
                }
            },
            "required": ["key", "value"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let key = match args.get("key").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("write_memory", "参数 \"key\" 是必需的非空字符串");
            }
        };
        let value = match args.get("value").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult::error("write_memory", "参数 \"value\" 是必需的字符串");
            }
        };

        let mut memory = ctx.memory.lock().unwrap();
        let existed = memory.contains_key(key);
        memory.insert(key.to_string(), value.to_string());
        if existed {
            ToolResult::ok("write_memory", format!("记忆 \"{}\" 已更新", key))
        } else {
            ToolResult::ok("write_memory", format!("记忆 \"{}\" 已保存", key))
        }
    }
}

/// Search memory entries via full-text search.
pub struct SearchMemory;

impl Tool for SearchMemory {
    fn name(&self) -> &str {
        "search_memory"
    }

    fn description(&self) -> &str {
        "对会话记忆进行全文搜索，返回匹配的记忆条目列表。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "搜索关键词"
                },
                "limit": {
                    "type": "integer",
                    "description": "最大返回结果数，默认 10",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: serde_json::Value) -> ToolResult {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return ToolResult::error("search_memory", "参数 \"query\" 是必需的非空字符串");
            }
        };
        let limit: usize = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(10);

        let memory = ctx.memory.lock().unwrap();
        let query_lower = query.to_lowercase();
        let matches: Vec<(&String, &String)> = memory
            .iter()
            .filter(|(_, v)| v.to_lowercase().contains(&query_lower))
            .collect();

        let results: Vec<String> = matches
            .into_iter()
            .take(limit)
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();

        if results.is_empty() {
            ToolResult::ok("search_memory", format!("未找到匹配 \"{}\" 的记忆", query))
        } else {
            ToolResult::ok("search_memory", format!("找到 {} 条匹配记忆:\n\n{}", results.len(), results.join("\n")))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool Registry
// ═══════════════════════════════════════════════════════════════════════════

/// Return all built-in tools registered with the orchestrator.
///
/// Tools are returned in execution-priority order: file ops first, then
/// agent management, layout control, and memory management.
pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        // File operations
        Box::new(ReadFile),
        Box::new(ReadDirectory),
        Box::new(SearchFiles),
        // Agent management
        Box::new(ListAgents),
        Box::new(StartAgent),
        Box::new(SendInstruction),
        Box::new(ReadOutput),
        // Layout control
        Box::new(SplitPane),
        Box::new(ArrangeGrid),
        // Memory management
        Box::new(ReadMemory),
        Box::new(WriteMemory),
        Box::new(SearchMemory),
    ]
}

/// Find a tool by name.
pub fn find_tool(name: &str) -> Option<Box<dyn Tool>> {
    builtin_tools().into_iter().find(|t| t.name() == name)
}

/// Execute a tool call by name with the given context and arguments.
pub fn execute_tool_call_with_ctx(call: &ToolCall, ctx: &ToolContext) -> ToolResult {
    match find_tool(&call.name) {
        Some(tool) => tool.execute(ctx, call.arguments.clone()),
        None => ToolResult::error(
            "tool_dispatch",
            format!("未知工具: {}", call.name),
        ),
    }
}

/// Execute a tool call by name with the given arguments (no context).
///
/// Uses a default context with no herdr client. Suitable for file-only tools.
pub fn execute_tool_call(call: &ToolCall) -> ToolResult {
    let ctx = ToolContext::new(String::new());
    execute_tool_call_with_ctx(call, &ctx)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: get all built-in tool names.
    fn tool_names() -> Vec<String> {
        builtin_tools().iter().map(|t| t.name().to_string()).collect()
    }

    /// Default context for file-only tool tests (no herdr client).
    fn default_ctx() -> ToolContext {
        ToolContext::new("test-workspace".into())
    }

    // ── read_file tests ───────────────────────────────────────────────

    #[test]
    fn test_read_file_exists() {
        let dir = std::env::temp_dir().join("zenix_ai_tool_test");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_read.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        let result = ReadFile.execute(&default_ctx(), serde_json::json!({ "path": file_path.to_str().unwrap() }));
        assert!(!result.is_error);
        assert_eq!(result.content, "Hello, World!");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_not_exists() {
        let result = ReadFile.execute(&default_ctx(), serde_json::json!({ "path": "/nonexistent/file.txt" }));
        assert!(result.is_error);
        assert!(result.content.contains("不存在"));
    }

    #[test]
    fn test_read_file_truncated() {
        let dir = std::env::temp_dir().join("zenix_ai_tool_test_trunc");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("big_file.txt");
        let content = "x".repeat(MAX_FILE_READ_BYTES + 1000);
        std::fs::write(&file_path, &content).unwrap();

        let result = ReadFile.execute(&default_ctx(), serde_json::json!({ "path": file_path.to_str().unwrap() }));
        assert!(!result.is_error);
        assert!(result.content.contains("[TRUNCATED"));
        assert!(result.content.starts_with("x"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── read_directory tests ──────────────────────────────────────────

    #[test]
    fn test_read_directory_list() {
        let dir = std::env::temp_dir().join("zenix_ai_tool_test_dir");
        let _ = std::fs::create_dir_all(&dir);
        let sub = dir.join("subdir");
        let _ = std::fs::create_dir_all(&sub);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.rs"), "b").unwrap();
        std::fs::write(sub.join("c.toml"), "c").unwrap();

        let result = ReadDirectory.execute(&default_ctx(), serde_json::json!({ "path": dir.to_str().unwrap() }));
        assert!(!result.is_error);
        assert!(result.content.contains("a.txt"));
        assert!(result.content.contains("b.rs"));
        assert!(result.content.contains("subdir"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_directory_not_exists() {
        let result = ReadDirectory.execute(&default_ctx(), serde_json::json!({ "path": "/nonexistent/dir" }));
        assert!(result.is_error);
        assert!(result.content.contains("不存在"));
    }

    // ── search_files tests ────────────────────────────────────────────

    #[test]
    fn test_search_files_glob_match() {
        let dir = std::env::temp_dir().join("zenix_ai_tool_test_search");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("main.rs"), "").unwrap();
        std::fs::write(dir.join("lib.rs"), "").unwrap();
        std::fs::write(dir.join("config.toml"), "").unwrap();
        std::fs::write(dir.join("README.md"), "").unwrap();

        let result = SearchFiles.execute(&default_ctx(), serde_json::json!({
            "path": dir.to_str().unwrap(),
            "pattern": "*.rs",
            "max_results": 50
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("main.rs"));
        assert!(result.content.contains("lib.rs"));
        assert!(!result.content.contains("config.toml"));
        assert!(!result.content.contains("README.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_search_files_max_results_limit() {
        let dir = std::env::temp_dir().join("zenix_ai_tool_test_limit");
        let _ = std::fs::create_dir_all(&dir);
        for i in 0..10 {
            std::fs::write(dir.join(format!("file_{i}.rs")), "").unwrap();
        }

        let result = SearchFiles.execute(&default_ctx(), serde_json::json!({
            "path": dir.to_str().unwrap(),
            "pattern": "*.rs",
            "max_results": 3
        }));
        assert!(!result.is_error);
        let count = result.content.matches("file_").count();
        assert_eq!(count, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Agent Management Tool Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_list_agents_no_herdr() {
        // Without herdr client, should return error
        let result = ListAgents.execute(&default_ctx(), serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("herdr IPC"));
    }

    #[test]
    fn test_list_agents_param_schema() {
        let params = ListAgents.parameters();
        assert_eq!(params["type"].as_str().unwrap(), "object");
        assert_eq!(params["required"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_start_agent_param_validation() {
        // Missing agent field
        let result = StartAgent.execute(&default_ctx(), serde_json::json!({ "workspace_id": "w1" }));
        assert!(result.is_error);
        assert!(result.content.contains("agent"));

        // Missing workspace_id
        let result = StartAgent.execute(&default_ctx(), serde_json::json!({ "agent": "claude" }));
        assert!(result.is_error);
        assert!(result.content.contains("workspace_id"));
    }

    #[test]
    fn test_start_agent_param_schema() {
        let params = StartAgent.parameters();
        assert_eq!(params["type"].as_str().unwrap(), "object");
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"agent".to_string()));
        assert!(required.contains(&"workspace_id".to_string()));
    }

    #[test]
    fn test_send_instruction_param_validation() {
        let result = SendInstruction.execute(&default_ctx(), serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("pane_id"));

        let result = SendInstruction.execute(&default_ctx(), serde_json::json!({ "pane_id": "p-1" }));
        assert!(result.is_error);
        assert!(result.content.contains("text"));
    }

    #[test]
    fn test_read_output_param_validation() {
        let result = ReadOutput.execute(&default_ctx(), serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("pane_id"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Layout Control Tool Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_split_pane_param_validation() {
        let result = SplitPane.execute(&default_ctx(), serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("pane_id"));

        let result = SplitPane.execute(&default_ctx(), serde_json::json!({ "pane_id": "p-1" }));
        assert!(result.is_error);
        assert!(result.content.contains("direction"));
    }

    #[test]
    fn test_split_pane_param_schema() {
        let params = SplitPane.parameters();
        assert_eq!(params["type"].as_str().unwrap(), "object");
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"pane_id".to_string()));
        assert!(required.contains(&"direction".to_string()));
    }

    #[test]
    fn test_arrange_grid_param_validation() {
        // Missing tab_id
        let result = ArrangeGrid.execute(&default_ctx(), serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("tab_id"));
    }

    #[test]
    fn test_arrange_grid_param_schema() {
        let params = ArrangeGrid.parameters();
        assert_eq!(params["type"].as_str().unwrap(), "object");
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"tab_id".to_string()));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Memory Management Tool Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_read_memory() {
        let ctx = ToolContext::new("ws".into());

        // Write a memory entry
        let result = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "architecture",
            "value": "Use microservices"
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("已保存"));

        // Read it back
        let result = ReadMemory.execute(&ctx, serde_json::json!({
            "key": "architecture"
        }));
        assert!(!result.is_error);
        assert_eq!(result.content, "Use microservices");
    }

    #[test]
    fn test_write_memory_update_existing() {
        let ctx = ToolContext::new("ws".into());

        let _ = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "config",
            "value": "v1"
        }));

        let result = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "config",
            "value": "v2"
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("已更新"));

        let result = ReadMemory.execute(&ctx, serde_json::json!({
            "key": "config"
        }));
        assert_eq!(result.content, "v2");
    }

    #[test]
    fn test_read_memory_not_found() {
        let ctx = ToolContext::new("ws".into());
        let result = ReadMemory.execute(&ctx, serde_json::json!({
            "key": "nonexistent"
        }));
        assert!(!result.is_error); // returns OK with "not found" message
        assert!(result.content.contains("不存在"));
    }

    #[test]
    fn test_read_memory_param_validation() {
        let ctx = ToolContext::new("ws".into());
        let result = ReadMemory.execute(&ctx, serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("key"));
    }

    #[test]
    fn test_write_memory_param_validation() {
        let ctx = ToolContext::new("ws".into());
        let result = WriteMemory.execute(&ctx, serde_json::json!({"key": "k"}));
        assert!(result.is_error);
        assert!(result.content.contains("value"));

        let result = WriteMemory.execute(&ctx, serde_json::json!({"value": "v"}));
        assert!(result.is_error);
        assert!(result.content.contains("key"));
    }

    #[test]
    fn test_search_memory() {
        let ctx = ToolContext::new("ws".into());

        let _ = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "arch",
            "value": "microservices architecture"
        }));
        let _ = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "db",
            "value": "use PostgreSQL"
        }));
        let _ = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "auth",
            "value": "JWT token based auth"
        }));

        let result = SearchMemory.execute(&ctx, serde_json::json!({
            "query": "architecture"
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("arch"));
        assert!(result.content.contains("microservices"));
    }

    #[test]
    fn test_search_memory_case_insensitive() {
        let ctx = ToolContext::new("ws".into());

        let _ = WriteMemory.execute(&ctx, serde_json::json!({
            "key": "k1",
            "value": "Hello World"
        }));

        let result = SearchMemory.execute(&ctx, serde_json::json!({
            "query": "hello"
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("k1"));
    }

    #[test]
    fn test_search_memory_no_results() {
        let ctx = ToolContext::new("ws".into());

        let result = SearchMemory.execute(&ctx, serde_json::json!({
            "query": "nonexistent_key_value"
        }));
        assert!(!result.is_error);
        assert!(result.content.contains("未找到"));
    }

    #[test]
    fn test_search_memory_limit() {
        let ctx = ToolContext::new("ws".into());

        for i in 0..5 {
            let _ = WriteMemory.execute(&ctx, serde_json::json!({
                "key": format!("item_{i}"),
                "value": format!("shared keyword {}", i)
            }));
        }

        let result = SearchMemory.execute(&ctx, serde_json::json!({
            "query": "shared",
            "limit": 2
        }));
        assert!(!result.is_error);
        // Should contain at most 2 results
        let count = result.content.matches("item_").count();
        assert!(count <= 2);
    }

    #[test]
    fn test_search_memory_param_validation() {
        let ctx = ToolContext::new("ws".into());
        let result = SearchMemory.execute(&ctx, serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.content.contains("query"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Tool Registry Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_builtin_tools_count() {
        let tools = builtin_tools();
        assert_eq!(tools.len(), 12);
    }

    #[test]
    fn test_builtin_tools_names() {
        let names = tool_names();
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"read_directory".to_string()));
        assert!(names.contains(&"search_files".to_string()));
        assert!(names.contains(&"list_agents".to_string()));
        assert!(names.contains(&"start_agent".to_string()));
        assert!(names.contains(&"send_instruction".to_string()));
        assert!(names.contains(&"read_output".to_string()));
        assert!(names.contains(&"split_pane".to_string()));
        assert!(names.contains(&"arrange_grid".to_string()));
        assert!(names.contains(&"read_memory".to_string()));
        assert!(names.contains(&"write_memory".to_string()));
        assert!(names.contains(&"search_memory".to_string()));
    }

    #[test]
    fn test_find_tool_by_name() {
        assert!(find_tool("read_file").is_some());
        assert!(find_tool("read_directory").is_some());
        assert!(find_tool("search_files").is_some());
        assert!(find_tool("list_agents").is_some());
        assert!(find_tool("start_agent").is_some());
        assert!(find_tool("send_instruction").is_some());
        assert!(find_tool("read_output").is_some());
        assert!(find_tool("split_pane").is_some());
        assert!(find_tool("arrange_grid").is_some());
        assert!(find_tool("read_memory").is_some());
        assert!(find_tool("write_memory").is_some());
        assert!(find_tool("search_memory").is_some());
        assert!(find_tool("nonexistent").is_none());
    }

    #[test]
    fn test_execute_tool_call_valid() {
        let call = ToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({}),
        };
        let result = execute_tool_call(&call);
        assert!(result.is_error);
    }

    #[test]
    fn test_execute_tool_call_unknown() {
        let call = ToolCall {
            name: "nonexistent_tool".into(),
            arguments: serde_json::json!({}),
        };
        let result = execute_tool_call(&call);
        assert!(result.is_error);
        assert!(result.content.contains("未知工具"));
    }

    // ── ToolResult construction tests ─────────────────────────────────

    #[test]
    fn test_tool_result_ok() {
        let result = ToolResult::ok("test", "content");
        assert_eq!(result.tool_name, "test");
        assert_eq!(result.content, "content");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("test", "failed");
        assert_eq!(result.tool_name, "test");
        assert_eq!(result.content, "failed");
        assert!(result.is_error);
    }

    // ── ToolCall / ToolResult construction tests ──────────────────────

    #[test]
    fn test_tool_call_serialization() {
        let call = ToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/tmp/test.txt" }),
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("read_file"));
        assert!(json.contains("/tmp/test.txt"));

        let deserialized: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "read_file");
        assert_eq!(
            deserialized.arguments["path"].as_str().unwrap(),
            "/tmp/test.txt"
        );
    }

    #[test]
    fn test_tool_result_serialization() {
        let result = ToolResult::ok("test", "ok content");
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("ok content"));
        assert!(!json.contains("true"));

        let result_err = ToolResult::error("test", "err");
        let json_err = serde_json::to_string(&result_err).unwrap();
        assert!(json_err.contains("true"));
    }

    // ── Tool description and schema tests ─────────────────────────────

    #[test]
    fn test_all_tools_have_descriptions() {
        for tool in builtin_tools() {
            let desc = tool.description();
            assert!(
                !desc.is_empty(),
                "Tool '{}' has empty description",
                tool.name()
            );
        }
    }

    #[test]
    fn test_all_tools_have_parameters_schema() {
        for tool in builtin_tools() {
            let params = tool.parameters();
            assert!(
                params.get("type").and_then(|t| t.as_str()) == Some("object"),
                "Tool '{}' parameters should have type 'object'",
                tool.name()
            );
        }
    }

    // ── Tool parameter schema tests ───────────────────────────────────

    #[test]
    fn test_read_file_parameters() {
        let params = ReadFile.parameters();
        assert_eq!(params["type"].as_str().unwrap(), "object");
        assert_eq!(params["properties"]["path"]["type"].as_str().unwrap(), "string");
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("path")));
    }

    #[test]
    fn test_search_files_parameters_default_max_results() {
        let params = SearchFiles.parameters();
        let max_results = &params["properties"]["max_results"];
        assert_eq!(max_results["type"].as_str().unwrap(), "integer");
        assert_eq!(max_results["default"].as_u64().unwrap(), 50);
    }

    #[test]
    fn test_list_agents_no_required_params() {
        let params = ListAgents.parameters();
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.is_empty());
    }

    #[test]
    fn test_start_agent_required_params() {
        let params = StartAgent.parameters();
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"agent".to_string()));
        assert!(required.contains(&"workspace_id".to_string()));
    }

    #[test]
    fn test_send_instruction_required_params() {
        let params = SendInstruction.parameters();
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"pane_id".to_string()));
        assert!(required.contains(&"text".to_string()));
    }

    #[test]
    fn test_read_output_optional_lines() {
        let params = ReadOutput.parameters();
        let required: Vec<String> = params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"pane_id".to_string()));
        assert!(!required.contains(&"lines".to_string()));
    }

    #[test]
    fn test_split_pane_direction_enum() {
        let params = SplitPane.parameters();
        let enum_values: Vec<String> = params["properties"]["direction"]["enum"]
            .as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(enum_values.contains(&"horizontal".to_string()));
        assert!(enum_values.contains(&"vertical".to_string()));
    }

    #[test]
    fn test_arrange_grid_defaults() {
        let params = ArrangeGrid.parameters();
        let rows_default = params["properties"]["rows"]["default"].as_u64().unwrap();
        let cols_default = params["properties"]["cols"]["default"].as_u64().unwrap();
        assert_eq!(rows_default, 1);
        assert_eq!(cols_default, 1);
    }

    #[test]
    fn test_memory_tools_required_params() {
        let read_params = ReadMemory.parameters();
        let read_required: Vec<String> = read_params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert_eq!(read_required, vec!["key"]);

        let write_params = WriteMemory.parameters();
        let write_required: Vec<String> = write_params["required"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(write_required.contains(&"key".to_string()));
        assert!(write_required.contains(&"value".to_string()));
    }

    // ── Glob matching tests ───────────────────────────────────────────

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("main.rs", "*.rs"));
        assert!(glob_match("lib.rs", "*.rs"));
        assert!(!glob_match("main.rs", "*.toml"));
        assert!(!glob_match("README.md", "*.rs"));
    }

    #[test]
    fn test_glob_match_question_mark() {
        assert!(glob_match("a.rs", "?.rs"));
        assert!(!glob_match("ab.rs", "?.rs"));
    }

    #[test]
    fn test_glob_match_leading_star() {
        assert!(glob_match("abc.txt", "*.txt"));
        assert!(glob_match("a.txt", "*.txt"));
        assert!(!glob_match("txt", "*.txt"));
    }

    #[test]
    fn test_glob_match_no_match() {
        assert!(!glob_match("file.rs", "*.toml"));
        assert!(!glob_match("file", ""));
    }

    // ── ToolContext tests ─────────────────────────────────────────────

    #[test]
    fn test_tool_context_new() {
        let ctx = ToolContext::new("ws-1".into());
        assert_eq!(ctx.workspace_id, "ws-1");
        assert!(ctx.herdr_client.is_none());
    }

    #[test]
    fn test_tool_context_with_herdr() {
        let herdr = crate::ai::herdr::HerdrIpcClient::new(std::path::PathBuf::from("/tmp/test.sock"));
        let ctx = ToolContext::with_herdr("ws-2".into(), herdr);
        assert_eq!(ctx.workspace_id, "ws-2");
        assert!(ctx.herdr_client.is_some());
    }

    // ── Full dispatch loop simulation ─────────────────────────────────

    #[test]
    fn test_execute_tool_call_with_ctx() {
        let ctx = default_ctx();
        let call = ToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({}),
        };
        let result = execute_tool_call_with_ctx(&call, &ctx);
        // No path → should error
        assert!(result.is_error);
    }
}
