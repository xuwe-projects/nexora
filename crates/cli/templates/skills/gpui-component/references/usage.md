# gpui-component 使用指南

**目录：** [初始化](#初始化) · [组件类型](#组件类型) · [常用组件](#常用组件)（Button、Input、Select、Checkbox、Icon、Dialog、Notification、Tabs、Tooltip、Form、List）· [主题](#主题) · [布局辅助函数](#布局辅助函数) · [遮罩层](#遮罩层dialogsheetnotification) · [共享 trait](#共享-trait)

## 初始化

### 1. Cargo.toml

```toml
[dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit"] }
gpui-component = { git = "https://github.com/longbridge/gpui-component" }
gpui-component-assets = { git = "https://github.com/longbridge/gpui-component" } # 可选图标
```

### 2. 初始化代码

```rust
fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx); // 必须首先调用

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|_| MyApp);
                    cx.new(|cx| Root::new(view, window, cx)) // 用 Root 包装首层视图
                }).expect("无法打开窗口");
            }).detach();
        });
}
```

每个窗口的第一层子元素都**必须使用 `Root`**。`Root` 保存 Dialog、Sheet 和 Notification
状态，但不会自动把遮罩层加入业务元素树；业务根视图仍须按本文“遮罩层”章节显式渲染。

---

## 组件类型

### 无状态组件（大多数组件）

直接在 `render` 中使用，无需保存状态：

```rust
use gpui_component::button::Button;

impl Render for MyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Button::new("btn").primary().label("Submit")
            .on_click(|_, _, _| tracing::debug!("按钮已点击"))
    }
}
```

### 有状态组件（Input、Select、Combobox 等）

需要在视图中保存一个 `Entity<State>`：

```rust
use gpui_component::input::{Input, InputState};

struct MyView {
    name: Entity<InputState>,
}

impl MyView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("你的姓名")),
        }
    }
}

impl Render for MyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Input::new(&self.name)
    }
}
```

---

## 常用组件

### Button

```rust
use gpui_component::button::{Button, ButtonGroup};

// 变体
Button::new("btn").label("默认")
Button::new("btn").primary().label("主要")
Button::new("btn").danger().label("删除")
Button::new("btn").warning().label("警告")
Button::new("btn").success().label("成功")
Button::new("btn").ghost().label("幽灵")
Button::new("btn").link().label("链接")

// 状态
Button::new("btn").label("文本").disabled(true)
Button::new("btn").label("文本").loading(true)
Button::new("btn").label("文本").selected(true)

// 带图标
Button::new("btn").icon(IconName::Plus).label("添加")

// 尺寸
Button::new("btn").xsmall().label("XS")
Button::new("btn").small().label("S")
Button::new("btn").large().label("L")

// 按钮组
ButtonGroup::new("group")
    .child(Button::new("a").label("A"))
    .child(Button::new("b").label("B"))
    .on_click(|indices, _, _| { /* 已选择的索引 */ })
```

### Input

```rust
use gpui_component::input::{Input, InputState};

// 在 new/init 中创建状态
let input = cx.new(|cx| InputState::new(window, cx)
    .placeholder("请输入文本……")
    .default_value("你好")
);

// 渲染
Input::new(&input)
Input::new(&input).cleanable(true)           // 清除按钮
Input::new(&input).disabled(true)
Input::new(&input).prefix(Icon::new(IconName::Search).small())
Input::new(&input).suffix(Button::new("b").ghost().icon(IconName::X).xsmall())
Input::new(&input).content_type(InputContentType::Password)
Input::new(&input).mask_toggle()             // 密码可见性切换
Input::new(&input).appearance(false)         // 移除默认边框和背景

// 读取值
let value = input.read(cx).value();

// 事件
cx.subscribe_in(&input, window, |view, state, event, window, cx| {
    match event {
        InputEvent::Change => { let v = state.read(cx).value(); }
        InputEvent::PressEnter { .. } => { /* 提交 */ }
        InputEvent::Focus | InputEvent::Blur => {}
    }
});
```

### Select

```rust
use gpui_component::select::{Select, SelectState};

// 简单字符串列表
let state = cx.new(|cx| {
    SelectState::new(vec!["苹果", "橙子", "香蕉"], Some(IndexPath::default()), window, cx)
});

// 渲染
Select::new(&state)
Select::new(&state).placeholder("请选择")

// 读取选中项
let selected = state.read(cx).selected_item();
```

### Checkbox / Switch / Radio

```rust
use gpui_component::{Checkbox, Switch};

// 无状态受控组件
Checkbox::new("cb").checked(self.checked)
    .on_click(|checked, _, cx| { /* &bool */ })

Switch::new("sw").checked(self.enabled)
    .on_click(|checked, _, cx| {})
```

### Icon

```rust
use gpui_component::{Icon, IconName};

Icon::new(IconName::Check)
Icon::new(IconName::Search).small()
Icon::new(IconName::Plus).large().text_color(cx.theme().primary)
```

### Dialog

```rust
use gpui_component::WindowExt as _;

// 从窗口上下文打开
window.open_dialog(cx, |dialog, _, cx| {
    dialog
        .title("确认")
        .child(div().child("确定要继续吗？"))
        .footer(|this, _, cx| {
            this.child(Button::new("cancel").label("取消"))
                .child(Button::new("ok").primary().label("确定")
                    .on_click(|_, window, cx| { window.close_dialog(cx); }))
        })
});
```

### Notification

```rust
// 简单字符串消息
window.push_notification("保存成功！", cx);

// 带类型变体
window.push_notification(
    Notification::new("上传完成").info().message("文件已上传"),
    cx,
);
```

### Tabs

```rust
use gpui_component::tab::{Tab, TabBar};

TabBar::new("tabs")
    .child(Tab::new("tab1").child("概览"))
    .child(Tab::new("tab2").child("设置"))
    .child(Tab::new("tab3").child("日志"))
```

### Tooltip

```rust
// 在任何带 .id() 的元素上添加 .tooltip()：
div()
    .id("my-btn")
    .tooltip(|window, cx| Tooltip::new("删除项目").build(window, cx))
    .child("删除")

// 或直接在 Button 上使用：
Button::new("btn").icon(IconName::Trash).tooltip("删除")
```

### Form

```rust
use gpui_component::form::{v_form, h_form, field};

// 垂直表单
v_form()
    .child(field().label("姓名").child(Input::new(&self.name)))
    .child(field().label("邮箱").child(Input::new(&self.email)))
    .child(Button::new("submit").primary().label("提交"))

// 水平标签对齐
h_form()
    .child(field().label("用户名").child(Input::new(&self.username)))
```

### List（可搜索、虚拟化）

```rust
use gpui_component::list::{List, ListState, ListDelegate, ListItem, ListEvent};

// 为数据类型实现 ListDelegate，然后：
let list_state = cx.new(|cx| ListState::new(MyDelegate::new(), window, cx));

// 渲染
List::new(&list_state)
// 事件
cx.subscribe(&list_state, |this, _, event, cx| {
    if let ListEvent::Select(index_path) = event {
        // 处理选择
    }
});
```

---

## 主题

```rust
use gpui_component::ActiveTheme as _;

// 访问颜色
cx.theme().primary
cx.theme().background
cx.theme().foreground
cx.theme().border
cx.theme().surface
cx.theme().muted
cx.theme().destructive

// 在样式中使用
div()
    .bg(cx.theme().surface)
    .text_color(cx.theme().foreground)
    .border_color(cx.theme().border)
```

### 切换主题

```rust
use gpui_component::Theme;

// 切换浅色/深色模式
cx.update_global::<Theme, _>(|theme, cx| {
    theme.toggle_mode(cx);
});

// 加载具名主题
Theme::global_mut(cx).apply_config(&theme_config);
```

---

## 布局辅助函数

gpui-component 为 GPUI 扩展了便捷的布局方法：

```rust
h_flex()    // div().flex().flex_row().items_center()
v_flex()    // div().flex().flex_col()

// 常用模式
h_flex().gap_2().items_center()
    .child(Icon::new(IconName::User))
    .child(label("用户名"))

v_flex().gap_4().p_4()
    .child(Input::new(&self.name))
    .child(Input::new(&self.email))
    .child(Button::new("submit").primary().label("提交"))
```

---

## 遮罩层（Dialog、Sheet、Notification）

要渲染遮罩层，请在第一层视图的 `render` 中加入以下内容：

```rust
impl Render for MyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.main_content(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
```

---

## 共享 trait

所有组件都遵循构建器模式 `Component::new("id").method().method()`：

- `Sizable`：`.xsmall()` / `.small()` / `.medium()`（默认）/ `.large()`
- `Disableable`：`.disabled(bool)`
- `Selectable`：`.selected(bool)`
- `Styled`：任意 GPUI 样式方法（`.w()`、`.bg()`、`.p_2()` 等）

对于本文未覆盖的组件，请从以下地址获取文档：
`https://longbridge.github.io/gpui-component/docs/components/{name}.md`
