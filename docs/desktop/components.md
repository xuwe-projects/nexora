---
title: 组件
order: 3
---

# 组件

Nexora 的桌面组件不是替代 `gpui-component`，而是在它之上补齐框架层常用的应用骨架、
CRUD 页面、表单对话框、字段容器和层级选择器。应用仍然直接依赖并导入 `gpui` 与
`gpui_component`；Nexora 只通过 `nexora::desktop` 暴露跨应用稳定复用的增强组件。

## 快速开始

生成的桌面应用通常只需要启用 Nexora 的 `desktop, derive` feature，并直接使用
`gpui`、`gpui_component` 的原生组件：

```toml
[dependencies]
nexora = { version = "0.19.0", features = ["desktop", "derive"] }
gpui = { workspace = true }
gpui-component = { workspace = true }
theme = { workspace = true }
```

常用导入如下：

```rust
use gpui::{Context, Entity, Render, Window};
use gpui_component::{
    Sizable as _,
    button::Button,
    input::{Input, InputEvent, InputState},
    table::{Column, DataTable, TableState},
};
use nexora::desktop::{
    Cascader, CascaderEvent, CascaderOption, CascaderState,
    CrudPanel, CrudPanelToolbar, CrudTableDelegate, CrudTableSelection,
    FormDialog, FormDialogState, FormItem,
    LabeledControl, TableCell,
};
```

组件状态应在 Feature 或页面私有组件的初始化阶段创建并长期保存。`render` 只读取状态并构造
元素，不创建 `InputState`、订阅、异步任务或长期 Entity。

## 组件总览

| 组件 | 用途 | 状态归属 |
| --- | --- | --- |
| `FormDialog` | 创建/编辑资源的标准表单对话框，带草稿追踪和未保存确认 | `Entity<FormDialogState>` |
| `FormItem` | `FormDialog` 中的标准字段行 | 无长期状态 |
| `LabeledControl` | 标签、说明、错误文本容器；也可作为类型化字段 Entity | 视觉模式无状态；字段模式为 `Entity<LabeledControl<V>>` |
| `CrudPanel` | 标准资源管理页骨架：摘要、工具栏、主体内容 | 无长期状态 |
| `CrudPanelToolbar` | CRUD 筛选区与操作区 | 无长期状态 |
| `CrudTableDelegate` | 把业务行接入 `gpui_component::DataTable` | 保存在 `TableState` 中 |
| `CrudTableSelection` | CRUD 表格的受控选择列 | 调用方持有 selected IDs |
| `TableCell` / `TableHeaderCell` | 表格正文与表头对齐辅助 | 无长期状态 |
| `Cascader` | 单选级联选择器 | `Entity<CascaderState>` |
| `SidebarRegion` | Sidebar Header/Footer 内稳定命中区域 | 无长期状态 |

## FormDialog

`FormDialog` 是业务资源创建和编辑的默认容器。它固定提供标题、可选描述、可滚动内容区、
取消和提交操作；遮罩只覆盖当前 Feature Panel，不覆盖 Sidebar 或窗口级菜单。点击遮罩不会
关闭表单，取消、右上角关闭和提交才是明确意图。

### 基础示例

```rust
use gpui::{Context, Entity, Render, Subscription, Window};
use gpui_component::{
    Sizable as _,
    input::{InputEvent, InputState},
};
use nexora::desktop::{FormDialog, FormDialogState, FormItem};

struct UserEditor {
    form: Entity<FormDialogState>,
    name: Entity<InputState>,
    email: Entity<InputState>,
    _name_subscription: Subscription,
}

impl UserEditor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = cx.new(FormDialogState::new);
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("姓名"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("邮箱"));

        let tracked_form = form.clone();
        let _name_subscription = cx.subscribe(&name, move |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                tracked_form.update(cx, |form, cx| {
                    form.set_field_draft(
                        "name",
                        "姓名",
                        "",
                        input.read(cx).value().to_string(),
                        cx,
                    );
                });
            }
        });

        Self { form, name, email, _name_subscription }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.update(cx, |form, cx| {
            form.reset_fields(cx);
            form.open(window, cx);
        });
    }
}

impl Render for UserEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        FormDialog::new("create-user-dialog", self.form.clone())
            .title("创建用户")
            .description("填写基础资料后创建账号。")
            .child(FormItem::new("姓名").required().input(&self.name))
            .child(FormItem::new("邮箱").input(&self.email))
            .submit_label("创建")
            .submit_disabled(self.name.read(cx).value().trim().is_empty())
            .with_size(theme::component_size(cx))
            .on_submit(cx.listener(Self::submit))
    }
}
```

