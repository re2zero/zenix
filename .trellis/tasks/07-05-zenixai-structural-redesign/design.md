# Design — ZenixAI 结构性重做

> 配套：`prd.md`（需求/约束/验收）、`implement.md`（执行清单）。本文只讲**怎么做**。

## 1. 架构总览

### 1.1 模块拆分（强制，修 N3.1）

废弃 `src/ai/chat.rs` 1364 行单文件，按职责拆为：

```
src/ai/chat/
├── mod.rs              # panel 入口：render_ai_chat_panel + 顶层 v_flex
├── state.rs            # AiChatPanelData + 输入历史 + streaming state
├── messages.rs         # uniform_list + ListState + follow_tail + 单条渲染
├── input.rs            # 多行输入 + 快捷键 + 发送/停止
├── agent_block.rs      # 工具调用卡片 + AgentStatus 枚举
├── error_card.rs       # 错误卡片渲染 + 重试
├── resize.rs           # 拖拽分隔条 + 持久化
└── markdown_ext.rs     # Heading/Blockquote/Table/Link/TaskItem 扩展
```

`src/ai/markdown.rs` 保持原位（不动旧的 6 个 segment），`markdown_ext.rs` 通过 `pub use` 组合扩展后的 enum。或者将 `MdSegment` 提到 `markdown.rs` 公共定义并在此文件追加变体——后续实现时根据循环引用情况择一。

### 1.2 关键状态字段变更（state.rs）

```rust
pub struct AiChatPanelData {
    // ── 现有保留 ─────────────────────────────────
    pub messages: Vec<ChatMessage>,                  // 保留
    pub input_value: String,                         // 保留
    pub current_session_id: Option<String>,
    pub current_session_title: Option<String>,
    pub config_bar_visible: bool,
    pub selected_provider_id: String,
    pub selected_model: String,
    pub available_models: Vec<String>,
    pub thinking_level: ThinkingLevel,
    pub selected_workspace: Option<String>,
    pub workspaces: Vec<WorkspaceOption>,
    pub config_dropdown: Option<ConfigDropdownTarget>,
    pub show_history: bool,
    pub history_sessions: Vec<Session>,
    pub agent_blocks: Vec<AgentBlock>,
    pub next_block_id: usize,

    // ── 新增/修改 ───────────────────────────────
    pub is_streaming: bool,                          // 保留但语义不变
    pub streaming_msg_idx: Option<usize>,            // NEW：当前正在生成的消息在 messages 中的下标，None 表示非流式
    pub abort_handle: Option<tokio::sync::oneshot::Sender<()>>,  // NEW：停止流发送端
    pub list_state: gpui::ListState,                 // NEW：uniform_list 滚动状态
    pub follow_tail: bool,                           // NEW：是否自动跟随尾部
    pub scrolled_off_count: usize,                   // NEW：已 FIFO 滚出的消息数，用于"已滚出 N 条"提示
    pub input_history: Vec<String>,                  // NEW：↑↓ 历史导航
    pub input_history_idx: Option<usize>,            // NEW：当前导航位置
    pub last_error: Option<crate::ai::error::AiError>,  // NEW：最近一次错误（用于重试）
    pub last_user_prompt: Option<String>,            // NEW：用于错误重试
}
```

**移除**：原 `is_streaming: bool` 位置不变但配合 `streaming_msg_idx` 一起用——`is_streaming=true && streaming_msg_idx=Some(i)` 表示流式中。

### 1.3 关键类型变更

```rust
// agent_block.rs
#[derive(Debug, Clone)]
pub enum AgentStatus {
    Pending,
    Running { started_at: std::time::Instant },
    Success { dur_ms: u64 },
    Failed { err: String },
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct AgentBlock {
    pub agent_name: String,
    pub status: AgentStatus,            // 替换原 status: String
    pub output: String,
    pub summary: Option<String>,
    pub pane_id: String,
    pub is_expanded: bool,
    pub block_id: usize,
    pub is_running: bool,                // 保留（旧字段，由 status 派生）
    pub output_history: String,
}
```

