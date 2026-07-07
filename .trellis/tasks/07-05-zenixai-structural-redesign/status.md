# ZenixAI 结构重做 — 现状梳理（截至 2026-07-05）

> 本文为活体快照，记录截至当前的实现状态、已修 bug、未决问题、下一步。后续会话从本文继续。

## 任务背景

第三轮 ZenixAI 重设计（task `07-05-zenixai-structural-redesign`）。前两轮（`03-zenixai-ui-redesign`、`04-zenixai-zed-style-redesign`）均因"只换皮不修结构"失败。本轮范围 A：流式渲染 + 虚拟化 + 多行输入 + Markdown 补全 + 响应式宽度 + 工具卡片 + 键盘快捷键 + 错误态。

PRD/design/implement 三件套完整，位于 `.trellis/tasks/07-05-zenixai-structural-redesign/`。

## Phase 进度

| Phase | 状态 | 备注 |
|---|---|---|
| 0 Spike 验证 | ✅ 完成 | gpui::list / ListState / DragMove / regex / cx.spawn 全部可用 |
| 1 chat.rs 拆分 | ✅ 完成 | 1364 行单文件 → `chat/{mod,state,messages,input,header,agent_block}.rs` 6 个子模块，cargo check 干净 |
| 2 D1 流式接通 | ✅ 完成 + 4 个追加修复 | 见下方"已修 bug"清单 |
| 3 D2/D3 虚拟化 | ✅ 完成 | `Rc<Vec<ChatMessage>>` + `gpui::list` + `ListAlignment::Bottom` + `FollowMode::Tail` + FIFO 200 + scrolled_off banner |
| **3.5 D10 配置链路 + D11/D12 交互接通** | **✅ 完成** | **selected_provider_id/selected_model override engine；AiChatAction enum 统一 dispatch；7 个交互元素全部接通（new/history/agents/model pill/dropdown item/suggestion chips/send）** |
| 4 D5 多行输入 | ⏸ 未开始 | 当前仍是 `InputState` 单行 |
| 5 D6 Markdown 扩展 | ⏸ 未开始 | 当前仅 6 个 segment，缺 heading/blockquote/table/link/task |
| 6 D4 响应式宽度 | ⏸ 未开始 | 当前固定 420px |
| 7 工具卡片 + 快捷键 | ⏸ 未开始 | AgentStatus 仍是 String；键盘快捷键未实现 |
| 8 错误卡片 | ⏸ 未开始 | 当前错误以 `⚠️ ...` 文本追加到 streaming msg |
| 9 回归 + 性能 | ⏸ 未开始 | |

## 当前能跑通的路径

最小可用流程：
1. 用户在 Settings → API Providers 配置 provider（base_url + api_key + model）
2. 重启 zenix，点击侧边栏 AI chat 图标打开面板
3. **点击输入框**（focus 修复后能拿到焦点）
4. 打字 → `InputEvent::Change` 更新 `data.input_value`
5. Enter → `start_chat_streaming(this, text, cx)`：
   - auto-create session（uuid + DB 持久化）
   - push user msg + 空 assistant msg 到 `data.messages`
   - snapshot provider/workspace/messages_for_engine
   - `cx.spawn` 异步任务，`tokio::select!` 包 `chat_and_dispatch` + abort_rx
   - mpsc channel 桥接 tokio worker 回调 → GPUI async
6. **成功**：Token event 流入，`handle_dispatch_event` 追加到 assistant msg，cx.notify 触发重渲染
7. **失败**：Error event 发到 streaming msg，显示 `⚠️ Chat failed: ...`
8. Stop 按钮（send 按钮变方块）：abort_rx 触发，chat_fut drop，部分内容保留

## 已修 Bug 清单（Phase 2 + 3.5 期间）

