1. `src/ai/chat.rs` 中扩展 AiChatPanel：
   - `AgentStatusView` struct
   - `start_status_polling()` 方法
   - `render_agent_button()` — 👤 + 计数徽标
   - `render_agent_overlay()` — 状态浮层
   - `render_bottom_buttons()` — 底部按钮栏
   - `icon_btn()` 辅助函数
2. `cargo check` 验证

## Implementation Steps

### Step 1: src/ai/chat.rs
- 添加 `AgentStatusView` struct
- AiChatPanel 添加 `agent_statuses: Vec<AgentStatusView>` 和 `show_agent_overlay: bool`
- 在 `new()` 中调用 `start_status_polling()`
- 实现状态轮询（每 5s herdr IPC）
- 实现 UI 渲染方法

### Step 2: 验证
- `cargo check`
