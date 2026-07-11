# 修复 MCP 显示/AI 语言/编排调度 3 个遗留问题

## Goal

修复 MCP 通信总线 + 编排验证接口实现后遗留的 3 个问题：
1. 内置 MCP 服务（zenix-orchestrator）在设置面板中可见
2. AI 回复语言与输入一致（输入中文→回复中文）
3. 编排功能能正确识别并分发任务到 worker

## Background

上一个 task (`07-09-mcp-orchestration-bus`) 实现了 MCP 通信总线和编排验证接口，包括：
- MCP Server（stdio + SSE 传输）
- 10 个 MCP 工具暴露（zenix_spawn_worker, zenix_send_text 等）
- 内置 zenix-orchestrator MCP 服务注册
- 编排器与 herdr IPC 集成

但实现后用户发现 3 个问题未达到预期效果。

## Requirements

### R1: 内置 MCP 服务在设置面板中可见

**问题**: 设置面板的 MCP tab 中没有显示内置的 "zenix-orchestrator" 服务。

**要求**:
- `ensure_builtin_orchestrator()` 已在 `src/mcp/mod.rs` 中实现，能注册内置服务到 zenix mcp.json
- 设置面板的 MCP tab 需要加载并显示所有 zenix-managed MCP servers（包括内置的）
- 用户应能在设置面板中看到 "zenix-orchestrator" 服务及其状态
- 应支持查看/切换内置服务的 agent 关联

### R2: AI 回复语言与输入一致

**问题**: 用户输入中文时，AI 回复了其他语言（如英文）。

**要求**:
- 系统 prompt 中应包含语言指令，告知 AI 使用与用户输入相同的语言回复
- 或者检测用户输入语言后在请求中动态设置 language 参数
- 不强制用户配置语言偏好，自动跟随输入语言
- 兼容所有支持的 AI provider（OpenAI 格式 API）

### R3: 编排功能正确分发任务

**问题**: 用户输入"帮我问一下 opencode 以下问题：我家里洗车店100米，需要洗车，是开车去还是走路去？"时，没有被正确分配到 worker。

**要求**:
- 编排器需要识别适合 worker 处理的任务（如需要外部工具、长时间运行、或需要特定 agent 执行的任务）
- 简单问题（如直接问答、无需工具）应直接回答而不分配 worker
- "帮我问一下 X" 这种委托式请求应触发 spawn_worker 并分配
- 编排器的判定逻辑需要更智能地识别"代为询问/执行"的意图
- 考虑 MCP tool 调用 vs 内部 tool calling 的路由逻辑

### Out of Scope

- AI 聊天面板的完整重构（已有单独规划）
- MCP Server 的自动启停策略
- Worker 间 P2P 通信

## Acceptance Criteria

- [ ] AC1: 打开设置面板 → MCP tab → 显示 "zenix-orchestrator" 服务
- [ ] AC2: 可以查看 zenix-orchestrator 的详情（类型、command、关联 agent）
- [ ] AC3: 输入中文"你好" → AI 回复中文
- [ ] AC4: 输入英文"hello" → AI 回复英文（不强制但不应中文输入返回英文）
- [ ] AC5: 输入"帮我问一下 opencode 关于洗车的问题" → 触发 worker 分配
- [ ] AC6: 简单问题如"什么是 MCP" → 直接回答，不分配 worker
- [ ] AC7: `cargo check` 通过，无新增警告

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
