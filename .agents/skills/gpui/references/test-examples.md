## 测试最佳实践

### 测试组织

将相关测试组织到模块中：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod entity_tests {
        use super::*;

        #[gpui::test]
        fn test_creation() { /* ... */ }

        #[gpui::test]
        fn test_updates() { /* ... */ }
    }

    mod async_tests {
        use super::*;

        #[gpui::test]
        async fn test_async_ops() { /* ... */ }
    }

    mod distributed_tests {
        use super::*;

        #[gpui::test]
        fn test_multi_app() { /* ... */ }
    }
}
```

### 初始化与清理

使用辅助函数完成通用初始化：

```rust
fn create_test_counter(cx: &mut TestAppContext) -> Entity<Counter> {
    cx.new(|cx| Counter::new(cx))
}

#[gpui::test]
fn test_counter_operations(cx: &mut TestAppContext) {
    let counter = create_test_counter(cx);

    // 测试操作
}
```

### 断言

使用描述清晰的断言：

```rust
#[gpui::test]
fn test_counter_bounds(cx: &mut TestAppContext) {
    let counter = create_test_counter(cx);

    // 测试上界
    for _ in 0..100 {
        counter.update(cx, |counter, cx| {
            counter.increment(cx);
        });
    }

    let count = counter.read_with(cx, |counter, _| counter.count);
    assert!(count <= 100, "计数器不应超过最大值");

    // 测试下界
    for _ in 0..200 {
        counter.update(cx, |counter, cx| {
            counter.decrement(cx);
        });
    }

    let count = counter.read_with(cx, |counter, _| counter.count);
    assert!(count >= 0, "计数器不应低于最小值");
}
```

### 性能测试

测试性能特征：

```rust
#[gpui::test]
fn test_operation_performance(cx: &mut TestAppContext) {
    let component = cx.new(|cx| MyComponent::new(cx));

    let start = std::time::Instant::now();

    // 执行大量操作
    for i in 0..1000 {
        component.update(cx, |comp, cx| {
            comp.perform_operation(i, cx);
        });
    }

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(100), "操作应快速完成");
}
```

## 运行测试

### 基础测试执行

```bash
# 运行全部测试
cargo test

# 运行指定测试
cargo test test_counter_operations

# 运行指定模块中的测试
cargo test entity_tests::

# 运行并显示输出
cargo test -- --nocapture
```

### 测试配置

为 GPUI 测试启用 `test-support` feature：

```toml
[features]
test-support = ["gpui/test-support"]
```

```bash
cargo test --features test-support
```

### 高级测试执行

```bash
# 使用多次迭代运行属性测试
cargo test -- --test-threads=1

# 运行名称匹配模式的测试
cargo test test_async

# 运行测试并在失败时显示回溯
RUST_BACKTRACE=1 cargo test
```

### CI/CD 集成

在持续集成中：

```yaml
# .github/workflows/test.yml
name: Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - name: Run tests
        run: cargo test --features test-support
```

GPUI 测试框架提供确定、快速且完整的测试能力，既能模拟真实应用行为，又能为复杂界面与异步场景的深入测试提供必要控制。