Feature 应在 `initialize` 或页面组件构造阶段创建表单 Entity，并让 `panel_overlay` 始终返回同
一个对话框层。不要在 `render` 中临时创建输入框、订阅或表单状态。

### API

| 类型 | API | 说明 |
| --- | --- | --- |
| `FormDialog` | `new(id, state)` | 创建绑定长期 `FormDialogState` 的对话框 |
| `FormDialog` | `title(title)` / `description(text)` | 设置标题区 |
| `FormDialog` | `columns(n)` | 标准字段按多列网格布局，最小为 `1` |
| `FormDialog` | `child(FormItem)` | 添加标准字段 |
| `FormDialog` | `section(element)` | 添加整行自定义内容，例如权限列表或提示 |
| `FormDialog` | `cancel_label(text)` / `submit_label(text)` | 覆盖按钮文案 |
| `FormDialog` | `submit_disabled(bool)` | 只禁用提交按钮，不影响取消 |
| `FormDialog` | `max_panel_height_ratio(ratio)` | 限制对话框相对当前 Panel 的最大高度，范围 `0.1..=1.0` |
| `FormDialog` | `on_submit(handler)` | 提交回调；没有默认业务行为 |
| `FormDialog` | `on_cancel(handler)` | 覆盖默认取消和未保存确认 |
| `FormDialog` | `with_size(size)` | 跟随主题组件尺寸 |
| `FormDialogState` | `open(window, cx)` / `close(window, cx)` | 打开或关闭，并处理焦点恢复 |
| `FormDialogState` | `set_submitting(bool, cx)` | 提交期间禁用取消、关闭和重复提交 |
| `FormDialogState` | `set_field_draft(key, label, original, draft, cx)` | 记录字段原值与草稿 |
| `FormDialogState` | `reset_fields(cx)` / `mark_saved(cx)` | 重置草稿或把当前草稿标记为已保存 |
| `FormDialogState` | `is_dirty()` / `unsaved_fields()` / `draft_values()` | 读取未保存状态 |

### FormItem

`FormItem` 负责字段标签、说明、必填标记、错误文本、尺寸传递和常用控件组合。复杂控件可以
用 `element` 传入任意 GPUI 元素。

| API | 说明 |
| --- | --- |
| `FormItem::new(label)` | 创建普通字段 |
| `description(text)` / `required()` / `error(text)` | 设置字段辅助信息 |
| `input(&state)` / `password_input(&state)` / `number_input(&state)` | 组合官方输入组件 |
| `checkbox(id, checked, handler)` | 组合官方复选框 |
| `field(&entity)` | 使用类型化 `LabeledControl<V>` 字段 |
| `element(element)` | 传入自定义控件 |
| `disabled(bool)` | 禁用标准控件 |
| `full_row()` | 在多列表单中跨越整行 |

## LabeledControl

`LabeledControl` 有两种模式：

1. 纯视觉模式：`LabeledControl::new(label, child)` 只负责标签、说明、必填标记和错误文本。
2. 类型化字段模式：`input`、`password_input`、`number_input`、`checkbox` 创建 builder，
   `build(window, cx)` 后得到长期字段 Entity，可注册到 `FormDialogState` 统一提交校验。

### 纯视觉字段

```rust
use gpui::px;
use gpui_component::{Sizable as _, input::Input};
use nexora::desktop::LabeledControl;

LabeledControl::new("关键字", Input::new(&self.keyword))
    .description("按名称、编码或邮箱搜索。")
    .width(px(260.0))
    .with_size(theme::component_size(cx))
```

### 类型化字段

```rust
use gpui::SharedString;
use gpui_component::input::InputState;
use nexora::desktop::{FormDialogState, FormItem, LabeledControl};

let name_input = cx.new(|cx| InputState::new(window, cx));
let name_field = LabeledControl::input("name", "姓名", &name_input)
    .required("请输入姓名")
    .pattern(r"^.{2,32}$", "姓名长度需要在 2 到 32 个字符之间")
    .on_change(|event| async move {
        tracing::debug!(name = %event.value(), "用户姓名已变更");
    })
    .build(window, cx);

let form = cx.new(|cx| FormDialogState::new(cx).field(&name_field));
let item = FormItem::field(&name_field);
```

