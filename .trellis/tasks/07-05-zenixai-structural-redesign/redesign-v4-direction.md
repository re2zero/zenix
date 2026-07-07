# ZenixAI Redesign v4 — 调研综合 + 设计方向

> 配套：`status.md`（前 5 phase 已完成代码资产）/ `prd.md` v1（旧 PRD，scope 错误，待废弃）
> 本文档取代旧 `design.md` 作为新方向的起点。等方向确认后写正式 PRD。

## 1. 背景

**用户痛点**（3 项全选）：视觉丑不像现代 AI 客户端 / 功能缺失严重 / 整体方向错了推倒重来。

**实测情况**：流式 token 能看到（D1 工作）。所以代码资产值得保留，"推倒重来"指**视觉设计 + 信息架构 + scope 划分**推倒，不是代码全删。

**前 3 轮失败根因**：
- 03 / 04：换皮不修结构
- 07-05（当前）：修了结构但 PRD 明确写"❌ 视觉重做留给后续 task"——这是核心错误判断，用户要的是一次性做对

## 2. 现状盘点（保留的代码资产）

| 资产 | 位置 | 状态 |
|---|---|---|
| chat/ 6 子模块拆分 | `src/ai/chat/{mod,state,messages,input,header,agent_block}.rs` 共 1942 行 | ✅ 已拆，保留 |
| D1 流式接通 + 4 bug fix | `src/app.rs:2623-2852` | ✅ 工作（用户实测） |
| D2/D3 虚拟化 | `state.rs:133-134` 用 `ListState::new(0, ListAlignment::Bottom, px(400.))` + `set_follow_mode(FollowMode::Tail)` | ✅ 已用 gpui 原生 |
| D10-D12 配置链路 | `start_chat_streaming` provider/model override；`AiChatAction` enum 统一 dispatch | ✅ 完成 |

**现状实际渲染（关键缺陷）**：

| 区域 | 现状 | 问题 |
|---|---|---|
| 整体 layout | header → messages → input → (overlay history/monitor 浮层) | history/monitor 浮层覆盖在 input 下方，视觉割裂 |
| header | "ZenixAI" title + provider pill + 4 按钮（new/history/agents/more） | More 按钮无 on_click（装饰）；provider pill 在头部占用宽度 |
| user 消息气泡 | `accent` 背景右对齐，max_w **320px 硬编码**，p(10) px(14) | 宽屏左右大块留白 |
| **assistant 消息** | **无背景色！只是裸文本**，max_w **340px 硬编码**，p(4) | 视觉极弱，与 user 反差过大；没有 avatar/logo 标识 |
| 时间戳 | 永远显示，9px muted_foreground | 视觉杂乱，每条消息下方一行小字 |
| streaming indicator | 6x6 圆点 + "streaming" 文本，**在 messages 上方独立区域** | 不在消息流内，token 出现时视觉反馈断裂 |
| agent_block | 渲染在 messages **上方独立区域**，不在消息流内 | 工具调用与对话内容割裂，看不出时序关系 |
| input bar | 单行 `InputState`（虽然 `multi_line(true)` 一行就能开） | 不能多行 |
| model selector | 绝对定位浮层（bottom 52 right 12 w 220） | 浮层定位脆弱 |
| empty state | 居中 bot icon + title + placeholder + 3 suggestion chip | OK 但 chip 文本过于通用 |
| markdown | 6 segment（Plain/Bold/Italic/InlineCode/CodeBlock/ListItem） | 缺 heading/blockquote/table/link/task |
| 错误显示 | `⚠️` 文本追加到 streaming msg | 不是卡片，无法重试 |
| 宽度 | 固定 420px（`AI_PANEL_WIDTH`） | 不可调 |

## 3. 参考产品关键 takeaway

| 产品 | 借鉴点 | 不借鉴 |
|---|---|---|
| **Claude Desktop** | warm canvas + 单一 coral accent；typography 主导（serif 标题 + sans 正文）；32px card padding；"calm, authoritative"；distraction-free（无装饰按钮） | serif 字体（与终端应用调性不符） |
| **Cursor** | streaming cursor 在 trailing edge（CSS 动画）；tool call lifecycle 卡片（pending/running/success/error/cancelled）；user/AI 强对比 | 浮动的 inline chat |
| **Zed Assistant**（同 GPUI） | Dock & Panel 系统（`Panel` trait: persistent_name/position/default_size/min_size）；`list_state.set_follow_mode(FollowMode::Tail)` 自动跟随 | 完整 dock 系统（zenix 已有 sidebar） |
| **Cline / Roo Code** | **批处理连续 tool calls 为 grouped UI blocks**（关键！减少视觉混乱）；"Save All"/"Deny All" 按钮；Plan/Act 模式 | webview React 实现 |
| **ChatGPT** | TTFT 间隙 typing indicator（三个动画点）；可折叠 reasoning blocks；auto-scroll + scroll-to-bottom 按钮；Focus + History 架构 | message branching（重新生成） |
| **Continue.dev** | 代码段附 action buttons（Apply/Insert/Copy） | — |

