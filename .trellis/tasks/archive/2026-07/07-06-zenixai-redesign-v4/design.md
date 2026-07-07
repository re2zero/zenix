# ZenixAI Redesign v4 — 技术设计

> 配套：`prd.md`（需求）/ `implement.md`（执行计划）
> 取代 `../07-05-zenixai-structural-redesign/design.md`（scope 错误）

## 1. 架构总览

### 1.1 核心模型：统一消息流

**现状**（4 个独立区域，视觉割裂）：
```
header
├─ agent_blocks 独立区域（msg_area 顶部）
├─ streaming_indicator 独立区域（msg_area 顶部）
├─ messages list（虚拟化）
└─ input bar
   + history/agents 浮层 overlay
```

**v4 目标**（单一消息流 + 抽屉）：
```
header（极简：title + new + menu）
├─ messages list（虚拟化，统一流）
│   ├─ role="user"      → accent 气泡，右对齐，88% 宽
│   ├─ role="assistant" → muted 气泡，左对齐，streaming tail cursor
│   ├─ role="tool"      → tool call 卡片（折叠/grouped）
│   ├─ role="error"     → alert 卡片 + 重试
│   └─ role="reasoning" → 折叠块（默认折叠 + duration）
├─ input bar（多行 + auto_grow）
│   └─ model selector（底部左侧）
└─ drawer overlay（history / agents 从右滑入，不遮挡 input）
```

### 1.2 设计原则

1. **复用 ChatMessage，不改 struct/DB schema**：`role: String` 已足够扩展（"tool"/"error"/"reasoning"）
2. **agent_blocks 双轨制**：运行时状态跟踪保留 `Vec<AgentBlock>`（lifecycle 管理）；渲染改为从 `messages` 读 role="tool" 条目。新增 agent block 时同时 push ChatMessage 到 messages
3. **虚拟化不破坏**：所有进入消息流的条目都是 ChatMessage，复用现有 `gpui::list` + `FollowMode::Tail`
4. **render-time 区分**：`render_message` 按 `role` 字符串 match，不同 role 走不同渲染分支

## 2. 数据模型改动

### 2.1 ChatMessage role 约定（不改 struct）

`src/ai/session.rs:22` 的 `ChatMessage` 不动。role 字符串扩展约定：

| role 值 | 含义 | content | agent_activities |
|---|---|---|---|
| `"user"` | 用户输入 | 用户文本 | None |
| `"assistant"` | AI 回复（含流式） | AI 文本 | None |
| `"tool"` | 工具/agent 调用 | 输出文本 | JSON: AgentBlock 元数据 |
| `"error"` | 错误 | 错误文本 | None |
| `"reasoning"` | 推理过程 | reasoning 文本 | JSON: `{duration_ms}` |

**agent_activities 复用**：role="tool" 时存 `serde_json::json!({"agent_name","status","summary","is_running","group_id"})`；role="reasoning" 时存 `{"duration_ms"}`。render 时按 role 解析。

### 2.2 AiChatPanelData 新字段

`src/ai/chat/state.rs:56` 新增：

```rust
// ── v4 视觉/交互 ──
pub ai_panel_width: f32,              // R11: 持久化宽度（取代 AI_PANEL_WIDTH 常量）
pub is_resizing_panel: bool,          // R11: 拖拽中（禁用 list pointer events 避免冲突）
pub drawer: Option<DrawerKind>,       // R8: 抽屉类型（取代 show_history/show_agents 两个 bool）
pub retry_last_prompt: Option<String>, // R7: 重试（复用 last_user_prompt 或独立）

// ── 多行输入历史 ──
pub input_history: Vec<String>,       // R10: 历史导航
pub input_history_idx: Option<usize>, // R10: 当前导航位置
```

