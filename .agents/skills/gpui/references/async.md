# 异步与后台任务

**目录：** [概述](#概述) · [快速开始](#快速开始) · [核心模式](#核心模式) · [常见陷阱](#常见陷阱)

## 概述

GPUI 提供集成的异步运行时，用于前台界面更新和后台计算。

**关键概念：**

- **前台任务**：在界面线程运行，可以更新 Entity（`cx.spawn`）
- **后台任务**：在工作线程运行，用于 CPU 密集型工作（`cx.background_spawn`）
- 所有 Entity 更新都在前台线程执行

## 快速开始

### 前台任务（界面更新）

从 `Context<Self>` 调用时，闭包接收 `(WeakEntity<Self>, &mut AsyncApp)`：

```rust
impl MyComponent {
    fn fetch_data(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            // 在界面线程运行，可以 await 并更新 Entity
            let data = fetch_from_api().await;

            this.update(cx, |state, cx| {
                state.data = Some(data);
                cx.notify();
            }).ok();
        }).detach();
    }
}
```

从 `&mut App`（不在 Entity 内部）调用时，闭包只接收 `(cx: &mut AsyncApp)`：

```rust
cx.spawn(async move |cx: &mut AsyncApp| {
    // 没有 Entity 引用
}).detach();
```

### 携带窗口上下文启动任务（spawn_in）

任务还需要通过 `update_in` 访问窗口时，使用 `spawn_in`：

```rust
impl MyComponent {
    fn animate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            // 此处的 cx 是 AsyncWindowContext
            this.update_in(cx, |state, window, cx| {
                // 此处可以访问窗口
                state.frame += 1;
                cx.notify();
            }).ok();
        }).detach();
    }
}
```

### 后台任务（繁重工作）

```rust
impl MyComponent {
    fn process_file(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();

        cx.background_spawn(async move {
            // 在后台线程运行，用于 CPU 密集型工作
            let result = heavy_computation().await;
            result
        })
        .then(cx.spawn(move |result, cx| {
            // 回到前台更新界面
            entity.update(cx, |state, cx| {
                state.result = result;
                cx.notify();
            }).ok();
        }))
        .detach();
    }
}
```

### 任务管理

```rust
struct MyView {
    _task: Task<()>,  // 保存但不访问时使用 _ 前缀
}

impl MyView {
    fn new(cx: &mut Context<Self>) -> Self {
        let _task = cx.spawn(async move |this, cx: &mut AsyncApp| {
            // Task 被丢弃时自动取消
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;

                this.update(cx, |state, cx| {
                    state.tick();
                    cx.notify();
                }).ok();
            }
        });

        Self { _task }
    }
}
```

## 核心模式

### 1. 异步获取数据（从 Context<Self> 启动）

```rust
cx.spawn(async move |this, cx: &mut AsyncApp| {
    let data = fetch_data().await?;
    this.update(cx, |state, cx| {
        state.data = Some(data);
        cx.notify();
    })?;
    Ok::<_, anyhow::Error>(())
}).detach();
```

### 2. 后台计算 + 界面更新

```rust
cx.background_spawn(async move {
    heavy_work()
})
.then(cx.spawn(move |this, cx: &mut AsyncApp| {
    this.update(cx, |state, cx| {
        state.result = result;
        cx.notify();
    }).ok();
}))
.detach();
```

### 3. 周期性任务

```rust
cx.spawn(async move |this, cx: &mut AsyncApp| {
    loop {
        cx.background_executor().timer(Duration::from_secs(5)).await;

        this.update(cx, |state, cx| {
            state.tick();
            cx.notify();
        }).ok();
    }
}).detach();
```

### 4. 任务取消

`Task` 被丢弃时会自动取消；要让任务持续运行，请将其保存在结构体中。

## 常见陷阱

### ❌ 不要：使用 `defer_in` 后再通过句柄更新同一个 Entity

`cx.defer_in(window, callback)` 会安排 `callback` **在当前 Entity 上**运行，GPUI 会重新获取该 Entity 的锁来执行回调。在延迟回调中对*同一个* Entity 调用 `entity.update(cx, …)` 会重入该锁并触发 panic：

```
cannot update … while it is already being updated
```

```rust
// ❌ Panic：defer_in 已锁定列表 Entity，调用 list.update 会重入
fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
    cx.defer_in(window, |list_state, window, cx| {
        parent.update(cx, |this, cx| {
            this.inner_list.update(cx, |_, _| {}); // 如果 inner_list 就是延迟执行的 Entity，则 PANIC
        });
    });
}
```

```rust
// ✅ 正确：直接使用 &mut 引用，无需加锁
fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
    cx.defer_in(window, |list_state, window, cx| {
        // 通过 &mut 引用直接访问列表数据
        list_state.delegate_mut().some_method();

        // 更新另一个 Entity；锁不同，因此安全
        parent.update(cx, |this, cx| { /* … */ });

        // 父级更新后直接同步列表状态，无需加锁
        list_state.delegate_mut().update_snapshot(new_val);
    });
}
```

规则是：在 `defer_in` 回调中，**绝不要对安排该回调的 Entity 调用 `entity.update(cx, …)` 或 `entity.read(cx)`**。应直接使用回调提供的 `&mut Entity` 引用。

### ❌ 不要：从后台任务更新 Entity

```rust
// ❌ 错误：不能从后台线程更新 Entity
cx.background_spawn(async move {
    entity.update(cx, |state, cx| { // 编译错误！
        state.data = data;
    });
});
```

### ✅ 应当：使用前台任务或任务链

```rust
// ✅ 正确：链接前台任务
cx.background_spawn(async move { data })
    .then(cx.spawn(move |data, cx| {
        entity.update(cx, |state, cx| {
            state.data = data;
            cx.notify();
        }).ok();
    }))
    .detach();
```
