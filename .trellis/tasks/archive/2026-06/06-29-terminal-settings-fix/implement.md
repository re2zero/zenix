# Implement: 终端设置透明度滑块 + 回滚行数可编辑

## 步骤

### Step 1: 修改 `src/ui/settings/mod.rs` — 添加 scrollback_input 字段

在 `SettingsDialogProps` struct 中添加：
```rust
pub scrollback_input: Option<&'a Entity<gpui_component::input::InputState>>,
```

在 `settings_dialog()` 的 destructuring 中添加对应变量。

### Step 2: 修改 `src/ui/settings/tabs.rs` — 重写 tab_terminal()

#### 2a. 透明度滑块
- 构建自定义 Slider 组件：track + thumb，范围 0.5–1.0
- 保留 +/- 按钮（步长 0.01）
- 显示百分比

#### 2b. 回滚行数可编辑
- 用 `gpui_component::input::Input` 替换静态文本
- 接收 `scrollback_input` 参数
- 保留 +/- 按钮（步长 100）
- 单位文本 "lines" 保持不变

### Step 3: 修改 `src/app.rs` — 创建并传递 InputState

- 在 settings dialog 创建处添加：
  ```rust
  let scrollback_input = cx.new_view(|cx| {
      gpui_component::input::InputState::new(cx)
          .with_placeholder("10000".into())
  });
  ```
- 将 `scrollback_input: Some(&scrollback_input)` 传入 `SettingsDialogProps`
- 处理 blur: spawn 一个 task 监听 InputState 的 blur，解析文本，发送 SetScrollback

### Step 4: 验证
```bash
cargo check
```
