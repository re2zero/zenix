# src/ — Main GPUI Application

## OVERVIEW
Core GPUI application for zenix desktop frontend. All UI, state, IPC, and system integration.

The UI is being rebuilt on **gpui-kit 0.7.3**: M1 deleted the old GPUI/PTY UI, M3 rebuilt native pane rendering, M4 built the workspace shell (`src/shell/`) on top of it, M5 made the client's settings real (`src/config/` + `src/shell/settings.rs`), M6 added the right-hand AI chat panel (`src/shell/chat.rs`, mounted through the registry's aside slot), M8a hosted the daemon's plugins (`src/plugins/`, the command palette), M8b mounts the UI a plugin declares for this client (`src/plugins/ui_manifest.rs` → registry contributions, data in `src/plugins/ui_data.rs`, drawn by `src/plugins/views.rs`), M8c opens the plugin surface outwards: every plugin action is an MCP tool (`src/mcp/plugin_tools.rs`) and a plugin directory is read, with its hazards, before it is linked (`src/plugins/trust.rs` + `trust_view.rs`), and M9 adds the post-rewrite capabilities: a project's layout as a file (`src/layout/`), the daemon's event stream beside the plugin hooks it fires (`src/events/`, the sidebar's `plugin-events` block), and the orchestration cockpit — the running agents as a fleet with their transitions (`src/fleet/`, drawn by `src/shell/cockpit.rs` under the chat panel) — plus the reverse MCP bridge: `zenix mcp-agent` serves exactly one tool over stdio (`src/mcp/agent_bridge.rs`, dispatched from `src/cli.rs`) so an agent with no shell can hand a prompt to another agent. See `../.relay/PLAN.md` (local, gitignored) for the milestone plan.

