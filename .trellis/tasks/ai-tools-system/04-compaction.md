# 子任务 04: 会话压缩

## 目标
实现长对话时的自动上下文压缩，避免超出 LLM 上下文窗口。

## 背景
- 编排器在长对话中需要管理上下文长度
- 当消息累积超过上下文窗口的 80% 时，自动压缩旧消息
- 对用户无感（自动处理）

## 设计参考
- pi_agent_rust `src/compaction.rs`
- Token 估算: 每 3 个字符 ≈ 1 token
- 触发条件: token 使用量 > 上下文窗口的 80%
- 压缩方式: 保留最近 N 条消息，让 LLM 生成摘要替换旧消息

## 实现

### 1. `ai/compactor.rs`

```rust
/// 压缩参数
pub struct CompactionConfig {
    /// 上下文窗口大小（token 数）
    pub context_window_tokens: usize,
    /// 预留 token 数（~8%）
    pub reserve_tokens: usize,
    /// 保留最近消息比例
    pub keep_recent_ratio: f32,
}

/// 压缩结果
pub struct CompactionResult {
    /// 压缩后的消息列表（摘要 + 保留的消息）
    pub messages: Vec<ChatMessageRef>,
    /// 被压缩的消息数量
    pub compressed_count: usize,
    /// 压缩前的 token 估算
    pub before_tokens: usize,
    /// 压缩后的 token 估算
    pub after_tokens: usize,
}

/// 压缩器
pub struct Compactor {
    config: CompactionConfig,
}

impl Compactor {
    pub fn new(config: CompactionConfig) -> Self;
    
    /// 估算消息列表的 token 数
    pub fn estimate_tokens(messages: &[ChatMessageRef]) -> usize;
    
    /// 检查是否需要压缩
    pub fn should_compress(&self, messages: &[ChatMessageRef]) -> bool;
    
    /// 执行压缩（需要 LLM 生成摘要）
    pub async fn compress(
        &self,
        messages: &[ChatMessageRef],
        engine: &OrchestratorEngine,
    ) -> Result<CompactionResult>;
}
```

### 2. Token 估算

```rust
pub fn estimate_tokens(text: &str) -> usize {
    // 保守估计: 每 3 个字符 ≈ 1 token
    text.chars().count() / 3 + 10 // 加上系统开销
}
```

### 3. 压缩逻辑

1. 估算当前消息的 token 数
2. 如果 > context_window * 0.8，触发压缩
3. 保留最近 N% 的消息（如 20%）
4. 让 LLM 生成前面对话的摘要
5. 用摘要替换被压缩的消息

### 4. 集成到 engine.rs

在 `chat_with_tools` 循环中，每次 LLM 调用前检查 token 使用量：

```rust
// In chat_with_tools loop
let token_count = Compactor::estimate_tokens(&loop_messages);
if compactor.should_compress(&loop_messages) {
    let result = compactor.compress(&loop_messages, self).await?;
    on_event(DispatchStreamEvent::Compacted(result.compressed_count));
    loop_messages = result.messages;
}
```

### 5. 默认配置

```rust
pub const DEFAULT_COMPACTOR: CompactionConfig = CompactionConfig {
    context_window_tokens: 128_000,  // GPT-4o 上下文窗口
    reserve_tokens: 10_240,          // ~8%
    keep_recent_ratio: 0.2,          // 保留最近 20%
};
```

## 不做什么
- 不修改数据库 schema
- 不改 UI
- 不实现压缩历史追踪

## 测试要求
- token 估算: 空字符串、短文本、长文本
- should_compress: 超过阈值、低于阈值
- compress: 完整压缩流程（mock LLM）

## 验证标准
- ✅ `cargo check` 编译通过
- ✅ 所有测试通过
- ✅ `cargo clippy` 无新增警告
