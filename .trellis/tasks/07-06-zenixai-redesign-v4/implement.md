# ZenixAI Redesign v4 — 执行计划

> 配套：`prd.md`（需求）/ `design.md`（技术设计）
> 分 3 phase 独立可验证，但 ship as ONE atomic PR

## 总览

```
Phase 1: 视觉层重做（R1-R4, R13）— 不动数据流，纯渲染层
Phase 2: 信息架构统一（R5-R9）— 改数据流 + 消息流统一
Phase 3: 功能补全（R10-R12, R14）— input/markdown/width/chips
```

每个 phase 完成后：`cargo check` + `cargo clippy` + 手动流式验证 + `trellis-check`。

## 前置准备（phase 0）

- [ ] 确认 gpui-component widget API（避免实施时才发现 API 不符）
  - grep `crates/ui/src/highlighter/` 确认 highlighter 公开 API
  - grep `crates/ui/src/table/` 确认 table 用法
  - grep `crates/ui/src/resizable/` 确认 resizable 或决定自实现 DragMove
  - grep `crates/ui/src/dock/` 确认 dock 是否适合 drawer，或自实现 overlay
  - grep `crates/ui/src/input/state.rs` 确认 `multi_line(true).auto_grow(true)` API
- [ ] 确认 engine.rs 是否有 `StreamEvent::Thinking`（决定 R9 reasoning 可行性）
- [ ] 截图当前状态作为 before 对比（4 主题）

## Phase 1: 视觉层重做

**目标**：chat 看起来像现代 AI 客户端。不动数据流、不动 messages 结构。

**改动文件**：
- `src/ai/chat/messages.rs` — user/assistant 气泡重做、时间戳 hover、streaming tail cursor
- `src/ai/chat/header.rs` — 极简化 + menu
- `src/ai/chat/input.rs` — model selector 移到底部
- `src/ai/chat/mod.rs` — render_ai_chat_panel 调整（删 streaming_indicator 独立区域改嵌入 tail）
- `src/ai/chat/state.rs` — RenderCtx / streaming tail 标记

### 步骤

1. **R2 user 气泡响应式宽度**
   - `messages.rs::render_message` 的 user 分支：max_w 从 320px 改为 `panel_width * 0.88`（panel_width 从 render_ctx 传入）
   - 右对齐保留，accent 背景保留
   - 验证：360/480/600 三档宽度下 user 气泡不溢出

2. **R1 assistant muted 气泡**
   - `messages.rs::render_message` 的 assistant 分支：加 `bg(secondary)` 或 `bg(muted)`，全宽左对齐
   - 验证：4 主题下 assistant 有可见背景，与 user accent 形成对比

3. **R6（部分）streaming tail cursor**
   - `mod.rs`：删除 `messages::render_streaming_indicator` 独立区域（mod.rs:99-106）
   - `messages.rs::render_message`：接收 `is_streaming_tail: bool` 参数，assistant 分支末尾若 true 追加 pulsing block（gpui Animation）
   - `mod.rs::render_ai_chat_panel`：list render_item 时判断 `ix == data.streaming_msg_idx && data.is_streaming`
   - 验证：流式过程中最后一条 assistant msg 末尾有动画 cursor；无独立 streaming 区域

4. **R3 时间戳 hover fade-in**
   - `messages.rs::render_message`：外层 div 加 hover 状态跟踪
   - 默认不渲染时间戳；hover 时绝对定位 fade-in
   - hover 状态管理：简化为 `AtomicUsize`（last_hovered_msg_id）或 HoverState entity
   - 验证：默认无时间戳；hover 消息时淡入

5. **R4 header 极简化**
   - `header.rs::render_header`：删 More 按钮（无 on_click）、provider pill
   - 保留：title + new chat 按钮 + menu 按钮
   - menu 用 gpui-component `menu`：History / Agents / Clear / Settings 项
   - menu 按钮 → `AiChatAction::OpenMenu`
   - 验证：header 恒定 ≤2 按钮；menu 展开含 4 项

6. **R13 model selector 移到 input bar 底部**
   - `header.rs`：删 model/provider dropdown（保留逻辑到 input）
   - `input.rs::render_input_bar`：底部加 model selector 行（左对齐），用 gpui-component `select` 或 `combobox`
   - `SearchableVec<String>` 直接做 delegate（参 memory 1441）
   - 验证：header 无 model selector；input bar 底部左侧可选 model

### Phase 1 验证

- [ ] `cargo check` 通过
- [ ] `cargo clippy` 无新 warning
- [ ] 手动：4 主题下截图对比 before/after
- [ ] 手动：流式发送消息，tail cursor 正常，D1 不回归
- [ ] 手动：虚拟化滚动正常（D2/D3 不回归）
- [ ] 手动：provider/model 选择生效（D10-D12 不回归）
- [ ] `trellis-check` sub-agent 验证

### Rollback point

Phase 1 完成后 git commit（`feat(zenixai): v4 phase 1 visual layer`）。若 Phase 2 出问题，可回退到此 commit。

---

