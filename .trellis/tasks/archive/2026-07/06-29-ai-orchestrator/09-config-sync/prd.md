# API Provider Agent 配置同步增强

## 目标

完善现有的 API Provider → Agent CLI 配置同步功能，确保配置变更时自动推送到各 Agent CLI 的配置文件。

## 现有能力

现有 `api_provider/mod.rs` 已经实现了：
- `sync_api_to_agent()` — 同步 provider 到 agent config
- `unsync_api_from_agent()` — 移除 provider 的 agent config
- `toggle_api_agent()` — 切换某个 agent 的启用状态
- `to_opencode_provider()` — 生成 OpenCode provider 配置
- `to_claude_env()` — 生成 Claude env vars
- `to_openai_env()` — 生成 OpenAI 格式 env vars
- AgentConfigAdapter 体系

## 需增强的内容

### 1. 批量同步

新增批量操作，在 Provider 设置页面的"应用"按钮触发：

```rust
/// 将 provider 同步到所有已启用的 agent
pub fn sync_to_all_enabled_agents(provider: &ApiProviderConfig) -> Result<(), String> {
    for agent_name in &provider.enabled_agents {
        sync_api_to_agent(provider, agent_name)?;
    }
    Ok(())
}

/// 将全部 providers 同步到各自的 agents
pub fn sync_all_providers() -> Result<(), String> {
    let providers = load_all();
    for provider in &providers {
        if !provider.enabled_agents.is_empty() {
            sync_to_all_enabled_agents(provider)?;
        }
    }
    Ok(())
}
```

### 2. 配置变更实时同步

在 Provider 设置保存时自动触发同步：

```rust
// 在 upsert 中增加自动同步
pub fn upsert_and_sync(config: &ApiProviderConfig) -> Result<(), String> {
    let result = upsert(config)?;
    sync_to_all_enabled_agents(config)?;
    Ok(result)
}
```

### 3. 配置验证增强

```rust
/// 验证一个 provider 的所有 agent 配置是否有效
pub fn validate_provider_sync(provider_name: &str) -> Result<Vec<String>, String> {
    let providers = load_all();
    let provider = providers.iter()
        .find(|p| p.name == provider_name)
        .ok_or_else(|| format!("provider '{provider_name}' not found"))?;

    let mut results = Vec::new();
    for agent_name in &provider.enabled_agents {
        let adapter = crate::config_adapters::adapters::adapter_for(agent_name)
            .ok_or_else(|| format!("unknown agent: {agent_name}"))?;
        let config_path = get_api_config_path(agent_name)?;
        if config_path.exists() {
            results.push(format!("{agent_name}: config exists at {}", config_path.display()));
        } else {
            results.push(format!("{agent_name}: no config file (pending sync)"));
        }
    }
    Ok(results)
}
```

## Requirements

- [ ] `sync_to_all_enabled_agents()` — 批量同步单个 provider 到所有 agents
- [ ] `sync_all_providers()` — 同步所有 providers
- [ ] `upsert_and_sync()` — 保存时自动同步
- [ ] `validate_provider_sync()` — 验证同步状态
- [ ] 设置面板增加"同步"按钮

## Acceptance Criteria

- [ ] 配置 API Provider 后，"同步"按钮可将配置写入各 Agent CLI
- [ ] 验证同步后，agent config 文件中出现对应的 provider/env 配置
- [ ] unsync 后，agent config 文件中移除对应的配置

## Out of Scope

- Agent CLI 的热加载配置（agent 需要重启才能生效）
- 多用户的配置同步（仅本地）
