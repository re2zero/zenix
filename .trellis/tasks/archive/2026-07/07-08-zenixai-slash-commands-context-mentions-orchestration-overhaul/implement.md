# Implement: zenixAI Role/Worker & Orchestration Overhaul

## Priority Order

```
Phase A: Foundation (R1, R2, R5 workspace detection)
  → roles.rs + workers.rs + roles.json
  → herdr workspace.list + workspace.get_current

Phase B: Workspace + @ Mention (R5 dropdown, R3, R4, R8 drawer)
  → Workspace dropdown + session binding in header
  → @ mention popup + input routing
  → DrawerKind::Agents session-per-workspace listing

Phase C: Prompt + Full Loop (R6, R7)
  → Dynamic system prompt generation
  → Compaction integration + error handling

Phase D: Management UI (R9)
  → Settings → 扩展 TAB
  → Role + Agent CLI management
```

## Phase A Checklist

### A1: RoleRegistry (`src/ai/roles.rs`)
- [ ] `RoleDef` struct with serde Serialize/Deserialize
- [ ] `RoleRegistry` with `load()` / `save()` to `~/.config/zenix/roles.json`
- [ ] Built-in templates: 3-5 个角色作为 const strings 编译进二进制
- [ ] `seed_builtin_templates()` — 首次运行时写入 `roles/` 目录
- [ ] `create_role(name, description, tags, cli_suggestion, markdown_content)` — 用户自定义
- [ ] `delete_role(id)`, `list_roles()`, `get_role(id)`, `get_template(id)`
- [ ] Unit tests

### A2: WorkerRegistry (`src/ai/workers.rs`)
- [ ] `WorkerInstance` struct, `WorkerStatus` enum, serde
- [ ] `WorkerRegistry` with `register()`, `update_status()`, `list_workers()`, `get_worker()`, `get_by_pane()`
- [ ] `send_to_worker()` — herdr IPC `send_pane_text`
- [ ] `abort_worker()` — herdr IPC to cancel
- [ ] Auto-cleanup: keep last 20 completed workers
- [ ] Thread-safe: `Arc<Mutex<WorkerRegistry>>`
- [ ] Unit tests

### A3: Workspace detection (`src/ai/herdr.rs`)
- [ ] Verify `workspace.list` method exists (check existing types)
- [ ] Add `workspace.get_current` IPC method if missing
- [ ] Verify `HerdrIpcClient` supports workspace target types

## Phase B Checklist

### B1: Workspace dropdown (`src/ai/chat/header.rs`)
- [ ] Fetch workspace list from herdr on chat open
- [ ] Auto-select current workspace from herdr
- [ ] Dropdown UI in header (reuse model dropdown pattern)
- [ ] `AiChatPanelData.workspace_bound` — `false` initially
- [ ] On first Send: `workspace_bound = true`, disable dropdown
- [ ] Visual locked state (disabled + indicator)
- [ ] Pass workspace context to engine for dispatch

### B2: @ Mention Popup (`src/ai/chat/mention_popup.rs`)
- [ ] Popup UI: deferred + absolute above input bar
- [ ] Read worker list from `WorkerRegistry`
- [ ] Fuzzy filter on worker_name + role + cli_name
- [ ] Keyboard: ↑↓ navigate, Enter/Tab select, Esc dismiss
- [ ] On select: insert `@worker_name` into input

### B3: Input routing (`src/ai/chat/input.rs`, `src/ai/chat/mod.rs`)
- [ ] Detect `@` character → show popup
- [ ] On Send: check `@worker_name` prefix in input
- [ ] Route via `WorkerRegistry.send_to_worker()`
- [ ] Create agent_block for output
- [ ] Fallback: normal LLM chat with workspace context
- [ ] Update placeholder text

### B4: Drawer session listing (`src/ai/chat/drawer.rs`)
- [ ] Implement `DrawerKind::Agents` rendering
- [ ] Group sessions by workspace name
- [ ] Each entry: workspace_name + cwd + last_msg_preview + timestamp
- [ ] Click to switch session

## Phase C Checklist

### C1: Dynamic system prompt (`src/ai/engine.rs`)
- [ ] `build_system_prompt(roles, workspace_info, tools)` method
- [ ] Inject registered roles list
- [ ] Inject current workspace info (name + cwd)
- [ ] Inject tool list with descriptions
- [ ] Dispatch JSON format documentation
- [ ] Update `chat_and_dispatch()` to use dynamic prompt

### C2: Full loop improvements (`src/ai/engine.rs`)
- [ ] Parallel dispatch — execute multiple agents concurrently
- [ ] Compaction integration in `chat_and_dispatch` loop
- [ ] Error classification improvements
- [ ] Retry logic for transient failures

## Phase D Checklist

### D1: Settings → 扩展 TAB (`src/ui/settings_extensions.rs`)
- [ ] New TAB "扩展" in settings panel
- [ ] Agent CLI section: `detect_all_agents()` results + install button
- [ ] Role list section: registered roles + import button + delete

## Validation Commands

```bash
cargo check 2>&1 | head -50
cargo test -p zenix --lib ai::roles 2>&1 | tail -20
cargo test -p zenix --lib ai::workers 2>&1 | tail -20
cargo test -p zenix --lib ai::chat 2>&1 | tail -20
cargo test 2>&1 | tail -30
```

## Rollback Points

- Phase A: `git stash` — role/worker model is self-contained
- Phase B: `git checkout -- src/ai/chat/mention_popup.rs`
- Phase C: `git checkout -- src/ai/engine.rs`
- Phase D: `git checkout -- src/ui/settings_extensions.rs`

## Risky Files

- `src/ai/engine.rs` (1976 lines) — complex dispatch flow
- `src/ai/chat/input.rs` — event handling affects all input
- `src/ai/chat/header.rs` — workspace dropdown must not break header
- `src/ai/herdr.rs` — workspace.get_current may need IPC extension
