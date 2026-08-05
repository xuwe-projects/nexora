# 事件与订阅

**目录：** [概述](#概述) · [快速开始](#快速开始) · [常用模式](#常用模式) · [subscribe_in](#subscribe_in--订阅时访问窗口) · [observe_window_activation](#observe_window_activation) · [observe_global](#observe_global) · [订阅生命周期](#订阅生命周期) · [最佳实践](#最佳实践)

## 概述

GPUI 提供用于组件协调的事件系统：

**事件机制：**

- **自定义事件**：定义并发出类型安全的事件
- **观察**：响应 Entity 状态变化
- **订阅**：监听其他 Entity 的事件
- **全局事件**：处理应用级事件

## 快速开始

### 定义并发出事件

```rust
#[derive(Clone)]
enum MyEvent {
    DataUpdated(String),
    ActionTriggered,
}

impl MyComponent {
    fn update_data(&mut self, data: String, cx: &mut Context<Self>) {
        self.data = data.clone();

        // 发出事件
        cx.emit(MyEvent::DataUpdated(data));
        cx.notify();
    }
}
```

### 订阅事件

```rust
impl Listener {
    fn new(source: Entity<MyComponent>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // 订阅事件
            cx.subscribe(&source, |this, emitter, event: &MyEvent, cx| {
                match event {
                    MyEvent::DataUpdated(data) => {
                        this.handle_update(data.clone(), cx);
                    }
                    MyEvent::ActionTriggered => {
                        this.handle_action(cx);
                    }
                }
            }).detach();

            Self { source }
        })
    }
}
```

### 观察 Entity 变化

```rust
impl Observer {
    fn new(target: Entity<Target>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // 观察 Entity 的所有变化
            cx.observe(&target, |this, observed, cx| {
                // 在 observed.update() 调用 cx.notify() 时执行
                println!("目标已变化");
                cx.notify();
            }).detach();

            Self { target }
        })
    }
}
```

## 常用模式

### 1. 父子通信

```rust
// 父级发出事件
impl Parent {
    fn notify_children(&mut self, cx: &mut Context<Self>) {
        cx.emit(ParentEvent::Updated);
        cx.notify();
    }
}

// 子级订阅事件
impl Child {
    fn new(parent: Entity<Parent>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.subscribe(&parent, |this, parent, event, cx| {
                this.handle_parent_event(event, cx);
            }).detach();

            Self { parent }
        })
    }
}
```

### 2. 全局事件广播

```rust
struct EventBus {
    listeners: Vec<WeakEntity<dyn Listener>>,
}

impl EventBus {
    fn broadcast(&mut self, event: GlobalEvent, cx: &mut Context<Self>) {
        self.listeners.retain(|weak| {
            weak.update(cx, |listener, cx| {
                listener.on_event(&event, cx);
            }).is_ok()
        });
    }
}
```

### 3. 观察者模式

```rust
cx.observe(&entity, |this, observed, cx| {
    // 响应任意状态变化
    let state = observed.read(cx);
    this.sync_with_state(state, cx);
}).detach();
```

## subscribe_in — 订阅时访问窗口

订阅回调需要 `&mut Window` 时使用：

```rust
// 保存订阅以维持其生命周期
struct MyComponent {
    _subscriptions: Vec<Subscription>,
}

impl MyComponent {
    fn new(input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _subscriptions = vec![
            cx.subscribe_in(input, window, |this, state, event, window, cx| {
                match event {
                    InputEvent::PressEnter { .. } => this.on_submit(window, cx),
                    InputEvent::Change => {
                        let val = state.read(cx).value();
                        this.on_change(val, cx);
                    }
                    _ => {}
                }
            }),
        ];
        Self { _subscriptions }
    }
}
```

`subscribe` 与 `subscribe_in` 的区别：

- `cx.subscribe(&entity, |this, source, event, cx|)` — 不能访问窗口
- `cx.subscribe_in(&entity, window, |this, source, event, window, cx|)` — 可以访问窗口

## observe_window_activation

```rust
let _sub = cx.observe_window_activation(window, |this, window, cx| {
    if window.is_window_active() {
        this.start_polling(cx);
    } else {
        this.stop_polling(cx);
    }
});
```

## observe_global

```rust
cx.observe_global::<Theme>(|cx| {
    cx.notify(); // 主题变化时重新渲染
});
```

## 订阅生命周期

订阅被丢弃时会自动取消。可用两种方式维持订阅：

```rust
// 1. .detach()：持续到 Entity 被丢弃
cx.subscribe(&entity, |this, _, event, cx| {
    // ...
}).detach();

// 2. 保存在结构体中：结构体被丢弃时取消
struct MyView {
    _subscriptions: Vec<Subscription>,
}
// _subscriptions.push(cx.subscribe(...));
```

永久订阅使用 `.detach()`；需要在组件卸载时停止的订阅则保存在结构体中。

## 最佳实践

### ✅ 分离永久订阅

```rust
// ✅ 分离订阅以维持其生命周期
cx.subscribe(&entity, |this, source, event, cx| {
    // 处理事件
}).detach();
```

### ✅ 保持事件类型简洁

```rust
#[derive(Clone)]
enum AppEvent {
    DataChanged { id: usize, value: String },
    ActionPerformed(ActionType),
    Error(String),
}
```

### ❌ 避免事件循环

```rust
// ❌ 不要创建相互订阅
entity1.subscribe(entity2) → emits event
entity2.subscribe(entity1) → emits event → infinite loop!
```

## 参考文档
