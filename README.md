# zenix

**Extensible terminal workspace manager for AI coding agents.**
*可扩展的 AI 编程助手终端工作区管理器。*

A [gpui-kit](https://crates.io/crates/gpui-kit) desktop client for [herdr](https://github.com/ogulcancelik/herdr) — workspace, tab and pane management, a native terminal, system-side client settings, and an AI chat panel that drives the daemon's agents.

zenix 是 [herdr](https://github.com/ogulcancelik/herdr) 的 gpui-kit 桌面客户端，提供工作区、标签页与窗格管理、原生终端渲染、客户端设置，以及可直接驱动 daemon 智能体的 AI 对话面板。

## Features / 功能

- **Workspace shell** — sidebar, tab strip, split panes, status bar; every panel mounts through a contribution registry, not hardcoded in the shell.
  *工作台外壳 — 侧栏、标签栏、分屏、状态栏；所有面板经贡献点注册表装配，不硬编码在壳里。*
- **Native pane rendering** — the daemon owns the panes; zenix paints their surfaces and sends semantic input (keys, IME composition, paste, mouse, scrollback) rather than a byte stream.
  *原生窗格渲染 — 窗格归 daemon 所有；zenix 绘制 surface 并发送语义输入（按键、输入法组合串、粘贴、鼠标、回滚），而非字节流。*
- **Client settings** — `config.json` (JSONC) is walked against a schema on every load: an unknown key is dropped, a wrong type falls back per field, an out-of-range number is clamped, and each problem is reported in the settings panel.
  *客户端设置 — 每次加载都把 `config.json`（JSONC）与 schema 逐字段核对：未知键丢弃、类型错误按字段回落、越界数值收敛，问题在设置面板里逐条显示。*
- **AI chat panel** — a resizable right-hand panel with real turns, tool cards and per-session history in SQLite; the orchestrator drives the daemon through its JSON API lane.
  *AI 对话面板 — 右侧可调宽面板，真实多轮对话、工具卡片、会话落 SQLite；编排器经 daemon 的 JSON API lane 驱动其智能体。*
- **Agent monitor** — the sidebar lists the daemon's agents from snapshot pushes (no polling).
  *智能体监视器 — 侧栏按 snapshot 推送列出 daemon 的智能体（无轮询）。*
- **MCP orchestration** — zenix serves its orchestration tools over MCP (SSE) for the whole time a window is open.
  *MCP 编排 — 窗口开启期间，zenix 经 MCP（SSE）暴露编排工具。*
- **Plugin / sub-app SDK** — `src/lib.rs` is the public API surface for extensions.
  *插件 / 子应用 SDK — `src/lib.rs` 是扩展的公共 API 面。*
- **Self-updating herdr** — seeds herdr on first run to `~/.local/bin/herdr`, then herdr self-updates that copy.
  *herdr 自升级 — 首次运行安装到 `~/.local/bin/herdr`，此后 herdr 自行升级该副本。*

## Architecture / 架构

The daemon is the state truth. zenix connects to two sockets under the herdr config root and adds no state of its own for workspace/tab/pane/agent data:

- `herdr-client.sock` — the generation-1 **endpoint** protocol: surfaces in, semantic input out.
- `herdr.sock` — the daemon's **JSON API** lane, the one the `herdr` CLI itself uses. Only this lane advertises the agent and pane-read methods the orchestrator needs.

There is no local PTY: daemon panes are the only terminal source.

daemon 是状态的唯一真相。zenix 连接 herdr 配置根下的两个 socket，自身不额外保存工作区/标签/窗格/智能体状态：`herdr-client.sock` 走 generation-1 **端点**协议（surface 进、语义输入出），`herdr.sock` 走 daemon 的 **JSON API** lane（`herdr` CLI 自己用的那条，只有它提供编排需要的智能体与窗格读取方法）。本地没有 PTY：daemon 窗格是唯一的终端来源。

## Project Structure / 项目结构

```
src/
  main.rs                  Entry point: fonts, config, keymap, window / 入口：字体、配置、键位、窗口
  lib.rs                   Public SDK for plugins & sub-apps / 插件与子应用 SDK
  shell/                   Workspace shell: ShellView, commands, keymap, contribution
                           registry, settings pages, chat panel, panel resize
                           / 工作台外壳：ShellView、命令、键位、贡献点注册表、设置页、对话面板、面板拖宽
  terminal/                Cell painting and semantic input / 单元格绘制与语义输入
  herdr/                   Endpoint facade: connection + live state (snapshots in,
                           commands out) / 端点门面：连接与实时状态（快照进、命令出）
  config/                  The client's own config: schema, diagnostics, global store
                           / 客户端自身配置：schema、诊断、全局存储
  ai/                      Orchestrator: engine, tools, sessions, prompts, compactor
                           / 编排器：引擎、工具、会话、提示词、压缩
  client/mod.rs            Herdr binary manager (seed, start, sockets) / herdr 二进制管理
  platform.rs              Platform paths and gated code / 平台路径与平台相关代码
  provider/, api_provider/ Agent-CLI and REST API providers / 智能体 CLI 与 REST API 提供商
  mcp/, agent/, skills/    MCP server, agent detection, skill system / MCP 服务、智能体探测、技能系统
  sys.rs, i18n.rs, util.rs System info, rust-i18n wiring, helpers / 系统信息、i18n、工具函数
res/
  zenix.desktop            Desktop entry / 桌面入口
  zenix.png                Application icon / 应用图标
  herdr                    The pinned daemon (fetched, gitignored), the package's first-run seed /
                            pin 的 daemon（脚本下载，不进 git）：包里的首启种子
assets/
  fonts/                   Lilex font family, compiled in via include_bytes! / Lilex 字体族（编译进二进制）
  themes/                  Theme JSON, not wired into the client yet / 主题 JSON（尚未接入）
  prompts/                 Orchestrator prompt, seeded to the config dir / 编排器提示词（播种到配置目录）
licenses/herdr/            herdr's Apache-2.0 text, shipped in the package / herdr 许可文本
vendor/                    Endpoint protocol + client (herdr-gpui copies) and the daemon pin /
                           端点协议与客户端（herdr-gpui 副本）+ daemon 版本 pin
scripts/fetch-herdr.sh     Downloads the pinned herdr release into res/herdr (SHA-256 checked) /
                           下载 pin 的 herdr 发行版到 res/herdr（校验 SHA-256）
build.rs                   Copies res/herdr next to the client; release builds verify it against the pin /
                           把 res/herdr 复制到客户端旁；release 构建按 pin 校验
```

## Dependencies & Licenses / 依赖与许可证

| Dependency | License | Relationship / 关系 |
|-----------|---------|-------------|
| [gpui-kit](https://crates.io/crates/gpui-kit) | Apache-2.0 | Statically linked UI framework / 静态链接 UI 框架 |
| [gpui-component](https://crates.io/crates/gpui-component) | Apache-2.0 | Widget components (statically linked) / 静态链接组件库 |
| [herdr](https://github.com/ogulcancelik/herdr) | Apache-2.0 | Bundled binary, socket IPC (mere aggregation) / 独立二进制，socket IPC |
| [herdr-protocol / herdr-client](vendor/) | Apache-2.0 | Vendored endpoint protocol and client (statically linked) / 内嵌端点协议与客户端 |
| [Lilex](https://github.com/mishamyrt/Lilex) | SIL OFL-1.1 | Terminal font, compiled into the binary / 终端字体（编译进二进制） |

zenix itself is licensed under **GPL-3.0-or-later** (see [LICENSE](LICENSE)); the full
third-party inventory is in [THIRD-PARTY.md](THIRD-PARTY.md).

zenix 本体使用 **GPL-3.0-or-later**（详见 [LICENSE](LICENSE)），第三方组件清单见 [THIRD-PARTY.md](THIRD-PARTY.md)。

## Build / 构建

### Prerequisites / 前置依赖

- Rust 1.85+ (edition 2024)
- System libraries: `libxcb1-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libfontconfig1-dev`, `libfreetype6-dev`, `libexpat1-dev`, `libwayland-dev`, `libegl1-mesa-dev`, `libgl1-mesa-dev`, `pkg-config`
- **No Zig, no submodule** — the herdr daemon is a released binary pinned by version and
  SHA-256 in `vendor/herdr-pins.tsv`. Run `scripts/fetch-herdr.sh` once (it writes `res/herdr`),
  or point `HERDR_DOWNLOAD_BASE` at a mirror if GitHub is slow.

```bash
# Install system deps (Debian/Ubuntu/Deepin)
sudo apt install libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libfontconfig1-dev libfreetype6-dev libexpat1-dev libwayland-dev \
  libegl1-mesa-dev libgl1-mesa-dev pkg-config

# Clone and fetch the pinned daemon / 克隆并取 pin 的 daemon
git clone <repo-url>
cd zenix
./scripts/fetch-herdr.sh
```

### Development / 开发运行

```bash
# Uses res/herdr (fetched above) / 使用 res/herdr（上面已下载）
cargo run

# No seed yet? A dev build falls back to the installed herdr / 没有 seed 时 dev 回落到系统已装 herdr
cargo run
```

### Release & Packaging / 发布构建与打包

```bash
# Release build: verifies res/herdr against the pin, then packages it
# 发布构建：按 pin 校验 res/herdr，然后打包
cargo build --release

# Build the .deb (runs the release build first) / 构建 .deb（会先跑发布构建）
cargo deb
# Output: target/debian/zenix_0.1.0-1_amd64.deb
```

### Herdr Launch Flow / Herdr 启动流程

```
zenix starts
  ↓
ensure_herdr() / 确保 herdr 可用
  ├── ~/.local/bin/herdr exists? → use it (herdr self-updates this copy)
  │                                → 直接使用（herdr 自行升级此副本）
  ├── Binary built next to zenix? → use it (development) / 构建树里的副本（开发时）
  ├── PATH lookup / PATH 查找
  └── Seed at /usr/share/zenix/herdr? → copy to ~/.local/bin/herdr → use it
       (first run after a deb install; later runs skip this step)
       deb 安装后的首次运行；后续启动跳过
  ↓
start_herdr_server() → herdr server
  ↓
wait for the endpoint socket, then connect / 等待端点 socket 后连接
```

## Configuration / 配置

Two configurations meet in this client, and one rule separates them: **a setting lives with whatever acts on it.**

- `~/.config/zenix/config.json` (JSONC) — what the *client* acts on: light/dark mode, UI language, terminal palettes, sidebar and chat-panel layout. Written only through the settings panel.
- the daemon's own `config.toml` — what the *daemon* acts on: the shell a new pane starts, the cwd a new workspace starts in, scrollback, keybindings, plugins. zenix neither reads nor writes it; the settings panel's Config page points at it.

两条配置在这客户端交汇，规则只有一条：**设置跟着执行它的那一侧走。** `~/.config/zenix/config.json`（JSONC）是*客户端*自己执行的设置（明暗模式、界面语言、终端调色板、侧栏与对话面板布局），只经设置面板写入；daemon 自己的 `config.toml` 是*daemon*执行的设置（新窗格的 shell、新工作区目录、回滚、键位、插件），zenix 不读不写，只在设置面板的 Config 页指向它。