移除/弃用：`show_history`/`show_agents` 合并为 `drawer: Option<DrawerKind>`；`config_bar_visible` 移除（config bar 重构进 input bar 底部）。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerKind {
    History,
    Agents,
}
```

### 2.3 AiChatAction 扩展

`src/ai/chat/state.rs:22` 新增 variants：

```rust
pub enum AiChatAction {
    // 现有
    Send, NewChat, SuggestionClick(String),
    SelectModel(String), SelectProvider(String),
    // 重构
    OpenMenu,                    // R4: header menu 展开
    OpenDrawer(DrawerKind),      // R8: 取代 ToggleHistory/ToggleAgents
    CloseDrawer,                 // R8: Esc/点遮罩
    RetryLast,                   // R7: 错误重试
    ResizePanel(f32),            // R11: 拖拽 handle
    NavigateHistory(HistoryDir), // R10: ↑↓ 历史
    // 移除: ToggleHistory, ToggleAgents, ToggleModelDropdown, ToggleProviderDropdown
}

pub enum HistoryDir { Up, Down }
```

## 3. 渲染管线改造

### 3.1 render_ai_chat_panel（`src/ai/chat/mod.rs`）

**删除**：
- `agent_blocks` 独立区域（mod.rs:90-97）→ role="tool" 进 messages
- `streaming_indicator` 独立区域（mod.rs:99-106）→ 嵌入 assistant tail
- `AI_PANEL_WIDTH` 常量使用（mod.rs:78）→ `data.ai_panel_width`

**新增**：
- 宽度用 `data.ai_panel_width`（clamp 360~720）
- drawer overlay 层（history/agents 从右滑入，绝对定位 w=320，半透明遮罩）
- resize handle（右边缘 4px 宽拖拽条，cursor=col-resize）

**结构**：
```rust
div().relative().w(px(data.ai_panel_width)).h_full().flex_col()
    .child(header::render_header(...))           // 极简
    .child(render_messages_or_empty(...))         // 统一消息流
    .child(input::render_input_bar(...))          // 多行 + model selector
    .when_some(data.drawer, |p, kind| {
        p.child(render_drawer_overlay(kind, ...)) // 抽屉 overlay
    })
    .child(render_resize_handle(...))             // 拖拽 handle
```

### 3.2 render_message（`src/ai/chat/messages.rs`）

按 role 分发到子模块：

```rust
pub fn render_message(msg: &ChatMessage, theme: &Theme, ctx: &RenderCtx) -> AnyElement {
    match msg.role.as_str() {
        "user"      => messages_user::render(msg, theme),
        "assistant" => messages_assistant::render(msg, theme, ctx),
        "tool"      => crate::ai::chat::tool_card::render(msg, theme),
        "error"     => crate::ai::chat::error_card::render(msg, theme),
        "reasoning" => crate::ai::chat::reasoning::render(msg, theme),
        _           => div().into_any_element(),
    }
}
```

`RenderCtx` 携带 streaming 上下文：
```rust
pub struct RenderCtx<'a> {
    pub is_streaming_tail: bool,  // 当前 msg 是否是流式最后一条
    pub is_hovered: bool,         // hover 状态（时间戳 fade）
}
```

### 3.3 streaming cursor 嵌入 tail（R6）

`render_message` 对 role="assistant"：
- 若 `ctx.is_streaming_tail == true`，在 content 末尾追加 pulsing block（gpui 动画 `Animation::new(Duration::from_millis(800)).repeat()`）
- 不再渲染独立 streaming_indicator

## 4. 各需求实现方案

### R1 assistant muted 气泡 / R2 user 响应式宽度

**改动**：`src/ai/chat/messages.rs`（或拆 `messages_user.rs`/`messages_assistant.rs`）

- user 气泡：`bg(accent)`, `text(accent_foreground)`, `ml(px(panel_width * 0.12))`, max_w `88%`
- assistant 气泡：`bg(secondary)` 或 `bg(muted)`, 全宽左对齐
- 宽度计算：render 时从 `data.ai_panel_width` 算 88%

**验证**：4 主题下对比，user/assistant 层级清晰。

### R3 时间戳 hover fade-in

**改动**：`messages.rs` render_message 包装一层 hover 容器

```rust
div()
    .id(msg_id)
    .on_hover_move(|hover, _, _| { /* update hover state */ })
    .child(content)
    .when(is_hovered, |p| p.child(
        div().absolute().top(px(-16)).text_size(px(9.))
            .text_color(muted_foreground)
            .child(format_timestamp(&msg.timestamp))
    ))
