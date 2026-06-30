# ZenixAI 配置 — 设计

## 数据结构

```rust
// src/ai/config.rs

use serde::{Deserialize, Serialize};
use crate::api_provider::{ApiProviderConfig, ThinkingLevel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZenixAIConfig {
    /// 引用哪个 API Provider（ApiProviderConfig.name）
    pub provider_id: String,
    /// 使用的模型名
    pub model: String,
    /// 思考度
    pub thinking_level: ThinkingLevel,
}

impl Default for ZenixAIConfig {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            model: String::new(),
            thinking_level: ThinkingLevel::Medium,
        }
    }
}
```

## ConfigFile 扩展

```rust
// src/config.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    // ... existing fields ...
    pub zenix_ai: Option<ZenixAIConfig>,
}
```

确保：
- 反序列化时 `zenix_ai` 不存在不报错（`Option`）
- 保存时将现有字段写回，不覆盖

## ProviderSelector

```rust
// src/ai/config.rs

/// 获取编排器当前配置的 API Provider
pub fn get_orchestrator_provider(config: &ConfigFile) -> Option<ApiProviderConfig> {
    let ai_config = config.zenix_ai.as_ref()?;
    let providers = crate::api_provider::load_all();
    // 通过 name 匹配
    providers.into_iter().find(|p| p.name == ai_config.provider_id)
}

/// 获取所有可用的 API Provider 列表（供 UI 下拉使用）
pub fn get_available_providers() -> Vec<ApiProviderConfig> {
    crate::api_provider::load_all()
}
```

## 设置面板 UI

在已有的 Settings Dialog 中增加一个 tab/分页。

### 定位

`src/ui/settings/` 目录下，现有的 settings dialog 是一个 GPUI 模态窗口。增加一个新的 section 叫做 "ZenixAI"。

实现方式（参考现有 modes 如 Appearance/Behavior/Plugins）：

```rust
// 在 settings dialog 中增加
fn render_zenixai_settings(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    v_flex()
        .child(
            Section::new("ZenixAI")
                .child(provider_selector(...))
                .child(model_selector(...))
                .child(thinking_level_selector(...))
        )
}
```

### Provider 下拉

```rust
// 从 get_available_providers() 获取列表
// 选择后更新 ZenixAIConfig.provider_id
// 同时触发 model 列表刷新
```

### Model 下拉

```rust
// 根据选中的 provider，调用 api_provider::fetch_models()
// 如果 fetch 失败，提示"无法获取模型列表，请手动输入"
// 也支持手动输入模型名（text input fallback）
```

### Thinking Level 下拉

```rust
// 复用 api_provider::ThinkingLevel 的 all() 方法
// 显示 label: Low / Medium / High / X-High
```

### 保存

配置变更后通过 `ConfigFile::save()` 持久化。

## 依赖

- `src/config.rs`（ConfigFile 结构体）
- `src/api_provider/mod.rs`（ApiProviderConfig, ThinkingLevel, fetch_models, load_all）
- `src/ui/settings/`（现有设置面板）
