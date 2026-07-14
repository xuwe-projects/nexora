# GPUI 组件代码风格指南

本指南基于对 `crates/ui/src` 中 `Button`、`Checkbox`、`Input`、`Select` 等组件的分析。

**目录：** [组件结构](#组件结构) · [必需的 trait 实现](#必需的-trait-实现) · [可选 trait](#可选-trait) · [变体模式](#变体模式) · [回调签名](#回调签名) · [导入组织](#导入组织) · [文档注释](#文档注释) · [应用用户样式覆盖](#应用用户样式覆盖) · [FluentBuilder 条件构建](#fluentbuilder-条件构建) · [主题颜色](#主题颜色) · [尺寸处理](#尺寸处理) · [检查清单](#新组件检查清单)

## 组件结构

### 标准无状态组件

```rust
use std::rc::Rc;

use crate::{ActiveTheme, Disableable, Sizable, Size, StyledExt as _, /* ... */};
use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
};

/// MyComponent 元素。
#[derive(IntoElement)]
pub struct MyComponent {
    // 1. 标识
    id: ElementId,
    base: Div,
    style: StyleRefinement,

    // 2. 配置
    size: Size,
    disabled: bool,
    selected: bool,
    tab_stop: bool,
    tab_index: isize,

    // 3. 内容
    label: Option<SharedString>,
    children: Vec<AnyElement>,

    // 4. 回调（放在最后）
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl MyComponent {
    /// 使用给定 id 创建新的 MyComponent。
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            base: div(),
            style: StyleRefinement::default(),
            size: Size::default(),
            disabled: false,
            selected: false,
            tab_stop: true,
            tab_index: 0,
            label: None,
            children: Vec::new(),
            on_click: None,
        }
    }

    /// 设置标签。
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 设置点击处理器。
    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}
```

### 有状态组件（可交互，需要 `.id()`）

包含鼠标交互（悬停、点击跟踪）的组件使用 `Stateful<Div>`：

```rust
use gpui::{Stateful, StatefulInteractiveElement as _, /* ... */};

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,  // 不能使用 Div；交互跟踪需要有状态元素
    // ...
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: div().flex_shrink_0().id(id),  // .id() 会将其变为 Stateful<Div>
            // ...
        }
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
```

---

## 必需的 trait 实现

```rust
// 所有接受子元素的组件
impl ParentElement for MyComponent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

// 所有外层 div 可设置样式的组件
impl Styled for MyComponent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

// 可交互组件（鼠标事件、悬停、点击）
impl InteractiveElement for MyComponent {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

// 实现 InteractiveElement 时必须同时实现
impl StatefulInteractiveElement for MyComponent {}

// 渲染
impl RenderOnce for MyComponent {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.base
            .id(self.id)
            // 最后应用用户样式覆盖
            .refine_style(&self.style)
            .children(self.children)
    }
}
```

---

## 可选 trait

```rust
impl Disableable for MyComponent {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for MyComponent {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Sizable for MyComponent {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}
```

实现 `Sizable` 后，会通过 `StyleSized` 自动获得 `.xsmall()`、`.small()`、`.medium()`、`.large()`。

---

## 变体模式

使用带默认方法实现的 `Variants` trait：

```rust
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

pub trait AlertVariants: Sized {
    fn with_variant(self, variant: AlertVariant) -> Self;

    fn info(self) -> Self { self.with_variant(AlertVariant::Info) }
    fn success(self) -> Self { self.with_variant(AlertVariant::Success) }
    fn warning(self) -> Self { self.with_variant(AlertVariant::Warning) }
    fn error(self) -> Self { self.with_variant(AlertVariant::Error) }
}

impl AlertVariants for MyAlert {
    fn with_variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }
}
```

---

## 回调签名

```rust
// 点击事件（ClickEvent 作为第一个参数）
on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>

// 状态变化（状态值作为第一个参数）
on_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>
on_change: Option<Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>>
on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>
```

始终使用 `Rc<dyn Fn>`，因为组件会被克隆并多次调用。

---

## 导入组织

```rust
// 1. 标准库
use std::rc::Rc;

// 2. crate 导入（项目内部）
use crate::{
    ActiveTheme, Disableable, Icon, IconName,
    Selectable, Sizable, Size, StyledExt as _,
    h_flex, v_flex,
};

// 3. gpui 导入
use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
    px, rems, relative,
};
```

---

## 文档注释

```rust
/// Checkbox 元素。               ← 结构体：单行，首字母大写，以句号结尾
#[derive(IntoElement)]
pub struct Checkbox { ... }

impl Checkbox {
    /// 使用给定 id 创建新的 Checkbox。           ← 构造函数
    pub fn new(id: impl Into<ElementId>) -> Self { ... }

    /// 设置复选框标签。                           ← setter
    pub fn label(mut self, label: impl Into<Text>) -> Self { ... }

    /// 设置复选框的点击处理器。
    ///
    /// `&bool` 参数表示点击后的新选中状态。
    pub fn on_click(mut self, ...) -> Self { ... }
}
```

- 结构体文档：`/// {Name} 元素。`
- 构造函数：`/// 使用给定 id 创建新的 {Name}。`
- setter：`/// 设置 {field}。`
- 不添加冗余注释，只记录不明显的行为

---

## 应用用户样式覆盖

使用 `refine_style` 将用户通过 `Styled` 设置的样式合并到根元素：

```rust
impl RenderOnce for MyComponent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            // 先应用组件默认值，再应用用户覆盖
            .refine_style(&self.style)
            .children(self.children)
    }
}
```

---

## FluentBuilder 条件构建

```rust
div()
    .when(self.disabled, |this| this.opacity(0.5).cursor_not_allowed())
    .when(self.selected, |this| this.bg(cx.theme().primary))
    .when_some(self.label.as_ref(), |this, label| {
        this.child(div().child(label.clone()))
    })
```

使用 `.when()` / `.when_some()` 时，始终导入 `use gpui::prelude::FluentBuilder as _;`。

---

## 主题颜色

```rust
// 在 render 中通过 cx.theme() 访问（需要导入 ActiveTheme）
use crate::ActiveTheme;

div()
    .bg(cx.theme().surface)
    .text_color(cx.theme().foreground)
    .border_color(cx.theme().border)
    .when(is_active, |el| el.bg(cx.theme().primary))
```

---

## 尺寸处理

```rust
// 根据 Size 获取像素值
let (width, height) = self.size.input_size();

// 或使用 match
let font_size = match self.size {
    Size::XSmall => rems(0.75),
    Size::Small => rems(0.875),
    Size::Medium | Size::Size(_) => rems(1.0),
    Size::Large => rems(1.125),
};
```

---

## 新组件检查清单

- [ ] `#[derive(IntoElement)]`
- [ ] 字段包含 `id: ElementId`、`base: Div`（或 `Stateful<Div>`）、`style: StyleRefinement`
- [ ] 实现 `RenderOnce`，并在根元素上调用 `.refine_style(&self.style)`
- [ ] 实现 `Styled`，返回 `&mut self.style`
- [ ] 接受子元素时实现 `ParentElement`
- [ ] 可交互时实现 `InteractiveElement` + `StatefulInteractiveElement`
- [ ] 存在尺寸变体时实现 `Sizable`
- [ ] 可禁用时实现 `Disableable`
- [ ] 可选择时实现 `Selectable`
- [ ] 回调使用 `Option<Rc<dyn Fn(...)>>`
- [ ] 为结构体和公开方法添加文档注释
- [ ] 导入 `prelude::FluentBuilder as _`
