# Entity 最佳实践

**目录：** [避免常见陷阱](#避免常见陷阱) · [性能优化](#性能优化) · [Entity 生命周期管理](#entity-生命周期管理) · [Entity 观察最佳实践](#entity-观察最佳实践) · [异步最佳实践](#异步最佳实践) · [测试最佳实践](#测试最佳实践) · [性能检查清单](#性能检查清单)

## 避免常见陷阱

### 避免重入访问同一个 Entity

**问题：** GPUI 会在 Entity 的整个渲染或更新期间持有其锁。持锁期间尝试 `read` 或 `update` *同一个* Entity 会 panic：

```
cannot update … while it is already being updated
```

```rust
// ❌ Panic：在 entity_a 自身的更新中再次更新 entity_a
entity_a.update(cx, |_, cx| {
    entity_a.update(cx, |_, _| {}); // PANIC
});

// ✅ 安全：在更新中修改另一个 Entity
entity_a.update(cx, |_, cx| {
    entity_b.update(cx, |_, _| {}); // 安全：不同的锁
});
```

**注意：** 以嵌套方式更新两个*不同* Entity 是安全的；限制只针对重入*同一个* Entity 的锁。

### `defer_in` 会重新锁定 Entity，同样适用重入规则

`cx.defer_in(window, callback)` 会安排 `callback` *在上下文所指的 Entity 上*运行。GPUI 会重新获取该 Entity 的锁来执行延迟回调，因此重入规则同样适用于回调内部：

```rust
// ❌ Panic：defer_in 运行时已锁定 ListState，调用 list.update 会重入
fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
    cx.defer_in(window, |list_state, window, cx| {
        // 整个回调执行期间 list_state 一直处于锁定状态！
        parent.update(cx, |this, cx| {
            this.list.update(cx, |_, _| {}); // PANIC：重入 ListState 的锁
        });
    });
}
```

**修复：** 直接使用回调提供的 `&mut` 引用，不要再通过 Entity 句柄访问：

```rust
// ✅ 正确：直接进行可变访问，无需加锁
fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
    cx.defer_in(window, |list_state, window, cx| {
        // 第 1 步：通过 list_state 直接调用钩子（无需 Entity 锁）
        list_state.delegate_mut().on_will_change(&mut op, &snapshot);

        // 第 2 步：更新父 Entity；锁不同，因此安全
        let new_sel = parent.update(cx, |this, cx| {
            this.state.apply_change(op);
            cx.notify();
            this.state.selection.clone() // 返回第 3 步所需的数据
        });

        // 第 3 步：直接同步列表状态，无需 Entity 锁
        if let Ok(sel) = new_sel {
            list_state.delegate_mut().update_snapshot(sel.clone());
            list_state.delegate_mut().on_confirm(&sel);
        }
    });
}
```

### 渲染回调不得访问外部 Entity

`render_item` 和其他渲染钩子都在 Entity 的渲染过程中运行。对外部 Entity（其自身可能也处于渲染/更新过程）调用 `entity.read(cx)` 或 `entity.update(cx, …)` 会因同样的重入错误而 panic。

**修复：** 维护普通的 `snapshot` 字段，并在每次变更后从渲染过程外部立即更新：

```rust
// ❌ Panic：在 ListState 渲染期间调用，访问外部 Entity 会重入
fn render_item(&mut self, ix: IndexPath, …) -> … {
    let checked = parent.read(cx).selection.contains(&ix); // PANIC
}

// ✅ 从快照字段读取，完全不访问 Entity
fn render_item(&mut self, ix: IndexPath, …) -> … {
    let checked = self.selection_snapshot.iter().any(|(sel_ix, _)| sel_ix == &ix);
}
// 每次从渲染外部变更后：
list.update(cx, |l, _| l.delegate_mut().update_snapshot(new_snapshot));
```

### 在闭包中使用弱引用

**问题：** 闭包中的强引用可能形成保留环并导致内存泄漏。

```rust
// ❌ 错误：强引用形成保留环
impl MyComponent {
    fn setup_callback(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity(); // 强引用

        some_callback(move || {
            entity.update(cx, |state, cx| {
                // 此闭包持有强引用
                // 如果 Entity 自身又保留此闭包，就会内存泄漏！
                cx.notify();
            });
        });
    }
}
```

**解决方案：** 在闭包中使用弱引用。

```rust
// ✅ 合适：弱引用可以防止保留环
impl MyComponent {
    fn setup_callback(&mut self, cx: &mut Context<Self>) {
        let weak_entity = cx.entity().downgrade(); // 弱引用

        some_callback(move || {
            // 安全：弱引用不会阻止清理
            let _ = weak_entity.update(cx, |state, cx| {
                cx.notify();
            });
        });
    }
}
```

### 在闭包中使用内部上下文

**问题：** 使用外部上下文会导致多重借用错误。

```rust
// ❌ 错误：使用外部 cx 会造成借用问题
entity.update(cx, |state, inner_cx| {
    cx.notify(); // 错误！使用了外部 cx
    cx.spawn(...); // 多重借用错误
});
```

**解决方案：** 始终使用闭包提供的内部上下文。

```rust
// ✅ 合适：使用内部 cx
entity.update(cx, |state, inner_cx| {
    inner_cx.notify(); // Correct
    inner_cx.spawn(...); // 正常工作
});
```

### 将 Entity 作为属性传递时使用弱引用

**问题：** 属性中的 Entity 强引用可能造成所有权问题。

```rust
// ❌ 有问题：子级中使用强引用
struct ChildComponent {
    parent: Entity<ParentComponent>, // 强引用
}
```

**更好的做法：** 父级关系使用弱引用。

```rust
// ✅ 合适：弱引用可以避免问题
struct ChildComponent {
    parent: WeakEntity<ParentComponent>, // 弱引用
}

impl ChildComponent {
    fn notify_parent(&mut self, cx: &mut Context<Self>) {
        // 检查父级是否仍然存在
        if let Ok(_) = self.parent.update(cx, |parent_state, cx| {
            // 更新父级
            cx.notify();
        }) {
            // 父级更新成功
        }
    }
}
```

## 性能优化

### 尽量减少 cx.notify() 调用

每次 `cx.notify()` 都会触发重新渲染，应尽可能批量更新。

```rust
// ❌ 错误：多次通知
impl MyComponent {
    fn update_multiple_fields(&mut self, cx: &mut Context<Self>) {
        self.field1 = new_value1;
        cx.notify(); // 不必要的中间通知

        self.field2 = new_value2;
        cx.notify(); // 不必要的中间通知

        self.field3 = new_value3;
        cx.notify();
    }
}
```

```rust
// ✅ 合适：完成所有更新后只通知一次
impl MyComponent {
    fn update_multiple_fields(&mut self, cx: &mut Context<Self>) {
        self.field1 = new_value1;
        self.field2 = new_value2;
        self.field3 = new_value3;
        cx.notify(); // 单次通知
    }
}
```

### 条件更新

只在状态实际变化时通知。

```rust
impl MyComponent {
    fn set_value(&mut self, new_value: i32, cx: &mut Context<Self>) {
        if self.value != new_value {
            self.value = new_value;
            cx.notify(); // 仅在发生变化时通知
        }
    }
}
```

### 为复杂操作使用 read_with

优先使用 `read_with`，不要分开多次调用 `read`。

```rust
// ❌ 效率较低：多次借用
let state_ref = entity.read(cx);
let value1 = state_ref.field1;
let value2 = state_ref.field2;
// state_ref 在整个作用域内都保持借用

// ✅ 效率更高：通过闭包只借用一次
let (value1, value2) = entity.read_with(cx, |state, cx| {
    (state.field1, state.field2)
});
```

### 避免过度创建 Entity

创建 Entity 有额外开销，应在合适时复用。

```rust
// ❌ 错误：在渲染中为每个项目创建 Entity
impl Render for MyList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().children(
            self.items.iter().map(|item| {
                // 不要在渲染中创建 Entity！
                let entity = cx.new(|_| item.clone());
                ItemView { entity }
            })
        )
    }
}
```

```rust
// ✅ 合适：只创建一次并复用 Entity
struct MyList {
    item_entities: Vec<Entity<Item>>,
}

impl MyList {
    fn add_item(&mut self, item: Item, cx: &mut Context<Self>) {
        let entity = cx.new(|_| item);
        self.item_entities.push(entity);
        cx.notify();
    }
}
```

## Entity 生命周期管理

### 清理弱引用

定期从集合中清理无效弱引用。

```rust
struct Container {
    weak_children: Vec<WeakEntity<Child>>,
}

impl Container {
    fn cleanup_invalid_children(&mut self, cx: &mut Context<Self>) {
        // 移除已失效的弱引用
        let before_count = self.weak_children.len();
        self.weak_children.retain(|weak| weak.upgrade().is_some());
        let after_count = self.weak_children.len();

        if before_count != after_count {
            cx.notify(); // 列表变化时通知
        }
    }
}
```

### Entity 克隆与共享

注意，克隆 `Entity<T>` 会增加引用计数。

```rust
// 每次克隆都会增加引用计数
let entity1: Entity<MyState> = cx.new(|_| MyState::default());
let entity2 = entity1.clone(); // 引用计数：2
let entity3 = entity1.clone(); // 引用计数：3

// 只有所有引用都被丢弃后，Entity 才会被丢弃
drop(entity1); // 引用计数：2
drop(entity2); // 引用计数：1
drop(entity3); // 引用计数：0，释放 Entity
```

### 正确清理资源

在 `Drop` 或显式清理方法中实现资源清理。

```rust
struct ManagedResource {
    handle: Option<FileHandle>,
}

impl ManagedResource {
    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.handle.take() {
            // 显式清理
            handle.close();
            cx.notify();
        }
    }
}

impl Drop for ManagedResource {
    fn drop(&mut self) {
        // Entity 被丢弃时自动清理
        if let Some(handle) = self.handle.take() {
            handle.close();
        }
    }
}
```

## Entity 观察最佳实践

### 适当地分离订阅

对需要持续存在的订阅调用 `.detach()`。

```rust
impl MyComponent {
    fn new(other_entity: Entity<OtherComponent>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // 只要两个 Entity 都存在，观察者就持续存在
            cx.observe(&other_entity, |this, observed, cx| {
                // 处理变化
                cx.notify();
            }).detach(); // 重要：调用 detach 使其永久有效

            Self { /* fields */ }
        })
    }
}
```

### 避免观察环

不要让 Entity 相互观察。

```rust
// ❌ 错误：相互观察可能导致无限循环
entity1.update(cx, |_, cx| {
    cx.observe(&entity2, |_, _, cx| {
        cx.notify(); // 可能触发 entity2 的观察者
    }).detach();
});

entity2.update(cx, |_, cx| {
    cx.observe(&entity1, |_, _, cx| {
        cx.notify(); // 可能触发 entity1 的观察者，导致无限循环
    }).detach();
});
```

## 异步最佳实践

### 在异步任务中始终使用弱引用

```rust
// ✅ 合适：启动的任务使用弱引用
impl MyComponent {
    fn fetch_data(&mut self, cx: &mut Context<Self>) {
        let weak_entity = cx.entity().downgrade();

        cx.spawn(async move |cx| {
            let data = fetch_from_api().await;

            // 获取数据期间 Entity 可能已被丢弃
            let _ = weak_entity.update(cx, |state, cx| {
                state.data = Some(data);
                cx.notify();
            });
        }).detach();
    }
}
```

### 优雅处理异步错误

```rust
impl MyComponent {
    fn fetch_data(&mut self, cx: &mut Context<Self>) {
        let weak_entity = cx.entity().downgrade();

        cx.spawn(async move |cx| {
            match fetch_from_api().await {
                Ok(data) => {
                    let _ = weak_entity.update(cx, |state, cx| {
                        state.data = Some(data);
                        state.error = None;
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak_entity.update(cx, |state, cx| {
                        state.error = Some(e.to_string());
                        cx.notify();
                    });
                }
            }
        }).detach();
    }
}
```

### 取消模式

为长时间运行的任务实现取消机制。

```rust
struct DataFetcher {
    current_task: Option<Task<()>>,
    data: Option<String>,
}

impl DataFetcher {
    fn fetch_data(&mut self, url: String, cx: &mut Context<Self>) {
        // 取消之前的任务
        self.current_task = None; // 丢弃 Task 即可取消

        let weak_entity = cx.entity().downgrade();

        let task = cx.spawn(async move |cx| {
            let data = fetch_from_url(&url).await?;

            let _ = weak_entity.update(cx, |state, cx| {
                state.data = Some(data);
                cx.notify();
            });

            Ok::<(), anyhow::Error>(())
        });

        self.current_task = Some(task);
    }
}
```

## 测试最佳实践

### Entity 测试使用 TestAppContext

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn test_entity_update(cx: &mut TestAppContext) {
        let entity = cx.new(|_| MyState { count: 0 });

        entity.update(cx, |state, cx| {
            state.count += 1;
            assert_eq!(state.count, 1);
        });

        let count = entity.read(cx).count;
        assert_eq!(count, 1);
    }
}
```

### 测试 Entity 观察

```rust
#[gpui::test]
fn test_entity_observation(cx: &mut TestAppContext) {
    let observed = cx.new(|_| MyState { value: 0 });
    let observer = cx.new(|cx| Observer::new(observed.clone(), cx));

    // 更新被观察的 Entity
    observed.update(cx, |state, cx| {
        state.value = 42;
        cx.notify();
    });

    // 验证观察者已收到通知
    observer.read(cx).assert_observed();
}
```

## 性能检查清单

发布基于 Entity 的代码前，确认：

- [ ] 闭包/回调中没有强引用（使用 `WeakEntity`）
- [ ] 没有嵌套更新 Entity（顺序执行更新）
- [ ] 更新闭包使用内部 `cx`
- [ ] 调用 `cx.notify()` 前批量完成更新
- [ ] 定期清理无效弱引用
- [ ] 复杂读取操作使用 `read_with`
- [ ] 正确分离订阅和观察者
- [ ] 异步任务使用弱引用
- [ ] Entity 之间没有观察环
- [ ] 异步操作具有适当错误处理
- [ ] 在 `Drop` 或显式方法中清理资源
- [ ] 测试覆盖 Entity 生命周期和交互