```rust
// error_card.rs
#[derive(Debug, Clone)]
pub enum ErrorKind {
    Network,
    Auth,
    RateLimit,
    Parse,
    Stream(String),
    Unknown,
}

impl ErrorKind {
    pub fn from_ai_error(e: &crate::ai::error::AiError) -> Self { /* ... */ }
}
```

## 2. 流式渲染数据流（修 D1，核心）

### 2.1 总流程

```
[用户按 Enter]
  ↓
src/ai/chat/input.rs::handle_send()
  ├─ push user msg → data.messages
  ├─ push 空 assistant msg → data.messages (streaming_msg_idx = Some(idx))
  ├─ is_streaming = true
  ├─ 创建 oneshot channel (tx 存入 data.abort_handle)
  └─ cx.spawn(async move |this, cx| {
       let abort_rx = rx;  // 持有 abort 接收端
       let result = engine.chat_and_dispatch(
           &messages_snapshot,
           &mut herdr_client,
           workspace_id,
           |event| {
               // 注意：此闭包运行在 tokio worker 线程
               // 必须用 cx.update 或 channel 把事件送回 GPUI 线程
               let _ = cx.update(|cx| {
                   let _ = this.update(cx, |this, cx| {
                       handle_dispatch_event(this, event, cx);
                   });
               });
           },
       ).await;
       // 收尾：置 is_streaming=false, streaming_msg_idx=None
       let _ = cx.update(|cx| {
           let _ = this.update(cx, |this, cx| {
               this.is_streaming = false;
               this.streaming_msg_idx = None;
               this.abort_handle = None;
               cx.notify();
           });
       });
   });
```

### 2.2 事件处理函数

```rust
// input.rs / state.rs
fn handle_dispatch_event(
    this: &mut AiChatPanelData,
    event: DispatchStreamEvent,
    cx: &mut gpui::Context<'_, ZenixApp>,
) {
    match event {
        DispatchStreamEvent::Token(text) => {
            if let Some(idx) = this.ai_chat_panel.as_mut().and_then(|d| d.streaming_msg_idx) {
                if let Some(data) = this.ai_chat_panel.as_mut() {
                    if let Some(msg) = data.messages.get_mut(idx) {
                        msg.content.push_str(&text);
                        if data.follow_tail {
                            // ListState 自动跟随（uniform_list 的 scroll_to 行为）
                            data.list_state.scroll_to(gpui::ListScrollTo::Bottom);
                        }
                        cx.notify();
                    }
                }
            }
        }
        DispatchStreamEvent::AgentOutput { agent_name, text } => {
            // 找到或创建对应 agent_block，追加到 output_history
            // ...
        }
        DispatchStreamEvent::AgentComplete(result) => {
            // 更新对应 agent_block 状态为 Success/Failed
            // ...
        }
        DispatchStreamEvent::Error(err) => {
            // 在 messages 中 push 一条 role="error" 的卡片
            // ...
        }
        DispatchStreamEvent::Done | DispatchStreamEvent::Compacted(_) => { /* 流结束 */ }
        DispatchStreamEvent::Dispatch(_) | DispatchStreamEvent::ToolCall(_, _) | DispatchStreamEvent::ToolResult(_, _, _) => { /* tool 相关 */ }
    }
}
```

### 2.3 Stop 实现

```rust
fn stop_streaming(this: &mut ZenixApp, cx: &mut Context<'_, ZenixApp>) {
    if let Some(data) = this.ai_chat_panel.as_mut() {
        if let Some(tx) = data.abort_handle.take() {
            let _ = tx.send(());  // 通知 spawn 任务中止
        }
        // 注意：abort 实际生效需要 engine 支持，或 spawn 任务用 select! 监听 rx
        // 当前 engine.chat_and_dispatch 不支持 cancellation，
        // 因此 spawn 任务里用 tokio::select! {
        //     _ = abort_rx => { /* 中止 */ }
        //     res = chat_and_dispatch(...) => { /* 正常完成 */ }
        // }
        // 中止时保留 streaming_msg_idx 指向的部分内容
        data.is_streaming = false;
        data.streaming_msg_idx = None;
        cx.notify();
    }
}
```

