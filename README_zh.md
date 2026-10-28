# zenix

**可扩展的 AI 编程助手终端工作区管理器。**

[herdr](https://github.com/ogulcancelik/herdr) 的原生 GPUI 桌面客户端 —— 提供工作区、标签页与窗格管理、原生终端渲染、客户端设置，以及可直接驱动 daemon 智能体的 AI 对话面板。

英文版见 [README.md](README.md)。

## 功能

- **工作台外壳** — 标识条、活动栏、侧栏、标签栏、窗格卡片与状态栏，由组件库组合而成。
- **统一命令面** — 单一命令目录同时供键位（`Ctrl-B` 前缀）、命令面板（`Ctrl-Shift-P`）与所有界面入口使用，三者不会各自漂移。
- **原生窗格渲染** — 窗格归 daemon 所有；zenix 绘制其 surface 并发送语义输入（按键、输入法组合串、粘贴、鼠标、回滚），而非字节流。
- **客户端设置** — 七页设置对话框经唯一的 schema 校验存储写入 `config.json`（JSONC）：未知键丢弃、类型错误按字段回落、越界数值收敛，问题就地逐条显示。
- **AI 对话面板** — 右侧可调宽面板，真实多轮对话、流式 markdown、工具卡片与审批门；会话落 SQLite，编排器经 daemon 的 JSON API lane 驱动其智能体。
- **舰队与工作树** — aside 的舰队页绘制每个智能体的状态变迁并按需对比两个窗格的末尾；侧栏列出聚焦仓库的工作树。
- **插件** — 已装插件、其声明的 `zenix-ui.toml` 槽位、链接前逐条评级的命令行、动作键位以及每个动作对应的 MCP 工具。
- **事件与布局** — daemon 的事件流可对照 21 个事件名的契约检查；项目布局以 `.zenix/layout.toml` 随仓库携带。
- **MCP 编排** — 窗口开启期间，zenix 经 MCP（SSE）暴露编排工具。
- **插件 / 子应用 SDK** — `src/lib.rs` 是扩展的公共 API 面。
- **herdr 自升级** — 首次运行安装到 `~/.local/bin/herdr`，此后 herdr 自行升级该副本。

## 架构

daemon 是状态的唯一真相。zenix 连接 herdr 配置根下的两个 socket，自身不额外保存工作区/标签/窗格/智能体状态：

- `herdr-client.sock` —— generation-1 **端点**协议：surface 进、语义输入出。
- `herdr.sock` —— daemon 的 **JSON API** lane，`herdr` CLI 自己用的那条；只有它提供编排需要的智能体与窗格读取方法。

本地没有 PTY：daemon 窗格是唯一的终端来源。

## 项目结构

```
src/
  main.rs                  入口：字体、配置、键位、窗口
  lib.rs                   插件与子应用 SDK（公共 API）
  cli.rs                   无窗口子命令：`zenix mcp-agent`、`--help`
  ui/                      窗口根：壳与框架、侧栏视图、设置、对话、舰队、aside、
                            命令/键位/面板、首屏、通知、主题
  terminal/                单元格绘制与语义输入
  fleet/                   智能体状态变迁与末尾读取
  events/                  daemon 事件流
  layout/                  .zenix/layout.toml 导出与应用
  worktrees/               工作树管理（list/create/open/remove）
  plugins/                 插件注册表、UI 声明、路径、信任
  herdr/                   端点门面：连接与实时状态（快照进、命令出）
  config/                  客户端自身配置：schema、诊断、全局存储
  ai/                      编排器：引擎、工具、会话、提示词、压缩
  client/mod.rs            herdr 二进制管理（播种、启动、socket）
  mcp/                     MCP 编排器 + 每插件动作一个工具 + 反向桥
  provider/, api_provider/ 智能体 CLI 与 REST API 提供商
  platform.rs              平台路径与平台相关代码
  sys.rs, i18n.rs, util.rs 系统信息、i18n、工具函数
res/
  zenix.desktop            桌面入口
  zenix.png                应用图标
  herdr                    pin 的 daemon（脚本下载，不进 git）：包里的首启种子
assets/
  prompts/                 编排器提示词（播种到配置目录）
licenses/
  herdr/LICENSE            herdr 的 Apache-2.0 许可文本（随包发布）
  ely/LICENSE              组件库的 MIT 许可文本（随包发布）
vendor/                   端点协议与客户端（herdr-gpui 副本）+ daemon 版本 pin
docs/ui/redesign/         界面设计提案（可交互 HTML）
scripts/fetch-herdr.sh     下载 pin 的 herdr 发行版到 res/herdr（校验 SHA-256）
build.rs                   把 res/herdr 复制到客户端旁；release 按 pin 校验
```

## 文档

- `docs/ui/redesign/` —— 界面设计提案：9 个功能面、26 个状态的可交互 HTML，外加四张审阅对照图；浏览器直接打开 `index.html`，同目录 `README.md` 记录了外壳实现所依据的决策。
- [AGENTS.md](AGENTS.md) 与 [src/AGENTS.md](src/AGENTS.md) —— 代码库知识库：结构、查阅位置、约定与反模式。
- [THIRD-PARTY.md](THIRD-PARTY.md) —— 第三方组件清单与依赖普查。

## 依赖与许可证

| 依赖 | 许可证 | 关系 |
|-----------|---------|-------------|
| [GPUI（`gpui-pre`）](https://crates.io/crates/gpui-pre) | Apache-2.0 | 静态链接 UI 框架，钉 `=0.3.10` |
| [ely 组件库](https://github.com/ELY-Open-Source/Ely-GPUI-Components) | MIT | 静态链接组件、对话与智能体组件，钉 `30c66fe` |
| [herdr](https://github.com/ogulcancelik/herdr) | Apache-2.0 | 独立二进制，socket IPC（mere aggregation） |
| [herdr-protocol / herdr-client](vendor/) | Apache-2.0 | 内嵌端点协议与客户端（静态链接） |
| [Inter](https://github.com/rsms/inter) · [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) · [IBM Plex Sans](https://github.com/IBM/plex) | SIL OFL-1.1 | 组件库字体（由组件库 assets 编译进二进制；zenix 不装字体，根节点沿用组件库的 UI 字体） |

zenix 本体使用 **GPL-3.0-or-later**（详见 [LICENSE](LICENSE)），第三方组件清单见 [THIRD-PARTY.md](THIRD-PARTY.md)。

## 构建

### 前置依赖

- Rust 1.95+（edition 2024；组件库的 MSRV）
- 系统库：`libxcb1-dev`、`libxkbcommon-dev`、`libxkbcommon-x11-dev`、`libfontconfig1-dev`、`libfreetype6-dev`、`libexpat1-dev`、`libwayland-dev`、`libegl1-mesa-dev`、`libgl1-mesa-dev`、`pkg-config`
- **不需要 Zig，也没有子模块** —— herdr daemon 是 pin 住的发行版二进制（版本 + SHA-256 记在
  `vendor/herdr-pins.tsv`）。跑一次 `scripts/fetch-herdr.sh` 生成 `res/herdr`；GitHub 慢时把
  `HERDR_DOWNLOAD_BASE` 指向镜像。

```bash
# 安装系统依赖（Debian/Ubuntu/Deepin）
sudo apt install libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libfontconfig1-dev libfreetype6-dev libexpat1-dev libwayland-dev \
  libegl1-mesa-dev libgl1-mesa-dev pkg-config

# 克隆并取 pin 的 daemon
git clone <repo-url>
cd zenix
./scripts/fetch-herdr.sh
```

### 开发运行

```bash
# 使用 res/herdr（上面已下载）
cargo run

# 没有 seed 时 dev 回落到系统已装 herdr
cargo run
```

### 发布构建与打包

```bash
# 发布构建：按 pin 校验 res/herdr，然后打包
cargo build --release

# 构建 .deb（会先跑发布构建）
cargo deb
# 输出：target/debian/zenix_0.1.0-1_amd64.deb
```

### Herdr 启动流程

```
zenix 启动
  ↓
ensure_herdr() —— 确保 herdr 可用
  ├── ~/.local/bin/herdr 存在？→ 直接使用（herdr 自行升级此副本）
  ├── zenix 旁边的构建副本存在？→ 使用（开发时）
  ├── PATH 查找
  └── /usr/share/zenix/herdr 种子存在？→ 复制到 ~/.local/bin/herdr → 使用
       （deb 安装后的首次运行；后续启动跳过）
  ↓
start_herdr_server() → herdr server
  ↓
等待端点 socket 后连接
```

## 配置

两条配置在这客户端交汇，规则只有一条：**设置跟着执行它的那一侧走。**

- `~/.config/zenix/config.json`（JSONC）—— *客户端*自己执行的设置：明暗模式、界面语言、终端调色板、侧栏与对话面板布局。只经设置面板写入。
- daemon 自己的 `config.toml` —— *daemon*执行的设置：新窗格的 shell、新工作区目录、回滚、键位、插件。zenix 不读不写，只在设置面板的 Config 页指向它。
