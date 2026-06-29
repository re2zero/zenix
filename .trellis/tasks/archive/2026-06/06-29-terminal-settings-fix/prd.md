# PRD: 完善终端设置 — 透明度滑块 + 回滚行数可编辑

## 背景
当前终端设置中，透明度只有 +/- 按钮（步长 0.1），回滚行数也只有 +/- 按钮（步长 100）。用户无法精确输入数值，体验不够好。

## 需求

### 1. 透明度（Opacity）
- 添加滑块（Slider），范围 50%–100%，步长 1%
- 保留 +/- 按钮（与字体大小控制一致的风格）
- 滑块拖拽时实时更新显示值
- 显示格式：百分比（如 "85%"）

### 2. 回滚行数（Scrollback Lines）
- 添加可编辑输入框，替代当前纯文本显示
- 点击输入框进入编辑模式
- Enter 或离开焦点时保存，解析为 u32
- 范围：100–100000，步长 100
- 非法输入（非数字、超出范围）时回滚到原值
- 保留 +/- 按钮
- 输入框宽度适配数值长度

## 接受标准
1. 透明度滑块可在 50%–100% 之间平滑拖动，数值实时更新
2. +/- 按钮对透明度仍然有效，步长 0.1
3. 回滚行数显示为可点击输入框
4. Enter 或点击外部区域后保存数值，自动验证范围
5. 非法输入时不保存，保持原值
6. 透明度值和回滚行数保存到 config.json
7. 所有文案使用现有 i18n 键

## 修改文件
- `src/ui/settings/tabs.rs` — `tab_terminal()` 函数、新增 `opacity_slider()`
- `src/ui/settings/mod.rs` — 新增 `scrollback_input` 字段到 `SettingsDialogProps`
- `src/app.rs` — 创建 `InputState` entity、订阅 blur/enter 事件

## 实现备注
- 透明度滑块用 GPUI 原语构建（track + thumb），无外部 slider 组件依赖
- 回滚输入用 `gpui_component::input::InputState`，通过 `cx.subscribe` 监听 `PressEnter` 和 `Blur` 事件
- 数值验证：范围 [100, 100000]，向下取整到 100 的倍数
- 非法输入（parse 失败）时不保存，保持原值
