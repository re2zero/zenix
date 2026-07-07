# Implementation: zenixAI Zed 风格界面重构

## 实施步骤

### 步骤 1: 顶部导航栏改造 (Header Redesign)
- 左侧显示"ZenixAI"字样并附带下拉箭头（▾）
- 右侧放置搜索（🔍）和更多选项（⋯）图标，尺寸为 28x28

### 步骤 2: 主输入区与引导语改造 (Input Area & Placeholder Redesign)
- 占位符文本："Message the ZenixAI Agent, @ to include context, / for commands"
- 文本颜色使用低对比度的灰色（`gpui::hsla(0.0, 0.0, 0.44, 1.0)`，即 #6e6e6e）
- 输入框背景色为 `gpui::rgba(0xFFFFFF04)`，边框为 `gpui::rgba(0xFFFFFF08)`

### 步骤 3: 底部模型选择与配置面板改造 (Bottom Panel & Model Selection Redesign)
- 面板第一行文本："Select a model…"附带下拉箭头（▾）
- 下方按钮："Configure"
- 右侧附带快捷键提示："Ctrl-Alt-C"

### 步骤 4: 底部操作栏极简化 (Bottom Action Bar Redesign)
- 左侧："+"号图标按钮
- 中间：设置图标（⚙）+ 文本（"ZenixAI Agent"或"Agents (n)"）+ 下拉箭头（▾）
- 右侧：极简的发送/播放按钮（▶）或停止按钮（◼）

## 文件修改清单
- `src/ai/chat.rs`: 主要 UI 渲染函数更新
  - `render_header()`
  - `render_input_area()`
  - `render_bottom_panel()`
  - `render_bottom_action_bar()`

## 测试验证
- 运行 `cargo check` 确保编译通过
- 运行 `cargo fmt --check` 确保代码格式正确
- 运行 `cargo clippy` 确保没有警告
