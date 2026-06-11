# zenix

**Extensible terminal workspace manager for AI coding agents.**

A GPUI-native desktop frontend for [herdr](https://github.com/ogulcancelik/herdr) — workspace, tab, and pane management with built-in system monitoring and theme support.

---

## 概述 / Overview

zenix 是一个面向 AI 编程助手的可扩展终端工作区管理器。基于 [GPUI](https://github.com/zed-industries/zed) 构建，作为 [herdr](https://github.com/ogulcancelik/herdr) 的桌面前端，提供工作区、标签页、窗格管理，内置系统监控和多主题支持。

## 功能 / Features

- **Terminal multiplexing** — spawn and manage multiple PTY sessions via herdr
- **System monitor** — real-time CPU, memory, network, disk stats in sidebar
- **Theme engine** — 4 built-in themes (Gruvbox, Solarized, Tokyo Night, Matrix)
- **IME support** — full Chinese/Japanese/Korean input via GPUI InputHandler
- **Plugin architecture** — `lib.rs` exposes SDK for extensions and sub-apps
- **Self-updating herdr** — seeds herdr on first run to `~/.local/bin/herdr`, then herdr self-updates

## 项目结构 / Project Structure

```
src/
  main.rs                  Entry point
  lib.rs                   Public API for plugins & sub-apps
  app.rs                   ZenixApp — state, events, render
  config.rs                Config store (~/.config/zenix/config.json)
  sys.rs                   System info collector
  client/mod.rs            Herdr binary manager (install, start, socket)
  terminal/
    mod.rs                 PTY backend, types, spawn
    element.rs             GPUI terminal rendering
    encoding.rs            Key/mouse event encoding
  ui/
    mod.rs                 UI module
    sidebar.rs             Sidebar, settings & system info panels
res/
  zenix.desktop            Desktop entry
  zenix.svg                Application icon
assets/
  fonts/                   Lilex font family
  themes/                  Theme JSON files
herdr/                     Git submodule (ogulcancelik/herdr, v0.6.10)
build.rs                   Compiles/embeds herdr from submodule
```

## 依赖与许可证 / Dependencies & Licenses

| Dependency | License | Relationship |
|-----------|---------|-------------|
| [gpui](https://github.com/zed-industries/zed) | GPL-3.0 | Statically linked UI framework |
| [gpui-component](https://github.com/longbridge/gpui-component) | Apache-2.0 | Widget components (statically linked) |
| [herdr](https://github.com/ogulcancelik/herdr) | AGPL-3.0 | Bundled as separate binary (socket IPC, mere aggregation) |
| [alacritty_terminal](https://github.com/zed-industries/alacritty) | Apache-2.0 | Terminal emulation backend |

zenix itself is licensed under **GPL-3.0-or-later** (see [LICENSE](LICENSE)).

## 构建 / Build

### 前置依赖 / Prerequisites

- Rust 1.85+ (edition 2024)
- System libraries: `libxkbcommon-dev`, `libwayland-dev`, `libfontconfig-dev`, `cmake`

```bash
# Install system deps (Debian/Ubuntu/Deepin)
sudo apt install libxkbcommon-dev libwayland-dev libfontconfig-dev cmake pkg-config

# Clone with submodule
git clone --recurse-submodules https://github.com/re2zero/deepin-herdr-rust.git
cd deepin-herdr-rust
```

### 开发运行 / Development

```bash
# Uses system-installed herdr (fast)
cargo run

# Force build herdr from submodule
HERDR_BUILD=1 cargo run
```

### 发布构建与打包 / Release & Packaging

```bash
# Full release build (compiles herdr from submodule source)
cargo build --release

# Build .deb package
cargo deb
# Output: target/debian/zenix_0.1.0-1_amd64.deb
```

### Herdr 启动流程 / Herdr Launch Flow

```
zenix starts
  ↓
ensure_herdr()
  ├── ~/.local/bin/herdr exists? → use it (herdr self-updates this copy)
  ├── Bundled binary exists? → use it (dev mode)
  ├── PATH lookup
  └── Seed at /usr/share/zenix/herdr? → copy to ~/.local/bin/herdr → use it
       (first run after deb install; subsequent runs skip this step)
  ↓
start_herdr_server() → herdr server --daemon
  ↓
is_socket_ready() → connect via Unix socket
```

### 终端 PATH 隔离 / Terminal PATH Isolation

Spawned PTY sessions prepend `~/.local/bin` to `PATH`, ensuring the bootstrapped herdr CLI is used inside zenix terminals, not a system-installed version.
