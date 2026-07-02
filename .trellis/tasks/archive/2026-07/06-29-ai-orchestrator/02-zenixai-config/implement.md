1. 创建 `src/ai/config.rs` — ZenixAIConfig 结构体定义 + ProviderSelector
2. 修改 `src/config.rs` — ConfigFile 增加 `zenix_ai` 字段
3. 设置面板扩展 — 增加 ZenixAI tab
4. `cargo check` 验证

## Implementation Steps

### Step 1: src/ai/config.rs
- `ZenixAIConfig` 结构体（provider_id, model, thinking_level）
- `get_orchestrator_provider(config) -> Option<ApiProviderConfig>`
- `get_available_providers() -> Vec<ApiProviderConfig>`

### Step 2: src/config.rs
- 在 `ConfigFile` struct 中增加 `pub zenix_ai: Option<ZenixAIConfig>`
- 添加 `use crate::ai::config::ZenixAIConfig;`
- 确保 serde derive 包含该字段

### Step 3: 设置面板
- 找到 settings dialog 的渲染代码
- 增加 ZenixAI section 或 tab
- Provider 下拉（从 `get_available_providers()`）
- Model 下拉（从 fetch_models() 或手动输入）
- Thinking Level 下拉
- 保存/加载配置

### Step 4: 验证
- `cargo check`
- 运行 zenix → 设置面板可见 ZenixAI tab
