1. `src/ai/engine.rs` 中：
   - 替换任务 03 的 `AgentTask` / `AgentResult` 占位为完整实现（移除 `#[allow(dead_code)]`）
   - 添加 `ChatAndDispatchResult` 结构体
   - 添加 `ORCHESTRATOR_SYSTEM_PROMPT` 常量
   - 实现 `parse_agent_tasks()` → `<dispatch>` 标签解析
   - 实现 `dispatch_single()` / `dispatch_all()`
   - 实现 `chat_and_dispatch()` — 编排器全周期（封装 chat_stream → parse → dispatch → summarize）
   - 实现 `summarize_results()`
2. `cargo check` 验证

## Implementation Steps

### Step 1: src/ai/engine.rs
- 将 `AgentTask` / `AgentResult` 占位改为不带 `#[allow(dead_code)]` 的完整实现，补 `Display` impl
- 新增 `ChatAndDispatchResult` struct（orchestrator_text, agent_results, summary）
- 新增 `ORCHESTRATOR_SYSTEM_PROMPT` 常量
- 新增 `parse_agent_tasks(reply: &str) -> Vec<AgentTask>`（使用 regex）
- 新增 `chat_and_dispatch()` — 完整编排周期：
  1. `chat_stream()` 流式输出到 UI
  2. 收集完整回复 → `parse_agent_tasks()` 提取任务
  3. `dispatch_all()` 执行任务（emit AgentDispatch/AgentComplete 事件）
  4. `summarize_results()` 汇总
  5. 返回 `ChatAndDispatchResult`
- 新增 `dispatch_all()` / `dispatch_single()`
- 新增 `summarize_results()`（调 `chat_simple()`）

> 注意：`regex` 依赖已在任务 03 的 Cargo.toml 中添加，此处不需要重复加。

### Step 2: 验证
- `cargo check`
