# Agent 输出 UI 区块 — 设计

## 组件结构

```rust
// src/ai/chat.rs 中的 AiChatPanel

impl AiChatPanel {
    /// 渲染 orchestrator 气泡中的 agent 活动区块
    fn render_agent_block(&self, activity: &AgentActivity) -> impl IntoElement {
        let status = match activity.status.as_str() {
            "running" => AgentBlockStatus::Running,
            "completed" => AgentBlockStatus::Completed,
            "failed" => AgentBlockStatus::Failed,
            _ => AgentBlockStatus::Pending,
        };

        v_flex()
            .rounded_md()
            .border_1()
            .border_color(self.theme.border)
            .overflow_hidden()
            .child(
                h_flex()
                    .px(px(8))
                    .py(px(6))
                    .bg(self.theme.surface)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, cx| {
                        if let Some(idx) = this.expanded_agents.iter().position(|a| a == &activity.agent_name) {
                            this.expanded_agents.remove(idx);
                        } else {
                            this.expanded_agents.push(activity.agent_name.clone());
                        }
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .gap(px(6))
                            .items_center()
                            .child(self.render_status_icon(status))
                            .child(
                                txt(px(11))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{}  ", activity.agent_name))
                            )
                            .child(
                                txt(px(10))
                                    .text_color(status.color(&self.theme))
                                    .child(status.label())
                            )
                    )
                    .flex_grow(1)
                    .child(
                        txt(px(10))
                            .text_color(self.theme.muted_foreground)
                            .child(if self.is_agent_expanded(&activity.agent_name) { "▼" } else { "▲" })
                    )
            )
            .when(self.is_agent_expanded(&activity.agent_name), |el| {
                el.child(
                    div()
                        .px(px(8))
                        .py(px(6))
                        .bg(self.theme.background)
                        .child(
                            txt(px(10))
                                .font_family("monospace")
                                .text_color(self.theme.foreground)
                                .child(activity.summary.as_deref().unwrap_or(""))
                        )
                )
            })
    }

    fn render_status_icon(&self, status: AgentBlockStatus) -> impl IntoElement {
        let (icon, color) = match status {
            AgentBlockStatus::Pending => ("⏳", self.theme.muted_foreground),
            AgentBlockStatus::Running => ("↗", self.theme.primary),
            AgentBlockStatus::Completed => ("✓", gpui::green()),
            AgentBlockStatus::Failed => ("✗", gpui::red()),
        };
        txt(px(11)).text_color(color).child(icon)
    }
}

/// Agent 区块状态
#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentBlockStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl AgentBlockStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Pending => "待处理",
            Self::Running => "运行中",
            Self::Completed => "完成",
            Self::Failed => "失败",
        }
    }

    fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Self::Pending => theme.muted_foreground,
            Self::Running => theme.primary,
            Self::Completed => gpui::hsla(0.33, 0.8, 0.5, 1.0),  // green
            Self::Failed => gpui::hsla(0.0, 0.8, 0.5, 1.0),       // red
        }
    }
}
```

## 集成到消息气泡

```rust
fn render_orchestrator_bubble(&self, msg: &ChatMessage) -> impl IntoElement {
    v_flex()
        .gap(px(4))
        .child(
            txt(px(11))
                .text_color(self.theme.foreground)
                .child(&msg.content)
        )
        // Agent activity blocks
        .children(msg.agent_activities.as_ref().map(|activities| {
            let this = self;
            activities.iter().map(move |a| this.render_agent_block(a))
        }))
}

// 在 render_message_bubble 中调用：
fn render_message_bubble(&self, msg: &ChatMessage) -> impl IntoElement {
    match msg.role.as_str() {
        "user" => self.render_user_bubble(msg),
        "orchestrator" => self.render_orchestrator_bubble(msg),
        _ => div(),
    }
}
```

## 展开状态管理

```rust
pub struct AiChatPanel {
    // ... other fields ...
    /// 当前展开的 agent 名称列表（UI 状态，不持久化）
    expanded_agents: Vec<String>,
}
```
