---
name: define-page
description: 用于在 Nexora GPUI 桌面应用中创建或修改页面。适用于新增自动注册的 Feature、普通独立 Window、唯一 SettingsWindow 或 LoginFeature，以及 Sidebar Header/Footer；覆盖强类型路由、生命周期、导航和测试。
---

# 定义 Nexora 桌面页面

## 先确认页面类型

- 主 Sidebar 中可见并作为标签内容打开：使用 `#[derive(nexora::Feature)]`。
- 不进入导航和标签、通过路径打开原生窗口：使用 `#[derive(nexora::Window)]`。
- 完整替换框架默认 `/settings`：使用唯一的 `#[derive(nexora::SettingsWindow)]`。
- 完整替换 Account 未登录门禁：使用唯一的 `#[derive(nexora::LoginFeature)]`。
- 自定义主 Sidebar 顶部或底部：使用唯一的 `SidebarHeader` 或 `SidebarFooter`。

先读取目标应用现有 `features.rs` 与目录布局。禁止创建 `mod.rs`；新文件必须由父模块声明并进入编译，否则 inventory 无法发现派生注册项。不要手写 FeatureId、路由 catalog、RootView 分支或第二套导航表。

## 创建 Feature

```rust
use nexora::{
    FeatureContextExt as _, FeatureElement,
    gpui::{Context, IntoElement, Window, div, prelude::*},
};

#[derive(Clone, serde::Deserialize)]
struct OrderPath {
    id: u64,
}

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "订单详情",
    path = "/orders/:id",
    path_params = OrderPath,
    navigation = false
)]
struct OrderFeature;

impl FeatureElement for OrderFeature {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().child(format!("订单 {}", cx.path().id))
    }
}
```

- 静态导航页配置 `title/path`，并按需增加 `section/icon/order/parent`。
- 含 `:name` 的动态路径必须声明 `path_params = T` 并设置 `navigation = false`。
- 查询参数用 `query_params = Q`；`T/Q` 必须实现 `serde::Deserialize + Clone + 'static`。
- 页面通过 `FeatureContextExt::path/query` 读取强类型参数，不再手工解析字符串。
- 子 Entity、订阅和任务放在 `initialize`，可见性副作用放在 `activated/deactivated`；不要在 `render` 中创建长期状态。
- 无法使用 `Default` 时声明 `#[nexora(factory = Type::new)]`；签名为 `fn new(&mut Window, &mut Context<Self>) -> Self`。

## 创建普通独立 Window

```rust
use nexora::gpui::prelude::*;

#[derive(Default, nexora::Window)]
#[nexora(title = "个人资料", path = "/profile", icon = "user")]
struct ProfileWindow;

impl nexora::WindowElement for ProfileWindow {
    fn render(
        &mut self,
        _window: &mut nexora::gpui::Window,
        _cx: &mut nexora::gpui::Context<Self>,
    ) -> impl nexora::gpui::IntoElement {
        nexora::gpui::div().child("个人资料")
    }
}
```

Window 支持与 Feature 相同的 `path_params/query_params/factory`。应用可覆盖 `window_options`、`initialize` 与 `closing`；Nexora 负责强类型绑定、`gpui_component::Root`、主题挂载和原生开窗，不要再手写 `cx.open_window` 或窗口 Global。

在 Feature 或其它 Entity 中引入 `NavigationContextExt as _`，调用 `cx.navigate("/profile")?`。Window 不进入 Sidebar 或标签。

## 专用单例与 Sidebar 插槽

- 默认设置窗口已经存在。自定义时只声明一个 `#[derive(nexora::SettingsWindow)]` 并实现 `WindowElement`；ID 与路径固定为 `settings`、`/settings`，不要再声明普通 `/settings` Window。
- Account 默认登录页已经存在。自定义时只声明一个 `#[derive(nexora::LoginFeature)]` 并直接实现 GPUI `Render`；它没有路径，不进入导航或标签。
- `#[derive(nexora::SidebarHeader)]` 与 `SidebarFooter` 的类型直接实现 GPUI `Render`，可选相同 factory。每个插槽最多一个，不修改框架 Shell。
- 多个专用实现会在 `AppRegistry::discover/build` 返回结构化重复错误，不按链接顺序任选一个。

## 组件与验证

- 优先使用 `gpui-component` 现有组件和主题 token，不重复实现 Sidebar、Settings、Tabs、Dialog、Menu 或 Table。
- 纯路由和元数据使用普通集成测试；涉及 App、Window、Entity、Global 或调度时使用 `#[gpui::test]`。
- 覆盖正常路由、动态参数错误、重复路径和生命周期；Settings Window 还要验证重复打开复用同一原生窗口。

```bash
cargo fmt --all
cargo check --workspace --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
nexora lint --workspace . --deny-warnings
```

最后确认模块已进入编译、没有手写注册表、没有 `mod.rs`、公开 Rust API 具有中文 rustdoc，并且页面内容没有被框架擅自包裹成固定 Panel 样式。
