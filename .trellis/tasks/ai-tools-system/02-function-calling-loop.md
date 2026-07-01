# 子任务 02: Function Calling 循环改造

## 目标
改造 `engine.rs`，将 `chat_and_dispatch` 从硬编码调度改为工具调用循环。

## 背景
当前 `chat_and_dispatch` 使用 `parse_dispatch_directive()` 解析 LLM 响应中的 JSON dispatch 指令。
改为：LLM 通过 function calling 调用工具，编排器执行工具后返回结果，循环直到 LLM 直接回复。

## 核心变更

### 1. 工具调用循环

```
loop {
    response = call_llm_with_tools(messages, tools)
    if response.has_tool_calls() {
        results = execute_tool_calls(response.tool_calls)
        messages += tool_results
        continue
    }
    return response.content  // LLM 直接回复
}
```

### 2. 函数签名

```rust
// 新增：带工具的 LLM 调用
pub async fn chat_with_tools(
    &self,
    messages: &[ChatMessageRef],
    tools: &[&dyn Tool],  // 当前启用的工具列表
    herdr_client: &mut HerdrIpcClient,
    workspace_id: &str,
    on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
) -> Result<String, AiError>;
```

### 3. 工具执行器

```rust
pub struct ToolExecutor {
    herdr_client: Arc<Mutex<HerdrIpcClient>>,
    workspace_id: String,
    session_id: Option<String>,
}

impl ToolExecutor {
    pub fn execute(&self, call: &ToolCall) -> ToolResult;
}
```

### 4. 新增 StreamEvent

```rust
pub enum StreamEvent {
    // ... existing
    ToolCall(String, serde_json::Value),  // tool_name, arguments
    ToolResult(String, String, bool),     // tool_name, content, is_error
}
```

### 5. 保留向后兼容

- `chat_simple` 不变
- `chat_and_dispatch` 保留（标记为 deprecated），内部调用 `chat_with_tools`

### 6. 并发工具调用

LLM 可能在一个响应中调用多个工具，需要并发执行：
```rust
// 收集所有 tool calls
// 用 tokio::spawn 并发执行
// 收集所有结果
```

## 不做什么
- 不实现 Agent 管理工具（子任务 03）
- 不实现记忆管理工具（子任务 03）
- 不实现布局控制工具（子任务 03）
- 不改 chat.rs 的 UI（工具结果在 UI 层组装）

## 验证标准
- ✅ `cargo check` 编译通过
- ✅ 所有现有测试通过
- ✅ 新增测试：工具调用循环、并发工具执行、工具参数解析
- ✅ `cargo clippy` 无新增警告
