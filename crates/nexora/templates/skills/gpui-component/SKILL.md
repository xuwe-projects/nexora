---
name: gpui-component
description: 指导如何在 GPUI 应用中使用 gpui-component UI 组件库。使用 gpui-component 组件（Button、Input、Select、Dialog、Tabs、Sidebar、List、Table 等）构建界面、初始化组件库、处理组件状态与主题，或为具体 UI 需求选择合适组件时使用。
---

## 文档

- **完整参考**：获取 `https://longbridge.github.io/gpui-component/llms-full.txt`
- **单个组件 API**：获取 `https://longbridge.github.io/gpui-component/docs/components/{name}.md`
  - 例如 `button.md`、`input.md`、`select.md`、`dialog.md`、`data-table.md`
- **站点任意页面**：在 URL 末尾追加 `.md`，即可获取 Markdown 格式内容

## 快速参考

**初始化** — 始终需要：

```rust
gpui_component::init(cx);               // 必须在 app.run() 中首先调用
Root::new(view, window, cx)             // 每个窗口的第一层视图
```

**无状态组件** — 直接在 `render` 中使用：

```rust
Button::new("id").primary().label("OK").on_click(|_, _, _| {})
```

**有状态组件** — 在结构体中持有 `Entity<State>`，并在 `render` 中传入引用：

```rust
// 在 new() 中：let input = cx.new(|cx| InputState::new(window, cx));
// 在 render 中：Input::new(&self.input)
```

**尺寸**：`.xsmall()`、`.small()`、`.medium()`（默认）、`.large()`

**主题**：`cx.theme().primary` · `.background` · `.foreground` · `.border` · `.muted`

## 组件目录

需要组件时先在此查找；需要完整 API 时，再获取对应的 `.md` 文档。

### 输入与表单

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `Input` | `input::{Input, InputState}` | 有状态。支持文本、密码、掩码和校验 |
| `NumberInput` | `input::{NumberInput, InputState}` | 有状态。绑定 `Entity<InputState>` 的步进数值输入 |
| `OtpInput` | `input::{OtpInput, OtpState}` | 有状态。绑定 `Entity<OtpState>` 的一次性密码输入 |
| `Select` | `select::{Select, SelectState}` | 有状态。下拉选择器 |
| `Combobox` | `combobox::{Combobox, ComboboxState}` | 有状态。可搜索选择器 |
| `Checkbox` | `checkbox::Checkbox` | 无状态。使用 `on_click(|&bool, ...|)` |
| `Switch` | `switch::Switch` | 无状态。开关切换 |
| `Radio` | `radio::{Radio, RadioGroup}` | 无状态。单选项与单选组 |
| `Slider` | `slider::{Slider, SliderState}` | 有状态。滑块 |
| `Toggle` | `toggle::Toggle` | 无状态。切换按钮 |
| `Rating` | `rating::Rating` | 无状态。评分 |
| `Stepper` | `stepper::Stepper` | 无状态。递增/递减 |
| `ColorPicker` | `color_picker::{ColorPicker, ColorPickerState}` | 有状态。颜色选择器 |
| `DatePicker` | `time::date_picker::{DatePicker, DatePickerState}` | 有状态。日期选择器 |
| `Form` | `form::{v_form, h_form, field}` | 表单字段的布局容器 |

### 展示与反馈

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `Button` | `button::{Button, ButtonGroup}` | 无状态。主要界面操作 |
| `Icon` | `{Icon, IconName}` | 无状态。Lucide 图标 |
| `Badge` | `badge::Badge` | 无状态。徽标 |
| `Tag` | `tag::Tag` | 无状态。可关闭标签 |
| `Avatar` | `avatar::Avatar` | 无状态。头像 |
| `Label` | `label::Label` | 无状态。表单标签 |
| `Kbd` | `kbd::Kbd` | 无状态。键盘按键展示 |
| `Alert` | `alert::Alert` | 无状态。信息/成功/警告/错误提示 |
| `Spinner` | `spinner::Spinner` | 无状态。加载指示器 |
| `Skeleton` | `skeleton::Skeleton` | 无状态。加载占位符 |
| `Progress` | `progress::{Progress, ProgressCircle}` | 无状态。进度条或进度环 |
| `Tooltip` | `tooltip::Tooltip` | 通过元素的 `.tooltip()` 使用 |
| `HoverCard` | `hover_card::{HoverCard, HoverCardState}` | 有状态。悬停卡片 |
| `Image` | `image::Image` | 无状态。图像 |
| `Clipboard` | `clipboard::Clipboard` | 无状态。复制按钮 |

### 遮罩层与弹出层

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `Dialog` | `dialog::Dialog` + `WindowExt` | 通过 `window.open_dialog(...)` 打开，使用 `close_dialog(...)` 关闭 |
| `AlertDialog` | `WindowExt` | 通过 `window.open_alert_dialog(...)` 打开 |
| `Sheet` | `sheet::Sheet` + `WindowExt` | 侧边面板，通过 `window.open_sheet(...)` 打开 |
| `Notification` | `notification::Notification` + `WindowExt` | 通过 `window.push_notification(...)` 推送 |
| `Popover` | `popover::Popover` | 浮动遮罩层 |
| `Menu` | `menu::{PopupMenu, DropdownMenu}` | 上下文菜单 |
| `DropdownButton` | `button::DropdownButton` | 带下拉菜单的按钮 |

### 导航与布局

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `Tabs` / `TabBar` | `tab::{Tab, TabBar}` | 标签页界面 |
| `Sidebar` | `sidebar::{Sidebar, SidebarMenu, ...}` | 应用导航面板 |
| `TitleBar` | `title_bar::TitleBar` | 窗口标题栏 |
| `Breadcrumb` | `breadcrumb::Breadcrumb` | 导航面包屑 |
| `Pagination` | `pagination::Pagination` | 分页导航 |
| `Accordion` | `accordion::Accordion` | 可折叠分区 |
| `Collapsible` | `collapsible::Collapsible` | 单个可折叠区域 |
| `GroupBox` | `group_box::GroupBox` | 带标签的容器 |
| `Resizable` | `resizable::Resizable` | 可拖动分隔面板 |
| `Scrollable` | `scroll::Scrollbar` | 自定义滚动条 |
| `FocusTrap` | `focus_trap::FocusTrap` | 模态框的键盘焦点约束 |

### 数据展示

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `DataTable` | `table::{DataTable, TableState, TableDelegate}` | 有状态。功能完整的数据表格 |
| `Table` | `table::{Table, ...}` | 简单表格 |
| `VirtualList` | `{v_virtual_list, h_virtual_list}` | 高性能大列表 |
| `List` | `list::{List, ListState, ListDelegate}` | 有状态。可搜索列表 |
| `Tree` | `tree::{Tree, TreeState, TreeDelegate}` | 有状态。层级结构 |
| `DescriptionList` | `description_list::DescriptionList` | 键值对列表 |
| `Settings` | `settings::Settings` | 设置面板 |

### 图表

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `Chart` | `chart::Chart` | 柱状图、折线图、面积图、饼图 |
| `Plot` | `plot::Plot` | 为数据使用 `#[derive(IntoPlot)]` |

## 参考文件

- [usage.md](references/usage.md) — 初始化模式、组件类型和常用示例
- [style-guide.md](references/style-guide.md) — 贡献者代码风格
