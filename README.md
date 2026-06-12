# zenix

**Extensible terminal workspace manager for AI coding agents.**
*可扩展的 AI 编程助手终端工作区管理器。*

A GPUI-native desktop frontend for [herdr](https://github.com/ogulcancelik/herdr) — workspace, tab, and pane management with built-in system monitoring and theme support.

zenix 是 [herdr](https://github.com/ogulcancelik/herdr) 的 GPUI 桌面前端，提供工作区、标签页、窗格管理，内置系统监控和多主题支持。

## Features / 功能

- **Terminal multiplexing** — spawn and manage multiple PTY sessions via herdr
  *终端多路复用 — 通过 herdr 管理多个 PTY 会话*
- **System monitor** — real-time CPU, memory, network, disk stats in sidebar
  *系统监控 — 侧边栏实时显示 CPU、内存、网络、磁盘状态*
- **Theme engine** — 4 built-in themes (Gruvbox, Solarized, Tokyo Night, Matrix)
  *主题引擎 — 内置 4 套主题*
- **IME support** — full Chinese/Japanese/Korean input via GPUI InputHandler
  *输入法支持 — 通过 GPUI 完整支持中日韩输入*
- **Plugin architecture** — `lib.rs` exposes SDK for extensions and sub-apps
  *插件架构 — lib.rs 暴露 SDK 供扩展和子应用使用*
- **Self-updating herdr** — seeds herdr on first run to `~/.local/bin/herdr`, then herdr self-updates
  *herdr 自升级 — 首次运行安装到 ~/.local/bin/herdr，后续自行升级*

## Project Structure / 项目结构

```
src/
  main.rs                  Entry point / 入口点
  lib.rs                   Public API for plugins & sub-apps / 插件 SDK
  app.rs                   ZenixApp — state, events, render / 主应用
  config.rs                Config store (~/.config/zenix/config.json) / 配置
  sys.rs                   System info collector / 系统信息采集
  client/mod.rs            Herdr binary manager (install, start, socket) / herdr 管理
  terminal/
    mod.rs                 PTY backend, types, spawn / 终端后端
    element.rs             GPUI terminal rendering / 终端渲染
    encoding.rs            Key/mouse event encoding / 键鼠编码
  ui/
    mod.rs                 UI module / UI 模块
    sidebar.rs             Sidebar, settings & system info panels / 侧边栏
res/
  zenix.desktop            Desktop entry / 桌面入口
  zenix.svg                Application icon / 应用图标
assets/
  fonts/                   Lilex font family / 字体
  themes/                  Theme JSON files / 主题文件
herdr/                     Git submodule (ogulcancelik/herdr, v0.6.10) / 子模块
build.rs                   Compiles/embeds herdr from submodule / 构建脚本
```

## Dependencies & Licenses / 依赖与许可证

| Dependency | License | Relationship / 关系 |
|-----------|---------|-------------|
| [gpui](https://github.com/zed-industries/zed) | GPL-3.0 | Statically linked UI framework / 静态链接 |
| [gpui-component](https://github.com/longbridge/gpui-component) | Apache-2.0 | Widget components (statically linked) / 静态链接 |
| [herdr](https://github.com/ogulcancelik/herdr) | AGPL-3.0 | Bundled binary, socket IPC (mere aggregation) / 独立二进制 |
| [alacritty_terminal](https://github.com/zed-industries/alacritty) | Apache-2.0 | Terminal emulation backend / 终端后端 |

zenix itself is licensed under **GPL-3.0-or-later** (see [LICENSE](LICENSE)).

zenix 本体使用 **GPL-3.0-or-later** 许可证（详见 [LICENSE](LICENSE)）。

## Build / 构建

### Prerequisites / 前置依赖

- Rust 1.85+ (edition 2024)
- System libraries: `libxkbcommon-dev`, `libwayland-dev`, `libfontconfig-dev`, `cmake`

```bash
# Install system deps (Debian/Ubuntu/Deepin)
sudo apt install libxkbcommon-dev libwayland-dev libfontconfig-dev cmake pkg-config

# Clone with submodule / 克隆含子模块
git clone --recurse-submodules <repo-url>
cd zenix
```

### Development / 开发运行

```bash
# Uses system-installed herdr (fast) / 使用系统安装的 herdr（快速）
cargo run

# Force build herdr from submodule / 强制从子模块编译 herdr
HERDR_BUILD=1 cargo run
```

### Release & Packaging / 发布构建与打包

```bash
# Full release build (compiles herdr from submodule source)
# 完整发布构建（从子模块源码编译 herdr）
cargo build --release

# Build .deb package / 构建 deb 包
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
  ├── Bundled binary exists? → use it (dev mode) / 开发模式
  ├── PATH lookup
  └── Seed at /usr/share/zenix/herdr? → copy to ~/.local/bin/herdr → use it
       (first run after deb install; subsequent runs skip this step)
       首次 deb 安装后复制；后续启动跳过此步
  ↓
start_herdr_server() → herdr server --daemon
  ↓
is_socket_ready() → connect via Unix socket / Unix socket 连接
```

### Terminal PATH Isolation / 终端 PATH 隔离

Spawned PTY sessions prepend `~/.local/bin` to `PATH`, ensuring the bootstrapped herdr CLI is used inside zenix terminals, not a system-installed version.

PTY 会话中 `~/.local/bin` 前置到 `PATH`，确保终端内使用的 herdr CLI 为内置版本，而非系统安装的旧版。
