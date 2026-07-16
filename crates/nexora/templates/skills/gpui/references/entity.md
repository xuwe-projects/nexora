# Entity 状态管理

**目录：** [概述](#概述) · [快速开始](#快速开始) · [核心原则](#核心原则) · [常见用例](#常见用例) · [扩展参考](#参考文档)

## 概述

`Entity<T>` 是指向 `T` 类型状态的句柄，用于安全地访问和更新状态。

**关键方法：**

- `entity.read(cx)` → `&T` — 只读访问
- `entity.read_with(cx, |state, cx| ...)` → `R` — 通过闭包读取
- `entity.update(cx, |state, cx| ...)` → `R` — 可变更新
- `entity.downgrade()` → `WeakEntity<T>` — 创建弱引用
- `entity.entity_id()` → `EntityId` — 获取唯一标识符

**Entity 类型：**

- **`Entity<T>`**：强引用（增加引用计数）
- **`WeakEntity<T>`**：弱引用（不会阻止清理，操作返回 `Result`）

## 快速开始

### 创建和使用 Entity

```rust
// 创建 Entity
let counter = cx.new(|cx| Counter { count: 0 });

// 读取状态
let count = counter.read(cx).count;

// 更新状态
counter.update(cx, |state, cx| {
    state.count += 1;
    cx.notify(); // 触发重新渲染
});

// 弱引用（用于闭包/回调）
let weak = counter.downgrade();
let _ = weak.update(cx, |state, cx| {
    state.count += 1;
    cx.notify();
});
```

### 在组件中使用

```rust
struct MyComponent {
    shared_state: Entity<SharedData>,
}

impl MyComponent {
    fn new(cx: &mut App) -> Entity<Self> {
        let shared = cx.new(|_| SharedData::default());

        cx.new(|cx| Self {
            shared_state: shared,
        })
    }

    fn update_shared(&mut self, cx: &mut Context<Self>) {
        self.shared_state.update(cx, |state, cx| {
            state.value = 42;
            cx.notify();
        });
    }
}
```

### 异步操作

从 `Context<Self>` 调用 `cx.spawn` 时，闭包接收 `(WeakEntity<Self>, &mut AsyncApp)`：

```rust
impl MyComponent {
    fn fetch_data(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            let data = fetch_from_api().await;

            // 通过弱引用安全地更新 Entity
            let _ = this.update(cx, |state, cx| {
                state.data = Some(data);
                cx.notify();
            });
        }).detach();
    }
}
```

## 核心原则

### 在闭包中始终使用弱引用

```rust
// ✅ 合适：弱引用可以防止保留环
let weak = cx.entity().downgrade();
callback(move || {
    let _ = weak.update(cx, |state, cx| cx.notify());
});

// ❌ 错误：强引用可能导致内存泄漏
let strong = cx.entity();
callback(move || {
    strong.update(cx, |state, cx| cx.notify());
});
```

### 使用内部上下文

```rust
// ✅ 合适：使用闭包提供的内部 cx
entity.update(cx, |state, inner_cx| {
    inner_cx.notify(); // Correct
});

// ❌ 错误：使用外部 cx（多重借用错误）
entity.update(cx, |state, inner_cx| {
    cx.notify(); // Wrong!
});
```

### 避免嵌套更新 Entity

嵌套调用 `entity.update(cx, …)` 很危险。默认原则是：**不要嵌套更新**。以下子场景说明哪些情况一定 panic，哪些情况只是可能 panic。

**同一个 Entity → 一定 panic。**
GPUI 会在 Entity 的整个更新或渲染期间持有其锁；重入同一把锁会立即 panic：

```
cannot update … while it is already being updated
```

```rust
// ❌ Panic：在 entity_a 自身的更新中再次更新 entity_a
entity_a.update(cx, |state, cx| {
    entity_a.update(cx, |_, _| {}); // PANIC：同一把锁
});
```

**不同 Entity → 通常安全，但间接循环仍会 panic。**
每个 Entity 都有自己的锁，因此在 `entity_a` 的更新中更新 `entity_b` 通常可以成功。但是，如果 `entity_b` 的回调直接或通过调用链再次访问 `entity_a`，GPUI 会尝试重新获取 `entity_a` 的锁并 panic。

```rust
// ✅ 通常安全：不同 Entity，且没有循环
entity_a.update(cx, |_, cx| {
    entity_b.update(cx, |_, _| {}); // 安全：不同的锁
});

// ❌ Panic：间接循环回到 entity_a
entity_a.update(cx, |_, cx| {
    entity_b.update(cx, |_, cx| {
        entity_a.update(cx, |_, _| {}); // PANIC：entity_a 仍处于锁定状态
    });
});
```

存在疑问时，应将调用顺序扁平化而不是嵌套：先结束外层更新，再从外部更新第二个 Entity。

**`defer_in` 不会绕过锁。** `cx.defer_in(window, callback)` 会安排 `callback` 在当前 Entity 上运行，也就是说 GPUI 会重新获取 Entity 的锁来执行它。重入规则同样适用于延迟回调内部：

```rust
// ❌ Panic：defer_in 会重新锁定 entity_a，在内部调用 entity_a.update 会重入
impl SomeDelegate for MyAdapter {
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        cx.defer_in(window, |list_state, window, cx| {
            // 此回调执行期间 list_state 一直处于锁定状态！
            parent.update(cx, |this, cx| {
                this.list.update(cx, |_, _| {}); // PANIC：列表已在上方锁定
            });
        });
    }
}

// ✅ 修复：直接使用回调提供的 &mut 引用
impl SomeDelegate for MyAdapter {
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        cx.defer_in(window, |list_state, window, cx| {
            // 直接访问列表数据，无需 Entity 锁
            list_state.delegate_mut().some_hook();

            // 更新父 Entity；锁不同，因此安全
            parent.update(cx, |this, cx| { /* … */ });

            // 父级更新后直接同步列表状态
            list_state.delegate_mut().update_snapshot(new_val);
        });
    }
}
```

**渲染回调的快照模式。** `render_item`（以及其他渲染钩子）在 Entity 的渲染过程中运行，绝不能对任何外部 Entity 调用 `entity.read(cx)` 或 `entity.update(cx, …)`。应维护普通的 `snapshot` 字段，并在每次变更后从渲染过程*外部*立即更新它：

```rust
// ❌ render_item 中发生 panic：ListState 已被锁定
fn render_item(&mut self, ix: IndexPath, window: &mut Window, cx: &mut Context<ListState<Self>>) -> … {
    let checked = parent_entity.read(cx).selection.contains(&ix); // PANIC
}

// ✅ 从普通快照字段读取，无需访问 Entity
fn render_item(&mut self, ix: IndexPath, window: &mut Window, cx: &mut Context<ListState<Self>>) -> … {
    let checked = self.selection_snapshot.iter().any(|(sel_ix, _)| sel_ix == &ix);
}
```

## 常见用例

1. **组件状态**：需要响应式更新的内部状态
2. **共享状态**：多个组件之间共享的状态
3. **父子协调**：协调相关组件（使用弱引用）
4. **异步状态**：管理由异步操作改变的状态
5. **观察**：响应其他 Entity 的变化

## 参考文档

- **API**：参见 [entity-api.md](entity-api.md) — Entity 类型、方法、生命周期和错误处理
- **模式**：参见 [entity-patterns.md](entity-patterns.md) — 模型-视图、跨 Entity 通信和观察者
- **最佳实践**：参见 [entity-best-practices.md](entity-best-practices.md) — 陷阱、内存、性能和异步
- **高级主题**：参见 [entity-advanced.md](entity-advanced.md) — 集合、注册表、防抖和状态机