**关键约束**：engine.chat_and_dispatch 不支持取消——通过 `tokio::select!` 在 spawn 任务中竞争 abort_rx 和 chat_and_dispatch 的 future。被 abort 时 chat_and_dispatch 的 future drop，HTTP 连接断开，部分内容保留。

### 2.4 线程安全注意

- engine 闭包运行在 tokio worker 线程，不能直接 `entity.update()`
- 必须用 `cx.update(|cx| { let _ = this.update(cx, |this, cx| { ... }); })` 跨线程
- `cx: &mut AsyncApp` 的 update 方法是线程安全的

## 3. 虚拟化方案（修 D2, D3）— 用 gpui::list（非 uniform_list）

### 3.1 选型：list vs uniform_list

**Phase 0 Spike-1 验证结论**：chat 消息含 markdown 块，高度不均匀——必须用 `gpui::list`（变高度），不能用 `gpui::uniform_list`（仅等高，会布局错乱）。

**`gpui::list` 原生 follow-tail 加成**（关键发现，省去自己实现）：
- `ListState::new(item_count, ListAlignment::Bottom, overdraw)` —— Bottom 对齐即 chat log 语义
- `FollowMode::Tail` + `FollowState` —— gpui 已内置自动跟随逻辑
- `ListScrollEvent { is_following_tail, visible_range, ... }` —— scroll_handler 回调可直接读跟随状态
- `scroll_to(ListOffset)` 在用户上滚时自动调用 `follow_state.stop_following()`

参考：`gpui/src/elements/list.rs:24-784`（zed 仓库 d989c7c）。

### 3.2 渲染入口

```rust
use gpui::{list, ListState, ListAlignment, ListOffset, px};

// state.rs
pub list_state: ListState,
// 初始化
list_state: ListState::new(0, ListAlignment::Bottom, px(400.)),

// render 时
let list_state = data.list_state.clone();
let entity = cx.entity().downgrade();
let theme = theme.clone();

let msg_list = list(
    list_state,
    move |i, window, cx| {
        // 注意：render_item 在 list 内部 layout 时被调用
        // 用 upgrade 拿到 ZenixApp 实体读取 messages
        let Some(app) = entity.upgrade() else {
            return div().into_any_element();
        };
        let app_ref = app.read(cx);
        let Some(data) = app_ref.ai_chat_panel.as_ref() else {
            return div().into_any_element();
        };
        let Some(msg) = data.messages.get(i) else {
            return div().into_any_element();
        };
        render_one_message(msg, i, &theme, cx).into_any_element()
    },
)
.with_sizing_behavior(gpui::ListSizingBehavior::Infer)
.flex_grow()
.min_h_0();
```

### 3.3 follow_tail 自动跟随

gpui 已内置 follow_tail 状态机。当用户上滚时，scroll 事件触发 `scroll_to` → 自动调用 `stop_following()`。要恢复跟随：在 token 追加前检查并设置跟随模式。

```rust
// 流式 token 追加后
fn after_token_append(this: &mut AiChatPanelData, cx: &mut Context<'_, ZenixApp>) {
    // 用 splice 通知 list 增量（而非 reset，避免重新测量全部）
    // 实际：消息条数未变，只是 content 变了 → 触发 list 重新测量最后一项
    this.list_state.splice(0..0, 0);  // 哑 splice 触发重测
    // 若 follow_tail，scroll 到底部
    if data_following_tail(this) {
        let total = this.messages.len();
        this.list_state.scroll_to(ListOffset { item_ix: total, offset_in_item: px(0.) });
    }
    cx.notify();
}

fn data_following_tail(this: &AiChatPanelData) -> bool {
    // 检查最近一次 scroll_handler 回调的 is_following_tail 字段
    this.last_follow_tail
}
```

