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
use gpui::{Context, IntoElement, Window, div, prelude::*};
use nexora::{FeatureContextExt as _, FeatureElement};

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

## 控制 Feature 体积

Feature 只负责路由页面的状态协调、生命周期和顶层布局，不要把列表、筛选、创建、编辑、
详情、删除确认等完整交互全部堆进一个 `users.rs`。当一个区域具有独立数据、表单状态、事件
或可以单独命名时，立即拆成页面私有组件：

```text
src/features.rs
src/features/users.rs
src/features/users/components.rs
src/features/users/components/create.rs
src/features/users/components/update.rs
src/features/users/components/table.rs
```

禁止为此创建 `mod.rs`。`users.rs` 声明 `mod components;`，`components.rs` 再声明并按需
`pub(super) use` 各组件。只有被多个 Feature 复用的组件才上移到 `src/components`。

无长期状态的页面私有组件优先实现可直接传给 `.child(...)` 的 `IntoElement`：

```rust
use gpui::{App, IntoElement, RenderOnce, Window, div};

#[derive(IntoElement)]
pub(super) struct CreateUser {
    title: &'static str,
}

impl CreateUser {
    pub(super) fn new() -> Self {
        Self { title: "创建用户" }
    }
}

impl RenderOnce for CreateUser {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div().child(self.title)
    }
}
```

- `#[derive(IntoElement)] + RenderOnce` 用于由 props 与回调即可渲染的一次性组件。
- 创建/编辑表单若有输入状态、异步请求或订阅，使用独立 `Entity<Component>` 并让组件实现
  `Render`；Feature 在 `initialize` 中创建并保存 Entity，`render` 只把它作为子元素组合。
- 组件通过回调、事件或共享 Entity 与 Feature 通信；不要为了拆文件复制业务状态，也不要让
  Feature 继续包含组件的全部字段和处理函数。
- 审查页面时如果 Feature 的 `render` 含多个可命名区域，或同一文件同时承担列表与 CRUD
  表单，先拆组件再继续添加功能；不要用机械行数作为唯一标准。

## 创建普通独立 Window

```rust
use gpui::prelude::*;

#[derive(Default, nexora::Window)]
#[nexora(title = "个人资料", path = "/profile", icon = "user")]
struct ProfileWindow;

impl nexora::WindowElement for ProfileWindow {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().child("个人资料")
    }
}
```

Window 支持与 Feature 相同的 `path_params/query_params/factory`。应用可覆盖 `window_options`、`initialize` 与 `closing`；Nexora 负责强类型绑定、`gpui_component::Root`、主题挂载和原生开窗，不要再手写 `cx.open_window` 或窗口 Global。

在 Feature 或其它 Entity 中引入 `NavigationContextExt as _`，调用 `cx.navigate("/profile")?`。Window 不进入 Sidebar 或标签。

## 专用单例与 Sidebar 插槽

- 默认设置窗口已经存在。自定义时只声明一个 `#[derive(nexora::SettingsWindow)]` 并实现 `WindowElement`；ID 与路径固定为 `settings`、`/settings`，不要再声明普通 `/settings` Window。
- Account 默认登录页已经存在。自定义时只声明一个 `#[derive(nexora::LoginFeature)]` 并直接实现 GPUI `Render`；它没有路径，不进入导航或标签。
- `#[derive(nexora::SidebarHeader)]` 与 `SidebarFooter` 的类型直接实现 GPUI `Render`，可选相同 factory。每个插槽最多一个，不修改框架 Shell。
- Sidebar 插槽只返回内容；Shell 始终用标准 `SidebarHeader`/`SidebarFooter` 外壳包裹它，
  保留 hover、内边距以及 Header 下方/Footer 上方分隔线。自定义内容不要复制这些外壳样式。
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
