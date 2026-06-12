# zenix

**可扩展的 AI 编程助手终端工作区管理器。**

[herdr](https://github.com/ogulcancelik/herdr) 的 GPUI 桌面前端，提供工作区、标签页、窗格管理，内置系统监控和多主题支持。

## 功能

- **终端多路复用** — 通过 herdr 管理多个 PTY 会话
- **系统监控** — 侧边栏实时显示 CPU、内存、网络、磁盘状态
- **主题引擎** — 内置 4 套主题（Gruvbox、Solarized、Tokyo Night、Matrix）
- **输入法支持** — 通过 GPUI 完整支持中日韩输入
- **插件架构** — `lib.rs` 暴露 SDK 供扩展和子应用使用
- **herdr 自升级** — 首次运行安装到 `~/.local/bin/herdr`，后续自行升级

## 项目结构

```
src/
  main.rs                  入口点
  lib.rs                   插件 SDK（公共 API）
  app.rs                   ZenixApp 主应用（状态、事件、渲染）
  config.rs                配置存储 (~/.config/zenix/config.json)
  sys.rs                   系统信息采集
  client/mod.rs            herdr 二进制管理（安装、启动、socket）
  terminal/
    mod.rs                 终端后端（PTY、类型、进程生成）
    element.rs             GPUI 终端渲染
    encoding.rs            键盘/鼠标事件编码
  ui/
    mod.rs                 UI 模块
    sidebar.rs             侧边栏、设置与系统信息面板
res/
  zenix.desktop            桌面入口文件
  zenix.svg                应用图标
assets/
  fonts/                   Lilex 字体族
  themes/                  主题 JSON 文件
herdr/                     Git 子模块（ogulcancelik/herdr, v0.6.10）
build.rs                   从子模块编译/嵌入 herdr
```

## 依赖与许可证

| 依赖 | 许可证 | 关系 |
|-----------|---------|-------------|
| [gpui](https://github.com/zed-industries/zed) | GPL-3.0 | 静态链接 UI 框架 |
| [gpui-component](https://github.com/longbridge/gpui-component) | Apache-2.0 | 静态链接组件库 |
| [herdr](https://github.com/ogulcancelik/herdr) | AGPL-3.0 | 独立二进制，socket IPC（mere aggregation） |
| [alacritty_terminal](https://github.com/zed-industries/alacritty) | Apache-2.0 | 终端仿真后端 |

zenix 本体使用 **GPL-3.0-or-later** 许可证（详见 [LICENSE](LICENSE)）。

## 构建

### 前置依赖

- Rust 1.85+ (edition 2024)
- 系统库：`libxkbcommon-dev`、`libwayland-dev`、`libfontconfig-dev`、`cmake`

```bash
# 安装系统依赖（Debian/Ubuntu/Deepin）
sudo apt install libxkbcommon-dev libwayland-dev libfontconfig-dev cmake pkg-config

# 克隆（含子模块）
git clone --recurse-submodules <repo-url>
cd zenix
```

### 开发运行

```bash
# 使用系统已安装的 herdr（快速）
cargo run

# 强制从子模块编译 herdr
HERDR_BUILD=1 cargo run
```

### 发布构建与打包

```bash
# 完整发布构建（从子模块源码编译 herdr）
cargo build --release

# 构建 .deb 包
cargo deb
# 输出：target/debian/zenix_0.1.0-1_amd64.deb
```

## Herdr 启动流程

```
zenix 启动
  ↓
ensure_herdr() — 确保 herdr 可用
  ├── ~/.local/bin/herdr 存在？→ 直接使用（herdr 自行升级此副本）
  ├── 内置二进制存在？→ 使用（开发模式）
  ├── PATH 查找
  └── /usr/share/zenix/herdr 种子存在？→ 复制到 ~/.local/bin/herdr → 使用
       （deb 安装后首次运行；后续启动跳过此步）
  ↓
start_herdr_server() → herdr server --daemon
  ↓
is_socket_ready() → Unix socket 连接
```

## 终端 PATH 隔离

PTY 会话中 `~/.local/bin` 前置到 `PATH`，确保终端内使用的 herdr CLI 为内置版本，不会被系统安装的旧版覆盖。
