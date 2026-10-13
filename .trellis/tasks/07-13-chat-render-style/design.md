# Design: AI 会话渲染样式优化 (v2)

## 1. 架构洞察

审查发现的关键修正（相对于 v1）：

1. **`AgentComplete`/`add_agent_block` 是死代码** — `DispatchStreamEvent::AgentComplete` 从未被 emit，`tool_card.rs` 渲染路径永不执行。用户实际看到的工具执行结果全部通过流式 `ToolCall`/`ToolResult` 内联 markdown 文本。
2. **`ToolResult` 事件缺少 `id` 字段** — 当同轮多次调用同一工具时无法正确匹配。
3. **`AgentResult` 缺少时间字段** — 需要追踪耗时。

**核心设计**: 将流式 ToolCall/ToolResult 改为填充 `ChatMessage.tool_calls`，在 `messages.rs` 中渲染为独立卡片。

## 2. 数据模型变更

### 2.1 ToolCallState — 流式工具调用状态

```rust
// src/ai/session.rs
#[derive(Debug, Clone)]
pub struct ToolCallState {
    pub id: String,            // LLM 分配的 tool_call_id (如 "call_abc123")
    pub name: String,          // 工具名称 (如 "spawn_worker")
    pub args: serde_json::Value,
    pub status: ToolCallStatus,
    /// 结果摘要 (取 content 前 80 字符)
    pub result_summary: Option<String>,
    /// 调用开始时间 (epoch ms)
    pub started_at: u64,
    /// 完成时间 (epoch ms)
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallStatus {
    Running,    // 已发出调用，等待结果
    Completed,  // 执行成功
    Failed,     // 执行出错
}
```

### 2.2 ChatMessage 新增字段

```rust
// src/ai/session.rs
pub tool_calls: Vec<ToolCallState>,  // 纯内存，不持久化
```

`ChatMessage` 未 derive `Serialize/Deserialize`，加字段无副作用。

### 2.3 DispatchStreamEvent::ToolResult 扩展

```rust
// 旧: ToolResult(String, String, bool)
// 新: 带 id
DispatchStreamEvent::ToolResult {
    id: String,         // tool_call_id，与 ToolCall 的 id 一致
    tool_name: String,
    content: String,
    is_error: bool,
}
```

### 2.4 AgentResult 增加耗时字段

```rust
pub struct AgentResult {
    // ... 现有字段
    pub duration_ms: Option<u64>,  // 新增，未来使用
}
```

`AgentResult` 当前未被 emit，增加字段预留。

## 3. 模块变更

### 3.1 engine.rs — ToolResult 携带 id

**变更点**: `chat_with_tools()` 中工具执行循环

```rust
// 旧:
let results: Vec<ToolResult> =
    tool_calls.iter().map(|tc| executor.execute(tc)).collect();
for result in &results {
    on_event(DispatchStreamEvent::ToolResult(
        result.tool_name.clone(), result.content.clone(), result.is_error,
    ));
}

// 新: 并行迭代 tool_calls 和 results，携带 id
for (tc, result) in tool_calls.iter().zip(results.iter()) {
    on_event(DispatchStreamEvent::ToolResult {
        id: tc.id.clone(),
        tool_name: result.tool_name.clone(),
        content: result.content.clone(),
        is_error: result.is_error,
    });
}
```

影响: 需同步更新 `chat_stream_with_tools()` 中的 `SseStreamEvent::ToolResult` 匹配分支（如果有，或确认无影响）。

### 3.2 app.rs — Stream Event 处理

```rust
/// 获取当前时间戳（epoch ms）。取代不存在的 `timestamp_ms()`。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 截取字符串前 max 字符。取代不存在的 `truncate()`。
fn summarize(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { format!("{}...", &s[..max]) }
}

// ── ToolCall 事件 ──
DispatchStreamEvent::ToolCall { id, name, args } => {
    if let Some(idx) = data.streaming_msg_idx {
        let msgs = Rc::make_mut(&mut data.messages);
        if let Some(msg) = msgs.get_mut(idx) {
            msg.tool_calls.push(ToolCallState {
                id: id.clone(),
                name: name.clone(),
                args: args.clone(),
                status: ToolCallStatus::Running,
                result_summary: None,
                started_at: now_ms(),
                completed_at: None,
            });
            data.list_state.remeasure_items(idx..idx + 1);
        }
        cx.notify();
    }
}

// ── ToolResult 事件 (新签名) ──
DispatchStreamEvent::ToolResult { id, tool_name: _, content, is_error } => {
    if let Some(idx) = data.streaming_msg_idx {
        let msgs = Rc::make_mut(&mut data.messages);
        if let Some(msg) = msgs.get_mut(idx) {
            if let Some(tc) = msg.tool_calls.iter_mut().find(|t| t.id == id) {
                tc.status = if is_error {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                tc.result_summary = Some(summarize(&content, 80));
                tc.completed_at = Some(now_ms());
                data.list_state.remeasure_items(idx..idx + 1);
            }
        }
        cx.notify();
    }
}

// ── RoundStart 事件 ──
// 新 round = 新 assistant 消息，tool_calls 留在旧消息上无需清理
DispatchStreamEvent::RoundStart { .. } => {
    // ... 现有逻辑 (finalize reasoning, create new msg)
}
```

