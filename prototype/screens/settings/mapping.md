# 设置窗口 · 原型 → gpui 翻译映射

真实主题能力来自 `crates/theme/src/lib.rs`（ColorScheme、字号、component_size、预设目录）。
设置窗口通过 Nexora 的 `SettingsWindow` 派生注册，表单控件全部使用 gpui-component。

## 区块映射

| 原型区块 | gpui / gpui-component 落地 | 说明 |
| --- | --- | --- |
| 独立设置窗口 | `#[derive(SettingsWindow)]` + 单例窗口（`OpenSettings` action）| 已有能力 |
| 左侧分类导航 | `gpui_component::sidebar::Sidebar` 或 `tab` 竖向 | 现成组件 |
| 分区标题/描述 | `div().text_*()` 文本 + `group_box` | 布局 |
| 表单字段容器 | `LabeledControl`（crates/ui）/ `gpui_component::form` | 已有封装 |
| 主题预设「分段选择」| `gpui_component::tab::TabBar` 或 `Select` | 数据来自 `theme::presets(cx)` |
| 颜色模式「分段选择」| 同上，值为 `ColorScheme::ALL`（跟随系统/浅色/深色）| `theme::set_color_scheme` |
| 基础字号「滑块」| `gpui_component::slider::Slider` | 范围 `MIN_FONT_SIZE..MAX_FONT_SIZE`(12–20)，`theme::set_font_size` |
| 组件尺寸「下拉」| `gpui_component::select::Select` | 值为 `Size`，`theme::set_component_size` |
| 开关（自动折叠/减少动效）| `gpui_component::switch::Switch` | 现成组件，勿用 div 模拟 |
| 底部取消/保存 | `gpui_component::button::Button`（ghost / primary）| 现成组件 |

## 说明

- 「主题预设」「颜色模式」两个分段控件是真实主题系统已有的数据：
  预设来自 `theme::presets()`，颜色模式来自 `ColorScheme::ALL`，字号有明确上下限常量。
  翻译时直接接这些 API，不要另造设置存储。
- 分段选择（segmented）原型用了自绘样式；gpui-component 无专门 segmented 控件，
  翻译时优先用 `TabBar` 达成同等观感，或用一组 `Button` 薄包装——需在代码注释记录选择理由。
- 全部颜色来自 `theme.*`（palettes.css 变量）。
