# Implement — ZenixAI 结构性重做

> 配套：`prd.md`（需求/验收）、`design.md`（架构）。本文讲**执行顺序与验证**。

## 执行原则

- **每个 Phase 末必须 `cargo check` 通过**才能进入下一 Phase
- **每个 Phase 末必须 LSP diagnostics clean**（修改文件）
- **完成全部 Phase 后**跑 clippy + test + 性能验证
- 顺序敏感：D1 流式优先（无流式则 AC-1/AC-8 无法验证），虚拟化次之（影响后续所有 render）
- 一个 Phase 内允许多次试错，但跨 Phase 必须状态一致
- 文件拆分按 design.md §1.1 执行；本清单用逻辑模块名（如 `chat/messages.rs`），实际新建时按 design 拆分

## Phase 0 — Spike 验证关键假设（必做，估时 0.5d）

> 设计依赖几个不确定的 API，必须先用最小 demo 验证。任一假设失败需回到 design 调整方案。

### 0.1 Spike-1: gpui::list / uniform_list API ✅
- [x] ~~写 demo~~ — 直接读 gpui 源码（d989c7c）确认 API
- [x] `gpui/src/elements/list.rs:24` — `pub fn list(state: ListState, render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement)`
- [x] `gpui/src/elements/list.rs:54` — `pub struct ListState(Rc<RefCell<StateInner>>)`
- [x] **关键修正**：用 `list`（变高度）替代 `uniform_list`（等高）——chat 消息高度不均
- [x] **bonus**：gpui 已内置 `FollowMode::Tail` + `ListAlignment::Bottom`，省去自实现 follow_tail
- [x] ListState API：`new(count, alignment, overdraw)`、`reset(count)`、`splice(range, count)`、`scroll_to(ListOffset)`、`scroll_to_reveal_item(ix)`、`scroll_by(distance)`、`is_scrollbar_dragging()`、`viewport_bounds()`、`bounds_for_item(ix)`
- [x] `scroll_to(ListOffset)` 在 item_ix < item_count 时自动调用 `follow_state.stop_following()`

### 0.2 Spike-2: regex 是否在 Cargo.toml ✅
- [x] `regex 1.12.4` 在 Cargo.lock（间接依赖），但 **不在 Cargo.toml 直接依赖**
- [x] PRD 约束"禁止新增依赖"——markdown Link parser **手写状态机**（`[` → 找 `]` → 找 `(` → 找 `)`）

### 0.3 Spike-3: 拖拽分隔条 API ✅
- [x] `gpui/src/elements/div.rs:63` — `pub struct DragMoveEvent<T>`
- [x] `div.rs:315` — `pub fn on_drag_move<T>(...)`
- [x] `div.rs:525` — `pub fn on_drop(...)`
- [x] `div.rs:576` — `pub fn on_drag<T, W>(...)`
- [x] 所有拖拽 API 可用

### 0.4 Spike-4: engine 跨线程 cx.update ✅
- [x] `quality-guidelines.md` 已记录标准模式：`cx.spawn(move |this, mut cx| async move { cx.update(|cx| { ... }) }).detach()`
- [x] GPUI 标准 AsyncApp 跨线程模式，无需 spike demo

---

## Phase 1 — 基础拆分（无功能改动，估时 0.5d）

> 目标：把 chat.rs 拆成 chat/ 目录，所有现有行为不变。

- [ ] 1.1 新建目录 `src/ai/chat/`
- [ ] 1.2 把 chat.rs 内容按职责分配到 `mod.rs` / `state.rs` / `messages.rs` / `input.rs` / `agent_block.rs` / `error_card.rs` / `resize.rs` / `markdown_ext.rs`
- [ ] 1.3 `chat.rs` 改为 `pub mod chat;`（或保留 chat.rs 内含 mod 声明）
- [ ] 1.4 公开 API（`AiChatPanelData`、`render_ai_chat_panel`、`AgentBlock`、`AgentMonitorEntry`、`ConfigDropdownTarget`、`WorkspaceOption`、`AI_PANEL_WIDTH`）保持原签名
- [ ] 1.5 调用方 `src/app.rs`、`src/ui/sidebar.rs` 不需改动
- [ ] **验证命令**：`cargo check`（零 warning）+ `cargo run` 手动验证 chat 面板照常显示