```

注意：gpui::list 每帧重建元素，hover 状态需存外部（HoverState map 或简化为 last_hovered_ix）。

### R4 header 极简

**改动**：`src/ai/chat/header.rs`

- 删 More 按钮、provider pill
- 保留：title「ZenixAI」+ new chat 按钮 + menu 按钮（gpui-component `menu`）
- menu 项：History / Agents / Clear chat / Settings

### R5 agent_blocks 进消息流

**改动**：`src/ai/chat/state.rs` + `agent_block.rs`

- `add_agent_block` 同时 push `ChatMessage { role: "tool", content: output, agent_activities: Some(json) }` 到 messages
- `agent_blocks: Vec<AgentBlock>` 保留运行时状态（is_running/is_expanded），通过 `block_id` 与 messages 关联
- render role="tool"：从 agent_activities JSON 解析 AgentBlock 状态，渲染折叠卡片
- **grouped block**：连续 ≥2 个 role="tool" 且同 group_id 的消息，render 时合并为一个 accordion（gpui-component `accordion` 或 `collapsible`）

### R6 streaming cursor 嵌入 tail

见 §3.3。

### R7 错误卡片化

**改动**：新增 `src/ai/chat/error_card.rs`

- 错误 push 为 `ChatMessage { role: "error", content: err_text }`
- render role="error"：gpui-component `alert` (variant=danger) + 重试按钮
- 重试按钮 → `AiChatAction::RetryLast` → 用 `last_user_prompt` 重新 `start_chat_streaming`

### R8 history/monitor 抽屉

**改动**：`src/ai/chat/header.rs`（移除浮层）+ 新增 `src/ai/chat/drawer.rs`

- drawer overlay：绝对定位 right=0, w=320, h_full, bg=background, border_l, shadow
- 半透明遮罩：absolute inset=0, bg=black/40, on_click=CloseDrawer
- history 按 date 分组：parse `created_at` → 今天/昨天/本周/更早
- 滑入动画：gpui `Animation::new(Duration::from_millis(200))` translate_x

### R9 reasoning 折叠块

**改动**：新增 `src/ai/chat/reasoning.rs`

- engine 的 `StreamEvent::Thinking`（若存在）push 为 `ChatMessage { role: "reasoning", content, agent_activities: Some(json!({"duration_ms": N})) }`
- render role="reasoning"：折叠块（默认折叠），标题「思考了 Ns」，点击展开
- 若 engine 无 Thinking 事件，此需求降级为「不显示」而非报错

**需确认**：engine.rs 是否有 StreamEvent::Thinking。direction §6 决策 15 提到「engine 有 StreamEvent::Thinking 但 chat 不消费」。implement 前先 grep 确认。

### R10 多行输入 + 历史导航

**改动**：`src/ai/chat/input.rs`

- `InputState::new(cx).multi_line(true).auto_grow(true)`
- Shift+Enter 换行（InputState 内置），Enter 发送（key_down 拦截）
- ↑/↓ 历史导航：input_entity 的 key_down handler，当 input 在行首/行尾且非多行中间，↑ 调出 `input_history[idx]`
- 发送时 push 到 `input_history`

### R11 响应式宽度 + 拖拽 handle

**改动**：`src/ai/chat/mod.rs` + `src/app.rs`（config）+ `src/config.rs`

- `ZenixApp.ai_panel_width: f32`（持久化到 config.ai_panel_width）
- resize handle：`div().absolute().right(0).w(px(4)).cursor_col_resize().on_drag(...)`，拖拽时 clamp 360~720，更新 `data.ai_panel_width`
- 持久化：拖拽 end 时写 config，或 debounce 写
- 移除 `AI_PANEL_WIDTH` 常量（或保留为 default 值）

### R12 markdown 扩展

**改动**：`src/ai/chat/messages.rs` 的 markdown parser（或拆 `markdown_ext.rs`）

- 现有 6 segment 保留，新增：heading（#/##/###）、blockquote（>）、table（| |）、link（[text](url)）、task（- [ ]/- [x]）
- code block：用 gpui-component `highlighter` 替代手写着色
- table：用 gpui-component `table`
- 无新直接依赖（highlighter/table 是 gpui-component 内置）

**需确认**：highlighter crate 的公开 API。implement 前 grep `crates/ui/src/highlighter/`。

### R13 model selector 移到 input bar 底部

**改动**：`src/ai/chat/input.rs` + `src/ai/chat/header.rs`

- header 删 model/provider pill
- input bar 底部左侧加 model selector：gpui-component `select` 或 `combobox`（`SearchableVec<String>` 直接做 delegate，参 memory 1441）
- 显示当前 model，点击展开 provider/model 列表

### R14 suggestion chips 上下文相关

**改动**：`src/ai/chat/header.rs`（empty state）

- 检测上下文：有无 git repo / 有无活跃 pane / 有无选中文件
- 3 类 chip：workspace（「分析当前项目结构」）、git（「总结当前 git 改动」）、code（「解释当前文件」）
- 点击 → `AiChatAction::SuggestionClick(text)` → 填入 input 或直接发送

## 5. 模块改动清单

### 新增文件

| 文件 | 职责 |
|---|---|
| `src/ai/chat/tool_card.rs` | role="tool" 渲染（折叠/grouped/lifecycle） |
| `src/ai/chat/error_card.rs` | role="error" 渲染（alert + 重试） |
| `src/ai/chat/reasoning.rs` | role="reasoning" 渲染（折叠块 + duration） |
| `src/ai/chat/drawer.rs` | history/agents 抽屉 overlay |
| `src/ai/chat/resize.rs` | 面板拖拽 handle 逻辑 |
| `src/ai/chat/markdown_ext.rs` | 扩展 markdown parser + highlighter 集成 |

### 修改文件

| 文件 | 改动 |
|---|---|
| `src/ai/chat/mod.rs` | 删 agent_blocks/streaming 独立区域；宽度改 ai_panel_width；加 drawer overlay + resize handle；render_message 按 role 分发 |
| `src/ai/chat/state.rs` | 新字段（ai_panel_width/drawer/input_history）；AiChatAction 扩展；add_agent_block 同时 push message |
| `src/ai/chat/messages.rs` | render_message 按 role 分发；user/assistant 气泡重做；时间戳 hover；streaming tail cursor |
| `src/ai/chat/input.rs` | 多行 + auto_grow + 历史导航 + model selector 底部 |
| `src/ai/chat/header.rs` | 极简化 + menu + empty state 上下文 chips；删 history/agents 浮层 |
| `src/ai/chat/agent_block.rs` | 保留 AgentBlock struct；渲染逻辑移到 tool_card.rs |
| `src/app.rs` | ai_panel_width 持久化；start_chat_streaming 支持 role="error"/"reasoning" push；DrawerKind action 处理 |
| `src/config.rs` | 新增 ai_panel_width 配置字段 |

### 移除

- `AI_PANEL_WIDTH` 常量（mod.rs:41）→ 改用 `data.ai_panel_width`（default 420）
- `show_history` / `show_agents` bool → 合并为 `drawer: Option<DrawerKind>`
- `config_bar_visible` / `config_dropdown` → 移到 input bar 底部 model selector

## 6. 状态机

### 6.1 流式状态

```
idle ──Send──▶ waiting_ttft ──first_token──▶ streaming ──done──▶ idle
   ▲                │                              │
   │                └──error──▶ error_shown ──retry──▶ waiting_ttft
   │                                   │
   └───────────────────────────────────┘
