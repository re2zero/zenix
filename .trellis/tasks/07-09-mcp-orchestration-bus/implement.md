# MCP 通信总线 + 编排验证接口 — 执行计划

## 执行步骤

### Step 0: SessionManager 线程安全化

**操作**：
- 将 `SessionManager::conn` 从 `Connection` 改为 `Mutex<Connection>`
- 在每个方法中加 `self.conn.lock().unwrap()` 包裹 SQLite 操作
- 动机：`rusqlite::Connection` 是 `Send` 但不是 `Sync`，MCP Server 多线程并发调用会编译失败

**变更**：
```rust
// before
pub struct SessionManager {
    conn: Connection,
}

// after  
pub struct SessionManager {
    conn: Mutex<Connection>,
}
```

**验证**：
```bash
cargo check
cargo test -p zenix -- session  # 全部通过
```

**回滚**：`git checkout -- src/ai/session.rs`

### Step 1: 添加 rmcp 依赖

**操作**：
- 在 `Cargo.toml` 中添加 `rmcp = { version = "2.2", features = ["server", "transport-stdio"] }`
- 如果需要 SSE 模式，额外添加 `features = ["transport-sse"]`

**验证**：
```bash
cargo check
```

### Step 2: 创建 MCP 工具模块

**操作**：
- 新建 `src/mcp/server.rs` — `McpOrchestrationServer` struct 和生命周期
- 新建 `src/mcp/handler.rs` — `McpToolHandler` 实现 `ServerHandler` trait
- 新建 `src/mcp/types.rs` — WorkerMessage 等通信类型
- 更新 `src/mcp/mod.rs` — pub mod server; pub mod handler; pub mod types;

**核心代码**：

`server.rs`:
```rust
use rmcp::{
    ServiceExt, RxJsonrpcSession,
    model::{ServerInfo, ProtocolVersion},
    transport::StdioTransport,
};

pub struct McpOrchestrationServer {
    session: Option<RxJsonrpcSession>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    join_handle: Option<tokio::task::JoinHandle<()>>,
}

impl McpOrchestrationServer {
    pub async fn start(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
    ) -> Result<Self> {
        let handler = McpToolHandler::new(executor, worker_registry, session_manager, workspace_id);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = rmcp::ServiceExt::serve(
            handler,
            StdioTransport::new(rmcp::transport::TransportOptions::default()),
            ServerInfo {
                protocol_version: ProtocolVersion::V_2025_11_05,
                server_name: "zenix-orchestrator".into(),
                server_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: rmcp::model::ServerCapabilities::builder()
                    .enable_tools()
                    .build(),
                instructions: Some("Zenix orchestration MCP server — manage workers and agent communication".into()),
            },
        ).await?;

        let session = Some(server.session());
        let join_handle = Some(tokio::task::spawn(async move {
            tokio::select! {
                _ = server => {},
                _ = shutdown_rx => {},
            }
        }));

        Ok(Self { session, shutdown_tx: Some(shutdown_tx), join_handle })
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.await;
        }
        // Drop session to close connection
        drop(self.session.take());
    }
}
```

`handler.rs`:
```rust
use rmcp::{
    model::*,
    server::ServerHandler,
    tool::{Tool, ToolCallResult},
    FromServer, HandlerResult, RequestId, RpcError,
};

#[derive(Clone)]
pub struct McpToolHandler {
    executor: Arc<ToolExecutor>,
    worker_registry: Arc<WorkerRegistry>,
    session_manager: Arc<SessionManager>,
    workspace_id: String,
}

impl McpToolHandler {
    pub fn new(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
    ) -> Self { ... }

    fn build_tool_list() -> Vec<Tool> { ... }

    fn execute_zenix_tool(&self, name: &str, args: Value) -> Result<CallToolResult, CallToolError> { ... }
}

#[rmcp::tool(tool)]
impl McpToolHandler {
    /// List all available orchestration tools (MCP discovery)
    fn list_tools(&self) -> Vec<Tool> { Self::build_tool_list() }

    // Each zenix_* tool is registered via the ServerHandler trait
}
```

**验证**：
```bash
cargo check
```

### Step 3: 实现 Worker 通信消息存储