**Phase 1 完成条件**：所有现有功能不回归，文件已拆分，cargo check 干净。

---

## Phase 2 — D1 流式接通（最高优先级，估时 1d）

> 目标：发送消息后真的能收到 token 流式追加，能看到 AI 回复。这是本轮地基。

- [ ] 2.1 在 `state.rs` 新增字段：`streaming_msg_idx: Option<usize>`、`abort_handle: Option<tokio::sync::oneshot::Sender<()>>`、`last_user_prompt: Option<String>`、`last_error: Option<AiError>`
- [ ] 2.2 实例化：`AiChatPanelData::new()` 中初始化新字段
- [ ] 2.3 在 `input.rs` 改造 PressEnter 处理（src/app.rs:1217-1253）：
    - [ ] 2.3.1 push user msg 后，再 push 一个空 assistant msg（content = ""），记下 `streaming_msg_idx = Some(messages.len()-1)`
    - [ ] 2.3.2 保存 `last_user_prompt = Some(text.clone())`
    - [ ] 2.3.3 创建 oneshot channel，tx 存入 `abort_handle`
    - [ ] 2.3.4 `cx.spawn(async move |this, cx| { ... })` 启动 spawn 任务
- [ ] 2.4 spawn 任务内：
    - [ ] 2.4.1 获取 engine handle + herdr_client + workspace_id（参照现有 AiOrchestratorEngine 初始化路径）
    - [ ] 2.4.2 用 `tokio::select!` 包裹 `engine.chat_and_dispatch(...)` 和 `abort_rx`
    - [ ] 2.4.3 闭包内 `cx.update(|cx| { this.update(cx, |this, cx| { handle_dispatch_event(...) }) })`
- [ ] 2.5 实现 `handle_dispatch_event`（design.md §2.2）：
    - [ ] `Token(text)` → `messages[streaming_msg_idx].content.push_str(&text)` + cx.notify()
    - [ ] `Done` → 置 `is_streaming=false`、`streaming_msg_idx=None`、`abort_handle=None`
    - [ ] `Error(err)` → 插入错误卡片、置 `is_streaming=false`
    - [ ] `AgentOutput/AgentComplete` → 更新对应 AgentBlock（先保留现有逻辑，下个 phase 完善）
    - [ ] 其他变体暂留空 match arm
- [ ] 2.6 实现 `stop_streaming`（design.md §2.3）：发送 abort 信号 + 状态收尾
- [ ] 2.7 把现有"停止"按钮 onClick 接到 `stop_streaming`
- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 在 chat 发送 "你好" + 观察 token 逐个出现 + 完整回复显示
    - 流式中点击停止 + 确认已生成部分保留 + 列表停止增长
    - 拔网线或停掉 provider 模拟错误 + 确认错误卡片显示

**Phase 2 完成条件**：AC-1（流式渲染可观察）通过；AC-8（错误态）核心路径通过；AC-9 不回归。

---

## Phase 3 — D2/D3 虚拟化 + FIFO 上限（估时 1d）

- [ ] 3.1 在 `state.rs` 新增字段：`list_state: gpui::ListState`、`follow_tail: bool`、`scrolled_off_count: usize`
- [ ] 3.2 实例化 `ListState::new(...)`
- [ ] 3.3 在 `messages.rs` 替换 `overflow_y_scrollbar()` 为 `uniform_list(...)`
    - [ ] 3.3.1 实现回调：`|this, range, cx| { (range.start..range.end).map(|i| render_one_message(...)).collect() }`
    - [ ] 3.3.2 链 `.track_scroll().with_state(list_state).flex_grow().min_h_0()`
- [ ] 3.4 实现 `update_follow_tail`（design.md §3.2）：
    - 渲染前根据 `list_state.scroll_position()` 更新 `follow_tail`
    - 流式 token 追加时若 `follow_tail=true` 则 `list_state.scroll_to(ListScrollTo::Bottom)`
