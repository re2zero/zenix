# src/ — Main GPUI Application

## OVERVIEW
Core GPUI application for zenix desktop frontend. All UI, state, IPC, and system integration.

The UI is being rebuilt on **gpui-kit 0.7.3**: M1 deleted the old GPUI/PTY UI, M3 rebuilt native pane rendering, and M4 built the workspace shell (`src/shell/`) on top of it. See `../.relay/PLAN.md` (local, gitignored) for the milestone plan.

## STRUCTURE
```
src/
├── main.rs            # Entry point: fonts, keymap, window, ShellView
├── lib.rs             # Public SDK for plugins/sub-apps
├── bin/zenix-probe.rs # Daemon endpoint smoke gate (Welcome+Snapshot+Surface)
├── util.rs            # Shared helpers (timestamp_now)
├── shell/             # Workspace shell (M4)
│   ├── mod.rs         # ShellView: title bar + sidebar + tab strip + grid + status bar
│   ├── commands.rs    # Command directory and the daemon method each becomes
│   ├── keymap.rs      # Prefix keymap (ctrl-b chords) + RunCommand action
│   ├── registry.rs    # Contribution registry: panels and status segments
│   └── panels.rs      # Built-in contributions (workspace panel, status segments)
├── terminal/          # Cell painting + semantic input (M3)
│   ├── mod.rs         # Grid geometry, hit testing, key/mouse mapping
│   ├── painter/       # Cell-grid painter (glyphs, images, graphics, regions parts)
│   ├── regions.rs     # Per-pane cached paint regions
│   ├── splits.rs      # Split hit testing and ratios
│   ├── theme.rs       # Terminal palettes (packed RGB)
│   └── view.rs        # TerminalView: paints one daemon surface, owns the connection
├── herdr/             # Endpoint facade: Connection (connect/requests) + LiveState
├── config/            # Config store (~/.config/zenix/config.json) + jsonc.rs/scanner.rs
├── sys.rs             # System info collector (CPU, mem, net, disk)
├── client/mod.rs      # Herdr binary manager (install, start, endpoint socket)
├── provider/          # Theme/asset providers
├── api_provider/      # REST API for external integrations
├── mcp/               # Model Context Protocol integration
├── agent/             # Agent detection/integration
├── skills/            # Skill system for extensibility
├── config_adapters/   # Per-agent config adapters
├── i18n.rs            # rust-i18n wiring (strings in locales/zenix.yml)
└── platform.rs        # Platform-specific code (Linux/macOS/Windows)
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| App entry point | main.rs | gpui-kit application run |
| Workspace shell | shell/mod.rs | ShellView: title bar, sidebar, tab strip, status bar |
| Commands & keymap | shell/commands.rs, shell/keymap.rs | One directory; chords and the palette read it |
| UI contributions | shell/registry.rs, shell/panels.rs | Sidebar panels and status segments mount through the registry |
| Plugin API surface | lib.rs | Public exports only |
| Daemon smoke gate | bin/zenix-probe.rs | `cargo run --bin zenix-probe` |
| Endpoint facade | herdr/connection.rs, herdr/state.rs | Commands out, snapshots in |
| Terminal painting | terminal/view.rs, terminal/painter/ | One daemon surface painted natively |
| Config parsing | config/mod.rs | JSONC via toml crate |
| Herdr lifecycle | client/mod.rs | Endpoint socket via `herdr_client::ConnectTarget` |
| Endpoint protocol/client | ../vendor/herdr-protocol, ../vendor/herdr-client | Copy-only vendored crates |
| Theme tokens | provider/builtin.rs | 4 built-in themes |
| System stats | sys.rs | sysinfo crate |
| Platform code | platform.rs | Linux/macOS/Windows gated |
| I18n keys | i18n.rs, ../locales/zenix.yml | rust-i18n `t("key")` |

## CONVENTIONS
- **UI depends on `gpui-kit` only**: `use gpui_kit::*`; never add git `gpui`/`gpui-component`/`gpui_platform` deps
- **GPUI Entity pattern**: State in Entity, actions via Context
- **No state mutation in render()**, with one documented exception: `TerminalView::render`
  calls `Regions::views` (`terminal/view.rs`), which creates and updates entities. GPUI only
  allows creating entities while a frame is being built, and gpui-kit 0.7.3 exposes no
  `compute_view`, so the frame is the only place that call can run. Any new call of that shape
  must be recorded here before it lands.
- **Async via cx.spawn()** — never block on GPUI event loop
- **FocusHandle** for keyboard/input focus management
- **Config = JSONC** parsed via toml crate (supports comments)
- **Endpoint is the state truth**: workspace/tab/pane/agent state comes from `ClientEvent::Snapshot` pushes — no polling
- **Shell surface mounts through the registry** (`shell/registry.rs`): sidebar panels and status
  segments are contributions, and the shell does not name its own parts directly
- **One cell metric**: the advance width the painter lays cells out with is the one every hit
  test, region view and IME cursor must use. `TerminalView` keeps the measured value (`cell`)
  and it is refreshed inside the canvas prepaint, because the painter's metrics are only
  available while a frame is built; a stale copy silently shifts every mouse hit and every
  drawn border by a growing offset.
- **No local PTY**: daemon panes are the only terminal source

## ANTI-PATTERNS
- ❌ Direct cargo run without the installed herdr or `HERDR_BUILD=1`
- ❌ unwrap() in production paths
- ❌ Mutating state in render() anywhere other than the recorded `Regions::views` call site
- ❌ Deriving the cell advance width outside the canvas prepaint (`cell`) — hit tests and drawn borders then disagree
- ❌ Platform #[cfg] outside platform.rs
- ❌ Hardcoded colors bypassing theme provider
- ❌ Hardcoding a shell panel, status segment or command list outside the contribution registry
- ❌ Editing vendored `../vendor/herdr-{protocol,client}/**` instead of re-syncing it
