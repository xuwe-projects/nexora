# 高级 Element 模式

**目录：** [自定义布局算法](#自定义布局算法) · [使用 trait 组合 Element](#使用-trait-组合-element) · [异步更新 Element](#异步更新-element) · [Element 记忆化](#element-记忆化) · [虚拟列表模式](#虚拟列表模式)

## 自定义布局算法

实现 GPUI 内置布局不支持的自定义布局算法。

### 瀑布流布局（Pinterest 风格）

```rust
pub struct MasonryLayout {
    id: ElementId,
    columns: usize,
    gap: Pixels,
    children: Vec<AnyElement>,
}

struct MasonryLayoutState {
    column_layouts: Vec<Vec<LayoutId>>,
    column_heights: Vec<Pixels>,
}

struct MasonryPaintState {
    child_bounds: Vec<Bounds<Pixels>>,
}

impl Element for MasonryLayout {
    type RequestLayoutState = MasonryLayoutState;
    type PrepaintState = MasonryPaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App
    ) -> (LayoutId, MasonryLayoutState) {
        // 初始化列
        let mut columns: Vec<Vec<LayoutId>> = vec![Vec::new(); self.columns];
        let mut column_heights = vec![px(0.); self.columns];

        // 将子元素分配到各列
        for child in &mut self.children {
            let (child_layout_id, _) = child.request_layout(
                global_id,
                inspector_id,
                window,
                cx
            );

            let child_size = window.layout_bounds(child_layout_id).size;

            // 查找最短列
            let min_column_idx = column_heights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap()
                .0;

            // 将子元素添加到最短列
            columns[min_column_idx].push(child_layout_id);
            column_heights[min_column_idx] += child_size.height + self.gap;
        }

        // 计算总布局尺寸
        let column_width = px(200.); // 固定列宽
        let total_width = column_width * self.columns as f32
            + self.gap * (self.columns - 1) as f32;
        let total_height = column_heights.iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .copied()
            .unwrap_or(px(0.));

        let layout_id = window.request_layout(
            Style {
                size: size(total_width, total_height),
                ..default()
            },
            columns.iter().flatten().copied().collect(),
            cx
        );

        (layout_id, MasonryLayoutState {
            column_layouts: columns,
            column_heights,
        })
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut MasonryLayoutState,
        window: &mut Window,
        cx: &mut App
    ) -> MasonryPaintState {
        let column_width = px(200.);
        let mut child_bounds = Vec::new();

        // 在各列中定位子元素
        for (col_idx, column) in layout_state.column_layouts.iter().enumerate() {
            let x_offset = bounds.left()
                + (column_width + self.gap) * col_idx as f32;
            let mut y_offset = bounds.top();

            for (child_idx, layout_id) in column.iter().enumerate() {
                let child_size = window.layout_bounds(*layout_id).size;
                let child_bound = Bounds::new(
                    point(x_offset, y_offset),
                    size(column_width, child_size.height)
                );

                self.children[child_idx].prepaint(
                    global_id,
                    inspector_id,
                    child_bound,
                    window,
                    cx
                );

                child_bounds.push(child_bound);
                y_offset += child_size.height + self.gap;
            }
        }

        MasonryPaintState { child_bounds }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _layout_state: &mut MasonryLayoutState,
        paint_state: &mut MasonryPaintState,
        window: &mut Window,
        cx: &mut App
    ) {
        for (child, bounds) in self.children.iter_mut().zip(&paint_state.child_bounds) {
            child.paint(global_id, inspector_id, *bounds, window, cx);
        }
    }
}
```

### 环形布局

```rust
pub struct CircularLayout {
    id: ElementId,
    radius: Pixels,
    children: Vec<AnyElement>,
}

impl Element for CircularLayout {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Vec<Bounds<Pixels>>;

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App
    ) -> (LayoutId, Vec<LayoutId>) {
        let child_layouts: Vec<_> = self.children
            .iter_mut()
            .map(|child| child.request_layout(global_id, inspector_id, window, cx).0)
            .collect();

        let diameter = self.radius * 2.;
        let layout_id = window.request_layout(
            Style {
                size: size(diameter, diameter),
                ..default()
            },
            child_layouts.clone(),
            cx
        );

        (layout_id, child_layouts)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_ids: &mut Vec<LayoutId>,
        window: &mut Window,
        cx: &mut App
    ) -> Vec<Bounds<Pixels>> {
        let center = bounds.center();
        let angle_step = 2.0 * std::f32::consts::PI / self.children.len() as f32;

        let mut child_bounds = Vec::new();

        for (i, (child, layout_id)) in self.children.iter_mut()
            .zip(layout_ids.iter())
            .enumerate()
        {
            let angle = angle_step * i as f32;
            let child_size = window.layout_bounds(*layout_id).size;

            // 将子元素定位到圆周上
            let x = center.x + self.radius * angle.cos() - child_size.width / 2.;
            let y = center.y + self.radius * angle.sin() - child_size.height / 2.;

            let child_bound = Bounds::new(point(x, y), child_size);

            child.prepaint(global_id, inspector_id, child_bound, window, cx);
            child_bounds.push(child_bound);
        }

        child_bounds
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _layout_ids: &mut Vec<LayoutId>,
        child_bounds: &mut Vec<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut App
    ) {
        for (child, bounds) in self.children.iter_mut().zip(child_bounds) {
            child.paint(global_id, inspector_id, *bounds, window, cx);
        }
    }
}
```

## 使用 trait 组合 Element

通过 trait 创建可复用行为，用于组合元素。

### Hoverable trait

```rust
pub trait Hoverable: Element {
    fn on_hover<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&mut Window, &mut App) + 'static;

    fn on_hover_end<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&mut Window, &mut App) + 'static;
}

// 自定义元素的实现
pub struct HoverableElement {
    id: ElementId,
    content: AnyElement,
    hover_handlers: Vec<Box<dyn Fn(&mut Window, &mut App)>>,
    hover_end_handlers: Vec<Box<dyn Fn(&mut Window, &mut App)>>,
    was_hovered: bool,
}

impl Hoverable for HoverableElement {
    fn on_hover<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&mut Window, &mut App) + 'static
    {
        self.hover_handlers.push(Box::new(f));
        self
    }

    fn on_hover_end<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&mut Window, &mut App) + 'static
    {
        self.hover_end_handlers.push(Box::new(f));
        self
    }
}

impl Element for HoverableElement {
    type RequestLayoutState = LayoutId;
    type PrepaintState = Hitbox;

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut LayoutId,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App
    ) {
        let is_hovered = hitbox.is_hovered(window);

        // 触发悬停事件
        if is_hovered && !self.was_hovered {
            for handler in &self.hover_handlers {
                handler(window, cx);
            }
        } else if !is_hovered && self.was_hovered {
            for handler in &self.hover_end_handlers {
                handler(window, cx);
            }
        }

        self.was_hovered = is_hovered;

        // 绘制内容
        self.content.paint(bounds, window, cx);
    }

    // ……其他方法……
}
```

### Clickable trait

```rust
pub trait Clickable: Element {
    fn on_click<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&MouseUpEvent, &mut Window, &mut App) + 'static;

    fn on_double_click<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&MouseUpEvent, &mut Window, &mut App) + 'static;
}

pub struct ClickableElement {
    id: ElementId,
    content: AnyElement,
    click_handlers: Vec<Box<dyn Fn(&MouseUpEvent, &mut Window, &mut App)>>,
    double_click_handlers: Vec<Box<dyn Fn(&MouseUpEvent, &mut Window, &mut App)>>,
    last_click_time: Option<Instant>,
}

impl Clickable for ClickableElement {
    fn on_click<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&MouseUpEvent, &mut Window, &mut App) + 'static
    {
        self.click_handlers.push(Box::new(f));
        self
    }

    fn on_double_click<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&MouseUpEvent, &mut Window, &mut App) + 'static
    {
        self.double_click_handlers.push(Box::new(f));
        self
    }
}
```

## 异步更新 Element

根据异步操作结果更新的元素。

```rust
pub struct AsyncElement {
    id: ElementId,
    state: Entity<AsyncState>,
    loading: bool,
    data: Option<String>,
}

pub struct AsyncState {
    loading: bool,
    data: Option<String>,
}

impl Element for AsyncElement {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App
    ) {
        // 显示加载状态或数据
        if self.loading {
            // 绘制加载指示器
            self.paint_loading(bounds, window, cx);
        } else if let Some(data) = &self.data {
            // 绘制数据
            self.paint_data(data, bounds, window, cx);
        }

        // 点击时触发异步更新
        window.on_mouse_event({
            let state = self.state.clone();
            let hitbox = hitbox.clone();

            move |event: &MouseUpEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    // 启动异步任务
                    cx.spawn({
                        let state = state.clone();
                        async move {
                            // 执行异步操作
                            let result = fetch_data_async().await;

                            // 完成后更新状态
                            state.update(cx, |state, cx| {
                                state.loading = false;
                                state.data = Some(result);
                                cx.notify();
                            });
                        }
                    }).detach();

                    // 立即设置加载状态
                    state.update(cx, |state, cx| {
                        state.loading = true;
                        cx.notify();
                    });

                    cx.stop_propagation();
                }
            }
        });
    }

    // ……其他方法……
}

async fn fetch_data_async() -> String {
    // 模拟异步操作
    tokio::time::sleep(Duration::from_secs(1)).await;
    "数据加载完成！".to_string()
}
```

## Element 记忆化

通过记忆化开销较大的元素计算来优化性能。

```rust
pub struct MemoizedElement<T: PartialEq + Clone + 'static> {
    id: ElementId,
    value: T,
    render_fn: Box<dyn Fn(&T) -> AnyElement>,
    cached_element: Option<AnyElement>,
    last_value: Option<T>,
}

impl<T: PartialEq + Clone + 'static> MemoizedElement<T> {
    pub fn new<F>(id: ElementId, value: T, render_fn: F) -> Self
    where
        F: Fn(&T) -> AnyElement + 'static,
    {
        Self {
            id,
            value,
            render_fn: Box::new(render_fn),
            cached_element: None,
            last_value: None,
        }
    }
}

impl<T: PartialEq + Clone + 'static> Element for MemoizedElement<T> {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App
    ) -> (LayoutId, LayoutId) {
        // 检查值是否变化
        if self.last_value.as_ref() != Some(&self.value) || self.cached_element.is_none() {
            // 重新计算元素
            self.cached_element = Some((self.render_fn)(&self.value));
            self.last_value = Some(self.value.clone());
        }

        // 请求缓存元素的布局
        let (layout_id, _) = self.cached_element
            .as_mut()
            .unwrap()
            .request_layout(global_id, inspector_id, window, cx);

        (layout_id, layout_id)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout_id: &mut LayoutId,
        window: &mut Window,
        cx: &mut App
    ) -> () {
        self.cached_element
            .as_mut()
            .unwrap()
            .prepaint(global_id, inspector_id, bounds, window, cx);
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout_id: &mut LayoutId,
        _: &mut (),
        window: &mut Window,
        cx: &mut App
    ) {
        self.cached_element
            .as_mut()
            .unwrap()
            .paint(global_id, inspector_id, bounds, window, cx);
    }
}

// Usage
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    MemoizedElement::new(
        ElementId::Name("memoized".into()),
        self.expensive_value.clone(),
        |value| {
            // 仅在值变化时调用开销较大的渲染函数
            div().child(format!("Computed: {}", value))
        }
    )
}
```

## 虚拟列表模式

仅渲染可见项目，从而高效渲染大型列表。

```rust
pub struct VirtualList {
    id: ElementId,
    item_count: usize,
    item_height: Pixels,
    viewport_height: Pixels,
    scroll_offset: Pixels,
    render_item: Box<dyn Fn(usize) -> AnyElement>,
}

struct VirtualListState {
    visible_range: Range<usize>,
    visible_item_layouts: Vec<LayoutId>,
}

impl Element for VirtualList {
    type RequestLayoutState = VirtualListState;
    type PrepaintState = Hitbox;

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App
    ) -> (LayoutId, VirtualListState) {
        // 计算可见范围
        let start_idx = (self.scroll_offset / self.item_height).floor() as usize;
        let end_idx = ((self.scroll_offset + self.viewport_height) / self.item_height)
            .ceil() as usize;
        let visible_range = start_idx..end_idx.min(self.item_count);

        // 仅为可见项目请求布局
        let visible_item_layouts: Vec<_> = visible_range.clone()
            .map(|i| {
                let mut item = (self.render_item)(i);
                item.request_layout(global_id, inspector_id, window, cx).0
            })
            .collect();

        let total_height = self.item_height * self.item_count as f32;
        let layout_id = window.request_layout(
            Style {
                size: size(relative(1.0), self.viewport_height),
                overflow: Overflow::Hidden,
                ..default()
            },
            visible_item_layouts.clone(),
            cx
        );

        (layout_id, VirtualListState {
            visible_range,
            visible_item_layouts,
        })
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut VirtualListState,
        window: &mut Window,
        _cx: &mut App
    ) -> Hitbox {
        // 在正确位置预绘制可见项目
        for (i, layout_id) in state.visible_item_layouts.iter().enumerate() {
            let item_idx = state.visible_range.start + i;
            let y = item_idx as f32 * self.item_height - self.scroll_offset;
            let item_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + y),
                size(bounds.width(), self.item_height)
            );

            // 可见时预绘制
            if item_bounds.intersects(&bounds) {
                // 预绘制项目……
            }
        }

        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut VirtualListState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App
    ) {
        // 绘制可见项目
        for (i, _layout_id) in state.visible_item_layouts.iter().enumerate() {
            let item_idx = state.visible_range.start + i;
            let y = item_idx as f32 * self.item_height - self.scroll_offset;
            let item_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + y),
                size(bounds.width(), self.item_height)
            );

            if item_bounds.intersects(&bounds) {
                let mut item = (self.render_item)(item_idx);
                item.paint(item_bounds, window, cx);
            }
        }

        // 处理滚动
        window.on_mouse_event({
            let hitbox = hitbox.clone();
            let total_height = self.item_height * self.item_count as f32;

            move |event: &ScrollWheelEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    self.scroll_offset -= event.delta.y;
                    self.scroll_offset = self.scroll_offset
                        .max(px(0.))
                        .min(total_height - self.viewport_height);
                    cx.notify();
                    cx.stop_propagation();
                }
            }
        });
    }
}

// 用法：高效渲染 10,000 个项目
let virtual_list = VirtualList {
    id: ElementId::Name("large-list".into()),
    item_count: 10_000,
    item_height: px(40.),
    viewport_height: px(400.),
    scroll_offset: px(0.),
    render_item: Box::new(|index| {
        div().child(format!("Item {}", index))
    }),
};
```

这些高级模式能够在保持性能和代码质量的同时实现复杂元素。