注册 scroll_handler 监听跟随状态变化：
```rust
// 在 list_state 初始化或 render 时安装
list_state = list_state.clone();
// gpui ListState 内部 scroll_handler 字段：通过 with_scroll_handler 链（若 API 提供）
// 或在 ListState::new 后用 Rc<RefCell> 共享状态，scroll_handler 更新 data.last_follow_tail
```

**回退方案**（若 ListState 没暴露 scroll_handler setter）：用 `is_scrollbar_dragging()` + `viewport_bounds()` + 最后一条 `bounds_for_item()` 判断是否在底部。

### 3.4 FIFO 滚动上限

```rust
fn push_message(this: &mut AiChatPanelData, msg: ChatMessage) {
    if let Some(data) = this.ai_chat_panel.as_mut() {
        const MAX: usize = 200;
        if data.messages.len() >= MAX {
            data.messages.remove(0);
            data.scrolled_off_count += 1;
            // 通知 list 第一项被移除
            data.list_state.splice(0..1, 0);
        }
        data.messages.push(msg);
        // 通知 list 新增一项
        let new_len = data.messages.len();
        data.list_state.splice(new_len-1..new_len-1, 1);
    }
}
```

渲染时在 list 顶部加一个"已滚出 N 条"提示条（独立 div，不算入 list item count）。

### 3.5 committed list + streaming tail 优化

`list` 内部用 SumTree 缓存已测量项的 layout。流式中只修改 `messages[streaming_msg_idx].content`，list 重新测量时**只重测 content 变化的项**。其他项高度不变则 layout 缓存命中。GPUI 的 list 用 SumTree 优化，单条变更不会全量重测。

若实测性能仍不达标：用 `list_state.splice(..)` 显式声明只最后一项变化。

### 3.3 FIFO 滚动上限

```rust
fn push_message(this: &mut AiChatPanelData, msg: ChatMessage) {
    if let Some(data) = this.ai_chat_panel.as_mut() {
        const MAX: usize = 200;
        if data.messages.len() >= MAX {
            data.messages.remove(0);
            data.scrolled_off_count += 1;
        }
        data.messages.push(msg);
    }
}
```

渲染时在 list 顶部加一个"已滚出 N 条"提示条（独立 div，不算入 list item count）。

### 3.4 committed list + streaming tail 优化

`uniform_list` 的 item count = `messages.len()`。流式中只修改 `messages[streaming_msg_idx].content`，其他项 content 不变。GPUI 的 uniform_list 内部会缓存已渲染项的 layout，**只要 content 长度不变就不会重新布局**。流式只影响最后一项的渲染预算。

若实测性能不达标，加 fast path：流式中只重渲染最后一项，通过 `uniform_list` 的 `range` 参数判断是否包含 streaming idx。

## 4. 多行输入（修 D5）

### 4.1 选型

- **优先**：gpui-component 的 `TextArea` 或 `TextInput` 多行模式（若存在）
- **回退**：现有 `gpui_component::input::InputState` + 自己计算行数 → 设置 `h(px(line_count * LINE_HEIGHT))`，超过 max 则 `overflow_y_scroll()`

### 4.2 实现

```rust
// input.rs
fn render_input_bar(data, theme, input_state, cx) -> impl IntoElement {
    let line_count = data.input_value.lines().count().max(1).min(8);
    let input_h = px(line_count as f32 * 22.0);  // 22px per line

    h_flex()
        .gap(px(8.))
        .items_end()
        .px(px(12.))
        .py(px(12.))
        .border_t_1()
        .border_color(border)
        .child(
            div()
                .flex_grow()
                .h(input_h)
                .when(line_count > 1, |d| d.overflow_y_scrollbar())
                .child(Input::new(input_state).multi_line(true)...)  // 若 API 支持
        )
        .child(send_button(data))
}
```

