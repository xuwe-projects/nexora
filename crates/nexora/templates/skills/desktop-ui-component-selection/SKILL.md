---
name: desktop-ui-component-selection
description: 用于构建、修改或审查 Nexora 的 GPUI 桌面交互，并优先从 gpui-component 选择现有组件。适用于导航、表单、弹层、表格、设置、应用壳、反馈状态，以及任何可能已有官方组件的界面。
---

# 桌面 UI 组件选择

## 核心原则

在 GPUI 桌面程序里实现交互时，先查 `gpui-component` 已有组件，再考虑自定义元素。不要手写已经存在的应用导航、折叠面板、表单控件、弹层、表格、树、列表、设置页、标题栏或状态栏。

官方组件文档入口：
`https://longbridge.github.io/gpui-component/zh-CN/docs/components/`

文档路由规则：
`https://longbridge.github.io/gpui-component/zh-CN/docs/components/<component-route>`

## 选择流程

1. 先判断交互意图：导航、输入、选择、弹层、数据展示、反馈、布局、内容展示。
2. 从下面的组件表选择最贴近语义的组件。
3. 打开对应官方文档确认当前 API、示例和主题行为。
4. 只有当官方组件无法表达业务语义时，才组合 `div()`、`h_flex()`、`v_flex()` 或自定义 `Render`。
5. 自定义组件要尽量包在 feature 模块内，不要复制官方组件能力。

## 应用壳与导航

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 应用侧边导航、多 feature 切换 | `Sidebar` | `sidebar` | 用 `SidebarHeader`、`SidebarGroup`、`SidebarMenu`、`SidebarMenuItem`、`SidebarFooter` 组合，不要手写侧边栏。 |
| 顶部原生窗口区域、标题栏按钮 | `TitleBar` | `title-bar` | 需要桌面窗口 chrome、标题、窗口控制时使用。 |
| 底部状态信息、任务状态、连接状态 | `StatusBar` | `status-bar` | 需要左/中/右三区状态展示时使用。 |
| 主窗口 Feature 标签 | `TabBar::segmented()` / `ApplicationTabStyle` | `tabs` | 框架默认用官方 segmented 标签；应用只通过 `ApplicationOptions::tab_style(...)` 选择 underline、pill 或 outline，不重写 Shell 标签栏。 |
| 页面内标签切换 | `Tabs` | `tabs` | 同一上下文内多个视图切换时使用，不要用按钮组模拟 tabs。 |
| 树形导航、文件树、层级资源 | `Tree` | `tree` | 有父子层级、展开收起和选中状态时使用。 |
| 多步骤流程、安装向导、发布流程 | `Stepper` | `stepper` | 用于表达步骤顺序和当前进度。 |
| 面板拆分、可拖拽调整宽度/高度 | `Resizable` | `resizable` | IDE 风格工作区、左右分栏、上下输出区优先用它。 |
| 设置页导航与配置面板 | `Settings` | `settings` | 配置项很多、需要设置页结构时使用。 |

## 折叠与显隐

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| FAQ、多个可展开内容面板 | `Accordion` | `accordion` | 多个同类折叠面板首选它。 |
| 单块内容展开/收起 | `Collapsible` | `collapsible` | 单一区域显隐或高级选项展开时使用。 |
| 滚动内容区域 | `Scrollable` | `scrollable` | 需要滚动条、滚动容器时使用，不要随意手写 overflow 行为。 |
| 大数据量列表滚动 | `VirtualList` | `virtual-list` | 行数很多、需要虚拟化时使用。 |
| 普通列表、可选列表 | `List` | `list` | 中小规模列表或选择列表使用。 |

