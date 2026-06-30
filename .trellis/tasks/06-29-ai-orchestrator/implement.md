# 多Agent编排调度 — 执行计划

## 子任务

10 个子任务各自独立可验证，按依赖顺序编排。

| 任务 | 名称 | 优先级 | 上游依赖 | 产出 |
|------|------|--------|---------|------|
| 01 | SQLite 持久化 | P1 | 无 | `ai/session.rs` |
| 02 | ZenixAI 配置 | P1 | 无 | `ai/config.rs` + 设置面板扩展 |
| 10 | 异常处理 | P1 | 无 | `ai/error.rs`（AiError, retry, user_facing_error） |
| 03 | 编排器引擎 | P1 | 01(ChatMessage), 10(AiError) | `ai/engine.rs`（SSE + chat_stream/chat_simple） |
| 05 | Herdr IPC 客户端 | P2 | 10(AiError 可选) | `ai/herdr.rs` |
| 06 | Agent 调度 | P2 | 03(engine), 05(herdr), 01(AgentActivity) | `ai/engine.rs`（chat_and_dispatch） |
| 04 | 聊天面板 UI | P1 | 01, 02, 03, 05(workspace), 06(dispatch) | `ai/chat.rs` + `sidebar.rs` + `app.rs` |
| 07 | Agent 输出 UI | P2 | 04, 06 | `ai/chat.rs`（AgentBlock 组件） |
| 09 | API 配置同步 | P3 | 无 | `api_provider/mod.rs`（批量 sync） |
| 08 | Agent 监控 UI | P3 | 05, 07 | `ai/chat.rs`（👤 浮层） |

## 依赖图

```
Phase 1:  01(SQLite)   02(Config)   10(Error)  09(ConfigSync)
              │             │                       │
              ▼             ▼                       │
Phase 2:  03(Engine) ←────┤                        │
           │               │                        │
           ▼               │                        │
Phase 3:  05(Herdr IPC)   │                        │
           │               │                        │
           ▼               ▼                        │
Phase 4:  06(Dispatch)                             │
           │                                       │
           ▼                                       │
Phase 5:  04(Chat Panel UI) ←──────────────────────┘
           │
           ▼
Phase 6:  07(Agent Output UI)   08(Agent Monitoring)
```

## MVP 范围

**首次迭代**: 01 → 02 → 10 → 03 → 04
- 纯聊天面板 + 编排器 LLM 对话
- 无 herdr agent 集成
- 会话持久化 + 搜索

**第二次迭代**: 05 → 06 → 04(集成chat_and_dispatch)
- Herdr IPC 连接
- 编排器自动调度 agent
- 工作区选择

**第三次迭代**: 07 → 08 → 09
- Agent 内联输出显示
- 运行监控
- 配置同步增强
