# 聊天面板 UI — 设计

## Panel 集成

```rust
// src/ui/sidebar.rs

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    None,
    SystemInfo,
    AiChat,
}

pub const AI_PANEL_WIDTH: f32 = 380.0;

// sidebar() 函数增加 AI 图标按钮
pub fn sidebar(
    theme: &Theme,
    active: Panel,
    settings_active: bool,
    on_ai: ThemeCallback,          // 新增
    on_sysinfo: ThemeCallback,
    on_settings: ThemeCallback,
) -> gpui::Div {
    // ... existing icons ...
    // 在 sysinfo 和 settings 之间或附近增加 AI 图标
}
```

## AiChatPanel

```rust
// src/ai/chat.rs

use gpui::*;
use super::session::*;
use super::config::*;
use crate::orchestrator::OrchestratorEngine;

pub struct AiChatPanel {
    // 数据
    sessions: Vec<Session>,
    messages: Vec<ChatMessage>,
    current_session_id: String,
    session_manager: SessionManager,

    // 输入
    input: SharedString,
    input_editor: View<Input>,  // gpui-component Input

    // 流式状态
    is_streaming: bool,
    streaming_content: String,

    // UI 状态
    show_history: bool,
    show_search: bool,
    search_query: SharedString,
    search_results: Vec<SearchHit>,

    // 配置
    provider_name: String,
    model: String,
    thinking_level: ThinkingLevel,
    workspace: Option<String>,

    // 工作区列表（来自 HerdrIpcClient）
    workspaces: Vec<WorkspaceInfo>,

    // 是否已对当前会话命名
    session_named: bool,
}

impl AiChatPanel {
    pub fn new(cx: &mut Context<Self>) -> Self;

    /// 发送消息处理链：
    /// 1. 添加 user message 到 messages
    /// 2. 持久化到 SQLite
    /// 3. 调用编排器 engine.chat_stream()
    /// 4. 流式渲染 tokens
    /// 5. 持久化 orchestator message
    fn on_send(&mut self, cx: &mut ViewContext<Self>);

    /// 切换会话
    fn on_select_session(&mut self, session_id: &str, cx: &mut ViewContext<Self>);

    /// 创建新会话
    fn on_new_session(&mut self, cx: &mut ViewContext<Self>);

    /// 搜索消息
    fn on_search(&mut self, cx: &mut ViewContext<Self>);

    /// 渲染消息列表
    fn render_messages(&self, cx: &mut ViewContext<Self>) -> impl IntoElement;

    /// 渲染单个聊天气泡
    fn render_message_bubble(&self, msg: &ChatMessage) -> impl IntoElement;

    /// 渲染 agent 活动区块
    fn render_agent_block(&self, activity: &AgentActivity) -> impl IntoElement;

    /// 渲染搜索界面
    fn render_search(&self) -> impl IntoElement;

    /// 渲染会话列表
    fn render_session_list(&self) -> impl IntoElement;

    /// 渲染底部栏
    fn render_bottom_bar(&self) -> impl IntoElement;

    /// 渲染底部按钮
    fn render_bottom_buttons(&self) -> impl IntoElement;
}
```

## render() 布局

```rust
impl Render for AiChatPanel {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .w(px(AI_PANEL_WIDTH))
            .h_full()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.border)
            // Header
            .child(self.render_header())
            // 会话选择器（条件渲染）
            .when(self.show_history, |el| el.child(self.render_session_list()))
            // 消息列表（可滚动）
            .child(
                v_flex()
                    .flex_grow(1)
                    .overflow_y_scroll()
                    .px(px(12))
                    .py(px(8))
                    .children(self.messages.iter().map(|msg| self.render_message_bubble(msg)))
            )
            // 搜索栏（条件渲染）
            .when(self.show_search, |el| el.child(self.render_search()))
            // 输入区
            .child(self.render_input_area())
            // 底部栏
            .child(self.render_bottom_bar())
            // 底部按钮
            .child(self.render_bottom_buttons())
    }
}
```

## 消息气泡渲染

```rust
fn render_message_bubble(&self, msg: &ChatMessage) -> impl IntoElement {
    match msg.role.as_str() {
        "user" => {
            h_flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(320))
                        .px(px(12))
                        .py(px(8))
                        .rounded_lg()
                        .bg(theme.primary.opacity(0.15))
                        .child(text(msg.content))
                )
        }
        "orchestrator" => {
            v_flex()
                .child(
                    div()
                        .child(text(msg.content))
                )
                // Agent 活动区块
                .children(msg.agent_activities.as_ref().map(|activities| {
                    activities.iter().map(|a| self.render_agent_block(a))
                }))
        }
        _ => div(),
    }
}
```

## 发送消息 — 使用 chat_and_dispatch()

使用 `cx.spawn()` + background_executor 驱动编排器全周期：

