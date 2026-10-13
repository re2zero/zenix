# UI 独特性与插件扩展架构

状态：**planning 完成（D1–D6 全部收敛）**；实施计划见 `implement.md`，技术设计见 `design.md`
关联：`.relay/PLAN.md`（硬约束 4/9、M4 地基、M8 实现、M9 后置项）
前置调查：`.openbitfun/plans/zenix-gpui-kit-herdr-rebuild.plan.md`（全量技术取证）

> 编号约定：本文档内部决策用 **PD** 前缀（PRD Decision），与 `.relay/state.json` 的全局待拍板决策 **D1–D6** 区分。映射见 PD 条目括号。

## Goal

zenix 重建（gpui-kit 0.7.3 + herdr 0.9.3 endpoint 原生客户端）之后，UI 层要具备 herdr-gpui 不具备的能力，且插件扩展必须**同时覆盖功能与 UI**——第三方能在不重编译 zenix 的前提下新增界面与能力。

用户价值：

- **内置与插件同源**：任何新面板/命令/工具都以"注册面装配"交付，不靠改内核（针对现有痛点：zenix 的插件 SDK 只是 Rust 重导出，接入必须重编 zenix）。
- **AI 与插件互通**：AI 能调插件动作，插件能调 AI 侧窄接口——zenix 相对 herdr-gpui 的差异面。

## 已确认事实

| # | 事实 | 证据 |
|---|---|---|
| F1 | zenix 现有"插件 SDK"= Rust 库再导出，接入需重编 zenix | [lib.rs](src/lib.rs) |
| F2 | herdr 插件 v1 已成体系：manifest 六类（build / startup / actions / events / panes / link_handlers）+ 运行时 `plugin.{link,list,unlink,enable,disable}`、`plugin.action.{list,invoke}`、`plugin.pane.{open,focus,close}`、`plugin.log.list` | `herdr v0.9.3: src/api/schema.rs:230-251`、`src/api/schema/plugins.rs` |
| F3 | 插件 API = 整个 herdr CLI（无独立 SDK），入口 `HERDR_BIN_PATH` | `v0.9.3:docs/next/website/src/content/docs/plugins.mdx` |
| F4 | 官方原文：**"Runtime action registration and native non-terminal plugin UI are not part of plugin v1."** → 原生非终端 UI 是空白 | 同上 plugins.mdx |
| F5 | herdr 明确不评审、不沙箱插件（插件以用户身份运行、可调全部 CLI） | 同上（Trust and security 节） |
| F6 | 插件 pane 是 **argv 进程**，`placement` = `Overlay`（默认）/ `Popup` / `Split` / `Tab` / `Zoomed`；`plugin.pane.open` 接受 `width/height`(PopupSize)、cwd、env；带出 `PluginPaneInfo{plugin_id, entrypoint, pane}` | `v0.9.3:src/api/schema/plugins.rs:418-469`、`src/app/api/plugins/panes.rs` |
| F7 | 插件运行期拿到 `HERDR_PLUGIN_ROOT` / `HERDR_PLUGIN_CONFIG_DIR` / `HERDR_PLUGIN_STATE_DIR`（每插件独立 config/state 目录） | `v0.9.3:src/app/api/plugins/env.rs` |
| F8 | 调用插件 action 时注入完整上下文：workspace/label/cwd、worktree、tab、focused pane(cwd/agent/status)、`selected_text`、`clicked_url`、`link_handler_id`、correlation_id；action 可按 `contexts`（global/workspace/tab/pane/selection）过滤 | `v0.9.3:src/app/api/plugins/context.rs`、`src/api/schema/plugins.rs:355-409` |
| F9 | zenix 侧已有 MCP 桥与 AI 编排可承接桥接 | [mcp/server.rs](src/mcp/server.rs)、[ai/tools.rs](src/ai/tools.rs) |
| F10 | 上下文传递通道 = **JSON 进环境变量 `HERDR_PLUGIN_CONTEXT_JSON`**（非 stdin / argv） | `v0.9.3:src/app/api/plugins/{mod,runtime}.rs` + `plugins.mdx:251-256` |
| F11 | manifest 的 action **无键位字段**（herdr 键位配置只管它的终端客户端）→ 键位映射只能归 zenix | `v0.9.3:src/app/api/plugins/manifest.rs` | 

## 技术决策

- **PD1 = herdr 插件 v1 是唯一插件契约**（对应 state.json D4，公子 2026-10-12 同意方案 A）。zenix 不自建 manifest / 安装 / 校验 / 信任存储；插件 API 就是整个 herdr CLI，生态与市场已存在，重复造只会分裂生态。空白恰在 UI 层（F4），补它才是差异化。
- **PD2 = UI 贡献的声明位是插件目录下的可选 `zenix-ui.toml`**（由 PD1 推出）。绝不往 `herdr-plugin.toml` 塞私有字段（保 herdr 侧与市场可用性）；未提供该文件的插件仍是合法插件，只是没有 UI 贡献。
- **PD3 = 表达力 L1 结构级声明式 + L3 自绘逃逸档，不做 L2**（对应 state.json D5，公子 2026-10-12 同意推荐）。
  理由：① 插件本来就是 argv 程序（F3），"画自己的界面"最自然的输出就是终端绘制，L3 表达力上限 = 任意像素，不需要白名单；② L2 的组件白名单 = 长期兼容负担（插件声明的组件在新版 zenix 被删/改名怎么办）+ schema 版本协商，等于维护第二个 UI 框架；③ 细腻原生交互做进 zenix 内置能力收益更高。
  正向副作用：M8 因此不需要组件白名单与版本协商，比含 L2 省约 2 会话。
