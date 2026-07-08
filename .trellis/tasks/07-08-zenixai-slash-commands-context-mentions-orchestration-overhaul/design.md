# Design: zenixAI Role/Worker System & Orchestration Overhaul

## 1. Architecture Overview

```
┌──────────────────────────────────────────────────────┐
│                  ZenixApp                            │
│  ┌──────────────────────────────────────────────┐    │
│  │  RoleRegistry (src/ai/roles.rs)              │    │
│  │  ├── roles: Vec<RoleDef>                      │    │
│  │  └── import_from_agency(path)                │    │
│  └──────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────┐    │
│  │  WorkerRegistry (src/ai/workers.rs)           │    │
│  │  ├── workers: Vec<WorkerInstance>             │    │
│  │  ├── register(name, cli, role, pane)         │    │
│  │  ├── send_to_worker(id, text)                │    │
│  │  └── abort_worker(id)                        │    │
│  └──────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────┐    │
│  │  AiChatPanelData (existing)                  │    │
│  │  ├── selected_workspace (cwd绑定)            │    │
│  │  ├── workspace_bound (发送后锁定)            │    │
│  │  ├── agent_statuses ← WorkerRegistry        │    │
│  │  └── @ mention popup ← WorkerRegistry       │    │
│  └──────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────┐    │
│  │  OrchestratorEngine (existing)               │    │
│  │  ├── SYSTEM_PROMPT ← includes roles          │    │
│  │  ├── dispatch → creates WorkerInstance       │    │
│  │  └── parse_dispatch_directive (enhanced)     │    │
│  └──────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────┐    │
│  │  DrawerKind::Agents → 会话列表(按workspace)  │    │
│  │  DrawerKind::History  → 全部历史会话          │    │
│  └──────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────┐    │
│  │  Settings → 扩展 TAB                        │    │
│  │  ├── 角色管理 (RoleManager)                  │    │
│  │  └── Agent CLI 管理                          │    │
│  └──────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────┘
```

## 2. Data Model

### RoleDef (structured config)

```rust
/// A registered role definition (stored in ~/.config/zenix/roles.json)
/// Prompt template stored as separate .md file to avoid JSON escaping issues
/// and maintain compatibility with OpenCode/Claude Code agents format.
struct RoleDef {
    id: String,                    // "frontend-developer"
    name: String,                  // "Frontend Developer"
    description: String,           // "专精 React/Vue 前端开发"
    cli_suggestion: Vec<String>,   // ["opencode", "claude"]
    tags: Vec<String>,             // ["frontend", "react", "ui"]
    source: String,                // "agency-agents" | "user"
    template_file: String,         // "frontend-developer.md" (relative to roles_dir)
}
```

### Roles storage layout

```
~/.config/zenix/roles.json                    ← 结构化元数据（无内嵌 markdown）
~/.config/zenix/roles/frontend-developer.md    ← 完整提示词模板
~/.config/zenix/roles/backend-architect.md
```

**Why**: 避免 JSON 中转义 markdown 的问题。`.md` 文件同时兼容 OpenCode/Claude Code agents 格式。

**roles.json** (元数据 only):
```json
{
  "roles": [
    {
      "id": "frontend-developer",
      "name": "Frontend Developer",
      "description": "专精 React/Vue 前端开发、UI 实现、性能优化",
      "cli_suggestion": ["opencode", "claude"],
      "tags": ["frontend", "react", "ui"],
      "source": "agency-agents",
      "template_file": "frontend-developer.md"
    }
  ]
}
```

**roles/frontend-developer.md** (模板文件，YAML frontmatter + body):
```markdown
---
name: Frontend Developer
description: 专精 React/Vue 前端开发、UI 实现、性能优化
tags: [frontend, react, ui]
---

你是一名资深前端开发工程师。你擅长 React/Vue/Angular 等现代前端框架...
```

### WorkerInstance (runtime state)

```rust
/// A running agent worker instance.
struct WorkerInstance {
    id: usize,                     // Unique worker ID
    worker_name: String,           // "oc_login-fix(frontend)"
    cli_name: String,              // "opencode"
    role_id: String,               // "frontend-developer"
    pane_id: String,               // herdr pane ID
    terminal_id: String,           // herdr terminal ID
    status: WorkerStatus,
    created_at: i64,
    output_history: String,
}

enum WorkerStatus {
    Starting,
    Running,
    Done,
    Error(String),
    Cancelled,
}
```

## 3. Component Design

### 3.1 RoleRegistry

**Location**: `src/ai/roles.rs` (new module)

**Responsibilities**:
- Load/save roles from `roles.json`
- Seed built-in templates to `roles/` on first run
- User-defined role CRUD (create from markdown, edit, delete)
- Provide role list to orchestrator for system prompt injection
- Provide role list to settings UI for management