**共同模式**：
1. User 右对齐 / AI 左对齐（或背景色对比区分）
2. Streaming cursor（CSS 动画，trailing edge）+ TTFT 间隙 typing indicator
3. Tool call cards with lifecycle states
4. 多行输入 + Enter/Shift+Enter
5. Approval UX（agent 场景）
6. History sidebar

## 4. Gap 分析（现状 vs 参考产品）

| 维度 | 现状 | Claude Desktop | Cursor | Zed | Gap |
|---|---|---|---|---|---|
| assistant 视觉 | 裸文本 | cream cards 有背景 | 浅灰背景 | 主题 tokens | **缺背景/边框，视觉弱** |
| 消息流统一 | agent_blocks 在外 | — | inline | inline | **agent_blocks 应进消息流** |
| streaming 反馈 | 上方独立 indicator | — | trailing cursor | FollowMode::Tail（已有） | **应嵌入消息流 tail** |
| 多行输入 | 单行 | 多行 | 多行 | 多行 | **`multi_line(true)` 一行改** |
| 宽度 | 固定 420 | elastic | elastic | dock elastic | **用 gpui-component resizable** |
| markdown | 6 segment | 完整 | 完整 | 完整 | **手扩 parser 或用 highlighter** |
| 错误 | ⚠️ 文本 | — | inline error | — | **alert 卡片 + 重试** |
| 时间戳杂乱 | 永远显示 | minimal | hover 显示 | — | **hover/fade 显示** |
| 头部装饰 | More 无 on_click | 极简 | 极简 | 极简 | **删 More 或绑 menu** |

## 5. 设计方向推荐

**核心理念**：从"功能堆砌"转向"对话为王"。参考 Claude Desktop 的 distraction-free 哲学 + Zed 的 GPUI 原生模式。

### 5.1 视觉语言（Claude-inspired 但适配终端应用）

- **不抄 Claude 的 serif + warm canvas**（与终端应用调性不符）
- **抄 Claude 的"calm, distraction-free, typography-led"哲学**
- 用 zenix 现有主题系统（gruvbox/solarized/tokyonight/matrix）的 token
- assistant 消息**加 muted 背景气泡**（不是 accent，避免与 user 冲突）
- user 消息保留 accent 背景，但 **max_w 改为响应式**（90% 宽度，max 480px）
- 删除永远显示的时间戳，改为 **hover 显示**（fade in）
- header 极简化：只保留 title + 1-2 个核心按钮（new chat + settings），其他进 menu

### 5.2 信息架构（统一消息流）

- **agent_blocks 进入消息流**（作为特殊 role="tool" 的 ChatMessage），不再独立区域
- **streaming indicator 嵌入消息流 tail**（cursor 在最后一条 assistant msg 的 trailing edge），不再独立区域
- **error 作为 role="error" 的 ChatMessage 进入消息流**，不再 ⚠️ 文本追加
- history / agent monitor 改为**抽屉式覆盖**（从右侧滑入，半透明背景），不再浮层
- model selector 从 input bar 移到 **header 区域**（输入区只留 input + send）

### 5.3 交互

- **多行输入**：`InputState::multi_line(true).auto_grow(true)` 一行改（gpui-component 现成）
- **streaming cursor**：在最后一条 assistant msg 内容末尾加 pulsing block（gpui 动画）
- **TTFT 间隙 typing indicator**：用户发消息后到第一个 token 之间显示三个动画点
- **tool call lifecycle 卡片**：用 gpui-component `accordion` 折叠展开；连续 tool calls 批处理（参考 Roo Code）
- **markdown 表格**：用 gpui-component `table`
- **代码块语法高亮**：用 gpui-component `highlighter`（不再手写）
- **错误重试**：用 gpui-component `alert` 卡片 + 重试按钮
- **响应式宽度**：用 gpui-component `resizable`
- **快捷键**：Esc 中止 / Ctrl+L 清空 / ↑↓ 历史导航

## 6. 关键设计决策对比表

