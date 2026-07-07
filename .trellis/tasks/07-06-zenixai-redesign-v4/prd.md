# ZenixAI Redesign v4: 视觉语言 + 信息架构 + 功能补全

> 蓝本：`../07-05-zenixai-structural-redesign/redesign-v4-direction.md`
> Mockup：`../07-05-zenixai-structural-redesign/mockup/v4-mockup.html` + 7 场景 PNG
> Supersedes `../07-05-zenixai-structural-redesign/prd.md`（scope 错误：旧 PRD 把视觉重做排除）

## Goal

一次性把 ZenixAI chat 做到「像样的现代 AI 客户端」。前三轮失败的根因：
- 03/04：换皮不修结构
- 07-05：修了结构但 PRD 明确写「❌ 视觉重做留给后续 task」——核心误判

v4 在保留 07-05 代码资产（chat/ 6 子模块、D1 流式、D2/D3 虚拟化、D10-D12 配置链路）的地基上，**同时**做视觉重做 + 信息架构统一 + 功能补全，ship as ONE atomic PR。

## 痛点（用户原话）

1. 视觉丑，不像现代 AI 客户端
2. 功能缺失严重
3. 整体方向错了，推倒重来

「推倒重来」指**视觉设计 + 信息架构 + scope 划分**推倒，**不是代码全删**。流式 token 已工作（用户实测），代码资产保留。

## 保留的代码资产（不动）

| 资产 | 位置 | 验证 |
|---|---|---|
| chat/ 6 子模块 | `src/ai/chat/{mod,state,messages,input,header,agent_block}.rs` 共 1942 行 | 已拆 |
| D1 流式 + 4 bug fix | `src/app.rs:2623-2852` | 用户实测工作 |
| D2/D3 虚拟化 | `state.rs:133-134` + `app.rs:3047-3048` 用 `ListState::new(0, ListAlignment::Bottom, ..)` + `set_follow_mode(FollowMode::Tail)` | 已用 gpui 原生 |
| D10-D12 配置链路 | `start_chat_streaming` provider/model override；`AiChatAction` enum 统一 dispatch | 完成 |

## 需求项（15 决策转可验证需求）

### 视觉语言（决策 1-4, 13）

**R1. assistant muted 背景气泡**
- assistant 消息加 muted 背景气泡（用主题 `secondary`/`muted` token，非 accent），与 user accent 背景形成层级对比
- 验收：4 主题（gruvbox/solarized/tokyonight/matrix）下 assistant 均有可见背景，不与 user 气泡混淆

**R2. user 气泡响应式宽度**
- user 气泡 max_w 从硬编码 320px 改为面板宽度的 88%（max 480px），右对齐
- 验收：面板宽 360/480/600 三档下，user 气泡宽度分别为 317/422/480（不溢出、不浪费）

**R3. 时间戳 hover fade-in**
- 删除永远显示的时间戳行，改为消息 hover 时 fade-in 显示（绝对定位）
- 验收：默认无时间戳；hover 消息时时间戳淡入显示；离开消失

**R4. header 极简化**
- header 只保留：title「ZenixAI」+ 2 按钮（new chat + menu）。menu 含 history/agents/clear/settings
- 删除装饰性「More」按钮（无 on_click）、provider pill
- 验收：header 恒定 ≤2 按钮；menu 点击展开含 history/agents/clear/settings 4 项

### 信息架构统一（决策 5, 6, 10, 12, 15）

**R5. agent_blocks 进入消息流**
- agent_blocks 从 messages 上方独立区域，改为作为 `role="tool"` 的 ChatMessage 进入消息流，按时序排列
- 连续 tool calls 批处理为 grouped block（折叠/展开，参 Roo Code）
- 验收：tool call 与对话消息按发生时序交错显示；连续 ≥2 个 tool call 自动折叠为 grouped block

**R6. streaming indicator 嵌入消息流 tail**
- streaming cursor 从 messages 上方独立区域，改为嵌入最后一条 assistant msg 内容末尾（pulsing block 动画）
- 验收：流式过程中，最后一条 assistant msg 文本末尾有动画 cursor；无独立 streaming 区域

**R7. 错误卡片化 + 进入消息流**
- 错误从 ⚠️ 文本追加，改为 `role="error"` 的 ChatMessage 进入消息流，用 alert 卡片 + 重试按钮
- 验收：错误以独立卡片显示在消息流；点击「重试」重新发起上次请求

**R8. history/monitor 抽屉式**
- history/agent monitor 从浮层（覆盖 input 下方），改为从右侧滑入的抽屉（320px，半透明遮罩），不遮挡 input
- history 按 date 分组（今天/昨天/本周/更早）
- 验收：打开抽屉时 input 仍可见可输入；抽屉内容按 date 分组；点击遮罩或 Esc 关闭

