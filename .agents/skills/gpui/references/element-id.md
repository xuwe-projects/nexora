# ElementId 标识符

`ElementId` 是 GPUI 元素的唯一标识符。具有以下需求的元素必须提供它：

- 处理鼠标事件（`on_click`、`on_hover` 等）
- 通过 `window.use_keyed_state` 保存状态
- 跟踪交互

## 将元素变为有状态元素

在 `div()` 上调用 `.id()` 以创建 `Stateful<Div>`：

```rust
div().id("my-element")          // 从 &str 创建 ElementId
div().id(42usize)               // 从 usize 创建 ElementId
div().id(ElementId::from(idx))  // Explicit
```

没有 `.id()` 的 div 无法接收鼠标事件或保存状态。

## 可接受的类型

```rust
impl Into<ElementId> for &str      // "my-id"
impl Into<ElementId> for String    // 例如 String::from("my-id")
impl Into<ElementId> for usize     // 0, 1, 2, ...
impl Into<ElementId> for u64
impl Into<ElementId> for SharedString
```

## 唯一性规则

ID 只需在同一个**有状态父元素的作用域**内唯一，而不需要全局唯一。GPUI 通过串联父级 ID 构建 `GlobalElementId`：

```rust
div().id("app").child(
    div().id("list1").children(vec![
        div().id(1usize).child("项目 1"),  // GlobalId：["app", "list1", 1]
        div().id(2usize).child("项目 2"),  // GlobalId：["app", "list1", 2]
    ])
).child(
    div().id("list2").children(vec![
        div().id(1usize).child("项目 1"),  // GlobalId：["app", "list2", 1]，无冲突
    ])
)
```

不同父作用域中的项目可以复用简单 ID（整数或短字符串）。

## 在组件结构体中使用

组件应始终保存 `id: ElementId`，并通过 `new()` 传入：

```rust
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,
    // ...
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: div().id(id),  // 将 id 应用于基础元素
            // ...
        }
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base  // 已应用 .id()
            .on_click(/* ... */)
    }
}
```

## 调用处用法

```rust
// 具名组件使用唯一字符串 ID
Button::new("save-btn").label("保存")
Button::new("cancel-btn").label("取消")

// 列表使用基于索引的 ID
for (i, item) in items.iter().enumerate() {
    div().id(i)  // 在此父元素内唯一
}

// 使用便于调试的描述性 ID
Input::new("search-input")
Select::new("country-select")
```