### 4.3 历史导航

```rust
fn handle_key_down(event, this, cx) {
    match event.keystroke {
        Key::Up if data.input_value.is_empty() => {
            if let Some(idx) = data.input_history_idx {
                let new_idx = idx.saturating_sub(1);
                if new_idx < data.input_history.len() {
                    data.input_history_idx = Some(new_idx);
                    let v = data.input_history[new_idx].clone();
                    set_input_value(this, v, cx);
                }
            } else if !data.input_history.is_empty() {
                let idx = data.input_history.len() - 1;
                data.input_history_idx = Some(idx);
                let v = data.input_history[idx].clone();
                set_input_value(this, v, cx);
            }
        }
        Key::Down if data.input_value.is_empty() => { /* 反向 */ }
        _ => {}
    }
}
```

## 5. Markdown 扩展（修 D6）

### 5.1 扩展 MdSegment

```rust
// markdown.rs（或 markdown_ext.rs）
#[derive(Debug, Clone)]
pub enum MdSegment {
    // ── 原有 ──
    Plain(String),
    Bold(String),
    Italic(String),
    InlineCode(String),
    CodeBlock(String),
    ListItem(String),

    // ── 新增 ──
    Heading { level: u8, text: String },       // 1-6
    Blockquote(Vec<MdSegment>),                 // 嵌套，内部复用 inline parser
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    Link { text: String, url: String },
    TaskItem { checked: bool, text: String },
}
```

### 5.2 parser 扩展

在 `parse_markdown` 主循环中增加分支：

```rust
fn parse_markdown(text: &str) -> Vec<MdSegment> {
    let mut segments = Vec::new();
    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        // 原有 code block / list / inline 处理 ...

        // NEW: Heading
        if let Some(captures) = parse_heading(line) {
            segments.push(MdSegment::Heading { level: captures.0, text: captures.1 });
            continue;
        }
        // NEW: Blockquote
        if line.starts_with("> ") {
            let mut content = vec![line[2..].to_string()];
            while let Some(next) = lines.peek() {
                if next.starts_with("> ") { content.push(lines.next().unwrap()[2..].to_string()); }
                else { break; }
            }
            let inner = parse_inline(&content.join("\n"));
            segments.push(MdSegment::Blockquote(inner));
            continue;
        }
        // NEW: TaskItem
        if let Some(captures) = parse_task_item(line) {
            segments.push(MdSegment::TaskItem { checked: captures.0, text: captures.1 });
            continue;
        }
        // NEW: Table（连续 | xxx | yyy | 行 + 分隔行 |---|---|）
        if line.starts_with('|') && lines.peek().map_or(false, |n| n.starts_with("|") && n.contains('-')) {
            // 收集 table
            // ...
            continue;
        }
        // 普通 inline（含 Link 识别）
        let inlined = parse_inline(line);  // 内部追加 Link 识别
        segments.extend(inlined);
    }
    segments
}
```

### 5.3 inline parser 加 Link

