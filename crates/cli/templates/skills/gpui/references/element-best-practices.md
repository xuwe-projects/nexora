# Element 最佳实践

**目录：** [状态管理](#状态管理) · [性能考虑](#性能考虑) · [交互处理](#交互处理) · [布局策略](#布局策略) · [错误处理](#错误处理) · [测试 Element 实现](#测试-element-实现) · [常见陷阱](#常见陷阱) · [性能检查清单](#性能检查清单)

## 状态管理

### 有效使用关联类型

**正确：** 使用关联类型在各阶段之间传递有意义的数据

```rust
// 正确：具有类型安全的结构化状态
type RequestLayoutState = (StyledText, Vec<ChildLayout>);
type PrepaintState = (Hitbox, Vec<ChildBounds>);
```

**错误：** 需要数据时仍使用空状态

```rust
// 错误：需要传递数据时却没有状态
type RequestLayoutState = ();
type PrepaintState = ();
// 现在无法将布局信息传给绘制阶段！
```

### 管理复杂状态

为具有复杂状态的元素创建专用结构体：

```rust
// 正确：为复杂状态使用专用结构体
pub struct TextElementState {
    pub styled_text: StyledText,
    pub text_layout: TextLayout,
    pub child_states: Vec<ChildState>,
}

type RequestLayoutState = TextElementState;
```

**优点：**

- 状态结构清晰明确
- 易于扩展
- 类型安全访问

### 状态生命周期

**黄金规则：** 状态在各阶段之间单向流动

```
request_layout → RequestLayoutState →
prepaint → PrepaintState →
paint
```

**不要：**

- 将本应放在关联类型中的状态保存在元素结构体中
- 尝试在 `paint` 阶段修改元素状态（使用 `cx.notify()` 安排重新渲染）
- 跨阶段边界传递可变引用

## 性能考虑

### 尽量减少绘制阶段的内存分配

**关键：** 动画期间每一帧都会调用绘制阶段，应尽量减少内存分配。

**正确：** 在 `request_layout` 或 `prepaint` 中预分配

```rust
impl Element for MyElement {
    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, Vec<StyledText>)
    {
        // 在布局期间分配一次
        let styled_texts = self.children
            .iter()
            .map(|child| StyledText::new(child.text.clone()))
            .collect();

        (layout_id, styled_texts)
    }

    fn paint(&mut self, .., styled_texts: &mut Vec<StyledText>, ..) {
        // 直接使用预分配的 styled_texts
        for text in styled_texts {
            text.paint(..);
        }
    }
}
```

**错误：** 在 `paint` 阶段分配

```rust
fn paint(&mut self, ..) {
    // 错误：在绘制阶段分配！
    let styled_texts: Vec<_> = self.children
        .iter()
        .map(|child| StyledText::new(child.text.clone()))
        .collect();
}
```

### 缓存开销较大的计算

对开销较大的操作使用记忆化：

```rust
pub struct CachedElement {
    // 缓存键
    last_text: Option<SharedString>,
    last_width: Option<Pixels>,

    // 缓存结果
    cached_layout: Option<TextLayout>,
}

impl Element for CachedElement {
    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, TextLayout)
    {
        let current_width = window.bounds().width();

        // 检查缓存是否有效
        if self.last_text.as_ref() != Some(&self.text)
            || self.last_width != Some(current_width)
            || self.cached_layout.is_none()
        {
            // 重新计算开销较大的布局
            self.cached_layout = Some(self.compute_text_layout(current_width));
            self.last_text = Some(self.text.clone());
            self.last_width = Some(current_width);
        }

        // 使用缓存的布局
        let layout = self.cached_layout.as_ref().unwrap();
        (layout_id, layout.clone())
    }
}
```

### 延迟渲染子元素

在可滚动容器中只渲染可见子元素：

```rust
fn paint(&mut self, .., bounds: Bounds<Pixels>, paint_state: &mut Self::PrepaintState, ..) {
    for (i, child) in self.children.iter_mut().enumerate() {
        let child_bounds = paint_state.child_bounds[i];

        // 仅绘制可见子元素
        if self.is_visible(&child_bounds, &bounds) {
            child.paint(..);
        }
    }
}

fn is_visible(&self, child_bounds: &Bounds<Pixels>, container_bounds: &Bounds<Pixels>) -> bool {
    child_bounds.bottom() >= container_bounds.top() &&
    child_bounds.top() <= container_bounds.bottom()
}
```

## 交互处理

### 正确处理事件冒泡

处理事件前始终检查事件阶段和边界：

```rust
fn paint(&mut self, .., window: &mut Window, cx: &mut App) {
    window.on_mouse_event({
        let hitbox = self.hitbox.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            // 先检查事件阶段
            if !phase.bubble() {
                return;
            }

            // 检查事件是否位于边界内
            if !hitbox.is_hovered(window) {
                return;
            }

            // 处理事件
            self.handle_click(event);

            // 处理后阻止继续传播
            cx.stop_propagation();
        }
    });
}
```

**不要忘记：**

- 根据需要检查 `phase.bubble()` 或 `phase.capture()`
- 检查命中区域悬停状态或边界
- 处理事件后调用 `cx.stop_propagation()`

### 命中区域管理

在 `prepaint` 阶段而不是 `paint` 阶段创建命中区域：

**正确：**

```rust
fn prepaint(&mut self, .., bounds: Bounds<Pixels>, window: &mut Window, ..) -> Hitbox {
    // 在 prepaint 中创建命中区域
    window.insert_hitbox(bounds, HitboxBehavior::Normal)
}

fn paint(&mut self, .., hitbox: &mut Hitbox, window: &mut Window, ..) {
    // 在 paint 中使用命中区域
    window.set_cursor_style(CursorStyle::PointingHand, hitbox);
}
```

**命中区域行为：**

```rust
// 普通：阻止事件穿透
HitboxBehavior::Normal

// 透明：允许事件穿透到下层元素
HitboxBehavior::Transparent
```

### 光标样式指南

设置合适的光标样式以提示可交互性：

```rust
// 文本选择
window.set_cursor_style(CursorStyle::IBeam, &hitbox);

// 可点击元素（桌面约定：使用默认光标，不使用手形光标）
window.set_cursor_style(CursorStyle::Arrow, &hitbox);

// 链接（Web 约定：使用手形光标）
window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

// 可调整尺寸的边缘
window.set_cursor_style(CursorStyle::ResizeLeftRight, &hitbox);
```

**桌面与 Web 约定：**

- 桌面应用：按钮使用 `Arrow`
- Web 应用：仅链接使用 `PointingHand`

## 布局策略

### 固定尺寸元素

对于尺寸已知且不变的元素：

```rust
fn request_layout(&mut self, .., window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
    let layout_id = window.request_layout(
        Style {
            size: size(px(200.), px(100.)),
            ..default()
        },
        vec![], // 没有子元素
        cx
    );
    (layout_id, ())
}
```

### 基于内容确定尺寸

对于由内容决定尺寸的元素：

```rust
fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
    -> (LayoutId, Size<Pixels>)
{
    // 测量内容
    let text_bounds = self.measure_text(window);
    let padding = px(16.);

    let layout_id = window.request_layout(
        Style {
            size: size(
                text_bounds.width() + padding * 2.,
                text_bounds.height() + padding * 2.,
            ),
            ..default()
        },
        vec![],
        cx
    );

    (layout_id, text_bounds)
}
```

### 弹性布局

对于适应可用空间的元素：

```rust
fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
    -> (LayoutId, Vec<LayoutId>)
{
    let mut child_layout_ids = Vec::new();

    for child in &mut self.children {
        let (layout_id, _) = child.request_layout(window, cx);
        child_layout_ids.push(layout_id);
    }

    let layout_id = window.request_layout(
        Style {
            flex_direction: FlexDirection::Row,
            gap: px(8.),
            size: Size {
                width: relative(1.0),  // 填满父元素宽度
                height: auto(),        // 自动高度
            },
            ..default()
        },
        child_layout_ids.clone(),
        cx
    );

    (layout_id, child_layout_ids)
}
```

## 错误处理

### 优雅降级

优雅处理错误，不要 panic：

```rust
fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
    -> (LayoutId, Option<TextLayout>)
{
    // 尝试创建样式文本
    match StyledText::new(self.text.clone()).request_layout(None, None, window, cx) {
        Ok((layout_id, text_layout)) => {
            (layout_id, Some(text_layout))
        }
        Err(e) => {
            // 记录错误
            eprintln!("文本布局失败：{}", e);

            // 回退到简单文本
            let fallback_text = StyledText::new("（文本加载失败）".into());
            let (layout_id, _) = fallback_text.request_layout(None, None, window, cx);
            (layout_id, None)
        }
    }
}
```

### 防御性边界检查

始终验证边界和索引：

```rust
fn paint_selection(&self, selection: &Selection, text_layout: &TextLayout, ..) {
    // 验证选择区域边界
    let start = selection.start.min(self.text.len());
    let end = selection.end.min(self.text.len());

    if start >= end {
        return; // 无效选择区域
    }

    let rects = text_layout.rects_for_range(start..end);
    // 绘制选择区域……
}
```

## 测试 Element 实现

### 布局测试

测试布局计算是否正确：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn test_element_layout(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut window = cx.open_window(Default::default(), |_, _| ()).unwrap();

            window.update(cx, |window, cx| {
                let mut element = MyElement::new();
                let (layout_id, layout_state) = element.request_layout(
                    None,
                    None,
                    window,
                    cx
                );

                // 断言布局属性
                let bounds = window.layout_bounds(layout_id);
                assert_eq!(bounds.size.width, px(200.));
                assert_eq!(bounds.size.height, px(100.));
            });
        });
    }
}
```

### 交互测试

测试交互是否正常工作：

```rust
#[gpui::test]
fn test_element_click(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut window = cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| MyElement::new())
        }).unwrap();

        window.update(cx, |window, cx| {
            let view = window.root_view().unwrap();

            // 模拟点击
            let position = point(px(10.), px(10.));
            window.dispatch_event(MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });

            // 断言元素已响应
            view.read(cx).assert_clicked();
        });
    });
}
```

## 常见陷阱

### ❌ 将布局状态保存在 Element 结构体中

**错误：**

```rust
pub struct MyElement {
    id: ElementId,
    // 错误：这应该放在 RequestLayoutState 中
    cached_layout: Option<TextLayout>,
}
```

**正确：**

```rust
pub struct MyElement {
    id: ElementId,
    text: SharedString,
}