**操作**：
- 在 `SessionManager` 中添加 worker 消息专用方法（或复用 `write_memory`/`search_memory`）
- 消息 key 格式：`worker_msg:<recipient_worker_name>:<uuid>`
- 已读消息 key 格式：`worker_read_msg:<recipient>:<sender>:<uuid>` 
- 提供 CRUD：send_message / read_inbox / mark_read / delete_inbox

**验证**：
```bash
cargo test -p zenix -- session  # 现有 session 测试通过
```

### Step 4: 实现 Worker 通信 MCP 工具

**操作**：
在 `handler.rs` 中添加：
- `zenix_worker_list` — 遍历 `WorkerRegistry::all_workers()` 返回列表
- `zenix_worker_send` — 验证 recipient 存在 → 写 SQLite → 尝试推送到 pane
- `zenix_worker_inbox` — 搜索 `worker_msg:<worker_name>:*` → 返回列表
- `zenix_worker_broadcast` — 遍历活跃 worker → 逐个发送

**验证**：
```bash
cargo check
```

### Step 5: 编排工具 MCP 封装

**操作**：
在 `handler.rs` 中为现有 6 个工具添加 MCP 封装：
- 每个工具名加 `zenix_` 前缀
- `input_schema` 从对应 `Tool::parameters()` 获取
- 执行时通过 `ToolExecutor::execute()` 调用
- 工具描述从 `Tool::description()` 获取

**验证**：
```bash
cargo check
```

### Step 6: 集成到 zenix 启动流程

**操作**：
- 在 `main.rs` 或 `app.rs` 中，于 tokio runtime 初始化后启动 MCP Server
- 传递 `ToolExecutor`（与编排器共享同一个 herdr_client）
- 传递 `WorkerRegistry` 和 `SessionManager`

**位置**：在 `app.rs` 的 `ZenixApp::new()` 或 `main.rs` 的 tokio 初始化块中

```rust
// main.rs 或 app.rs 初始化
if let Ok(mcp_server) = McpOrchestrationServer::start(
    executor.clone(),
    worker_registry.clone(),
    session_manager.clone(),
    workspace_id.clone(),
).await {
    app_state.mcp_server = Some(mcp_server);
}
```

**验证**：
```bash
cargo build  # 编译通过
```

### Step 7: 配置与降级

**操作**：
- 在 `config/mod.rs` 中添加 MCP Server 配置项（stdio 启用/禁用、socket path）
- 如果 `rmcp` crate 不可用或 MCP Server 启动失败 → 降级运行（不阻塞 zenix 启动）
- 错误日志记录降级原因

```rust
pub struct McpServerConfig {
    pub enabled: bool,         // 默认 true
    pub stdio_enabled: bool,   // 默认 true
    pub sse_enabled: bool,     // 默认 false
    pub socket_path: PathBuf,  // ~/.config/zenix/mcp.sock
}
```

**验证**：
```bash
cargo build
```

### Step 8: 验证与测试

**操作**：
- 集成测试：启动 zenix → 连接 MCP Server → 调用工具 → 验证结果
- 验证所有 10 个 MCP 工具均可调用且返回正确结果
- 验证 MCP Server 关闭与 zenix 联动

**验证**：
```bash
# 手动验证（需 herdr 运行中）
cargo run  # 启动后，用任意 MCP 客户端连接验证

# 单元测试
cargo test -p zenix -- mcp
```

### Step 9: 更新 import.jsonl / check.jsonl

**操作**：
- 更新 `check.jsonl` 添加 MCP Server 相关检查项
- 写入实现过程中发现的关键接口约定和设计约束

## Rollback 点

| 步骤 | 回滚操作 | 条件 |
|------|---------|------|
| Step 1 | `git checkout -- Cargo.toml Cargo.lock` | 编译失败或依赖冲突 |
| Step 3 | 删除新增的 session manager 方法 | 单元测试失败 |
| Step 4-5 | `git checkout -- src/mcp/` | 逻辑错误 |
| Step 6 | 删除 app.rs 中 MCP 启动代码 | 启动崩溃 |
| Step 7 | `git checkout -- src/config/` | 配置解析错误 |

## 质量门禁

- [ ] `cargo check` 无错误
- [ ] `cargo clippy` 无新增警告
- [ ] `cargo test` 全部通过
- [ ] 手动验证：herdr 运行中通过 MCP 客户端调用 `zenix_list_workers` 返回正确
