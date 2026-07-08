# PRD: ZenixAI Chat Polish — Tool Folding, Reasoning Blocks & Cleanup

## Goal
完成 v4 视觉方向中尚未实现的三项收尾工作，使 chat 面板功能完整。

## Features

### A: Tool Call 折叠展开 (Scene 3)
- 工具调用卡片 header 可点击切换折叠/展开
- 折叠时隐藏 body，▼ 箭头变为 ▶
- 默认展开（与现有行为一致）
- 折叠状态跨列表重建不丢失

### B: Reasoning 折叠块 (Scene 7)
- Anthropic SSE 解析器识别 `delta.thinking` 思维链内容
- 新增 `StreamEvent::Thinking` / `DispatchStreamEvent::Thinking` 事件变体
- `ChatMessage` 新增 `reasoning_text: Option<String>` 字段
- 流式渲染时 thinking 内容追加到对应消息的 reasoning 字段
- assistant 消息上方渲染折叠块：默认折叠 + "reasoned" 标签
- 折叠状态持久化（与 tool card 同机制）

### C: 小清理
- Link `[text](url)` 添加 onClick 调用 `open::that(url)`
- 删除 `AI_PANEL_WIDTH` 常量残留（`mod.rs:47`, `sidebar.rs:24`）

## 排除
- 终端宽度黄条（延后）
- ThinkingLevel 配置传递到 API 请求（现有行为不变）
- reasoning 时间计算（先显示静态标签）
