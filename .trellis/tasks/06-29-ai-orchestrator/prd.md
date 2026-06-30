# 多Agent编排调度 — AI会话侧边栏

## Goal

在 zenix 中实现多 agent 编排调度功能：新增 AI 会话侧边栏，通过轻量级编排 LLM（本地小参数模型）负责任务分解和调度决策，将具体执行任务分发给配置了大参数模型的 Agent CLI（如 Claude Code、Codex、OpenCode 等），每个 Agent 运行在独立的 herdr plane 中。

## 背景

- herdr 通过 Unix socket JSON-line API 暴露完整的 pane 操作：`pane.split`（创建 plane）、`pane.read`（读输出）、`pane.send_text`（写输入）、`pane.wait_for_output`（等待输出匹配）、`agent.start`（启动 agent 进程）、`events.subscribe`（事件订阅）、`pane.report-agent`（agent 状态自报告）
- zenix 已有 `ApiProviderConfig`（支持 OpenAI/Anthropic 格式的 LLM API 配置）、`ProviderRegistry`（8 种已知 Agent CLI）、`ConfigFile`（应用配置）、侧边栏面板注册模式（`settings_panel`、`agent_panel` 等 `Option<Entity<...>>` 模式）
- zenix 当前 HTTP 客户端仅 `ureq`（同步），AI SSE 流式响应需要添加异步 HTTP 支持

## 已确认的产品形态

### 1. 交互模型：混合模式（聊天 + 实时 plane 监控面板）

用户通过右面板 AI 聊天界面描述需求，配合实时状态监控：

- **主交互**：聊天式 — 用户输入 → 编排器 LLM 规划 → 分发 Agent → 汇总结果 → 回复
- **辅视图**：plane 监控面板 — 展示每个 agent 的运行状态、实时输出、历史记录
- 用户可点进某个 plane 查看完整输出，获得对执行过程的完全可见性

### 2. 编排器模型配置：双模（默认 API + 可切本地）

- 默认使用 API 小模型（GPT-4o-mini / Claude Haiku）做编排
- 可切换到本地模型（Ollama 等 OpenAI-compatible 端点）
- 模型从 Settings → 扩展 → API Providers 中选取
- 编排器不直接写代码/跑测试 — 只负责规划、调度、汇总

### 3. API 提供商的双重角色

zenix 的 API Providers 配置不仅是编排器的模型来源，也是所有 Worker Agent CLI 的**统一 API 管理入口**：

- 用户在一个地方管理 API key
- zenix 通过 `config_adapters/` 基础设施自动将 API 配置同步到各 Agent CLI（Claude Code → `~/.claude.json`、Codex → env/config、OpenCode → `opencode.jsonc`）
- 编排器用 API Provider 列表选模型
- Worker Agent CLI 各自读取更新后的配置

### 4. UI 布局

- AI 会话位于**右面板**（复用现有 right tabs），作为一个新的 AI tab
- 底部栏四按钮：**[+ 新会话]** **[📋 历史]** **[👤 运行中 N]** **[⚙]**
  - **"+"** → 新建会话
  - **"📋"** → 弹出历史会话列表
  - **"👤 运行中 N"** → 展开活跃 agent 实时状态浮层（状态 + 最近输出）
  - **"⚙"** → 当前会话设置覆盖
- 配置栏位于输入框和底部按钮之间：模型选择 ▏思考度 ▏workspace 选择
- Agent 输出以内联折叠区块嵌入聊天流，完成后默认折叠
- 右面板宽度可拖拽调整

### 5. 工作目录模型

不新增 zenix 项目管理，直接复用 **herdr workspace**：

- 编排器从 herdr 读取现有 workspaces（每个 workspace 有 CWD 和 label）
- 选择 workspace → 所有 Agent CLI 在该 workspace 的 CWD 下执行
- 编排器通过 IPC 在对应 workspace 中创建 pane / 启动 agent
- workspace 标签名就是用户在编排器中选择的项目名

### 6. 异常处理策略

| 场景 | 编排器行为 |
|------|-----------|
| Agent CLI 启动失败 / 二进制不存在 | 错误上报用户（提示安装） |
| 编排器 LLM 调用失败（API 断连/超时） | 上报用户，建议检查 API 配置或换 provider |
| Agent 执行失败或需要进一步处理 | 编排器自行智能处理（重试、换 agent、修正指令） |
| 用户中途切换会话 | 当前 agent 继续保持后台运行 |

### 7. 多会话支持

- 支持创建多个命名会话
- 每个会话有独立的聊天历史和关联项目
- 会话切换时，旧会话的 agent 后台继续运行（不中断）

### 7. 数据持久化

- **存储引擎**：SQLite（每个会话一条消息流记录）
- **全文搜索**：FTS5 索引 `messages.content`，支持实时搜索用户输入和历史消息
- **搜索结果**：点击即跳转到对应消息位置，恢复上下文

#### 会话生命周期

- **创建**：自动新建，编排器根据用户第一条输入智能命名（如"jwt token 刷新实现"）
- **存储上限**：无上限（预计数量不大）
- **删除**：删除当前会话后，自动切到新会话
- **活跃状态**：对应 workspace 存在则标记活跃，workspace 删除后标记孤立

## Requirements

- [x] 交互模型：聊天 + 实时 plane 监控（已确认）
- [x] 编排器模型：双模（默认 API + 可切本地）（已确认）
- [x] API Providers 双角色：既是编排器模型来源，也同步到 Agent CLI（已确认）
- [x] UI 布局：右面板 AI tab + 底部按钮栏（已确认）
- [x] 工作目录：复用 herdr workspace CWD（已确认）
- [x] 异常策略：致命错误报用户，agent 失败编排器自处理（已确认）
- [ ] 搜索功能：支持 FTS5 全文搜索 + 跳转定位（已讨论，待设计细化）
- [ ] Agent CLI 配置同步：config_adapters 实现多 agent 分发（设计阶段细化）

## Out of Scope（初版）

- 跨会话拖拽消息
- Agent CLI 配置的 UI 编辑器
- 多模态输入（图片/文件上传）