## 表单与输入

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 表单整体布局、校验结构 | `Form` | `form` | 多字段提交、字段说明、错误状态优先用它组织。 |
| 单行文本输入 | `Input` | `input` | 文本、搜索、路径等短输入使用。 |
| 多行文本或代码编辑 | `Editor` | `editor` | 长文本、脚本、代码片段使用。 |
| 数字输入 | `NumberInput` | `number-input` | 数值字段不要用普通 `Input` 代替。 |
| 下拉单选 | `Select` | `select` | 固定选项且不需要搜索时使用。 |
| 可搜索单选或多选 | `Combobox` | `combobox` | 选项多、需要搜索过滤时使用。 |
| 日期选择 | `DatePicker` | `date-picker` | 日期字段首选。 |
| 日历视图或日期范围基础能力 | `Calendar` | `calendar` | 需要展示日历网格时使用。 |
| 颜色选择 | `ColorPicker` | `color-picker` | 主题色、标记色、品牌色配置使用。 |
| 一次性验证码 | `OtpInput` | `otp-input` | OTP、短码输入使用。 |
| 二元勾选 | `Checkbox` | `checkbox` | 独立布尔项或多选列表使用。 |
| 二元开关 | `Switch` | `switch` | 即时启用/禁用配置使用。 |
| 单选组 | `Radio` | `radio` | 互斥选项使用。 |
| 数值范围滑动 | `Slider` | `slider` | 连续范围调节使用。 |
| 星级或评分输入 | `Rating` | `rating` | 评分、优先级、满意度使用。 |
| 开关式按钮状态 | `Toggle` | `toggle` | 工具栏中的 bold、preview、pin 等开关动作使用。 |
| 按钮动作 | `Button` | `button` | 明确命令、提交、取消、工具按钮使用。 |

## 弹层、菜单与临时界面

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 模态确认、编辑弹窗 | `Dialog` | `dialog` | 需要阻断当前流程并获取用户决定时使用。 |
| 危险确认、删除确认 | `AlertDialog` | `alert-dialog` | 破坏性操作确认优先用它。 |
| 从边缘滑出的面板 | `Sheet` | `sheet` | 详情、过滤器、临时设置从侧边出现时使用。 |
| 小型浮层内容 | `Popover` | `popover` | 轻量表单、过滤器、更多信息使用。 |
| 鼠标悬浮详情 | `HoverCard` | `hover-card` | 预览卡片、用户信息、资源摘要使用。 |
| 菜单、上下文菜单、应用菜单 | `Menu` | `menu` | 操作集合、右键菜单、原生菜单使用。 |
| 按钮触发的下拉动作 | `DropdownButton` | `dropdown_button` | “构建”旁边带更多构建模式等场景使用。 |
| 悬浮提示 | `Tooltip` | `tooltip` | 图标按钮、缩写、不可见语义解释使用。 |
| 焦点限制 | `FocusTrap` | `focus-trap` | 自定义 modal 或临时交互区域需要限制键盘焦点时使用。 |

## 数据展示

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 普通表格 | `Table` | `table` | 简单二维数据展示使用。 |
| 高性能数据表、排序、复杂行列 | `DataTable` | `data-table` | 数据量大、表格交互多时优先用它。 |
| 标准 CRUD 资源管理 Panel | `nexora::desktop::{CrudPanel, CrudPanelToolbar}` | 项目封装 | 标题/描述加刷新、可选筛选/操作区、表格或列表主体的 Feature Panel 优先使用；没有筛选和操作时不渲染工具栏卡片。 |
| 标准 CRUD DataTable | `#[derive(nexora::CrudTableRow)]` + `CrudTableDelegate<T>` | 项目封装 | 行结构字段声明列；delegate 继续接入原生 `DataTable`，操作列用 `action_column`，复杂场景保留手写 `TableDelegate`。 |
| CRUD 表格表头 | `nexora::desktop::TableHeaderCell` | 项目封装 | `DataTable` 的 `render_th` 默认用它让表头水平、垂直居中；需要按列语义覆盖时使用 `.left()`、`.center()`、`.right()` 或完全自定义表头元素。 |
| CRUD 表格正文单元格 | `nexora::desktop::TableCell` | 项目封装 | `DataTable` 的 `render_td` 优先用它；默认垂直居中、水平靠左，可用 `.left()`、`.center()`、`.right()` 和 `.top()`、`.middle()`、`.bottom()` 覆盖；网格线优先使用 `DataTable::bordered(true)` 等原生表格样式。 |
| 分页 | `Pagination` | `pagination` | 远程分页或大数据分页使用。 |
| 图表 | `Chart` | `chart` | 常规业务图表使用。 |
| 绘图或更偏数值绘制的图形 | `Plot` | `plot` | 需要 plot 风格数据展示时使用。 |
| 键值描述、详情摘要 | `DescriptionList` | `description-list` | 展示配置、元数据、构建产物详情使用。 |
| 进度条 | `Progress` | `progress` | 构建、下载、打包进度使用。 |

