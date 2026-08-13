# Shell、CrudPanel 与表单组件选型审计

本记录对应 gpui-component 锁定 revision
`55b6bb88905d8e76cd23d9e3ebea3151dcdb84a0`。快速 usage 快照、来源校验值和已知示例冲突见
`.agents/skills/gpui-component/references/SOURCE.md`。复杂 API 以该 revision 的源码、完整组件文档和
`crates/story` 为准。

## 组件选型

| UI 区域 | 既有 Nexora 实现 | 官方候选 | 最终选择 | 薄包装或能力缺口 |
| --- | --- | --- | --- | --- |
| 窗口全局顶栏 | Panel Header 动作散落在页面标题栏 | `TitleBar`、`Button`、`Tooltip`、`Badge`、`Popover`、`Menu` | 官方组件组合，加 `ShellToolbarAction` 注册契约 | 只保留稳定 ID、排序和应用注册，不重复绘制控件 |
| Feature 标签 | 既有 Shell 标签运行时 | `TabBar`、`Tab` | 保留运行时，视觉和交互继续使用官方 Tabs | Nexora 只协调路由、拖放、固定与关闭 |
| Sidebar | 固定宽度、不可折叠的官方 Sidebar 组合 | `Sidebar`、`SidebarToggleButton`、`Input` | 使用官方 Icon collapse 与动画 | Nexora 只持久化设备级折叠偏好并协调搜索聚焦 |
| 全局搜索 | 无框架级宿主 | `Dialog`、`Input`、`ListItem`、`Alert`、`Spinner` | 官方组件组合，加 Provider/历史/异步状态 Entity | Provider 合并、revision、历史隔离和动作语义不属于单个官方控件 |
| 标准 CRUD 筛选 | `LabeledControl` 视觉包装 | `Form`、`Field`、官方输入控件 | 官方 Form/Field | 值转换、异步校验和服务器错误收敛为无视觉状态 |
| 标准 CRUD 表格 | `CrudTableRow`、delegate、`DataTableLayout` | `DataTable`、`Pagination`、`Skeleton` | 保留强类型/持久化契约，渲染使用官方组件 | 行宏、服务端查询缓存与 workspace 列布局是框架能力 |
| 创建/编辑 | `FormDialog` + 自定义 FormItem 网格 | `Form`、`Field`、官方输入控件 | `FormDialog` 仅协调 Panel 范围流程，字段视觉使用官方 Form/Field | 官方 Dialog 是窗口级，无法替代 Feature Panel 遮罩 |
| 分组内容 | 公共 `Card` | `GroupBox::fill()`、`GroupBox::outline()`、普通布局 | 按真实语义使用 GroupBox 或布局 | 不建立 Card 别名 |

## 删除、薄化与保留

删除：

- `PanelHeader`、面包屑和 `PanelHeaderAction`。
- 公共 `Card`。
- `FormItem`、`FormItemControl`。
- `LabeledControl` 的视觉容器、Render 与视觉 builder。
- 旧任意内容 `CrudPanel`、三卡片结构和页面级 refresh API。

薄化：

- `FormDialog` 只保留 Panel 范围遮罩、草稿、脏字段、取消确认、提交门禁、字段校验协调、
  焦点与 Task 生命周期。
- `ShellToolbarAction` 标准路径只生成官方图标按钮；复杂内容必须由应用组合官方 Popover/Menu。
- `CrudPanel<Row, Query>` 只服务标准单主数据集，Feature 保有权限和业务动作。

继续保留的 Nexora 能力及官方缺口：

- `PanelDialog`：官方 Dialog 是窗口级，不能只遮罩当前 Feature Panel。
- `Cascader`：Select/Combobox/SearchableList 不表达层级路径级联选择。
- `CrudTableRow` 与标准 delegate：提供强类型行身份、列元数据和减少样板。
- `DataTableLayout`：官方 DataTable 不负责 workspace 列顺序和宽度持久化。
- `TableHeaderCell` / `TableCell`：补充复杂内容和稳定垂直对齐语义。
- `SidebarRegion`：界定应用品牌与上下文插槽，不注入交互视觉。
- `LoginGate`：业务复合界面，内部标准控件仍使用官方组件。
- `window_layers`：锁定版本 Root 仍要求宿主显式渲染 Dialog、Sheet 和 Notification 层。

## iMES 审计边界

标准单列表页面迁移到新 `CrudPanel<Row, Query>`。BOM、库存盘点、条码追踪等主从工作台只移除
废弃外壳并适配 Shell/刷新，不强行改造成单列表。仅在本次迁移触及的 iMES 私有控件直接重复
Button、Form、Dialog、Tabs 等官方能力时一并替换，其他业务视觉不扩大重写范围。
