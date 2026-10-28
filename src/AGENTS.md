# src/ — Main GPUI Application

## OVERVIEW
Core GPUI application for zenix desktop frontend. All UI, state, IPC, and system integration.

**UI rewrite in progress (2026-10-28).** Branch `feat/ui-shell`, plan `.relay/PLAN.md`, milestone U4 (U3-1, U3-2, U3-3, U3-4, U3-5, U4-1, U4-2, U4-3, U4-4 and U4-5 done). The shell is being rebuilt on the component library over `gpui-pre 0.3.10`; `gpui-kit` is gone from the graph. U0 quarantined the old UI: `shell/`, the window-facing `terminal/` files, `plugins/{views,manager,drawer,panels,trust_view}.rs` and `config/theme.rs` were taken out of the module tree; the terminal files returned in U2-3 and the rest were deleted in U4-1. U1-1 added `ui/theme.rs`: the config's appearance becomes the library's mode, per-mode palettes (no face of zenix's own: the library's Inter and JetBrains Mono are used), installed by `config::apply` before the first window opens. U1-2 moved the schema onto those four layers (`appearance.mode` / `.accent` / `.syntax` / `.terminal_follows_mode`, dark by default) and gave the retired `theme` / `theme_mode` keys migration plus `Deprecated` diagnostics. U1-3 retired the named terminal palettes: `terminal::Theme` is now built from the chrome palette's ANSI 16 (`ui::theme::ThemeSettings::terminal_theme`), with the design's ground per tone and the dark anchor when the terminal does not follow; `terminal.palette_dark` / `palette_light` load as `Deprecated` diagnostics. U1-4 fixed the rebuild's primitive vocabulary: the kit's `h_flex`/`v_flex`, `Theme`, `Icon`, `Button`, `Input`, `Switch` and `Select` map one-to-one onto `ely_core::layout::{h_stack, v_stack}`, `theme`, `primitives`, `buttons` and `forms`, spelled in `ui/mod.rs`'s module docs; the active tree imports none of the kit. U2-1 replaced the placeholder with the real frame: `ui/shell.rs` starts the one endpoint connection, drains its clipboard/notices, follows snapshots and repaints, and `ui/chrome.rs` composes the shell from the library — identity strip (`drag_region` + `WindowControls`; no controls under X11's server-decoration fallback), `ActivityBar` rail, 260px sidebar column naming the selected view and its data source, the focused workspace's `TabBar` (select / create / close / reorder over `tab.*`), and a `StatusBar` in the main column (connection label/dot, snapshot counts, branch, config diagnostics, source). `lib.rs` asks for client decorations, places the macOS traffic lights from the theme, and registers one close guard that saves the window's geometry for both close paths. U2-2 filled the sidebar: `ui/sidebar.rs` draws the workspace tree (the focused workspace opens into its tabs' sub-rows), the focused repo's WORKTREES section over `crate::worktrees` (a press focuses an open worktree or opens one, `+` creates one), the agents in `agent_order`, the fleet with `crate::fleet`'s watched status and held time, the event ring through its class chips over `crate::events`, and the plugin registry with local/daemon badges over `crate::plugins::host`; each view's head carries a live count and its foot names the source, and the column folds below 900px without writing the breakpoint back into the config. `ui/shell.rs` now owns those four hosts beside the endpoint connection and reads each cache on its pulse. U2-3 returned the panes: every window-facing terminal file is back in the module tree on the new stack (`terminal/{cards,copy_mode,error,find,input,links,painter,regions,selection,split_drag,splits,view}.rs`), and `terminal::view::TerminalView` is a component of `ui::Root`, not a connection owner — `Root::pull` hands it every pulse's `LiveState` (revision or not, so find/copy/split answers land) and the config/appearance observers hand it the terminal theme and metrics. It paints the daemon surface one card per pane (8px radius, shared-edge gaps, focused card on the accent), maps keys/paste/IME/wheel/mouse onto the daemon's semantic events, focuses panes through `focus_pane`, drags splits into `layout.set_split_ratio`, and carries selection + copy, scrollbar dragging, links, scrollback search (Ctrl-F) and copy mode. U2-4 added the command surface: `ui/commands.rs` is the one directory (id, locale label, chords, local-vs-daemon dispatch), `ui/keymap.rs` the prefix keymap (`ctrl-b`; `secondary-shift-p` claimed outright; a plugin's declared chord follows the same bare/modified rule and is rebuilt from the `Catalog` on every registry pulse), and `ui/palette.rs` the library's `CommandPalette` over commands / workspaces / plugin actions with an MRU ring. The identity strip's middle is the search trigger, the status bar always names the prefix and lights while armed, and find/copy mode have directory entries. `ui/theme.rs` also installs the library's message catalog in the config's locale. U3-1 added the settings surface: `ui/settings.rs` draws a 1080×780 card over a scrim with three nav groups and seven pages (appearance / terminal / AI / keys / plugins / MCP / about), a page-level reset for the two config pages, and focus taken by `take_focus` / `give_back` and trapped in a `FocusScope`; it opens from `ui/commands.rs`'s `view.settings` (`secondary-comma`), the rail's gear, or the palette, writes through `config::update` (AI through `api_provider::save_all`), and its accent presets are the live `tone_palette` while the code themes are custom chips (the library's `SyntaxThemePicker` would print `Ely`). `appearance.density` now shortens the sidebar's rows; `Root::new` owns the MCP orchestrator so the MCP page can say `listening`, and `LiveState.server_version` feeds About. U3-2 rebuilt the conversation on the library: `ui/chat.rs` is `ChatView` (an entity of `Root`) — a `PromptInput` composer whose `/` layer carries new chat / history / clear, a `MessageList` of `MessageBubble`s with `StreamingMarkdown` bodies and reasoning disclosures, tool cards (arguments / result / failure), an approval gate that parks the request and blocks the turn until deny / always / allow, a Retry card, a welcome state with starters, and session history in `ai/zenix-ai.db` (load, inline rename, two-step delete, relative stamps computed at paint) — and `ui/aside.rs` draws the config-sized column, its tab strip and unread badge, and the 34px rail when `layout.aside_visible` is off. Its code gates are green (G5 529 at U3-3), and **U3-2 is done**: the doubled chat glyphs sat on the inherited platform face (IBM Plex Sans, gpui's default text style), not on the library's Inter; `Root`'s root frame now names the library's theme face once, the reproduced `denied` / `Understood — I did not run it.` lines render clean, and the terminal keeps the library's mono. The earlier atlas reading is kept in `.relay/findings.md` as history, not proof. U3-3 added the aside's fleet tab: `ui/fleet.rs` is the panel's `State` (roster/TAIL mode, up to two picks, the tails and a read generation) and its body — cards built from the snapshot's agents plus `crate::fleet`'s watched status (dot, name, held age, pane line, an eight-slot transition strip, the status badge, focus-pane and read-tail buttons), the stream's five newest transitions, a source note, and the TAIL comparison: two mono columns of the panes' last 200 lines, read on demand off the UI thread (`crate::fleet::read_tail`, generation-guarded), with end-aligned differences washed in the selection colour and a footer counting them; a departed pane keeps its column with the read's age and a "no longer running" line, and a failed read is a red line rather than a panic. `ui/aside.rs` now offers `Chat` and `Fleet` (the strip and the rail), and `Root` owns `aside_tab`, the panel's `State` and the in-flight read task. U3-4 added the management surfaces, six modules in `ui/`: `confirm.rs` (the shared confirmation for worktree removal and plugin unlink, a force switch, focus taken and trapped), `manager.rs` (the 480px plugin drawer: overview / actions / panes / logs over one contract, graded argv, slots, logs, enable switch, `Commands…`, `Unlink`), `trust.rs` (the pre-link review: a path field, every argv graded with its rule sentence, declared slots, an acknowledgement checkbox gating Link), `inspector.rs` (the 420px event drawer over JSON / raw / contract plus the 660px 21-name contract), `layout_sheet.rs` (the 840px layout inspector: the file's text with line numbers, split preview, pane/split/schema/gap facts, copy/export/apply) and `worktree.rs` (the new-worktree form; the daemon names the directory). `events`' `Event` carries `wire`/`raw`, `LayoutState` keeps the file text and its ratios serialize to three decimals, `worktrees::create` takes branch/base, `plugins::trust::reason_label` maps the six rule sentences into the locale, and `Root` owns the surfaces plus the pending confirmation with `raised()` keeping shell chords from firing behind them. Its code gates are green (G5 545 at U3-4). U3-5 drew the first screen and the degraded states: `ui/empty.rs` is the block over the grid when there is no workspace — four entries that each run a directory command (New Workspace / Open Layout / Link a Plugin / Open Settings), and a daemon card that states the connection's own failing reason, reads `zenix/herdr-server.log` on demand, copies the start command and starts the daemon in place off the UI thread (`client::start_herdr_server` now appends the daemon's output there, `daemon_log_tail` / `daemon_start_command`); `ui/toast.rs` is the library's `Toaster`/`ToastViewport` stack, pushed to only by pulses — a daemon refusal (`LiveState.rejected`), a command the connection could not send, and a worktree or plugin call the daemon refused (`WorktreeState.last_error` / `PluginState.last_error`, added beside the existing outcome line so a removal's refusal is no longer silent) — and the layout inspector gained a directory field for the first-run 'open layout' path (it reads the chosen project and says Apply needs a workspace). The sidebar's workspaces view has its own centred empty state with the New Workspace button (disabled until the connection can take it) and its foot now says 'waiting for daemon' / 'last snapshot · daemon disconnected' in the connection's tone; the status bar adds 'no workspace', 'daemon not running' and 'waiting for snapshot', and the title strip's search trigger names the missing daemon. Its code gates are green (G5 551 at U3-5); G7 '/tmp/zenix-shots/ui-u3-5/'. U4-1 has landed the cleanup: the quarantine is gone (`shell/`, `plugins/{views,manager,drawer,panels,trust_view}.rs`, `config/theme.rs`, `assets/themes/*.json` deleted), `plugins/ui_data.rs` went with its only readers, the crate-level `allow(dead_code)` is removed and the tree holds zero warnings, the dead items the compiler then named are gone, and the unused direct `open` dependency left `Cargo.toml`. U4-2 gated the whole tree on the real desktop: G0–G6 green (G5 532 passed, zero warnings, probe 0.9.5 match), G7 shot on `:0` (11 surfaces in `/tmp/zenix-shots/ui-u4-2/`), `cargo build --release` + `cargo deb` producing the package, and one real defect fixed — the window grew 20px per restart because `ui::shell::save_window_bounds` stored the outer X11 size (client decorations' insets included); it now reads `Window::inner_window_bounds()`. U4-3 closed the delivery paperwork (component-library MIT text in `licenses/ely/LICENSE` and the `.deb`, the re-measured dependency census in `THIRD-PARTY.md`, READMEs rewritten off the retired kit, `docs/ui/redesign/` named there); no source changed, so the gates stayed at fmt / check / clippy 0 and G5 532. U4-4 closed the design-comparison gaps: `sidebar::workspace_row` now draws the snapshot's branch chip and an open worktree row draws its workspace's `git_ahead_behind` (`+n` / `-n`, zeroes dropped, closed worktrees silent), the settings frame's spacing was tightened so the appearance page's density row fits in the 780px card, and the U4-2 light shot (a duplicate of the dark one) was re-shot in real light mode. The pane card's in-card title bar is recorded as not implementable on generation-1 (the daemon lays out the grid from the reported surface size), not as unfinished work. G5 533. A follow-up the same day made the aside's width draggable (`ui/aside.rs`'s `Width`: 240–4096px, 60% window cap at paint, one config write per gesture, double-click resets to 372px; `Root` owns the value and handlers) — three tests, G5 536 — and acted on the report by removing every font choice: all `.font_family` calls, `register_fonts` and the Lilex assets are gone, the terminal uses only the library's `mono_family` (JetBrains Mono) for its cell grid, and the root frame names the library's theme face once so inherited text follows Inter rather than gpui's platform default (IBM Plex Sans); the library's Inter / JetBrains Mono / IBM Plex Sans faces arrive with their OFL texts in the package. The doubled glyphs had two causes stacked: the inherited Plex path — the reproduced `denied` and `Understood — I did not run it.` lines render clean once inheritance lands on Inter — and `swash 0.2.8`'s hinting cache, which leaves `entry.size` / `entry.coords` stale when it reuses an LRU entry (fixed upstream in 0.2.9), so the 11px reasoning body rasterized at a neighbouring size. The lock carries `swash 0.2.10`, and the same mock reasoning turn on the :87 fixture now renders clean at 11px beside the 13px answer; both causes are recorded in `.relay/findings.md`. The M-milestone history below describes the retired UI and is kept as history.

The UI is being rebuilt on **gpui-kit 0.7.3**: M1 deleted the old GPUI/PTY UI, M3 rebuilt native pane rendering, M4 built the workspace shell (`src/shell/`) on top of it, M5 made the client's settings real (`src/config/` + `src/shell/settings.rs`), M6 added the right-hand AI chat panel (`src/shell/chat.rs`, mounted through the registry's aside slot), M8a hosted the daemon's plugins (`src/plugins/`, the command palette), M8b mounts the UI a plugin declares for this client (`src/plugins/ui_manifest.rs` → registry contributions, data in `src/plugins/ui_data.rs`, drawn by `src/plugins/views.rs`), M8c opens the plugin surface outwards: every plugin action is an MCP tool (`src/mcp/plugin_tools.rs`) and a plugin directory is read, with its hazards, before it is linked (`src/plugins/trust.rs` + `trust_view.rs`), and M9 adds the post-rewrite capabilities: a project's layout as a file (`src/layout/`), the daemon's event stream beside the plugin hooks it fires (`src/events/`, the sidebar's `plugin-events` block), and the orchestration cockpit — the running agents as a fleet with their transitions (`src/fleet/`, drawn by `src/shell/cockpit.rs` under the chat panel) — plus the reverse MCP bridge: `zenix mcp-agent` serves exactly one tool over stdio (`src/mcp/agent_bridge.rs`, dispatched from `src/cli.rs`) so an agent with no shell can hand a prompt to another agent. See `../.relay/PLAN.md` (local, gitignored) for the milestone plan.

