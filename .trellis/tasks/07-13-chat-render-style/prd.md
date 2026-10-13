# PRD: AI 会话渲染样式优化

## 1. 背景

当前 zenixAI 聊天面板的渲染存在若干视觉一致性问题：

1. **流式工具调用/结果没有独立容器** — `DispatchStreamEvent::ToolCall` 和 `ToolResult` 以纯 markdown 文本追加到 assistant 消息正文，与 LLM 文字答复完全混在一起，用户无法区分"这是 LLM 在说话"还是"这是工具调用"。
2. **不同工具用同一内联格式** — `spawn_worker`、`read_output`、`send_text`、`close_worker` 等不同类型的工具执行结果全部使用同一种 inline markdown 格式，无 icon/颜色区分。
3. **思考块视觉区分度不够** — reasoning block 有边框但颜色跟普通 assistant 消息背景相近，辨识度不足。
4. **工具调用缺时间戳** — 不显示执行耗时。
5. **消息布局缺少 role 标识** — 没有直观的角色标签（User / Assistant）和时间线感。
6. **Agent 监视器列表视觉平淡** — 纯文本列表 + 状态色点。

## 2. 目标

为上述问题提供系统的视觉优化方案，使 zenixAI 聊天的信息层级更清晰、可读性更强。

## 3. 范围

### In Scope

- `src/ai/session.rs` — 新增 ToolCallState/ToolCallStatus 类型
- `src/ai/engine.rs` — DispatchStreamEvent::ToolResult 签名变更（新增 id）
- `src/app.rs` — Stream event 处理逻辑，从 inline markdown 改为 tool_calls 字段
- `src/ai/chat/messages.rs` — 工具调用卡片渲染 + 思考块样式 + 时间线指示器
- `src/ai/chat/header.rs` — agent monitor 样式增强
- `src/ai/chat/mod.rs` — RenderCtx 传递（is_first_in_group）

### Out of Scope

- 消息数据库表结构变更
- 新增功能（如消息编辑、删除、搜索）
- 面板布局重排（如左侧/右侧切换）

## 4. 需求

### 4.1 流式工具调用 → 独立渲染卡片 + 类型区分 + 耗时 (P0+P1+P3)

- [ ] `DispatchStreamEvent::ToolCall` 不再追加 inline markdown，改为填充 `ChatMessage.tool_calls: Vec<ToolCallState>` 
- [ ] `ToolResult` 事件携带 `id` 精确匹配更新对应 `ToolCallState` 状态（Completed/Failed）
- [ ] `messages.rs::render_message()` 在 reasoning block 之后、content 之前渲染工具调用卡片
- [ ] 卡片 header: [工具名 icon] [工具名] [状态 ✓/●/✕] [耗时 ⏱ 1.2s]
- [ ] 工具名 → icon 映射：`spawn_worker→🤖`, `send_text→📨`, `read_output→📖`, `wait_for_output→⏳`, `list_workers→📋`, `close_worker→🗑`
- [ ] Running 态显示脉冲动画 + 实时计时
- [ ] 卡片支持展开/折叠：折叠显示参数摘要（关键字段），展开显示完整 JSON + 结果全文

### 4.2 思考块样式强化 (P2)

- [ ] 添加左侧 3px 强调色色条（用 h_flex + child div 实现）
- [ ] 添加 🧠 思考图标
- [ ] 折叠态只显示 `🧠 Thought X.Xs` 一行，不占高度
- [ ] 流式中的固定高度从 `h(px(90))` 改为 `max_h(px(200)).overflow_y_scroll()`

### 4.3 消息时间线指示器 (P4)

- [ ] 每条消息左侧添加角色指示色条（user=accent, assistant=transparent, tool=muted）
- [ ] 连续同角色消息合并指示器（仅第一条显示，后续不重复）

### 4.4 Agent 监视器样式增强 (P5)

- [ ] 每个 agent 条目增加输出摘要预览（截取 `recent_output` 的前 60 字符）
- [ ] 状态标签用背景色 pill 替代纯文本

## 5. 验收标准

1. 流式中看到 `spawn_worker` 调用时，显示独立的 Tool Call 卡片，而非 inline markdown 文本
2. 不同工具显示不同 icon（🤖/📨/📖/⏳/📋/🗑）
3. 思考块左侧有垂直强调色条 + 🧠 图标
4. 工具卡片显示 `✓ 1.2s` 执行耗时
5. 消息左侧有 role 指示色条（角色切换时显示）
6. Agent 监视器显示输出摘要 + 状态 pill

## 6. 非目标

- 不改变现有消息存储模型
- 不重构 gpui::list 虚拟化逻辑
- 不改动 markdown 渲染引擎
