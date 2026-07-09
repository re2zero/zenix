# 工具系统重构：清理 + 记忆持久化 + 编排工具增强

## Goal

将 ZenixAI 编排器从旧的文本 JSON dispatch 协议迁移到行业标准的 function calling 工具系统，消除两条并行路径的混乱；将记忆存储从进程内 HashMap 迁移到 SQLite；用 6 个最小编排工具替换现有的 12 个零散工具，覆盖 worker 全生命周期。

## Background

### 架构现状（两条并行调度路径）
- `chat_and_dispatch()` — 旧的 dispatch 路径，解析 LLM 响应中的 JSON dispatch 指令，调度 agent 到 herdr pane
- `chat_with_tools()` — 新的 function calling 工具路径，完整的工具调用循环 + compaction 集成
- **运行时实际调用的是 `chat_and_dispatch()`**，`chat_with_tools()` 未被任何生产代码引用
- `build_system_prompt()` 同时输出了两种格式（`## 可用工具` + `## Agent Dispatch 格式`），造成 LLM 困惑

### 工具系统现状
- `tools.rs`：12 个工具全部完整实现（非 stub），内存使用 HashMap，标记 `#![allow(dead_code)]`
- `session.rs`：无 `memories` 表
- `herdr.rs`：未封装 `pane.close` / `tab.close` IPC 方法

### 编排器工作流（第一性原理推导）

编排器 LLM 的职责循环：用户 → 规划 → spawn workers → 监控 → 汇总 → 清理 → 回复

最小原语恰好 6 个：

| # | 操作 | herdr IPC | 说明 |
|---|------|-----------|------|
| 1 | spawn_worker | agent.start + pane.split | 创建 worker 到网格位置 |
| 2 | send_text | pane.send_text | 发送指令/对话 |
| 3 | read_output | pane.read | 读取输出 |
| 4 | wait_for_output | pane.wait_for_output | 等待完成标记 |
| 5 | list_workers | agent.list / pane.list | 列举运行中 workers |
| 6 | close_worker | pane.close（新增）| 关闭 worker pane |

Worker 间通信不在此次范围。设计文档包含 MCP 总线方案的未来展望。

## Requirements

### R1 — 清理旧 dispatch 路径，启用 function calling
- R1.1 删除 `chat_and_dispatch()`，app.rs 切换到 `chat_with_tools()`
- R1.2 `build_system_prompt()` 只保留工具描述，删除旧 JSON dispatch 格式
- R1.3 删除 dispatch 解析函数：`parse_dispatch_directive()`, `extract_json_block()`, `parse_json_dispatch()`, `parse_text_dispatch()`, `find_pattern()`, `classify_agent_error()`
- R1.4 保留 `AgentTask` / `AgentResult` / `DispatchDirective` 等类型（engine.rs 中 agent 执行结果仍需要）
- R1.5 移除 `#![allow(dead_code)]`（tools.rs, compactor.rs, engine.rs）

### R2 — 记忆使用 SQLite 持久化
- R2.1 在 `session.rs` 中添加 `memories` 表 + `memories_fts` FTS5 索引
- R2.2 `ToolContext.memory` 从 HashMap 替换为 `Arc<Mutex<SessionManager>>`
- R2.3 `ToolExecutor` 持有 SessionManager 引用（或其 DB 路径）
- R2.4 已有记忆数据接口不变（read/write/search），底层存储换为 SQLite

### R3 — 6 个编排工具替换现有 12 工具
- R3.1 新工具注册表只保留 6 工具：`spawn_worker`, `send_text`, `read_output`, `wait_for_output`, `list_workers`, `close_worker`
- R3.2 删除旧工具代码：`ReadFile`, `ReadDirectory`, `SearchFiles`, `SplitPane`, `ArrangeGrid`, `ListAgents`, `StartAgent`, `SendInstruction`, `ReadOutput`, `ReadMemory`, `WriteMemory`, `SearchMemory`
- R3.3 `ReadFile`/`ReadDirectory`/`SearchFiles` 移到独立内部模块（非 Tool trait）
- R3.4 `HerdrIpcClient` 新增 `close_pane()` 方法（pane.close）和 `close_tab()` 方法（tab.close）
- R3.5 `spawn_worker` 支持 `position: {row, col}` 参数，内部自动推导网格布局

## 不做什么
- Worker 间通信（MCP 总线方案放入未来展望）
- Compactor 的 LLM 摘要策略（当前 drop-oldest 已够用）
- UI 层修改（chat panel 消息渲染、样式）
- 会话数据库 schema 变更（sessions/messages 表不变）
- 工具并发执行（串行足以满足 P0）

## Acceptance Criteria

- [ ] AC1: `cargo check` 编译通过，无 `dead_code` 警告
- [ ] AC2: `cargo clippy` 无新增警告
- [ ] AC3: 所有现有测试通过
- [ ] AC4: `chat_and_dispatch()`, `parse_dispatch_directive()`, `extract_json_block()` 等旧代码已移除
- [ ] AC5: `app.rs::start_chat_streaming` 调用 `chat_with_tools()` 而非 `chat_and_dispatch()`
- [ ] AC6: `build_system_prompt()` 只包含工具描述，无旧 dispatch 格式
- [ ] AC7: SQLite `memories` 表 + `memories_fts` 存在，记忆工具读写持久化
- [ ] AC8: `close_pane()` 方法在 HerdrIpcClient 中可用
- [ ] AC9: 6 个编排工具功能正确，对应测试通过
- [ ] AC10: 旧文件操作工具移出 Tool 注册表，降为内部模块

## 未来展望（本篇不进）
- MCP Server 暴露通信工具（worker_send, worker_inbox, worker_broadcast）
- spawn_worker 自动注入 MCP 配置
- Worker 间通过 MCP 协议协作
