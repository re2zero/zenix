# PRD: zenixAI @ Agent Mentions & Orchestration Overhaul

## Goal

将 zenixAI 从一个简单的 LLM 聊天面板，推进为真正的**任务编排/分发/监督中心**。两个主要交付物：

1. **`@` Agent 提及** — 输入框中 `@worker_name` 直接对特定 worker agent 发消息（类似群聊 @）
2. **编排体系整理** — 提示词、工具、管理面板的整体梳理和补全

---

## 已知事实（代码库调研结果）

### 当前状态

**输入系统（`src/ai/chat/input.rs`）**:
- 使用 `gpui_component::input::Input` 多行输入
- placeholder 写着 "Ask anything, dispatch agents, or type / for commands"
- Enter 发送、Shift+Enter 换行
- 输入框下有 model/thinking 下拉选择器和发送按钮

**编排引擎（`src/ai/engine.rs`）**:
- `SYSTEM_PROMPT` 只有一行（第 42 行）: "你是 ZenixAI 编排器..."
- `OrchestratorEngine` 实现了 SSE streaming、tool calling loop、agent dispatch
- `parse_dispatch_directive()` 支持从 LLM 回复中解析 JSON/text 格式的 dispatch 指令
- `execute_agent_task()` 通过 herdr IPC 启动 Agent CLI、发送指令、等待 `[AGENT_DONE]` 模式
- `chat_and_dispatch()` 编排循环：LLM → 解析 dispatch → 执行 agent → 汇总 → 再次调用 LLM
- `chat_with_tools()` 支持 function calling 循环，但当前 dispatch 路径走的是 text parsing 而非 tool calls

**工具系统（`src/ai/tools.rs`）**:
- 12 个内置工具：文件操作（read_file/read_directory/search_files）、Agent 管理（list_agents/start_agent/send_instruction/read_output）、布局控制（split_pane/arrange_grid）、内存管理（read_memory/write_memory/search_memory）
- 全部通过 serde_json 参数传递
- Memory 工具仅使用进程内 HashMap，未持久化

**状态模型（`src/ai/chat/state.rs`）**:
- `AiChatPanelData` 已有 `agent_blocks: Vec<AgentBlock>`、`agent_statuses: Vec<AgentMonitorEntry>`、`active_agent_count: usize`
- `DrawerKind::Agents` 抽屉已定义但未实现
- `AgentBlock` 和 `AgentMonitorEntry` 定义在 `agent_block.rs` 中

**Agent 检测（`src/agent/mod.rs`）**:
- `detect_all_agents()` 通过 `ProviderRegistry` 检测已安装的 Agent CLI

**MCP 集成（`src/mcp/mod.rs`）**:
- 已实现跨 Agent 的 MCP 服务器配置管理

**技能系统（`src/skills/mod.rs`）**:
- 已实现技能扫描、安装、链接、skills.sh API

### 缺失项

1. `@` worker 提及弹出：无任何实现
2. 无角色/worker 注册体系 — 编排器不知道有哪些 worker、各负责什么
3. 编排器提示词过于简单（一行），缺乏详细的工具描述、dispatch 格式说明、约束条件
4. 无 workspace 选择/绑定 — 当前无法识别在哪个 herdr workspace 工作，对话无 cwd 上下文
5. DrawerKind::Agents 已占位但未实现（应展示按 workspace 分组的会话列表）
6. 没有统一的 Agent CLI / 角色管理 UI
7. 工具/技能/MCP 没有统一的管理入口

---

## 设计概念

### 角色 (Role)

一个角色定义了"某个 AI agent 是什么、擅长做什么"。来自 `agency-agents` 等模板库的 `.md` 文件（与 OpenCode agents 格式兼容），包含：
- 名称和描述
- 核心职责
- 专业领域和技术栈
- 工作流程和产出标准

角色**注册**到 zenixAI 后，编排器知道有哪些可用角色、各角色的分工。

### Worker

Worker 是一个**正在运行的 Agent CLI 实例**。编排器根据任务需求选择一个角色、启动一个 Agent CLI（如 opencode/claude），并赋予它一个 **worker_name**。

worker_name 格式：`<cli缩写>_<标识>(<角色>)`
- `oc_mark(frontend)` — 用 OpenCode 跑前端开发角色
- `cd_alice(backend)` — 用 Claude Code 跑后端角色
- `ox_bob(reviewer)` — 用 Codex 跑代码审查角色

### @ 提及

`@` 在输入框中弹出的是 **worker 列表**（不是 CLI 类型），选择后消息直接路由到该 worker 的 herdr pane。

---

## 需求

### R1: 角色注册体系

