---
name: gpui-component
description: 指导所有 GPUI 可见 UI 实现中正确使用 gpui-component 组件库。适用于新增、修改或审查按钮、输入、选择器、表单、弹层、导航、列表、表格、反馈状态和布局交互；即使用户没有明确要求使用组件库，也应优先用 gpui-component，纯 GPUI 控件实现只能作为最后手段。
---

## Nexora UI 使用边界

- 涉及 Nexora 可见 UI 时，先执行 `desktop-ui-component-selection`，由它完成组件选型门禁和组件选型记录。
- 本 Skill 负责 `gpui-component` API、初始化、状态、主题、尺寸和正确使用方式，不复制完整选型表。
- 组件目录只是快速索引；复杂交互必须核对 workspace 当前锁定版本的源码或对应官方文档。
- 实现者不能因为不知道组件 API 就改为手写组件；先查源码、文档、story 或已有调用点。
- 现有组件能力不足时，优先包装、组合或转发其 builder API，保留官方组件默认主题、尺寸、焦点、键盘、disabled 和 loading 等语义。
- 纯布局容器可以使用 GPUI Element；语义控件必须优先使用组件库。
- 标准 CRUD 表格每列只展示所属字段，不合并头像、姓名、用户名等多个值；锁定版本的
  `DataTable` 表头和正文共用尺寸，不能用统一自定义行高迁就合并正文。
- CRUD 布尔状态使用 `nexora::desktop::TableSwitchCell`，其他状态使用官方填充 `Tag` 和
  Secondary/Info/Primary/Success/Warning/Danger 语义变体；状态 Tag 禁止 outline、Custom、
  Color 和全状态统一颜色，分类 Tag 才能使用 `Tag::color(ColorName)`。
- `CrudPanel` 多页继续使用普通 `Pagination` 并显示最多 5 个页码；单页由项目薄组合补充当前页
  `1`，不创建第二套分页状态或公共分页控件。

## 文档

- **锁定参考来源**：先读 [references/SOURCE.md](references/SOURCE.md)，确认实际 revision 与已知冲突。
- **锁定源码**：复杂 builder、事件、状态和可见行为必须核对 Cargo.lock 指向的 checkout；
  `usage.md` 仅作快速快照，不能覆盖锁定源码。
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

**尺寸**：业务页面中实现 `Sizable` 的组件优先 `.with_size(theme::component_size(cx))`，让设置里的
组件尺寸即时生效；`.xsmall()`、`.small()`、`.medium()`、`.large()` 只在紧凑表格行、图标工具按钮
等有明确语义时手动使用。

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
| `Form` / `Field` | `form::{v_form, h_form, field}` | 官方字段布局、label、description、required、visible 与 col_span |

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
| `Dialog` | `dialog::Dialog` + `WindowExt` | 锁定源码通过 `window.open_dialog(...)` 打开，使用 `close_dialog(...)` 关闭 |
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
| `Scrollable` | `scroll::ScrollableElement` | 为内容提供官方滚动条与滚动容器扩展 |
| `FocusTrap` | `focus_trap::FocusTrap` | 模态框的键盘焦点约束 |

### 数据展示

| 组件 | 导入路径 | 说明 |
|------|----------|------|
| `DataTable` | `table::{DataTable, TableState, TableDelegate}` | 有状态。功能完整的数据表格 |
| `Table` | `table::{Table, ...}` | 简单表格 |
| `VirtualList` | `{v_virtual_list, h_virtual_list}` | 高性能大列表 |
| `List` | `list::{List, ListState, ListDelegate}` | 有状态。可搜索列表 |
| `SearchableList` | `searchable_list::{SearchableListState, SearchableListDelegate, ...}` | 搜索、键盘游标和选择语义 |
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
- [SOURCE.md](references/SOURCE.md) — 快照来源、revision、校验值和已知 API 冲突
- [style-guide.md](references/style-guide.md) — 贡献者代码风格
