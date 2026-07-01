# 子任务 05: 配置隔离检查

## 目标
确保 AI 会话在没有配置任何 API Provider 时不可用，提供友好的错误提示。

## 背景
- 当前 `app.rs` 在打开 AI 面板时直接创建 `AiChatPanelData`，没有检查配置
- 需要添加配置检查，无配置时显示提示而不是空白面板

## 实现

### 1. 在 `engine.rs` 中添加配置检查

```rust
impl OrchestratorEngine {
    /// 检查是否有可用的 API Provider
    pub fn has_available_provider() -> bool {
        let providers = crate::api_provider::load_all();
        providers.iter().any(|p| !p.api_key.is_empty())
    }
    
    /// 获取第一个可用的 Provider
    pub fn get_available_provider() -> Option<ApiProviderConfig> {
        crate::api_provider::load_all()
            .into_iter()
            .find(|p| !p.api_key.is_empty())
    }
}
```

### 2. 修改 `app.rs`

在 AI 面板渲染时检查配置：

```rust
Panel::AiChat => {
    let data = self.ai_chat_panel.get_or_insert_with(
        crate::ai::chat::AiChatPanelData::new
    );
    
    // 检查是否有可用配置
    if !crate::ai::engine::OrchestratorEngine::has_available_provider() {
        // 显示配置提示
        return render_config_empty(&theme);
    }
    
    // ... 正常渲染
}
```

### 3. 添加空配置渲染

```rust
fn render_config_empty(theme: &Theme) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_4()
        .child(txt(px(24.)).child("🤖"))
        .child(txt(px(14.)).text_color(theme.muted_foreground).child(
            "请先在「设置 → 扩展 → API 提供商」中配置一个 API Provider"
        ))
        .child(Button::new("open_settings")
            .primary()
            .label("打开设置")
            .on_click(...))
}
```

### 不做什么
- 不改 engine.rs 的核心逻辑
- 不改 tools.rs
- 不改 compactor.rs

## 测试要求
- has_available_provider: 无配置返回 false
- get_available_provider: 有配置返回 Some

## 验证标准
- ✅ `cargo check` 编译通过
- ✅ 所有测试通过
- ✅ `cargo clippy` 无新增警告