**Role sources**:

1. **Built-in templates** — 3-5 个常用角色编译进二进制（`include_str!()`），首次运行时 seed 到 `roles/` 目录
2. **User-defined** — 用户在设置中创建，填写 name/description/tags + 粘贴 markdown 内容，保存为 `.md` 文件

**Reading template at runtime**:
```
When building system prompt:
  RoleRegistry.get_template(role_id)
    → read ~/.config/zenix/roles/{template_file}
    → return full .md content as prompt text
    → cache in memory after first read
```

### 3.2 WorkerRegistry

**Location**: `src/ai/workers.rs` (new module)

**Responsibilities**:
- Maintain list of active worker instances (started by orchestrator dispatch)
- Register new worker when orchestrator dispatches
- Update worker status via on_event callbacks
- Send text to worker pane (direct herdr IPC for @ mentions)
- Abort worker execution
- Provide worker list to @ popup
- Clean up completed workers (keep last N for history)

**Status update flow**:
```
DispatchStreamEvent::AgentOutput { agent_name, text }
  → find worker by pane_id → append to output_history

DispatchStreamEvent::AgentComplete(result)
  → update status = Done

@ mention send:
  send_pane_text(pane_id, text)
  → create agent block in chat
  → wait for pane output
```

### 3.3 Workspace Selection & Session Binding

**Location**: `src/ai/chat/header.rs` (enhance existing header)

**States**:

```
New session:
  → herdr IPC: workspace.list → populate workspaces dropdown
  → herdr IPC: workspace.get_current → auto-select active workspace
  → dropdown shows: workspace_name (cwd)

User switches workspace:
  → updates selected_workspace in AiChatPanelData

User sends first message:
  → workspace_bound = true
  → dropdown disabled / shows locked workspace
  → all subsequent dispatch uses this workspace/cwd

DrawerKind::Agents:
  → Shows zenixAI sessions organized by workspace
  → Each session entry: workspace_name + cwd + last_message_preview + timestamp
  → Click → switch to that session (if bound to different cwd, warn)
```

**Data fields on AiChatPanelData**:
```rust
pub selected_workspace: Option<WorkspaceOption>,  // Current workspace selection
pub workspace_bound: bool,                         // True after first message sent
pub workspaces: Vec<WorkspaceOption>,              // Available workspaces from herdr
```

**herdr IPC methods needed**:
- `workspace.list` — already supported in `HerdrIpcClient`
- `workspace.get_current` — get the currently active workspace in herdr (may need new method)

### 3.4 @ Mention Popup

**Location**: `src/ai/chat/mention_popup.rs` (new module)

**UI**:
- Overlay popover positioned above the input bar
- Uses `div().absolute().deferred()` (same pattern as model dropdown)
- Filters workers by fuzzy matching on worker_name + role + cli_name
- Keyboard: ↑↓ navigate, Enter/Tab select, Esc dismiss

**Data source**: Reads from `WorkerRegistry.active_workers`

**Selection action**:
1. Insert `@worker_name` text into input
2. On Send: check if input matches `@worker_name rest_of_message`
3. If yes → route to worker instead of LLM
4. If not → normal LLM chat (with workspace binding)

### 3.5 Orchestrator System Prompt

**Location**: `src/ai/engine.rs` (update `SYSTEM_PROMPT`)

**New structure** (generated at runtime):

```
你是 ZenixAI 编排器。你负责理解用户需求、规划任务分解、将具体执行工作分配给
专业 Agent CLI。

## 可用角色
{roles_from_registry}

## 可用工具
{tools_list}

## 当前工作空间
当前会话绑定到 workspace: {workspace_name} ({workspace_cwd})
所有 agent 在此工作空间下执行。

## Agent Dispatch 格式
当你需要分配任务给 Agent 时，在回复中包含以下 JSON：

```json
{
  "agents": [
    {
      "agent_name": "opencode",
      "role": "frontend-developer",
      "worker_name": "oc_login-fix(frontend)",
      "instruction": "修复 src/auth/login.rs 的 token 刷新逻辑",
      "cwd": "/home/user/project"
    }
  ]
}
```

worker_name 的标识部分请根据任务内容生成简短有意义的名字。

## 工作流
1. 分析用户需求
2. 规划任务分解
3. 选择角色和 Agent CLI
4. 分配任务（并行任务一次分配）
5. 收集执行结果
6. 汇总报告

## 约束
- 你不直接写代码或运行命令
- 你只负责规划、调度和结果汇总
- 用中文回复
```

### 3.6 Settings → 扩展 TAB

**Location**: `src/ui/settings_extensions.rs` (new module)

**Sections**:
1. **Agent CLI** — 检测到的 CLI 列表，显示安装状态，安装按钮
2. **角色管理** — 已注册角色列表，导入按钮（从 agency-agents 路径导入），编辑/删除

