# Directory Structure

> How the Rust codebase is organized in this project.

---

## Directory Layout

```
src/
├── main.rs                    # Entry point — GPUI app init, icon/theme/font loading
├── lib.rs                     # Public SDK — re-exports for plugins/sub-apps
├── app.rs                     # ZenixApp — state, events, render (core entity)
├── platform.rs                # Platform-specific code (#[cfg] for Linux/macOS/Windows)
├── single_instance.rs         # Single-process enforcement via lock file
├── sys.rs                     # System info (CPU/mem/net/disk via sysinfo crate)
├── i18n.rs                    # Locale setup (rust_i18n::i18n!("locales"))
├── agent/                     # Agent detection & integration
│   └── mod.rs
├── api_provider/              # REST API provider management
│   └── mod.rs
├── client/                    # Herdr binary manager + Unix socket IPC
│   └── mod.rs
├── config/                    # Config store + JSONC parsing
│   ├── mod.rs                 # ConfigStore (load/save/default values)
│   ├── jsonc.rs               # JSON-with-comments parser
│   └── scanner.rs             # ConfigMap for field-preserving JSON read/write
├── mcp/                       # Model Context Protocol integration
│   └── mod.rs
├── provider/                  # Theme/asset providers
│   ├── mod.rs                 # ProviderRegistry (AI agent CLI tool registry)
│   └── builtin.rs             # Built-in provider definitions
├── skills/                    # Skill system for extensibility
│   └── mod.rs
├── terminal/                  # PTY backend + GPUI terminal rendering
│   ├── mod.rs                 # BackendCommand, BackendEvent, BackendTx
│   ├── element.rs             # TerminalElement (GPUI element impl)
│   └── encoding.rs            # Key/mouse event encoding for PTY
├── ui/                        # Sidebar, settings, system info panels
│   ├── mod.rs                 # Panel types, SIDEBAR_WIDTH, PANEL_WIDTH
│   ├── sidebar.rs
│   └── settings.rs            # Agent/skill/MCP/API provider settings UI
```

## Module Organization Rules

### When to use a single file vs. a module directory

| Criterion | Single file (e.g., `sys.rs`) | Module dir (e.g., `terminal/`) |
|-----------|------------------------------|--------------------------------|
| Lines of code | < ~300 | > ~300 |
| Number of related types | 1-2 | 3+ |
| Needs sub-modules | No | Yes (element, encoding, etc.) |

### Module declaration order in `main.rs` / `lib.rs`

- `main.rs`: fully alphabetical
- `lib.rs`: grouped — SDK/public modules first (agent, api_provider, i18n, mcp, platform, provider, skills), then binary-core modules (app, client, config, single_instance, sys, terminal, ui). Each group is internally alphabetical.

### Module visibility

- `main.rs`: all modules are `mod` (private to the binary)
- `lib.rs`: modules are `pub mod` (public SDK for plugins); only intentionally public APIs are re-exported at the crate root

### Platform-gated code

All `#[cfg(any(target_os = "linux", target_os = "freebsd"))]` etc. blocks go in `src/platform.rs`. Do **not** scatter platform checks across multiple files. Core code uses the platform module's public functions.

## Naming Conventions

- **Files**: snake_case (e.g., `config_store.rs`, not `ConfigStore.rs`)
- **Modules**: snake_case directories (e.g., `terminal/`, not `Terminal/`)
- **Types**: PascalCase (e.g., `ZenixApp`, `BackendTx`)
- **Functions**: snake_case (e.g., `load_config()`)
- **Constants**: SCREAMING_SNAKE_CASE (e.g., `EMBEDDED_THEME_JSONS`)
- **GPUI Actions**: PascalCase enums implementing the `Action` trait (e.g., `TerminalAction`, `McpAction`)

## Example: Adding a New Feature Module

1. Create `src/my_feature/mod.rs` (or `src/my_feature/` directory if >1 file)
2. Add `mod my_feature;` in `main.rs` (binary) or `lib.rs` (SDK) in alphabetical order
3. If the module has a public API, add `pub mod` and re-export key types in `lib.rs`
4. If it needs platform gating, use `crate::platform::` functions — don't add `#[cfg]` directly
