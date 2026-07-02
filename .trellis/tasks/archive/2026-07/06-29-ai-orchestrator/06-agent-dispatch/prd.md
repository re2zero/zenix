# 编排器调度逻辑

## 目标

为编排器设计 system prompt，使其能理解用户需求、分解为 agent 任务、通过 herdr pane 分发给 Agent CLI 执行，并汇总结果。

## 接口契约（下游 07、08 须知）

```rust
// 扩展 engine.rs 中的数据结构

/// 编排器决定分发 agent 任务时发出的事件
pub struct AgentTask {
    pub agent_name: String,      // "claude" | "opencode" | "codex"
    pub instruction: String,     // 给 agent 的完整指令
    pub cwd: String,             // 工作目录
    pub pane_id: Option<String>, // 由 dispatch 模块分配
}

/// Agent 执行结果
pub struct AgentResult {
    pub agent_name: String,
    pub pane_id: String,
    pub exit_status: String,     // "completed" | "failed" | "timeout"
    pub output: String,          // 完整输出
    pub summary: Option<String>, // 编排器生成的摘要
}
```

## System Prompt 设计

```markdown
你是 ZenixAI 编排器。你的职责：
1. 理解用户需求
2. 分解为独立的任务
3. 将每个任务分配给合适的 Agent CLI（claude / opencode / codex）
4. 监控执行进度
5. 汇总结果回复用户

调度规则：
- 每次只需要启动一个 agent 时，直接分配
- 需要多人并行时，说明并行任务
- Agent 执行失败时，分析错误原因并重试或切换 agent
- 所有 agent 执行完成后，给出简洁的汇总

Agent CLI 能力：
- claude: 复杂开发任务、代码生成、调试
- opencode: 配置管理、简单代码变更
- codex: 通用任务

可用工具（由系统提供）：
- 创建 pane 并启动 agent
- 发送指令到 agent pane
- 读取 agent 输出
- 等待 agent 完成
```

## 调度流程

```
编排器收到 user message
  → 调用 LLM（带 system prompt + 消息历史）
  → LLM 回复包含调度标记
  → parse_agent_tasks() 提取 AgentTask 列表
  → 对每个 AgentTask:
    1. HerdrIpcClient.create_pane(cwd)
    2. HerdrIpcClient.start_agent(agent_name)
    3. HerdrIpcClient.send_text(instruction)
    4. HerdrIpcClient.wait_for_output(completion_marker)
    5. HerdrIpcClient.read_output("recent")
  → 收集所有 AgentResult
  → 调用 LLM 汇总结果
  → 回复用户
```

## Requirements

- [ ] 编排器 system prompt（在 engine.rs 中定义常量 `ORCHESTRATOR_SYSTEM_PROMPT`）
- [ ] `parse_agent_tasks()` 从 LLM 回复中提取调度指令
- [ ] `dispatch_agent_task()` 执行单个 agent 任务的全流程（create pane → start → send → wait → read）
- [ ] `dispatch_all()` 管理多个 agent 的执行（可串行，可先跳过并行）
- [ ] 结果汇总（调用 chat_simple() 让编排器写总结）
- [ ] 错误处理（agent 失败 → 重试或上报）

## Acceptance Criteria

- [ ] 编排器 system prompt 清晰定义了调度职责
- [ ] `dispatch_agent_task()` 可在 herdr 中完整执行一个 agent 任务
- [ ] Agent 执行结果正确传回编排器
- [ ] 编排器能整理结果生成 human-readable 回复

## Out of Scope

- 并行 agent 调度（后续迭代）
- agent 执行的实时 streaming（交给 07）
- 超时和重试策略（交给 10）