## STRUCTURE
```
src/
├── main.rs            # Entry point: fonts, config init, keymap, window, ShellView
├── lib.rs             # Public SDK for plugins/sub-apps
├── cli.rs             # The binary's non-window verbs (`zenix mcp-agent`, `--help`)
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
│   ├── cockpit.rs     # The cockpit block: the fleet, its transitions, the tail comparison (M9)
│   ├── palette.rs     # The command palette: the directory + the daemon's plugin actions
│   └── panel_resize.rs# A panel's edge: drag, reset, and what to save
├── fleet/             # The cockpit's data: per-agent status transitions over the API lane (M9)
├── plugins/           # herdr plugin host (M8)
│   ├── model.rs       # The daemon's plugin vocabulary, as plugin.* reports it
│   ├── api.rs         # The plugin.* methods, one call each, over the JSON API lane
│   ├── host.rs        # The registry cache, its worker thread, and the shell's requests
│   ├── panels.rs      # The host's own sidebar blocks (installed plugins, log ring)
│   ├── ui_manifest.rs # zenix-ui.toml: what a plugin declares for this host to draw
│   ├── ui_data.rs     # The JSON those declarations' commands print, read off-thread
│   ├── views.rs       # Drawing a declaration: panel, sidebar row, status segment
│   ├── trust.rs       # A manifest → the commands it would run, and its hazards (M8c)
│   └── trust_view.rs  # The pre-link review: the path input, the commands, the link (M8c)
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
├── provider/          # Agent CLI catalog (name, config dir, capabilities)
├── api_provider/      # REST API for external integrations
├── mcp/               # Model Context Protocol integration
│   ├── mod.rs         # The orchestrator entry and its socket; the config-management half is gone
│   ├── handler.rs     # The orchestrator's tools, plus every plugin action (M8c)
│   ├── plugin_tools.rs# A plugin action as an MCP tool: naming, context, outcome
│   ├── agent_bridge.rs# The reverse bridge: `zenix mcp-agent`, one tool, stdio (M9)
│   └── sse.rs         # The unix-socket listener an agent connects to
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
| Orchestration cockpit | shell/cockpit.rs, fleet/mod.rs | Fleet + transitions + tail comparison; data comes from per-pane `pane.agent_status_changed` subscriptions |
| Daemon event stream | events/mod.rs, plugins/panels.rs | One session-wide subscription, drawn beside the plugin hooks (`plugin-events`) |
| Plugin host | plugins/{host,api,model}.rs | `plugin.*` over the JSON API lane; reads are on demand, nothing is pushed |
| Plugin-declared UI | plugins/{ui_manifest,ui_data,views}.rs | `zenix-ui.toml` → registry contributions, fed by a command's stdout JSON on the cadence the declaration sets |
| Plugin actions over MCP | mcp/plugin_tools.rs | `plugin.action.list` → tools, `invoke` → the log ring, stdout and exit code (M8c) |
| Reverse MCP bridge | cli.rs, mcp/agent_bridge.rs | `zenix mcp-agent`: stdio MCP, exactly one tool (`agent_send`), no window and no daemon dependency beyond the API socket |
| Pre-link trust review | plugins/trust.rs, plugins/trust_view.rs | Manifest → argv list + hazards; `plugin.link` only after it is read |
| Plugin API surface | lib.rs | Public exports only |
| Daemon smoke gate | bin/zenix-probe.rs | `cargo run --bin zenix-probe` |
| Endpoint facade | herdr/connection.rs, herdr/state.rs | Commands out, snapshots in |
| Terminal painting | terminal/view.rs, terminal/painter/ | One daemon surface painted natively |
| Config parsing | config/mod.rs | JSONC, walked against the schema; problems become diagnostics |
| Herdr lifecycle | client/mod.rs | Endpoint socket via `herdr_client::ConnectTarget` |
| Endpoint protocol/client | ../vendor/herdr-protocol, ../vendor/herdr-client | Copy-only vendored crates |
| Agent CLI catalog | provider/mod.rs, provider/builtin.rs | 8 known agent CLIs: name, config dir, MCP config file |
| Terminal palettes | terminal/theme.rs | Built-in palettes as packed RGB; nothing reads `../assets/themes/` yet |
| System stats | sys.rs | sysinfo crate; `SystemInfo`/`CpuSamples` are the SDK re-export, nothing in the tree calls `collect` |
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
  `HERDR_PLUGIN_CONTEXT_JSON` to the same JSON shape the daemon uses. A repaint must not run a
  plugin's side effects again, so the reader keeps a schedule, not a per-frame call: `refresh_s`
  sets the cadence (absent = ten seconds, `0` = once, `n` = clamped to an hour), and only what is
  on screen is read at all — a drawer panel while the aside is open on its tab, a status segment
  always. A re-read keeps the last answer on screen rather than replacing it with "reading".
- **One cell metric**: the advance width the painter lays cells out with is the one every hit
  test, region view and IME cursor must use. `TerminalView` keeps the measured value (`cell`)
  and it is refreshed inside the canvas prepaint, because the painter's metrics are only
  available while a frame is built; a stale copy silently shifts every mouse hit and every
  drawn border by a growing offset.
- **A bridged tool's name is this client's, not the daemon's** (`mcp/plugin_tools.rs`): the daemon's
  action id is dotted, and a tool name is read by an MCP client, so the mapping folds it into
  `[A-Za-z0-9_]` inside the specification's 64 characters. The mapping is a pure function of the
  qualified id and the list and the call each rebuild it, so neither side caches a table that can go
  stale. A bridged call answers with the command's own outcome, which means waiting on the log ring:
  `plugin.action.invoke` returns as soon as the daemon has spawned the command.
- **Nothing is linked before its commands have been read** (`plugins/trust.rs`): linking hands herdr a
  directory whose commands run as the user. The review shows every argv the manifest declares plus
  the hazards worth stopping on; it reads one file and runs nothing.
- **The cockpit follows the fleet one connection at a time** (`fleet/mod.rs`): `events.subscribe`
  cannot be renegotiated, so a fleet that gained or lost an agent is followed by dropping that
  connection and subscribing to the new pane set, which is why the worker polls its own watch
  between reads (`ApiStream::next_line_within`, the one poll-shaped reader on that lane). The
  per-pane stream also spells its kind with dots (`pane.agent_status_changed`) where the
  session-wide one writes underscores (`pane_created`), and it replays nothing: a status that has
  not moved since the client started watching has no transition and no start time, and the panel
  says so instead of inventing one.
- **No local PTY**: daemon panes are the only terminal source

## ANTI-PATTERNS
- ❌ Building without the daemon seed — `scripts/fetch-herdr.sh` writes `res/herdr`; a dev build without it silently uses whatever herdr is installed
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
