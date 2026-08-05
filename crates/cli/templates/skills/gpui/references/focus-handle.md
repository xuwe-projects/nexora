# 焦点与键盘导航

**目录：** [概述](#概述) · [快速开始](#快速开始) · [焦点事件](#焦点事件) · [键盘导航](#键盘导航) · [常用模式](#常用模式) · [最佳实践](#最佳实践)

## 概述

GPUI 的焦点系统用于实现键盘导航和焦点管理。

**关键概念：**

- **FocusHandle**：对可聚焦元素的引用
- **焦点跟踪**：跟踪当前获得焦点的元素
- **键盘导航**：使用 Tab / Shift-Tab 在元素之间移动
- **焦点事件**：`on_focus`、`on_blur`

## 快速开始

### 创建 FocusHandle

```rust
struct FocusableComponent {
    focus_handle: FocusHandle,
}

impl FocusableComponent {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}
```

### 让元素可以获得焦点

```rust
impl Render for FocusableComponent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_enter))
            .child("可聚焦内容")
    }

    fn on_enter(&mut self, _: &Enter, cx: &mut Context<Self>) {
        // 获得焦点时处理 Enter 键
        cx.notify();
    }
}
```

### 焦点管理

```rust
impl MyComponent {
    fn focus(&mut self, cx: &mut Context<Self>) {
        self.focus_handle.focus(cx);
    }

    fn is_focused(&self, cx: &App) -> bool {
        self.focus_handle.is_focused(cx)
    }

    fn blur(&mut self, cx: &mut Context<Self>) {
        cx.blur();
    }
}
```

## 焦点事件

### 处理焦点变化

```rust
impl Render for MyInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(cx);

        div()
            .track_focus(&self.focus_handle)
            .on_focus(cx.listener(|this, _event, cx| {
                this.on_focus(cx);
            }))
            .on_blur(cx.listener(|this, _event, cx| {
                this.on_blur(cx);
            }))
            .when(is_focused, |el| {
                el.bg(cx.theme().focused_background)
            })
            .child(self.render_content())
    }
}

impl MyInput {
    fn on_focus(&mut self, cx: &mut Context<Self>) {
        // 处理获得焦点
        cx.notify();
    }

    fn on_blur(&mut self, cx: &mut Context<Self>) {
        // 处理失去焦点
        cx.notify();
    }
}
```

## 键盘导航

### Tab 顺序

调用 `track_focus()` 的元素会自动参与 Tab 导航。

```rust
div()
    .child(
        input1.track_focus(&focus1)  // Tab 顺序：1
    )
    .child(
        input2.track_focus(&focus2)  // Tab 顺序：2
    )
    .child(
        input3.track_focus(&focus3)  // Tab 顺序：3
    )
```

### 容器内焦点

```rust
impl Container {
    fn focus_first(&mut self, cx: &mut Context<Self>) {
        if let Some(first) = self.children.first() {
            first.update(cx, |child, cx| {
                child.focus_handle.focus(cx);
            });
        }
    }

    fn focus_next(&mut self, cx: &mut Context<Self>) {
        // 自定义焦点导航逻辑
    }
}
```

## 常用模式

### 1. 挂载时自动聚焦

```rust
impl MyDialog {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        // 创建时聚焦
        focus_handle.focus(cx);

        Self { focus_handle }
    }
}
```

### 2. 焦点约束（模态框）

```rust
impl Modal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, cx| {
                if event.key == Key::Tab {
                    // 将焦点限制在模态框内
                    this.focus_next_in_modal(cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.render_content())
    }
}
```

### 3. 条件聚焦

```rust
impl Searchable {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .when(self.search_active, |el| {
                el.on_mount(cx.listener(|this, _, cx| {
                    this.focus_handle.focus(cx);
                }))
            })
            .child(self.search_input())
    }
}
```

## 最佳实践

### ✅ 跟踪交互元素的焦点

```rust
// ✅ 合适：为键盘交互跟踪焦点
input()
    .track_focus(&self.focus_handle)
    .on_action(cx.listener(Self::on_enter))
```

### ✅ 提供可见的焦点指示

```rust
let is_focused = self.focus_handle.is_focused(cx);

div()
    .when(is_focused, |el| {
        el.border_color(cx.theme().focused_border)
    })
```

### ❌ 不要：忘记跟踪焦点

```rust
// ❌ 错误：没有 track_focus，键盘导航无法工作
div()
    .on_action(cx.listener(Self::on_enter))
```
