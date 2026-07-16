# 测试参考

**目录：** [测试模式](#测试模式) · [测试无崩溃状态管理（重入）](#测试无崩溃状态管理重入) · [属性测试](#属性测试) · [分布式系统测试](#分布式系统测试) · [模拟与隔离](#模拟与隔离)

## 测试模式

### 基础 Entity 测试

测试 Entity 的创建、更新和读取：

```rust
#[gpui::test]
fn test_counter_entity(cx: &mut TestAppContext) {
    let counter = cx.new(|cx| Counter::new(cx));

    // 测试初始状态
    let initial_count = counter.read_with(cx, |counter, _| counter.count);
    assert_eq!(initial_count, 0);

    // 测试更新
    counter.update(cx, |counter, cx| {
        counter.count = 42;
        cx.notify();
    });

    let updated_count = counter.read_with(cx, |counter, _| counter.count);
    assert_eq!(updated_count, 42);
}
```

### 事件测试

测试事件发出与处理：

```rust
#[derive(Clone)]
struct ValueChanged {
    new_value: i32,
}

impl EventEmitter<ValueChanged> for MyComponent {}

#[gpui::test]
fn test_event_emission(cx: &mut TestAppContext) {
    let component = cx.new(|cx| {
        let mut comp = MyComponent::default();

        // 订阅自身事件
        cx.subscribe_self(|this, event: &ValueChanged, cx| {
            this.received_value = event.new_value;
            cx.notify();
        });

        comp
    });

    // 发出事件
    component.update(cx, |_, cx| {
        cx.emit(ValueChanged { new_value: 123 });
    });

    // 验证事件已处理
    let received = component.read_with(cx, |comp, _| comp.received_value);
    assert_eq!(received, 123);
}
```

### Action 测试

测试 Action 分发与处理：

```rust
actions!(my_app, [Increment, Decrement]);

#[gpui::test]
fn test_action_dispatch(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| MyComponent::new(cx))
        }).unwrap()
    });

    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let counter = window.root(&mut cx).unwrap();

    // 通过焦点句柄分发 Action
    let focus_handle = counter.read_with(&cx, |counter, _| counter.focus_handle.clone());
    cx.update(|window, cx| {
        focus_handle.dispatch_action(&Increment, window, cx);
    });

    let count = counter.read_with(&cx, |counter, _| counter.count);
    assert_eq!(count, 1);
}
```

### 异步测试

测试异步操作和后台任务：

```rust
impl MyComponent {
    fn load_data(&self, cx: &mut Context<Self>) -> Task<i32> {
        cx.spawn(async move |this, cx| {
            // 模拟异步工作
            this.update(cx, |comp, _| comp.loading = true).await;
            // 返回结果
            42
        })
    }

    fn background_update(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // 后台工作
            this.update(cx, |comp, _| {
                comp.value += 10;
            }).await;
        }).detach();
    }
}

#[gpui::test]
async fn test_async_operations(cx: &mut TestAppContext) {
    let component = cx.new(|cx| MyComponent::new(cx));

    // 测试等待完成的任务
    let result = component.update(cx, |comp, cx| comp.load_data(cx)).await;
    assert_eq!(result, 42);

    // 测试已分离任务
    component.update(cx, |comp, cx| comp.background_update(cx));

    // 分离后的任务要等到当前任务让出执行权才会运行
    let value_before = component.read_with(cx, |comp, _| comp.value);
    assert_eq!(value_before, 0);

    // 运行待处理任务
    cx.run_until_parked();

    let value_after = component.read_with(cx, |comp, _| comp.value);
    assert_eq!(value_after, 10);
}
```

### 定时器测试

测试基于定时器的操作：

```rust
impl MyComponent {
    fn delayed_action(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;

            this.update(cx, |comp, cx| {
                comp.action_performed = true;
                cx.notify();
            }).await;
        }).detach();
    }
}

#[gpui::test]
async fn test_timers(cx: &mut TestAppContext) {
    let component = cx.new(|cx| MyComponent::new(cx));

    component.update(cx, |comp, cx| comp.delayed_action(cx));

    // Action 此时尚未完成
    let performed = component.read_with(cx, |comp, _| comp.action_performed);
    assert!(!performed);

    // 一直运行到停驻（定时器完成）
    cx.run_until_parked();

    let performed = component.read_with(cx, |comp, _| comp.action_performed);
    assert!(performed);
}
```

### 外部 I/O 测试

涉及外部系统的测试使用 `allow_parking()`：

```rust
#[gpui::test]
async fn test_external_io(cx: &mut TestAppContext) {
    // 允许为外部 I/O 停驻
    cx.executor().allow_parking();

    // 模拟外部操作
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        tx.send(42).ok();
    });

    let result = rx.await.unwrap();
    assert_eq!(result, 42);
}
```

## 测试无崩溃状态管理（重入）

GPUI 中最危险的一类缺陷是 **Entity 重入**：代码尝试读取或更新一个已经在渲染/更新过程中被锁定的 Entity。这类缺陷在编译期不可见，只会在运行时 panic，通常发生在用户点击列表或下拉框项目时。

**重入 panic 的关键特征：**

- 由用户交互（点击、按键）触发，而不是在静态渲染期间触发。
- `#[should_panic]` 只能确认缺陷存在；测试必须在*不* panic 的情况下通过。
- 触发操作后必须调用 `cx.run_until_parked()`，让延迟回调（`defer_in`）执行。

### 模式：通过真实 delegate 驱动 `confirm` / `cancel`

```rust
#[gpui::test]
fn test_confirm_does_not_panic(cx: &mut TestAppContext) {
    // 按真实应用相同的方式构建组件状态。
    let state = cx.new(|cx| {
        SelectState::new(MyDelegate::default(), None, &mut cx.window_handle(), cx)
    });

    // 模拟用户点击第一项，以执行 on_confirm 及其延迟回调；
    // 重入缺陷通常隐藏在这里。
    state.update(cx, |this, cx| {
        this.state.list.update(cx, |list, cx| {
            list.delegate_mut().set_selected_index(Some(IndexPath::new(0)), window, cx);
            list.delegate_mut().confirm(false, window, cx);
        });
    });

    // 让 defer_in 回调执行；这是触发崩溃所必需的。
    cx.run_until_parked();

    // 如果执行到此处且未 panic，说明重入问题已修复。
    let selected = state.read_with(cx, |s, _| s.selected_value().cloned());
    assert!(selected.is_some());
}
```

### 模式：验证 `on_will_change` 和 `on_confirm` 钩子调用正确

使用记录型 delegate 断言钩子按正确顺序、使用正确参数触发：

```rust
#[derive(Default)]
struct RecordingDelegate {
    items: Vec<MyItem>,
    will_change_calls: Vec<Vec<IndexPath>>,
    confirm_calls: Vec<Vec<IndexPath>>,
}

impl SearchableListDelegate for RecordingDelegate {
    // ……所需实现……

    fn on_will_change(
        &mut self,
        change: &mut SearchableListChange<Self>,
        _current: &[(IndexPath, Self::Item)],
    ) {
        self.will_change_calls.push(
            change.select_queue.iter().map(|(ix, _)| *ix).collect()
        );
    }

    fn on_confirm(&mut self, final_selection: &[(IndexPath, Self::Item)]) {
        self.confirm_calls.push(
            final_selection.iter().map(|(ix, _)| *ix).collect()
        );
    }
}

#[gpui::test]
fn test_hooks_fire_in_correct_order(cx: &mut TestAppContext) {
    let state = cx.new(|cx| SelectState::new(RecordingDelegate::with_items(3), None, window, cx));

    // 模拟确认第 0 项
    state.update(cx, |this, cx| {
        // ……触发确认……
    });
    cx.run_until_parked();

    state.read_with(cx, |s, cx| {
        let delegate = s.state.list.read(cx).delegate().delegate;
        assert_eq!(delegate.will_change_calls.len(), 1);
        assert_eq!(delegate.confirm_calls.len(), 1);
        assert_eq!(delegate.confirm_calls[0], vec![IndexPath::new(0)]);
    });
}
```

### 模式：快速多次确认（快照一致性）

```rust
#[gpui::test]
fn test_rapid_confirms_keep_consistent_snapshot(cx: &mut TestAppContext) {
    let state = cx.new(|cx| SelectState::new(MyDelegate::with_items(5), None, window, cx));

    for i in 0..5 {
        state.update(cx, |this, cx| {
            // 触发确认第 i 项
        });
        cx.run_until_parked();

        state.read_with(cx, |s, cx| {
            let snapshot = s.state.list.read(cx).delegate().selection_snapshot.clone();
            let selection = s.state.selection.clone();
            assert_eq!(snapshot, selection, "确认第 {i} 项后快照与选择不同步");
        });
    }
}
```

### 检查清单：每个使用 `defer_in` 的组件都要测试

- [ ] `confirm` 路径：不 panic、最终选择正确、快照与选择一致
- [ ] `cancel` 路径：不 panic、选择不变、Popover 已关闭
- [ ] `on_will_change` 否决：选择不变，且不调用 `on_confirm`
- [ ] `on_will_change` 修改变更：最终选择反映 delegate 的修改
- [ ] 快速连续确认：每次操作后快照都与选择一致
- [ ] 即使在变更后立即调用，`render_item` 也绝不 panic

## 属性测试

使用随机数据测试边界场景：

```rust
#[gpui::test(iterations = 10)]
fn test_counter_random_operations(cx: &mut TestAppContext, mut rng: StdRng) {
    let counter = cx.new(|cx| Counter::new(cx));

    let mut expected = 0i32;
    for _ in 0..100 {
        let delta = rng.random_range(-10..=10);
        expected += delta;

        counter.update(cx, |counter, cx| {
            counter.count += delta;
            cx.notify();
        });
    }

    let actual = counter.read_with(cx, |counter, _| counter.count);
    assert_eq!(actual, expected);
}
```

## 分布式系统测试

测试多个应用上下文之间的通信：

```rust
#[derive(Clone)]
struct NetworkMessage {
    from: String,
    to: String,
    data: i32,
}

#[gpui::test]
fn test_distributed_apps(cx_a: &mut TestAppContext, cx_b: &mut TestAppContext) {
    // 在不同应用上下文中创建组件
    let comp_a = cx_a.new(|_| MyComponent::new("A".to_string()));
    let comp_b = cx_b.new(|_| MyComponent::new("B".to_string()));

    // 模拟消息传递
    comp_a.update(cx_a, |comp, cx| {
        comp.send_message("B", 42, cx);
    });

    // 运行异步操作
    cx_a.run_until_parked();

    // 验证另一个上下文已收到消息
    comp_b.update(cx_b, |comp, _| {
        comp.receive_messages();
    });

    let messages = comp_b.read_with(cx_b, |comp, _| comp.messages.clone());
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data, 42);
}
```

### 交错测试

以随机执行顺序测试并发操作：

```rust
#[gpui::test(iterations = 10)]
fn test_concurrent_operations(
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
    mut rng: StdRng,
) {
    let comp_a = cx_a.new(|_| MyComponent::new());
    let comp_b = cx_b.new(|_| MyComponent::new());

    // 跨上下文执行随机操作
    for i in 0..20 {
        if rng.random_bool(0.5) {
            comp_a.update(cx_a, |comp, cx| {
                comp.perform_operation(i, cx);
            });
        } else {
            comp_b.update(cx_b, |comp, cx| {
                comp.perform_operation(i, cx);
            });
        }
    }

    // 运行所有待处理操作
    cx_a.run_until_parked();

    // 验证最终状态
    let state_a = comp_a.read_with(cx_a, |comp, _| comp.state);
    let state_b = comp_b.read_with(cx_b, |comp, _| comp.state);

    // 断言无论执行顺序如何，不变量都成立
    assert!(state_a.is_consistent());
    assert!(state_b.is_consistent());
}
```

## 模拟与隔离

### 网络模拟

创建模拟网络以测试分布式功能：

```rust
struct MockNetwork {
    messages: Arc<Mutex<Vec<NetworkMessage>>>,
}

impl MockNetwork {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn send(&self, message: NetworkMessage) {
        self.messages.lock().unwrap().push(message);
    }

    fn receive_all(&self) -> Vec<NetworkMessage> {
        self.messages.lock().unwrap().drain(..).collect()
    }
}

#[gpui::test]
fn test_networked_components(cx: &mut TestAppContext) {
    let network = Arc::new(MockNetwork::new());

    let sender = cx.new(|_| MessageSender::new(network.clone()));
    let receiver = cx.new(|_| MessageReceiver::new(network));

    // 发送消息
    sender.update(cx, |sender, _| {
        sender.send("Hello");
    });

    // 接收消息
    receiver.update(cx, |receiver, _| {
        receiver.receive_all();
    });

    let received = receiver.read_with(cx, |receiver, _| receiver.messages.clone());
    assert_eq!(received, vec!["Hello"]);
}
```