```rust
fn on_send(&mut self, cx: &mut ViewContext<Self>) {
    let input = self.input.to_string();
    if input.trim().is_empty() { return; }
    self.input.clear();

    // 1. 添加 user message
    let user_msg = ChatMessage { role: "user".into(), content: input.clone(), .. };
    self.messages.push(user_msg.clone());
    let message_sequence = self.messages.len() as i64;
    // 持久化 user message...
    self.session_manager.add_message(&ChatMessage { sequence: message_sequence, ..user_msg });

    // 2. 调 chat_and_dispatch()（任务 06 的集成方法）
    self.is_streaming = true;
    cx.notify();

    let engine = self.engine.clone();
    let messages = self.messages.clone();
    let session_id = self.current_session_id.clone();
    let session_named = self.session_named;
    let session_manager = self.session_manager.clone();

    cx.spawn(|this, mut cx| async move {
        // 收集流式 tokens 供循环使用
        let mut collected_text = String::new();

        let result = engine.chat_and_dispatch(
            &messages,
            ORCHESTRATOR_SYSTEM_PROMPT,
            &mut herdr_client,
            |event| {
                match event {
                    StreamEvent::Token(token) => {
                        collected_text.push_str(&token);
                        // 推送到 UI 流式渲染
                        this.update(&mut cx, |panel, _| {
                            panel.streaming_content = collected_text.clone();
                            cx.notify();
                        }).ok();
                    }
                    StreamEvent::AgentDispatch(task) => {
                        // 添加新 agent 活动块到 streaming_content
                        this.update(&mut cx, |panel, _| {
                            panel.add_agent_activity(&task.agent_name, "running");
                            cx.notify();
                        }).ok();
                    }
                    StreamEvent::AgentOutput { agent_name, text } => {
                        // 追加 agent 实时输出
                        this.update(&mut cx, |panel, _| {
                            panel.append_agent_output(&agent_name, &text);
                            cx.notify();
                        }).ok();
                    }
                    StreamEvent::AgentComplete(result) => {
                        // 标记 agent 完成
                        this.update(&mut cx, |panel, _| {
                            panel.update_agent_status(&result.agent_name, &result.exit_status);
                            cx.notify();
                        }).ok();
                    }
                    StreamEvent::Error(e) => {
                        this.update(&mut cx, |panel, _| {
                            panel.set_error(&e);
                            cx.notify();
                        }).ok();
                    }
                    StreamEvent::Done => {}
                }
            },
        ).await;

        // 3. chat_and_dispatch() 完成，构建并持久化 orchestrator message
        this.update(&mut cx, |panel, cx| {
            let final_content = match &result {
                Ok(r) => {
                    let mut text = r.orchestrator_text.clone();
                    if let Some(ref summary) = r.summary {
                        text.push_str("\n\n");
                        text.push_str(summary);
                    }

                    // 将 agent_results 转为 AgentActivity
                    let activities: Vec<AgentActivity> = r.agent_results.iter().map(|ar| AgentActivity {
                        agent_name: ar.agent_name.clone(),
                        status: ar.exit_status.clone(),
                        summary: Some(ar.output.chars().take(200).collect()),
                        ..Default::default()
                    }).collect();

                    // 持久化 orchestrator message（含 agent_activities）
                    let msg = ChatMessage {
                        session_id: panel.current_session_id.clone(),
                        role: "orchestrator".into(),
                        content: text.clone(),
                        agent_activities: Some(activities),
                        sequence: (panel.messages.len() as i64) + 1,
                    };
                    panel.messages.push(msg.clone());
                    panel.session_manager.add_message(&msg).ok();

                    // 自动命名（仅首次）
                    if !panel.session_named {
                        panel.auto_name_session(&msg, cx);
                    }

                    text
                }
                Err(e) => format!("编排器出错：{e}"),
            };

            panel.streaming_content.clear();
            panel.is_streaming = false;
            cx.notify();
        }).ok();
    }).detach();
}

/// 会话自动命名（首次回复后调用）
fn auto_name_session(&mut self, first_reply: &ChatMessage, cx: &mut ViewContext<Self>) {
    let user_first_msg = self.messages.iter()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");

    let reply_preview: String = first_reply.content.chars().take(200).collect();
    let prompt = format!(
        "根据以下对话内容生成一个简短的会话标题（10字以内）：\n\n用户：{user_first_msg}\n\nAI：{reply_preview}"
    );

    cx.spawn(|this, mut cx| async move {
        if let Ok(title) = engine.chat_simple(&prompt, "返回一个短标题，不要多余文字").await {
            this.update(&mut cx, |panel, cx| {
                let title = title.trim().chars().take(30).collect::<String>();
                panel.session_manager.update_session_title(&panel.current_session_id, &title).ok();
                panel.session_named = true;
                cx.notify();
            }).ok();
        }
    }).detach();
}
```

> 注意：此处的 `chat_and_dispatch()` 是任务 06 提供的高层方法。`on_send()` 不再直接调 `chat_stream()`。

## 搜索 UI

```rust
fn render_search(&self) -> impl IntoElement {
    v_flex()
        .px(px(8))
        .py(px(4))
        .child(
            h_flex()
                .child(Input::new("search_input")
                    .placeholder("搜索历史消息...")
                    .on_input(cb))
        )
        .children(self.search_results.iter().map(|hit| {
            h_flex()
                .cursor_pointer()
                .on_click(move |_, cx| { /* scroll to message */ })
                .child(text(hit.snippet))
        }))
}
```

## ZenixApp 集成

```rust
// src/app.rs
pub struct ZenixApp {
    // ... existing fields ...
    ai_panel: Option<Entity<AiChatPanel>>,  // lazy init on first AI click
}

impl Render for ZenixApp {
    fn render(...) {
        let panel = match active_panel {
            Panel::AiChat => self.ai_panel.as_ref().map(|p| p.clone().into_any_element()),
            Panel::SystemInfo => Some(system_info_panel(theme, sysinfo)),
            Panel::None => None,
        };

        h_flex()
            .child(terminal_area)
            .children(panel)
            .child(sidebar(theme, active_panel, ..., on_ai_click, ...))
    }
}
```

## 事件处理

```rust
impl ZenixApp {
    fn on_ai_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut App) {
        if self.active_panel == Panel::AiChat {
            self.active_panel = Panel::None;
        } else {
            self.active_panel = Panel::AiChat;
            // lazy init AiChatPanel
            if self.ai_panel.is_none() {
                let entity = cx.new(|cx| AiChatPanel::new(cx));
                self.ai_panel = Some(entity);
            }
        }
    }
}
```
