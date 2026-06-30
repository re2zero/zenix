# 多Agent编排调度 — 执行计划

## 子任务

10 个子任务各自独立可验证，按依赖顺序编排。

| 任务 | 名称 | 优先级 | 上游依赖 | 产出 |
|------|------|--------|---------|------|
| 01 | SQLite 持久化 | P1 | 无 | `ai/session.rs` | ✅ 已完成 (commit 2c4c3ec) |
| 02 | ZenixAI 配置 | P1 | 无 | `ai/config.rs` + 设置面板扩展 | ✅ 已完成 (commit 529717b) |
| 10 | 异常处理 | P1 | 无 | `ai/error.rs`（AiError, retry, user_facing_error） | ✅ 已完成 (commit 529717b) |
| 03 | 编排器引擎 | P1 | 01(ChatMessage), 10(AiError) | `ai/engine.rs`（SSE + chat_stream/chat_simple） | ✅ 已完成 (commit 529717b) |
| 05 | Herdr IPC 客户端 | P2 | 10(AiError 可选) | `ai/herdr.rs` | ✅ 已完成 (本次迭代) |
| 06 | Agent 调度 | P2 | 03(engine), 05(herdr), 01(AgentActivity) | `ai/engine.rs`（chat_and_dispatch） | ✅ 已完成 (本次迭代) |
| 04 | 聊天面板 UI | P1 | 01, 02, 03, 05(workspace), 06(dispatch) | `ai/chat.rs` + `sidebar.rs` + `app.rs` | ✅ 已完成 (commit 529717b) |
| 07 | Agent 输出 UI | P2 | 04, 06 | `ai/chat.rs`（AgentBlock 组件） | ⬜ 待实现 |
| 09 | API 配置同步 | P3 | 无 | `api_provider/mod.rs`（批量 sync） | ⬜ 待实现 |
| 08 | Agent 监控 UI | P3 | 05, 07 | `ai/chat.rs`（👤 浮层） | ⬜ 待实现 |

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

**首次迭代** ✅ 已完成 (commit 529717b)
- 纯聊天面板 + 编排器 LLM 对话
- 会话持久化 + FTS5 搜索
- ZenixAI 设置面板（provider/model/thinking）
- 右面板 AI 图标 + AiChat 面板切换

**第二次迭代** ✅ 已完成
- Herdr IPC 客户端 (`ai/herdr.rs`) — 1696 行，23 个测试
- 编排器自动调度 agent (`chat_and_dispatch`) — 20 个测试
- tokio runtime 修复（main.rs + engine.rs `chat_stream`）
- 代码质量清理（dead code、clippy warnings）

**Check 结果** (2026-06-30):
- ✅ 编译 0 错误
- ✅ 93/93 测试通过
- ✅ 17 种 herdr RPC 方法 wire protocol 校验通过
- ✅ 跨模块一致性验证通过
- ✅ 边界情况处理完整
- 🔧 修复: tokio runtime 缺失导致 `chat_stream` 运行失败（critical）
- 🔧 修复: dead code cleanup（chat.rs, config.rs, error.rs）
- 🔧 修复: clippy warnings（`&url`→`url`, `unwrap_or_else`→`unwrap_or_default`, 移除死方法）

**第三次迭代** (未来): 07 → 08 → 09
- Agent 内联输出显示
- 运行监控浮层
- 配置同步增强
