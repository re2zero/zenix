# 修复编排功能：thinking合并/工具循环超时/herdr连接

## Goal

修复 `chat_with_tools` 编排循环中的 3 个 bug，使 AI 聊天能正常进行多轮工具调用并将任务分发给 worker agent。

## Background

`chat_with_tools()` 在 `src/ai/engine.rs` 中实现了带 function calling 的 LLM 对话循环。用户测试"帮我问一下 opencode 洗车问题"时暴露了 3 个问题。

## Root Causes

### Issue 1: 多轮思考内容合并到第一个 block

**根因**: `DispatchStreamEvent::Thinking(String)` 没有携带 `tool_round` 信息。所有轮次的 thinking 追加到同一个 `msg.reasoning_text`。

### Issue 2: 工具调用循环 10 次超时

**根因**: tool call 消息格式错误。
- LLM 返回 tool_calls 后，assistant 消息（含 tool_calls）**未加入** `loop_messages`
- `ChatMessageRef` 缺少 `tool_calls` / `tool_call_id` 字段
- `parse_sse_tool_calls()` 丢弃了 `SseToolCall.id`（tool_call_id）
- 工具结果格式为 `{role: "tool", content: "..."}` 而非标准 `{role: "tool", tool_call_id: "...", content: "..."}`
- LLM 无法理解消息上下文，反复重试直到超时

### Issue 3: herdr 连接失败 / pane 创建失败

**根因**: `ToolExecutor` 中没有前置连接检查，herdr 不可达时工具反复失败。

## Requirements

### R1: 思考内容按轮次分离
- `DispatchStreamEvent::Thinking` 增加 `round: u32` 字段
- `chat_with_tools` 在每轮循环中传入当前 `tool_round`
- UI 端按 round 渲染不同的 reasoning block

### R2: 修复 tool call 消息格式
- `ChatMessageRef` 增加可选 `tool_calls` 和 `tool_call_id` 字段
- `ToolCall` 增加 `id: String` 字段（保存 SSE 的 tool_call_id）
- 在工具结果加入 `loop_messages` 前，先加入 assistant 的 tool_calls 消息
- 工具结果格式增加 `tool_call_id`

### R3: herdr 连接预检
- 工具执行前调用 `client.connect()` 确认 herdr 可达
- herdr 不可达时给 LLM 清晰的终止信号

## Out of Scope

- 完整重写编排系统
- MCP Server 相关问题

## Acceptance Criteria

- [ ] AC1: 多轮工具调用的 thinking 分别显示在不同 block 中
- [ ] AC2: `cargo check` 通过，无新增警告
- [ ] AC3: LLM 返回 tool_calls 后，消息格式符合 OpenAI API 规范
- [ ] AC4: 工具结果正确关联到原始 tool_call_id
- [ ] AC5: herdr 不可达时工具返回清晰错误

## Notes

- 三个 issue 相互独立，可分别修改并测试
- Issue 2 是最关键修复，需要同时修改 engine.rs 和 tools.rs