### API

| 类型 | API | 说明 |
| --- | --- | --- |
| `LabeledControl<()>` | `new(label, child)` | 纯视觉字段容器 |
| `LabeledControl<()>` | `input(key, label, state)` | 文本字段 builder |
| `LabeledControl<()>` | `password_input(key, label, state)` | 密码字段 builder |
| `LabeledControl<()>` | `number_input::<V>(key, label, state)` | 数值字段 builder，`V` 为内置数值类型或可选数值 |
| `LabeledControl<()>` | `checkbox(key, label, id, checked)` | 布尔字段 builder |
| `LabeledControlBuilder<V>` | `description(text)` | 字段说明 |
| `LabeledControlBuilder<V>` | `required(message)` | 必填规则 |
| `LabeledControlBuilder<V>` | `pattern(regex, message)` | 文本正则规则 |
| `LabeledControlBuilder<V>` | `parse_error(message)` | 数值转换失败文案 |
| `LabeledControlBuilder<V>` | `on_input(handler)` / `on_change(handler)` / `on_blur(handler)` | 类型化异步事件 |
| `LabeledControlBuilder<V>` | `build(window, cx)` | 初始化字段 Entity |
| `LabeledControl<V>` | `key()` / `value()` / `visible_error()` / `has_error()` | 读取字段状态 |

异步事件中的 `event.current_target()` 可以设置或清除当前事件来源的错误；字段已经产生新
revision 后，旧异步结果会被丢弃，避免慢请求覆盖新输入。

## CrudPanel

`CrudPanel` 是标准资源管理页面的三段式骨架：顶部摘要卡片、可选工具栏、主体内容区。主体
使用 `flex_1` 和 `min_h_0` 填满剩余高度，因此表格、虚拟列表或编辑器可以自己管理滚动。

```rust
use gpui_component::{
    Sizable as _,
    button::Button,
    input::Input,
};
use nexora::desktop::{CrudPanel, CrudPanelToolbar};

let toolbar = CrudPanelToolbar::new()
    .filter(Input::new(&self.keyword).placeholder("搜索城市"))
    .action(Button::new("search").label("查询"))
    .action(Button::new("create").primary().label("创建"));

CrudPanel::new("城市", self.render_table(window, cx))
    .description("维护城市及所属国家或地区")
    .refresh("refresh-cities", self.loading, false, cx.listener(Self::reload))
    .toolbar(toolbar)
    .with_size(theme::component_size(cx))
```

### API

| 类型 | API | 说明 |
| --- | --- | --- |
| `CrudPanel` | `new(title, content)` | 创建资源管理 Panel |
| `CrudPanel` | `description(text)` | 顶部摘要说明 |
| `CrudPanel` | `refresh(id, loading, disabled, handler)` | 右上角刷新当前数据 |
| `CrudPanel` | `toolbar(toolbar)` | 替换整块工具栏 |
| `CrudPanel` | `filter(element)` / `filters(elements)` | 向默认工具栏追加筛选控件 |
| `CrudPanel` | `action(element)` / `actions(elements)` | 向默认工具栏追加操作控件 |
| `CrudPanel` | `has_toolbar()` | 查询是否有工具栏内容 |
| `CrudPanel` | `with_size(size)` | 跟随主题组件尺寸 |
| `CrudPanelToolbar` | `new()` | 创建空工具栏 |
| `CrudPanelToolbar` | `filter` / `filters` | 添加筛选区控件 |
| `CrudPanelToolbar` | `action` / `actions` | 添加操作区控件 |
| `CrudPanelToolbar` | `is_empty()` | 判断工具栏是否为空 |

页面级“重新拉取当前数据”放在 `refresh`；查询、创建、导入、导出和批量操作放在 toolbar
action 区，避免刷新和查询语义混在一起。

## CrudTableRow 与 CrudTableDelegate

CRUD 表格优先用 `#[derive(nexora::CrudTableRow)]` 描述行数据，再用
`CrudTableDelegate<T>` 接入 `gpui_component::DataTable`。复杂表格仍可直接实现原生
`TableDelegate`。

### 基础示例

