# 异常处理完善

## 目标

为编排系统建立完整的错误处理机制，涵盖 LLM API 调用、herdr IPC、agent 执行三个层面的异常场景。

## 异常场景分类

### Level 1: 需要用户介入

| 场景 | 表现 | 处理 |
|------|------|------|
| herdr 未运行 | socket 连接失败 | 提示用户启动 herdr，提供启动命令 |
| LLM API Provider 不可用 | HTTP 连接超时 | 提示检查 provider 配置和网络 |
| API key 无效 | HTTP 401/403 | 提示检查 API key |
| 数据库错误 | SQLite 操作失败 | 提示检查磁盘和权限 |

### Level 2: 编排器自主处理

| 场景 | 表现 | 处理 |
|------|------|------|
| Agent CLI 启动失败 | pane 创建后 agent 无响应 | 重试 1 次 → 切换 agent → 上报 |
| Agent 执行超时 | 超过最大等待时间 | 终止 agent，重新分配 |
| Agent 执行失败 | 返回错误输出 | 重新分析错误后重试或分配其他 agent |
| LLM 调用暂时失败 | HTTP 5xx | 自动重试 2 次（指数退避） |
| SSE 流中断 | 连接意外断开 | 请求剩余内容（非流式 fallback）|

### Level 3: 代码异常

| 场景 | 表现 | 处理 |
|------|------|------|
| JSON 解析错误 | 响应格式不匹配 | 日志记录 + 返回错误事件 |
| Socket 断开 | 读写失败 | 标记连接断开，下次自动重连 |
| 内存不足 | 大量 agent 输出 | agent 输出限制（如 100KB 截断）|

## 错误类型定义

```rust
// src/ai/error.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AiError {
    #[error("LLM API error: {0}")]
    ApiError(String),

    #[error("API rate limited: retry after {0}s")]
    RateLimited(u64),

    #[error("Herdr IPC error: {0}")]
    HerdrError(String),

    #[error("Agent '{agent}' failed: {detail}")]
    AgentFailed { agent: String, detail: String },

    #[error("Agent '{agent}' timed out after {timeout}s")]
    AgentTimeout { agent: String, timeout: u64 },

    #[error("Database error: {0}")]
    Database(String),

    #[error("{0}")]
    Other(String),
}
```

## 重试机制

```rust
/// 带指数退避的重试
pub async fn retry_with_backoff<F, T>(
    operation: F,
    max_retries: u32,
    initial_delay: Duration,
) -> Result<T, AiError>
where
    F: Fn() -> futures::future::BoxFuture<'_, Result<T, AiError>>,
{
    let mut delay = initial_delay;
    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt == max_retries {
                    return Err(e);
                }
                // 指数退避: 1s → 2s → 4s → 8s
                smol::Timer::after(delay).await;
                delay *= 2;
            }
        }
    }
    unreachable!()
}
```

## Agent 失败处理

```rust
impl OrchestratorEngine {
    /// 执行 agent 任务，含失败恢复逻辑
    async fn execute_agent_with_fallback(
        &self,
        task: &AgentTask,
        herdr: &mut HerdrIpcClient,
    ) -> Result<AgentResult, AiError> {
        // 尝试 1: 首选 agent
        match self.try_execute_agent(task, herdr).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                // 日志记录失败
                tracing::warn!("Primary agent '{}' failed: {e}", task.agent_name);
                // 尝试 2: 使用 claude 作为 fallback
                let fallback_task = AgentTask {
                    agent_name: "claude".into(),
                    ..task.clone()
                };
                match self.try_execute_agent(&fallback_task, herdr).await {
                    Ok(result) => return Ok(result),
                    Err(e2) => {
                        return Err(AiError::AgentFailed {
                            agent: task.agent_name.clone(),
                            detail: format!("Primary: {e}, Fallback: {e2}"),
                        });
                    }
                }
            }
        }
    }
}
```

## Output 截断

```rust
/// agent 输出截断（防止内存溢出）
fn truncate_agent_output(output: &str, max_bytes: usize) -> String {
    if output.len() > max_bytes {
        let truncated: String = output.chars().take(max_bytes).collect();
        format!("{}...\n[Output truncated at {} bytes]", truncated, max_bytes)
    } else {
        output.to_string()
    }
}
```

## Requirements

- [ ] `AiError` 枚举定义
- [ ] `retry_with_backoff()` 通用重试函数
- [ ] `execute_agent_with_fallback()` agent 级失败恢复
- [ ] LLM API 错误处理（401/403 → 提示检查 key, 5xx → 重试）
- [ ] Socket 断连检测 + 自动重连
- [ ] Agent 输出截断（100KB 限制）
- [ ] 超时处理（agent 超时 → 终止 → 重试/上报）
- [ ] 用户可理解的错误消息（非技术性）

## Acceptance Criteria

- [ ] API key 错误 → 显示"请检查 API key"而不是 JSON 解析错误
- [ ] herdr socket 断开 → 显示"请确保 herdr 正在运行"
- [ ] Agent 5 分钟无响应 → 超时终止并通知
- [ ] 网络临时中断 → 自动重试 2 次后报错
- [ ] agent 输出超过 100KB → 截断并标记

## Out of Scope

- 持久化重试状态（程序重启后重试状态丢失）
- 分布式错误追踪