**R9. reasoning 折叠块**
- assistant msg 内嵌 reasoning 块（默认折叠 + 显示 duration），点击展开
- 验收：含 reasoning 的 assistant msg 显示「思考了 Ns」折叠行；点击展开显示 reasoning 内容；再次点击折叠

### 功能补全（决策 7, 8, 9, 11, 14）

**R10. 多行输入 + 历史导航**
- input bar 从单行改为 `InputState::multi_line(true).auto_grow(true)`，支持 Shift+Enter 换行、Enter 发送
- ↑/↓ 在历史输入中导航
- 验收：input 可输入多行（≥4 行 auto-grow）；Shift+Enter 换行；Enter 发送；↑ 调出上一条历史输入

**R11. 响应式宽度 + 持久化**
- 面板宽度从固定 `AI_PANEL_WIDTH=420`，改为 resizable 360~720，拖拽 handle 调节
- 宽度持久化到 config（`ai_panel_width` 字段），重启保留
- 验收：拖拽 handle 实时改变宽度（clamp 360~720）；重启 zenix 后宽度保留；<360 不再缩小，>720 不再放大

**R12. markdown 扩展**
- 在现有 6 segment（Plain/Bold/Italic/InlineCode/CodeBlock/ListItem）基础上加：heading/blockquote/table/link/task
- 代码块用 gpui-component `highlighter` 做语法高亮（替代手写着色）
- table 用 gpui-component `table`
- 验收：assistant 回复含 heading/blockquote/table/link/task/code-block 时正确渲染；代码块有语法高亮

**R13. model selector 移到 input bar 底部**
- model selector 从 header pill，移到 input bar 底部左侧（参 Cursor/Claude/Continue 行业惯例）
- 用 gpui-component `combobox` 或 `select`（SearchableVec<String> 直接做 delegate）
- 验收：header 无 model selector；input bar 底部左侧有 model selector；点击展开 provider/model 列表；选择后生效

**R14. suggestion chips 上下文相关**
- empty state 的 suggestion chips 从 3 个通用文本，改为上下文相关（workspace/git/code 三类）
- 验收：empty state 显示 ≥3 个 chip，文本与当前 workspace 上下文相关（如「分析当前 git 改动」「解释这个文件」）

## Scope

### 必做（v4 核心，决定「像不像样」）

R1-R14 全部。一次性做对。

### 可选（v4 polish，时间允许）

- tool calls 批处理增强（grouped block 的「Save All」「Deny All」按钮，agent 审批场景）
- @提及 / /命令系统骨架（仅前端识别 + autocomplete UI，不接后端）
- TTFT 间隙 typing indicator（用户发消息到首个 token 之间的三个动画点）

### 排除（v5+）

- 附件上传
- artifact 面板
- message branching（重新生成）
- 多 provider 同时对话

## 约束

### 硬约束（不可违反）

1. **不引入新直接依赖**：markdown 扩展手写 parser；`highlighter`/`table`/`resizable` 等都是 gpui-component 内置
2. **不删 chat/ 6 子模块结构**：保留 D 拆分，可按需新增子模块（如 `chat/error_card.rs`/`chat/reasoning.rs`）
3. **不破坏 D1 流式 / D2-D3 虚拟化 / D10-D12 配置链路**：改动前先读懂现有实现
4. **GPUI 后台 executor 线程不继承 tokio runtime**：所有 tokio spawn 必须用 `engine.rs` 的 `tokio_handle()` 辅助函数（参 memory 1938/1951）
5. **Tab 键不能被 gpui-component Root 拦截**：保持 07-05 的 `TabToPty`/`ShiftTabToPty` context-free binding 方案（参 memory 1666）
6. **所有交互通过 AiChatAction enum 统一 dispatch**：新增交互加 enum variant，不另起 handler（参 memory 1936/1949）
7. **ship as ONE atomic PR**：不分阶段合并，避免半状态（参 memory 1906）
8. **不用 `unwrap()`**：用 `anyhow::Context` 或 `thiserror`
9. **不在 render() 里 mutate state**：用 `compute_view()` 或 action

### 设计约束（反 AI slop）

- 禁紫色渐变
- 禁 emoji 图标（用 gpui-component icon 或 SVG）
- 禁圆角卡片 + 左 border accent 的 AI slop 套路
- 配色用主题 token，不硬编码

## 依赖（gpui-component widget 复用，无需自实现）

