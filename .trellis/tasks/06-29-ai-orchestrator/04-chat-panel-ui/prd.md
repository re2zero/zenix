# 聊天面板 UI

## 目标

实现 AI 编排侧边栏的完整聊天界面，包括消息列表、输入框、会话管理、搜索功能，以及底部配置栏。

## UI 布局

```
┌─ AI Chat Panel (380px) ─────────────────┐
│  ┌─ Header ────────────────────────────┐ │
│  │  会话标题                    [+][🕐] │ │
│  ├─────────────────────────────────────┤ │
│  │  ┌─ 会话选择器 (📋 激活时) ───────┐  │ │
│  │  │  会话1                          │  │ │
│  │  │  会话2  ← 当前                  │  │ │
│  │  │  会话3                          │  │ │
│  │  └─────────────────────────────────┘  │ │
│  ├─ Messages (scroll) ────────────────┤ │
│  │  ┌─────────────────────────────────┐ │ │
│  │  │ User: 帮我实现登录功能            │ │ │
│  │  │ → 编排器正在思考中...             │ │ │
│  │  │ ↗ Claude: [working...]          │ │ │
│  │  │   [▲ 展开输出]                   │ │ │
│  │  │ ✓ Claude: 任务完成 ✨             │ │ │
│  │  │ → 编排器: 登录功能已实现，包      │ │ │
│  │  │   含以下模块...                   │ │ │
│  │  └─────────────────────────────────┘ │ │
│  ├─ Search bar (🔍 激活时) ────────────┤ │
│  │  ┌─ 🔍 输入搜索词 ───────────────┐  │ │
│  │  │  结果1: 消息内容摘要...          │  │ │
│  │  │  结果2: 消息内容摘要...          │  │ │
│  │  └────────────────────────────────┘  │ │
│  ├─ Input area ────────────────────────┤ │
│  │  ┌─ Text input ──────────────────┐ │ │
│  │  │ 请输入你的需求...              │ │ │
│  │  └───────────────────────────────┘ │ │
│  ├─ Bottom bar ───────────────────────┤ │
│  │  Model: gpt-4o-mini  🧠: Medium   │ │ │
│  │  Workspace: project-zenix          │ │ │
│  │  [+][📋][👤 2][🔍][⚙]            │ │ │
│  └─────────────────────────────────────┘ │
└───────────────────────────────────────────┘
```

## 组件结构

```rust
pub struct AiChatPanel {
    // State
    session_manager: SessionManager,
    current_session_id: String,
    messages: Vec<ChatMessage>,
    input: SharedString,
    is_streaming: bool,
    streaming_content: String,
    agent_statuses: Vec<AgentStatus>,
    
    // UI states
    show_history: bool,
    show_search: bool,
    search_query: String,
    search_results: Vec<SearchHit>,
    
    // Config (read from ZenixAIConfig)
    provider_name: String,
    model: String,
    thinking_level: ThinkingLevel,
    workspace: Option<String>,
}

// Events / Actions
pub enum AiPanelEvent {
    SendMessage(String),
    SelectSession(String),
    NewSession,
    DeleteSession(String),
    Search(String),
    ToggleHistory,
    ToggleSearch,
    OpenSettings,
    ConfigChanged { provider: String, model: String, thinking: ThinkingLevel },
}
```

## Panel 集成

```rust
// src/ui/sidebar.rs
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    None,
    SystemInfo,
    AiChat,
}

// Sidebar 增加 AI 图标按钮（🤖）
// 点击切换 active_panel = Panel::AiChat
```

## Message Bubble 渲染

每条消息按 role 显示不同样式：

- **user**: 右对齐，彩色背景（primary color）
- **orchestrator**: 左对齐，默认背景
- Agent 活动块: 黄色/绿色/红色状态指示 + 可折叠输出

## 会话管理

- **➕ 按钮**: 创建新会话（调用 SessionManager.create_session）
- **📋 按钮**: 切换显示会话列表
- **🕐 图标**（Header 上）: 同 📋，历史会话切换
- 会话列表: 显示 title + 时间，点击切换到该会话
- 当前会话高亮

## 搜索功能

- **🔍 按钮**: 输入框切换为搜索模式
- 实时搜索（FTS5）
- 结果列表: snippet 展示命中内容
- 点击结果: 关闭搜索，跳转到对应消息（通过 sequence 滚动定位）

注意：搜索目的是定位用户**曾经输入过的内容**，不是搜索 LLM 输出。

## 依赖

- 任务 01（`SessionManager` / `ChatMessage`）
- 任务 02（`ZenixAIConfig` / `ProviderSelector`）
- 任务 03（`OrchestratorEngine` / `StreamEvent`）
- 任务 05（`HerdrIpcClient` → 获取 workspace 列表供底部栏选择）
- 任务 06（`chat_and_dispatch()` — `on_send()` 调用入口）

## 会话自动命名

当编排器首次回复后（第一次 `chat_and_dispatch()` 完成）：

1. 取 user 的第一条消息 + 编排器的回复（截取前 200 字）
2. 调 `OrchestratorEngine.chat_simple()` 生成标题 prompt
3. 结果作 title，调 `SessionManager.update_session()`
4. 更新 UI header

首次回复后才命名（不是创建时就命名），保证标题反映对话内容。

## Requirements

- [ ] Panel::AiChat 变体 + sidebar AI 图标
- [ ] AiChatPanel struct（所有状态字段）
- [ ] 消息列表渲染（user/orchestrator 气泡）
- [ ] 输入框 + 发送（调 `chat_and_dispatch()`）
- [ ] 流式输出实时渲染
- [ ] 会话管理 UI（+ / 📋 / 切换）
- [ ] 会话自动命名（首次回复后调 chat_simple）
- [ ] 搜索 UI（🔍 → 输入 → 结果 → 跳转）
- [ ] 底部栏（model/thinking/workspace 显示，workspace 列表来自 HerdrIpcClient）
- [ ] Bottom buttons: [+][📋][👤][🔍][⚙]

## Acceptance Criteria

- [ ] 点击 sidebar AI 图标 → 显示聊天面板
- [ ] 输入消息 → 显示 user 气泡
- [ ] 编排器回复流式渲染到消息列表
- [ ] Agent 活动区块显示在工作区中
- [ ] 会话自动生成标题
- [ ] 点击 + 创建新会话
- [ ] 📋 切换会话
- [ ] 🔍 搜索历史消息并跳转
- [ ] ⚙ 打开设置面板
- [ ] 底部栏显示正确的 model / thinking level / workspace
