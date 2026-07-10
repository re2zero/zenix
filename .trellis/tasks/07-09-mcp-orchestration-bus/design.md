# MCP 通信总线 + 编排验证接口 — 技术设计

## 系统架构

```
┌─────────────────────────────────────────────────────────┐
│                      zenix                              │
│                                                         │
│  ┌─────────────────┐    ┌───────────────────────────┐  │
│  │  Orchestrator    │    │  MCP Server               │  │
│  │  Engine          │    │  (rmcp + StdioTransport)  │  │
│  │                  │    │                           │  │
│  │  chat_with_tools │    │  tools/list ──────────────┤  │
│  │  (function call) │    │  tools/call ──────────────┤  │
│  │       │          │    │                           │  │
│  └───────╫──────────┘    └─────────╫─────────────────┘  │
│          │                         │                     │
│          └─────────┬───────────────┘                     │
│                    │                                     │
│          ┌─────────▼───────────────┐                     │
│          │   ToolExecutor          │                     │
│          │   (Arc<Mutex<>>)        │                     │
│          │                         │                     │
│          │   herdr_client ────────►│── socket ──► herdr  │
│          │   session_manager ─────►│── SQLite            │
│          │   worker_registry ─────►│── in-memory         │
│          └─────────────────────────┘                     │
└─────────────────────────────────────────────────────────┘
               ▲                           ▲
               │ stdio                     │ stdio/SSE
               │                           │
  ┌────────────┴───┐           ┌───────────┴───┐
  │  Worker A      │           │  External AI  │
  │  Claude Code   │           │  Agent/CLI    │
  │  MCP client    │           │  (验证接口)    │
  └────────────────┘           └───────────────┘
```

## MCP Server 实现

### 依赖

```toml
# Cargo.toml
rmcp = { version = "2.2", features = ["server", "transport-stdio"] }
```

### 传输层

| 模式 | 用途 | 细节 |
|------|------|------|
| **Stdio** | Worker 通信 | 默认模式，子进程通过 stdin/stdout 与 MCP Server 通信 |
| **Unix Socket (SSE)** | 外部验证 | 可选，监听 `~/.config/zenix/mcp.sock` 供外部 AI 代理连接 |

启动时先尝试 Stdio 模式，如果检测到 `ZENIX_MCP_SSE` 环境变量则同时启动 SSE listener。

### 模块结构

```rust
src/
  mcp/
    mod.rs              // 现有：MCP 配置管理
                         // 新增：MCP Server 入口
    server.rs           // 新增：MCP Server 实现
    handler.rs          // 新增：工具注册与请求处理
    types.rs            // 新增：消息/通信类型
```

#### server.rs — MCP Server 生命周期

```rust
pub struct McpOrchestrationServer {
    runtime: Arc<ServerRuntime>,
    /// Exit signal sender
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl McpOrchestrationServer {
    /// 创建并启动 MCP Server（Stdio 模式 + 可选 SSE）
    pub fn start(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
    ) -> Result<Self>;

    /// 等待 shutdown 信号
    pub async fn wait_for_shutdown(self);

    /// 触发优雅关闭
    pub fn shutdown(self);
}
```

#### handler.rs — ServerHandler 实现

```rust
pub struct McpToolHandler {
    executor: Arc<ToolExecutor>,
    worker_registry: Arc<WorkerRegistry>,
    session_manager: Arc<SessionManager>,
    workspace_id: String,
}

impl ServerHandler for McpToolHandler {
    // handle_initialize_request — 标准握手
    // handle_list_tools_request — 返回可用工具列表
    // handle_call_tool_request — 执行并返回结果
    // on_initialized — 客户端就绪回调
}
```

### MCP 工具定义

现有编排工具的 MCP 映射（加 `zenix_` 前缀避免冲突）：