## 4. Data Flow

### 4.1 Workspace Selection Flow

```
Application start / New chat:
  → herdr IPC: workspace.list → show in dropdown
  → herdr IPC: workspace.get_current → auto-select
  → selected_workspace set, workspace_bound = false

User switches workspace in dropdown:
  → selected_workspace updated
  → available_models may change (different cwd env)
  → workspace_bound still false

User sends first message:
  → workspace_bound = true
  → dropdown disabled (visual lock)
  → Dispatch/chat uses selected_workspace.cwd as base

User clicks DrawerKind::Agents:
  → Shows sessions grouped by workspace
  → Click = switch to that session
```

### 4.2 Dispatch Flow (new)

```
User: "修复登录页前端bug"
  → Session bound to workspace "/home/user/project-a"

Orchestrator LLM:
  → System prompt includes roles + current workspace
  → LLM decides: assign to frontend-developer role using opencode
  → LLM generates: {"agents": [{"agent_name": "opencode", "role": "frontend-developer", "worker_name": "oc_fix-login(frontend)", ...}]}

Engine:
  → parse_dispatch_directive() extracts JSON
  → For each agent:
    1. herdr_client.start_agent("opencode", workspace_id, cwd)
    2. WorkerRegistry.register("oc_fix-login(frontend)", "opencode", "frontend-developer", pane_id)
    3. herdr_client.send_agent_text(terminal_id, instruction)
    4. Monitor output → WorkerRegistry.update_status(id, Running)
    5. Wait for [AGENT_DONE] → WorkerRegistry.update_status(id, Done)
    6. Create AgentBlock in chat
```

### 4.3 @ Mention Flow

```
User types "@oc_fix" → popup shows matching workers
User selects "oc_fix-login(frontend)" → inserts @worker_name
User types rest → sends
  → Engine detects @worker_name prefix (at start of input)
  → Routes directly to WorkerRegistry.send_to_worker(id, message)
  → Creates agent block
  → Monitors output via herdr IPC read_pane
```

## 5. File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `src/ai/roles.rs` | NEW | RoleRegistry, RoleDef, import logic |
| `src/ai/workers.rs` | NEW | WorkerRegistry, WorkerInstance, status mgmt |
| `src/ai/chat/mention_popup.rs` | NEW | @ popup UI |
| `src/ai/engine.rs` | MODIFY | System prompt generation, worker registration |
| `src/ai/chat/state.rs` | MODIFY | Add workspace_bound, wire WorkerRegistry |
| `src/ai/chat/input.rs` | MODIFY | @ detection, placeholder, workspace binding |
| `src/ai/chat/header.rs` | MODIFY | Workspace dropdown, auto-detect, lock after send |
| `src/ai/chat/mod.rs` | MODIFY | Integrate @ popup, workspace header |
| `src/ai/chat/drawer.rs` | MODIFY | DrawerKind::Agents → sessions-per-workspace listing |
| `src/ai/herdr.rs` | MODIFY | Add workspace.get_current method if missing |
| `src/ui/settings.rs` | MODIFY | Add 扩展 TAB |
| `src/ui/settings_extensions.rs` | NEW | Role/CLI management UI |

## 6. Trade-offs

1. **RoleRegistry 进程内 vs 持久化**: 角色存在 `roles.json`，只在启动时加载。修改后需重新加载。不实时同步。
2. **Worker 状态同步**: on_event 回调在 engine 的 tokio 线程中，通过 `cx.spawn` + `entity.update` 更新到 GPUI 主线程。存在短暂延迟。
3. **导入格式兼容性**: agency-agents `.md` 的 frontmatter 格式不统一——需要宽容解析（缺失字段用默认值）。
4. **@ 消息路由**: 检测 `@worker_name` 在输入开头有效。不支持消息中间 `@` 路由。
5. **Workspace 锁定**: 首次发送后锁定不可逆。如需切换 workspace → 新建会话。
6. **herdr workspace 探测**: `workspace.get_current` 可能 herdr IPC 未实现，fallback 到第一个 workspace。

## 7. Dependencies Between Deliverables

```
Phase A (Foundation): R1 + R2 (no deps between them)
Phase B: R3 (@ mention) ← R2 (WorkerRegistry)
         R5 (workspace) ← herdr IPC workspace.list
Phase C: R4 (system prompt) ← R1 + R5
         R7 (full loop) ← R2 + R4
Phase D: R6 (management UI) ← R1
```

Ordered implementation:
1. workspace: herdr IPC + header dropdown + session binding (R5)
2. roles.rs + workers.rs: data models + registry (R1, R2)
3. @ popup + input routing (R3)
4. system prompt generation (R4)
5. full loop integration + drawer session listing (R7 + drawer)
6. settings → 扩展 TAB (R6)
