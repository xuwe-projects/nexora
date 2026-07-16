
## 使用时机

在以下场景使用底层 `Element` trait：

- 需要精细控制布局计算
- 构建复杂且性能敏感的组件
- 实现自定义布局算法（瀑布流、环形布局等）
- 高层 `Render` / `RenderOnce` API 无法满足需求

**以下场景优先使用 `Render` / `RenderOnce`：** 简单组件、标准布局、声明式界面

## 快速开始

`Element` trait 可以直接控制三个渲染阶段：

```rust
impl Element for MyElement {
    type RequestLayoutState = MyLayoutState;  // 传给后续阶段的数据
    type PrepaintState = MyPaintState;        // 绘制所需数据

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    // 阶段 1：计算尺寸和位置
    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, Self::RequestLayoutState)
    {
        let layout_id = window.request_layout(
            Style { size: size(px(200.), px(100.)), ..default() },
            vec![],
            cx
        );
        (layout_id, MyLayoutState { /* ... */ })
    }

    // 阶段 2：创建命中区域并准备绘制
    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut Self::RequestLayoutState,
                window: &mut Window, cx: &mut App) -> Self::PrepaintState
    {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        MyPaintState { hitbox }
    }

    // 阶段 3：渲染并处理交互
    fn paint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut Self::RequestLayoutState,
             paint_state: &mut Self::PrepaintState, window: &mut Window, cx: &mut App)
    {
        window.paint_quad(paint_quad(bounds, Anchor::all(px(4.)), cx.theme().background));

        window.on_mouse_event({
            let hitbox = paint_state.hitbox.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    // 处理交互
                    cx.stop_propagation();
                }
            }
        });
    }
}

// 让元素可以作为子元素使用
impl IntoElement for MyElement {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}
```

## 核心概念

### 三阶段渲染

1. **request_layout**：计算尺寸与位置，返回布局 ID 和状态
2. **prepaint**：创建命中区域、计算最终边界并准备绘制
3. **paint**：渲染元素并设置交互（鼠标事件、光标样式）

### 状态流

```
RequestLayoutState → PrepaintState → paint
```

状态通过关联类型单向流动，并以可变引用形式在各阶段之间传递。

### 关键操作

- **布局**：`window.request_layout(style, children, cx)` — 创建布局节点
- **命中区域**：`window.insert_hitbox(bounds, behavior)` — 创建交互区域
- **绘制**：`window.paint_quad(...)` — 渲染视觉内容
- **事件**：`window.on_mouse_event(handler)` — 处理用户输入

## 参考文档

### 完整 API 文档

- **API**：参见 [element-api.md](element-api.md) — 关联类型、命中区域系统、事件处理和光标样式
- **示例**：参见 [element-examples.md](element-examples.md) — 文本元素、交互元素和复杂元素
- **模式**：参见 [element-patterns.md](element-patterns.md) — 文本、容器、交互、复合和可滚动元素
- **最佳实践**：参见 [element-best-practices.md](element-best-practices.md) — 性能、状态和常见陷阱
- **高级主题**：参见 [element-advanced.md](element-advanced.md) — 瀑布流/环形布局、记忆化和虚拟列表