| MCP 工具 | 映射到编排工具 | 类型 |
|----------|---------------|------|
| `zenix_spawn_worker` | `SpawnWorker` | 编排 |
| `zenix_send_text` | `SendText` | 编排 |
| `zenix_read_output` | `ReadOutput` | 编排 |
| `zenix_wait_for_output` | `WaitForOutput` | 编排 |
| `zenix_list_workers` | `ListWorkers` | 编排 |
| `zenix_close_worker` | `CloseWorker` | 编排 |
| `zenix_worker_list` | 新增 | 通信 |
| `zenix_worker_send` | 新增 | 通信 |
| `zenix_worker_inbox` | 新增 | 通信 |
| `zenix_worker_broadcast` | 新增 | 通信 |

### 工具实现

#### SpawnWorker（MCP 封装）

`SpawnWorker` 已完整实现。MCP handler 调用 `ToolExecutor::execute` 并保持参数 schema 一致。

#### SendText / ReadOutput / WaitForOutput / ListWorkers / CloseWorker

同上，通过 `ToolExecutor` 复用现有 herdr IPC 逻辑。

#### 新增：Worker 通信工具

```rust
// Message 存储在 SQLite memories 表中，key 格式：worker_msg:<recipient>:<uuid>
struct WorkerMessage {
    from: String,
    to: String,
    text: String,
    timestamp: String,
}
```

**`zenix_worker_list`**
- 参数：无
- 逻辑：从 `WorkerRegistry` 读取所有 worker 信息，返回名称、状态、pane_id
- 输出：JSON 数组

**`zenix_worker_send`**
- 参数：`{ "worker_name": "...", "text": "..." }`
- 逻辑：
  1. 校验 `worker_name` 在 `WorkerRegistry` 中存在
  2. 生成 UUID 消息 ID
  3. 通过 `SessionManager::write_memory` 存储到 `worker_msg:<recipient>:<uuid>` 
  4. 同时通过 `zenix_send_text` 向目标 worker 的 pane 推送通知（可选，取决于 worker 是否运行中）
- 输出：`{ "status": "sent", "message_id": "..." }`

**`zenix_worker_inbox`**
- 参数：`{ "worker_name": "..." }`
- 逻辑：
  1. 从 `SessionManager::search_memory` 搜索 `worker_msg:<worker_name>:*` 
  2. 返回所有未读消息列表
  3. 读取后标记为已读（删除 `worker_msg` 记录或移入 `worker_read_msg`）
- 输出：JSON 数组 `[{ from, text, timestamp }]`

**`zenix_worker_broadcast`**
- 参数：`{ "text": "..." }`
- 逻辑：
  1. 从 `WorkerRegistry::active_workers` 获取所有活跃 worker
  2. 对每个 worker 调用 `worker_send` 逻辑
  3. 保存到所有收件人的 `worker_msg` 中
- 输出：`{ "status": "broadcast", "count": N }`

### MCP 工具注册流程

```rust
fn register_tools(handler: &mut McpToolHandler) -> Vec<Tool> {
    vec![
        Tool {
            name: "zenix_spawn_worker".into(),
            description: Some("在 herdr 中创建 worker pane 并启动 Agent CLI".into()),
            input_schema: spawn_worker_schema(),  // 从 Tool::parameters() 转换
            ..Default::default()
        },
        // ... 其余 9 个工具
    ]
}
```

### 与 OrchestratorEngine 的关系

| 维度 | Duality |
|------|---------|
| **编排器内部** | `chat_with_tools()` 直接调用 `ToolExecutor`，不经过 MCP 协议 |
| **Worker 外部** | 通过 MCP 协议调用同一组 `ToolExecutor` |
| **共享状态** | `ToolExecutor` 使用 `Arc<Mutex<>>` 保护的 herdr_client，MCP 调用和编排器内部调用共享同一连接 |
| **互斥** | 一个请求持有 herdr_client 的锁，其他请求等待 |

## Worker 通信架构

### 消息流