| 用途 | widget | 源文件 |
|---|---|---|
| 多行输入 | `InputState::multi_line(true).auto_grow(true)` | `crates/ui/src/input/state.rs:540,446` |
| 响应式宽度 | `resizable::Resizable` 或自实现 DragMove | `crates/ui/src/resizable/` |
| model selector | `combobox` / `select`（SearchableVec<String>） | `combobox.rs`, `select.rs` |
| 抽屉式 history | `dock` 或自实现 | `crates/ui/src/dock/` |
| 工具卡片折叠 | `accordion` / `collapsible` | `accordion.rs`, `collapsible.rs` |
| markdown 表格 | `table` | `crates/ui/src/table/` |
| 代码块高亮 | `highlighter` | `crates/ui/src/highlighter/` |
| 错误卡片 | `alert` | `alert.rs` |
| 状态标签 | `badge` / `tag` | `badge.rs`, `tag.rs` |
| 流式 loading | `spinner` / `skeleton` | `spinner.rs`, `skeleton.rs` |
| tooltip | `tooltip` | `tooltip.rs` |
| menu | `menu` | `crates/ui/src/menu/` |

## 端到端验收标准

### 场景 1：空状态
- [ ] header 极简（title + new + menu）
- [ ] 居中 empty state：上下文相关 suggestion chips（≥3）
- [ ] input bar 底部左侧有 model selector
- [ ] input 可多行输入

### 场景 2：流式响应
- [ ] user 气泡 accent 背景、右对齐、88% 宽
- [ ] assistant 气泡 muted 背景、左对齐
- [ ] streaming cursor 嵌入最后一条 assistant msg tail（动画）
- [ ] send 按钮变 stop（流式中可中止）
- [ ] 无独立 streaming indicator 区域

### 场景 3：工具调用
- [ ] tool call 作为消息进入消息流（role="tool"），按时序
- [ ] 连续 ≥2 个 tool call 折叠为 grouped block
- [ ] grouped block 显示 lifecycle 状态点（pending/running/success/error）

### 场景 4：错误 + 多行
- [ ] 错误以 alert 卡片进入消息流（非 ⚠️ 文本追加）
- [ ] alert 含「重试」按钮，点击重新发起请求
- [ ] input 支持 ≥4 行 auto-grow
- [ ] Shift+Enter 换行，Enter 发送

### 场景 5：响应式宽度
- [ ] 拖拽 handle 实时改变宽度（clamp 360~720）
- [ ] 360/480/600 三档下布局均合理（无溢出、无浪费）
- [ ] 重启后宽度保留

### 场景 6：history 抽屉
- [ ] 从右滑入 320px，半透明遮罩
- [ ] 不遮挡 input（input 仍可输入）
- [ ] 按 date 分组（今天/昨天/本周/更早）
- [ ] Esc 或点遮罩关闭

### 场景 7：reasoning 折叠
- [ ] 含 reasoning 的 assistant msg 显示「思考了 Ns」折叠行
- [ ] 默认折叠，点击展开
- [ ] duration 准确

### 跨场景
- [ ] 时间戳默认不显示，hover 消息 fade-in
- [ ] 4 主题（gruvbox/solarized/tokyonight/matrix）下视觉均合理
- [ ] markdown 含 heading/blockquote/table/link/task/code-block 时正确渲染
- [ ] 代码块语法高亮
- [ ] Tab 键正常传入终端 PTY（不被拦截）
- [ ] 流式 token 实时显示（D1 不回归）
- [ ] 虚拟化滚动正常（D2/D3 不回归）
- [ ] provider/model 选择生效（D10-D12 不回归）

## 风险

| 风险 | 缓解 |
|---|---|
| widget API 与设计不符 | 实施前先 grep gpui-component 源码确认 API（参考 direction §7） |
| D1 流式回归 | 每个 phase 实施后跑流式验证；streaming cursor 改动只动渲染层不动数据流 |
| 拖拽 handle 与 list 滚动冲突 | handle 用独立 overlay 层，事件不冒泡到 list |
| markdown 手写 parser 边界 case | 测试用例覆盖 heading/blockquote/table/link/task/code-block 嵌套 |
| 一次性 atomic PR 过大 | implement.md 分 3 phase 独立可验证，但合并为单 PR |

## 参考资料

- 设计方向主文档：`../07-05-zenixai-structural-redesign/redesign-v4-direction.md`
- mockup：`../07-05-zenixai-structural-redesign/mockup/`（7 场景 + 全景图）
- 参考产品：Claude Desktop（distraction-free 哲学）、Cursor（streaming cursor + tool lifecycle）、Zed Assistant（同 GPUI 的 FollowMode::Tail）、Cline/Roo Code（批处理 tool calls）、ChatGPT（reasoning 折叠）、Continue.dev（代码段 action buttons）