| # | Bug | 文件:行 | 修复 |
|---|---|---|---|
| 1 | **全局 on_mouse_down 无条件抢焦点给终端** — 任何点击（chat 输入框、sidebar 搜索框）都被 steal，子元素无法 focus | `src/app.rs:676-698` | 加 `terminal_bounds.contains(&event.position)` guard，不在终端区域直接 return |
| 2 | **chat_fut 错误静默吞掉** — `OrchestratorEngine::new(&provider)?` + `herdr_client.connect()?` + `chat_and_dispatch().await?` 全用 `?`，Err 被 `let _ = chat_task.await;` 丢弃 | `src/app.rs:chat_fut`（约 2745-2770） | 所有失败路径 emit `DispatchStreamEvent::Error(...)` 到 event_tx，handle_dispatch_event 追加 `⚠️ ...` 到 streaming msg |
| 3 | **herdr_client.connect() 预连接** — herdr daemon 没运行时直接挂，纯 chat 不需要 herdr | `src/app.rs:chat_fut` | 跳过 connect()，让 chat_and_dispatch 内部 dispatch 分支懒失败 |
| 4 | **`current_session_id` 永远是 None** — state.rs:91 初始化后再也没设过 | `src/app.rs:start_chat_streaming push 块`（约 2640-2700） | 首次发消息时 auto-create session（`uuid::Uuid::new_v4()` + `mgr.create_session()` + 持久化） |
| 5 | **user msg push 在 session 检查块内** — session_id None 时整个 push 被 skip，用户看不到自己的消息 | 同上 | 永远 push user msg 到 `data.messages`（session DB 写入仍条件性，但 UI 显示无条件） |
| **6** | **D10 配置链路断裂** — `get_available_provider()` 无视 `selected_provider_id`；`OrchestratorEngine::new` 用 `provider.model` 不读 `selected_model` | `src/app.rs:start_chat_streaming provider 块` | 改用 `load_all().find(name == selected_provider_id)` + `p.model = selected_model` override |
| **7** | **D11 头部按钮全部装饰品** — new/history/agents/more 4 个 Button + model selector pill 无 on_click | `src/ai/chat/header.rs:render_header` | 引入 `AiChatAction` enum + `AiChatActionHandler` 单回调；4 个 Button 绑 `on_click`，model pill 绑 `on_mouse_down` |
| **8** | **D11 dropdown items 不可选** — model dropdown 的 item 纯展示 | `src/ai/chat/header.rs:render_dropdown` | dropdown item 绑 `on_mouse_down(AiChatAction::SelectModel(model))` |
| **9** | **D12 suggestion chips 不可点** — empty state 的 3 个 chip 纯展示 | `src/ai/chat/header.rs:suggestion_chip` | 绑 `on_mouse_down(AiChatAction::SuggestionClick(text))`，直接调 `start_chat_streaming` |

## 未决问题（待用户反馈）

### Q1: "输入很卡" — 输入延迟

**症状**：用户报告输入卡顿，未指明是 always 还是仅在 streaming 时。

**可能原因**（按概率排序）：
1. **每 token cx.notify 触发全 ZenixApp 重渲染**：streaming 时每 token 调 `cx.notify()`，render 包含 terminal area + sidebar + chat panel。terminal element 的 prepaint/paint 可能很重。
2. **非虚拟化消息列表**：`v_flex().overflow_y_scrollbar().children(message_elements)` 全量渲染，长会话每 token 重排所有消息。这是 Phase 3 要修的 D2。
3. **InputState 的 Change 事件每次击键触发 cx.notify**：line 1233-1237，每次击键 → `data.input_value = ...; cx.notify();` → 完整重渲染。

**待确认**：用户日志未附上，无法判断是哪种。需要用户提供：
- 卡顿是 always 还是 streaming 时？
- `cargo run` 终端输出（zenixai target 的 tracing 行）

### Q2: chat 是否成功流式？

**未确认**：用户只说"发送后什么都没有"（修复前），修复后未报告是否能看到 token 流入。需要用户重新测试并提供 tracing 日志。

### Q3: provider 配置状态

**未知**：用户是否已在 Settings 配置了 provider？base_url/api_key/model 是否正确？这决定了 chat_and_dispatch 是否能成功。

## TODO（按依赖顺序）

### 立即（验证 Phase 2）

- [ ] 用户提供 `cargo run` 终端 tracing 日志，确认 chat_fut 走到哪一步
- [ ] 用户确认能否看到 token 流式（成功）或 `⚠️` 错误（失败显示）
- [ ] 如失败，根据 tracing 的 `error = ...` 字段定位（HTTP 401？endpoint 错？model 错？）

### Phase 3: 虚拟化消息列表（修 Q1 的原因 #2）

- [ ] state.rs 加字段：`pub list_state: gpui::ListState`、`pub follow_tail: bool`、`pub scrolled_off_count: usize`
- [ ] mod.rs `render_ai_chat_panel` 用 `gpui::list(state, render_item)` 替换 `v_flex().overflow_y_scrollbar()`
- [ ] `ListState::new(0, ListAlignment::Bottom, px(400.))` 初始化
- [ ] token 追加后 `list_state.scroll_to(ListOffset { item_ix: total, offset_in_item: px(0.) })`（如果 follow_tail）
- [ ] FIFO 200 条上限，`push_message` 包装 `Vec::push`，超限时 `remove(0)` + `list_state.splice(0..1, 0)`
- [ ] 顶部"已滚出 N 条"提示条

参考：`/home/zero/.cargo/git/checkouts/zed-a70e2ad075855582/d989c7c/crates/gpui/src/elements/list.rs`（gpui 0.2.2，与 Cargo.lock 一致）

### Phase 4: 多行输入（修 Q1 的原因 #3 部分）

