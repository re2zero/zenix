# PROJECT KNOWLEDGE BASE

**Generated:** 2026-06-23
**Commit:** a1b2c3d
**Branch:** main

## OVERVIEW
zenix is a cross-platform GPUI desktop frontend for herdr terminal workspace manager. It provides workspace/tab/pane management, system monitoring, theme engine, and plugin architecture via Rust + GPUI.

## STRUCTURE
```
zenix/
├── src/                    # Main GPUI application
│   ├── main.rs            # Entry point
│   ├── lib.rs             # Public SDK for plugins/sub-apps
│   ├── app.rs             # ZenixApp — state, events, render
│   ├── config.rs          # Config store (~/.config/zenix/config.json)
│   ├── sys.rs             # System info collector (CPU, mem, net, disk)
│   ├── client/mod.rs      # Herdr binary manager (install, start, socket IPC)
│   ├── terminal/          # PTY backend + GPUI rendering
│   ├── ui/                # Sidebar, settings, system info panels
│   ├── provider/          # Theme/asset providers
│   ├── api_provider/      # REST API for external integrations
│   ├── mcp/               # Model Context Protocol integration
│   ├── agent/             # Agent detection/integration
│   ├── skills/            # Skill system for extensibility
│   └── platform.rs        # Platform-specific code (Linux/macOS/Windows)
├── herdr/                 # Git submodule (ogulcancelik/herdr v0.6.10)
├── vendor/
│   ├── gpui_wgpu_patched/ # Patched wgpu renderer for GPUI
│   └── libghostty-vt/     # Vendored terminal parser (Zig)
├── assets/
│   ├── fonts/             # Lilex font family
│   └── themes/            # Theme JSON (gruvbox, solarized, tokyonight, matrix)
├── build.rs               # Compiles/embeds herdr from submodule
├── Cargo.toml             # Workspace manifest
└── justfile               # Task runner (if present)
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| App lifecycle, state, events | src/app.rs | ZenixApp core |
| Plugin/sub-app SDK | src/lib.rs | Public API surface |
| Herdr IPC & lifecycle | src/client/mod.rs | Unix socket, binary bootstrap |
| Terminal rendering | src/terminal/element.rs | GPUI element + alacritty_terminal |
| Theme system | src/provider/builtin.rs, assets/themes/ | 4 built-in themes |
| System monitoring | src/sys.rs | sysinfo crate, real-time stats |
| Config schema | src/config/mod.rs | JSONC with comments |
| I18n (CN/EN) | src/i18n.rs | rust-i18n macro |
| Build/packaging | Cargo.toml, build.rs | deb packaging, herdr embedding |

## CONVENTIONS
- **Rust 2024 edition**, GPL-3.0-or-later
- **GPUI patterns**: Entity/Context/Action model, `cx.spawn()` for async, `FocusHandle` for focus
- **No `unwrap()` in production** — use `anyhow::Context` or `thiserror`
- **Platform-gated code** in `src/platform.rs` only; core uses `#[cfg(...)]` on imports/fns
- **Config uses JSONC** (JSON with comments) via `toml` crate for parsing
- **Herdr binary**: auto-seeds to `~/.local/bin/herdr` on first run; herdr self-updates thereafter
- **Terminal PATH isolation**: PTY sessions prepend `~/.local/bin` to PATH

## ANTI-PATTERNS (THIS PROJECT)
- ❌ Direct `cargo run` without checking herdr binary — use `just` or ensure `HERDR_BUILD=1`
- ❌ Editing vendored `vendor/gpui_wgpu_patched/` without updating patch index
- ❌ Adding deps without checking `gpui`, `gpui-component`, `alacritty_terminal` first
- ❌ Mutating state in GPUI `render()` — use `compute_view()` or actions
- ❌ Hardcoding theme colors — use theme provider tokens

## COMMANDS
```bash
# Development
cargo run                    # Uses system herdr (fast)
HERDR_BUILD=1 cargo run      # Force build herdr from submodule

# Build
cargo build --release        # Full release (compiles herdr from submodule)
cargo deb                    # Build .deb package

# Testing/Quality
cargo check                  # Type check
cargo fmt --check            # Format check
cargo clippy                 # Lint
```

## NOTES
- **Herdr submodule** at `herdr/` is pinned to v0.6.10 tag; update via `git submodule update --remote`
- **GPUI from zed-industries/zed** git dep — breaks occasionally; pin rev in Cargo.lock if needed
- **wgpu patch** in `vendor/gpui_wgpu_patched/` fixes subpixel rendering; tracked via `[patch]` in Cargo.toml
- **I18n**: Chinese/English via `rust-i18n` macro; add keys in `src/i18n.rs` and `.po` files
- **Desktop entry** at `res/zenix.desktop`, icon at `res/zenix.svg` → PNG at build
- **License**: GPL-3.0-or-later (zenix), AGPL-3.0 (herdr), Apache-2.0 (gpui-component, alacritty_terminal)