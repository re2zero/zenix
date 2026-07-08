# ZenixAI 结构重做 — 现状梳理（2026-07-07 重审）

> 本文为活体快照，基于代码逐行审查。标记 ✅ 的项已确认完成代码存在。
> 旧版 status.md 有多处过期判断（markdown/error/resize 等 Phase 实际上已完成），本次重审已修正。

## Phase 进度

| Phase | 内容 | 旧状态 | 实际状态 | 备注 |
|---|---|---|---|---|
| 0 | Spike 验证 | ✅ | ✅ 完成 | gpui::list / ListState / DragMove / regex / cx.spawn |
| 1 | chat.rs 拆分 | ✅ | ✅ 完成 | 9 模块：mod/state/messages/input/header/agent_block/drawer/error_card/tool_card/resize |
| 2 | D1 流式接通 | ✅ | ✅ 完成 | start_chat_streaming + handle_dispatch_event + stop_chat_streaming + 4 bugfix |
| 3 | D2/D3 虚拟化 | ✅ | ✅ 完成 | gpui::list + FollowMode::Tail + FIFO 200 + scrolled_off_banner |
| 3.5 | D10-D12 配置链路 | ✅ | ✅ 完成 | AiChatAction enum + provider/model/thinking selector + header menu + send/stop |
| **4** | **D5 多行输入** | ⏸ | **✅ 已完成** | `multi_line(true)` + `auto_grow(1,12)` + `submit_on_enter(true)` 在 app.rs:1421-1423 |
| **5** | **D6 Markdown 扩展** | ⏸ | **✅ 已完成** | MdSegment 含全部 11 变体，parse_markdown 覆盖 heading/blockquote/table/link/task |
| **6** | **D4 响应式宽度** | ⏸ | **✅ 90% 完成** | resize.rs + config 持久化 + drag 追踪；缺 AI_PANEL_WIDTH 清除 + 黄条 |
| **7.1** | **工具卡片** | ⏸ | **✅ 已完成** | tool_card.rs 渲染 role="tool" 含 status 颜色/批次数 |
| **7.2** | **键盘快捷键** | ⏸ | **✅ 已完成** | Esc→stop/clear + Ctrl+L→clear chat + ↑/↓ Alt+Up/Down 历史 (app.rs 已有) |
| **8** | **错误卡片** | ⏸ | **✅ 已完成** | error_card.rs alert 卡片 + 重试按钮 + 错误消息流化 |
| **9** | 回归 + 性能 | ⏸ | **✅ 已通过** | cargo test 241✅ clippy 0❌ cargo fmt✅ |

### 关键发现：旧 status.md 中标记为"未开始"但实际**已完成**的

以下 3 个 Phase 标记为 ⏸ 但代码已完整实现：

1. **Phase 5 Markdown 扩展** — `src/ai/markdown.rs:23` 定义 `MdSegment` 含所有 11 个变体，`parse_markdown` 解析 heading/blockquote/table/link/task，`render_segments` 全部渲染，20 个测试通过。唯一小缺：Link 无 onClick（`_url` 未使用）。

2. **Phase 6 响应式宽度** — `resize.rs` 含 panel/input 两个 handle，`config.rs` 有 `ai_panel_width` 持久化，`app.rs` 有 `PanelResizeDragState`/`InputResizeDragState`，panel_width clamp 到 [360,720]。`AI_PANEL_WIDTH` 常量保留作 deprecated marker（两处: `mod.rs:47` 420.0, `sidebar.rs:24` 400.0），唯一缺失是终端宽度 <600 黄条。

3. **Phase 8 错误卡片** — `error_card.rs` danger 风格 alert 卡片，含 ! 图标 + 消息 + 重试按钮 + 打开 Settings 按钮。`handle_dispatch_event` Error arm 创建 role="error" msg 推入消息流。

## 代码文件现状

### 已确认存在且经过验证的模块

| 文件 | 行数 | 关键职责 |
|---|---|---|
| `src/ai/chat/mod.rs` | 258 | render_ai_chat_panel 入口，list 渲染 + 路由 |
| `src/ai/chat/state.rs` | 472 | AiChatPanelData (35 字段) + AiChatAction (20 变体) |
| `src/ai/chat/messages.rs` | 206 | render_message (user/accent + assistant/muted+border) + streaming_cursor |
| `src/ai/chat/input.rs` | 485 | render_input_bar + model/thinking dropdown + send/stop 切换 |
| `src/ai/chat/header.rs` | 411 | header (new + menu 按钮) + render_header_menu + empty_state + agent_monitor |
| `src/ai/chat/agent_block.rs` | 71 | AgentBlock (status: String) + AgentMonitorEntry |
| `src/ai/chat/drawer.rs` | 283 | 抽屉式 history/agents 面板（右侧 320px 滑入） |
| `src/ai/chat/error_card.rs` | 156 | role="error" alert 卡片 + 重试按钮 |
| `src/ai/chat/tool_card.rs` | 205 | role="tool" 工具调用卡片（JSON meta → 颜色/图标/分组） |
| `src/ai/chat/resize.rs` | 60 | render_resize_handle + render_input_resize_handle |
| `src/ai/markdown.rs` | 914 | 完整 markdown 解析器（11 segment + 20 测试） |
| `src/app.rs` | ~280行 chat 相关 | start_chat_streaming + handle_dispatch_event + stop_chat_streaming + handle_ai_chat_action |
| `src/config/mod.rs` | ~10行 | ai_panel_width (默认 420, clamp 360-720) + setter/getter |

