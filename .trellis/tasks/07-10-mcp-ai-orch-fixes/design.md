# 修复 MCP 显示/AI 语言/编排调度 3 个遗留问题 — 技术设计

## 问题 1: 内置 MCP 服务在设置面板中不可见

### 根因分析

`src/app.rs` 中 `refresh_mcp_data()` 函数的逻辑：

```rust
fn refresh_mcp_data(&mut self, cx: &mut gpui::Context<Self>) {
    let mut servers = crate::mcp::load_zenix_mcp_servers();
    servers.retain(|s| s.name != "zenix-orchestrator");   // ← 先移除
    if self.config.mcp_server_sse_enabled() {               // ← 条件性重加
        let builtin = crate::mcp::ensure_builtin_orchestrator();
        servers.insert(0, builtin);
    }
    self.mcp_zenix_servers = servers;
    // ...
}
```

**根因**: 当 `mcp_server_sse_enabled()` 返回 `false`（默认值）时，`zenix-orchestrator` 被移除后永远不会被重新加入显示列表。

### 修复方案

1. **始终显示内置服务**: 无论 SSE 是否启用，都在列表中保留 `zenix-orchestrator`。SSE 标记仅控制是否启动 SSE listener，不影响 UI 显示。
2. **显示服务状态**: 在 MCP 卡片上显示内置服务的运行状态（stdio 运行时状态、socket 路径）。

### 变更范围

| 文件 | 变更 |
|------|------|
| `src/app.rs` | `refresh_mcp_data()` 无条件保留 `zenix-orchestrator` |
| `src/ui/settings/components.rs` | 可选：MCP 卡片显示内置服务标识 |
| `src/ui/settings/tabs.rs` | 可选：MCP tab 显示内置服务说明 |

## 问题 2: AI 回复语言与输入不匹配

### 根因分析

- 系统 prompt 硬编码为"用中文回复。"（`src/ai/engine.rs` 的 `DEFAULT_SYSTEM_PROMPT`）
- 没有输入语言检测机制
- 没有根据用户输入语言动态调整系统 prompt 的逻辑
- UI locale 设置（en/zh-CN）仅影响界面翻译，不影响 AI 系统 prompt

### 修复方案

**方案: 动态语言自适应**

1. 添加语言检测工具函数，使用简单的 Unicode 范围检测判断输入语言（中文 / 英文 / 其他）
2. `build_system_prompt()` 接受语言参数，根据检测结果设置回复语言指令
3. 在用户消息发送时检测语言，传递给 system prompt 构建

```rust
fn detect_language(text: &str) -> &'static str {
    if text.chars().any(|c| c >= '\u{4e00}' && c <= '\u{9fff}') {
        "zh"  // 中文
    } else {
        "en"  // 英文
    }
}

fn build_system_prompt(..., lang: &str) -> String {
    let lang_instruction = match lang {
        "zh" => "用中文回复。",
        _ => "Reply in the same language as the user's input.",
    };
    // ...
}
```

### 变更范围

| 文件 | 变更 |
|------|------|
| `src/ai/engine.rs` | `build_system_prompt()` 加 `lang` 参数；添加语言检测函数 |
| `src/ai/chat/mod.rs` | 调用 `chat_with_tools` 时传入检测到的语言 |

## 问题 3: 编排功能不能正确分发"代为询问"任务

### 根因分析

- 编排器的 task dispatch 完全依赖 LLM 的 function calling，**没有硬编码规则**
- 系统 prompt 没有明确指导 LLM 如何处理"帮我问一下 X"这类委托模式
- LLM 可能将"帮我问一下 opencode 以下问题"误解为普通对话，而不是需要 spawn_worker 的信号
- 没有 pre-processing 层来识别明显的委托意图

### 修复方案

**方案 A（系统 prompt 增强）**: 在系统 prompt 中增加明确的委托模式识别指令

**方案 B（pre-processing）**: 在将消息提交给 LLM 之前，先做模式匹配检测，如果匹配"帮我问一下 X"模式，自动插入 spawn_worker 工具调用的提示

**采用方案 A + 选择性 B**:

1. **增强系统 prompt**：添加明确的委托模式处理指令
2. **Pre-processing 辅助**：检测明确委托模式时，在消息前附加辅助提示（不是自动执行工具，而是引导 LLM 使用工具）

### 变更范围

| 文件 | 变更 |
|------|------|
| `src/ai/engine.rs` | 增强 `DEFAULT_SYSTEM_PROMPT` 和 `build_system_prompt()` 中的委托处理指令 |
| `src/ai/chat/mod.rs` | 可选：消息发送前检测委托模式，附加引导提示 |

## 架构影响

- 无新增依赖
- 无模块结构变更
- 三个问题均为独立修改，不影响现有功能

## 兼容性

- 内置 MCP 显示变更不影响 MCP Server 启动逻辑
- 语言检测不改变已有 API provider 配置
- 编排 prompt 增强兼容现有工具定义，不影响 workers 状态管理