```rust
use gpui_component::table::{Column, DataTable, TableState};
use nexora::desktop::{CrudTableDelegate, TableCell};

#[derive(Clone, nexora::CrudTableRow)]
struct CityRow {
    #[nexora(row_id, column(name = "ID", width = 64., fixed_left))]
    id: u64,
    #[nexora(column(title = "城市", width = 160., sortable))]
    name: String,
    #[nexora(column(title = "状态", width = 76., align = "center", render = Self::status_cell))]
    enabled: bool,
}

impl CityRow {
    fn status_cell(row: &Self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> TableCell {
        TableCell::new(if row.enabled { "启用" } else { "停用" }).center()
    }
}

let delegate = CrudTableDelegate::new(rows)
    .action_column(
        Column::new("actions", "操作").width(gpui::px(160.)).selectable(false),
        |row, _window, _cx| render_row_actions(row),
    )
    .empty_title("暂无城市")
    .empty_description("创建城市后会显示在这里。");

let table = DataTable::new(cx.new(|cx| TableState::new(delegate, window, cx))).bordered(true);
```

### 派生属性

| 属性 | 说明 |
| --- | --- |
| `#[nexora(row_id)]` | 声明唯一业务 ID，必须且只能有一个 |
| `#[nexora(skip)]` | 字段不生成列 |
| `#[nexora(column)]` | 使用字段名作为列 key 和标题 |
| `column(key = "...")` | 覆盖列 key |
| `column(name = "...")` / `column(title = "...")` | 覆盖表头文案 |
| `column(width = 120.)` / `min_width` / `max_width` | 设置列宽 |
| `column(sortable)` / `ascending` / `descending` | 设置排序能力和默认方向 |
| `column(fixed_left)` | 固定在左侧 |
| `column(resizable = false)` / `movable = false` / `selectable = false` | 转发原生列行为 |
| `column(header_align = "left")` | 表头对齐，支持 `left`、`center`、`right` |
| `column(align = "right")` / `cell_align = "right"` | 正文水平对齐 |
| `column(vertical_align = "top")` | 正文垂直对齐，支持 `top`、`middle`、`bottom` |
| `column(render = Self::render_status)` | 自定义正文渲染函数 |
| `column(text = Self::status_text)` | 自定义文本导出函数 |

### Delegate API

| API | 说明 |
| --- | --- |
| `CrudTableDelegate::new(rows)` | 从初始行创建 delegate，并校验 ID 唯一 |
| `rows()` / `columns()` | 读取当前行和列 |
| `replace_rows(rows)` / `append_rows(rows)` | 替换或追加已加载行 |
| `update_rows(|rows| ...)` | 只修改非身份字段或调整行顺序 |
| `set_total(total)` | 设置数据源总行数，用于加载更多 |
| `set_loading(bool)` / `set_loading_more(bool)` | 设置整表加载或下一页加载状态 |
| `on_load_more(handler)` | 滚动到底部时加载下一页 |
| `selection(selection)` | 启用受控选择列 |
| `set_selected_ids(ids)` | 回写受控 selected IDs 快照 |
| `selection_enabled()` / `loaded_rows_checked(cx)` / `has_selectable_loaded_rows(cx)` | 读取选择状态 |
| `action_column(column, render)` | 追加操作列 |
| `action_text(text)` | 为最近追加的操作列设置文本导出 |
| `empty_title(text)` / `empty_description(text)` | 空状态文案 |

### 受控选择

```rust
use nexora::desktop::{CrudTableDelegate, CrudTableSelection};

let selection = CrudTableSelection::new(
    self.selected_city_ids.clone(),
    cx.listener(Self::select_city),
    cx.listener(Self::select_loaded_cities),
)
.row_selectable(|row, _cx| row.enabled);

let delegate = CrudTableDelegate::new(rows).selection(selection);
```

选择列不直接修改业务集合。单行和表头点击只派发 `RowSelectionEvent` 或
`LoadedRowsSelectionEvent`，调用方更新自己的 selected IDs 后，再在 `TableState` 更新上下文中
调用 `set_selected_ids` 并通知刷新。

## Cascader

`Cascader` 是单选级联选择器，复用 `gpui-component` 的 Popover、Input、Button、Icon 和滚动
能力。它支持任意深度选项、稳定值路径、禁用节点、清空、搜索、路径分隔符和
`change_on_select`。

