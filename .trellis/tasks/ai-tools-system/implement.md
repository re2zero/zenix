# 子任务 01: 工具系统定义 + 文件操作工具

## 目标
创建 `ai/tools.rs`，定义 `Tool` trait 和 `ToolCall`/`ToolResult` 类型，实现 3 个文件操作工具：`read_file`、`read_directory`、`search_files`。

## 产出
- `src/ai/tools.rs` — 工具系统核心文件
- 12 个工具的完整定义（P0 阶段只实现文件操作部分，其他工具先定义空壳或标记 TODO）
- 单元测试

## 详细设计

### 1. 核心类型

```rust
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,  // 工具的具体参数
}

pub struct ToolResult {
    pub tool_name: String,
    pub content: String,      // 工具返回结果
    pub is_error: bool,       // 是否出错
}

pub trait Tool: Send + Sync {
    /// 工具名（LLM 调用时用）
    fn name(&self) -> &str;
    /// 工具描述（写入 LLM 的系统提示）
    fn description(&self) -> &str;
    /// 工具参数的 JSON Schema（function calling 用）
    fn parameters(&self) -> serde_json::Value;
    /// 执行工具
    fn execute(&self, args: serde_json::Value) -> ToolResult;
}
```

### 2. 文件操作工具（3 个）

#### `read_file`
- 参数: `path` (string, required) — 文件路径
- 限制: 最大读取 50KB，超过截断并提示
- 错误处理: 文件不存在 → 友好的错误消息

#### `read_directory`
- 参数: `path` (string, required) — 目录路径
- 递归列出目录结构，限制最大深度 3 层
- 输出格式: 类似 `tree` 的文本表示

#### `search_files`
- 参数: `path` (string, required) — 搜索路径
- 参数: `pattern` (string, required) — glob 模式
- 参数: `max_results` (integer, optional, default 50) — 最大结果数
- 使用 `glob` crate 或简单实现

### 3. 其他工具（标记为 TODO，不实现 execute）

Agent 管理（4 个）: `list_agents`, `start_agent`, `send_instruction`, `read_output`
布局控制（2 个）: `split_pane`, `arrange_grid`
记忆管理（3 个）: `read_memory`, `write_memory`, `search_memory`

### 4. 工具注册

```rust
pub fn builtin_tools() -> Vec<Box<dyn Tool>>;
```

### 5. 集成点

- `ai/mod.rs` 导出 `tools` 模块
- 后续子任务 02 将改造 `engine.rs` 的 `chat_and_dispatch` 为工具调用循环

## 不做什么

- 不实现 Agent 管理的工具（依赖 herdr IPC 初始化，子任务 03）
- 不实现记忆管理工具（依赖 SessionManager 集成，子任务 03）
- 不改造 engine.rs 的调度逻辑（子任务 02）

## 验证标准

- ✅ `cargo check` 编译通过
- ✅ `cargo test` 测试通过（至少 8 个测试）
- ✅ `cargo clippy` 无新增警告
- ✅ 3 个文件操作工具功能正确
