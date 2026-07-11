# 修复编排功能 — 执行计划

## 执行顺序

Issue 2 和 Issue 3 有依赖关系（都改 tools.rs），先改 Issue 2 的 ToolCall 结构再改 Issue 3。
Issue 1 完全独立。

```
Step 1: ToolCall 结构变更 (tools.rs + engine.rs)
  ↓
Step 2: 修复 tool call 消息格式 (engine.rs)
  ↓
Step 3: herdr 连接预检 (tools.rs)
  ↓
Step 4: 思考内容按轮次分离 (engine.rs + app.rs)
  ↓
Step 5: cargo check + clippy
```

## Step 1: ToolCall 结构 + parse_sse_tool_calls 返回值变更

**操作**:
1. `src/ai/tools.rs`: `ToolCall` 增加 `pub id: String` 字段
2. `src/ai/engine.rs`: `parse_sse_tool_calls()` 返回 `Vec<(String, String, serde_json::Value)>`（tool_call_id, name, args）
3. `ChatMessageRef` 增加 `tool_calls: Option<Vec<ToolCall>>` 和 `tool_call_id: Option<String>`，用 `#[serde(skip_serializing_if = "Option::is_none")]` 跳过空值
4. 更新 `chat_with_tools` 和 `chat_simple_with_tools` 中 `raw_calls` 到 `ToolCall` 的转换，传入 `id`

**验证**: `cargo check`

## Step 2: 修复 tool call 消息循环

**操作**:
1. `chat_with_tools` 中：在工具执行后，先推入 assistant tool_calls 消息，再逐个推入 tool 结果消息
2. `chat_simple_with_tools` 中同样修复（保持一致性）

**详细代码**:
```rust
// After executing tools, before next LLM call:

// 1. Add assistant message with tool_calls
loop_messages.push(ChatMessageRef {
    role: "assistant".into(),
    content: String::new(),
    tool_calls: Some(tool_calls.clone()),
    tool_call_id: None,
});

// 2. Add tool result messages with tool_call_id
for (i, result) in results.iter().enumerate() {
    loop_messages.push(ChatMessageRef {
        role: "tool".into(),
        content: format!(
            "Tool '{}' returned: {}{}",
            result.tool_name,
            if result.is_error { "ERROR: " } else { "" },
            result.content
        ),
        tool_calls: None,
        tool_call_id: Some(tool_calls[i].id.clone()),
    });
}
```

**验证**: `cargo check`

## Step 3: herdr 连接预检

**操作**:
在 `src/ai/tools.rs` 的 `SpawnWorker::execute()` 开头，获取 herdr_client 后调用 `connect()`：

```rust
fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
    let mut client = executor.herdr_client.lock().unwrap();
    if let Err(e) = client.connect() {
        return ToolResult::error("spawn_worker",
            format!("无法连接到 herdr daemon: {e}。请确认 herdr 正在运行。"));
    }
    // ... 原有逻辑不变
}
```

**验证**: `cargo check`

## Step 4: 思考内容按轮次分离

**操作**:
1. `src/ai/engine.rs`: `StreamEvent::Thinking` 变体改为 `Thinking { round: u32, text: String }`
2. `src/ai/engine.rs`: `DispatchStreamEvent::Thinking` 变体改为 `Thinking { round: u32, text: String }`
3. `parse_tool_call_sse_stream` 中发出 `StreamEvent::Thinking { round, text }`（round 由调用方传入）
4. `chat_with_tools` 中传入当前 `tool_round`
5. `src/app.rs`: `handle_dispatch_event` 中按 round 处理 reasoning（当前简化：不同 round 追加时加分隔标记）

**验证**: `cargo check`

## Step 5: 验证

```bash
cargo check
cargo clippy
```

## 回滚点

| 步骤 | 回滚操作 |
|------|---------|
| Step 1 | `git checkout -- src/ai/tools.rs src/ai/engine.rs` |
| Step 2 | `git checkout -- src/ai/engine.rs` |
| Step 3 | `git checkout -- src/ai/tools.rs` |
| Step 4 | `git checkout -- src/ai/engine.rs src/app.rs` |

## 质量门禁

- [ ] `cargo check` 无错误
- [ ] `cargo clippy` 无新增警告
- [ ] 代码 review: tool call 消息格式正确
- [ ] 代码 review: thinking round 传递无误
