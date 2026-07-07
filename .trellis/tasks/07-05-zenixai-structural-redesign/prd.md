# ZenixAI 结构性重做：流式渲染 + 虚拟化 + 多行输入

## Goal

第三轮 ZenixAI 重设计：废弃换皮路线，改打结构地基。范围 A：流式 token 渲染、虚拟化消息列表、多行 TextArea、Markdown 补全、响应式面板宽度、工具调用卡片化。Supersedes 03-zenixai-ui-redesign 和 04-zenixai-zed-style-redesign。

## Background — 为什么前两轮失败

### 当前实现的具体缺陷（chat.rs 1364 行）

| # | 缺陷 | 证据 |
|---|---|---|
| D1 | **流式完全没接通**。`src/app.rs:1249-1253` PressEnter 处理只设 `is_streaming=true`，**没有 cx.spawn()，没有 chat_stream/chat_and_dispatch 调用**——engine 根本没被调用 | 致命：用户发消息后只看到绿点永远转，不会收到任何回复 |
| D2 | **无虚拟化**。`messages: Vec<ChatMessage>` 全量渲染（chat.rs:93, 291-296 用 `overflow_y_scrollbar()`） | 长会话性能崩，每 token 重渲染整个历史 |
| D3 | **无滚动上限**。Vec 无截断，history_sessions 也不裁剪（chat.rs:115） | 内存增长无界 |
| D4 | **固定 420px 宽**。`AI_PANEL_WIDTH = 420.0`（chat.rs:35, 259） | 小屏挤死终端，大屏浪费空间 |
| D5 | **单行输入框**。用 `InputState` 单行实体（chat.rs:555-578） | 无法编写多行 prompt |
| D6 | **Markdown 残缺**。markdown.rs 手写 parser，MdSegment 只有 Plain/Bold/Italic/InlineCode/CodeBlock/ListItem 六种——标题/引用/表格/链接/任务列表**根本没 tokenize**（markdown.rs:19-27, 37-38） | AI 输出的常见格式无法正确显示 |
| D7 | **无错误态渲染**。chat 面板完全没有 error UX | 网络/鉴权错误用户无感知 |
| D8 | **消息气泡 max_w 320/340 硬编码**（chat.rs:1002, 1022） | 宽屏左右大块留白 |
| D9 | **缺键盘快捷键**。只有 Enter/Shift+Enter，无 Esc 取消、无历史导航 | 效率低 |

### 前两轮复盘

- **Task 03（"Modern AI Agent Minimalist"）**：加了 pill 配置条、彩色气泡 → 用户嫌乱
- **Task 04（"Zed-style"）**：PRD 原文承认 *"没有真正理解 Zed Agent 的 UI 设计模式，导致界面效果没有实质性改善"*。规划的 Zed 风格底栏（"+" 图标 / "ZenixAI Agent ▾" / 底部 model 面板）**一行代码都没落地**

**根因**：两轮都把"重设计"当成配色和形状的换皮，从未触碰结构性缺陷——D1 流式根本没接、D2 虚拟化、D5 多行输入、D6 markdown 完整性。

## Scope

### In Scope（本轮交付）

| 项 | 描述 | 解决缺陷 |
|---|---|---|
| **流式 token 渲染** | PressEnter 触发 `cx.spawn()` → `chat_and_dispatch` → 处理 `DispatchStreamEvent::Token` 增量追加到当前生成消息 | D1 |
| **虚拟化消息列表** | 用 `gpui::uniform_list` 替换 `overflow_y_scrollbar()`，配合 follow_tail 语义 | D2, D3 |
| **多行 TextArea** | 替换 Input 为支持 auto-resize 的多行实体（1-8 行） | D5 |
| **Markdown 补全** | 扩展现有手写 parser 支持 `#`-`######` / `>` / 表格 / `[]()` / `- [ ]`-`[x]` | D6 |
| **响应式面板宽度** | 废弃 420 常量，默认 480，min 360 max 720，可拖拽 + 持久化到 config `ai_panel_width` | D4 |
| **工具调用卡片化** | AgentBlock 加生命周期状态枚举（Pending/Running/Success/Failed/Cancelled） | （已有半成品，补全） |
| **Stop 真·abort** | 中止 engine 流，保留部分响应 | D1 子项 |
| **键盘快捷键** | Esc 取消流式 / Ctrl+L 清空 / ↑↓ 输入历史导航 | D9 |
| **错误态 UX** | 错误以行内卡片插入消息流，可重试 | D7 |

