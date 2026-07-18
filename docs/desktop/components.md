---
title: 公共桌面组件
order: 3
---

# 公共桌面组件

Nexora 优先直接使用 gpui-component；只有缺失的跨应用交互才在 `nexora::desktop` 提供增强
组件。应用仍直接依赖 `gpui` 与 `gpui-component`，不要通过 Nexora 转发它们的类型。

## FormDialog

`FormDialog` 是创建/编辑资源的默认表单容器，由三个固定区域组成：标题与可选描述、可纵向
滚动的内容区、取消与提交操作。它组合 `PanelDialog`，遮罩只覆盖当前 Feature 内容区，用户
仍可操作 Sidebar 与其他菜单。内容高度受 Panel 限制，字段过长时只滚动 y 轴。

```rust
use gpui::{Context, Entity, Render, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use nexora::desktop::{FormDialog, FormDialogState};

struct Editor {
    form: Entity<FormDialogState>,
    name: Entity<InputState>,
    _name_subscription: Subscription,
}

impl Editor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = cx.new(FormDialogState::new);
        let name = cx.new(|cx| InputState::new(window, cx));
        let tracked_form = form.clone();
        let subscription = cx.subscribe(&name, move |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                tracked_form.update(cx, |form, cx| {
                    form.set_field_draft(
                        "name",
                        "名称",
                        "已保存名称",
                        input.read(cx).value().to_string(),
                        cx,
                    );
                });
            }
        });
        Self { form, name, _name_subscription: subscription }
    }
}
```

渲染时使用 `FormDialog::new(id, state, title, content, on_submit)`。`on_submit` 没有默认业务
行为，调用方必须实现；可用 `description`、`cancel_label`、`submit_label`、
`submit_disabled` 和 `on_cancel` 定制。默认取消会检查全部草稿：无修改时关闭，有修改时列出
未保存字段与当前草稿并要求确认。自定义取消可以从同一个状态读取：

- `is_dirty()`：是否有任意未保存字段；
- `unsaved_fields()`：按稳定字段键排序的原值与草稿；
- `draft_values()`：全部字段的草稿快照；
- `set_submitting(true)`：异步提交期间禁止关闭和重复提交；
- `mark_saved()`：把当前草稿提升为保存基线；
- `close(window, cx)`：提交成功或自定义取消完成后关闭。

Feature 应在 `initialize` 创建表单组件 Entity，并让 `panel_overlay` 始终返回同一个对话框层；
不要在 `render` 中创建 Input、订阅或任务，也不要根据打开状态在 `Some` 与 `None` 之间切换。

## Cascader

`Cascader` 是单选级联选择器，复用 gpui-component 的 Popover、Input、Button、Icon 和滚动
能力。它支持任意深度选项、稳定值路径、禁用节点、清空、搜索、路径分隔符和
`change_on_select`，不会把展示文本当作提交值。

```rust
use gpui::{Context, Entity, Window};
use nexora::desktop::{Cascader, CascaderEvent, CascaderOption, CascaderState};

let options = [
    CascaderOption::new("resources", "资料中心").children([
        CascaderOption::new("production", "生产建模").children([
            CascaderOption::new("workshop", "车间"),
            CascaderOption::new("line", "线别"),
        ]),
    ]),
];
let cascader: Entity<CascaderState> = cx.new(|cx| {
    CascaderState::new("resource-cascader", options, window, cx)
        .placeholder("请选择资源")
        .separator(" / ")
        .allow_clear(true)
        .searchable(true)
});

cx.subscribe(&cascader, |_, _, event: &CascaderEvent, _| {
    let CascaderEvent::Change(selection) = event;
    // selection.values() 是 ["resources", "production", "workshop"]。
});

let element = Cascader::new(&cascader);
```

业务回填使用 `set_value`；路径中任一值不存在时返回包含失败值和深度的
`CascaderValueError`，并保持旧选择不变。每个 Cascader 的 `id` 以及同级 option `value` 都应
稳定唯一。长期 `Entity<CascaderState>` 必须在初始化阶段创建。

## Card 与 SidebarRegion

`Card` 提供与工作区背景不同的 `group_box` 内容面、主题边框、圆角和轻量阴影，表格、表单和
摘要不应只画一个与桌面同色的边框。业务继续用 GPUI 样式控制内边距与大小。

`SidebarRegion::new(id)` 只提供稳定命中区域和布局扩展点，不隐式添加 hover、selected、
cursor 或点击语义。品牌、工厂选择器和账号菜单应使用不同稳定 ID，各自决定交互视觉。
