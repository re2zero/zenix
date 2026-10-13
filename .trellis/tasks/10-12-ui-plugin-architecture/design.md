# 插件扩展架构设计（M8 技术设计）

状态：**设计定稿（D1–D5 已定）**；仅第 8 节"待 Q3 决定"的选项未收敛
关联：`.relay/PLAN.md`（硬约束 4/9、M4 地基、M8 实现）、`prd.md`（需求 R1–R6 / 验收 AC1–AC5）
取证：herdr v0.9.3 源码与文档（全部走 `git -C herdr show v0.9.3:<path>`）

## 1. 边界：谁负责什么

| 关注点 | 归属 | 依据 |
|---|---|---|
| 插件来源 / 安装 / manifest 校验 / 日志留存 / 信任存储 | **herdr daemon** | F2、F3 |
| 插件代码执行、依赖、持久化状态 | **插件进程自己**（`HERDR_PLUGIN_STATE_DIR`） | F7 |
| 界面渲染、调用上下文装配、命令面板/键位、MCP 暴露、安装前审查预览 | **zenix 宿主** | 本设计 |

结论：zenix 不加插件格式、不做安装器、不做权限系统；只做"宿主面"。这是 D1 的落地含义。

## 2. 三层宿主能力

### 2.1 插件原生宿主（R2）

- **数据源**：`plugin.list` → `InstalledPluginInfo`（含 actions / events / panes / link_handlers / warnings / source），`plugin.action.list` → `PluginActionInfo`。启动拉一次缓存；`plugin.*` 任何变更操作后失效重取；另订阅 herdr 事件以捕获外部变更（事件是否覆盖 plugin 变更**待实测**，未覆盖则退化为手动刷新 + 定时兜底）。
- **贡献 id**：`plugin.<plugin_id>.<action_id>`，直接复用 daemon 的 `qualified_id()` 语义，不另造命名空间。
- **可见性过滤**：按 `action.contexts`（global/workspace/tab/pane/selection）与当前焦点状态过滤；`selection` 类只在有选区时出现在命令面板，避免噪声。
- **调用**：`plugin.action.invoke{ action_id, plugin_id, context }`，`context` 为 zenix 采集的当前上下文（见 §3）。
- **键位**：manifest **没有** action 键位字段（herdr 的键位配置在 daemon/终端客户端侧），因此键位映射归 zenix：放在 `zenix-ui.toml` 的 `key` 字段（见 2.2），也在 zenix 设置里可改。
- **pane**：`plugin.pane.open` 的 `Overlay` / `Popup` 由 zenix 画在悬浮层（见 2.3）；`Split` / `Tab` / `Zoomed` 交回 daemon 的布局树，走 M3 的原生渲染。
- **日志**：`plugin.log.list{plugin_id, limit}` → 插件日志面板（配合 R6）。

### 2.2 UI 贡献点（R3，**表达力 = L1 结构级声明式**）

**声明位**：`<plugin_root>/zenix-ui.toml`（可选文件；herdr 不读不校验，缺失即"无 UI 贡献"）。

**schema v1 草案**：

```toml
zenix_ui_version = 1                 # schema 版本，未知大版本 → 跳过并告警
min_zenix_version = "0.2.0"          # 低于当前 zenix 则拒绝装配该文件

[[command]]                          # 命令面板条目 / 键位
id = "apply"                         # 插件内本地 id（ascii 字母数字 _ -）
title = "Apply project layout"
action = "apply"                     # 指向 herdr-plugin.toml 里的 action id
when = ["workspace"]                 # 复用 context 名，控制可见性
key = "ctrl-shift-l"                 # 可选，zenix 侧键位

[[sidebar_item]]
id = "board"
title = "Project board"
icon = "layout"                      # gpui-kit IconName
order = 50
panel = "board"                      # 指向下面的 [[panel]]

[[panel]]
id = "board"
title = "Project board"
placement = "drawer"                 # overlay | drawer | bottom
view = "table"                       # 内置视图种类：list | table | kv | log | form
[panel.source]
command = ["node", "dist/board.js"]  # 二选一：取 stdout JSON
# event = "worktree.created"         # 或订阅 socket 事件

[[status_item]]
id = "wip"
source = { command = ["node", "dist/wip.js"] }
format = "{count} WIP"

[[settings_section]]
id = "general"
title = "Layout"
[[settings_section.field]]
key = "theme"
label = "Theme"
type = "string"                      # string | bool | number | enum + options
```

**约束**：

- 贡献点只承载**展示 + 触发**：可点项一律绑定 herdr action id，不自造动作系统（避免第二套执行语义）。
- 幂等 id：全局键为 `<plugin_id>:<kind>:<local_id>`，同键覆盖、乱序装配结果稳定；重复 id 为装配告警而非失败。
- 向前兼容：未知字段容忍；未知 `view` 种类降级为 `kv` 并告警；未知 `placement` 降级为 `drawer`。**不做**组件白名单，因此没有"组件被删"的兼容负担（这正是排除 L2 的收益）。
- 数据获取在宿主侧串行化 + 超时（建议 5s）+ 失败只影响该面板（显示错误卡片，不阻塞内核）。

### 2.3 自绘逃逸档（R4，**表达力 = L3**）