| # | 决策点 | 现状 | 推荐 | 理由 |
|---|---|---|---|---|
| 1 | 视觉语言 | 主题 tokens 但配色冲突 | Claude 哲学 + zenix 主题 tokens | "calm, distraction-free"适配终端应用 |
| 2 | assistant 气泡 | 裸文本无背景 | muted 背景气泡 + max_w 响应式 | 视觉层级清晰，参考 Cursor #F5F5F5 模式 |
| 3 | user 气泡 max_w | 320px 硬编码 | 88% 宽（max 480px） | 宽屏不浪费 |
| 4 | 时间戳 | 永远显示 9px | hover fade-in | 减少 noise |
| 5 | agent_blocks 位置 | messages 上方独立 | 进入消息流（role="tool"） | 时序统一 |
| 6 | streaming indicator | messages 上方独立 | 嵌入最后一条 assistant tail cursor | 视觉连续 |
| 7 | 多行输入 | 单行 InputState | `multi_line(true).auto_grow(true)` | 一行改 |
| 8 | 宽度 | 固定 420 | resizable 360~720 + 持久化 | gpui-component resizable |
| 9 | markdown 扩展 | 6 segment 手写 | 加 heading/blockquote/table/link/task + highlighter crate | table 用 gpui-component；code 用 highlighter |
| 10 | 错误显示 | ⚠️ 文本追加 | alert 卡片 + 重试 + 进入消息流 | 可重试 |
| 11 | **model selector 位置** | header 内 pill | **input bar 底部左侧** | 参 Cursor/Claude/Continue；header 真正极简；选 model 是发送前最后一步 |
| 12 | history/monitor | 浮层覆盖 input 下方 | **抽屉式从右滑入**（不遮挡 input） | 不割裂对话；按 date 分组 |
| 13 | header 按钮 | 4 按钮（More 装饰） | 极简 2 按钮 + menu | 极简 |
| 14 | suggestion chips | 3 个通用文本 | 上下文相关（workspace/git/code） | 实用 |
| 15 | reasoning 块 | 无（engine 有 StreamEvent::Thinking 但 chat 不消费） | assistant msg 内嵌折叠块（默认折叠 + duration） | 参 ChatGPT/Claude reasoning |

## 7. gpui-component widget 复用清单

| 用途 | widget | 文件 |
|---|---|---|
| 多行输入 | `InputState::multi_line(true).auto_grow(true)` | `input/state.rs:540,446` |
| 响应式宽度 | `resizable::Resizable` 或自实现 DragMove | `resizable/` |
| model selector | `combobox` 或 `select` 或自实现 popover | `combobox.rs`, `select.rs`, `popover.rs` |
| 抽屉式 history | `dock` 或自实现 | `dock/` |
| 工具卡片折叠 | `accordion` 或 `collapsible` | `accordion.rs`, `collapsible.rs` |
| markdown 表格 | `table` | `table/` |
| 代码块高亮 | `highlighter` | `highlighter/` |
| 错误卡片 | `alert` | `alert.rs` |
| 状态标签 | `badge` / `tag` | `badge.rs`, `tag.rs` |
| 流式 loading | `spinner` / `skeleton` | `spinner.rs`, `skeleton.rs` |
| tooltip | `tooltip` | `tooltip.rs` |
| 更多按钮菜单 | `menu` | `menu/` |

**结论**：所有需要的 widget 都有现成，**无需自实现任何 UI 组件**。

## 8. Scope 划分

### 必做（v4 核心，决定"像不像样"）

1. **视觉语言重做**：assistant 加 muted 气泡 / user 响应式 max_w / 时间戳 hover / header 极简
2. **消息流统一**：agent_blocks + streaming indicator + error 都进消息流
3. **多行输入**：`multi_line(true).auto_grow(true)` + 历史导航
4. **markdown 扩展**：heading/blockquote/table/link/task + highlighter
5. **响应式宽度**：resizable 360~720 + 持久化到 config
6. **错误卡片化**：alert + 重试
7. **streaming cursor + TTFT indicator**：嵌入消息流 tail
8. **model selector 移到 input bar 底部**（参 Cursor/Claude/Continue 行业惯例）
9. **history/monitor 抽屉式**：从右滑入，不遮挡 input，按 date 分组
10. **reasoning 折叠块**：assistant msg 内嵌，默认折叠 + duration

### 可选（v4 polish，时间允许）

- suggestion chips 上下文相关（workspace/git/code）
- tool calls 批处理（参考 Roo Code grouped blocks）
- @提及 / /命令系统骨架

### 排除（v5+）

- 附件上传
- artifact 面板
- message branching（重新生成）
- 多 provider 同时对话

## 9. 与旧 PRD 的差异

| 旧 PRD（07-05） | v4 |
|---|---|
| "❌ 视觉重做留给后续 task" | 视觉重做是 v4 核心 |
| scope A：流式 + 虚拟化 + 多行 + markdown + 宽度 + 工具卡片 + 错误 | 同 + **视觉语言重做 + 信息架构统一** |
| 不提"Zed 风格"，只抄结构性机制 | 抄 Claude 哲学 + Zed 结构 + Cursor 流式 + Roo 批处理 |
| agent_blocks 留独立区域 | agent_blocks 进消息流 |

**保留的代码资产**：chat/ 模块结构、D1 流式、D2/D3 虚拟化、D10-D12 配置链路全部保留。v4 是在这些地基上重做视觉层 + 信息架构 + 补功能。

## 10. 下一步

1. **用户 review 本方向文档**：确认 / 调整 / 否决
2. 方向确认后 → 用 HTML mockup 可视化关键场景（基于 zenix tokyonight 主题 + Claude 哲学），playwright 截图对比
3. mockup 确认后 → 正式 PRD + design.md + implement.md（取代旧 design.md）
4. 新建 task `08-zenixai-redesign-v4`，把 07-05 标记 paused（保留代码资产）
5. task.py start → 分阶段实施
