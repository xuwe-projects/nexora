# 全局状态

**目录：** [概述](#概述) · [快速开始](#快速开始) · [常见用例](#常见用例) · [最佳实践](#最佳实践) · [使用时机](#使用时机)

## 概述

GPUI 的全局状态提供应用级共享数据，可从任意上下文访问。

**关键 trait**：在类型上实现 `Global`，使其可以全局访问。

## 快速开始

### 定义全局状态

```rust
use gpui::Global;

#[derive(Clone)]
struct AppSettings {
    theme: Theme,
    language: String,
}

impl Global for AppSettings {}
```

### 初始化和访问全局状态

```rust
fn main() {
    let app = Application::new();
    app.run(|cx: &mut App| {
        // 仅在启动阶段初始化一次
        cx.set_global(AppSettings {
            theme: Theme::Dark,
            language: "en".to_string(),
        });

        // 访问全局状态（只读）
        let settings = cx.global::<AppSettings>();
        println!("Theme: {:?}", settings.theme);
    });
}
```

### 更新全局状态

全局值已经存在后，统一通过 `update_global` 原地修改。不要读取、克隆整个值后再次调用 `set_global` 替换；那会绕过集中更新逻辑，并容易丢失并发发生的字段变化。

```rust
impl MyComponent {
    fn change_theme(&mut self, new_theme: Theme, cx: &mut Context<Self>) {
        cx.update_global::<AppSettings, _>(|settings, cx| {
            settings.theme = new_theme;
            // 更新全局状态不会自动触发通知
            // 手动通知相关组件
        });

        cx.notify(); // 重新渲染此组件
    }
}
```

`set_global` 只用于首次注册；`update_global` 只用于已注册的类型，调用前必须确保启动流程已完成初始化。

## 常见用例

### 1. 应用配置

```rust
#[derive(Clone)]
struct AppConfig {
    api_endpoint: String,
    max_retries: u32,
    timeout: Duration,
}

impl Global for AppConfig {}

// 启动时首次注册；后续修改使用 update_global
cx.set_global(AppConfig {
    api_endpoint: "https://api.example.com".to_string(),
    max_retries: 3,
    timeout: Duration::from_secs(30),
});

// 可在任意位置访问
let config = cx.global::<AppConfig>();
```

### 2. 功能开关

```rust
#[derive(Clone)]
struct FeatureFlags {
    enable_beta_features: bool,
    enable_analytics: bool,
}

impl Global for FeatureFlags {}

impl MyComponent {
    fn render_beta_feature(&self, cx: &App) -> Option<impl IntoElement> {
        let flags = cx.global::<FeatureFlags>();

        if flags.enable_beta_features {
            Some(div().child("测试功能"))
        } else {
            None
        }
    }
}
```

### 3. 共享服务

```rust
#[derive(Clone)]
struct ServiceRegistry {
    http_client: Arc<HttpClient>,
    logger: Arc<Logger>,
}

impl Global for ServiceRegistry {}

impl MyComponent {
    fn fetch_data(&mut self, cx: &mut Context<Self>) {
        let registry = cx.global::<ServiceRegistry>();
        let client = registry.http_client.clone();

        cx.spawn(async move |cx| {
            let data = client.get("api/data").await?;
            // 处理数据……
            Ok::<_, anyhow::Error>(())
        }).detach();
    }
}
```

## 最佳实践

### ✅ 为共享资源使用 Arc

```rust
#[derive(Clone)]
struct GlobalState {
    database: Arc<Database>,  // 克隆成本低
    cache: Arc<RwLock<Cache>>,
}

impl Global for GlobalState {}
```

### ✅ 默认不可变

全局状态默认只读；需要修改时使用内部可变性：

```rust
#[derive(Clone)]
struct Counter {
    count: Arc<AtomicUsize>,
}

impl Global for Counter {}

impl Counter {
    fn increment(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    fn get(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}
```

### ❌ 不要：滥用全局状态

```rust
// ❌ 错误：全局状态过多
cx.set_global(UserState { ... });
cx.set_global(CartState { ... });
cx.set_global(CheckoutState { ... });

// ✅ 合适：组件状态使用 Entity
let user_entity = cx.new(|_| UserState { ... });
```

### ❌ 不要：用 set_global 覆盖已有值

```rust
// ❌ 错误：已有全局值的更新不应重新注册
let mut settings = cx.global::<AppSettings>().clone();
settings.theme = Theme::Light;
cx.set_global(settings);

// ✅ 正确：在 GPUI 更新边界内原地修改
cx.update_global::<AppSettings, _>(|settings, _cx| {
    settings.theme = Theme::Light;
});
```

## 使用时机

**以下场景使用全局状态：**

- 应用级配置
- 功能开关
- 共享服务（HTTP 客户端、日志器）
- 只读参考数据

**以下场景使用 Entity：**

- 组件专用状态
- 频繁变化的状态
- 需要通知机制的状态