```rust
use nexora::desktop::{Cascader, CascaderEvent, CascaderOption, CascaderState};

let options = [
    CascaderOption::new("resources", "资料中心").children([
        CascaderOption::new("production", "生产建模").children([
            CascaderOption::new("workshop", "车间"),
            CascaderOption::new("line", "线别"),
        ]),
    ]),
];

let cascader = cx.new(|cx| {
    CascaderState::new("resource-cascader", options, window, cx)
        .placeholder("请选择资源")
        .separator(" / ")
        .allow_clear(true)
        .searchable(true)
});

cx.subscribe(&cascader, |_, _, event: &CascaderEvent, _| {
    let CascaderEvent::Change(selection) = event;
    tracing::info!(values = ?selection.values(), labels = ?selection.labels());
});

Cascader::new(&cascader).w(gpui::px(280.0))
```

### API

| 类型 | API | 说明 |
| --- | --- | --- |
| `CascaderOption` | `new(value, label)` | 创建选项节点 |
| `CascaderOption` | `disabled(bool)` | 禁用节点 |
| `CascaderOption` | `child(option)` / `children(options)` | 添加子节点 |
| `CascaderOption` | `value()` / `label()` / `is_disabled()` / `children_ref()` / `is_leaf()` | 读取节点信息 |
| `CascaderState` | `new(id, options, window, cx)` | 初始化长期状态和内部搜索输入 |
| `CascaderState` | `placeholder(text)` / `separator(text)` | 设置展示文案 |
| `CascaderState` | `allow_clear(bool)` / `searchable(bool)` / `change_on_select(bool)` / `disabled(bool)` | 设置交互能力 |
| `CascaderState` | `set_search_placeholder(text, window, cx)` | 更新搜索框提示 |
| `CascaderState` | `selection()` / `is_open()` | 读取当前状态 |
| `CascaderState` | `set_value(values, cx)` | 受控回填；路径不存在时返回 `CascaderValueError` |
| `CascaderState` | `clear(cx)` | 清空并派发空路径变更事件 |
| `CascaderSelection` | `values()` / `labels()` / `is_empty()` | 读取选择结果 |
| `CascaderValueError` | `value()` / `depth()` | 定位回填失败的值和层级 |
| `Cascader` | `new(&state)` | 渲染绑定状态的选择器元素 |

每个 Cascader 的 `id` 以及同级 option `value` 都应稳定唯一。业务提交使用 `values()`，不要把
展示文本当作后端值。

## 表格辅助组件

`TableHeaderCell` 默认水平、垂直居中；`TableCell` 默认垂直居中、水平靠左。二者适合在
手写 `TableDelegate` 或自定义 `CrudTableRow` 渲染函数中复用。

```rust
use nexora::desktop::{TableCell, TableHeaderCell, TableCellVerticalAlign};

TableHeaderCell::new("金额").right();
TableCell::new("128.00").right().middle();
TableCell::new("说明文本").vertical_align(TableCellVerticalAlign::Top);
```

| 组件 | API |
| --- | --- |
| `TableHeaderCell` | `new(label)`、`element(element)`、`align`、`left`、`center`、`right`、`alignment` |
| `TableCell` | `new(content)`、`align`、`left`、`center`、`right`、`vertical_align`、`top`、`middle`、`bottom`、`horizontal_alignment`、`vertical_alignment` |

## SidebarRegion

`SidebarRegion::new(id)` 用于 Sidebar Header/Footer 内由应用自行控制交互视觉的区域。它只
提供稳定元素 ID、横向排列、完整宽度和样式扩展点，不隐式添加 hover、selected 背景、圆角、
cursor 或点击语义。

```rust
use gpui_component::StyledExt as _;
use nexora::desktop::SidebarRegion;

SidebarRegion::new("factory-switcher")
    .px_2()
    .py_1()
    .child(current_factory_name)
```

品牌、工厂选择器、账号菜单等区域应使用不同稳定 ID，并且只在真正可交互时自行添加
hover、selected、cursor 和点击行为。

## 使用原则

- 优先使用 `gpui_component` 原生 Button、Input、Select、Popover、DataTable、Dialog 等组件。
- 当页面是典型资源管理页时，用 `CrudPanel` + `CrudTableDelegate` 统一结构。
- 当交互是创建/编辑资源时，用 `FormDialog`，并把输入状态、订阅和提交任务放在页面私有组件。
- 有声明式校验、异步校验或提交前聚焦需求的字段，用类型化 `LabeledControl` 并注册到 `FormDialogState`。
- 只有需要层级路径值时才使用 `Cascader`；普通枚举选择继续使用 `gpui_component` 的选择类组件。
- 所有支持尺寸语义的组件都传入 `theme::component_size(cx)`，让设置中的组件尺寸即时生效。
