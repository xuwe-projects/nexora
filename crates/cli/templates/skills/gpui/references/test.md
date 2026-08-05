
## 概述

GPUI 提供完整的测试框架，可用于测试界面组件和异步操作。测试在确定性执行器上运行，并支持复现复杂调度场景。`#[gpui::test]` 注入 `TestAppContext`；需要模拟窗口交互时，从测试窗口创建 `VisualTestContext`。需要真实平台渲染和像素输出时，才单独创建 `VisualTestAppContext`。

### 规则

- 如果测试不需要窗口或渲染，无需使用 `#[gpui::test]` 和 `TestAppContext`，直接编写普通 Rust 测试。
- `VisualTestContext` 与 `VisualTestAppContext` 不是新旧名称，也不能互换：前者属于确定性测试环境中的单窗口上下文，后者拥有真实平台级应用上下文。

## 核心测试基础设施

### 测试属性

#### 基础测试

```rust
#[gpui::test]
fn my_test(cx: &mut TestAppContext) {
    // 测试实现
}
```

#### 异步测试

```rust
#[gpui::test]
async fn my_async_test(cx: &mut TestAppContext) {
    // 异步测试实现
}
```

#### 带迭代次数的属性测试

```rust
#[gpui::test(iterations = 10)]
fn my_property_test(cx: &mut TestAppContext, mut rng: StdRng) {
    // 使用随机数据进行属性测试
}
```

### 测试上下文

#### TestAppContext

`TestAppContext` 无需窗口即可访问 GPUI 核心功能：

```rust
#[gpui::test]
fn test_entity_operations(cx: &mut TestAppContext) {
    // 创建 Entity
    let entity = cx.new(|cx| MyComponent::new(cx));

    // 更新 Entity
    entity.update(cx, |component, cx| {
        component.value = 42;
        cx.notify();
    });

    // 读取 Entity
    let value = entity.read_with(cx, |component, _| component.value);
    assert_eq!(value, 42);
}
```

#### VisualTestContext

`VisualTestContext` 绑定由 `TestAppContext` 创建的模拟窗口，用于事件分发、焦点、布局和绘制流程测试。它仍属于 `#[gpui::test]` 的确定性测试环境：

```rust
#[gpui::test]
fn test_with_window(cx: &mut TestAppContext) {
    // 创建包含组件的窗口
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| MyComponent::new(cx))
        }).unwrap()
    });

    // 转换为可视上下文
    let mut cx = VisualTestContext::from_window(window.into(), cx);

    // 访问窗口和组件
    let component = window.root(&mut cx).unwrap();
}
```

#### VisualTestAppContext

`VisualTestAppContext` 使用真实平台实现来产生实际渲染输出，适合截图、像素和真实平台集成测试。它不是 `#[gpui::test]` 注入参数，也不是从测试窗口转换出的 `VisualTestContext`。

- 仅在确实验证真实像素或平台行为时使用；普通组件行为仍使用 `TestAppContext`/`VisualTestContext`。
- 按当前 GPUI 平台要求在主线程创建；当前真实视觉测试能力主要面向 macOS。
- 由专用视觉测试 runner 负责平台、资源、窗口和截图生命周期，不把它混入普通单元测试。
- 不要把 `VisualTestContext::from_window(...)` 示例改写成 `VisualTestAppContext`。

## 其他资源

- 详细模式（包括无重入崩溃测试）参见 [test-reference.md](test-reference.md)
- 示例与最佳实践参见 [test-examples.md](test-examples.md)