- [ ] 3.5 实现 FIFO 上限（design.md §3.3）：`push_message` 函数包装 `Vec::push`，超 200 时 `remove(0)` 且 `scrolled_off_count += 1`
- [ ] 3.6 在 list 顶部条件渲染"已滚出 N 条更早消息"提示条（仅当 `scrolled_off_count > 0`）
- [ ] 3.7 替换所有 `data.messages.push(...)` 调用点为 `push_message(this, msg)`（含 user msg、assistant msg、error msg）
- [ ] 3.8 把空态判断从 `data.messages.is_empty()` 改为 `data.messages.is_empty() && scrolled_off_count == 0`（避免滚出全部后回到空态）
- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 发 50 轮对话（每轮 ~200 词）+ 观察滚动 60fps 不掉帧
    - 发到第 201 条 + 确认第 1 条被滚出 + 顶部出现"已滚出 1 条"提示
    - 流式中向上滚动 + 确认列表不强制拉回底部；滚到底部恢复跟随
    - 用 `cargo run --release` + massif/heaptrack 观察内存峰值不持续增长

**Phase 3 完成条件**：AC-2（长会话不卡）通过；AC-9 不回归。

---

## Phase 4 — D5 多行输入（估时 0.5d）

- [ ] 4.1 Spike：检查 gpui-component 是否有 `TextArea` 或 `Input::multi_line`
- [ ] 4.2 优先用现成组件；否则在现有 Input 之上做 auto-resize（design.md §4.2）
- [ ] 4.3 替换 input.rs 中单行 Input 为多行实现
- [ ] 4.4 高度根据内容行数动态计算（1-8 行），超过 8 行内部滚动
- [ ] 4.5 验证 Enter 发送 / Shift+Enter 换行保留
- [ ] 4.6 验证面板宽度变化时内容/光标不丢失
- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 在输入框写 5 行文本 + 验证发送后 engine 收到完整带换行的 prompt
    - 缩放面板宽度 + 验证光标位置不跳

**Phase 4 完成条件**：AC-3 通过。

---

## Phase 5 — D6 Markdown 扩展（估时 1d）

- [ ] 5.1 在 `markdown_ext.rs`（或直接 `markdown.rs`）扩展 `MdSegment` 枚举新增 5 变体
- [ ] 5.2 扩展 `parse_markdown` 主循环：
    - [ ] 5.2.1 Heading：识别 `^#{1,6}\s` 前缀，提取级别 + 文本
    - [ ] 5.2.2 Blockquote：识别 `^>\s`，多行连续收集
    - [ ] 5.2.3 TaskItem：识别 `^[-*] \[[ xX]\]\s`
    - [ ] 5.2.4 Table：识别 `^\|.*\|$` 连续行 + `^\|[-: ]+\|$` 分隔行
- [ ] 5.3 扩展 `parse_inline`：
    - [ ] 5.3.1 Link：识别 `[text](url)`（用 regex 或手写状态机，按 Spike-2 结果）
- [ ] 5.4 扩展 `render_segments`（design.md §5.4）：
    - [ ] 5.4.1 Heading 渲染：按 level 映射字号 + 粗体
    - [ ] 5.4.2 Blockquote：左边框 + muted 颜色 + 缩进
    - [ ] 5.4.3 Table：v_flex 嵌套 h_flex（或借 gpui-component 的 Table 组件）
    - [ ] 5.4.4 Link：accent 颜色 + 下划线 + onClick `open::that(url)`
    - [ ] 5.4.5 TaskItem：checkbox 图标 + 文本
- [ ] 5.5 测试用例：构造一段 markdown 含所有新元素，喂给 `parse_markdown` + `render_segments`，肉眼验证
- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 让 AI 生成一段含 `## / > / 表格 / [link](url) / - [ ]` 的回复 + 验证全部正确渲染
    - 点击链接 + 验证在系统浏览器打开

**Phase 5 完成条件**：AC-4 通过；AC-9 不回归（原 6 个 segment 渲染不变）。

