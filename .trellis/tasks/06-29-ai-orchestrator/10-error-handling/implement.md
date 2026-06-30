1. Cargo.toml 确认 `thiserror` 依赖已存在（若没有则添加）
2. 创建 `src/ai/error.rs` — AiError + retry_with_backoff + user_facing_error + truncate_agent_output
3. `src/ai/mod.rs` — 添加 `pub mod error;`
4. `cargo check` 验证

> 注意：本任务单独作为 P1 完成，`src/ai/error.rs` 是独立模块，不依赖其他任务的具体类型
> （AiError 所有字段使用 String，不引用 AgentTask/HerdrIpcClient 等）。
> 后续任务（03/05/06）在自己的实现中导入 `AiError` 并将其包装进自己的错误处理。

## Implementation Steps

### Step 0: Cargo.toml
- 确认存在 `thiserror` 依赖。若没有则添加：
  ```toml
  thiserror = "2"
  ```

### Step 1: src/ai/error.rs
- `AiError` enum with `thiserror::Error` derive
  - 所有 variant 使用 String 字段（不引用外部类型）
  - `ApiError(String)`, `RateLimited(u64)`, `HerdrError(String)`, `AgentFailed { agent, detail }`, `AgentTimeout { agent, timeout }`, `Database(String)`, `ConfigError(String)`, `Other(String)`
- `retry_with_backoff()` 泛型异步重试（指数退避）
- `user_facing_error()` 将技术错误转为人可读消息
- `truncate_agent_output()` 截断长输出

### Step 2: src/ai/mod.rs
- `pub mod error;`
- `pub use error::*;`

### Step 3: 验证
- `cargo check`
