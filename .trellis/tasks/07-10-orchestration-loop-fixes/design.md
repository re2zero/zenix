# 修复编排功能 — 技术设计

## Issue 1: 思考内容按轮次分离

### 现状

```
DispatchStreamEvent::Thinking(String)  // 无 round 信息
→ handle_dispatch_event 追加到 msg.reasoning_text (单个 String)
```

### 修改方案

**Step 1**: `DispatchStreamEvent` 增加 round 字段：
```rust
pub enum DispatchStreamEvent {
    Thinking { round: u32, text: String },
    // ... 其他变体不变
}
```

**Step 2**: `StreamEvent` 增加 round 字段：
```rust
pub enum StreamEvent {
    Thinking { round: u32, text: String },
    // ...
}
```

**Step 3**: `chat_with_tools` 中传入 round：
```rust
let current_round = tool_round;
on_event_clone(DispatchStreamEvent::Thinking {
    round: current_round,
    text,
});
```

**Step 4**: `handle_dispatch_event` 中按 round 追加到不同 reasoning 块

### 变更文件

| 文件 | 变更 |
|------|------|
| `src/ai/engine.rs` | `DispatchStreamEvent::Thinking` + `StreamEvent::Thinking` 增加 round |
| `src/app.rs` | `handle_dispatch_event` 按 round 追加 reasoning |

## Issue 2: 修复 tool call 消息格式

### 现状

```
LLM SSE stream:
  delta.tool_calls[0] = {id: "call_xxx", type: "function", function: {name: "spawn_worker", arguments: '{"agent":"opencode"}'}}

parse_sse_tool_calls():
  → discards "id" → returns ("spawn_worker", {agent: "opencode"})

chat_with_tools():
  → tool_calls iter().map(|tc| executor.execute(tc))
  → loop_messages.push({role: "tool", content: "Tool 'spawn_worker' returned: ..."})
  → NEXT ITERATION: API gets [{role: "system", ...}, {role: "user", ...}, {role: "tool", ...}]
    ★ MISSING: {role: "assistant", tool_calls: [...]}
    ★ WRONG FORMAT: {role: "tool"} without tool_call_id
```

### 目标格式 (OpenAI API)

```json
[
  {"role": "system", "content": "..."},
  {"role": "user", "content": "用户的问题"},
  {"role": "assistant", "tool_calls": [
    {"id": "call_xxx", "type": "function", "function": {"name": "spawn_worker", "arguments": "{\"agent\": \"opencode\", \"instruction\": \"...\"}"}}
  ]},
  {"role": "tool", "tool_call_id": "call_xxx", "content": "Worker 'opencode' 已启动..."}
]
```

### 修改方案

**Step 1**: `ToolCall` 增加 `id: String` 字段：
```rust
pub struct ToolCall {
    pub id: String,         // 新增：SSE tool_call_id
    pub name: String,
    pub arguments: serde_json::Value,
}
```

**Step 2**: `parse_sse_tool_calls` 返回三元组 `(id, name, args)`：
```rust
pub(crate) fn parse_sse_tool_calls(delta: &SseDeltaWithTools) -> Vec<(String, String, serde_json::Value)> {
    // 返回 (tool_call_id, name, args)
}
```

**Step 3**: `ChatMessageRef` 增加可选字段：
```rust
pub struct ChatMessageRef {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}
```

**Step 4**: `chat_with_tools` 中先加 assistant tool_calls 消息，再加 tool 结果消息：
```rust
// 1. 加入 assistant 的 tool_calls 消息
loop_messages.push(ChatMessageRef {
    role: "assistant".into(),
    content: String::new(),
    tool_calls: Some(tool_calls_with_id.clone()),
    tool_call_id: None,
});

// 2. 逐个加入 tool 结果消息，带 tool_call_id
for (i, result) in results.iter().enumerate() {
    loop_messages.push(ChatMessageRef {
        role: "tool".into(),
        content: result.content.clone(),
        tool_calls: None,
        tool_call_id: Some(tool_calls_with_id[i].id.clone()),
    });
}
```

### 变更文件

| 文件 | 变更 |
|------|------|
| `src/ai/tools.rs` | `ToolCall` 增加 `id` 字段 |
| `src/ai/engine.rs` | `ChatMessageRef` 增加 `tool_calls`/`tool_call_id`；`parse_sse_tool_calls` 返回三元组；`chat_with_tools` 加入 assistant tool_calls 消息 |

## Issue 3: herdr 连接预检

### 现状

`spawn_worker` 工具在 `execute()` 中调用 `client.start_agent()`，内部调用 `send_request_raw()` 新建 `UnixStream::connect()`。如果 herdr 未运行，连接失败返回错误。

### 修改方案

在 `SpawnWorker::execute()` 中首次获取 herdr client 时，执行 `client.connect()` 预检：

```rust
fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult {
    let mut client = executor.herdr_client.lock().unwrap();
    
    // 预检连接
    if let Err(e) = client.connect() {
        return ToolResult::error("spawn_worker", 
            format!("无法连接到 herdr daemon: {e}。请确认 herdr 正在运行。"));
    }
    // ... 后续逻辑
}
```

### 变更文件

| 文件 | 变更 |
|------|------|
| `src/ai/tools.rs` | `SpawnWorker::execute()` 开头添加 `client.connect()` 预检 |
