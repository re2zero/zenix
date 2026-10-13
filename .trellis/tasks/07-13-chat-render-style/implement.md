# Implement: AI 会话渲染样式优化 (v2)

## 审查修正

- `AgentComplete` 是死代码，tool_card.rs 保留不动但不在 scope 内
- `ToolResult` 事件签名变更（新增 `id` 字段），影响 engine.rs + app.rs 两处 match
- 思考块左色条用 `h_flex` + child div 实现，GPUI 无单侧 border API
- 工具卡片类型区分 + 耗时计算直接合并到 Step 2 一次完成（同为 `render_tool_call_card()`）
- `overflow_y_scrollbar` 替代 `overflow_y_scroll`（已验证存在于代码库中）
- `timestamp_ms()` → `now_ms()`，`truncate()` → `summarize()`，在 app.rs 中实现

## 依赖关系

```
Step 1 → Step 2 → Step 4 → Step 5 → Step 6
```

所有步骤严格顺序执行。Step 1 是所有后续步骤的前置（数据模型 + 事件签名变更）。Step 2 完成后即可在渲染中看到工具卡片。Step 4/5/6 互不依赖，但需 Step 1 完成的 tool_calls 数据和 Step 2 完成的渲染函数可用。

---

### Step 1: ToolCallState 数据模型 + stream 事件签名变更

**文件**: `src/ai/session.rs`, `src/ai/engine.rs`, `src/app.rs`

**变更**:
1. `src/ai/session.rs`: 新增 `ToolCallState` 结构体 + `ToolCallStatus` enum。在 `ChatMessage` 中新增 `tool_calls: Vec<ToolCallState>` 字段。
2. `src/ai/engine.rs`: `DispatchStreamEvent::ToolResult` 签名从 tuple 改为 struct 变体，携带 `id: String`。同步更新 `chat_with_tools()` 中的构造逻辑（`tool_calls.iter().zip(results.iter())`）。
3. `src/app.rs`: 
   - `DispatchStreamEvent::ToolCall` → push ToolCallState（含 `now_ms()` 时间戳）
   - `DispatchStreamEvent::ToolResult` → 按 id 匹配更新状态 + `summarize()` 摘要
   - 删除原 inline markdown 追加逻辑（约 6 行）
   - 在文件级添加 `now_ms()` 和 `summarize()` 辅助函数

**验证**:
- `cargo check --no-deps` 通过
- `cargo test` 通过
- `cargo clippy --no-deps` 无新增 warning

**回退**: `git diff src/ai/session.rs src/ai/engine.rs src/app.rs`

---

### Step 2: 流式工具调用卡片渲染 + 类型区分 + 耗时 (合并 Step 3)

**文件**: `src/ai/chat/messages.rs`

**变更**:
1. 在 `render_message()` 中，reasoning block 之后、content 之前，遍历 `msg.tool_calls` 渲染
2. 实现 `render_tool_call_card()` 函数（一个函数完成全部工具卡片逻辑）：
   - 紧凑卡片：`muted` 背景 + 3px 强调色左条（`h_flex` + child `div().w(px(3)).bg(accent)`）
   - 卡片 header: [icon] [工具名] [状态 ✓/●/✕] [耗时 ⏱ 1.2s]
   - 工具名→icon 映射：`spawn_worker→🤖`, `send_text→📨`, `read_output→📖`, `wait_for_output→⏳`, `list_workers→📋`, `close_worker→🗑`
   - 参数摘要：提取关键字段（agent, instruction, pattern, pane_id），不展示原始 JSON
   - Running 态: 脉冲动画 via `with_animation`（透明度脉冲 `pulsating_between`）
   - Running 态实时计时：用 `started_at` 与当前时间差计算 `elapsed`
   - Completed: 绿色 ✓ + 耗时 `✓ 1.2s`
   - Failed: 红色 ✕
   - 可折叠：click header 展开/收起；折叠显示参数摘要，展开显示格式化 JSON + 结果全文

**验证**:
- `cargo check --no-deps` 通过
- `cargo fmt --check` 无 diff
- `cargo test` 通过

---

### Step 4: 思考块样式强化

**文件**: `src/ai/chat/messages.rs`

**变更**:
1. `render_reasoning_block()`: 容器从 `v_flex` 改为 `h_flex` + child div
   - 左: `div().w(px(3)).bg(accent).rounded_full()` 作为色条
   - 右: `v_flex().flex(1)` 包含 header + body
2. Header: 添加 `🧠` 图标前缀（当前为 `▶/▼`）
3. 折叠态: `collapsed` 时只显示 header，隐藏 body
4. 流式高度: `max_h(px(200)).overflow_y_scroll()` 替代 `h(px(90))`

**验证**:
- `cargo check --no-deps` 通过
- `cargo fmt --check` 无 diff
- `cargo test` 通过

---

### Step 5: 消息时间线指示器

**文件**: `src/ai/chat/messages.rs`

**变更**:
1. `render_message()` 外层容器加 `.relative()`
2. `RenderCtx` 新增 `is_first_in_group: bool`，由 `mod.rs::render_ai_chat_panel` 在 list 闭包中计算
3. `role_indicator()` 函数: 绝对定位 left(-6)，w(3)，根据 role 着色
4. 角色切换检测: `ix == 0 || messages[ix].role != messages[ix-1].role`

**验证**:
- `cargo check --no-deps` 通过
- `cargo fmt --check` 无 diff

---

### Step 6: Agent 监视器样式增强

**文件**: `src/ai/chat/header.rs`

**变更**:
1. `render_agent_monitor()` 中的条目渲染:
   - 状态文本改为 `status_pill()`（背景色 rounded_full pill）
   - 增加 `recent_output[..60]` 灰色预览文本
2. `status_pill()`: 根据 status 返回不同 bg/fg 色的 pill 元素

**验证**:
- `cargo check --no-deps` 通过
- `cargo fmt --check` 无 diff
- `cargo test` 通过

---

### Step 7: 完整功能验证

**验证**:
1. `cargo check --no-deps` — 无错误
2. `cargo fmt --check` — 无 diff
3. `cargo clippy --no-deps` — 无新增 warning
4. `cargo test` — 全部通过
5. `cargo run` — 启动正常，chat panel 渲染无 panic

**审查门禁**:
- 每个 step 产生 git diff，回退粒度为单个 step
- 所有渲染路径在 4 主题下视觉正常