- 内置 3-5 个常用角色模板（frontend/backend/reviewer/devops/general），编译进二进制，首次运行时 seed
- 用户可自定义角色：填写名称/描述/标签 + 粘贴 markdown 模板内容
- `roles.json` 只存元数据（id, name, description, tags, cli_suggestion），不内嵌 markdown
- `.md` 模板文件存储在 `~/.config/zenix/roles/` 目录下
- 运行时按需读取文件内容注入 system prompt
- 支持角色到 Agent CLI 的映射建议（例如 frontend → opencode）
- 支持管理（添加/删除/编辑）

### R2: Worker 生命周期管理

- 编排器 dispatch agent 时，生成 worker_name 并注册
- worker_name 格式：`<cli缩写>_<随机标识>(<角色缩写>)`
- Worker 状态追踪：starting → running → done / error / cancelled
- 所有活跃 worker 列表在 `AiChatPanelData.agent_statuses` 中维护

### R3: `@` Worker 提及

- 输入 `@` 弹出活跃 worker 列表（含状态标签）
- 每个 worker 显示：worker_name、角色、当前状态
- 支持模糊筛选（`@oc` 匹配 `@oc_mark(frontend)`）
- 选中后插入 `@worker_name`
- 发送时消息不经过编排器 LLM，直接通过 herdr IPC `send_pane_text` 发送
- 执行结果以 agent block 卡片返回
- 键盘导航（↑↓ 选择、Tab/Enter 确认、Esc 关闭）

### R4: Placeholder 更新

- "Dispatch agents or type @ to mention a worker"

### R5: Workspace 选择与会话绑定

- 新会话时自动获取 herdr 当前选中的 workspace（cwd）
- 下拉菜单供用户切换 workspace
- 首次发送消息后，会话绑定该 workspace，之后不可更改
- 绑定后编排器 system prompt 注入当前 workspace 信息
- DrawerKind::Agents 抽屉显示按 workspace 分组的会话列表

### R6: 编排器提示词体系

- 详细的 system prompt，包含：
  - 角色定义（编排器 vs 执行器）
  - 已注册角色列表（让编排器知道谁做什么）
  - 可用工具列表及使用场景
  - Agent dispatch 的 JSON 格式说明
  - 工作流指南（规划 → 分配 → 检查 → 汇总）
  - 约束条件（不直接写代码、不运行命令等）
- 可选的用户自定义提示词注入

### R7: 编排链路完整闭环

- LLM 调用 → 工具/agent dispatch → 结果收集 → 汇总报告 的完整循环
- 支持并行 dispatch（多个 agent 同时执行）
- 上下文压缩（compactor.rs 已实现，需接入）
- 错误处理和重试机制

### R8: Drawer 会话管理

- `DrawerKind::Agents` 抽屉展示按 workspace 分组的会话列表
- 每个会话显示：workspace_name、cwd、最后消息预览、时间戳
- 点击切换到该会话

### R9: Agent CLI / 角色管理 UI

- 显示已检测到的 Agent CLI（通过 `detect_all_agents()`）
- 显示已注册的角色列表
- 一键安装 Agent CLI
- 配置默认 Agent CLI 和角色映射

---

## 验收标准

1. 首次运行时内置角色模板自动 seed 到 `roles/` 目录
2. 编排器 dispatch agent 时生成 worker_name 并注册
3. Workspace 下拉自动获取 herdr 当前 workspace，新会话可切换
4. 首次发送后 workspace 锁定，不可更改
5. 输入 `@` 弹出活跃 worker 列表，支持筛选和键盘导航
6. `@` 选中的消息直接发送到对应 worker 的 herdr pane
7. 编排器使用完整 system prompt（含角色列表和 workspace 信息）
8. Worker 状态实时更新，支持中止 worker 执行
9. DrawerKind::Agents 显示按 workspace 分组的会话
10. Placeholder 更新为正确描述
11. 设置面板"扩展"TAB 中有角色管理和 Agent CLI 管理

---

## 超出范围

- 不实现 `/` 命令弹出（已有 UI 入口替代）
- 不实现新的 LLM provider（复用现有 api_provider 系统）
- 不改变消息渲染系统（messages.rs 不做大改）
- 不重写 herdr IPC 协议
- 不引入新的外部依赖

---

## 已确认的设计决策

- `@` 弹出用 gpui 原生 popover（方案 A：deferred + absolute）
- 角色模板来源：内置模板编译进二进制 + 用户自定义（粘贴 markdown 内容）
- 存储：`roles.json` 存元数据，`.md` 文件存模板内容
- 角色管理界面放在设置面板的"扩展"TAB 中
- Worker 命名格式：`<cli缩写>_<LLM生成标识>(<角色缩写>)`
- Worker 标识由编排器 LLM 根据任务描述智能生成（如 `oc_login-fix(frontend)`）

## 遗留问题

- 编排器提示词需要多详细？是否允许用户编辑？（设计阶段确定）
- Agent 监督面板是放抽屉里还是直接嵌入聊天面板底部？（设计阶段确定）