## 反馈与状态

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 页面内提示 | `Alert` | `alert` | 成功、警告、错误、信息提示使用。 |
| 全局通知 | `Notification` | `notification` | 异步任务完成、失败、后台事件使用。 |
| 加载占位骨架 | `Skeleton` | `skeleton` | 数据加载时保持布局稳定。 |
| 小型加载状态 | `Spinner` | `spinner` | 按钮内、局部区域等待状态使用。 |
| 数量、状态、标签徽标 | `Badge` | `badge` | 未读数、状态点、版本号使用。 |
| 标签、可移除标记 | `Tag` | `tag` | feature 标记、筛选条件、分类标签使用。 |

## 内容、媒体与辅助

| 交互需求 | 首选组件 | 文档路由 | 使用提示 |
| --- | --- | --- | --- |
| 图标 | `Icon` | `icon` | 工具按钮、导航、状态提示使用官方图标，不要手写 SVG。 |
| 图片展示 | `Image` | `image` | 需要加载失败回退或统一图片样式时使用。 |
| 用户头像 | `Avatar` | `avatar` | 用户、团队、账号身份展示使用。 |
| Markdown 或 HTML 文本 | `TextView` | `text-view` | 帮助文档、说明、日志富文本展示使用。 |
| 复制到剪贴板 | `Clipboard` | `clipboard` | Token、路径、命令、hash 复制使用。 |
| 键盘快捷键展示 | `Kbd` | `kbd` | 显示 `Cmd+K`、`Esc` 等快捷键。 |
| 字段标签 | `Label` | `label` | 表单字段、可访问名称使用。 |
| 分组框 | `GroupBox` | `group-box` | 表单或设置中的小范围视觉分组使用。 |

## 常见误用

| 不要这样做 | 应该这样做 |
| --- | --- |
| 用 `div()` 手写应用侧边栏、选中态和导航分组。 | 使用 `Sidebar`、`SidebarGroup`、`SidebarMenuItem`。 |
| 用按钮加状态模拟页面 tabs。 | 使用 `Tabs`。 |
| 用普通输入框输入数字、日期、颜色。 | 使用 `NumberInput`、`DatePicker`、`ColorPicker`。 |
| 用自定义弹层处理删除确认。 | 使用 `AlertDialog`。 |
| 用普通列表渲染上万条数据。 | 使用 `VirtualList` 或 `DataTable`。 |
| 在图标按钮旁边写说明文字解释含义。 | 使用 `Icon` + `Tooltip`。 |
| 在设置页里堆散乱表单。 | 使用 `Settings`、`Form`、`GroupBox`。 |

## 实现约定

- 公开 Rust API 仍遵守本仓库 rustdoc 规则：公开类型、函数、方法、模块都写中文 rustdoc。
- 桌面应用根布局优先组合 `Sidebar`、`TitleBar`、`StatusBar`、`Scrollable`、`Resizable`。
- feature 页面内部优先使用语义组件，不要把所有 UI 都降级为 `div()`。
- 符合标题摘要、刷新、筛选/操作、数据主体结构的标准 CRUD Panel，优先使用
  `nexora::desktop::{CrudPanel, CrudPanelToolbar}`；查询、创建、导入、导出等命令放入工具栏
  action 区，顶部刷新只负责重新拉取当前数据。
- CRUD 资源表格优先用 `CrudTableRow` 派生宏加 `CrudTableDelegate<T>`；它只增强
  gpui-component `DataTable` 的常规样板，不改变 `Column`、`TableState` 和 `TableDelegate`
  的原生用法。
- 修改或新增复杂交互前，打开对应组件文档确认当前 API。
- 如果需要组件库没有覆盖的新交互，先做薄封装，并把封装限制在当前 feature 或明确的共享 UI crate 中。
