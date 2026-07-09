# 工具系统重构 — 执行计划

## 执行顺序

### Step 1: `src/ai/herdr.rs` — 新增 pane.close / tab.close IPC

**变更**：
- `HerdrMethod` 枚举添加 `PaneClose(PaneTarget)` 和 `TabClose(TabTarget)` 变体
- `HerdrIpcClient` 添加 `close_pane(pane_id)` 和 `close_tab(tab_id)` 方法
- 参考 `close_workspace` 的实现模式

**验证**：
```bash
cargo check
cargo test -p zenix -- ai::herdr  # 测试通过
```

**风险**：低。纯新增，不修改已有行为。

### Step 2: `src/ai/session.rs` — 添加 memories 表 + FTS5

**变更**：
- 在 `SCHEMA_SQL` 尾部追加 `memories` 表 + `memories_fts` FTS5 索引 + 三个触发器（INSERT/DELETE/UPDATE 同步）
- `SessionManager` 添加 `write_memory()`, `read_memory()`, `search_memory()`, `delete_memory()` 方法
- 新增 tests

**验证**：
```bash
cargo check
cargo test -p zenix -- ai::session  # 新+旧测试通过
```

**风险**：低。纯新增表和方法。

### Step 3: 创建 `src/ai/file_ops.rs` — 提取文件操作函数

**变更**：
- 新建 `src/ai/file_ops.rs`，从 `tools.rs` 提取 `read_file_contents`, `list_directory_recursive`, `search_files_recursive`, `matches_glob`, `glob_match` 等纯函数
- 保留原测试逻辑，适配为直接函数调用测试
- 不从 `tools.rs` 删除代码（Step 4 才删除）

**验证**：
```bash
cargo check
```

**风险**：低。纯代码提取，不修改行为。

### Step 4: `src/ai/tools.rs` — 重写工具系统

**变更最大的一步**，涉及多处联调：

**4a. 修改 `Tool` trait**：
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult;
}
```
移除 `ToolContext` 结构体。

**4b. 重构 `ToolExecutor`**：
```rust
pub struct ToolExecutor {
    herdr_client: Arc<Mutex<HerdrIpcClient>>,
    workspace_id: String,
    session_manager: Arc<Mutex<SessionManager>>,
    // grid_state: 可选，追踪网格布局状态
}
```
更新 `available_tools()` 方法。

**4c. 删除旧工具实现**（12 个结构体 + execute + tests）：
- `ReadFile`, `ReadDirectory`, `SearchFiles` → 已移到 file_ops.rs
- `SplitPane`, `ArrangeGrid`, `ListAgents`, `StartAgent`, `SendInstruction`, `ReadOutput` → 删除
- `ReadMemory`, `WriteMemory`, `SearchMemory` → 删除

**4d. 实现 6 个新工具**：

| 工具 | 文件名函数 | 关键点 |
|------|-----------|--------|
| `SpawnWorker` | 从现有 `StartAgent` 改造 | 添加 position(row,col) 参数；网格布局算法 |
| `SendText` | 从现有 `SendInstruction` 改造 | 使用 pane.send_text（非 agent.send）|
| `ReadOutput` | 从现有 `ReadOutput` 改造 | 使用 pane.read（非 agent.read）|
| `WaitForOutput` | 新增 | 封装 pane.wait_for_output |
| `ListWorkers` | 从现有 `ListAgents` 改造 | list + 简化输出格式 |
| `CloseWorker` | 新增 | 封装 close_pane |

**4e. 更新 `builtin_tools()` 注册表**：只返回 6 个工具

**4f. 更新 tests**：
- 重现文件操作的测试（移到 file_ops.rs）
- 为 6 个新工具写测试（SpawnWorker/CloseWorker 需要 mock herdr）
- SendText/ReadOutput/ListWorkers 可构造 HerdrIpcClient 测试（连假的 socket）
- WaitForOutput 测试参数验证

**验证**：
```bash
cargo check
cargo test
# 预期：tools.rs 原有测试需要迁移或重写
```

**风险**：高。Tool trait 签名变更影响所有引用处 + engine.rs 联调。

**回滚点**：Step 4 前打 tag。如果编译失败，检查：
- `ToolContext` 的所有引用是否已替换
- `builtin_tools()` 返回值是否适配
- 旧工具导出路径（mod.rs）是否清理

### Step 5: `src/ai/engine.rs` — 清理 dispatch 代码 + 修复 tool payload + 更新 system prompt

**注意**：审查发现 `chat_stream_with_tools()` 发送空 HTTP POST（无请求体）。此步骤同时修复该缺陷。

**5a. SSE 解析提取 + 删除 chat_stream_with_tools**

将 `chat_stream_with_tools()` 中的 SSE 流迭代/解析逻辑提取为独立函数：

```rust
/// Parse an SSE stream from an LLM tool-calling response.
/// Returns collected tool_calls.
async fn parse_tool_call_sse_stream(
    resp: reqwest::Response,
    on_event: impl Fn(StreamEvent) + Send + 'static,
) -> Result<Vec<(String, serde_json::Value)>, AiError>
```

提取后删除 `chat_stream_with_tools()` 方法。

**5b. 重构 `chat_with_tools()`**（engine.rs:1077-1202）

循环结构改为直接在循环内构建 payload + 发送 HTTP：

```
loop:
  1. 保留 compaction check
  2. prepend system prompt（如果 messages 第一条不是 system）
  3. build_tool_call_payload(messages, tools, model) → payload
  4. http_client.post(endpoint).json(&payload).send() → resp
  5. parse_tool_call_sse_stream(resp, on_event) → tool_calls
  6. tool_calls 为空 → 返回 collected text
  7. executor.execute(tc) → 追加 tool_result → 继续