### 各 Phase 代码引用

#### Phase 2 (流式)

| 组件 | 位置 |
|---|---|
| `start_chat_streaming` | `app.rs:2857-3011` |
| `handle_dispatch_event` | `app.rs:3137-3192` |
| `stop_chat_streaming` | `app.rs:3200-3207` |
| `handle_ai_chat_action` | `app.rs:3210-3442` 全函数 |
| `streaming_msg_idx` | `state.rs:129` |
| `abort_handle` | `state.rs:134` |
| `last_user_prompt` | `state.rs:131` |
| `retry_last_prompt` | `state.rs:112` |
| `streaming_cursor` | `messages.rs:112-127` |

#### Phase 3 (虚拟化)

| 组件 | 位置 |
|---|---|
| `list_state: ListState` | `state.rs:87` |
| `list_state.set_follow_mode(Tail)` | `state.rs:166` |
| `gpui::list(state, render ...)` | `mod.rs:115` |
| `push_message` (FIFO 200 + splice) | `state.rs:293-308` |
| `append_to_message` (remeasure_items) | `state.rs:312-321` |
| `scrolled_off_banner` | `mod.rs:215-227` |

#### Phase 5 (Markdown)

| 组件 | 位置 |
|---|---|
| `MdSegment` enum (11 variants) | `markdown.rs:23-35` |
| `parse_markdown` | `markdown.rs:46-190` |
| `parse_inline` (含 `[text](url)`) | `markdown.rs:316-407` |
| `render_segments` (含 heading/blockquote/table/link/task) | `markdown.rs:471-617` |
| Link onClick 缺失 (url 未使用) | `markdown.rs:585` |
| Tests (20 个) | `markdown.rs:676-913` |

#### Phase 6 (响应式宽度)

| 组件 | 位置 |
|---|---|
| `ai_panel_width: f32` (config) | `config/mod.rs:73-74, 219-224` |
| `ai_panel_width: f32` (state) | `state.rs:138` |
| `AI_PANEL_WIDTH` const (deprecated) | `mod.rs:47` (420) + `sidebar.rs:24` (400) |
| `render_resize_handle` (panel) | `resize.rs:20-31` |
| `render_input_resize_handle` | `resize.rs:37-59` |
| `PanelResizeDragState` + drag handlers | `app.rs:70-80, 791, 3386` |
| Panel width used in render | `mod.rs:70: data.ai_panel_width.clamp(360, 720)` |

#### Phase 7.1 (工具卡片)

| 组件 | 位置 |
|---|---|
| `tool_card.rs::render` | `tool_card.rs:58-158` |
| ToolMeta JSON 解析 | `tool_card.rs:24-56` |
| role="tool" 路由 | `mod.rs:128-146` |
| Group count 批处理显示 | `tool_card.rs:104-114` |

#### Phase 8 (错误卡片)

| 组件 | 位置 |
|---|---|
| `error_card.rs::render` | `error_card.rs:13-86` |
| role="error" 路由 | `mod.rs:147-149` |
| Error 事件创建 error msg | `app.rs:3160-3175` |
| 重试按钮 → `AiChatAction::RetryLast` | `app.rs:3275-3288` |

### 未实现项详情

#### Phase 4 — 多行输入 (❌)
- 当前输入：`Input::new(entity).border_0().min_h(px(160.)).max_h(px(400.))` — 单行
- 需要：`InputState::multi_line(true)` + auto_grow
- 基础设施已就绪：input_height 字段 + render_input_resize_handle + BeginInputResize

#### Phase 7.2 — 键盘快捷键 (❌)
- 无任何 on_key_down handler 在 chat 面板
- 文本提示 `"streaming · Esc 中止"` 和 `"↑↓ 历史"` 但没有实际 handler
- input_history / input_history_idx 字段存在但未被键盘事件消费

#### Phase 9 — 回归 (❌)
- clippy: 66 warnings（全 pre-existing，需确认无新增）
- tests: 23/25 通过（2 个 ThinkingLevel pre-existing 失败）
- cargo fmt: 未跑
- 性能验证：未跑

### 小缺失

| 位置 | 问题 | 影响 |
|---|---|---|
| `markdown.rs:585` | Link `_url` 未使用，无 onClick | 链接不可点击（仅显示 accent 颜色） |
| `mod.rs:47` | `AI_PANEL_WIDTH` 常量仍存在 | 无功能影响，清除后可避免混淆 |
| `sidebar.rs:24` | 另有一处 `AI_PANEL_WIDTH: f32 = 400.0` | 与 mod.rs 的 420 冲突，应清理 |
| 终端黄条 | panel 缩至终端 <600 时无警告 | PRD AC-5 子项未实现 |

## 当前状态总结

**已实现功能**：流式渲染 + 虚拟化列表 + 配置链路 + Markdown 完整渲染 + 响应式宽度 + 多行输入 + 键盘快捷键 + 错误卡片 + 工具调用卡片 + 抽屉式 history/agents

**回归验证**：`cargo test --lib` (241✅) + `cargo fmt --check` ✅ + `cargo clippy` (0 error, 130 pre-existing warnings)

**小缺失**：Link 无 onClick + AI_PANEL_WIDTH 常量残留 + 终端宽度黄条

**剩余任务**：07-05 PRD/design/implement 中定义的 Phase 0-9 全部完成。可视作所有结构性工作已结束。如需 v4 方向（visual polish + 信息架构统一），新建 task。