- [ ] spike gpui-component 是否有 TextArea 或 multi-line Input
- [ ] 替换 `InputState` 为多行实体
- [ ] 行数 1-8 动态高度
- [ ] 面板宽度变化时光标/内容保持

### Phase 5: Markdown 扩展

- [ ] markdown.rs 的 `MdSegment` 加变体：`Heading(u8, String)`、`Blockquote(Vec<MdSegment>)`、`Table { headers, rows }`、`Link { text, url }`、`TaskItem { checked, text }`
- [ ] parse_markdown 行级识别扩展
- [ ] parse_inline 加 `[text](url)` 手写状态机（PRD 禁新增 regex 直接依赖）
- [ ] render_segments 加新变体的 GPUI element 映射
- [ ] Link onClick 触发 `open::that(url)`

### Phase 6: 响应式宽度

- [ ] ZenixApp 加字段 `ai_panel_width: f32`（默认 480，min 360，max 720）
- [ ] config.rs 加 `ai_panel_width: Option<f32>`
- [ ] 删除 `pub const AI_PANEL_WIDTH: f32 = 420.0;`（chat/mod.rs:41）
- [ ] 实现 `render_resize_handle`（resize.rs 新文件）
- [ ] `on_drag_move` clamp + `on_drop` 持久化

### Phase 7: 工具卡片 + 键盘 + 头部按钮绑定

- [ ] 头部 4 个按钮绑 on_click：
  - new chat → 创建新 session，清空 messages
  - history → toggle `data.show_history`，加载 `data.history_sessions`
  - agents → toggle `data.show_agents`
  - more → 弹菜单（导出/清空/设置）
- [ ] model selector pill → toggle `data.config_dropdown = Some(Model)` / None
- [ ] dropdown item click → `data.selected_model = ...; data.config_dropdown = None;`
- [ ] 注意：`OrchestratorEngine::new` 用 `provider.model`，**不读** `data.selected_model` — 要让 model 选择真正生效需在 start_chat_streaming 中 override（用 selected_model 替换 provider.model 构造 engine）
- [ ] AgentBlock 加 `AgentStatus` 枚举（Pending/Running/Success/Failed/Cancelled）替换字符串 status
- [ ] Esc 键：streaming 中 → stop；非 streaming → 清空输入
- [ ] Ctrl+L：清空 messages 保留 session
- [ ] ↑/↓（输入框空时）：浏览 input_history

### Phase 8: 错误卡片

- [ ] ChatMessage.role = "error" 分支渲染
- [ ] ErrorKind 枚举（Network/Auth/RateLimit/Parse/Stream/Unknown）
- [ ] render_error_card（红边框 + 图标 + 重试按钮）
- [ ] 重试按钮 → 重新发起 `start_chat_streaming(last_user_prompt)`

### Phase 9: 回归 + 性能

- [ ] 4 个主题视觉无回归
- [ ] cargo clippy 干净
- [ ] cargo test --lib ai::chat 通过（修 2 个 pre-existing ThinkingLevel 测试或标记 ignore）
- [ ] 200 条消息滚动 60fps
- [ ] 流式 token 重渲染 ≤ 16ms

## 关键文件 / 行参考

### 已修改

| 文件 | 关键行 | 内容 |
|---|---|---|
| `src/app.rs` | 676-698 | on_mouse_down 加 terminal_bounds guard |
| `src/app.rs` | 1216-1226 | PressEnter 调用 start_chat_streaming |
| `src/app.rs` | 1337-1352 | send_click_cb cx.listener 注入 render_ai_chat_panel |
| `src/app.rs` | 2623-2797 | start_chat_streaming 全函数 |
| `src/app.rs` | 2802-2839 | handle_dispatch_event |
| `src/app.rs` | 2843-2852 | stop_chat_streaming |
| `src/ai/chat/mod.rs` | 52-57 | render_ai_chat_panel 加 on_send_click 参数 |
| `src/ai/chat/input.rs` | 21-26 | render_input_bar 加 on_send_click 参数 |
| `src/ai/chat/input.rs` | 71 | Button::new("ai-chat-send").on_click(on_send_click) |
| `src/ai/chat/state.rs` | 67-72 | AiChatPanelData 加 streaming_msg_idx / last_user_prompt / abort_handle |
| `src/ai/chat/state.rs` | 95-98 | Default 初始化新字段 |

### 新增（拆分自原 chat.rs 1364 行）

- `src/ai/chat/mod.rs` — 169 行，render_ai_chat_panel + txt helper + AI_PANEL_WIDTH 常量
- `src/ai/chat/state.rs` — AiChatPanelData + Config/Workspace 类型 + 方法
- `src/ai/chat/messages.rs` — render_message + render_streaming_indicator
- `src/ai/chat/input.rs` — render_input_bar
- `src/ai/chat/header.rs` — render_header + render_dropdown + render_history_panel + render_agent_monitor + render_empty_state + suggestion_chip
- `src/ai/chat/agent_block.rs` — AgentBlock + AgentMonitorEntry + render_agent_block