```

关键：此步骤使 `chat_with_tools()` 端到端可用（之前因空 POST 不可用）。

**5c. 删除 dispatch 相关的函数**：
- `chat_and_dispatch()` 整个方法
- `parse_dispatch_directive()` + 相关解析函数
- `classify_agent_error()`
- `call_llm_collect()`
- `DEFAULT_SYSTEM_PROMPT`（不再需要）

**5d. 更新 `build_system_prompt()`**：
- 删除 `## Agent Dispatch 格式` 段落
- 6 个工具的描述简洁列出

**5e. 保留类型**：
- `AgentTask`、`AgentResult`、`DispatchDirective`、`DispatchStreamEvent`、`CompactionInfo`
- `execute_agent_task()` 函数

**验证**：
```bash
cargo check
cargo test
```

**风险**：中。删除的 dispatch 解析函数无其他引用（先 grep 确认）。

### Step 6: `src/ai/mod.rs` — 更新导出

**变更**：
- 添加 `mod file_ops;`
- 更新 `pub use tools::*` 为只导出 6 个新工具 + ToolExecutor + ToolCall/ToolResult
- 删除旧的导出（`builtin_tools`, `execute_tool_call`, `find_tool` 等酌情保留内部使用）
- 移除 `#[allow(unused_imports)]`

**验证**：
```bash
cargo check
```

### Step 7: `src/app.rs` — 切换调用路径

**变更**：
- `start_chat_streaming()` 中 `chat_and_dispatch` 替换为 `chat_with_tools`
- 构建 `ToolExecutor` 和 tools 引用列表
- 验证 `DispatchStreamEvent` 的 handle_dispatch_event 分支

**关键代码位置**：app.rs:3198

**验证**：
```bash
cargo check
```

**风险**：中。运行时行为变更需要实际测试。

### Step 8: `src/ai/engine.rs` — 清理 StreamEvent 死分支

**变更**：
- 检查 `StreamEvent::ToolCall` / `StreamEvent::ToolResult` 的旧使用路径
- 确认 `chat_stream_with_tools` 输出正确处理
- 移除 `#![allow(dead_code)]`

### Step 9: 最终质量检查

```bash
cargo check
cargo fmt --check
cargo clippy
cargo test
```

修复所有警告和 lint 问题。

## 回滚点

| Step | 回滚方式 | 条件 |
|------|---------|------|
| Step 1 | 删除新增方法 | 编译/测试失败 |
| Step 2 | 不修改 schema 版本号，删除表 | 编译/测试失败 |
| Step 3 | 不引用 file_ops.rs 即可 | 编译失败 |
| Step 4 | **主要回滚点**。先 stash 或 commit | Tool trait 变更后多文件受影响 |
| Step 5 | 恢复删除的函数 | 编译/功能失败 |
| Step 6 | 恢复 mod.rs | 编译失败 |
| Step 7 | 切回 chat_and_dispatch | 运行时异常 |

## 验证命令速查

```bash
# 每次变更后
cargo check 2>&1 | head -80

# 跑测试
cargo test -p zenix -- ai::tools 2>&1 | tail -20
cargo test -p zenix -- ai::session 2>&1 | tail -20
cargo test -p zenix -- ai::engine 2>&1 | tail -20

# 最终
cargo fmt --check && cargo clippy 2>&1 | tail -30
cargo test 2>&1 | tail -30
```

## 需要确认的点（implement 前）

- [ ] `DispatchStreamEvent` 中 `ToolCall`/`ToolResult` 分支在 UI 侧（handle_dispatch_event）的行为：现有代码是否处理它们？是否需要调整？
- [ ] `chat_with_tools` 的第一次 LLM 调用时，系统提示如何传入？当前 `chat_and_dispatch` 在 `build_system_prompt` 中注入，`chat_with_tools` 也是从 engine 的 `system_prompt` 字段读取
- [ ] grid 布局算法：当 position 超出当前 pane 数时，是否需要重排现有 panes，还是只追加？