### Out of Scope（明确不做）

- ❌ 视觉重做（配色、气泡形状、间距 polish）—— 留给后续 `zenixai-visual-polish` 任务
- ❌ engine / session / herdr IPC 后端重构 —— **仅消费**现有 StreamEvent/DispatchStreamEvent，不改协议
- ❌ 新功能（附件上传、@提及、artifact 面板、历史搜索）
- ❌ i18n 新增 key（除新引入的错误/快捷键提示必需）
- ❌ terminal/element.rs 改动
- ❌ 引入新依赖（pulldown-cmark 等）—— 手扩现有 parser

## Requirements

### 功能性需求

#### F1. 流式 token 渲染（修 D1）

- **F1.1** PressEnter 在 `data.messages.push(user_msg)` 后必须 `cx.spawn()` 一个异步任务，调用 engine 的 `chat_and_dispatch`（或退化为 `chat_stream`），传入闭包处理 `DispatchStreamEvent`
- **F1.2** 闭包内对 `DispatchStreamEvent::Token(text)` 必须通过 `entity.update(cx, |this, cx| { ... })` **增量追加**到一条新生成的 assistant 消息的 `content` 字段（push_str），并 `cx.notify()`
- **F1.3** `DispatchStreamEvent::Done` 关闭该条 assistant 消息，置 `is_streaming = false`
- **F1.4** `DispatchStreamEvent::Error` 在消息流中插入错误卡片（见 F8），置 `is_streaming = false`，保留已生成部分
- **F1.5** 流式过程中消息列表必须自动滚动跟随尾部，但用户向上手动滚动时**暂停跟随**；点击"回到底部"或滚到底部恢复跟随
- **F1.6** 流式中"停止"按钮必须真正中止 engine 流（通过 `CancellationToken` / `tokio::sync::oneshot` 或 abort handle），并保留已生成的部分内容作为一条完整的助手消息

#### F2. 虚拟化消息列表（修 D2, D3）

- **F2.1** 使用 `gpui::uniform_list(vertical, "chat_messages", msg_count, cx, |i, range, this, cx| render_one(i))` 替换 `overflow_y_scrollbar()` 容器；仅渲染可见项 + overscan
- **F2.2** 维护一个 `ListState` 实体跟踪滚动位置；实现 `follow_tail`：当滚动条在底部 32px 内时 `follow_tail=true`，新消息追加自动滚到底；用户上滚则 `follow_tail=false`
- **F2.3** 滚动上限 200 条；超出按 FIFO 滚出（删除最旧），删除时给出"已滚动出 N 条更早消息"的提示（独立子元素，不计入消息数）
- **F2.4** 已完成的消息（非流式中）在后续 token 流入时**不重渲染**：通过将 uniform_list 的 item count 分为"committed N 条 + 1 条 streaming tail"，token 只更新最后一条

#### F3. 多行输入（修 D5）

- **F3.1** 替换 `InputState` 单行实体为支持多行的实体（优先用 gpui-component 的 `TextArea`/`TextInput` 多行模式，若不可用则在现有 Input 之上实现 auto-resize）
- **F3.2** 初始 1 行高，最高 8 行，超过出现内部滚动；高度根据内容行数动态计算
- **F3.3** Enter 发送 / Shift+Enter 换行（保留现有行为）
- **F3.4** 输入框内容/光标/选择状态在面板宽度变化时保持

#### F4. Markdown 补全（修 D6）

- **F4.1** 在现有 `MdSegment` enum 上新增变体：`Heading(u8, String)`（级别 1-6）、`Blockquote(String)`、`Table { headers, rows }`、`Link { text, url }`、`TaskItem { checked, text }`
- **F4.2** 扩展 `parse_markdown` 行级识别：`^#{1,6}\s` → Heading；`^>\s` → Blockquote；`^\|.*\|$` 连续行 → Table；`^- \[[ xX]\] ` → TaskItem
- **F4.3** 扩展 `parse_inline` 识别 `\[([^\]]+)\]\(([^)]+)\)` → Link
- **F4.4** 扩展 `render_segments`：Heading 按 u8 级别映射字号（h1=22px→h6=14px）+ 粗体；Blockquote 左边框 + muted；Table 渲染为 gpui-component Table 或简单 flex 网格；Link 渲染为可点击文本（onClick 触发 `open::that(url)`）；TaskItem 渲染为带 checkbox 图标的列表项
- **F4.5** 代码块语法高亮保持现状（不扩展语言集合）

