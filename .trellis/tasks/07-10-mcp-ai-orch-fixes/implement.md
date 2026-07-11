# 修复 MCP 显示/AI 语言/编排调度 3 个遗留问题 — 执行计划

## 资源

- 3 个独立问题，可并行修复
- 每个问题对应一个独立的 child task

## 执行步骤

### Step 1: 修复内置 MCP 服务显示

**问题**: `refresh_mcp_data()` 在 SSE 未启用时移除 `zenix-orchestrator` 且不重新加入。

**操作**:
1. 修改 `src/app.rs` 中 `refresh_mcp_data()`:
   - 移除条件判断（`if self.config.mcp_server_sse_enabled()`）
   - 始终调用 `ensure_builtin_orchestrator()` 并将内置服务插入列表
   - 保持 SSE 配置仅控制 MCP Server 的 SSE listener 启停，不影响 UI 显示

```rust
// 修改前
servers.retain(|s| s.name != "zenix-orchestrator");
if self.config.mcp_server_sse_enabled() {
    let builtin = crate::mcp::ensure_builtin_orchestrator();
    servers.insert(0, builtin);
}

// 修改后
servers.retain(|s| s.name != "zenix-orchestrator");
let builtin = crate::mcp::ensure_builtin_orchestrator();
servers.insert(0, builtin);
```

**验证**: `cargo check` 通过

---

### Step 2: 修复 AI 回复语言

**问题**: 系统 prompt 硬编码为"用中文回复"，没有语言检测机制。用户输入中文时 AI 可能回复其他语言。

**操作**:

1. **添加语言检测工具函数**（在 `src/ai/engine.rs`）:
```rust
/// Detect user input language based on character ranges.
fn detect_language(text: &str) -> &'static str {
    if text.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff' | '\u{3000}'..='\u{303f}' | '\uff00'..='\uffef')) {
        "zh"
    } else {
        "en"
    }
}
```

2. **修改 `build_system_prompt()`**:
   - 添加 `lang: &str` 参数
   - 根据语言设置回复指令

3. **更新调用处**（`src/ai/chat/mod.rs` 或 `src/ai/engine.rs` 调用处）:
   - 在发送用户消息时调用 `detect_language()` 检测语言
   - 将结果传入 `build_system_prompt()`

**验证**: `cargo check` 通过

---

### Step 3: 修复编排调度（委托模式识别）

**问题**: 系统 prompt 没有指导 LLM 处理"帮我问一下 X"等委托模式，LLM 可能直接回答问题而不 spawn worker。

**操作**:

1. **增强系统 prompt**（`src/ai/engine.rs`）:
   - 在 `DEFAULT_SYSTEM_PROMPT` 和 `build_system_prompt()` 中添加明确的委托模式处理指令

```rust
pub const DEFAULT_SYSTEM_PROMPT: &str = "你是 ZenixAI 编排器。你负责理解用户需求、规划任务分解、将具体执行工作分配给专业 Agent CLI（如 Claude Code、Codex、OpenCode 等）。你不直接写代码或运行命令，只负责规划、调度和结果汇总。用中文回复。

## 委托模式识别
当用户说"帮我问一下 X"、"帮我查一下 Y"、"用 Z 工具做 W"等委托式的请求时，你应该使用 spawn_worker 工具创建一个新的 worker agent 来执行该任务。

## 简单 vs 复杂任务
- 简单的问答或闲聊：直接回复，不需要 spawn worker
- 需要执行命令、写代码、查询外部信息等：使用 spawn_worker 创建 worker 执行
- 用户明确指定"问一下 X"、"帮我查 Y"：必须使用 spawn_worker 将任务委托给相应的 agent";
```

2. **可选：添加 pre-processing**:
   - 在 `src/ai/chat/mod.rs` 的消息处理中，检测"帮我问一下"、"帮我查一下"等模式
   - 如果匹配，在消息前附加系统提示（而非自动执行）

**验证**: `cargo check` 通过

---

### 验证步骤

```bash
cargo check                          # 编译检查
cargo clippy                         # 确保无新增警告
```

---

### 回滚点

| 步骤 | 回滚操作 | 条件 |
|------|---------|------|
| Step 1 | `git checkout -- src/app.rs` | 编译失败或逻辑错误 |
| Step 2 | `git checkout -- src/ai/engine.rs` | 编译失败 |
| Step 3 | `git checkout -- src/ai/engine.rs src/ai/chat/mod.rs` | 编译失败 |

---

### 质量门禁

- [ ] `cargo check` 无错误
- [ ] `cargo clippy` 无新增警告
- [ ] 验证内置 MCP 在设置面板可见
- [ ] 验证语言检测逻辑正确性
- [ ] 验证编排 prompt 增强不影响现有工具调用