```
Worker A (通过 MCP)                    Worker B (通过 MCP)
    │                                       │
    │  zenix_worker_send("Worker B", ...)    │
    ├───────────────────────────────────────►│
    │                                       │
    │                                       │  zenix_worker_inbox("Worker B")
    │                                       │◄── returns [{from: "Worker A", text: ...}]
    │                                       │
    │  zenix_worker_send("Worker A", ...)    │
    │◄───────────────────────────────────────┤
```

### 编排器中继

编排器作为"智能中继"时的工作流：

```
1. Worker A 完成 → 输出包含 API 契约
2. 编排器 read_output → 理解契约内容
3. 编排器 spawn_worker B → 指令中注入提炼后的契约
4. Worker B 开始工作 → 知道 A 的输出
```

这种模式下编排器**不是消息管道**，而是**理解后再注入**，信息密度更高、上下文更干净。

## 编排验证接口

任何支持 MCP 协议的客户端都可以通过以下方式验证编排系统：

### 验证流程

```
Client                              MCP Server (zenix)
  │                                       │
  │── initialize ────────────────────────►│
  │◄── InitializeResult ─────────────────│
  │                                       │
  │── tools/list ────────────────────────►│
  │◄── [zenix_spawn_worker, ...,          │
  │     zenix_worker_list, ...] ──────────│
  │                                       │
  │── tools/call(zenix_spawn_worker,      │
  │     {agent:"claude", ...}) ───────────►│── herdr IPC ──► herdr
  │◄── {pane_id, terminal_id, ...} ───────│
  │                                       │
  │── tools/call(zenix_read_output, ...) ─►│
  │◄── {output: "..."} ──────────────────│
  │                                       │
  │── tools/call(zenix_close_worker, ...) ►│
  │◄── {status: "closed"} ───────────────│
```

### 验证接口的适用场景

| 场景 | 方式 | 说明 |
|------|------|------|
| 开发者手动验证 | 任何 MCP 客户端（Claude Code MCP, MCP Inspector） | 连接后直接调工具 |
| CI/CD 验证 | 脚本使用 MCP 客户端 SDK | 自动化编排流程测试 |
| AI 代理验证 | AI 代理通过 MCP 协议连接后调用 | 验证编排系统是否正常工作 |

## 配置

```rust
/// MCP Server 配置
pub struct McpServerConfig {
    /// Stdio 模式启用（默认 true）
    pub stdio_enabled: bool,
    /// SSE/Unix Socket 模式启用（默认 false）
    pub sse_enabled: bool,
    /// SSE listener 路径（Unix socket）
    pub socket_path: PathBuf,
}
```

配置存储在 `~/.config/zenix/config.json` 中，UI 设置面板提供开关。

## 兼容性

- **现有的 `Tool` trait 不动**：MCP handler 调用 `ToolExecutor::execute`，参数 schema 从 `tool.parameters()` 获取
- **`chat_with_tools` 不变**：编排器内部仍然直接调用 `ToolExecutor`
- **`SessionManager` 复用**：消息存储复用已有的 SQLite `memories` 表
- **`WorkerRegistry` 复用**：worker 状态跟踪不受影响

## 未来展望

1. **Spawn 注入 MCP 配置**：`spawn_worker` 指令中自动附加 MCP Server 连接信息，worker 开箱即用
2. **事件推送**：MCP `notifications` 机制推送 worker 状态变化（需评估 herdr `events.subscribe` 集成）
3. **消息持久化清理**：worker 通信消息的 TTL 自动清理策略
4. **任务增强工具**：利用 rmcp 的 Task Augmented Tool Calling 实现长时间运行的编排任务

## 回滚与安全性

- 如果 MCP Server 启动失败，zenix 应降级运行（仅编排器内部工具可用）
- MCP Server 崩溃不应影响 zenix 主进程（隔离的 tokio task）
- Unix socket 设置 `0700` 权限防止未授权访问
