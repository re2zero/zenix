# 实施计划（M8 插件扩展面）

状态：**计划就绪（planning 完成）**；`task.py start` **未执行**——M8 三段均硬依赖 M4（贡献点注册面），现在翻 `in_progress` 会让后续会话误判"正在实现插件面"。
依赖门禁：**M2**（herdr 门面：`plugin.*` / `agent.*` / `events.*` JSON API + endpoint snapshot）· **M3**（pane surface 绘制 + 语义输入 + cell 尺寸）· **M4**（`ContributionRegistry` + 命令面板 + 侧栏/状态栏/设置容器 + 键位表 + context 采集器）全部完成后才 `task.py start`。
设计依据：`design.md`（定稿）· 需求与验收：`prd.md`（AC1–AC5）

## 顺序（三段 = 三次会话，各自可独立验收）

### M8a herdr 插件原生宿主（1 会话）

| # | 动作 | 文件锚点 |
|---|---|---|
| 1 | `plugin.*` API 封装（list / action.list / action.invoke / pane.{open,focus,close} / log.list），方法名与参数对照 `herdr api schema --json` 实测 | `src/herdr/plugin_api.rs`（新） |
| 2 | 缓存与失效：启动拉取 → `InstalledPluginInfo` 缓存；`plugin.*` 变更后失效；能订阅事件则订阅，否则定时兜底 | `src/herdr/plugin_api.rs`、`src/herdr/mod.rs` |
| 3 | action → 注册面（`ContributionKind::Command`，id = `plugin.<plugin_id>.<action_id>`），按 `action.contexts` 过滤可见性 | `src/plugins/registry.rs`（新） |
| 4 | 键位映射：读 `zenix-ui.toml` 的 `key`（若存在）→ 键位表；设置页可改 | `src/plugins/ui_manifest.rs`、`src/app/keys.rs` |
| 5 | 插件 pane 记账：`PluginPaneInfo{pane_id → (plugin_id, entrypoint)}`；`PaneInfo` 不带来源字段，必须自记 | `src/plugins/panes.rs`（新） |
| 6 | `Overlay`/`Popup` placement → zenix 悬浮层；`Split`/`Tab`/`Zoomed` → 交回 daemon 布局 | `src/workspace/overlay.rs` |
| 7 | 插件日志面板：`plugin.log.list{plugin_id, limit}` | `src/panels/plugin_log.rs`（新） |

**验收**：真插件 action 可点、pane 可看、日志可查（截图 + 日志留证）；方法缺失时按 `Connected` 事件里的 `methods` 列表降级（`advertised_in`）。

### M8b UI 贡献点 + 自绘逃逸档（1 会话）

| # | 动作 | 文件锚点 |
|---|---|---|
| 1 | `zenix-ui.toml` 解析：`zenix_ui_version` / `min_zenix_version` 门 + 未知字段容忍 + 未知 `view`/`placement` 降级 | `src/plugins/ui_manifest.rs` |
| 2 | 贡献点装配：`command` / `sidebar_item` / `panel` / `status_item` / `settings_section` → 注册面 | `src/plugins/registry.rs` |
| 3 | 内置视图种类渲染：`list` / `table` / `kv` / `log` / `form`（gpui-kit 组件；先查 registry 源码的真实 API，不发明） | `src/panels/views/*.rs`（新） |
| 4 | 数据源：`command`（stdout JSON，5s 超时，失败显示错误卡片）/ `event`（订阅）；**必须 `background_spawn`，不得阻塞渲染** | `src/panels/source.rs`（新） |
| 5 | 自绘逃逸档：overlay pane 视图接入 M3 painter + 输入转发 + cell 尺寸变化触发重排 | `src/workspace/overlay.rs`、`src/terminal/painter/` |

**验收**：自制最小插件声明 1 panel(table) + 1 status_item + 1 command 全部渲染成功；另 1 个自绘插图画 ANSI 图并接受输入（截图留证）。

### M8c MCP 正向桥 + 信任审查面（1 会话）

| # | 动作 | 文件锚点 |
|---|---|---|
| 1 | action → MCP tool：名称映射层（`plugin.<id>.<action>` → 允许字符集，`__` 转义 + 反向映射表）；**先实测 rmcp 的命名约束** | `src/mcp/plugin_tools.rs`（新） |
| 2 | 入参 schema：可选 context 覆盖字段；返回 stdout + 退出码 | 同上 |
| 3 | 安装/链接前预览：manifest 全量 argv 展开（build/startup/actions/events/panes/link_handlers）+ `PluginSourceInfo` + 高危标记（绝对路径 / 网络下载 / 写 HOME 外 / sudo） | `src/panels/plugin_trust.rs`（新） |
| 4 | 运行期：日志与退出码 + 本地记录（上次运行时间 / 失败次数，存 zenix config） | `src/plugins/state.rs`（新） |

**验收**：MCP 客户端 `list_tools` 能看到插件工具并调用成功；预览页对含 `bash -c` 的测试 manifest 给出高危标记。

## 验证命令（每段收尾必跑）

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --lib
cargo run --bin zenix-probe            # M2 起的 daemon 冒烟（含 plugin.list 摘要）
# MCP：rmcp 客户端 smoke（list_tools + call_tool）
# 视觉：Xvfb :97 截图 → /tmp/zenix-shots/（路径写进 HANDOFF）
```

## 风险文件与回滚点

- 改动集中在 `src/plugins/**` + `src/panels/**` + 注册表接线；**回滚 = 删这两目录 + 摘接线**，不留半成品开关。
- 不碰 `herdr/` 子模块与 `vendor/`（只读依赖）；不为 M8 改 `src/ai` 与 MCP 的既有对外契约。
- 三处待实测（实现时先做）：rmcp 工具名约束 · plugin 变更是否可订阅事件 · overlay pane 尺寸变化的重排触发点。

## `task.py start` 前检查

- [x] `design.md` 定稿（126 行）
- [x] `prd.md` 收敛（Q1/Q2/Q3 全部折叠为 D1–D6）
- [x] `.relay/PLAN.md` 的 M8 范围/预算与本文一致
- [ ] **M2 / M3 / M4 完成**（硬门禁；未完成前不 `start`）
