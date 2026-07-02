1. Cargo.toml 添加 `reqwest`, `tokio`, `futures` 依赖
2. `src/main.rs` — 初始化 tokio runtime
3. 创建 `src/ai/engine.rs` — SSE 解析器 + OrchestratorEngine
4. `src/ai/mod.rs` — 导出 engine 模块
5. `cargo check` 验证

## Implementation Steps

### Step 1: Cargo.toml
```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio = { version = "1", features = ["rt", "macros"] }
futures = "0.3"
regex = "1"
```

### Step 2: src/main.rs
```rust
fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = rt.enter();  // 使 reqwest 可用
    // 其余保持不变...
}
```

### Step 3: src/ai/engine.rs
- `AgentTask` / `AgentResult` 占位结构体（`#[allow(dead_code)]`，任务 06 会替换）
- `StreamEvent` enum（含 Agent* variant，但 SSE 解析只 emit Token/Error/Done）
- `extract_sse_content()` SSE 解析器（OpenAI + Anthropic）
- `OrchestratorEngine::new(provider)`
- `OrchestratorEngine::chat_stream(messages, system_prompt, on_event)`
- `OrchestratorEngine::chat_simple(message, system_prompt) -> String`

> 注意：StreamEvent 的 AgentDispatch/AgentOutput/AgentComplete 是留给任务 06 用的占位。
> 引用自任务 01 的 `ChatMessage`（`super::session::ChatMessage`）需要在实现时确认字段名一致。

### Step 4: src/ai/mod.rs
- 添加 `pub mod engine;`

### Step 5: 验证
- `cargo check`
