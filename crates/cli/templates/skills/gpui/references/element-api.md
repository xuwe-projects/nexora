# Element API 参考

**目录：** [Element trait 结构](#element-trait-结构) · [关联类型](#关联类型) · [方法](#方法) · [IntoElement 集成](#intoelement-集成) · [布局系统集成](#布局系统集成) · [命中区域系统](#命中区域系统) · [事件处理](#事件处理) · [光标样式](#光标样式)

## Element trait 结构

`Element` trait 要求实现三个关联类型和五个方法：

```rust
pub trait Element: 'static + IntoElement {
    type RequestLayoutState: 'static;
    type PrepaintState: 'static;

    fn id(&self) -> Option<ElementId>;
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>>;
    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState);
    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState;
    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    );
}
```

## 关联类型

### RequestLayoutState

从 `request_layout` 阶段传递给 `prepaint` 和 `paint` 阶段的数据。

**用途：**

- 保存布局计算结果（带样式文本、子布局 ID）
- 缓存开销较大的计算
- 在各阶段之间传递子元素状态

**示例：**
```rust
// 简单场景：不需要状态
type RequestLayoutState = ();

// 单个值
type RequestLayoutState = StyledText;

// 多个值
type RequestLayoutState = (StyledText, Vec<ChildLayout>);

// 复杂结构体
pub struct MyLayoutState {
    pub styled_text: StyledText,
    pub child_layouts: Vec<(LayoutId, ChildState)>,
    pub computed_bounds: Bounds<Pixels>,
}
type RequestLayoutState = MyLayoutState;
```

### PrepaintState

从 `prepaint` 阶段传递给 `paint` 阶段的数据。

**用途：**

- 保存交互所需的命中区域
- 缓存视觉边界
- 保存预绘制结果

**示例：**
```rust
// 简单场景：只有一个命中区域
type PrepaintState = Hitbox;

// 可选命中区域
type PrepaintState = Option<Hitbox>;

// 多个值
type PrepaintState = (Hitbox, Vec<Bounds<Pixels>>);

// 复杂结构体
pub struct MyPaintState {
    pub hitbox: Hitbox,
    pub child_bounds: Vec<Bounds<Pixels>>,
    pub visible_range: Range<usize>,
}
type PrepaintState = MyPaintState;
```

## 方法

### id()

返回用于调试和检查的可选唯一标识符。

```rust
fn id(&self) -> Option<ElementId> {
    Some(self.id.clone())
}

// 如果不需要 ID
fn id(&self) -> Option<ElementId> {
    None
}
```

### source_location()

返回用于调试的源码位置。除非确有调试需要，通常返回 `None`。

```rust
fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
    None
}
```

### request_layout()

计算元素树的尺寸和位置。

**参数：**

- `global_id`：全局元素标识符（可选）
- `inspector_id`：检查器元素标识符（可选）
- `window`：可变窗口引用
- `cx`：可变应用上下文

**返回值：**

- `(LayoutId, Self::RequestLayoutState)`：布局 ID 和供后续阶段使用的状态

**职责：**

1. 调用 `child.request_layout()` 计算子元素布局
2. 使用 `window.request_layout()` 创建自身布局
3. 返回布局 ID 和要传给后续阶段的状态

**示例：**
```rust
fn request_layout(
    &mut self,
    global_id: Option<&GlobalElementId>,
    inspector_id: Option<&InspectorElementId>,
    window: &mut Window,
    cx: &mut App,
) -> (LayoutId, Self::RequestLayoutState) {
    // 1. 计算子元素布局
    let child_layout_id = self.child.request_layout(
        global_id,
        inspector_id,
        window,
        cx
    ).0;

    // 2. 创建自身布局
    let layout_id = window.request_layout(
        Style {
            size: size(px(200.), px(100.)),
            ..default()
        },
        vec![child_layout_id],
        cx
    );

    // 3. 返回布局 ID 和状态
    (layout_id, MyLayoutState { child_layout_id })
}
```

### prepaint()

通过创建命中区域和计算最终边界为绘制做准备。

**参数：**

- `global_id`：全局元素标识符（可选）
- `inspector_id`：检查器元素标识符（可选）
- `bounds`：布局引擎计算出的最终边界
- `request_layout`：布局状态的可变引用
- `window`：可变窗口引用
- `cx`：可变应用上下文

**返回值：**

- `Self::PrepaintState`：供绘制阶段使用的状态

**职责：**

1. 根据布局边界计算子元素的最终边界
2. 为所有子元素调用 `child.prepaint()`
3. 使用 `window.insert_hitbox()` 创建命中区域
4. 返回供绘制阶段使用的状态

**示例：**
```rust
fn prepaint(
    &mut self,
    global_id: Option<&GlobalElementId>,
    inspector_id: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    request_layout: &mut Self::RequestLayoutState,
    window: &mut Window,
    cx: &mut App,
) -> Self::PrepaintState {
    // 1. 计算子元素边界
    let child_bounds = bounds; // 或计算出的子区域

    // 2. 预绘制子元素
    self.child.prepaint(
        global_id,
        inspector_id,
        child_bounds,
        &mut request_layout.child_state,
        window,
        cx
    );

    // 3. 创建命中区域
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

    // 4. 返回绘制状态
    MyPaintState { hitbox }
}
```

### paint()

渲染元素并处理交互。

**参数：**

- `global_id`：全局元素标识符（可选）
- `inspector_id`：检查器元素标识符（可选）
- `bounds`：用于渲染的最终边界
- `request_layout`：布局状态的可变引用
- `prepaint`：预绘制状态的可变引用
- `window`：可变窗口引用
- `cx`：可变应用上下文

**职责：**

1. 先绘制子元素（从底到顶）
2. 绘制自身内容（背景、边框等）
3. 设置交互（鼠标事件、光标样式）

**示例：**
```rust
fn paint(
    &mut self,
    global_id: Option<&GlobalElementId>,
    inspector_id: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    request_layout: &mut Self::RequestLayoutState,
    prepaint: &mut Self::PrepaintState,
    window: &mut Window,
    cx: &mut App,
) {
    // 1. 先绘制子元素
    self.child.paint(
        global_id,
        inspector_id,
        child_bounds,
        &mut request_layout.child_state,
        &mut prepaint.child_paint_state,
        window,
        cx
    );

    // 2. 绘制自身内容
    window.paint_quad(paint_quad(
        bounds,
        Anchor::all(px(4.)),
        cx.theme().background,
    ));

    // 3. 设置交互
    window.on_mouse_event({
        let hitbox = prepaint.hitbox.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if hitbox.is_hovered(window) && phase.bubble() {
                // 处理点击
                cx.stop_propagation();
            }
        }
    });

    window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
}
```

## IntoElement 集成

元素还必须实现 `IntoElement`，才能作为子元素使用：

```rust
impl IntoElement for MyElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
```

这样即可在元素树中直接使用自定义元素：

```rust
div()
    .child(MyElement::new()) // 因为实现了 IntoElement，所以可以使用
```

## 通用参数

### 全局 ID 与检查器 ID

两者都是用于调试和检查的可选标识符：

- `global_id`：整个应用内的唯一标识符
- `inspector_id`：开发工具/检查器使用的标识符

通常不做修改，直接传递给子元素。

### 窗口与上下文

- `window: &mut Window`：窗口专用操作（绘制、命中区域、事件）
- `cx: &mut App`：应用级操作（启动任务、访问全局状态）

## 布局系统集成

### window.request_layout()

使用指定样式和子元素创建布局节点：

```rust
let layout_id = window.request_layout(
    Style {
        size: size(px(200.), px(100.)),
        flex: Flex::Column,
        gap: px(8.),
        ..default()
    },
    vec![child1_layout_id, child2_layout_id],
    cx
);
```

### Bounds<Pixels>

表示矩形区域：

```rust
pub struct Bounds<T> {
    pub origin: Point<T>,
    pub size: Size<T>,
}

// 创建边界
let bounds = Bounds::new(
    point(px(10.), px(20.)),
    size(px(100.), px(50.))
);

// 访问属性
bounds.left()    // origin.x
bounds.top()     // origin.y
bounds.right()   // origin.x + size.width
bounds.bottom()  // origin.y + size.height
bounds.center()  // 中心点
```

## 命中区域系统

### 创建命中区域

```rust
// 普通命中区域（阻止事件穿透）
let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

// 透明命中区域（允许事件穿透）
let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Transparent);
```

### 使用命中区域

```rust
// 检查是否悬停
if hitbox.is_hovered(window) {
    // ...
}

// 设置光标样式
window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

// 在事件处理器中使用
window.on_mouse_event(move |event, phase, window, cx| {
    if hitbox.is_hovered(window) && phase.bubble() {
        // 处理事件
    }
});
```

## 事件处理

### 鼠标事件

```rust
// 按下鼠标
window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
    if phase.bubble() && bounds.contains(&event.position) {
        // 处理按下鼠标
        cx.stop_propagation(); // 阻止冒泡
    }
});

// 松开鼠标
window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
    // 处理松开鼠标
});

// 移动鼠标
window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
    // 处理移动鼠标
});

// Scroll
window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
    // 处理滚动
});
```

### 事件阶段

事件会经过两个阶段：

- **捕获**：自顶向下（父 → 子）
- **冒泡**：自底向上（子 → 父）

```rust
move |event, phase, window, cx| {
    if phase.capture() {
        // 在捕获阶段处理
    } else if phase.bubble() {
        // 在冒泡阶段处理
    }

    cx.stop_propagation(); // 阻止事件继续传播
}
```

## 光标样式

可用的光标样式：

```rust
CursorStyle::Arrow
CursorStyle::IBeam           // 文本选择
CursorStyle::PointingHand    // Clickable
CursorStyle::ResizeLeft
CursorStyle::ResizeRight
CursorStyle::ResizeUp
CursorStyle::ResizeDown
CursorStyle::ResizeLeftRight
CursorStyle::ResizeUpDown
CursorStyle::Crosshair
CursorStyle::OperationNotAllowed
```

用法：

```rust
window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
```
