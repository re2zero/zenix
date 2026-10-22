# PROJECT KNOWLEDGE BASE

**Generated:** 2026-10-13
**Commit:** 809cd6e
**Branch:** main

## OVERVIEW
zenix is a cross-platform GPUI desktop frontend for herdr terminal workspace manager. It provides workspace/tab/pane management, client settings, an AI chat panel, and a plugin architecture via Rust + GPUI.

The UI is being rebuilt on **gpui-kit 0.7.3** as a native client of the daemon's generation-1 endpoint protocol (multi-session plan in `.relay/PLAN.md` — local, gitignored). The old GPUI/PTY UI was deleted in M1, M3 rebuilt native pane rendering, and M4 has landed the workspace shell (`src/shell/`) with split-ratio and sidebar drag, tab reorder commands and the close chain, so the window is usable again. M5 has made the client's own settings real: `src/config/` holds a schema-checked `config.json` (with per-field diagnostics for a hand-edited file) and `src/shell/settings.rs` mounts the `gpui-kit` settings surface from the contribution registry. M6 has made the AI side real again: `src/shell/panels.rs` shows the daemon's agents from snapshot pushes, and `src/shell/chat.rs` is the right-hand panel's chat block — real turns over `src/ai` (streamed, with tool cards) persisted to `ai/zenix-ai.db`, mounted through the registry's aside slot. M7 has made delivery real: `cargo deb` produces the package, whose asset list ships the release-built herdr as the first-run seed (`build.rs` refreshes `res/herdr` on every release build — a stale seed leaves a fresh install unable to connect), the project and third-party license texts, and the desktop entry and icon. The Lilex faces and the theme JSONs are no longer packaged: the fonts are compiled into the binary and nothing reads the theme files yet. M8 has made the plugin surface real: M8a mounted the daemon's plugin registry (what is installed, its actions, its panes, the log ring) as registered contributions and added the command palette (`src/shell/palette.rs`) on `plugin.*` over the JSON API lane (`src/herdr/api.rs`); M8b mounts what a *plugin* declares for this client — `<plugin_root>/zenix-ui.toml` (`src/plugins/ui_manifest.rs`), drawn as sidebar rows, drawer panels and status segments through the very same registry (`Contribution::body`), fed by commands read off the UI thread (`src/plugins/ui_data.rs`, `src/plugins/views.rs`). M8c points the surface outward: every plugin action the daemon reports is an MCP tool beside the orchestrator's own (`src/mcp/plugin_tools.rs`, answering with the command's exit code and output), and a plugin directory is read — every argv it would run, plus its hazards — before it is linked (`src/plugins/trust.rs` + `trust_view.rs`). M9 has added the post-rewrite capabilities: a project's layout as a file that travels with the repo (`src/layout/`, `.zenix/layout.toml`), the daemon's event stream drawn beside the plugin hooks it fires (`src/events/`), and the orchestration cockpit (`src/fleet/` + `src/shell/cockpit.rs`) — the running agents as a fleet, with the transitions each went through and a tail comparison, and the reverse bridge: `zenix mcp-agent` serves exactly one MCP tool over stdio (`agent_send`, `src/mcp/agent_bridge.rs`) so an agent with no shell can hand a prompt to another agent, dispatched from `src/cli.rs` before any window exists. M11 has been restyling the shell against `docs/ui/prototype.html`: M11a drew panes as cards and split the status bar by side, M11b established that split ratios belong to the daemon (`layout.set_split_ratio`, restored from its own `session.json`), M11c gave the right-hand panel three tabs (Chat / Fleet / Panels, all mounted at once so their inputs survive a switch) and the empty grid four entries, M11d made one `zenix-ui.toml` declaration the row, the chord and the MCP tool (`src/plugins/actions.rs`; a chord with a modifier is claimed outright, a bare one follows the prefix), and M11e marked every plugin contribution with its source badge and added the plugin manager (`src/plugins/manager.rs`: one plugin's slots, actions, panes and logs, with enable / disable / unlink over `plugin.*`), and M11f gave a declaration its own clock: `refresh_s` in `zenix-ui.toml` sets how often its command is read (absent = ten seconds, `0` = once), and nothing off screen is read at all — a drawer panel only while the aside is open on its tab, a status segment always, the last answer kept on screen while a re-read runs. M11g–i (2026-10-22) finished the chrome against the prototype: the title bar is the identity strip (brand, workspace pill with its branch chip, connection dot, a centered search trigger into the palette, icon toggles with active states), the rail has its active-edge bar, an Agents badge and the settings gear, tabs wear their agents' status dot and pane count, and the palette reads in three named sections (commands / workspaces / plugin actions) with a keyboard-path footer. M11j (2026-10-22) closed the restyle's last gap: every sidebar view has a 34px head (view name, live count, the plugins view's manager button) and a foot naming its data source, the workspaces view is a tree — the focused workspace opens into its tab sub-rows (agent dot, pane count, click to focus) under a New Workspace button — the events view reads through filter chips (all / agents / plugins / workspaces & tabs / layout, one class per event), the settings surface gained the keys / plugins / mcp reference pages (the keys page renders the command directory's real chords; the others state the plugin host's and MCP surface's facts — nothing the client cannot act on is offered as a toggle), the rail has its sixth (fleet) view mounting the cockpit's roster and transitions as a sidebar block, and the status bar's diagnostics and branch segments are clicks into the settings surface and the workspaces tree. The restyle set out in M11 is complete; what may grow next is new scope, not residue. Tab and workspace reordering is pointer-driven since M10-2 (`src/shell/reorder.rs`). M12 (2026-10-22) made the theme JSONs real: `appearance.theme` names a family (default / gruvbox / solarized / tokyonight / matrix) whose embedded configs (`src/config/theme.rs`, `include_str!` over `assets/themes/`) fill the global theme's light and dark slots, so a mode toggle stays inside the family; a variant the family lacks falls back to the built-in slot, and the terminal gained six matching hand-authored palettes.

## STRUCTURE
```
zenix/
├── src/                    # Main GPUI application
│   ├── main.rs            # Entry point: fonts, config init, keymap, window, ShellView
│   ├── lib.rs             # Public SDK for plugins/sub-apps
│   ├── cli.rs             # Non-window verbs: `zenix mcp-agent`, `--help`
│   ├── bin/zenix-probe.rs # Daemon endpoint smoke gate (Welcome+Snapshot+Surface)
│   ├── util.rs            # Shared helpers (timestamp_now)
│   ├── shell/             # Workspace shell (M4) + settings (M5) + chat panel (M6) + cockpit (M9) + palette (M8a) + empty state (M11c)
│   ├── terminal/          # Cell painting + semantic input (M3): painter/, regions/splits, view.rs
│   ├── fleet/             # The cockpit's data (M9): per-agent status transitions over the API lane
│   ├── events/            # The daemon's event stream (M9): one subscription, drawn beside plugin hooks
│   ├── layout/            # A project's layout as a file (M9): .zenix/layout.toml, export/apply
│   ├── plugins/           # herdr plugin host (M8): registry cache, zenix-ui.toml, declared data, views, trust review
│   ├── herdr/             # Endpoint facade: Connection + LiveState (snapshots in, commands out)
│   ├── config/            # The client's config (M5): schema + diagnostics + store/global; jsonc.rs/scanner.rs
│   ├── sys.rs             # System info collector (CPU, mem, net, disk)
│   ├── client/mod.rs      # Herdr binary manager (install, start, endpoint socket)
│   ├── provider/          # Agent CLI catalog (name, config paths, capabilities)
│   ├── api_provider/      # REST API for external integrations
│   ├── mcp/               # MCP orchestrator (M6) + one tool per plugin action (M8c) + the one-tool reverse bridge (M9)
│   ├── config_adapters/   # Per-agent config adapters
│   ├── i18n.rs            # rust-i18n wiring (strings in locales/zenix.yml)
│   └── platform.rs        # Platform-specific code (Linux/macOS/Windows)
├── licenses/herdr/        # herdr's Apache-2.0 text, shipped into the package
├── scripts/               # fetch-herdr.sh (download + SHA-256 pin), sync-herdr-vendor.sh
├── vendor/                # Endpoint protocol + client, copied from herdr-gpui (Apache-2.0)
│   ├── herdr-protocol/    # Generation-1 wire + handshake model
│   ├── herdr-client/      # Endpoint client (connect/session/surface/input)
│   └── test-support/      # sandbox.rs, only for herdr-client's opt-in live tests
├── assets/
│   ├── fonts/             # Lilex font family
│   ├── prompts/           # Prompt assets
│   └── themes/            # Theme JSON (gruvbox, solarized, tokyonight, matrix)
├── locales/zenix.yml      # I18n strings (rust-i18n)
├── build.rs               # Copies res/herdr next to the client; release verifies it against the pin
├── Cargo.toml             # Package manifest (path deps into vendor/)
└── .relay/                # Session relay: PLAN.md / state.json / journal/ (local, gitignored)
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| App lifecycle, state, events | src/main.rs, src/shell/mod.rs | ShellView: title bar, sidebar, tab strip, status bar |
| Commands & keymap | src/shell/commands.rs, src/shell/keymap.rs | One command directory; chords and palette read it |
| UI contributions | src/shell/registry.rs, src/shell/panels.rs | Panels, status segments and settings pages mount through the registry |
| Client settings | src/config/mod.rs, src/shell/settings.rs | Schema + diagnostics + the config global; pages mount from the registry |
| AI chat panel | src/shell/chat.rs | Right-hand panel: turns over `src/ai`, tool cards, sessions in `ai/zenix-ai.db` |
| Agent monitor | src/shell/panels.rs | Sidebar rows built from snapshot `agents` + `agent_order` (push, no polling) |
| Orchestration cockpit | src/shell/cockpit.rs, src/fleet/mod.rs | Fleet + transitions (per-pane `pane.agent_status_changed`) + on-demand tail comparison |
| Terminal painting | src/terminal/view.rs, src/terminal/painter/ | One daemon surface painted natively |
| Endpoint facade | src/herdr/connection.rs, src/herdr/state.rs | Commands out (in-flight tracked), snapshots in |
| Plugin host (installed/actions/logs) | src/plugins/{api,host,model,panels}.rs | `plugin.*` on the JSON API lane; the daemon pushes nothing, so reads are on demand |
| Plugin actions, one derivation | src/plugins/actions.rs | `Catalog` pairs the daemon's actions with `zenix-ui.toml`; the palette row, the keymap chord and the MCP tool are its three projections |
| Plugin manager | src/plugins/manager.rs | One plugin's page: slots, actions, panes, logs; enable / disable / unlink are `plugin.*` calls |
| Plugin-declared UI | src/plugins/{ui_manifest,ui_data,views}.rs | `zenix-ui.toml` becomes registry contributions; data is a command's stdout JSON |
| Command palette | src/shell/palette.rs | The command directory, the plugin catalog's rows, and the chords they run |
| Reverse MCP bridge | src/cli.rs, src/mcp/agent_bridge.rs | `zenix mcp-agent`: stdio MCP with one tool (`agent_send`), no window |
| Plugin/sub-app SDK | src/lib.rs | Public API surface |
| Herdr IPC & lifecycle | src/client/mod.rs | Endpoint socket via `herdr_client::ConnectTarget`, binary bootstrap |
| Daemon smoke gate | src/bin/zenix-probe.rs | `cargo run --bin zenix-probe` — Welcome + Snapshot + Surface |
| Endpoint protocol | vendor/herdr-protocol/src/wire.rs | Wire field/variant order is a bincode compatibility contract |
| Endpoint client | vendor/herdr-client/src/ | connect/session/surface/input; copy-only |
| Agent CLI catalog | src/provider/builtin.rs, src/provider/mod.rs | 8 known agent CLIs: name, config dir, MCP config file |
| Terminal palettes | src/terminal/theme.rs | Built-in palettes as packed RGB, incl. six named after the chrome's theme families |
| Chrome theme families | src/config/theme.rs | The four embedded theme sets; `appearance.theme` picks one, variants fall back per mode |
| System monitoring | src/sys.rs | sysinfo crate, real-time stats |
| Config schema | src/config/mod.rs | JSONC walked against the schema; problems become diagnostics |
| I18n (CN/EN) | src/i18n.rs, locales/zenix.yml | rust-i18n macro |
| Build/packaging | Cargo.toml, build.rs | `cargo deb` asset list, license texts, herdr embedding and the `res/herdr` seed |
| Plan & handoff | .relay/PLAN.md §8, .relay/journal/HANDOFF-*.md | Read the relay protocol first (local, gitignored) |

## CONVENTIONS
- **Rust 2024 edition**, GPL-3.0-or-later
- **UI depends on `gpui-kit` only** — GPUI arrives via `use gpui_kit::*`; do not add git `gpui`/`gpui-component`/`gpui_platform` deps or `[patch]` entries
- **GPUI patterns**: Entity/Context/Action model, `cx.spawn()` for async, `FocusHandle` for focus
- **No state mutation in `render()`** except the one recorded call site: `Regions::views` in `src/terminal/view.rs` (gpui-kit 0.7.3 has no `compute_view`) — see `src/AGENTS.md`
- **Shell surface mounts through the registry**: sidebar panels, status segments, settings pages and the command list live in `src/shell/{registry,panels,settings,commands}.rs`, not hardcoded in the shell
- **A plugin action is one declaration**: palette row, chord and MCP tool all come from `src/plugins/actions.rs`'s `Catalog` — never assemble one of the three from `plugin.action.list` directly
- **A disabled plugin is offered nowhere**: `plugin.action.list` still answers for one while `plugin.action.invoke` refuses it (`plugin_disabled`), so `Catalog::runnable` filters before offering; its *declarations* stay readable through `PluginState.declared`
- **No `unwrap()` in production** — use `anyhow::Context` or `thiserror`
- **Platform-gated code** in `src/platform.rs` only; core uses `#[cfg(...)]` on imports/fns
- **Client settings live in the config global**: read with `config::get`, write with `config::update` (value + behavior + file move together); settings the daemon acts on belong in its own `config.toml` — see `src/config/mod.rs`'s module docs
- **Config is JSONC** (comments allowed), walked against a schema on every load: an unknown key is dropped, a wrong type falls back per field, an out-of-range number is clamped, and each is reported as a diagnostic
- **Endpoint is the state truth**: workspace/tab/pane/agent state comes from `ClientEvent::Snapshot` pushes — do not add polling
- **No local PTY**: daemon panes are the only terminal source (the local PTY/alacritty path was removed by decision)
- **Herdr binary is pinned, not built**: `vendor/herdr-pins.tsv` records version + per-platform SHA-256; `scripts/fetch-herdr.sh` downloads that release into `res/herdr` (gitignored), `build.rs` copies it beside the client and a release build refuses a seed that does not match. The client auto-seeds `~/.local/bin/herdr` on first run; herdr self-updates thereafter, and `zenix-probe` reports when the running daemon's version differs from the pin
- **Packaged assets**: fonts and theme JSONs are *not* shipped — the Lilex faces are compiled in via `include_bytes!` and the theme JSONs are compiled in via `include_str!` (`src/config/theme.rs`) — nothing reads them from disk. License texts (project GPL, Lilex OFL, herdr's Apache-2.0 from `licenses/herdr/LICENSE`, vendored crates' Apache-2.0 with NOTICEs) ship under `/usr/share/doc/zenix/`; the inventory is `THIRD-PARTY.md`

## ANTI-PATTERNS (THIS PROJECT)
- ❌ Building without a daemon seed — run `scripts/fetch-herdr.sh` once; a dev build without `res/herdr` silently falls back to whatever herdr is installed
- ❌ Editing `vendor/herdr-{protocol,client}/**` casually — they are local source now, but a change to `wire.rs` is a protocol decision: record it in `vendor/HERDR_UPSTREAM.md` and say why
- ❌ Reintroducing the herdr submodule, or a source build of the daemon — the pin (`vendor/herdr-pins.tsv`) plus `scripts/fetch-herdr.sh` is the contract
- ❌ Adding UI deps or API calls without checking real `gpui-kit` / `gpui-pre` signatures first (see the gpui-dev skill)
- ❌ Mutating state in GPUI `render()` anywhere other than the recorded `Regions::views` call site
- ❌ Hardcoding a shell panel, status segment, settings page or command outside the contribution registry (`src/shell/registry.rs`)
- ❌ Keeping a setting in a view struct beside the config, or writing `config.json` directly instead of through `config::update`
- ❌ Hardcoding theme colors — use theme provider tokens

## COMMANDS
```bash
# Development
./scripts/fetch-herdr.sh                  # Once: download the pinned daemon into res/herdr
./scripts/fetch-herdr.sh --check          # Verify res/herdr against vendor/herdr-pins.tsv
cargo run                                 # Uses res/herdr, else the installed herdr

# Build
cargo build --release        # Verifies res/herdr against the pin, then packages it
cargo deb                    # Build .deb package

# Testing/Quality
cargo fmt --check                       # Format check
cargo check --workspace --all-targets   # Type check (zero warnings since the 2026-10-21 dead-code sweep)
cargo test --lib                        # Unit tests (467 passed at the M12 close)
cargo run --bin zenix-probe             # Daemon endpoint smoke gate (every session since M2)
cargo test -p herdr-protocol -p herdr-client   # Vendored crate tests
cargo clippy                            # Lint — the CI gate is `-D warnings` with no `-A dead_code`
```

## NOTES
- **No herdr submodule, no Zig**: the daemon is a pinned release binary (`vendor/herdr-pins.tsv`, currently 0.9.5; SHA-256 values are GitHub's own asset digests). Bumping the daemon = edit the TSV, `scripts/fetch-herdr.sh`, then `cargo run --bin zenix-probe` and check `version: … match=true`
- **Upstream herdr source is not in the tree**: code comments cite `herdr/src/...:line` as evidence from when the submodule existed. To re-check one, `git clone --depth 1 --branch v0.9.5 https://github.com/herdrdev/herdr` into /tmp — never into the workspace
- **UI stack**: `gpui-kit = "0.7.3"` → `gpui-pre 0.3.9` / `gpui-component 0.7.3`; the old git `gpui` dep, the target `gpui_platform` blocks and `vendor/gpui_wgpu_patched` are gone
- **gpui-kit `test-support`**: the crate's GPUI test entry (`#[gpui_kit::test]`) is a normal feature and lives in `[dev-dependencies]`. Enabling it makes Cargo resolve `gpui-fast-web` → `unicode-properties =0.1.3`, while `gpui-pre-web` pins `0.1.4`; the two cannot coexist, so `Cargo.lock` holds 0.1.3 via `cargo update -p unicode-properties --precise 0.1.3`. Re-apply that command if the lock is regenerated.
- **Vendored crates are local source**: `vendor/herdr-protocol` + `vendor/herdr-client` (plus `vendor/test-support/sandbox.rs`) are copies of herdr-gpui `crates/*` (commit `87f4dc0`), with `NOTICE.md`/`LICENSE-APACHE` kept; `vendor/HERDR_UPSTREAM.md` records provenance and `scripts/sync-herdr-vendor.sh <herdr-gpui>` re-copies and diffs `wire.rs`
- **I18n**: Chinese/English via `rust-i18n` macro; keys live in `locales/zenix.yml`
- **Desktop entry** at `res/zenix.desktop`, icon at `res/zenix.png`
- **License**: GPL-3.0-or-later (zenix), Apache-2.0 (herdr, gpui-kit/gpui-component, vendored endpoint crates), SIL OFL-1.1 (Lilex) — see `THIRD-PARTY.md`
