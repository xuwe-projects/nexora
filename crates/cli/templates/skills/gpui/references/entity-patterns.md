# Entity 模式

**目录：** [应用场景](#应用场景) · [常用模式](#常用模式) · [模式选择指南](#模式选择指南)

## 应用场景

### 模型与视图分离

使用 Entity 将业务逻辑（模型）与界面（视图）分离。

```rust
struct CounterModel {
    count: usize,
    listeners: Vec<Box<dyn Fn(usize)>>,
}

struct CounterView {
    model: Entity<CounterModel>,
}

impl CounterModel {
    fn increment(&mut self, cx: &mut Context<Self>) {
        self.count += 1;

        // 通知监听器
        for listener in &self.listeners {
            listener(self.count);
        }

        cx.notify();
    }

    fn decrement(&mut self, cx: &mut Context<Self>) {
        if self.count > 0 {
            self.count -= 1;
            cx.notify();
        }
    }
}

impl CounterView {
    fn new(cx: &mut App) -> Entity<Self> {
        let model = cx.new(|_cx| CounterModel {
            count: 0,
            listeners: Vec::new(),
        });

        cx.new(|cx| Self { model })
    }

    fn increment_count(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            model.increment(cx);
        });
    }
}

impl Render for CounterView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.model.read(cx).count;

        div()
            .child(format!("Count: {}", count))
            .child(
                Button::new("increment")
                    .label("Increment")
                    .on_click(cx.listener(|this, _, cx| {
                        this.increment_count(cx);
                    }))
            )
    }
}
```

### 组件状态管理

使用 Entity 管理复杂组件状态。

```rust
struct TodoList {
    todos: Vec<Todo>,
    filter: TodoFilter,
    next_id: usize,
}

struct Todo {
    id: usize,
    text: String,
    completed: bool,
}

enum TodoFilter {
    All,
    Active,
    Completed,
}

impl TodoList {
    fn new() -> Self {
        Self {
            todos: Vec::new(),
            filter: TodoFilter::All,
            next_id: 0,
        }
    }

    fn add_todo(&mut self, text: String, cx: &mut Context<Self>) {
        self.todos.push(Todo {
            id: self.next_id,
            text,
            completed: false,
        });
        self.next_id += 1;
        cx.notify();
    }

    fn toggle_todo(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(todo) = self.todos.iter_mut().find(|t| t.id == id) {
            todo.completed = !todo.completed;
            cx.notify();
        }
    }

    fn remove_todo(&mut self, id: usize, cx: &mut Context<Self>) {
        self.todos.retain(|t| t.id != id);
        cx.notify();
    }

    fn set_filter(&mut self, filter: TodoFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        cx.notify();
    }

    fn visible_todos(&self) -> impl Iterator<Item = &Todo> {
        self.todos.iter().filter(move |todo| match self.filter {
            TodoFilter::All => true,
            TodoFilter::Active => !todo.completed,
            TodoFilter::Completed => todo.completed,
        })
    }
}
```

### 跨 Entity 通信

协调父 Entity 与子 Entity 之间的状态。

```rust
struct ParentComponent {
    child_entities: Vec<Entity<ChildComponent>>,
    global_message: String,
}

struct ChildComponent {
    id: usize,
    message: String,
    parent: WeakEntity<ParentComponent>,
}

impl ParentComponent {
    fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            child_entities: Vec::new(),
            global_message: String::new(),
        })
    }

    fn add_child(&mut self, cx: &mut Context<Self>) {
        let parent_weak = cx.entity().downgrade();
        let child_id = self.child_entities.len();

        let child = cx.new(|cx| ChildComponent {
            id: child_id,
            message: String::new(),
            parent: parent_weak,
        });

        self.child_entities.push(child);
        cx.notify();
    }

    fn broadcast_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.global_message = message.clone();

        // 更新所有子级
        for child in &self.child_entities {
            child.update(cx, |child_state, cx| {
                child_state.message = message.clone();
                cx.notify();
            });
        }

        cx.notify();
    }
}

impl ChildComponent {
    fn notify_parent(&mut self, message: String, cx: &mut Context<Self>) {
        if let Ok(_) = self.parent.update(cx, |parent_state, cx| {
            parent_state.global_message = format!("Child {}: {}", self.id, message);
            cx.notify();
        }) {
            // 已成功通知父级
        }
    }
}
```

### Entity 异步操作

管理异步状态更新。

```rust
struct DataLoader {
    loading: bool,
    data: Option<String>,
    error: Option<String>,
}

impl DataLoader {
    fn new() -> Self {
        Self {
            loading: false,
            data: None,
            error: None,
        }
    }

    fn load_data(&mut self, cx: &mut Context<Self>) {
        // 设置加载状态
        self.loading = true;
        self.error = None;
        cx.notify();

        // 获取供异步任务使用的弱引用
        let entity = cx.entity().downgrade();

        cx.spawn(async move |cx| {
            // 模拟异步操作
            tokio::time::sleep(Duration::from_secs(2)).await;
            let result = fetch_data().await;

            // 使用结果更新 Entity
            let _ = entity.update(cx, |state, cx| {
                state.loading = false;
                match result {
                    Ok(data) => state.data = Some(data),
                    Err(e) => state.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }).detach();
    }
}

async fn fetch_data() -> Result<String, anyhow::Error> {
    // 实际获取实现
    Ok("已获取数据".to_string())
}
```

### 后台任务协调

将后台任务与 Entity 更新结合使用。

```rust
struct ImageProcessor {
    images: Vec<ProcessedImage>,
    processing: bool,
}

struct ProcessedImage {
    path: PathBuf,
    thumbnail: Option<Vec<u8>>,
}

impl ImageProcessor {
    fn process_images(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.processing = true;
        cx.notify();

        let entity = cx.entity().downgrade();

        cx.background_spawn({
            let paths = paths.clone();
            async move {
                let mut processed = Vec::new();

                for path in paths {
                    // 在后台线程处理图像
                    let thumbnail = generate_thumbnail(&path).await;
                    processed.push((path, thumbnail));
                }

                // 将结果发送回前台
                processed
            }
        })
        .then(cx.spawn(move |processed, cx| {
            // 在前台线程更新 Entity
            let _ = entity.update(cx, |state, cx| {
                for (path, thumbnail) in processed {
                    state.images.push(ProcessedImage {
                        path,
                        thumbnail: Some(thumbnail),
                    });
                }
                state.processing = false;
                cx.notify();
            });
        }))
        .detach();
    }
}
```

## 常用模式

### 1. 有状态组件

为维护内部状态的组件使用 Entity。

```rust
struct StatefulComponent {
    value: i32,
    history: Vec<i32>,
}

impl StatefulComponent {
    fn update_value(&mut self, new_value: i32, cx: &mut Context<Self>) {
        self.history.push(self.value);
        self.value = new_value;
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(prev_value) = self.history.pop() {
            self.value = prev_value;
            cx.notify();
        }
    }
}
```

### 2. 共享状态

使用 Entity 在多个组件之间共享状态。

```rust
struct SharedState {
    theme: Theme,
    user: Option<User>,
}

struct ComponentA {
    shared: Entity<SharedState>,
}

struct ComponentB {
    shared: Entity<SharedState>,
}

// 两个组件都可以读取/更新同一个共享状态
impl ComponentA {
    fn update_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.shared.update(cx, |state, cx| {
            state.theme = theme;
            cx.notify();
        });
    }
}
```

### 3. 事件协调

使用 Entity 协调组件之间的事件。

```rust
struct EventCoordinator {
    listeners: Vec<WeakEntity<dyn EventListener>>,
}

trait EventListener {
    fn on_event(&mut self, event: &AppEvent, cx: &mut App);
}

impl EventCoordinator {
    fn emit_event(&mut self, event: AppEvent, cx: &mut Context<Self>) {
        // 通知所有监听器
        self.listeners.retain(|weak_listener| {
            weak_listener.update(cx, |listener, cx| {
                listener.on_event(&event, cx);
            }).is_ok()
        });
        cx.notify();
    }
}
```

### 4. 异步状态管理

管理随异步操作结果变化的状态。

```rust
struct AsyncState<T> {
    state: AsyncValue<T>,
}

enum AsyncValue<T> {
    Loading,
    Loaded(T),
    Error(String),
}

impl<T> AsyncState<T> {
    fn is_loading(&self) -> bool {
        matches!(self.state, AsyncValue::Loading)
    }

    fn value(&self) -> Option<&T> {
        match &self.state {
            AsyncValue::Loaded(v) => Some(v),
            _ => None,
        }
    }
}
```

### 5. 父子关系

使用弱引用管理层级关系。

```rust
struct Parent {
    children: Vec<Entity<Child>>,
}

struct Child {
    parent: WeakEntity<Parent>,
    data: String,
}

impl Child {
    fn notify_parent_of_change(&mut self, cx: &mut Context<Self>) {
        if let Ok(_) = self.parent.update(cx, |parent, cx| {
            // 父级可以响应子级变化
            cx.notify();
        }) {
            // 通知成功
        }
    }
}
```

### 6. 观察者模式

使用观察者响应 Entity 状态变化。

```rust
struct Observable {
    value: i32,
}

struct Observer {
    observed: Entity<Observable>,
}

impl Observer {
    fn new(observed: Entity<Observable>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // 观察 Entity
            cx.observe(&observed, |this, observed_entity, cx| {
                // 响应变化
                let value = observed_entity.read(cx).value;
                println!("值已变为：{}", value);
            }).detach();

            Self { observed }
        })
    }
}
```

### 7. 事件订阅

处理其他 Entity 发出的事件。

```rust
#[derive(Clone)]
enum DataEvent {
    Updated,
    Deleted,
}

struct DataSource {
    data: Vec<String>,
}

impl DataSource {
    fn update_data(&mut self, cx: &mut Context<Self>) {
        // 更新数据
        cx.emit(DataEvent::Updated);
        cx.notify();
    }
}

struct DataConsumer {
    source: Entity<DataSource>,
}

impl DataConsumer {
    fn new(source: Entity<DataSource>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // 订阅事件
            cx.subscribe(&source, |this, source, event: &DataEvent, cx| {
                match event {
                    DataEvent::Updated => {
                        // 处理更新
                        cx.notify();
                    }
                    DataEvent::Deleted => {
                        // 处理删除
                    }
                }
            }).detach();

            Self { source }
        })
    }
}
```

### 8. 资源管理

管理外部资源并正确清理。

```rust
struct FileHandle {
    path: PathBuf,
    file: Option<File>,
}

impl FileHandle {
    fn open(&mut self, cx: &mut Context<Self>) -> Result<()> {
        self.file = Some(File::open(&self.path)?);
        cx.notify();
        Ok(())
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.file = None;
        cx.notify();
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        // Entity 被丢弃时清理
        if let Some(file) = self.file.take() {
            drop(file);
        }
    }
}
```

## 模式选择指南

| 需求 | 模式 | 复杂度 |
|------|------|--------|
| 具有内部状态的组件 | 有状态组件 | 低 |
| 多个组件共享状态 | 共享状态 | 低 |
| 协调组件间事件 | 事件协调 | 中 |
| 处理异步数据获取 | 异步状态管理 | 中 |
| 父子组件层级 | 父子关系 | 中 |
| 响应状态变化 | 观察者模式 | 中 |
| 处理自定义事件 | 事件订阅 | 中到高 |
| 管理外部资源 | 资源管理 | 高 |

选择满足需求的最简单模式；复杂场景可按需组合多种模式。
