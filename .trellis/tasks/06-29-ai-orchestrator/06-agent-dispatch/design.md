# 编排器调度逻辑 — 设计

## 架构

调度逻辑在 `OrchestratorEngine` 中扩展。核心新增方法是 `chat_and_dispatch()`——它将 `chat_stream()` + `parse_agent_tasks()` + `dispatch_all()` + `summarize_results()` 封装为一个完整的编排周期。

```
chat_and_dispatch()  ← 任务 04 唯一需要调用的方法
  ├─ chat_stream(messages, system_prompt, on_event)
  │    → LLM SSE stream → Token 事件回调（推送到 UI 流式渲染）
  │    → LLM stream 结束后，收集完整回复字符串
  │
  ├─ parse_agent_tasks(reply) → Vec<AgentTask>
  │
  ├─ if !tasks.is_empty():
  │    ├─ dispatch_all(tasks, herdr, on_event)
  │    │    → emit AgentDispatch / AgentOutput / AgentComplete 事件
  │    │    → 调用方（任务 04）在收到 AgentComplete 时更新 UI
  │    ├─ summarize_results(original_request, results) → 汇总文本
  │
  ├─ 返回: ChatAndDispatchResult { orchestator_text, agent_results, summary }
  └─ 调用方负责: 持久化 assistant message（含 agent_activities）到 SQLite
```

## System Prompt 常量

```rust
// src/ai/engine.rs

pub const ORCHESTRATOR_SYSTEM_PROMPT: &str = r#"
你是 ZenixAI 编排器——智能任务调度中心。

## 核心职责
1. 理解用户需求，分解为可独立执行的子任务
2. 将子任务分配给合适的 Agent CLI 执行
3. 监控执行进度，处理异常
4. 汇总执行结果，给用户简洁的回复

## Agent CLI 清单
- claude: 适合复杂开发、代码生成、调试、设计
- opencode: 适合配置管理、简单变更、代码检查
- codex: 通用任务、文件操作

## 调度规则
- 你需要决定何时将任务派发给 agent，以及派发给谁
- 如果需要派发 agent，在回复中包含以下格式标记：
  <dispatch agent="claude" cwd="/path">
  给 agent 的完整指令
  </dispatch>
- 每次只派发一个 agent，串行执行
- 等待 agent 完成后，再决定下一步

## 输出要求
- 回复使用中文
- agent 执行结果用简洁语言总结
- 不要自己写代码——你是调度者，不是执行者
"#;
```

## Agent Task 解析

```rust
/// 从编排器的文本回复中解析 dispacth 标记
fn parse_agent_tasks(reply: &str) -> Vec<AgentTask> {
    let mut tasks = Vec::new();
    // 匹配 <dispatch agent="..." cwd="...">...</dispatch>
    let re = regex::Regex::new(
        r#"<dispatch\s+agent="([^"]*)"(?:\s+cwd="([^"]*)")?\s*>(.*?)</dispatch>"#
    ).unwrap();

    for cap in re.captures_iter(reply) {
        tasks.push(AgentTask {
            agent_name: cap[1].to_string(),
            instruction: cap[3].trim().to_string(),
            cwd: cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
            pane_id: None,
        });
    }
    tasks
}
```

## Dispatch 执行

```rust
impl OrchestratorEngine {
    /// 执行 agent 任务调度
    pub async fn dispatch_all(
        &self,
        tasks: &[AgentTask],
        herdr: &mut HerdrIpcClient,
        on_event: &mut impl FnMut(StreamEvent),
    ) -> Vec<AgentResult> {
        let mut results = Vec::new();

        for task in tasks {
            on_event(StreamEvent::AgentDispatch(task.clone()));

            match self.dispatch_single(task, herdr, on_event).await {
                Ok(result) => {
                    on_event(StreamEvent::AgentComplete(result.clone()));
                    results.push(result);
                }
                Err(e) => {
                    on_event(StreamEvent::Error(format!(
                        "Agent '{}' failed: {e}", task.agent_name
                    )));
                    // 错误处理由 10-error-handling 完善
                }
            }
        }

        results
    }

    /// 执行单个 agent 任务（create pane → start → send → wait → read）
    async fn dispatch_single(
        &self,
        task: &AgentTask,
        herdr: &mut HerdrIpcClient,
        on_event: &mut impl FnMut(StreamEvent),
    ) -> Result<AgentResult> {
        // 1. 获取 workspace
        let workspaces = herdr.list_workspaces()?;

        // 2. 找匹配的 workspace（按 cwd 匹配，或使用第一个）
        let workspace = // ... find by cwd or default ...

        // 3. 创建 pane
        let pane = herdr.create_pane(&workspace.id, &task.cwd)?;

        // 4. 启动 agent
        herdr.start_agent(&pane.id, &task.agent_name)?;

        // 5. 发送指令
        herdr.send_text(&pane.id, &task.instruction)?;

        // 6. 等待 agent 完成（匹配完成标记）
        match herdr.wait_for_output(&pane.id, "task complete", 300_000) {
            Ok(output) => {
                // 7. 读取完整输出
                let full = herdr.read_output(&pane.id, "recent")?;
                Ok(AgentResult {
                    agent_name: task.agent_name.clone(),
                    pane_id: pane.id,
                    exit_status: "completed".into(),
                    output: full,
                    summary: None,
                })
            }
            Err(e) => {
                // 超时或失败
                Ok(AgentResult {
                    agent_name: task.agent_name.clone(),
                    pane_id: pane.id,
                    exit_status: "failed".into(),
                    output: e.to_string(),
                    summary: None,
                })
            }
        }
    }
}
```

