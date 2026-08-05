# Entity API 参考

**目录：** [Entity 类型](#entity-类型) · [创建 Entity](#创建-entity) · [Entity 操作](#entity-操作) · [Entity 的上下文方法](#entity-的上下文方法) · [异步操作](#异步操作) · [Entity 生命周期](#entity-生命周期) · [EntityId](#entityid) · [错误处理](#错误处理) · [类型转换](#类型转换)

## Entity 类型

### Entity<T>

对 `T` 类型状态的强引用。

**方法：**
- `entity_id()` → `EntityId` — 返回唯一标识符
- `downgrade()` → `WeakEntity<T>` — 创建弱引用
- `read(cx)` → `&T` — 不可变访问状态
- `read_with(cx, |state, cx| ...)` → `R` — 通过闭包读取并返回闭包结果
- `update(cx, |state, cx| ...)` → `R` — 使用 `Context<T>` 可变更新并返回闭包结果
- `update_in(cx, |state, window, cx| ...)` → `R` — 在可访问 `Window` 时更新（需要 `AsyncWindowContext` 或 `VisualTestContext`）

**重要说明：**
- 尝试更新正在被更新的 Entity 会触发 panic
- 在闭包中使用其提供的内部 `cx`，避免多重借用问题
- 在异步上下文中，返回值会包装在 `anyhow::Result` 中

### WeakEntity<T>

对 `T` 类型状态的弱引用。

**方法：**
- `upgrade()` → `Option<Entity<T>>` — 如果 Entity 仍存在，则转换为强引用
- `read_with(cx, |state, cx| ...)` → `Result<R>` — 如果 Entity 存在则读取
- `update(cx, |state, cx| ...)` → `Result<R>` — 如果 Entity 存在则更新
- `update_in(cx, |state, window, cx| ...)` → `Result<R>` — 如果 Entity 存在则在可访问窗口时更新

**用例：**
- 避免 Entity 之间的循环依赖
- 在闭包/回调中保存引用，同时不阻止清理
- 表示组件之间的可选关系

**重要：** 由于 Entity 可能已不存在，所有操作都返回 `Result`。

### AnyEntity

动态类型的 Entity 句柄，用于保存不同类型的 Entity。

### AnyWeakEntity

动态类型的弱 Entity 句柄。

## 创建 Entity

### cx.new()

使用初始状态创建新的 Entity。

```rust
let entity = cx.new(|cx| MyState {
    count: 0,
    name: "Default".to_string(),
});
```

**参数：**
- `cx: &mut App` 或其他上下文类型
- 接收 `&mut Context<T>` 并返回初始状态 `T` 的闭包

**返回值：** `Entity<T>`

## Entity 操作

### 读取状态

#### read()

直接以只读方式访问状态。

```rust
let count = my_entity.read(cx).count;
```

**使用时机：** 简单字段访问，且不需要上下文操作。

#### read_with()

在可访问上下文的闭包中读取。

```rust
let count = my_entity.read_with(cx, |state, cx| {
    // 可以同时访问状态和上下文
    state.count
});

// 返回多个值
let (count, theme) = my_entity.read_with(cx, |state, cx| {
    (state.count, cx.theme().clone())
});
```

**使用时机：** 需要上下文操作、多个返回值或复杂逻辑。

### 更新状态

#### update()

使用 `Context<T>` 进行可变更新。

```rust
my_entity.update(cx, |state, cx| {
    state.count += 1;
    cx.notify(); // 触发重新渲染
});
```

**可用操作：**
- `cx.notify()` — 触发重新渲染
- `cx.entity()` — 获取当前 Entity
- `cx.emit(event)` — 发出事件
- `cx.spawn(task)` — 启动异步任务
- 其他 `Context<T>` 方法

#### update_in()

在同时访问 `Window` 和 `Context<T>` 时更新。

```rust
my_entity.update_in(cx, |state, window, cx| {
    state.focused = window.is_window_focused();
    cx.notify();
});
```

**要求：** `AsyncWindowContext` 或 `VisualTestContext`

**使用时机：** 需要焦点状态、窗口边界等窗口专用操作。

## Entity 的上下文方法

### cx.entity()

获取当前正在更新的 Entity。

```rust
impl MyComponent {
    fn some_method(&mut self, cx: &mut Context<Self>) {
        let current_entity = cx.entity();  // Entity<MyComponent>
        let weak = current_entity.downgrade();
    }
}
```

### cx.observe()

观察 Entity 的变化。

```rust
cx.observe(&entity, |this, observed_entity, cx| {
    // 在 observed_entity.update() 调用 cx.notify() 时执行
    println!("Entity 已变化");
}).detach();
```

**返回值：** `Subscription` — 调用 `.detach()` 使其永久有效

### cx.subscribe()

订阅 Entity 发出的事件。

```rust
cx.subscribe(&entity, |this, emitter, event: &SomeEvent, cx| {
    // 在 emitter 发出 SomeEvent 时执行
    match event {
        SomeEvent::DataChanged => {
            cx.notify();
        }
    }
}).detach();
```

**返回值：** `Subscription` — 调用 `.detach()` 使其永久有效

### cx.observe_new_entities()

为某一类型的新 Entity 注册回调。

```rust
cx.observe_new_entities::<MyState>(|entity, cx| {
    println!("已创建新 Entity：{:?}", entity.entity_id());
}).detach();
```

## 异步操作

### cx.spawn()

启动前台任务（界面线程）。

```rust
cx.spawn(async move |this, cx| {
    // `this`：WeakEntity<T>
    // `cx`：&mut AsyncApp

    let result = some_async_work().await;

    // 安全地更新 Entity
    let _ = this.update(cx, |state, cx| {
        state.data = result;
        cx.notify();
    });
}).detach();
```

**注意：** 在启动的任务中始终使用 Entity 弱引用，以防止保留环。

### cx.background_spawn()

启动后台任务（后台线程）。

```rust
cx.background_spawn(async move {
    // 长时间运行的计算
    let result = heavy_computation().await;
    // 此处不能直接更新 Entity
    // 使用通道或启动前台任务来更新
}).detach();
```

## Entity 生命周期

### 创建

Entity 通过 `cx.new()` 创建，并立即注册到应用中。

### 引用计数

- `Entity<T>` 是强引用（增加引用计数）
- `WeakEntity<T>` 是弱引用（不增加引用计数）
- 克隆 `Entity<T>` 会增加引用计数

### 释放

所有强引用被丢弃后，Entity 会自动释放。

```rust
{
    let entity = cx.new(|cx| MyState::default());
    // Entity 存在
} // 如果没有其他强引用，Entity 在此处被丢弃
```

**防止内存泄漏：**
- 在闭包/回调中使用 `WeakEntity`
- 父子关系使用 `WeakEntity`
- 避免循环强引用

## EntityId

每个 Entity 都有唯一标识符。

```rust
let id: EntityId = entity.entity_id();

// 可以比较 EntityId
if entity1.entity_id() == entity2.entity_id() {
    // 同一个 Entity
}
```

**用例：**
- 调试与日志记录
- 无需借用即可比较 Entity
- 使用 Entity 作为键的哈希表

## 错误处理

### WeakEntity 操作

所有 `WeakEntity` 操作都返回 `Result`：

```rust
let weak = entity.downgrade();

// 处理潜在失败
match weak.read_with(cx, |state, cx| state.count) {
    Ok(count) => println!("Count: {}", count),
    Err(e) => eprintln!("Entity 已不存在：{}", e),
}

// 或使用 Result 组合器
let _ = weak.update(cx, |state, cx| {
    state.count += 1;
    cx.notify();
}).ok(); // 忽略错误
```

### 更新时的 panic

对同一个 Entity 进行嵌套更新会 panic：

```rust
// ❌ 会 panic
entity.update(cx, |state1, cx| {
    entity.update(cx, |state2, cx| {
        // Panic：Entity 已被借用
    });
});
```

**解决方案：** 顺序执行更新，或使用不同的 Entity。

## 类型转换

### Entity → WeakEntity

```rust
let entity: Entity<T> = cx.new(|cx| T::default());
let weak: WeakEntity<T> = entity.downgrade();
```

### WeakEntity → Entity

```rust
let weak: WeakEntity<T> = entity.downgrade();
let strong: Option<Entity<T>> = weak.upgrade();
```

### AnyEntity

```rust
let any: AnyEntity = entity.into();
let typed: Option<Entity<T>> = any.downcast::<T>();
```

## 最佳实践指南

### 始终使用内部 cx

```rust
// ✅ 合适：使用内部 cx
entity.update(cx, |state, inner_cx| {
    inner_cx.notify(); // 使用 inner_cx，而不是外部 cx
});

// ❌ 错误：使用外部 cx
entity.update(cx, |state, inner_cx| {
    cx.notify(); // 错误！会发生多重借用错误
});
```

### 在闭包中使用弱引用

```rust
// ✅ 合适：弱引用
let weak = cx.entity().downgrade();
callback(move || {
    let _ = weak.update(cx, |state, cx| {
        cx.notify();
    });
});

// ❌ 错误：强引用（形成保留环）
let strong = cx.entity();
callback(move || {
    strong.update(cx, |state, cx| {
        // 可能永远不会被丢弃
        cx.notify();
    });
});
```

### 顺序更新

```rust
// ✅ 合适：顺序更新
entity1.update(cx, |state, cx| { /* ... */ });
entity2.update(cx, |state, cx| { /* ... */ });

// ❌ 错误：嵌套更新
entity1.update(cx, |_, cx| {
    entity2.update(cx, |_, cx| {
        // 如果 Entity 之间有关联，可能 panic
    });
});
```
