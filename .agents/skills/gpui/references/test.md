
## 概述

GPUI 提供完整的测试框架，可用于测试界面组件、异步操作和分布式系统。测试在单线程执行器上运行，从而提供确定性执行，并支持测试复杂异步场景。GPUI 测试使用 `#[gpui::test]` 属性；基础测试使用 `TestAppContext`，依赖窗口的测试使用 `VisualTestContext`。

### 规则

- 如果测试不需要窗口或渲染，无需使用 `#[gpui::test]` 和 `TestAppContext`，直接编写普通 Rust 测试。

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

`VisualTestContext` 在 `TestAppContext` 基础上增加窗口支持：

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

## 其他资源

- 详细模式（包括无重入崩溃测试）参见 [test-reference.md](test-reference.md)
- 示例与最佳实践参见 [test-examples.md](test-examples.md)
