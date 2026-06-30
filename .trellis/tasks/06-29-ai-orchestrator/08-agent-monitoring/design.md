# Agent 监控 UI — 设计

## 状态轮询

```rust
// src/ai/chat.rs 中 AiChatPanel 扩展

impl AiChatPanel {
    /// 启动状态轮询（在 new() 中调用）
    fn start_status_polling(&mut self, cx: &mut ViewContext<Self>) {
        // 只有 herdr 可用时才轮询
        cx.spawn(|this, mut cx| async move {
            loop {
                smol::Timer::after(Duration::from_secs(5)).await;
                if let Ok(statuses) = Self::fetch_statuses().await {
                    this.update(&mut cx, |panel, cx| {
                        panel.agent_statuses = statuses;
                        cx.notify();
                    }).ok();
                }
            }
        }).detach();
    }

    /// 后台线程获取 agent 状态
    async fn fetch_statuses() -> Result<Vec<AgentStatusView>> {
        match HerdrIpcClient::connect() {
            Ok(mut client) => {
                let agents = client.list_agent_statuses()?;
                // 转换格式
                Ok(agents.into_iter().map(|a| AgentStatusView {
                    agent_name: a.agent_name,
                    status: a.status,
                    workspace: String::new(), // 从 pane 信息获取
                    pane_id: a.pane_id,
                }).collect())
            }
            Err(_) => Ok(vec![]),
        }
    }
}
```

## 👤 按钮

```rust
fn render_agent_button(&self) -> impl IntoElement {
    let running_count = self.agent_statuses.iter()
        .filter(|a| a.status == "working" || a.status == "blocked")
        .count();

    h_flex()
        .relative()  // for overlay positioning
        .child(
            icon_btn("agent_status", "👤", self.show_agent_overlay, |_, cx| {
                // toggle overlay
            })
        )
        .when(running_count > 0, |el| {
            // 计数徽标
            el.child(
                div()
                    .absolute()
                    .top(px(0))
                    .right(px(0))
                    .w(px(14))
                    .h(px(14))
                    .rounded_full()
                    .bg(gpui::red())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(txt(px(8)).text_color(gpui::white()).child(running_count.to_string()))
            )
        })
}
```

## 状态浮层

```rust
fn render_agent_overlay(&self) -> impl IntoElement {
    let running = self.agent_statuses.iter()
        .filter(|a| a.status == "working")
        .count();

    v_flex()
        .absolute()
        .bottom(px(40))
        .right(px(0))
        .w(px(280))
        .rounded_md()
        .bg(self.theme.background)
        .border_1()
        .border_color(self.theme.border)
        .shadow_lg()
        .p(px(12))
        .child(
            txt(px(11))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("运行中 Agents ({running})"))
        )
        .child(Separator::horizontal())
        .children(self.agent_statuses.iter().map(|a| {
            h_flex()
                .gap(px(6))
                .items_center()
                .child(status_dot(&a.status))
                .child(txt(px(10)).child(&a.agent_name))
                .child(txt(px(10)).text_color(self.theme.muted_foreground).child(&a.status))
                .flex_grow(1)
                .child(txt(px(9)).text_color(self.theme.muted_foreground).child(&a.workspace))
        }))
        .when(self.agent_statuses.is_empty(), |el| {
            el.child(txt(px(10)).text_color(self.theme.muted_foreground).child("No agents running"))
        })
}

fn status_dot(status: &str) -> impl IntoElement {
    let color = match status {
        "working" => gpui::green(),
        "blocked" => gpui::yellow(),
        "idle" => self.theme.muted_foreground,
        "done" => gpui::blue(),
        _ => self.theme.muted_foreground,
    };
    div()
        .w(px(6))
        .h(px(6))
        .rounded_full()
        .bg(color)
}
```

## 集成到底部按钮栏

```rust
fn render_bottom_buttons(&self) -> impl IntoElement {
    h_flex()
        .px(px(8))
        .py(px(4))
        .gap(px(4))
        .child(icon_btn("new_session", "+", false, |_, cx| { /* new session */ }))
        .child(icon_btn("history", "📋", self.show_history, |_, cx| { /* toggle history */ }))
        .child(self.render_agent_button())  // 👤 with badge
        .child(icon_btn("search", "🔍", self.show_search, |_, cx| { /* toggle search */ }))
        .child(icon_btn("settings", "⚙", false, |_, cx| { /* open settings */ }))
}
```
