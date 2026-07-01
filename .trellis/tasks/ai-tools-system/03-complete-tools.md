# 子任务 03: 实现 Agent 管理 + 布局控制 + 记忆管理工具

## 目标
实现子任务 01 中定义的 9 个工具空壳（Agent 管理 4 个 + 布局控制 2 个 + 记忆管理 3 个），让工具系统完整可用。

## 背景
- 子任务 01: 工具系统定义 + 文件操作工具 ✅
- 子任务 02: Function Calling 循环 ✅
- 现在需要实现剩余 9 个工具

## 实现内容

### 1. Agent 管理工具（4 个）

这些工具通过 `ToolExecutor` 持有的 `Arc<Mutex<HerdrIpcClient>>` 调用 herdr IPC。

#### `list_agents`
- 参数: 无
- 功能: 调用 herdr `agent.list`，列出所有 agent 状态
- 返回: 格式化的 agent 状态列表

#### `start_agent`
- 参数: `agent` (string, required) — Agent CLI 名称
- 参数: `workspace_id` (string, optional) — workspace 标识
- 参数: `cwd` (string, optional) — 工作目录
- 功能: 调用 herdr `agent.start`，在 workspace 中启动 agent

#### `send_instruction`
- 参数: `pane_id` (string, required) — 窗格 ID
- 参数: `text` (string, required) — 指令文本
- 功能: 调用 herdr `agent.send`，发送文本给 agent

#### `read_output`
- 参数: `pane_id` (string, required) — 窗格 ID
- 参数: `lines` (integer, optional, default 50) — 读取最近 N 行
- 功能: 调用 herdr `agent.read`，读取 agent 输出

### 2. 布局控制工具（2 个）

#### `split_pane`
- 参数: `tab_id` (string, required) — 标签页 ID
- 参数: `direction` (string, optional, default "right") — "left" / "right" / "up" / "down"
- 功能: 调用 herdr `pane.split`
- 参数: `cwd` (string, optional) — 新窗格的工作目录

#### `arrange_grid`
- 参数: `tab_id` (string, required) — 标签页 ID
- 参数: `rows` (integer, optional, default 1) — 行数
- 参数: `cols` (integer, optional, default 1) — 列数
- 功能: 调用 herdr `pane.arrange`，网格排列窗格

### 3. 记忆管理工具（3 个）

这些工具操作 SQLite 数据库（与 session 相同的 DB）。

#### `read_memory`
- 参数: `key` (string, required) — 记忆键
- 功能: 查询 `memories` 表，返回记忆值
- 数据库扩展: 需要新建 `memories` 表

#### `write_memory`
- 参数: `key` (string, required) — 记忆键
- 参数: `value` (string, required) — 记忆值
- 功能: INSERT OR REPLACE 到 `memories` 表

#### `search_memory`
- 参数: `query` (string, required) — 搜索关键词
- 功能: 查询 `memories_fts` 表，返回匹配的记忆

### 4. 数据库扩展

在 `session.rs` 中添加 `memories` 表和 FTS5：

```sql
CREATE TABLE IF NOT EXISTS memories (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    key, value,
    content=memories,
    content_rowid=key
);
```

### 5. ToolExecutor 改造

`ToolExecutor` 需要持有 SessionManager 引用以支持记忆工具：

```rust
pub struct ToolExecutor {
    herdr_client: Arc<Mutex<HerdrIpcClient>>,
    workspace_id: String,
    session_id: Option<String>,
    db_path: PathBuf,  // 用于创建 SessionManager
}
```

### 6. 工具注册更新

`builtin_tools()` 中各工具从 stub 变为完整实现。

### 不做什么
- 不改 function calling 循环（子任务 02 已完成）
- 不改 UI（chat.rs）
- 不实现会话压缩（子任务 04）

## 测试要求
- list_agents: 测试 herdr IPC 调用（mock）
- start_agent: 测试 herdr IPC 调用
- send_instruction: 测试 herdr IPC 调用
- read_output: 测试 herdr IPC 调用
- split_pane: 测试参数验证
- arrange_grid: 测试参数验证
- write_memory / read_memory: 测试 SQLite
- search_memory: 测试 FTS5

## 验证标准
- ✅ `cargo check` 编译通过
- ✅ 所有测试通过
- ✅ `cargo clippy` 无新增警告