#### F5. 响应式面板宽度（修 D4）

- **F5.1** 删除 `AI_PANEL_WIDTH: f32 = 420.0` 常量；改为 `ZenixApp` 字段 `ai_panel_width: f32`，从 config 读取，默认 480.0
- **F5.2** 渲染时 `div().w(px(this.ai_panel_width))`
- **F5.3** 在 chat 面板左边缘渲染 4px 拖拽分隔条（`on_drag`/`on_mouse_move`），动态修改 `ai_panel_width`，clamp 到 [360, 720]
- **F5.4** 拖拽结束时（`on_drop`/`on_mouse_up`）持久化到 `~/.config/zenix/config.json` 新字段 `ai_panel_width`（config.rs 加字段 `Option<f32>`）
- **F5.5** 窗口宽度 - 面板宽度 < 600px 时（终端被挤到 < 400px）在面板顶部显示细黄条提示"终端宽度被压缩"，但不强制阻止

#### F6. 工具调用卡片

- **F6.1** AgentBlock 用枚举替换字符串 status：
  ```rust
  pub enum AgentStatus { Pending, Running, Success { dur_ms: u64 }, Failed { err: String }, Cancelled }
  ```
- **F6.2** 卡片始终显示：agent 名 + 状态图标（◐/↻/✓/✕/⌀）+ 耗时（Running 时实时计时，完成时定格）
- **F6.3** 默认折叠（仅摘要），点击展开查看完整 output_history
- **F6.4** Failed 状态显示错误摘要 + 重试按钮（仅当 tool 支持）
- **F6.5** 接通：`DispatchStreamEvent::AgentOutput` 追加到对应 AgentBlock 的 output；`AgentComplete(AgentResult)` 转换为 Success/Failed 状态

#### F7. 键盘快捷键（修 D9）

- **F7.1** Esc：流式进行中 → 中止（等同点 Stop）；非流式 → 清空输入框
- **F7.2** Ctrl+L：清空当前会话消息（保留 session_id，仅清空 `data.messages`，不删 session）
- **F7.3** ↑/↓（输入框内容为空时）：浏览输入历史（按 Enter 提交过的 prompt，存于 `data.input_history: Vec<String>`，最多 50 条）

#### F8. 错误态 UX（修 D7）

- **F8.1** 新增消息类型 `ChatMessage.role = "error"`，在 `render_message` 中渲染为行内卡片（红色边框 + 图标 + 错误类型 + 简短信息 + 可选重试按钮）
- **F8.2** engine 报错（网络/鉴权/限流/解析失败）时，错误以行内卡片插入消息流，不弹窗
- **F8.3** 流式中途失败时，已生成的部分内容**保留**，错误卡片附在其后
- **F8.4** 重试按钮重新触发相同的 user prompt（从 `input_history.last()` 取）

### 非功能性需求

#### N1. 性能

- **N1.1** 单次 token 流入的重渲染预算 ≤ 16ms（60fps 流畅）
- **N1.2** 200 条消息的列表滚动帧率不低于 60fps
- **N1.3** 内存：滚动出的消息从 Vec 移除，不持有引用

#### N2. 兼容性

- **N2.1** 不破坏现有 config 文件（新字段 `ai_panel_width` 可选，缺省用默认值）
- **N2.2** 不改 engine/session/herdr 的对外接口（仅消费 StreamEvent/DispatchStreamEvent）
- **N2.3** 现有主题（gruvbox/solarized/tokyonight/matrix）下视觉一致

#### N3. 可维护性

- **N3.1** chat.rs 当前 1364 行单文件过大。本轮按职责拆分为：`chat/mod.rs`（panel 入口）、`chat/messages.rs`（消息渲染 + ListState）、`chat/input.rs`（多行输入 + 快捷键）、`chat/agent_block.rs`（工具卡片）、`chat/markdown_ext.rs`（扩展 segment）。**强制拆分**——单文件继续膨胀是技术债
- **N3.2** 不引入新的外部依赖（除 Cargo.toml 已有）

## Constraints / Anti-patterns（教训硬约束）

### 必须遵守

