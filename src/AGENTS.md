# src/ — Main GPUI Application

## OVERVIEW
Core GPUI application for zenix desktop frontend. All UI, state, IPC, and system integration.

The UI is being rebuilt on **gpui-kit 0.7.3**: M1 deleted the old GPUI/PTY UI, M3 rebuilt native pane rendering, M4 built the workspace shell (`src/shell/`) on top of it, M5 made the client's settings real (`src/config/` + `src/shell/settings.rs`), M6 added the right-hand AI chat panel (`src/shell/chat.rs`, mounted through the registry's aside slot), M8a hosted the daemon's plugins (`src/plugins/`, the command palette), and M8b mounts the UI a plugin declares for this client (`src/plugins/ui_manifest.rs` → registry contributions, data in `src/plugins/ui_data.rs`, drawn by `src/plugins/views.rs`). See `../.relay/PLAN.md` (local, gitignored) for the milestone plan.

## STRUCTURE
```
src/
├── main.rs            # Entry point: fonts, config init, keymap, window, ShellView
├── lib.rs             # Public SDK for plugins/sub-apps
├── bin/zenix-probe.rs # Daemon endpoint smoke gate (Welcome+Snapshot+Surface)
├── util.rs            # Shared helpers (timestamp_now)
├── shell/             # Workspace shell (M4)
│   ├── mod.rs         # ShellView: title bar + sidebar + tab strip + grid + status bar
│   ├── commands.rs    # Command directory and the daemon method each becomes
│   ├── keymap.rs      # Prefix keymap (ctrl-b chords) + RunCommand action
│   ├── registry.rs    # Contribution registry: panels, status segments, settings pages
│   ├── panels.rs      # Built-in contributions (workspace panel, status segments)
│   ├── settings.rs    # Built-in settings pages, mounted from the registry
│   ├── chat.rs        # The right-hand panel's block: turns, tool cards, session store (M6)
│   └── panel_resize.rs# A panel's edge: drag, reset, and what to save
├── plugins/           # herdr plugin host (M8)
│   ├── model.rs       # The daemon's plugin vocabulary, as plugin.* reports it
│   ├── api.rs         # The plugin.* methods, one call each, over the JSON API lane
│   ├── host.rs        # The registry cache, its worker thread, and the shell's requests
│   ├── panels.rs      # The host's own sidebar blocks (installed plugins, log ring)
│   ├── ui_manifest.rs # zenix-ui.toml: what a plugin declares for this host to draw
│   ├── ui_data.rs     # The JSON those declarations' commands print, read off-thread
│   └── views.rs       # Drawing a declaration: panel, sidebar row, status segment
├── terminal/          # Cell painting + semantic input (M3)
│   ├── mod.rs         # Grid geometry, hit testing, key/mouse mapping
│   ├── painter/       # Cell-grid painter (glyphs, images, graphics, regions parts)
│   ├── regions.rs     # Per-pane cached paint regions
│   ├── splits.rs      # Split hit testing and ratios
│   ├── theme.rs       # Terminal palettes (packed RGB)
│   └── view.rs        # TerminalView: paints one daemon surface, owns the connection
├── herdr/             # Endpoint facade: Connection (connect/requests) + LiveState
├── config/            # The client's config: schema, diagnostics, store/global (~/.config/zenix/config.json)
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
| App entry point | main.rs | gpui-kit application run; `config::init` before the first window |
| Workspace shell | shell/mod.rs | ShellView: title bar, sidebar, tab strip, status bar |
| Commands & keymap | shell/commands.rs, shell/keymap.rs | One directory; chords and the palette read it |
| UI contributions | shell/registry.rs, shell/panels.rs, shell/settings.rs | Panels, status segments and settings pages mount through the registry |
| Client settings | config/mod.rs | Schema, schema diagnostics, and the config global |
| AI chat panel | shell/chat.rs | Turns over `crate::ai`, tool cards, sessions in `ai/zenix-ai.db` |
| Plugin host | plugins/{host,api,model}.rs | `plugin.*` over the JSON API lane; reads are on demand, nothing is pushed |
| Plugin-declared UI | plugins/{ui_manifest,ui_data,views}.rs | `zenix-ui.toml` → registry contributions, fed by a command's stdout JSON |
| Plugin API surface | lib.rs | Public exports only |
| Daemon smoke gate | bin/zenix-probe.rs | `cargo run --bin zenix-probe` |
| Endpoint facade | herdr/connection.rs, herdr/state.rs | Commands out, snapshots in |
| Terminal painting | terminal/view.rs, terminal/painter/ | One daemon surface painted natively |
| Config parsing | config/mod.rs | JSONC, walked against the schema; problems become diagnostics |
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
  must be recorded here before it lands. The second recorded shape is `window.use_keyed_state`
  (the chat panel's `render` in `shell/chat.rs`): it is GPUI's own per-element state, so the
  panel entity is created on the first frame that draws it and reused after, and the call
  itself keeps no view state.
- **Async via cx.spawn()** — never block on GPUI event loop
- **FocusHandle** for keyboard/input focus management
- **Config = one global, one writer**: client settings live in the config global (`config/mod.rs`,
  `~/.config/zenix/config.json`). Read them with `config::get(cx)` and change them with
  `config::update(cx, …)`, which moves the in-memory value, the client's behavior and the file
  together; a view must not keep a second copy of a setting. Settings the *daemon* acts on
  (pane shell, new-workspace cwd, scrollback, keybindings, plugins) belong in its own
  `config.toml` — the boundary is spelled out in `config`'s module docs.
- **A hand-edited file must not break or fool the client**: every load walks the file against
  `config::SECTIONS`, drops unknown keys, falls back per field on a wrong type, clamps a number
  out of range, and reports each as a `Diagnostic` the settings panel and status bar show.
- **Endpoint is the state truth**: workspace/tab/pane/agent state comes from `ClientEvent::Snapshot` pushes — no polling
- **Shell surface mounts through the registry** (`shell/registry.rs`): sidebar panels, status
  segments and settings pages are contributions, and the shell does not name its own parts directly.
  A contribution's `body` is either a built-in builder function or a plugin's parsed declaration
  (`plugins/ui_manifest.rs`), and both are mounted, dispatched and named the same way — so a plugin
  panel and the shell's own cannot drift apart.
- **A declaration is data, never code**: `zenix-ui.toml` names a herdr action to invoke, a command
  whose stdout is read as JSON, and one of the built-in views. The host adds no plugin format, no
  installer and no permission system of its own; reading it can only downgrade (unknown `view` →
  `kv`, unknown `placement` → drawer, unknown field → ignored, a newer schema version → skipped
  whole), and every downgrade leaves a warning.
- **A declared command runs off the UI thread, in the plugin's own root** (`plugins/ui_data.rs`):
  the daemon runs a plugin's commands from `<plugin_root>`, so this host does too, and it sets
  `HERDR_PLUGIN_CONTEXT_JSON` to the same JSON shape the daemon uses. One read per declaration until
  someone asks for a refresh — a repaint must not run a plugin's side effects again.
- **One cell metric**: the advance width the painter lays cells out with is the one every hit
  test, region view and IME cursor must use. `TerminalView` keeps the measured value (`cell`)
  and it is refreshed inside the canvas prepaint, because the painter's metrics are only
  available while a frame is built; a stale copy silently shifts every mouse hit and every
  drawn border by a growing offset.
- **No local PTY**: daemon panes are the only terminal source

## ANTI-PATTERNS
- ❌ Direct cargo run without the installed herdr or `HERDR_BUILD=1`
- ❌ unwrap() in production paths
- ❌ Mutating state in render() anywhere other than the recorded `Regions::views` and
  `use_keyed_state` call sites
- ❌ Deriving the cell advance width outside the canvas prepaint (`cell`) — hit tests and drawn borders then disagree
- ❌ Platform #[cfg] outside platform.rs
- ❌ Hardcoded colors bypassing theme provider
- ❌ Hardcoding a shell panel, status segment, settings page or command list outside the contribution registry
- ❌ Running a declared source command from `render` (or on the UI thread at all) — the reader owns it
- ❌ Letting a declaration carry an executable action of its own instead of a herdr action id
- ❌ A setting kept in a view struct beside the config (the two drift; write it through `config::update`)
- ❌ Editing vendored `../vendor/herdr-{protocol,client}/**` instead of re-syncing it
