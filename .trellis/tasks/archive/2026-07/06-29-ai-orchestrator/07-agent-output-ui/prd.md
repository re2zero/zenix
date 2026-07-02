# Agent 输出 UI 区块

## 目标

在编排器的聊天气泡中，将 agent 活动渲染为内联折叠区块。每个 agent 任务显示为：状态徽标 + agent 名称 + 可折叠输出。

## UI 设计

```
┌─ orchestator 气泡 ────────────────────┐
│  我已分析你的需求，接下来分配 agent 处理：  │
│                                        │
│  ┌─ Agent Block ──────────────────────┐│
│  │  [ ↗ Claude ] [● working] [▼]     ││
│  │  ┌─ 展开内容 ──────────────────┐   ││
│  │  │  Creating project structure...│   ││
│  │  │  Adding auth middleware...    │   ││
│  │  │  ✓ Login feature complete    │   ││
│  │  └──────────────────────────────┘   ││
│  └──────────────────────────────────────┘│
│                                          │
│  ┌─ Agent Block ──────────────────────┐  │
│  │  [ ✓ Claude ] [completed] [▲]      │  │
│  │  (点击展开查看完整输出)               │  │
│  └──────────────────────────────────────┘  │
│                                            │
│  登录功能已完成，包含以下模块：                │
│  - 用户注册/登录 API                       │
│  - JWT token 验证 middleware              │
│  - 前端登录页面                            │
└────────────────────────────────────────────┘
```

## 组件设计

```rust
/// Agent 活动区块
pub struct AgentBlock {
    agent_name: String,
    status: AgentBlockStatus,
    output: String,
    is_expanded: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AgentBlockStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl AgentBlockStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Pending => "⏳ pending",
            Self::Running => "● working",
            Self::Completed => "✓ completed",
            Self::Failed => "✗ failed",
        }
    }

    fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Self::Pending => theme.muted_foreground,
            Self::Running => theme.primary,
            Self::Completed => gpui::green(),
            Self::Failed => gpui::red(),
        }
    }
}

// 渲染函数
pub fn render_agent_block(block: &AgentBlock, theme: &Theme) -> impl IntoElement {
    v_flex()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(
            // Header: [agent_name] [status] [expand toggle]
            h_flex()
                .cursor_pointer()
                .on_click(|_, cx| { /* toggle expanded */ })
                .child(text(block.agent_name))
                .child(text(block.status.label()).text_color(block.status.color()))
                .child(text(if block.is_expanded { "▼" } else { "▲" }))
        )
        .when(block.is_expanded, |el| {
            // Output content
            el.child(
                div()
                    .font_family("monospace")
                    .text_size(px(10))
                    .child(&block.output)
            )
        })
}
```

## Requirements

- [ ] `AgentBlock` 组件（状态 + agent 名称 + 展开/折叠）
- [ ] `AgentBlockStatus` 枚举（颜色/标签）
- [ ] 三个状态：working（黄色/动态）、completed（绿色）、failed（红色）
- [ ] 点击折叠/展开交互
- [ ] 输出内容等宽字体（monospace）
- [ ] 在 orchestrator 气泡中渲染（`render_message_bubble` 中调用）

## Acceptance Criteria

- [ ] Agent output 显示为内联区块
- [ ] 点击可折叠/展开
- [ ] 不同状态显示不同颜色
- [ ] working 状态有动态效果（可选）
- [ ] 输出文本使用等宽字体

## Out of Scope

- Agent 输出实时追加（仅显示最终完整输出或阶段性快照）
- 富文本/ANSI 渲染
