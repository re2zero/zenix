# Design: 终端设置透明度滑块 + 回滚行数可编辑

## 架构概览

不引入新的状态类型，复用现有的 `TerminalAction` enum 和回调模式。
透明度通过自定义 GPUI 滑块组件实现；回滚行数通过 `gpui_component::input::Input` 实现可编辑输入。

## 1. 透明度（Opacity Slider）

### 组件结构
```
setting_row("透明度")
  ├─ Button("-")          — 减少 0.01
  ├─ Slider 组件          — 可拖拽滑块
  │   ├─ Track (背景条)
  │   └─ Thumb (拖拽手柄)
  ├─ Button("+")          — 增加 0.01
  └─ Text("85%")          — 当前值显示
```

### Slider 实现方案
使用 GPUI 原语手动构建：
- `Div` track: 固定高度，背景色，圆角
- `Div` thumb: 圆形，根据 opacity 值计算 left 偏移
- `on_mouse_down` 在 track 上：跳转到点击位置
- `on_mouse_move` 带按钮状态：拖动更新
- 范围：0.5–1.0，精度 0.01

### 回调
`TerminalAction::SetOpacity(f32)` 已存在，直接使用。

## 2. 回滚行数（Editable Input）

### 组件结构
```
setting_row("回滚行数")
  ├─ Button("-")          — 减少 100
  ├─ Input (可编辑)       — Enter/blur 时保存
  │   └─ Entity<InputState>
  ├─ Button("+")          — 增加 100
  └─ Text("lines")        — 单位
```

### Input 生命周期
1. 初始值 = 当前 scrollback 值
2. 用户编辑 → 文本变化
3. Enter 或 blur → 解析文本为 u32
4. 验证：范围 [100, 100000]，步长 100（向下取整到 100 的倍数）
5. 有效 → 发送 `TerminalAction::SetScrollback(u32)`
6. 无效 → 恢复原值

### 状态管理
在 `SettingsDialogProps` 中新增 `scrollback_input: Option<&Entity<InputState>>`。
在 `app.rs` 中创建 `Entity<InputState>` 并通过 `cx.spawn` 监听 blur 事件。

### 回调
`TerminalAction::SetScrollback(u32)` 已存在，直接使用。

## 兼容性
- 不修改 `TerminalAction` enum
- 不修改 `ConfigFile` 结构
- 新增字段使用 `Option` 保持向后兼容
- 不添加新的 i18n 键，复用现有 `settings.terminal.opacity` 和 `settings.terminal.scrollback`

## 文件变更
| 文件 | 变更 |
|------|------|
| `src/ui/settings/tabs.rs` | 重写 `tab_terminal()` 的 opacity 和 scrollback 部分 |
| `src/ui/settings/mod.rs` | 新增 `scrollback_input` 字段到 `SettingsDialogProps` |
| `src/app.rs` | 创建 InputState entity，传递给 props，处理 blur |
