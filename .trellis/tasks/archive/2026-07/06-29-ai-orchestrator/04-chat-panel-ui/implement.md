1. 修改 `src/ui/sidebar.rs` — Panel 枚举 + AI 图标 + AI_PANEL_WIDTH
2. 创建 `src/ai/chat.rs` — AiChatPanel 完整 UI 实现
3. 修改 `src/app.rs` — ZenixApp 集成（ai_panel 字段、render、事件）
4. `cargo check` 验证

## Implementation Steps

### Step 1: ui/sidebar.rs
- `Panel` 枚举增加 `AiChat` 变体
- `AI_PANEL_WIDTH = 380.0` 常量
- Sidebar 增加 AI 图标按钮（`on_ai` 回调）
- 修改 `sidebar()` 函数签名

### Step 2: src/ai/chat.rs
实现 AiChatPanel：
1. struct 定义（所有状态字段，含 workspaces: Vec<WorkspaceInfo> 和 session_named: bool）
2. `AiChatPanel::new()` — 初始化 session_manager、加载 workspace 列表等
3. `render()` — Header + Messages + Input + BottomBar + Buttons
4. `render_message_bubble()` — 用户/编排器气泡
5. `render_session_list()` — 📋 会话列表
6. `render_search()` — 🔍 搜索
7. `render_bottom_bar()` — 配置显示 + workspace 选择器
8. `render_bottom_buttons()` — [+][📋][👤][🔍][⚙]
9. `on_send()` — 提交消息（调用 OrchestratorEngine.chat_and_dispatch，任务 06）
10. `auto_name_session()` — 首次回复后自动生成标题
11. `add_agent_activity()` / `append_agent_output()` / `update_agent_status()` — agent 状态管理
12. `on_select_session()` — 切换会话
13. `on_new_session()` — 创建会话
14. `on_search()` — FTS5 搜索

### Step 3: src/app.rs
- ZenixApp 增加 `ai_panel: Option<Entity<AiChatPanel>>` 字段
- render 中根据 `active_panel` 显示 AiChatPanel
- `on_ai_click()` 事件处理器（toggle + lazy init）
- `handle_ai_event()` 中转 AiPanelEvent

### Step 4: 验证
- `cargo check`