```

- `waiting_ttft`：用户发消息后到首个 token（可选 typing indicator 三个点）
- `streaming`：streaming_msg_idx 锁定最后一条 assistant msg，tail cursor 动画
- `error_shown`：role="error" 消息 push 进流，重试 → 用 last_user_prompt 重新 start

### 6.2 tool lifecycle

```
pending ──start──▶ running ──exit_code=0──▶ success
                       │
                       └──exit_code!=0──▶ error
```

- AgentBlock.status 字段跟踪
- render role="tool" 时按 status 显示状态点（badge/tag）
- grouped block：连续同 group_id 的 tool 消息合并渲染

### 6.3 drawer 状态

```
closed ──OpenDrawer(kind)──▶ open(kind) ──Esc/遮罩点击──▶ closed
                                  │
                                  └──OpenDrawer(other)──▶ open(other)（切换）
```

## 7. 配置链路

### 7.1 ai_panel_width 持久化

```
拖拽 handle ──▶ data.ai_panel_width 更新（实时 render）
            ──▶ debounce 500ms ──▶ config.ai_panel_width = value
                               ──▶ write ~/.config/zenix/config.json

启动 ──▶ config.ai_panel_width ──▶ ZenixApp.ai_panel_width ──▶ data.ai_panel_width
       （clamp 360~720，default 420）