### 关键 GPUI API（已验证可用，Cargo d989c7c）

| API | 路径 | 用途 |
|---|---|---|
| `gpui::list(state, render_item)` | `gpui/src/elements/list.rs:24` | 变高度虚拟化列表 |
| `gpui::ListState::new(count, alignment, overdraw)` | `list.rs:314` | 状态实体 |
| `gpui::ListAlignment::Bottom` | `list.rs:168` | chat log 自底向上 |
| `gpui::FollowMode::Tail` | `list.rs:113` | 自动跟随尾部 |
| `gpui::ListState::scroll_to(ListOffset)` | `list.rs:604` | 滚动到指定 offset |
| `gpui::ListState::splice(range, count)` | `list.rs:464` | 增量通知 list |
| `DragMoveEvent<T>` | `gpui/src/elements/div.rs:63` | 拖拽 |
| `Button::on_click(impl Fn(&ClickEvent, &mut Window, &mut App))` | `button.rs:332` | 按钮点击 |

### Engine API（已确认）

| API | 文件:行 | 用途 |
|---|---|---|
| `OrchestratorEngine::get_available_provider()` | `engine.rs:1426` | 取首个有 api_key 的 provider |
| `OrchestratorEngine::new(&provider)` | `engine.rs:217` | 构造引擎（用 provider.model） |
| `chat_and_dispatch(&self, &messages, &mut herdr, &workspace_id, on_event)` | `engine.rs:1287` | 流式 + 调度循环 |
| `DispatchStreamEvent::{Token, Dispatch, AgentOutput, AgentComplete, ToolCall, ToolResult, Compacted, Error, Done}` | `engine.rs:136` | 事件枚举 |
| `HerdrIpcClient::new(socket_path)` | `herdr.rs:728` | 构造（不连接） |
| `HerdrIpcClient::connect(&mut self)` | `herdr.rs:736` | ping + 设 timeout |
| `crate::client::herdr_socket_path()` | `client/mod.rs:60` | `~/.config/herdr/herdr-client.sock` |

## 已存的项目记忆（Engram）

- `architecture/ai-chat-streaming` — 流式架构（mpsc 桥接 + tokio::select! abort + 部分 content 保留）
- `architecture/ai-chat-module-layout` — chat/ 拆分（6 子模块、子访问父用 super::、re-export 模式）

**待补存**：
- on_mouse_down focus-stealing bug 修复
- current_session_id 永远 None bug 修复
- chat_fut 错误吞掉 bug 修复

## 失败/踩坑记录

### 前两轮重设计为什么失败

1. **Task 03**：加了 pill 配置条、彩色气泡 → 用户嫌乱
2. **Task 04**：PRD 承认"没真正理解 Zed Agent UI 设计模式"，规划的 Zed 风格底栏一行没落地
3. **根因**：两轮都把"重设计"当配色/形状换皮，**从未触碰结构性缺陷**（D1 流式根本没接通、D2 无虚拟化、D5 单行输入、D6 markdown 残缺）

### 本轮已避免的坑

- ✅ Phase 0 先验证关键 API（gpui::list 存在、FollowMode::Tail 内置）再设计
- ✅ 流式接通前先拆分文件（避免在 1364 行单文件里改）
- ✅ 每步 cargo check 验证不破坏编译
- ✅ 发现 focus bug 后立即修，没继续叠加功能

### 本轮踩过但已修复的坑

- ❌ → ✅ `git mv chat.rs chat.rs.bak` + `rm chat.rs.bak` 导致 git index 残留（已 `git rm --cached`）
- ❌ → ✅ 拆分时把 `FluentBuilder as _` import 删了但代码用 `.when()`（已加回）
- ❌ → ✅ mod.rs 缺 `ScrollableElement as _` trait import 导致 `overflow_y_scrollbar` 方法找不到（已加）
- ❌ → ✅ Button on_send_click 闭包类型不匹配（已改 cx.listener 包裹）

## 测试基线

### 单元测试

`cargo test --lib ai::chat`：**23/25 通过**

失败的 2 个：
- `ai::chat::state::tests::test_cycle_thinking_level`
- `ai::chat::state::tests::test_cycle_thinking_level_loops`

**原因**：`ThinkingLevel::default()` 返回 `Medium`（`api_provider/mod.rs:38`），但测试断言 `Low`。**Pre-existing bug，与本次拆分无关**。

### 编译

- `cargo check`：通过，66 warnings（全为 pre-existing，零新增）
- `cargo test --lib ai::chat --no-run`：通过

### 活体测试

- ⏸ 用户未反馈修复后的测试结果（focus fix + error surfacing + auto-session + push fix）
- ⏸ 用户提到"输入很卡"但未提供 tracing 日志
