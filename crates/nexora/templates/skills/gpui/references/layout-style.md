# 布局与样式

**目录：** [概述](#概述) · [快速开始](#快速开始) · [常用模式](#常用模式) · [样式方法](#样式方法) · [h_flex / v_flex](#h_flex--v_flex-辅助函数) · [Tailwind 简写](#tailwind-风格简写) · [溢出与滚动](#溢出与滚动) · [绝对定位](#绝对定位) · [层叠顺序](#层叠顺序) · [主题集成](#主题集成) · [条件样式](#条件样式) · [文本样式](#文本样式)

## 概述

GPUI 提供具有 Rust 类型安全保障的类 CSS 样式系统。

**关键概念：**

- Flexbox 布局系统
- 用于链式设置样式的 `Styled` trait
- 尺寸单位：`px()`、`rems()`、`relative()`
- 颜色、边框和阴影

## 快速开始

### 基础样式

```rust
use gpui::*;

div()
    .w(px(200.))
    .h(px(100.))
    .bg(rgb(0x2196F3))
    .text_color(rgb(0xFFFFFF))
    .rounded(px(8.))
    .p(px(16.))
    .child("带样式内容")
```

### Flexbox 布局

```rust
div()
    .flex()
    .flex_row()  // 列布局则使用 flex_col()
    .gap(px(8.))
    .items_center()
    .justify_between()
    .children([
        div().child("Item 1"),
        div().child("Item 2"),
        div().child("Item 3"),
    ])
```

### 尺寸单位

```rust
div()
    .w(px(200.))           // Pixels
    .h(rems(10.))          // 相对于字体大小
    .w(relative(0.5))      // 父元素宽度的 50%
    .min_w(px(100.))
    .max_w(px(400.))
```

## 常用模式

### 内容居中

```rust
div()
    .flex()
    .items_center()
    .justify_center()
    .size_full()
    .child("Centered")
```

### 卡片布局

```rust
div()
    .w(px(300.))
    .bg(cx.theme().surface)
    .rounded(px(8.))
    .shadow_md()
    .p(px(16.))
    .gap(px(12.))
    .flex()
    .flex_col()
    .child(heading())
    .child(content())
```

### 响应式间距

```rust
div()
    .p(px(16.))           // 四周内边距
    .px(px(20.))          // 水平内边距
    .py(px(12.))          // 垂直内边距
    .pt(px(8.))           // 顶部内边距
    .gap(px(8.))          // 子元素间距
```

## 样式方法

### 尺寸

```rust
.w(px(200.))              // Width
.h(px(100.))              // Height
.size(px(200.))           // 宽度和高度
.min_w(px(100.))          // 最小宽度
.max_w(px(400.))          // 最大宽度
```

### 颜色

```rust
.bg(rgb(0x2196F3))        // Background
.text_color(rgb(0xFFFFFF)) // 文本颜色
.border_color(rgb(0x000000)) // 边框颜色
```

### 边框

```rust
.border(px(1.))           // 边框宽度
.rounded(px(8.))          // 圆角半径
.rounded_t(px(8.))        // 顶部圆角
.border_color(rgb(0x000000))
```

### 间距

```rust
.p(px(16.))               // Padding
.m(px(8.))                // Margin
.gap(px(8.))              // Flex 子元素间距
```

### Flexbox

```rust
.flex()                   // 启用 Flexbox
.flex_row()               // 行方向
.flex_col()               // 列方向
.items_center()           // 项目居中对齐
.justify_between()        // 项目两端对齐
.flex_grow_1()            // 增长并填满空间
```

## h_flex / v_flex 辅助函数

gpui-component 提供以下简写辅助函数（从 `gpui_component` 导入）：

```rust
use gpui_component::{h_flex, v_flex};

// h_flex() = div().flex().flex_row().items_center()
h_flex()
    .gap_2()
    .child(icon)
    .child(label)

// v_flex() = div().flex().flex_col()
v_flex()
    .gap_4()
    .p_4()
    .child(input1)
    .child(input2)
    .child(submit_btn)
```

它们是 gpui-component 的标准布局原语，应优先于原始的 `div().flex()` 使用。

## Tailwind 风格简写

GPUI 提供 Tailwind 风格的间距与尺寸简写：

```rust
// 间距（0=0、1=4px、2=8px、3=12px、4=16px……）
.p_2()    // 内边距：8px
.px_4()   // 水平内边距：16px
.py_3()   // 垂直内边距：12px
.m_2()    // 外边距：8px
.gap_3()  // 间距：12px

// Size
.size_full()   // 宽度：100%，高度：100%
.size_4()      // 宽度：16px，高度：16px
.w_full()      // width: 100%
.h_full()      // height: 100%
.flex_1()      // flex: 1 1 0（填满剩余空间）
.flex_shrink_0() // 防止收缩
```

## 溢出与滚动

```rust
div()
    .overflow_hidden()          // 裁剪内容
    .overflow_x_hidden()        // 水平方向裁剪
    .overflow_y_scrollbar()     // 在 Y 轴显示滚动条
    .overflow_scroll()          // 两个方向均可滚动
```

## 绝对定位

```rust
div()
    .relative()                 // 相对定位（容器）
    .child(
        div()
            .absolute()         // 绝对定位
            .top_0()
            .right_0()
            .child("badge")
    )

// 内缩辅助函数
div().absolute().inset_0()      // 上/右/下/左均为 0（填满父元素）
div().absolute().top(px(8.)).left(px(8.))
```

## 层叠顺序

```rust
div()
    .relative()
    .child(content)
    .child(
        div()
            .absolute()
            .top_0()
            .right_0()
            .child("badge")
    ) // 后面的子元素通常绘制在前面的同级元素上方
```

GPUI 通用的 `Styled` API **不提供** `z_index(...)` 方法。

普通元素的层叠通常由以下因素控制：

- 父子组合关系
- 绝对定位
- 同级元素的渲染顺序（后渲染的元素覆盖先渲染的元素）

如果在本仓库中看到 `z_index(...)` 方法，请确认它属于当前使用的具体组件。例如，Dock Tile 系统中的 `TileItem::z_index(...)` 是自定义组件 API，而不是通用的 GPUI `Div` 样式方法。

## 主题集成

```rust
div()
    .bg(cx.theme().surface)
    .text_color(cx.theme().foreground)
    .border_color(cx.theme().border)
    .when(is_hovered, |el| {
        el.bg(cx.theme().hover)
    })
```

## 条件样式

```rust
use gpui::prelude::FluentBuilder as _;

div()
    .when(is_active, |el| el.bg(cx.theme().primary))
    .when(!is_active, |el| el.opacity(0.5))
    .when_some(optional_color.as_ref(), |el, color| el.bg(*color))
```

## 文本样式

```rust
div()
    .text_sm()          // 小字号
    .text_base()        // 基础字号
    .text_lg()          // 大字号
    .font_bold()        // 粗体
    .line_height_snug() // 更紧凑的行高
    .truncate()         // 单行溢出显示省略号
    .whitespace_nowrap()
```