```rust
fn parse_inline(s: &str) -> Vec<MdSegment> {
    let mut out = Vec::new();
    let mut rest = s;
    let link_re = regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();  // 编译到 lazy_static
    // ... 在原有 ` ** ` * ` 扫描循环中先匹配 link 模式
}
```

**注意**：现有 Cargo.toml 是否已有 `regex`？需 spike 确认。若没有，手写简单状态机替代。

### 5.4 渲染扩展

```rust
fn render_segments(segments: &[MdSegment], theme: &Theme) -> Vec<AnyElement> {
    // 在原 match 基础上增加：
    MdSegment::Heading { level, text } => {
        let size = px(match level { 1 => 22.0, 2 => 20.0, 3 => 18.0, 4 => 16.0, 5 => 14.0, _ => 13.0 });
        txt(size).font_weight(FontWeight::BOLD).text_color(fg).line_height(size * 1.3).child(text)
    }
    MdSegment::Blockquote(inner) => {
        let inner_rendered = render_segments(&inner, theme);
        v_flex().border_l_2().border_color(muted).pl(px(12.)).children(inner_rendered)
    }
    MdSegment::Table { headers, rows } => {
        render_table(headers, rows, theme)  // 用 v_flex 嵌套 h_flex，简化 GFM
    }
    MdSegment::Link { text, url } => {
        let url_clone = url.clone();
        txt(px(13.)).text_color(accent).underline().child(text.clone())
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = open::that(&url_clone);
                cx.stop_propagation();
            })
    }
    MdSegment::TaskItem { checked, text } => {
        h_flex().gap(px(8.))
            .child(div().w(px(14.)).h(px(14.)).when(*checked, |d| d.bg(accent)).border_1())
            .child(txt(px(13.)).text_color(fg).child(text.clone()))
    }
}
```

## 6. 响应式宽度（修 D4）

### 6.1 状态

```rust
// ZenixApp（不是 AiChatPanelData，因为要持久化到 config）
pub ai_panel_width: f32,  // 从 config 加载，默认 480.0

// config.rs
pub struct ConfigFile {
    // ... 原有
    pub ai_panel_width: Option<f32>,  // NEW
}
```

### 6.2 拖拽分隔条

```rust
// resize.rs
const RESIZE_HANDLE_W: f32 = 4.0;
const MIN_W: f32 = 360.0;
const MAX_W: f32 = 720.0;
const MIN_TERMINAL_W: f32 = 400.0;

