//! ZenixAI 编排器配置。
//!
//! 存储编排器使用哪个 API Provider、模型、思考度。不存储 API key，
//! key 由 api_provider 管理。

use serde::{Deserialize, Serialize};

use crate::api_provider::ThinkingLevel;

/// 编排器 AI 的配置（存储在 ConfigFile 的 zenix_ai 字段中）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZenixAIConfig {
    /// 使用的 ApiProviderConfig 的 name 字段。
    pub provider_id: String,
    /// 模型名（与 ApiProviderConfig.model 相同）。
    pub model: String,
    /// 思考度。
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


