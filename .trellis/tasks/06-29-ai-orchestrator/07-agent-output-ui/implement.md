1. `src/ai/chat.rs` 中扩展 AiChatPanel：
   - `AgentBlockStatus` 枚举
   - `render_status_icon()` / `render_agent_block()` / `render_orchestrator_bubble()`
   - `expanded_agents` 状态字段 + toggle 逻辑
2. `cargo check` 验证

纯 UI 改动，不涉及数据流或后端。
