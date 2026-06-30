# ZenixAI 配置

## 目标

为 AI 编排器定义独立的配置结构体，并在设置面板中增加配置入口。配置决定编排器使用哪个 API Provider、模型、思考度。

## 实现方式

复用现有的 ApiProviderConfig 体系。ZenixAI 配置只存储"引用"——Provider ID + model + thinking_level，不存储 API key。

### 配置结构

```rust
// src/ai/config.rs

/// 编排器 AI 的配置（存储在 ConfigFile 中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZenixAIConfig {
    /// 使用的 ApiProviderConfig 的 name 字段
    pub provider_id: String,
    /// 模型名（与 ApiProviderConfig.model 相同）
    pub model: String,
    /// 思考度
    pub thinking_level: ThinkingLevel,
}

impl Default for ZenixAIConfig {
    fn default() -> Self {
        // 如果没有任何 API provider 配置，提供一个空默认
        Self {
            provider_id: String::new(),
            model: String::new(),
            thinking_level: ThinkingLevel::Medium,
        }
    }
}
```

### 存储方式

存储在现有的 ConfigFile 中，作为 `zenix_ai` key：

```jsonc
// ~/.config/zenix/config.json
{
  "zenix_ai": {
    "provider_id": "My OpenAI",
    "model": "gpt-4o-mini",
    "thinking_level": "medium"
  }
}
```

在 `src/config.rs` 的 ConfigFile struct 中增加字段：

```rust
pub struct ConfigFile {
    // ... existing fields ...
    pub zenix_ai: Option<ZenixAIConfig>,
}
```

### ProviderSelector

下游需要的辅助函数（供 03-orchestrator-engine 使用）：

```rust
/// 获取当前编排器配置的 ApiProviderConfig 实例
pub fn get_orchestrator_provider(config: &ConfigFile) -> Option<ApiProviderConfig> {
    let ai_config = config.zenix_ai.as_ref()?;
    let providers = crate::api_provider::load_all();
    providers.into_iter().find(|p| p.name == ai_config.provider_id)
}
```

### 设置面板

在 SystemInfo 的 Settings Dialog 中增加一个二级 tab/分页 —— "ZenixAI"：

- Provider 下拉选择（从已配置的 API Provider 列表中选）
- Model 下拉（根据选中的 Provider 动态加载）
- Thinking Level 下拉

复用现有 settings dialog 的 UI 模式。

## Requirements

- [ ] ZenixAIConfig 结构体定义
- [ ] ConfigFile 增加 `zenix_ai` 字段（persist/load）
- [ ] 设置面板 → ZenixAI tab
- [ ] Provider 下拉从已注册的 API Provider 列表填充
- [ ] Model 下拉根据 Provider 动态加载
- [ ] Thinking Level 下拉
- [ ] ProviderSelector 辅助函数

## Acceptance Criteria

- [ ] 设置面板中可见 ZenixAI 配置 tab
- [ ] 选择一个 API Provider 后保存，重启后配置保留
- [ ] `get_orchestrator_provider()` 返回正确的 ApiProviderConfig
- [ ] 修改配置后，运行中的 03-orchestrator-engine 可读到新值

## Out of Scope

- 模型列表的实时 fetch（复用现有 api_provider 的 fetch_models）
- 多 provider 切换时的模型列表自动更新
