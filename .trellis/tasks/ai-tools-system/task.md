# AI 内置工具系统

## 目标
实现 ZenixAI 编排器的内置工具系统，让 LLM 通过 function calling 调用工具操作 herdr IPC、文件读写、记忆管理等，替代硬编码的 `chat_and_dispatch` 调度逻辑。

## 子任务列表

| # | 名称 | 状态 |
|---|------|------|
| 01 | 工具系统定义 + 文件操作工具 | ✅ 完成 |
| 02 | Function Calling 循环改造 | ✅ 完成 |
| 03 | 记忆管理 + Agent 管理 + 布局控制工具 | ✅ 完成 |
| 04 | 会话压缩 | ✅ 完成 |
| 05 | 配置隔离检查 | ✅ 完成 |

## 设计参考
- Spec §10.6: `src/ai/tools.rs` 定义 Tool trait + 12 个内置工具
- pi_agent_rust `src/tools.rs`: 工具 trait 设计、JSON Schema 参数
- pi_agent_rust `src/agent.rs`: Function Calling 循环架构