type RequestLayoutState = TextLayout; // 正确：状态保存在关联类型中
```

### ❌ 在绘制阶段修改 Element

**错误：**

```rust
fn paint(&mut self, ..) {
    self.counter += 1; // 错误：在 paint 中修改元素
}
```

**正确：**

```rust
fn paint(&mut self, .., window: &mut Window, cx: &mut App) {
    window.on_mouse_event(move |event, phase, window, cx| {
        if phase.bubble() {
            self.counter += 1;
            cx.notify(); // 安排重新渲染
        }
    });
}
```

### ❌ 在绘制阶段创建命中区域

**错误：**

```rust
fn paint(&mut self, .., bounds: Bounds<Pixels>, window: &mut Window, ..) {
    // 错误：在 paint 中创建命中区域
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
}
```

**正确：**

```rust
fn prepaint(&mut self, .., bounds: Bounds<Pixels>, window: &mut Window, ..) -> Hitbox {
    // 正确：在 prepaint 中创建命中区域
    window.insert_hitbox(bounds, HitboxBehavior::Normal)
}
```

### ❌ 忽略事件阶段

**错误：**

```rust
window.on_mouse_event(move |event, phase, window, cx| {
    // 错误：没有检查事件阶段
    self.handle_click(event);
});
```

**正确：**

```rust
window.on_mouse_event(move |event, phase, window, cx| {
    // 正确：检查事件阶段
    if !phase.bubble() {
        return;
    }
    self.handle_click(event);
});
```

## 性能检查清单

发布 Element 实现前，确认：

- [ ] `paint` 阶段没有内存分配（事件处理器除外）
- [ ] 开销较大的计算已缓存或记忆化
- [ ] 可滚动容器只渲染可见子元素
- [ ] 命中区域在 `prepaint` 而不是 `paint` 中创建
- [ ] 事件处理器会检查阶段和边界
- [ ] 布局状态通过关联类型传递，而不是保存在元素中
- [ ] Element 实现了带回退方案的适当错误处理
- [ ] 测试覆盖布局计算和交互