- **机制**：插件以 overlay/popup pane 输出 ANSI/图形，zenix 把该 pane 的 surface 画进悬浮层。**不引入新渲染协议**。
- **接线**（几乎全是 M3 的能力复用）：
  1. 用 `PluginPaneInfo{plugin_id, entrypoint, pane}` 记账"哪些 pane 是插件 pane"（`PaneInfo` 本身不带来源字段，必须由 zenix 自己记账）。
  2. 在 overlay 层挂一个 pane 视图，绘制复用 M3 的 painter + region 缓存。
  3. 尺寸：daemon 通过 `HERDR_CELL_WIDTH_PX` / `HERDR_CELL_HEIGHT_PX` 告知插件；zenix 侧在 cell 尺寸或悬浮层尺寸变化时触发重排（"通知 daemon 重排"的具体触发点**待 M3/M8 联合实测**）。
  4. 输入：复用 M3 的语义输入转发（`pane_id` + `ClientPaneInputEvent`），键盘焦点归属由 zenix 的焦点管理决定。
- **明确不做**：不解析插件私有 UI 协议、不做内嵌 webview、不做 iframe 式沙箱。

## 3. 调用上下文（硬约束 9 的落地）

- **形状**：zenix 定义 `ContributionContext`，字段对齐 daemon 的 `PluginInvocationContext`（F8）：`workspace_id/label/cwd`、`worktree`、`tab_id/label`、`focused_pane_id/cwd/agent/status`、`selected_text`、`invocation_source`、`correlation_id`、`clicked_url`、`link_handler_id`。
- **传递**：与 daemon 保持一致——**JSON 进环境变量 `HERDR_PLUGIN_CONTEXT_JSON`**。调用 herdr 插件时由 daemon 注入（zenix 只填 `context` 参数）；调用 `zenix-ui.toml` 贡献时 zenix 自己注入同名变量，让插件作者只学一套。
- **内置贡献**：内置命令收到**同一个 Rust 类型**，因此"内置与插件同路"可断言（AC5）——不是"文档上约定"，是类型系统保证。
- **采集器**：M4 提供；无焦点/无选区时字段为 `None`，不做伪造默认值。

## 4. MCP 桥（R5）

- **正向（v1 必做）**：插件 action → MCP 工具。工具名需映射为 MCP 允许的字符集，插件 id 含 `.`/`:` 时必须转义为 `__` 并维护反向映射表（**rmcp 的命名约束待实测**，设计上先留映射层）。描述取 manifest 的 `title`/`description`；入参为可选的 context 覆盖字段；返回为 action 命令的 stdout / 退出码。
- **反向（是否进 v1 待 Q3）**：插件调用 AI 侧工具。方案 A：zenix 暴露本地入口（`zenix mcp call <tool> <json>` 子命令或本地 socket），插件经 `HERDR_BIN_PATH` 之外的另一条路径调用；方案 B：只提供"发消息给 agent"的窄接口（`zenix agent send`），不开放任意工具。方案 B 攻击面小、语义清晰，倾向 B。

## 5. 信任审查面（R6）

- **安装/链接前**：从 manifest 展开**全部将执行的 argv**（build / startup / actions / events / panes / link_handlers）+ 来源信息（`PluginSourceInfo`：kind/owner/repo/subdir/requested_ref/resolved_commit）→ 一屏可读预览；高危标记（绝对路径命令、网络下载、写 HOME 之外、`sudo`/提权、`node -e`/`bash -c` 内联脚本）。
- **运行期**：`plugin.log.list` 增量拉取 + 退出码；zenix 本地记录"上次运行时间 / 失败次数"（存 zenix config，不写回 herdr）。
- **边界**：不做沙箱、不做权限系统——与 herdr 立场一致（F5），只提供可见性。

## 6. 与 M2/M3/M4 的接口面（前置依赖清单）

- **M2 必须提供**：herdr JSON API 门面（`plugin.*` / `agent.*` / `events.*` / `layout.*` / `worktree.*`）+ endpoint snapshot 流。
- **M3 必须提供**：把任意 pane surface 画进指定矩形（含 overlay 层）、语义输入转发 API、cell 尺寸与 scroll 状态。
- **M4 必须提供**：`ContributionRegistry`（kind / id / 装配顺序 / 可见性过滤）、命令面板、侧栏容器、状态栏、设置分节容器、快捷键表、`ContributionContext` 采集器。**M4 至少要有一个内置消费者走注册面**（否则地基未被自证）。

## 7. 兼容与回滚

- `zenix-ui.toml` 独立版本号 + 未知字段容忍 + 未知种类降级（不报错、不阻断）。
- 注册表贡献可逐个关闭（配置里 disable 某插件的 UI），装配失败不影响内核启动。
- 回滚点：M8 的改动集中在 `src/plugins/**` + 注册表接线；删除该目录 + 摘掉接线即回到"无插件面"状态，不留半成品开关。

## 8. 待 Q3 决定

1. **反向 MCP**（插件调 AI 侧能力）是否进 v1：推荐窄接口方案 B（`zenix agent send`），不开放任意工具。
2. **待选独特功能的取舍与优先级**：插件事件流可视化 / 布局即代码（herdr `layout.apply`/`layout.export`）/ AI 编排驾驶舱 / 多机驾驶舱（herdr-client 自带 SSH·WSL target）——哪些进 M8 或另立里程碑，哪些进 B 档。