- ✅ **结构优先于样式**：本轮交付物**禁止**改动现有气泡颜色/形状/间距/字体——这些留给后续 polish 任务
- ✅ **同框架优先**：流式、虚拟化方案必须先用 gpui/gpui-component 已有能力
- ✅ **不破坏后端**：engine/session/herdr IPC 协议冻结，只消费
- ✅ **每步可验证**：每个 acceptance criterion 必须有可观察的成功信号

### 禁止（前两轮踩过的坑）

- ❌ 禁止新增"配置 pill 栏"等装饰性 UI（Task 03 失败点）
- ❌ 禁止承诺"Zed 风格"但不落地（Task 04 失败点）——本轮不提"Zed 风格"，只抄 Zed 的**结构性机制**（uniform_list follow_tail、状态实体、可调宽度）
- ❌ 禁止把"流式绿点"当流式 UX 的全部
- ❌ 禁止在 chat 面板里 hardcode 任何像素值作为最终样式（min/max 边界可硬编码）
- ❌ 禁止为 markdown 新增 pulldown-cmark 等依赖——手扩现有 parser

## Acceptance Criteria

### AC-1 流式渲染可观察（修 D1）

- [ ] 发送一条 prompt，肉眼可见 token 逐个出现
- [ ] 流式中向上滚动，列表不强制拉回底部；点击"回到底部"或滚到底部，恢复跟随
- [ ] 流式中点击"停止"，已生成部分保留为一条完整消息，列表停止增长
- [ ] 流式期间每 token 重渲染耗时 ≤ 16ms（gpui profiler 或 cargo flamegraph 验证）

### AC-2 长会话不卡（修 D2, D3）

- [ ] 发送 50 轮对话（每轮 ~200 词），列表滚动 60fps 不掉帧
- [ ] 第 201 条消息插入时，第 1 条被滚出，UI 出现"已滚出 1 条更早消息"提示
- [ ] 内存占用稳定（用 massif/heaptrack 观察峰值不持续增长）

### AC-3 多行输入可用（修 D5）

- [ ] Shift+Enter 能换行，输入框自动增高直到 8 行后内部滚动
- [ ] 多行 prompt 发送后，engine 收到的 prompt 包含换行
- [ ] 面板宽度变化时输入框内容/光标不丢失

### AC-4 Markdown 完整渲染（修 D6）

- [ ] AI 返回带 `## 标题` / `> 引用` / GFM 表格 / `[文字](url)` / `- [ ] 任务` 的内容时全部正确渲染
- [ ] 点击链接在系统默认浏览器打开
- [ ] 任务列表 checkbox 可视化区分已完成/未完成

### AC-5 响应式宽度（修 D4）

- [ ] 默认面板宽度 480px
- [ ] 拖拽分隔条可在 360-720px 调整
- [ ] 重启 zenix 后宽度保持上次值（config 持久化生效）
- [ ] 窗口缩小使终端 < 400px 时显示黄条提示

### AC-6 工具调用卡片

- [ ] tool call 触发时出现 Pending 卡片
- [ ] 完成时卡片显示 ✓ + 耗时 + 摘要，可点击展开查看完整 output
- [ ] 失败时卡片显示 ✕ + 错误摘要
- [ ] AgentOutput 事件实时追加到对应卡片

### AC-7 键盘快捷键（修 D9）

- [ ] Esc：流式中 → 中止；非流式 → 清空输入
- [ ] Ctrl+L：清空当前会话消息
- [ ] ↑/↓（输入框为空时）：浏览历史 prompt

### AC-8 错误态（修 D7）

- [ ] engine 报错时错误以行内卡片插入消息流，不弹窗
- [ ] 流式中途失败，已生成内容保留，错误卡片附在其后
- [ ] 错误卡片重试按钮重新触发相同 prompt

### AC-9 不回归

- [ ] 现有 4 个主题下视觉无回归
- [ ] 现有 Enter 发送 / Shift+Enter 换行行为保留
- [ ] 现有 history 面板、agent monitor 面板功能保留
- [ ] engine / herdr IPC 接口未变（grep `pub fn chat_stream` / `pub async fn chat_and_dispatch` 签名未改）
- [ ] cargo check / cargo clippy / cargo test --lib 通过

## Notes

- 视觉 polish（气泡样式、间距、配色）明确留给后续任务 `zenixai-visual-polish`
- 后续附件/`@`提及/artifact 面板作为本轮之后的新任务
- 本轮 PRD 在 `03-zenixai-ui-redesign/prd.md`、`04-zenixai-zed-style-redesign/prd.md` 之上提炼，已剔除两轮中失败的视觉路线