fn render_resize_handle(this: &ZenixApp, cx: &mut Context<'_, ZenixApp>) -> impl IntoElement {
    let entity = cx.entity().downgrade();
    div()
        .w(px(RESIZE_HANDLE_W))
        .h_full()
        .cursor_col_resize()
        .bg(transparent)
        .hover(|s| s.bg(border))
        .on_drag_move(move |e: &gpui::DragMoveEvent, cx| {
            let Some(app) = entity.upgrade() else { return; };
            app.update(cx, |this, cx| {
                let delta = e.event.position.x - e.drag_would_be.position.x;
                let new_w = (this.ai_panel_width - delta.x).clamp(MIN_W, MAX_W);
                let terminal_w = window_bounds.width - sidebar_w - new_w;
                if terminal_w < MIN_TERMINAL_W { /* 视觉提示但允许 */ }
                this.ai_panel_width = new_w;
                cx.notify();
            });
        })
        .on_drop(move |e: &gpui::DropEvent, cx| {  // 拖拽结束
            persist_ai_panel_width(cx);
        })
}
```

实际 API 名字（`on_drag_move`/`DragMoveEvent`/`DropEvent`）以 GPUI 当前版本为准，spike 验证。

### 6.3 持久化

```rust
fn persist_ai_panel_width(cx: &mut Context<'_, ZenixApp>) {
    let w = this.ai_panel_width;
    cx.spawn(async move |this, cx| {
        let mut cfg = crate::config::ConfigFile::load().await.unwrap_or_default();
        cfg.ai_panel_width = Some(w);
        let _ = cfg.save().await;
    }).detach();
}
```

## 7. 键盘快捷键（修 D9）

```rust
fn handle_chat_key_down(event: &KeyDownEvent, this: &mut ZenixApp, cx: &mut Context<'_, ZenixApp>) {
    match event.keystroke {
        Key::Escape => {
            if data.is_streaming { stop_streaming(this, cx); }
            else { clear_input(this, cx); }
        }
        Key { key: "l", modifiers: Modifiers { control: true, .. } } => {
            clear_messages_keep_session(this, cx);
        }
        Key::Up if data.input_value.is_empty() => { history_prev(this, cx); }
        Key::Down if data.input_value.is_empty() => { history_next(this, cx); }
        _ => {}
    }
}
```

注册在 chat 面板根 div 的 `on_key_down`，**不**用全局 keybinding（避免和终端抢键）。

## 8. 错误卡片（修 D7）

```rust
// error_card.rs
fn render_error_card(msg: &ChatMessage, theme: &Theme) -> AnyElement {
    let kind = ErrorKind::from_message(&msg.content);  // 解析序列化的错误类型
    v_flex()
        .w_full()
        .border_1()
        .border_color(red)
        .rounded_md()
        .p(px(12.))
        .gap(px(6.))
        .child(h_flex().gap(px(8.))
            .child(error_icon(&kind))
            .child(txt(px(13.)).font_weight(BOLD).text_color(red).child(kind.label())))
        .child(txt(px(12.)).text_color(muted).child(&msg.content))
        .when(kind.is_retryable(), |d| d.child(
            Button::new("retry").primary().compact().label("重试")
                .on_click(move |_, _, cx| { /* 重发 last_user_prompt */ })
        ))
        .into_any_element()
}
```

错误以 `role = "error"` 的 ChatMessage 形式插入 messages，渲染时分支处理。

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| **gpui::uniform_list API 在当前 Cargo.lock 版本下不可用或签名不同** | Spike-1：先写最小 demo 验证 API；若不可用退化为自实现虚拟化（基于 `overflow_y_scrollbar` + 仅渲染可见范围 items） |
| **engine 闭包跨线程调用 `cx.update` 失败**（spawn 任务 drop 后） | 用 `Downgrade<ZenixApp>` 而非强引用；update 返回 Result 时静默忽略 `Err`（任务已取消是预期） |
| **abort 通过 drop future 不会立即断开 HTTP**（reqwest 内部状态） | 可接受——HTTP 连接 drop 后底层 socket 通常被关闭；实测若仍有少量 token 流入，由事件处理幂等忽略即可 |
| **markdown parser 扩展引入正则依赖** | Spike-2：检查 Cargo.toml 是否已有 `regex`；没有则手写状态机（heading/blockquote/task 都不需要正则，只有 link 需要） |
| **ListState API 跟踪 follow_tail 不准** | 回退方案：在 list 底部加一个 sentinel `div().h(px(1.)).id("bottom_anchor")`，用 `scroll_to(Anchor::Bottom)` 替代 ListScrollTo::Bottom |
| **拖拽分隔条与现有 sidebar layout 冲突** | Spike-3：先看 ZenixApp 当前 render 中 sidebar/chat/terminal 的 flex 结构，确认 chat 面板是否在独立 flex 子项中（应在，否则要重构布局） |

## 10. 兼容性保证

- engine.rs 公开 API：**只读**消费 `StreamEvent`/`DispatchStreamEvent`/`ChatMessageRef`/`chat_stream`/`chat_and_dispatch`/`call_llm_collect`，签名一字不改
- session.rs：保留 `ChatMessage` 结构，新增 `role = "error"` 是字符串值不改类型
- config.rs：新增字段 `ai_panel_width: Option<f32>`，旧 config 文件缺省时用默认 480.0
- herdr IPC 协议：完全不动

## 11. 回滚策略

- 本轮所有改动应在 git 一个 feature branch（`feat/zenixai-structural-v3`）
- 若交付前发现致命问题，整体 `git checkout main` 即可回到上一个稳定状态
- 不分阶段合并——本轮作为一个原子 PR（避免半截状态导致 chat 完全不可用）

## 12. 实现顺序（详见 implement.md）

1. **Spike**：uniform_list API + markdown regex 依赖 + 拖拽 API（验证关键假设）
2. **基础拆分**：chat.rs → chat/ 模块树（纯结构，无功能改动）
3. **D1 流式接通**：cx.spawn + chat_and_dispatch + 增量追加（最小可用）
4. **D2/D3 虚拟化**：uniform_list + follow_tail + FIFO 上限
5. **D5 多行输入**
6. **D6 markdown 扩展**
7. **D4 响应式宽度 + 持久化**
8. **工具卡片化 + 错误卡片 + 键盘快捷键**
9. **回归测试 + 性能验证**
