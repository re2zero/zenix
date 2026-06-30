1. Cargo.toml 添加 `rusqlite = { version = "0.32", features = ["bundled"] }`
2. 创建 `src/ai/mod.rs` — mod session 声明 + pub use
3. 创建 `src/ai/session.rs` — SessionManager 完整实现（+ Schema + CRUD + FTS5）
4. `src/lib.rs` — 添加 `pub mod ai;`
5. 运行 `cargo check` 验证

## Implementation Steps

### Step 1: Cargo.toml
- 在 `[dependencies]` 中追加: `rusqlite = { version = "0.32", features = ["bundled"] }`

### Step 2: src/ai/mod.rs
```rust
pub mod session;
pub use session::*;
```

### Step 3: src/ai/session.rs
实现以下内容（顺序依赖）：
1. `Session`, `ChatMessage`, `AgentActivity`, `SearchHit` 结构体定义
2. `Schema` 常量（CREATE TABLE + FTS5 + triggers）
3. `SessionManager::open()` + `open_in_memory()`（用于测试）
4. `SessionManager::list_sessions()` / `get_session()` / `create_session()` / `update_session()` / `delete_session()`
5. `SessionManager::add_message()` / `get_messages()`
6. `SessionManager::search_messages()` 带 FTS5 安全查询
7. 单元测试

### Step 4: src/lib.rs
- 在现有 pub mod 中加入 `pub mod ai;`

### Step 5: 验证
- `cargo check`
- `cargo test`
