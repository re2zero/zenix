# zenix · 界面重构提案

状态：**已拍板** · 2026-10-27（公子「可以」）

这是一份可交互的设计提案，不是实现分支。目标：彻底移除 `gpui-kit` / `gpui-component`，把外壳、设置、对话、舰队、插件、事件与布局全部建立在组件库的设计系统上。

## 打开

直接在浏览器打开 `index.html`（无需服务器）：

- 右上角切换 **深色 / 浅色** 两套基底模式，所有界面实时换色。
- 设置 → 外观里可以看到四层主题配置：模式、强调色、代码主题、自定义导入。界面文案不出现组件库名称。
- 左侧导航进入 9 个功能面；每个面右上角切换状态（空态 / 对话 / 审批 / 历史、管理器 / 信任审查、事件流 / 契约等）。
- 图上的编号圆点是交互注释，悬停可见说明。
- 同目录的 `renders/` 是四张审阅用对照图（核心界面 / 状态 / 设置 / 两套模式）。

## 工程说明：关于「0.8.1」

> 以下是工程版本信息，不出现在界面文案里。

`0.8.1` 是 **GPUI Kit** 的版本号，不是组件库的。组件库远端 main 已经到 `30c66fe`（本地 checkout 是 `3a15280`），crate 版本为 `ely-core 0.2.0` / `ely-gpui-component 0.1.1`，没有 `0.8.1` 这个 tag。它钉的 `gpui-pre = 0.3.10` 正是 GPUI Kit 0.8.1 使用的同一快照。

所以这里的「基于 0.8.1 开发」= **组件库 latest main `30c66fe` + gpui-pre 0.3.10 + Rust 1.98.1**，而不是把 gpui-kit 0.8.1 加回来。等组件库发布 0.8.x 时，再按新 rev 升级即可。

## 结论先说

1. **彻底替换可行**。组件库已提供 `AppShell`、`TitleBar`、`ActivityBar`、`TabBar`、`StatusBar`、`Sidebar`、`SplitPane`、`Card`、`SettingsLayout/Section/Row`、`CommandPalette`、`ConfirmationCard`、`Drawer`、`DashboardCard`、`EmptyState`、`Timeline`、`Badge` 等；聊天与智能体组件已经在实验分支里跑通。
2. **终端 painter 保留**。它是 daemon surface 的绘制器，不是组件库；改用 `gpui-pre` 直接依赖 + 组件 token，不再 import gpui-kit。
3. **`gpui-component` 随 gpui-kit 一起消失**。验收标准是 `cargo tree -i gpui-kit` 为空，且 Cargo.toml 里没有 `gpui-kit` / `gpui-component`。
4. **版本要一起动**。当前实验分支钉在组件库 `eb2be8e`（gpui-pre 0.3.9，因为要和 gpui-kit 共存）；去掉 gpui-kit 后应升到组件库 `30c66fe`（gpui-pre 0.3.10，Rust 1.98 底线；本机已装 1.98.1），并把 `file://` 依赖换成 GitHub rev。

## 主题：直接用组件库，不自建

组件库的「很多主题可以配置」实际是四层，不是五套具名 chrome family：

| 层 | 组件 | 影响范围 |
|---|---|---|
| 模式 | `ThemeSelector` | Light / Dark / System，只切 Palette |
| 强调色 | `AccentColorPicker` | 命名预设（由 zenix 传入）+ 任意自定义色，只覆盖 `accent`；`focus` 仍是蓝色 token |
| 代码主题 | `SyntaxThemePicker` | 标准 / 静谧 / 纸面三套语法配色（底层映射），只影响代码与 markdown 代码块 |
| 自定义 | `ThemeImporter` / `ThemeEditor` | 可选：导入或编辑一份 Palette JSON，不做组件级覆盖 |

因此本次删掉 Zenix / Gruvbox / Solarized / Tokyo Night / Matrix 五个自建 family 与主题 JSON，终端也不再单独维护调色板：它直接使用内置 ANSI 16 色。品牌身份靠布局、组件库的 Inter + JetBrains Mono 字体与图标，不靠自建色板。

## 推荐

- 默认：**深色** + 默认单色强调色 + 标准代码主题；浅色模式使用 **浅色**。
- 设置页 **100% 卡片式**：1080×780 居中对话框，左侧页导航，右侧卡片分组，改动即时写入；外观页暴露四层主题配置。
- 终端默认跟随浅色模式变浅；想要深色锚点可以在外观页关闭「终端跟随浅色模式」。
- 管理面不抢主区域：插件管理器 480px 右侧抽屉、事件检查器 420px 抽屉、布局 840px 模态；终端始终在主区域。
- 尺寸：侧栏 260 / 对话 372 / 设置 1080×780。

## 已拍板的七件事

| # | 决策 | 推荐 |
|---|------|------|
| 01 | 基底模式 | 深色默认 + 浅色 + 跟随系统 |
| 02 | 设置形态 | 卡片 + 即时写入，7 页 |
| 03 | 管理面位置 | 抽屉 / 模态，不抢主区域 |
| 04 | 舰队形态 | Rail 花名册 + aside 卡片 / TAIL 双层 |
| 05 | 尺寸 | 260 / 372 / 1080×780 |
| 06 | 可配置主题 | 暴露强调色 / 代码主题 / 自定义导入（组件），不保留自建 family |
| 07 | 浅色终端 | 默认跟随浅色模式；可关掉，保留深色锚点 |

## 实施

在 `feat/ui-shell` 上按 U0–U4 实施（依赖手术 → 设计系统 → 壳层 → 管理面 → 清理验收），每阶段保持可编译、可运行、可截图对比。详见 `index.html` 的「迁移计划」与「组件映射」。

## 文件

- `index.html` — 提案正文与交互
- `app.css` — 页面与两套 artboard token
- `ui.js` — 外壳、侧栏、对话、舰队等共享组件
- `screens.js` — 9 个功能面、26 个状态与注释
- `renders/` — 四张审阅对照图
