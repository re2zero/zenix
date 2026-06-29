# Settings UI Refactor & Agent Management

**Created:** 2026-06-29
**Author:** re2zero
**Priority:** P0

---

## Problem Statement

The settings dialog (`src/ui/settings.rs`, 3281 lines) and its supporting modules
have grown organically without a clear architectural boundary. The current state
has several critical issues:

1. **Empty-shell agents clutter the registry**: `copilot`, `kimi`, `cursor`,
   `windsurf` are registered in `ProviderRegistry` but have no MCP config file
   path, no skills directory, and no actual config format support. They appear in
   the Agent CLI tab as undetectable placeholders with no useful management.

2. **No per-agent config format adaptation**: `sync_api_to_agent()` only handles
   three cases — Claude (writes `ANTHROPIC_*` env vars), OpenCode (writes provider
   entry), and a generic fallback. Codex, Droid, Pi, OMP, Kilo, and Hermes each
   have different config file formats and env var conventions.

3. **Dual MCP data sources cause confusion**: Zenix manages its own MCP servers
   (`~/.config/zenix/mcp.json` in NDJSON format) separately from per-agent
   configs. The sync function (`sync_zenix_server_to_agent`) is a one-way bridge
   that doesn't handle conflicts or removal properly.

4. **Skill management is format-locked**: Only Claude-format skills (directory with
   `SKILL.md`) are supported. No adaptation for agents with different skill systems.

5. **God-object Props struct**: `SettingsDialogProps` has ~50 fields, mixing UI
   state, business data, callbacks, and form state — all coupled together.

---

## Goals

1. **Clean up the provider registry**: Remove agents that have no actual config
   management capability (copilot, kimi, cursor, windsurf).
2. **Design a per-agent config adapter**: Each supported agent gets a concrete
   adapter implementing a unified `AgentConfigAdapter` trait.
3. **Unify MCP management**: Single view across Zenix-managed and per-agent MCP
   servers, with proper sync/unsync semantics.
4. **Extend skill management**: Support multiple skill formats per agent.
5. **Extend API provider generation**: Every supported agent gets proper config
   generation for API keys, models, and thinking levels.
6. **Restructure settings module**: Split the monolithic file into logical
   modules with proper state management.

---

## Non-Goals

- Adding support for new agent CLIs (only clean up existing ones)
- Changing the GPUI rendering framework or component library
- Modifying the terminal emulator or PTY backend
- Adding new features beyond the scope of settings management

---

## Agent Registry Cleanup

### Remove (4 agents)

| Agent | Reason |
|-------|--------|
| `copilot` | GitHub Copilot CLI has no local config file, uses GitHub OAuth |
| `kimi` | Kimi CLI has no documented local config path |
| `cursor` | Cursor is a GUI app, MCP config is in app data (not cross-platform) |
| `windsurf` | Windsurf is a GUI app, MCP config is in app data (not cross-platform) |

### Keep & Enhance (8 agents)

| Agent | Config Dir | MCP File | Skills Dir | Adapter Needed |
|-------|-----------|----------|-----------|---------------|
| `claude` | `~/.claude` | `claude_desktop_config.json` | `skills/` | ✅ Env var injection |
| `codex` | `~/.codex` | `config.json` | `skills/` | ✅ Codex config format |
| `opencode` | `~/.opencode` | `config.json` | `skills/` | ✅ Already partially done |
| `droid` | `~/.droid` | `config.json` | `skills/` | ⚠️ Anthropic format, verify |
| `pi` | `~/.pi` | `config.json` | `skills/` | ⚠️ Anthropic format, verify |
| `omp` | `~/.omp` | `config.json` | `skills/` | ⚠️ Anthropic format, verify |
| `kilo` | `~/.kilo` | `config.json` | `skills/` | ⚠️ Anthropic format, verify |
| `hermes` | `~/.hermes` | `config.json` | `skills/` | ⚠️ Anthropic format, verify |

---

## Architecture: AgentConfigAdapter Trait

```rust
/// Per-agent configuration adapter.
/// Each supported agent implements this trait to declare how Zenix
/// should manage its config files.
pub trait AgentConfigAdapter: Send + Sync {
    /// Short machine name (e.g. "claude").
    fn name(&self) -> &str;
    /// Human-readable display name.
    fn display_name(&self) -> &str;
    /// Binary names to check on PATH.
    fn binary_names(&self) -> &[&str];
    /// Install command.
    fn install_command(&self) -> &str;
    /// All config paths this adapter manages.
    fn config_paths(&self) -> Vec<ConfigPathSpec>;
    /// Generate env vars or config entries for an API provider.
    fn sync_api_provider(&self, provider: &ApiProviderConfig) -> Result<SyncResult, String>;
    /// Remove API provider sync.
    fn unsync_api_provider(&self, provider_name: &str) -> Result<(), String>;
    /// Sync an MCP server entry.
    fn sync_mcp_server(&self, server: &McpServerEntry) -> Result<SyncResult, String>;
    /// Sync a skill (create symlink or copy).
    fn sync_skill(&self, skill_name: &str) -> Result<SyncResult, String>;
    /// Unsync a skill (remove symlink).
    fn unsync_skill(&self, skill_name: &str) -> Result<(), String>;
}

/// What config paths an adapter manages.
pub enum ConfigPathSpec {
    /// Claude-format JSON MCP config.
    McpConfig(PathBuf),
    /// Skills directory.
    SkillsDir(PathBuf),
    /// API provider config file (format varies by agent).
    ApiConfig(PathBuf),
    /// Generic config directory (for agents with custom formats).
    ConfigDir(PathBuf),
}

/// Result of a sync operation.
pub enum SyncResult {
    /// Config file was created or updated.
    Updated(PathBuf),
    /// Config file did not exist; nothing to do.
    NotFound(PathBuf),
    /// No config path applicable for this operation.
    NotApplicable,
}
```

