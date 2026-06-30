1. 创建 `src/ai/herdr.rs` — HerdrIpcClient 完整实现
2. 创建 `src/ai/herdr_noop.rs` — 非 Unix 平台的 no-op 桩
3. `src/ai/mod.rs` — 导出 herdr 模块
4. `cargo check` 验证

## Implementation Steps

### Step 1: src/ai/herdr.rs
- `HerdrIpcClient` struct + `connect()` + `socket_path()`
- `send_request()` JSON-line 协议
- 数据结构: `WorkspaceInfo`, `TabInfo`, `PaneInfo`, `AgentStatus`, `Event`
- 操作: `list_workspaces()`, `create_pane()`, `start_agent()`, `send_text()`, `read_output()`, `wait_for_output()`, `list_agent_statuses()`
- `subscribe_events()` → 线程 + crossbeam_channel

### Step 2: Platform stub (optional, for non-unix)
- `#[cfg(not(unix))]` 版本返回错误信息

### Step 3: src/ai/mod.rs
- `#[cfg(unix)] pub mod herdr;` 或通过平台门控

### Step 4: 验证
- `cargo check`
- 如果 herdr 进程运行，可执行集成测试