---

## Phase 6 — D4 响应式宽度 + 持久化（估时 0.5d）

- [ ] 6.1 在 ZenixApp 加字段 `ai_panel_width: f32`，从 config 初始化（缺省 480.0）
- [ ] 6.2 在 `config.rs` ConfigFile 加字段 `ai_panel_width: Option<f32>`
- [ ] 6.3 删除 `src/ai/chat.rs:35` 的 `pub const AI_PANEL_WIDTH: f32 = 420.0;`
- [ ] 6.4 修改所有引用点（`AI_PANEL_WIDTH` → 从 ZenixApp 读 `this.ai_panel_width`）：
    - [ ] src/app.rs:1009 `Panel::AiChat => sidebar::AI_PANEL_WIDTH` → 读字段
    - [ ] src/ai/chat/mod.rs render 中 `div().w(px(AI_PANEL_WIDTH))` → `div().w(px(this.ai_panel_width))`
- [ ] 6.5 在 `resize.rs` 实现 `render_resize_handle`（design.md §6.2）
    - [ ] 4px 宽拖拽条，hover 显示
    - [ ] on_drag_move 时 clamp 到 [360, 720]
    - [ ] on_drop 时持久化到 config
- [ ] 6.6 在 chat 面板根 div 左侧挂上 resize handle
- [ ] 6.7 实现 `persist_ai_panel_width`（design.md §6.3）
- [ ] 6.8 实现终端宽度压缩提示（窗口宽 - sidebar - panel < 600 时显示黄条）
- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 拖拽面板左边沿 + 验证宽度变化 + clamp 生效
    - 重启 zenix + 验证宽度保持
    - 缩窗口 + 验证终端 < 400 时黄条显示

**Phase 6 完成条件**：AC-5 通过。

---

## Phase 7 — 工具卡片化 + 键盘快捷键（估时 1d）

### 7.1 工具调用卡片（修 D6 + AgentBlock 完善）
- [ ] 7.1.1 在 `agent_block.rs` 定义 `AgentStatus` 枚举（design.md §1.3）
- [ ] 7.1.2 `AgentBlock.status: String` 改为 `AgentStatus`
- [ ] 7.1.3 更新所有创建 AgentBlock 的调用点（如 `from_herdr`、流式事件处理）
- [ ] 7.1.4 重写 `render_agent_block`：状态图标 + 耗时 + 摘要 + 展开折叠
- [ ] 7.1.5 Failed 状态加重试按钮（若 tool 支持）
- [ ] 7.1.6 接通：`DispatchStreamEvent::AgentOutput` → 找到/创建对应 AgentBlock，追加到 `output_history`
- [ ] 7.1.7 接通：`DispatchStreamEvent::AgentComplete(AgentResult)` → 转换为 Success/Failed

### 7.2 键盘快捷键（修 D9）
- [ ] 7.2.1 在 chat 面板根 div 加 `on_key_down(handle_chat_key_down)`
- [ ] 7.2.2 Esc：流式中 → `stop_streaming`；非流式 → 清空输入
- [ ] 7.2.3 Ctrl+L：清空 `data.messages`（保留 session_id）
- [ ] 7.2.4 ↑/↓（输入框空时）：浏览 `input_history`，实现 history_prev/history_next
- [ ] 7.2.5 在 `handle_send` 中把提交的 prompt push 到 `input_history`（max 50）

- [ ] **验证命令**：
    - `cargo check` 干净
    - `cargo run` + 触发 tool call + 验证 Pending → Running → Success/Failed 卡片状态正确
    - 流式中按 Esc + 验证中止
    - 输入框空时按 ↑ + 验证历史 prompt 出现
    - 按 Ctrl+L + 验证消息清空但 session 保留

**Phase 7 完成条件**：AC-6 + AC-7 通过。

---

## Phase 8 — 错误卡片完善（估时 0.5d）