### Adapter Implementations

1. **ClaudeAdapter** — Writes `ANTHROPIC_*` env vars into `claude_desktop_config.json`
2. **CodexAdapter** — Writes to Codex's config format (YAML/JSON with OpenAI-compatible keys)
3. **OpenCodeAdapter** — Writes npm provider entries into `config.json`
4. **AnthropicFamilyAdapter** — Shared adapter for Droid, Pi, OMP, Kilo, Hermes
   (all use `~/<agent>/config.json` with similar env var patterns)

---

## MCP Management Refactor

### Current State

- **Zenix NDJSON** (`~/.config/zenix/mcp.json`): Zenix's own MCP server list
- **Per-agent configs**: Each agent has its own `mcpServers` in its config JSON
- **Sync bridge**: `sync_zenix_server_to_agent()` writes Zenix servers into per-agent configs

### Target State

1. **Single unified view**: Present all MCP servers in one place, tagged by source
2. **Clear ownership**: Zenix-managed servers are the source of truth; per-agent configs reflect this
3. **Proper unsync**: Removing a Zenix server should also remove it from all per-agent configs
4. **Deduplication**: Unified view deduplicates servers appearing across agents

---

## Skill Management Extension

### Current

- Only supports Claude-format (directory with `SKILL.md`)
- `best_source()` hardcodes priority: claude > codex > pi > omp > kilo > hermes

### Target

- Still primarily Claude-format (most agents use it)
- Add validation: verify `SKILL.md` exists before linking
- Add per-agent skill directory path resolution via `AgentConfigAdapter`
- Keep `best_source()` but make it configurable per adapter

---

## API Provider Generation Extension

### Current

Only 3 cases in `sync_api_to_agent()`:
- `claude` → ANTHROPIC_* env vars
- `opencode` → npm provider entry
- `_` (fallback) → OPENAI_API_KEY env vars

### Target

Each adapter generates the correct config:

| Agent | API Config Format |
|-------|------------------|
| claude | `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_MODEL` in config JSON |
| codex | OpenAI-compatible env vars (`OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`) |
| opencode | npm provider entry in config JSON |
| droid/pi/omp/kilo/hermes | Agent-specific env vars (verify actual format) |

For agents without API config support: skip silently or show "not supported" in UI.

---

## Settings Module Restructure

### Current

Single file: `src/ui/settings.rs` (3281 lines)
All state via `SettingsDialogProps` (~50 fields)

### Target

```
src/settings/
├── mod.rs           # Module entry, public API
├── state.rs         # AppState enum (tab selection, search, form, etc.)
├── actions.rs       # Action enum (user-triggered events)
├── tabs/
│   ├── general.rs       # Language, about
│   ├── appearance.rs    # Theme, font
│   ├── terminal.rs      # Cursor, opacity, scrollback
│   ├── agent.rs         # Agent CLI detection & management
│   └── extensions.rs    # MCP, API providers, Skills sub-tabs
├── components/
│   ├── mcp_card.rs
│   ├── api_card.rs
│   ├── skill_card.rs
│   ├── agent_list.rs
│   └── search_bar.rs
└── adapters/
    ├── mod.rs           # AgentConfigAdapter trait
    ├── claude.rs
    ├── codex.rs
    ├── opencode.rs
    └── anthropic_family.rs
```

---

## Migration Plan

### Phase 1: Cleanup & Design
- [ ] Remove copilot, kimi, cursor, windsurf from `builtin.rs`
- [ ] Design and implement `AgentConfigAdapter` trait
- [ ] Implement `ClaudeAdapter`
- [ ] Implement `OpenCodeAdapter` (refactor existing code)
- [ ] Implement `AnthropicFamilyAdapter` for droid/pi/omp/kilo/hermes

### Phase 2: Backend Logic
- [ ] Rewrite `sync_api_to_agent()` to use adapters
- [ ] Unify MCP data sources
- [ ] Extend skill management to use adapter for paths
- [ ] Add adapter-based config path resolution

### Phase 3: UI Restructure
- [ ] Create `settings/` module with state machine
- [ ] Split `settings.rs` into tab modules
- [ ] Create component modules
- [ ] Update `app.rs` to use new settings API
- [ ] Remove old `SettingsDialogProps` pattern

### Phase 4: Verification
- [ ] `cargo check` and `cargo clippy`
- [ ] Verify all 8 remaining agents appear correctly
- [ ] Test MCP sync/unsync round-trip
- [ ] Test API provider sync for each agent type
- [ ] Test skill link/unlink

---

## Acceptance Criteria

1. ✅ `copilot`, `kimi`, `cursor`, `windsurf` removed from provider registry
2. ✅ All 8 remaining agents have working config adapters
3. ✅ API provider sync works correctly for each supported agent
4. ✅ MCP servers visible in unified view across all agents
5. ✅ Skill linking works for all supported agents
6. ✅ No `unwrap()` in production code
7. ✅ `cargo check` passes with no warnings
8. ✅ Settings UI functionally equivalent to current (no regressions)