```

### 7.2 provider/model 链路（D10-D12 不变）

`start_chat_streaming`（app.rs 自由函数）：
- provider 按 `data.selected_provider_id` 查找
- model 从 `data.selected_model` override
- 现有逻辑保留，model selector 移位不影响

## 8. 兼容性

### 8.1 session DB 不迁移

- ChatMessage struct 不改
- role 字符串扩展（"tool"/"error"/"reasoning"）与现有 "user"/"assistant" 共存
- 旧 session 加载时，未知 role 跳过渲染（match _ 分支）

### 8.2 保留资产不破坏

| 资产 | v4 影响 |
|---|---|
| D1 流式 | streaming_msg_idx 机制保留；tail cursor 是渲染层改动，不动数据流 |
| D2/D3 虚拟化 | list_state + FollowMode::Tail 保留；role 扩展不改 list 结构 |
| D10-D12 配置 | start_chat_streaming 不动；model selector 移位是 UI 改动 |
| chat/ 6 模块 | 全保留；新增 6 个子模块（tool_card/error_card/reasoning/drawer/resize/markdown_ext） |
| Tab 拦截 | 不涉及（chat input 的 Tab 处理独立于终端 PTY Tab） |

### 8.4 tokio runtime 约束

所有 GPUI executor 上的 tokio 操作用 `engine.rs::tokio_handle()`（参 memory 1938/1951）。start_chat_streaming 已正确使用，新增代码遵循同样模式。

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| role 扩展导致旧 session 渲染异常 | match _ 分支返回空 div；加载旧 session 时过滤未知 role |
| grouped tool block 与虚拟化冲突 | group 渲染在单个 list item 内（连续 tool 消息合并为一个 render 输出），不改 list item 数量 |
| highlighter/table API 不符 | implement phase 3 前先 grep `crates/ui/src/highlighter/` + `table/` 确认 API |
| 拖拽 handle 与 list 滚动事件冲突 | handle 用 `on_drag` 独立手势，拖拽中设 `is_resizing_panel=true` 禁用 list pointer events |
| markdown 手写 parser 边界 case | 测试覆盖：heading/blockquote/table/link/task/code-block 嵌套组合 |
| 一次性 PR 过大 | implement.md 分 3 phase 独立可验证，但合并为单 PR（atomic） |

## 10. 实施顺序约束

依赖图（决定 phase 划分）：

```
Phase 1（视觉层，不动数据流）:
  R1/R2（气泡）→ R3（时间戳）→ R4（header 极简）
  R13（model selector 移位）— 可并行
  无依赖，最先做

Phase 2（信息架构，改数据流）:
  R5（agent_blocks 进流）→ R7（error 进流）→ R6（streaming tail）
  R9（reasoning）— 依赖 R5 模式
  R8（drawer）— 独立
  依赖 Phase 1 的 render_message 分发

Phase 3（功能补全）:
  R10（多行输入）→ R11（响应式宽度）
  R12（markdown 扩展）
  R14（suggestion chips）
  依赖 Phase 1/2 完成
```

详见 `implement.md`。