- [ ] 8.1 在 `error_card.rs` 定义 `ErrorKind` 枚举
- [ ] 8.2 实现 `ErrorKind::from_ai_error(&AiError)`（分类 Network/Auth/RateLimit/Parse/Stream/Unknown）
- [ ] 8.3 ChatMessage `role = "error"` 的内容序列化为 `kind::message` 格式（或加 `error_kind` 字段）
- [ ] 8.4 实现 `render_error_card`（design.md §8）
- [ ] 8.5 在 `render_message` 中按 role 分支：`"user"`/`"assistant"` 用原渲染，`"error"` 用 error_card
- [ ] 8.6 错误卡片重试按钮：从 `last_user_prompt` 取出，重新调用 Phase 2 的 send 路径
- [ ] **验证命令**：
    - `cargo run` + 停掉 provider 模拟错误 + 验证错误卡片显示
    - 启动 provider + 点击重试 + 验证重新发起

**Phase 8 完成条件**：AC-8 全通过。

---

## Phase 9 — 回归 + 性能验证（估时 0.5d）

### 9.1 视觉回归
- [ ] 9.1.1 切换 gruvbox/solarized/tokyonight/matrix 主题，每个主题发一条对话，肉眼比对气泡颜色/间距无变化
- [ ] 9.1.2 history 面板打开/关闭正常
- [ ] 9.1.3 agent monitor 面板打开/关闭正常
- [ ] 9.1.4 settings 面板正常

### 9.2 功能回归
- [ ] 9.2.1 Enter 发送 / Shift+Enter 换行保留
- [ ] 9.2.2 engine 接口未改：`grep "pub async fn chat_stream" src/ai/engine.rs` 签名一致
- [ ] 9.2.3 herdr IPC 测试（若有）通过

### 9.3 质量门
- [ ] 9.3.1 `cargo fmt --check`
- [ ] 9.3.2 `cargo clippy -- -D warnings`（无 warning）
- [ ] 9.3.3 `cargo test --lib`（全部通过）
- [ ] 9.3.4 `cargo check` 干净

### 9.4 性能验证
- [ ] 9.4.1 流式 token 重渲染 ≤ 16ms（cargo flamegraph 或简单 timestamp 测量）
- [ ] 9.4.2 200 条消息滚动 60fps
- [ ] 9.4.3 内存稳定（massif/heaptrack 峰值不持续增长）

**Phase 9 完成条件**：AC-9 全通过；所有验收 AC-1~AC-9 通过。

---

## 总验收清单（提交前自检）

- [ ] AC-1 流式渲染可观察
- [ ] AC-2 长会话不卡（50 轮 + 200 上限）
- [ ] AC-3 多行输入可用
- [ ] AC-4 Markdown 完整渲染（5 个新元素）
- [ ] AC-5 响应式宽度（拖拽 + 持久化）
- [ ] AC-6 工具调用卡片（5 状态）
- [ ] AC-7 键盘快捷键（Esc/Ctrl+L/↑↓）
- [ ] AC-8 错误态（行内卡片 + 重试）
- [ ] AC-9 不回归（主题/Enter/herdr 接口/clippy/test）
- [ ] cargo check / clippy / test / fmt 全清

## 时间估算

| Phase | 内容 | 估时 |
|---|---|---|
| 0 | Spike 验证 | 0.5d |
| 1 | 基础拆分 | 0.5d |
| 2 | D1 流式接通 | 1d |
| 3 | D2/D3 虚拟化 | 1d |
| 4 | D5 多行输入 | 0.5d |
| 5 | D6 Markdown | 1d |
| 6 | D4 响应式宽度 | 0.5d |
| 7 | 工具卡片 + 快捷键 | 1d |
| 8 | 错误卡片 | 0.5d |
| 9 | 回归 + 性能 | 0.5d |
| **合计** | | **7d** |

## 回滚点

- Phase 0 spike 失败 → 回到 design.md §9 调整方案
- 任一 Phase 末 cargo check 不过 → 不进入下一 Phase
- Phase 2-8 任一 Phase 完成后可作为"半成品但可用"状态提交（除 Phase 2 外其他都是增量）
- 整体失败 → `git checkout main` 回到上一个稳定状态（本轮所有改动在 feature branch）