## Phase 2: 信息架构统一

**目标**：消除视觉割裂，所有内容进单一消息流 + 抽屉。

**改动文件**：
- `src/ai/chat/state.rs` — add_agent_block 同时 push message；drawer 字段；AiChatAction 扩展
- `src/ai/chat/mod.rs` — 删 agent_blocks 独立区域；drawer overlay；render_message 按 role 分发
- `src/ai/chat/tool_card.rs`（新）— role="tool" 渲染
- `src/ai/chat/error_card.rs`（新）— role="error" 渲染
- `src/ai/chat/reasoning.rs`（新）— role="reasoning" 渲染
- `src/ai/chat/drawer.rs`（新）— history/agents 抽屉
- `src/ai/chat/agent_block.rs` — 渲染逻辑移到 tool_card.rs，struct 保留
- `src/app.rs` — error/reasoning push 到 messages；drawer action 处理

### 步骤

1. **state.rs 数据模型扩展**
   - 新增 `DrawerKind` enum
   - AiChatPanelData：`drawer: Option<DrawerKind>`, `retry_last_prompt` 字段
   - 移除 `show_history`/`show_agents`（合并到 drawer）
   - `add_agent_block`：push ChatMessage { role: "tool", content, agent_activities: Some(json) }
   - AiChatAction 扩展：OpenDrawer/CloseDrawer/RetryLast/OpenMenu，移除 ToggleHistory/ToggleAgents
   - 验证：`cargo check` 通过；现有 toggle 逻辑编译报错（待后续步骤修复）

2. **R5 agent_blocks 进消息流**
   - `mod.rs`：删除 agent_blocks 独立区域（mod.rs:90-97）
   - 新增 `tool_card.rs`：render role="tool"，从 agent_activities JSON 解析 AgentBlock 状态
   - 折叠/grouped：连续 role="tool" 且同 group_id 合并为 accordion
   - `messages.rs::render_message`：role="tool" 分支调 `tool_card::render`
   - `agent_block.rs`：保留 struct，渲染函数移走
   - 验证：tool call 出现在消息流按时序；连续 ≥2 个折叠为 grouped

3. **R7 错误卡片化**
   - `app.rs`：stream error 时 push `ChatMessage { role: "error", content: err_text }`（替代 ⚠️ 文本追加）
   - 新增 `error_card.rs`：render role="error"，gpui-component `alert` + 重试按钮
   - 重试 → `AiChatAction::RetryLast` → `start_chat_streaming(last_user_prompt)`
   - 验证：错误显示为 alert 卡片；重试按钮重新发起请求

4. **R9 reasoning 折叠块**（若 engine 支持 Thinking）
   - grep 确认 `engine.rs` 有 `StreamEvent::Thinking`
   - 若有：app.rs 收到 Thinking 时 push `ChatMessage { role: "reasoning", content, agent_activities: Some(json!({"duration_ms": N})) }`
   - 新增 `reasoning.rs`：render role="reasoning"，默认折叠，标题「思考了 Ns」
   - 若 engine 无 Thinking：此步骤跳过，PRD R9 标记为「待 engine 支持」
   - 验证：reasoning 显示为折叠块；duration 准确

5. **R8 history/monitor 抽屉**
   - 新增 `drawer.rs`：render_drawer_overlay(kind, data, theme)
   - overlay：绝对定位 right=0 w=320 h_full，半透明遮罩 inset=0
   - history 按 date 分组（parse created_at）
   - 滑入动画：gpui Animation translate_x
   - `mod.rs`：替换 show_history/show_agents 浮层为 drawer overlay
   - Esc / 遮罩点击 → `AiChatAction::CloseDrawer`
   - 验证：抽屉从右滑入；不遮挡 input；按 date 分组；Esc 关闭

6. **render_message 按 role 分发**
   - `messages.rs::render_message`：match role 分发到 user/assistant/tool/error/reasoning 子模块
   - RenderCtx 携带 streaming tail + hover 上下文
   - 验证：5 种 role 渲染正确；未知 role 空 div

### Phase 2 验证

- [ ] `cargo check` + `cargo clippy` 通过
- [ ] 手动：tool call 进消息流，时序正确
- [ ] 手动：错误显示为卡片，重试工作
- [ ] 手动：history 抽屉滑入，不遮挡 input，date 分组
- [ ] 手动：流式不回归（D1）
- [ ] 手动：虚拟化不回归（D2/D3）
- [ ] `trellis-check` sub-agent 验证

### Rollback point

Phase 2 完成后 git commit（`feat(zenixai): v4 phase 2 unified message flow`）。

---

## Phase 3: 功能补全

**目标**：多行输入、响应式宽度、markdown 完整、上下文 chips。

**改动文件**：
- `src/ai/chat/input.rs` — 多行 + auto_grow + 历史导航
- `src/ai/chat/resize.rs`（新）— 拖拽 handle
- `src/ai/chat/mod.rs` — 宽度改 ai_panel_width；resize handle
- `src/ai/chat/markdown_ext.rs`（新）— 扩展 parser + highlighter
- `src/ai/chat/messages.rs` — markdown 渲染用扩展 parser
- `src/ai/chat/header.rs` — empty state 上下文 chips
- `src/ai/chat/state.rs` — ai_panel_width / input_history 字段
- `src/app.rs` — ai_panel_width 持久化
- `src/config.rs` — ai_panel_width 配置