需在 `app.rs` 中同步更新 `ToolResult` 的枚举匹配处（所有 match arm）。

### 3.3 messages.rs — Tool Call 卡片渲染

在 `render_message()` 中，按顺序渲染：

```
1. Reasoning block (现有)
2. Tool call cards (新增) ← 遍历 msg.tool_calls
3. Content (现有)
```

```rust
// 工具类型 → (展示名, icon)
fn tool_display(name: &str) -> (&str, &str) {
    match name {
        "spawn_worker"    => ("🤖", "启动 worker"),
        "send_text"       => ("📨", "发送指令"),
        "read_output"     => ("📖", "读取输出"),
        "wait_for_output" => ("⏳", "等待完成"),
        "list_workers"    => ("📋", "列出 worker"),
        "close_worker"    => ("🗑", "关闭 worker"),
        _                 => ("🛠", "工具调用"),
    }
}
```

每个 `ToolCallState` 渲染为紧凑卡片：

```
┌──────────────────────────────────────────┐
│ 🤖 spawn_worker         ● running  0.3s  │  ← clickable header (expand/collapse)
│ agent: opencode, instruction: "..."      │  ← body (key params)
└──────────────────────────────────────────┘
```

- Running 态: 脉冲动画绿点，`⏱ 0.0s` 实时计时
- Completed: 绿色 ✓ + 耗时 `✓ 1.2s`
- Failed: 红色 ✕
- 参数显示: 提取关键字段 (agent, instruction, pattern, pane_id)，不展示原始 JSON
- 卡片背景: `muted`，左侧 3px 强调色条标识工具调用（区别于普通消息）
- 展开态: 显示完整参数 JSON（格式化缩进）+ 结果全文

### 3.4 messages.rs — 思考块样式强化

```rust
// 使用 h_flex + child div 模拟左侧 3px 色条
// GPUI 无 border_l_3()，用 div 元素实现
// 色条高度由外层 h_flex items_stretch 撑满，跟随内容
h_flex()
    .items_stretch()          // 确保左右子元素等高
    .child(
        div()
            .w(px(3))
            .bg(accent)
            .rounded_full()
    )
    .child(
        v_flex()
            .flex(1)
            .child(header)
            .child(body)
    )

// 折叠态: 只显示 "🧠 Thought X.Xs" 一行 header，body 隐藏
// 流式: max_h(px(200)).overflow_y_scrollbar() 替代 h(px(90))
// 注意: 当前 GPUI 版本提供 overflow_y_scrollbar，overflow_y_scroll 可能不存在
```

### 3.5 messages.rs — 消息时间线指示器

```rust
// 每条消息容器左侧绝对定位色条
// 仅在角色切换时显示 (is_first_in_group)
let indicator = if is_first_in_group {
    div()
        .absolute()
        .left(px(-6))
        .top(px(14))
        .w(px(3)).h(px(20))
        .rounded_full()
        .bg(match role {
            "user" => accent,
            "tool" => muted,
            _ => transparent,  // assistant 无
        })
        .into_any_element()
} else {
    div().into_any_element()
};

div()
    .relative()
    .child(indicator)
    .child(message_body)
```

`is_first_in_group` = 当前 role !== 上一条 role。连续同角色合并。

### 3.6 header.rs — Agent 监视器样式增强

```rust
fn status_pill(status: &str) -> AnyElement {
    let (bg, fg_color) = match status {
        "working" => (accent.with_alpha(0.15), accent),
        "done" => (green.with_alpha(0.15), green),
        "failed" => (red.with_alpha(0.15), red),
        _ => (muted.with_alpha(0.15), muted),
    };
    div()
        .px(px(6)).py(px(2)).rounded_full()
        .bg(bg).text_color(fg_color).text_size(px(9))
        .child(status)
}
```

每个条目增加 `recent_output[..60]` 显示为灰色预览文本。

## 4. 兼容性

- `ChatMessage.tool_calls` 为纯内存字段，不写入 SQLite。切换 session 后重建为空 Vec。
- 数据库中已有的 `role="tool"` 消息（当前无，因为 add_agent_block 是死代码）保持原样。
- `DispatchStreamEvent::ToolResult` 签名变更 — 需更新 `app.rs` 中所有 match arm。
- 旧消息 content 中的 inline markdown 工具文本兼容，不影响渲染。

## 5. 边界情况

| 场景 | 处理 |
|------|------|
| 同轮多次工具调用 | 每个 ToolCall 追加独立 ToolCallState，各 render 一个卡片 |
| 流式中断 (Esc) | tool_calls 保持最后状态，running 态的显式超时态 |
| 工具名不识别 | `tool_display()` fallback → `("🛠", "工具调用")` |
| 参数含敏感信息 | 参数摘要只展示类型信息 (agent, instruction key names)，不 dump 值全文 |
| RoundStart | 新 assistant 消息初始化空 tool_calls，旧消息保留 |
