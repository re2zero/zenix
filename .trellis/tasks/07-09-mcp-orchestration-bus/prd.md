# MCP 通信总线 + 编排验证接口

## Goal

在 zenix 中启动内置 MCP Server，将编排工具（spawn_worker、send_text、read_output、wait_for_output、list_workers、close_worker）通过 MCP 协议对外暴露，使 herdr 中运行的 Agent CLI（Claude Code、Codex、OpenCode 等）能够通过 MCP 工具与编排器通信、worker 间互相通信，并提供外部 AI 代理的编排验证接口。

## Requirements

### R1: MCP Server 基础架构
- zenix 启动时在内置 tokio runtime 上运行一个 MCP Server
- 支持 **stdio 传输**（zenix 子进程直接连接）和可选的 **SSE 传输**（外部代理通过网络连接）
- 实现 MCP 协议握手（`initialize`/`initialized`）、工具列表（`tools/list`）、工具调用（`tools/call`）
- MCP Server 生命周期绑定到 zenix 主进程（zenix 退出则停止）
- 使用 `rmcp` 或等效 Rust MCP SDK，避免从零实现 JSON-RPC 协议

### R2: 编排工具暴露为 MCP Tools
- 现有的 6 个编排工具（spawn_worker、send_text、read_output、wait_for_output、list_workers、close_worker）通过 MCP 对外暴露
- 工具名称不加前缀，MCP Server 自身提供命名空间隔离
- MCP 工具调用时通过 `ToolExecutor` 执行，复用已有的 herdr IPC 逻辑
- 参数 schema 与现有 function calling 定义一致，保持单一事实来源

### R3: Worker 通信工具
- 新增 MCP 工具使 worker 能相互发现和通信：

| 工具 | 参数 | 用途 |
|------|------|------|
| `worker_list` | — | 查看所有 worker 及其状态 |
| `worker_send` | `worker_name`, `text` | 发送消息给指定 worker |
| `worker_inbox` | — | 读取自己的未读消息 |
| `worker_broadcast` | `text` | 广播消息给所有 worker |

- 消息存储使用已有的 SQLite `memories` 表（SessionManager），保证持久化和重启恢复
- 消息格式：`{from, to, text, timestamp}`

### R4: 编排验证接口（调试接口）
- 外部 AI 代理（Claude Code、Codex、OpenCode 等）通过 MCP 协议连接到 zenix MCP Server
- Agent 的 LLM 生成调用编排工具的 prompt，执行完整编排流程
- LLM 根据工具返回的结构化 JSON 结果自动判断编排系统是否正常
- 10 个 MCP 工具本身即为验证接口，无需额外专用工具

### R5: Spawn 时注入 MCP 配置（前瞻设计）
- `spawn_worker` 工具创建 worker 时，指令中自动附加 MCP 连接信息
- worker 知道如何连接到 zenix MCP Server 并使用通信工具
- *本任务中只做注入机制设计，不强制所有 Agent CLI 消费*

### R6: 兼容性与约束
- 不破坏现有的编排器 function calling 工作流
- `ToolExecutor` 保持线程安全（`Arc<Mutex<>>`），MCP 调用与编排器内部调用共享同一 herdr 连接
- 不引入新的外部 HTTP server 依赖（MCP stdio 模式足够）

## Acceptance Criteria

- [ ] AC1: zenix 启动后 MCP Server 在指定路径（Unix socket 或 stdio）上可用
- [ ] AC2: 任意 MCP 客户端（如 Claude Code MCP client）连接后能列出 10 个工具
- [ ] AC3: 通过 MCP 调用 `spawn_worker` 能在 herdr 中创建 worker pane
- [ ] AC4: 通过 MCP 调用 `send_text` 能向 worker 发送指令
- [ ] AC5: 通过 MCP 调用 `read_output` 能读取 worker 输出
- [ ] AC6: 通过 MCP 调用 `wait_for_output` 能等待 worker 完成
- [ ] AC7: 通过 MCP 调用 `list_workers` 能列出运行中 workers
- [ ] AC8: 通过 MCP 调用 `close_worker` 能关闭 worker pane
- [ ] AC9: Worker A 通过 `worker_send` 发送的消息，Worker B 通过 `worker_inbox` 能收到
- [ ] AC10: 外部 AI 代理连接 MCP Server 后能完成"spawn → wait → read → close"编排流程并得到正确结果
- [ ] AC11: MCP Server 在 zenix 退出时自动停止，无残留进程
- [ ] AC12: 所有 MCP 工具的 JSON schema 与现有 function calling 工具定义保持同步

## Out of Scope

- Worker 间直接 P2P 通信（不经过 MCP Server 中继）
- Agent CLI 侧自动消费 MCP 工具（spawn 注入机制设计但不强制实现）
- REST API 或 WebSocket 替代方案（MCP 协议足够）
- MCP 鉴权/加密（本地 socket/stdio，无网络暴露风险）

## Notes

- 当前 herdr IPC 已支持 `pane.close`，`close_worker` 工具可直接使用
- 记忆存储已迁移到 SQLite，worker 通信消息复用同一存储层
- 使用 `rmcp` crate 实现 MCP 协议，避免自实现 JSON-RPC