## 结果汇总

```rust
impl OrchestratorEngine {
    /// 汇总所有 agent 执行结果
    async fn summarize_results(
        &self,
        original_request: &str,
        results: &[AgentResult],
    ) -> Result<String> {
        let summary_prompt = format!(
            "用户需求：{original_request}\n\nAgent 执行结果：\n{}",
            results.iter().map(|r| {
                format!("--- {} ({}) ---\n{}", r.agent_name, r.exit_status, r.output)
            }).collect::<Vec<_>>().join("\n\n")
        );

        self.chat_simple(&summary_prompt, "用中文简洁总结 agent 的执行结果").await
    }
}
```

## 返回结果

```rust
/// `chat_and_dispatch()` 的返回类型
pub struct ChatAndDispatchResult {
    /// 编排器的文字回复（不含 dispatch 标记的纯文本）
    pub orchestrator_text: String,
    /// 所有 agent 的执行结果（每个 AgentResult 会映射到 AgentActivity 持久化）
    pub agent_results: Vec<AgentResult>,
    /// 编排器对 agent 结果的汇总（第二次非流式调用的回复）
    pub summary: Option<String>,
}
```

## 持久化说明

agent 输出的持久化不由本任务处理。`chat_and_dispatch()` 返回 `ChatAndDispatchResult`，调用方（任务 04 的 `on_send()`）负责：

1. 构造 `ChatMessage`（role="orchestrator", content = orchestrator_text + summary, agent_activities = 从 agent_results 转换）
2. 调 `SessionManager::add_message()` 写入 SQLite

```rust
// 转换关系（在任务 04 中实现）
fn agent_results_to_activities(results: &[AgentResult]) -> Vec<AgentActivity> {
    results.iter().map(|r| AgentActivity {
        agent_name: r.agent_name.clone(),
        status: r.exit_status.clone(),   // "completed" | "failed"
        summary: r.summary.clone().unwrap_or(r.output.chars().take(200).collect()),
        started_at: None,  // 由 dispatch_single 内部记录
        completed_at: None,
    }).collect()
}
```

## 替换任务 03 的占位类型

任务 03 在 `engine.rs` 中定义了 `AgentTask` / `AgentResult` 占位结构体（`#[allow(dead_code)]`）。
本任务需移除这两个占位结构体的 `#[allow(dead_code)]` 并补全方法：

- `AgentTask` → 补充 `Display` impl，方便日志
- `AgentResult` → 新增 `to_activity()` 方法转为 `AgentActivity`

移除 `#[allow(dead_code)]` 后，本任务的 dispatch 逻辑会正确使用它们，`StreamEvent` 的 Agent* variant 也不再是 dead code。

## Requirements & Dependencies

- 依赖 `regex` crate（已在任务 03 的 Cargo.toml 中添加）
- 依赖 `HerdrIpcClient`（任务 05）
- 扩展 `src/ai/engine.rs`：增加 `chat_and_dispatch()` + `dispatch_all()` + `dispatch_single()` + `parse_agent_tasks()`
- 移除任务 03 设置的 `#[allow(dead_code)]` 标注
- dispatch agent 的操作在 `cx.background_executor()` 中执行
- 返回结果不持久化——调用方（任务 04）负责写入 SQLite