### 步骤

1. **R10 多行输入 + 历史导航**
   - `input.rs`：InputState::new().multi_line(true).auto_grow(true)
   - Enter 发送、Shift+Enter 换行（key_down handler）
   - ↑/↓ 历史导航：input_history Vec + idx，行首↑调出上一条
   - state.rs：input_history / input_history_idx 字段
   - 验证：≥4 行 auto_grow；Shift+Enter 换行；↑ 调出历史

2. **R11 响应式宽度 + 持久化**
   - state.rs：ai_panel_width 字段（default 420）
   - config.rs：ai_panel_width 配置字段
   - 新增 `resize.rs`：拖拽 handle（on_drag），clamp 360~720，实时更新 ai_panel_width
   - mod.rs：w(px(AI_PANEL_WIDTH)) → w(px(data.ai_panel_width))；加 resize handle 子元素
   - app.rs：拖拽 end debounce 写 config；启动读 config
   - 移除 AI_PANEL_WIDTH 常量（保留为 default 值）
   - 验证：拖拽实时改宽度；clamp 生效；重启保留

3. **R12 markdown 扩展**
   - 新增 `markdown_ext.rs`：在现有 6 segment 基础上加 heading/blockquote/table/link/task
   - code block：用 gpui-component `highlighter`（先 grep 确认 API）
   - table：用 gpui-component `table`
   - messages.rs：assistant 分支的 markdown 渲染改用 markdown_ext
   - 验证：heading/blockquote/table/link/task/code-block 正确渲染；代码高亮

4. **R14 suggestion chips 上下文相关**
   - header.rs::render_empty_state：检测上下文（git repo / 活跃 pane / 选中文件）
   - 3 类 chip：workspace / git / code
   - 点击 → SuggestionClick(text)
   - 验证：empty state 显示 ≥3 个上下文相关 chip

### Phase 3 验证

- [ ] `cargo check` + `cargo clippy` 通过
- [ ] 手动：多行输入 + 历史导航
- [ ] 手动：拖拽改宽度，重启保留
- [ ] 手动：markdown 含 heading/table/code-block 正确渲染
- [ ] 手动：empty state chips 上下文相关
- [ ] 全场景回归（D1 流式 / D2-D3 虚拟化 / D10-D12 配置 / Tab 拦截）
- [ ] `trellis-check` sub-agent 全量验证

---

## 集成验证（3 phase 全部完成后）

### 必跑

- [ ] `cargo check`
- [ ] `cargo clippy`（无新 warning）
- [ ] `cargo test`（现有测试不回归）
- [ ] `cargo fmt --check`

### 场景验收（参 PRD §端到端验收标准）

- [ ] 场景 1：空状态（header 极简 + chips + model selector 底部 + 多行 input）
- [ ] 场景 2：流式响应（user accent / assistant muted / tail cursor / send→stop）
- [ ] 场景 3：工具调用（消息流 + grouped + lifecycle）
- [ ] 场景 4：错误 + 多行（alert 卡片 + 重试 + auto_grow）
- [ ] 场景 5：响应式宽度（拖拽 + clamp + 持久化）
- [ ] 场景 6：history 抽屉（滑入 + 不遮挡 input + date 分组 + Esc）
- [ ] 场景 7：reasoning 折叠（若 engine 支持）
- [ ] 跨场景：4 主题 / 时间戳 hover / markdown 完整 / Tab 不拦截

### 视觉对比

- [ ] 4 主题截图对比 before（phase 0 截图）/ after
- [ ] 确认无 AI slop（紫色渐变 / emoji 图标 / 圆角卡片+左 border accent）

## trellis-check 全量

3 phase 全部完成后，dispatch `trellis-check` sub-agent 做全量验证（spec 合规 + lint + type-check + 跨层数据流 + 代码复用 + 一致性）。

## Ship

- [ ] git commit（atomic，3 phase 合并为单 commit 或保留 3 commit 但单 PR）
- [ ] PR 描述含 before/after 截图
- [ ] 不分阶段合并（参 memory 1906）

## Sub-agent dispatch 约定

每个 phase 用 `trellis-implement` sub-agent 执行，prompt 格式：

```
Active task: .trellis/tasks/07-06-zenixai-redesign-v4

[CONTEXT]: 读取 prd.md + design.md + implement.md 的 Phase N 部分
[GOAL]: 完成 Phase N 的所有步骤
[MUST DO]: 遵循 design.md 的架构决策；不动保留资产；每步 cargo check
[MUST NOT DO]: 不引入新依赖；不在 render() mutate state；不用 unwrap()
[VERIFY]: Phase N 验证清单全部通过
```

每个 phase 完成后 `trellis-check` 验证。失败则用 session_id 继续 fix。
