# Agent 监控 UI

## 目标

在 chat 底部栏的 👤 按钮中显示所有运行中 agent 的状态。通过按钮悬浮或点击显示浮层，展示每个 agent 的名称、状态和所属 workspace。

## UI 设计

```
┌─ Bottom Buttons ──────────────────────┐
│  [+][📋][👤 2][🔍][⚙]               │
└───────────────────────────────────────┘
                │
                ▼  (click 👤)
┌─ Agent Status Overlay ───────────────┐
│  运行中 Agents (2)                    │
│                                      │
│  Claude    ● working    project-zenix│
│  OpenCode  ● working    docs-site    │
│  Codex     ✓ idle                    │
│                                      │
│  ─────────────────────────────────   │
│  All agents idle                     │
└──────────────────────────────────────┘
```

## 实现方式

```rust
// 在 AiChatPanel 中添加

/// 从 HerdrIpcClient 获取的状态快照
pub struct AgentStatusView {
    pub agent_name: String,
    pub status: String,        // "idle" | "working" | "blocked" | "done"
    pub workspace: String,
    pub pane_id: String,
}

impl AiChatPanel {
    /// 定期刷新 agent 状态（timer 驱动，每 5s）
    fn refresh_agent_statuses(&mut self, cx: &mut ViewContext<Self>);

    /// 渲染 👤 按钮上的计数
    fn render_agent_count_badge(&self) -> impl IntoElement;

    /// 渲染状态浮层
    fn render_agent_overlay(&self) -> impl IntoElement;
}
```

## 状态更新机制

```rust
/// 定时轮询（简单可靠的方式）
fn start_status_polling(&mut self, cx: &mut ViewContext<Self>) {
    cx.spawn(|this, mut cx| async move {
        loop {
            // 每 5s 轮询
            smol::Timer::after(Duration::from_secs(5)).await;

            this.update(&mut cx, |panel, cx| {
                // 通过 HerdrIpcClient.list_agent_statuses() 获取状态
                // 更新 agent_statuses
                cx.notify();
            }).ok();
        }
    }).detach();
}
```

## Requirements

- [ ] 👤 按钮显示运行中 agent 数量
- [ ] Agent 状态浮层（点击显示/隐藏）
- [ ] 每个 agent 行：名称 + 状态指示器（颜色点）+ 所属 workspace
- [ ] 状态定时刷新（每 5s 轮询）
- [ ] 无活跃 agent 时 👤 显示 0 或灰显

## Acceptance Criteria

- [ ] 👤 按钮正确显示运行中 agent 数量
- [ ] 点击 👤 显示状态浮层
- [ ] 浮层中 agent 状态每 5s 更新
- [ ] agent 从 working → done 后浮层自动反映

## Out of Scope

- Agent 输出实时流式显示（已有 07）
- Agent pane 的远程控制（kill/resume）
