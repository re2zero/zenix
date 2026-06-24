# src/ — Main GPUI Application

## OVERVIEW
Core GPUI application for zenix desktop frontend. All UI, state, IPC, and system integration.

## STRUCTURE
```
src/
├── main.rs            # Entry point
├── lib.rs             # Public SDK for plugins/sub-apps
├── app.rs             # ZenixApp — state, events, render
├── config.rs          # Config store (~/.config/zenix/config.json)
├── sys.rs             # System info collector (CPU, mem, net, disk)
├── client/mod.rs      # Herdr binary manager (install, start, socket IPC)
├── terminal/          # PTY backend + GPUI rendering
├── ui/                # Sidebar, settings, system info panels
├── provider/          # Theme/asset providers
├── api_provider/      # REST API for external integrations
├── mcp/               # Model Context Protocol integration
├── agent/             # Agent detection/integration
├── skills/            # Skill system for extensibility
└── platform.rs        # Platform-specific code (Linux/macOS/Windows)
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| App entry point | main.rs | GPUI application run |
| Plugin API surface | lib.rs | Public exports only |
| App state/events | app.rs | ZenixApp entity |
| Config parsing | config/mod.rs | JSONC via toml crate |
| Herdr lifecycle | client/mod.rs | Unix socket IPC |
| Terminal rendering | terminal/element.rs | alacritty_terminal + GPUI |
| Theme tokens | provider/builtin.rs | 4 built-in themes |
| System stats | sys.rs | sysinfo crate |
| Platform code | platform.rs | Linux/macOS/Windows gated |

## CONVENTIONS
- **GPUI Entity pattern**: State in Entity, actions via Context, render is pure
- **No state mutation in render()** — use actions or compute_view()
- **Async via cx.spawn()** — never block on GPUI event loop
- **FocusHandle** for keyboard/input focus management
- **Config = JSONC** parsed via toml crate (supports comments)

## ANTI-PATTERNS
- ❌ Direct cargo run without HERDR_BUILD or system herdr
- ❌ unwrap() in production paths
- ❌ Platform #[cfg] outside platform.rs
- ❌ Hardcoded colors bypassing theme provider