- **PD4 = 内置贡献与插件贡献同形状、同通道 context**（PLAN 硬约束 9）。字段对齐 F8，通道对齐 F10（`HERDR_PLUGIN_CONTEXT_JSON`）。内置命令收同一个 Rust 类型，所以 AC5 可断言，不是文档约定。
- **PD5 = 贡献点注册表在 M4 落地，M8 才接插件**。M4 至少要有一个内置消费者走注册面（自证地基），否则"内置与插件同路"未经验证就扩大范围。
- **PD6 = 范围 = 方案 2**（对应 state.json D6，公子 2026-10-12 定）：**M8 只做插件面**（插件原生宿主 + UI 贡献点/自绘逃逸档 + 正向 MCP 桥 + 信任审查面）；其余候选（插件事件流可视化、布局即代码、AI 编排驾驶舱、反向 MCP、多机驾驶舱）全部后置到 **M9**（≈3 会话），多机驾驶舱列 B 档点名才做。
  理由：M8 要验证的核心命题是"插件架构是否成立"，混入独特功能会稀释验证；且后置项都依赖 M8 的注册面与 M4 的壳，顺序上本来就得排在后面。

## Requirements

- **R1 贡献点注册表**：侧栏项 / 命令 / 面板 / 状态栏条目 / 设置分节 / MCP 工具一律经注册面装配；内置与插件同路；代码不留硬编码清单。
- **R2 herdr 插件原生宿主**：`plugin.list` → 按 `contexts` 过滤 action 进命令面板/侧栏并绑键位（键位来自 `zenix-ui.toml`，见 F11）；`plugin.action.invoke` 传完整 context；`plugin.pane.open` 的 Overlay/Popup pane 走 zenix 原生悬浮渲染，Split/Tab/Zoomed 交回 daemon 布局；`plugin.log.list` → 插件日志面板；`PluginPaneInfo` 用于记账插件 pane。
- **R3 UI 贡献点（L1）**：插件经 `zenix-ui.toml` 声明式贡献面板 / 状态条目 / 设置分节 / 命令，宿主用 gpui-kit 组件渲染（宿主渲染 = 安全 + 原生观感）。schema v1 见 `design.md` §2.2。
- **R4 自绘逃逸档（L3）**：需要完全自定义外观的插件复用 pane 渲染管线——插件进程输出 ANSI/图形，宿主当面板/悬浮层画；不引入新的渲染协议（依据 F6）。接线见 `design.md` §2.3。
- **R5 MCP 桥（v1 仅正向）**：插件 action → MCP 工具，AI 可调用插件能力。反向（插件调 AI 侧能力）后置 M9，且收窄为 `zenix agent send` 窄接口，不开放任意工具（`design.md` §4）。
- **R6 信任审查面**：安装/链接前预览 manifest 与将执行的 argv、来源与 commit 锁定；运行期插件日志与退出码可见（对应 F5：不做沙箱，只做可见性）。

## Acceptance Criteria

- [ ] AC1 至少 1 个真实 herdr 插件在 zenix 内 action 可点、pane 可看、日志可查
- [ ] AC2 至少 1 个 `zenix-ui.toml` 声明式贡献点由宿主渲染成功（非插件注入代码）
- [ ] AC3 MCP 侧能列出并调用该插件 action（正向；反向调 AI 侧能力属 M9 验收）
- [ ] AC4 新增一个内置面板时无需改动插件层代码（反向证明注册面成立）
- [ ] AC5 同一 action 从"命令面板调用"与"插件触发"两条入口收到同形状 context（PD4 可断言）

## 明确不做

- 不做第二套插件安装 / 清单校验 / 信任存储（复用 herdr 的 link/install 与 registry，避免生态分裂）
- 不做插件沙箱（与 herdr 立场一致，只做可见性与审查）
- 不做 wasm / Rust dylib 插件 ABI（Rust ABI 不稳定，收益不明）
- 不在 `herdr-plugin.toml` 里塞 zenix 私有字段（保 herdr 侧兼容）
- 不自造动作系统：贡献点的可点项一律绑定 herdr action id
- 不做组件白名单（L2 已由 PD3 排除），因此不承担"组件被删"的兼容策略

## 开放问题

**无。** planning 就绪。

`task.py start` 的唯一硬门禁：**M2 / M3 / M4 完成**（M8 三段都依赖 M4 的注册面与 M3 的渲染/输入能力），见 `implement.md` 末节检查表。