## STRUCTURE
```
src/
├── main.rs            # Thin binary: calls zenix::run()
├── lib.rs             # Startup path (`run`) + public SDK for plugins/sub-apps
├── cli.rs             # The binary's non-window verbs (`zenix mcp-agent`, `--help`)
├── bin/zenix-probe.rs # Daemon endpoint smoke gate (Welcome+Snapshot+Surface)
├── util.rs            # Shared helpers (timestamp_now)
├── ui/                # The new window root (U2-1..U3-5)
│   ├── mod.rs         # Module docs + the primitive vocabulary table
│   ├── shell.rs       # Root: the one connection, LiveState pulls, the pane grid + chat entities, keymap/palette/aside/fleet state, close guard, AppShell composition
│   ├── chrome.rs      # Identity strip with the search trigger and chat toggle, rail, sidebar column, tab strip, status bar with the prefix cap, the grid's slot
│   ├── sidebar.rs     # The five sidebar views: rows, head counts, source feet
│   ├── commands.rs    # The command directory: ids, locale labels, chords, local/daemon dispatch (U2-4)
│   ├── keymap.rs      # Prefix keymap (ctrl-b; the palette's claimed chord; plugin chords from Catalog) (U2-4)
│   ├── palette.rs     # The library's CommandPalette over commands / workspaces / plugin actions, MRU ring (U2-4)
│   ├── settings.rs    # The seven-page settings dialog over a scrim: Page table, nav groups, State, reset, focus takeover (U3-1)
│   ├── chat.rs        # The conversation: composer, message stream, tool/approval/error cards, welcome, session history (U3-2)
│   ├── fleet.rs       # The fleet tab: cards + transition strips, recent transitions, on-demand TAIL read + end-aligned diff (U3-3)
│   ├── aside.rs       # The panel's tabs (Chat / Fleet), the config-sized column, its strip and unread badge, the 34px rail (U3-2/U3-3)
│   ├── empty.rs       # The first-run block over the grid: four directory entries + the daemon card (reason, log tail, start in place) (U3-5)
│   ├── toast.rs       # The one short-lived message stack: refusals, unsent commands, host failures (U3-5)
│   └── theme.rs       # Appearance bridge: config → library Mode/Palette/fonts + library locale (U1-1/U2-4)
├── fleet/             # The cockpit's data: per-agent status transitions over the API lane (M9)
├── plugins/           # herdr plugin host (M8)
│   ├── model.rs       # The daemon's plugin vocabulary, as plugin.* reports it
│   ├── api.rs         # The plugin.* methods, one call each, over the JSON API lane
│   ├── host.rs        # The registry cache, its worker thread, and the shell's requests
│   ├── actions.rs     # One action as the palette, keymap and MCP bridge see it (Catalog)
│   ├── paths.rs       # The state/config dirs the daemon hands every plugin command
│   ├── ui_manifest.rs # zenix-ui.toml: what a plugin declares for this host to show
│   └── trust.rs       # A manifest → the commands it would run, and its hazards (M8c)
├── terminal/          # Cell painting + semantic input (M3)
│   ├── mod.rs         # Grid geometry, hit testing, key/mouse mapping
│   ├── painter/       # Cell-grid painter (glyphs, images, graphics, regions parts)
│   ├── regions.rs     # Per-pane cached paint regions
│   ├── splits.rs      # Split hit testing and ratios
│   ├── split_drag.rs  # A split border drag → layout.set_split_ratio (one request in flight)
│   ├── cards.rs       # Pane card geometry: content rect, border bands, channel tone
│   ├── selection.rs   # Pointer selection and the text a release copies
│   ├── links.rs       # OSC-8/inline links, hover and activation
│   ├── copy_mode.rs   # Keyboard copy mode over pane.copy_motion
│   ├── find.rs        # Scrollback search over pane.copy_search
│   ├── input.rs       # The Linux weak input handler (IME lifetime)
│   ├── theme.rs       # Terminal colors: the chrome's ANSI 16, packed
│   └── view.rs        # TerminalView: paints one daemon surface, maps input, drags splits
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
| App entry point | ../src/lib.rs (`run`) + main.rs | `gpui_platform::application()` with `ely_core::Assets`; `ely_core::init` then `config::init` before the first window |
| The new window root | ui/mod.rs | The real chrome (AppShell / ActivityBar / TabBar / StatusBar) since U2-1; `ui/sidebar.rs` since U2-2; the pane grid (`terminal::view::TerminalView`) since U2-3; the command surface (`commands.rs` / `keymap.rs` / `palette.rs`) since U2-4 |
| The appearance bridge | ui/theme.rs | The config's four layers → library `Mode` / per-mode `Palette` (no face installed; the library's defaults are used), and the library's message catalog in the config's locale; `sync` is called from `config::apply` before the first window |
| Command directory & keymap | ui/commands.rs, ui/keymap.rs | One table (id + locale label + chords + local/daemon dispatch); Ctrl-B prefix, `secondary-shift-p` claimed outright, plugin chords rebuilt from `Catalog` on each registry pulse |
| Command palette | ui/palette.rs | The library's `CommandPalette` over commands / workspaces / plugin actions; encoded row values, MRU ring, and the picked value's resolution |
| Settings dialog | ui/settings.rs | 7 pages over a scrim (appearance / terminal / AI / keys / plugins / MCP / about); `Page` table + nav groups, `State` (open/page/providers), `reset_page`, focus takeover/trap; entry `view.settings` = `secondary-comma`, rail gear, palette |
| Chat panel | ui/chat.rs | `ChatView`: composer with `/` commands, `MessageList` + `StreamingMarkdown`, reasoning/tool/approval/error cards, welcome starters, sessions in `ai/zenix-ai.db` (rename, two-step delete, unread count) |
| Fleet panel | ui/fleet.rs | `State` (mode / picks / tails / generation) + cards, transition strips, recent transitions, the on-demand TAIL read and the end-aligned diff |
| Fleet data | fleet/mod.rs | `Stream` (`pane.agent_status_changed`, per pane), `FleetState`, `read_tail` (read off the UI thread, generation-guarded by the caller) |
| Chat aside | ui/aside.rs | The panel's tabs (`Chat` / `Fleet`), the reader-sized column (a drag on its left edge; double-click resets 372px), its strip and unread badge; the 34px rail when `layout.aside_visible` is off |
| First-run block | ui/empty.rs | The four entries over the grid and the daemon card: reason, `zenix/herdr-server.log` tail, copy command, start in place (U3-5) |
| Toasts | ui/toast.rs | The library's `Toaster`/`ToastViewport`; refused requests, unsent commands, worktree/plugin failures (U3-5) |
| Workspace shell | ui/shell.rs, ui/chrome.rs | `Root` owns the one connection + pushed state; `chrome.rs` draws the identity strip, rail, sidebar, tab strip and status bar |
| Client settings | config/mod.rs | Schema, schema diagnostics, and the config global |
| Daemon event stream | events/mod.rs, ui/sidebar.rs | One session-wide subscription, drawn beside the plugin hooks (`plugin-events`) |
| Plugin host | plugins/{host,api,model}.rs | `plugin.*` over the JSON API lane; reads are on demand, nothing is pushed |
| Plugin-declared UI | plugins/ui_manifest.rs | `zenix-ui.toml` is read for what the manager and the trust review show |
| Plugin management | ui/manager.rs | The 480px drawer: contract, graded argv, slots, actions, panes, logs; enable / unlink (U3-4) |
| Plugin actions over MCP | mcp/plugin_tools.rs | `plugin.action.list` → tools, `invoke` → the log ring, stdout and exit code (M8c) |
| Reverse MCP bridge | cli.rs, mcp/agent_bridge.rs | `zenix mcp-agent`: stdio MCP, exactly one tool (`agent_send`), no window and no daemon dependency beyond the API socket |
| Pre-link trust review | plugins/trust.rs, ui/trust.rs | Manifest → argv list + hazards; `plugin.link` only after it is read |
| Plugin API surface | lib.rs | Public exports only |
| Daemon smoke gate | bin/zenix-probe.rs | `cargo run --bin zenix-probe` |
| Endpoint facade | herdr/connection.rs, herdr/state.rs | Commands out, snapshots in |
| Terminal painting | terminal/view.rs, terminal/painter/ | One daemon surface painted natively, one card per pane; Root pushes each pulse's LiveState in (`sync`) and the observers push the theme/metrics |
| Config parsing | config/mod.rs | JSONC, walked against the schema; problems become diagnostics |
| Herdr lifecycle | client/mod.rs | Endpoint socket via `herdr_client::ConnectTarget` |
| Endpoint protocol/client | ../vendor/herdr-protocol, ../vendor/herdr-client | Copy-only vendored crates |
| Agent CLI catalog | provider/mod.rs, provider/builtin.rs | 8 known agent CLIs: name, config dir, MCP config file |
| Terminal colors | terminal/theme.rs | The chrome palette's ANSI 16 packed for the grid, plus the xterm cube/ramp; `ui::theme::ThemeSettings::terminal_theme` builds it |
| System stats | sys.rs | sysinfo crate; `SystemInfo`/`CpuSamples` are the SDK re-export, nothing in the tree calls `collect` |
| Platform code | platform.rs | Linux/macOS/Windows gated |
| I18n keys | i18n.rs, ../locales/zenix.yml | rust-i18n `t("key")` |

## CONVENTIONS
- **UI depends on the component library + `gpui-pre` only**: `use gpui::*` for GPUI, `ely_core` for drawing primitives; never add `gpui-kit` / `gpui-component` back, and keep every Ely crate on the pinned commit
- **Text rasterization stays on `swash 0.2.10`**: both `gpui-pre-wgpu` and `cosmic-text` accept caret `0.2.6`, but 0.2.8 leaves `entry.size` / `entry.coords` stale when its hinting cache reuses an LRU entry, rasterizing small text at a neighbouring size; do not let a lockfile refresh pull it back down
- **GPUI Entity pattern**: State in Entity, actions via Context
- **No state mutation in render()**. The one recorded exception is `TerminalView`'s paint path calling `Regions::views`: the painter's per-pane views are entities that must be created/updated while a frame is being built, and GPUI offers no `compute_view` at this pin. `window.use_keyed_state` has the same constraint — only from `request_layout`/`prepaint`/`paint`, never from a click callback.
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
- **Every shell surface is a module of `ui/`, and every command is one row of `ui/commands.rs`**: the
  keymap binds that table's chords, the palette draws its rows, and the chrome's own triggers call the
  same `Root::run_command` — so a command cannot be reachable in one place and different in another.
- **One UI face from the root, one mono from the terminal**: `Root`'s outermost frame sets
  `.font_family(cx.theme().font_family)` so inherited text (markdown bodies, source feet, status
  text) speaks the library's Inter; `TerminalView` names `cx.theme().mono_family` because its cells
  need a fixed advance. Never install a zenix font or name another face — inheriting text would fall
  back to gpui's platform default (IBM Plex Sans), the split the U4-5 report caught.
  Do not add a second table, a second dispatch, or a contribution registry.
- **A plugin's declaration is data, never code**: `zenix-ui.toml` names herdr actions and built-in
  slot kinds; the client shows it (the manager's contract page, the trust review's slots) and never
  executes it. The host adds no plugin format, no installer and no permission system of its own;
  reading it can only downgrade (unknown `view` → `kv`, unknown `placement` → drawer, unknown field →
  ignored, a newer schema version → skipped whole), and every downgrade leaves a warning.
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
- ❌ Mutating state in render() anywhere other than the recorded `terminal/view.rs`
  `Regions::views` call site (and `window.use_keyed_state` while a frame is built)
- ❌ Deriving the cell advance width outside the canvas prepaint (`cell`) — hit tests and drawn borders then disagree
- ❌ Platform #[cfg] outside platform.rs
- ❌ Hardcoded colors bypassing theme provider
- ❌ Adding a shell command anywhere but `ui/commands.rs`'s directory — the chord, the palette row and the chrome trigger read the one table
- ❌ Letting a declaration carry an executable action of its own instead of a herdr action id
- ❌ A setting kept in a view struct beside the config (the two drift; write it through `config::update`)
- ❌ Editing vendored `../vendor/herdr-{protocol,client}/**` instead of re-syncing it
