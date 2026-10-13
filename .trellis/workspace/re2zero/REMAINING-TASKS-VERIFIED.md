# 剩余任务 — 验证记录

> 生成日期: 2026-07-12
> 验证方式: 逐一读取源代码确认，`cargo check` 通过

---

## 一、已验证已完成的功能（我的前期分析有多处错误）

| 功能 | 状态 | 证据 |
|------|------|------|
| MCP Server（stdio + SSE） | ✅ 完成 | `src/mcp/server.rs` — start/shutdown/SSE listener |
| MCP handler（10 个工具） | ✅ 完成 | `src/mcp/handler.rs` — build_tools() + handle_tool_call() |
| Worker 通信工具 | ✅ **我前次说未实现是错的** | `handler.rs:108-126` 定义，`132-188` 实现，使用 SQLite memories 存储 |
| Function Calling 消息格式 | ✅ 完成 | `ToolCall.id` (tools.rs:29)、`ChatMessageRef.tool_calls/tool_call_id` (engine.rs:146-150)、assistant 消息注入 (engine.rs:930-960) |
| Thinking round 分离 | ✅ 完成 | `StreamEvent::Thinking { round, text }` (engine.rs:160) |
| herdr 连接预检 | ✅ 完成 | `tools.rs:189` — `client.connect()` |
| MCP 显示（无 SSE 条件） | ✅ 完成 | `app.rs:1194-1202` — 无条件保留 zenix-orchestrator |
| 语言检测函数 | ✅ 完成 | `engine.rs:44-56` — detect_language；`app.rs:3234-3239` — 调用检测 |
| 动态 prompt 的委托指令 | ✅ 完成 | `build_system_prompt()` — "帮我问一下 X → spawn_worker" 指令 |
| SessionManager 线程安全 | ✅ 完成 | `session.rs:5` — `std::sync::Mutex` |
| MCP 配置项 | ✅ 完成 | `config/mod.rs:90-94` — mcp_server_enabled/sse_enabled |
| MCP Server 集成到启动 | ✅ 完成 | `app.rs:331-377` — 按 mcp_server_enabled 配置启动 |
| SSE 模块 | ✅ 完成 | `src/mcp/sse.rs` + `mod.rs:695 pub mod sse` |
| check.jsonl/implement.jsonl | ✅ **我前次说缺失是错的** | 两者均存在且有内容 |

## 二、真实遗漏（已验证）

### Issue #1: DEFAULT_SYSTEM_PROMPT 缺失语言自适应和委托指令 [中]

**位置**: `src/ai/engine.rs:40`

```rust
pub const DEFAULT_SYSTEM_PROMPT: &str = "你是 ZenixAI 编排器。你负责将用户的任务原样分发给\
专业 Agent CLI（如 Claude Code、Codex、OpenCode 等）。\
你不分析或改写任务内容，只负责分发和汇总结果。用中文回复。";
```

**根因**: `build_system_prompt()` (engine.rs:76-78) 在无 roles 且无 workspace 时直接返回 DEFAULT_SYSTEM_PROMPT，跳过了所有语言检测和委托指令逻辑。

**影响路径**:
```
用户首次使用（无角色、无 workspace 绑定）
  → app.rs line 3234: user_lang = "zh"（硬编码 fallback 报警句）
  → engine.rs line 77: 返回 DEFAULT_SYSTEM_PROMPT
  → 最终 prompt: "用中文回复"，无"帮我问一下"委托模式识别
```

**影响**:
- 中文输入以外的语言得不到正确回复指令
- LLM 不会将"帮我问一下 X"识别为 spawn_worker 触发信号
- 用户首次开箱体验差

**修复方案**: 修改 `build_system_prompt()` 的 fallback 路径，或直接增强 DEFAULT_SYSTEM_PROMPT。

### Issue #2: 死代码（未清理）[低]

| 位置 | 符号 | 说明 |
|------|------|------|
| engine.rs:155 | `#[allow(dead_code)] pub enum StreamEvent` | 被 `DispatchStreamEvent` 替代？ |
| engine.rs:174 | `DEFAULT_AGENT_TIMEOUT_MS` | 有 allow(dead_code) |
| engine.rs:179 | `AGENT_DONE_PATTERN` | 有 allow(dead_code) |
| engine.rs:183 | `AgentTask` | 有 allow(dead_code) |
| engine.rs:983 | `chat_simple_with_tools()` | 定义但无调用方 |

### Issue #3: Task 元数据过期 [低]

| task | task.json status | 实际代码状态 |
|------|-----------------|-------------|
| 07-09-mcp-orchestration-bus | in_progress | MCP Server 全部代码已提交 |
| 07-10-orchestration-loop-fixes | in_progress | 3 个 issue 全部已修复并提交 |
| 07-10-mcp-ai-orch-fixes | in_progress | MCP 显示已修复，语言检测/委托指令部分完成 |

### Issue #4: MCP 工具命名与 design.md 不一致 [低]

- design.md 指定 `zenix_` 前缀（如 `zenix_spawn_worker`）
- 实际实现使用裸名（`spawn_worker`、`worker_list`）
- 不影响功能，无冲突风险（MCP Server 命名空间天然隔离）

---

## 三、综合结论

**核心 AI 编排功能（MCP 总线 → 工具循环 → Worker 通信 → 语言检测）的代码实现基本完整。** 上一次分析中我夸大了多项遗漏。

**唯一值得优先修复的问题是 Issue #1** — DEFAULT_SYSTEM_PROMPT 的 fallback 路径缺失语言自适应和委托指令。这影响所有无角色/workspace 的首次用户体验。

其他 issues 均为低优先级的清理和元数据更新。
