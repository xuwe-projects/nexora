# 常用 Element 模式

**目录：** [文本渲染元素](#文本渲染元素) · [容器元素](#容器元素) · [交互元素](#交互元素) · [复合元素](#复合元素) · [可滚动元素](#可滚动元素) · [模式选择指南](#模式选择指南)

## 文本渲染元素

用于显示和操作文本内容的元素。

### 模式特征

- 使用 `StyledText` 完成文本布局与渲染
- 在 `paint` 阶段结合命中区域交互处理文本选择
- 在 `prepaint` 阶段为文本交互创建命中区域
- 通过 run 支持文本高亮和自定义样式

### 实现模板

```rust
pub struct TextElement {
    id: ElementId,
    text: SharedString,
    style: TextStyle,
}

impl Element for TextElement {
    type RequestLayoutState = StyledText;
    type PrepaintState = Hitbox;

    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, StyledText)
    {
        let styled_text = StyledText::new(self.text.clone())
            .with_style(self.style);
        let (layout_id, _) = styled_text.request_layout(None, None, window, cx);
        (layout_id, styled_text)
    }

    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, styled_text: &mut StyledText,
                window: &mut Window, cx: &mut App) -> Hitbox
    {
        styled_text.prepaint(None, None, bounds, &mut (), window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(&mut self, .., bounds: Bounds<Pixels>, styled_text: &mut StyledText,
             hitbox: &mut Hitbox, window: &mut Window, cx: &mut App)
    {
        styled_text.paint(None, None, bounds, &mut (), &mut (), window, cx);
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
    }
}
```

### 用例

- 带语法高亮的代码编辑器
- 富文本显示
- 自定义格式标签
- 可选择文本区域

## 容器元素

用于管理和布局子元素的元素。

### 模式特征

- 管理子元素的布局与位置
- 按需处理滚动和裁剪
- 实现类似 Flex/Grid 的布局
- 协调子元素交互与事件委托

### 实现模板

```rust
pub struct ContainerElement {
    id: ElementId,
    children: Vec<AnyElement>,
    direction: FlexDirection,
    gap: Pixels,
}

impl Element for ContainerElement {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Vec<Bounds<Pixels>>;

    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, Vec<LayoutId>)
    {
        let child_layout_ids: Vec<_> = self.children
            .iter_mut()
            .map(|child| child.request_layout(window, cx).0)
            .collect();

        let layout_id = window.request_layout(
            Style {
                flex_direction: self.direction,
                gap: self.gap,
                ..default()
            },
            child_layout_ids.clone(),
            cx
        );

        (layout_id, child_layout_ids)
    }

    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, layout_ids: &mut Vec<LayoutId>,
                window: &mut Window, cx: &mut App) -> Vec<Bounds<Pixels>>
    {
        let mut child_bounds = Vec::new();

        for (child, layout_id) in self.children.iter_mut().zip(layout_ids.iter()) {
            let child_bound = window.layout_bounds(*layout_id);
            child.prepaint(child_bound, window, cx);
            child_bounds.push(child_bound);
        }

        child_bounds
    }

    fn paint(&mut self, .., child_bounds: &mut Vec<Bounds<Pixels>>,
             window: &mut Window, cx: &mut App)
    {
        for (child, bounds) in self.children.iter_mut().zip(child_bounds.iter()) {
            child.paint(*bounds, window, cx);
        }
    }
}
```

### 用例

- 面板与分割视图
- 列表容器
- 网格布局
- 标签页容器

## 交互元素

响应用户输入（鼠标、键盘、触摸）的元素。

### 模式特征

- 为交互区域创建适当的命中区域
- 正确处理鼠标、键盘和触摸事件
- 管理焦点与光标样式
- 支持悬停、激活和禁用状态

### 实现模板

```rust
pub struct InteractiveElement {
    id: ElementId,
    content: AnyElement,
    on_click: Option<Box<dyn Fn(&MouseUpEvent, &mut Window, &mut App)>>,
    hover_style: Option<Style>,
}

impl Element for InteractiveElement {
    type RequestLayoutState = LayoutId;
    type PrepaintState = (Hitbox, bool); // 命中区域和 is_hovered

    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, LayoutId)
    {
        let (content_layout, _) = self.content.request_layout(window, cx);
        (content_layout, content_layout)
    }

    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, content_layout: &mut LayoutId,
                window: &mut Window, cx: &mut App) -> (Hitbox, bool)
    {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let is_hovered = hitbox.is_hovered(window);

        self.content.prepaint(bounds, window, cx);

        (hitbox, is_hovered)
    }

    fn paint(&mut self, .., bounds: Bounds<Pixels>, content_layout: &mut LayoutId,
             prepaint: &mut (Hitbox, bool), window: &mut Window, cx: &mut App)
    {
        let (hitbox, is_hovered) = prepaint;

        // 悬停时绘制悬停背景
        if *is_hovered {
            if let Some(hover_style) = &self.hover_style {
                window.paint_quad(paint_quad(
                    bounds,
                    Anchor::all(px(4.)),
                    hover_style.background_color.unwrap_or(cx.theme().hover),
                ));
            }
        }

        // 绘制内容
        self.content.paint(bounds, window, cx);

        // 处理点击
        if let Some(on_click) = self.on_click.as_ref() {
            window.on_mouse_event({
                let on_click = on_click.clone();
                let hitbox = hitbox.clone();
                move |event: &MouseUpEvent, phase, window, cx| {
                    if hitbox.is_hovered(window) && phase.bubble() {
                        on_click(event, window, cx);
                        cx.stop_propagation();
                    }
                }
            });
        }

        // 设置光标样式
        window.set_cursor_style(CursorStyle::PointingHand, hitbox);
    }
}
```

### 用例

- 按钮
- 链接
- 可点击卡片
- 拖动手柄
- 菜单项

## 复合元素

组合多个子元素并进行复杂协调的元素。

### 模式特征

- 组合多个不同类型的子元素
- 管理跨子元素的复杂状态
- 协调动画与过渡
- 处理子元素之间的焦点委托

### 实现模板

```rust
pub struct CompositeElement {
    id: ElementId,
    header: AnyElement,
    content: AnyElement,
    footer: Option<AnyElement>,
}

struct CompositeLayoutState {
    header_layout: LayoutId,
    content_layout: LayoutId,
    footer_layout: Option<LayoutId>,
}

struct CompositePaintState {
    header_bounds: Bounds<Pixels>,
    content_bounds: Bounds<Pixels>,
    footer_bounds: Option<Bounds<Pixels>>,
}

impl Element for CompositeElement {
    type RequestLayoutState = CompositeLayoutState;
    type PrepaintState = CompositePaintState;

    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, CompositeLayoutState)
    {
        let (header_layout, _) = self.header.request_layout(window, cx);
        let (content_layout, _) = self.content.request_layout(window, cx);
        let footer_layout = self.footer.as_mut()
            .map(|f| f.request_layout(window, cx).0);

        let mut children = vec![header_layout, content_layout];
        if let Some(footer) = footer_layout {
            children.push(footer);
        }

        let layout_id = window.request_layout(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: relative(1.0),
                    height: auto(),
                },
                ..default()
            },
            children,
            cx
        );

        (layout_id, CompositeLayoutState {
            header_layout,
            content_layout,
            footer_layout,
        })
    }

    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut CompositeLayoutState,
                window: &mut Window, cx: &mut App) -> CompositePaintState
    {
        let header_bounds = window.layout_bounds(layout.header_layout);
        let content_bounds = window.layout_bounds(layout.content_layout);
        let footer_bounds = layout.footer_layout
            .map(|id| window.layout_bounds(id));

        self.header.prepaint(header_bounds, window, cx);
        self.content.prepaint(content_bounds, window, cx);
        if let (Some(footer), Some(bounds)) = (&mut self.footer, footer_bounds) {
            footer.prepaint(bounds, window, cx);
        }

        CompositePaintState {
            header_bounds,
            content_bounds,
            footer_bounds,
        }
    }

    fn paint(&mut self, .., paint_state: &mut CompositePaintState,
             window: &mut Window, cx: &mut App)
    {
        self.header.paint(paint_state.header_bounds, window, cx);
        self.content.paint(paint_state.content_bounds, window, cx);
        if let (Some(footer), Some(bounds)) = (&mut self.footer, paint_state.footer_bounds) {
            footer.paint(bounds, window, cx);
        }
    }
}
```

### 用例

- 对话框（标题 + 内容 + 页脚）
- 多分区卡片
- 表单布局
- 带工具栏的面板

## 可滚动元素

包含可滚动内容区域的元素。

### 模式特征

- 管理滚动状态（偏移量、速度）
- 处理滚动事件（滚轮、拖动、触摸）
- 绘制滚动条（轨道和滑块）
- 将内容裁剪到可见区域

### 实现模板

```rust
pub struct ScrollableElement {
    id: ElementId,
    content: AnyElement,
    scroll_offset: Point<Pixels>,
    content_size: Size<Pixels>,
}

struct ScrollPaintState {
    hitbox: Hitbox,
    visible_bounds: Bounds<Pixels>,
}

impl Element for ScrollableElement {
    type RequestLayoutState = (LayoutId, Size<Pixels>);
    type PrepaintState = ScrollPaintState;

    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, (LayoutId, Size<Pixels>))
    {
        let (content_layout, _) = self.content.request_layout(window, cx);
        let content_size = window.layout_bounds(content_layout).size;

        let layout_id = window.request_layout(
            Style {
                size: Size {
                    width: relative(1.0),
                    height: px(400.), // 固定视口高度
                },
                overflow: Overflow::Hidden,
                ..default()
            },
            vec![content_layout],
            cx
        );

        (layout_id, (content_layout, content_size))
    }

    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut (LayoutId, Size<Pixels>),
                window: &mut Window, cx: &mut App) -> ScrollPaintState
    {
        let (content_layout, content_size) = layout;

        // 使用滚动偏移量计算内容边界
        let content_bounds = Bounds::new(
            point(bounds.left(), bounds.top() - self.scroll_offset.y),
            *content_size
        );

        self.content.prepaint(content_bounds, window, cx);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        ScrollPaintState {
            hitbox,
            visible_bounds: bounds,
        }
    }

    fn paint(&mut self, .., layout: &mut (LayoutId, Size<Pixels>),
             paint_state: &mut ScrollPaintState, window: &mut Window, cx: &mut App)
    {
        let (_, content_size) = layout;

        // 绘制内容
        self.content.paint(paint_state.visible_bounds, window, cx);

        // 绘制滚动条
        self.paint_scrollbar(paint_state.visible_bounds, *content_size, window, cx);

        // 处理滚动事件
        window.on_mouse_event({
            let hitbox = paint_state.hitbox.clone();
            let content_height = content_size.height;
            let visible_height = paint_state.visible_bounds.size.height;

            move |event: &ScrollWheelEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    // 更新滚动偏移量
                    self.scroll_offset.y -= event.delta.y;

                    // 限制在有效范围内
                    let max_scroll = (content_height - visible_height).max(px(0.));
                    self.scroll_offset.y = self.scroll_offset.y
                        .max(px(0.))
                        .min(max_scroll);

                    cx.notify();
                    cx.stop_propagation();
                }
            }
        });
    }
}

impl ScrollableElement {
    fn paint_scrollbar(
        &self,
        bounds: Bounds<Pixels>,
        content_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut App
    ) {
        let visible_height = bounds.size.height;
        let content_height = content_size.height;

        if content_height <= visible_height {
            return; // 不需要滚动条
        }

        let scrollbar_width = px(8.);

        // 计算滑块位置和尺寸
        let scroll_ratio = self.scroll_offset.y / (content_height - visible_height);
        let thumb_height = (visible_height / content_height) * visible_height;
        let thumb_y = scroll_ratio * (visible_height - thumb_height);

        // 绘制轨道
        window.paint_quad(paint_quad(
            Bounds::new(
                point(bounds.right() - scrollbar_width, bounds.top()),
                size(scrollbar_width, visible_height)
            ),
            Anchor::default(),
            cx.theme().scrollbar_track,
        ));

        // 绘制滑块
        window.paint_quad(paint_quad(
            Bounds::new(
                point(bounds.right() - scrollbar_width, bounds.top() + thumb_y),
                size(scrollbar_width, thumb_height)
            ),
            Anchor::all(px(4.)),
            cx.theme().scrollbar_thumb,
        ));
    }
}
```

### 用例

- 可滚动列表
- 打开大型文件的代码编辑器
- 长篇文本内容
- 图库

## 模式选择指南

| 需求 | 模式 | 复杂度 |
|------|------|--------|
| 显示带样式文本 | 文本渲染 | 低 |
| 布局多个子元素 | 容器 | 低到中 |
| 处理点击/悬停 | 交互 | 中 |
| 复杂多部分界面 | 复合 | 中到高 |
| 可滚动的大型内容 | 可滚动 | 高 |

选择能够满足需求的最简单模式，再按需扩展。
