---
name: develop-nexora-apps
description: 使用 Nexora 设计、实现或审查 Rust 桌面全栈应用。适用于 Feature 页面、独立 Window、Account 登录页、Settings 窗口、Sidebar Header/Footer、强类型路由与配置、Account Axum 路由组合、Application 启动和 nexora CLI 工作流。
---

# 开发 Nexora 应用

## 先确认边界

- 先读取项目 `Cargo.toml` 和已有模块，确认是 single 还是 workspace 布局。
- 按端启用最小 Cargo feature：桌面端使用 `desktop,derive`，Account 桌面端加 `account-client`，服务端使用 `account-server`；`account` 同时启用两端。
- 让声明 Feature、Window 和专用单例的模块进入编译。Nexora 会自动发现注册项，不要再手写一份路由表。
- 把业务内容留在应用中；不要复制框架 Shell、登录流程、设置窗口调度或 Account Router。

## 实现页面和窗口

导航页面使用 `#[derive(nexora::Feature)]`，并实现 `FeatureElement`。`render` 返回的就是完整 Panel；长期 Entity、订阅和任务放到 `initialize` 等生命周期中。

```rust
use nexora::{
    FeatureContextExt as _, FeatureElement,
    gpui::{Context, IntoElement, Window, div, prelude::*},
};

#[derive(Clone, serde::Deserialize)]
struct UserPath {
    id: u64,
}

#[derive(Clone, serde::Deserialize)]
struct UserQuery {
    tab: Option<String>,
}

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "用户详情",
    path = "/users/details/:id",
    path_params = UserPath,
    query_params = UserQuery,
    navigation = false
)]
struct UserDetailsFeature;

impl FeatureElement for UserDetailsFeature {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = cx.path();
        let query = cx.query();
        // 根据已校验的强类型参数构造界面。
        div().child(format!("{}: {:?}", path.id, query.tab))
    }
}
```

- 静态可导航 Feature 设置 `title`、`path` 和可选的 `section/icon/order/parent`。
- 普通页面保持默认外层滚动；虚拟列表、编辑器或 DataTable 已自行管理滚动视口时，设置 `content_scrollable = false`，避免 Shell 产生双层滚动。
- 需要覆盖内容区与 Panel Header、但保留 Sidebar 和窗口 TitleBar 的对话框时，实现 `FeatureElement::panel_overlay`。浮层必须是在 `initialize` 中创建并长期持有的 Entity，hook 始终返回同一个 `AnyView`；显示、隐藏和内容变化由浮层 Entity 自己管理，不要根据 Feature 临时状态在 `Some` 与 `None` 之间切换。
- 带 `:name` 的动态路径必须声明 `path_params = T` 并设置 `navigation = false`。查询字段用 `query_params = Q`；`T` 和 `Q` 均通过 `serde::Deserialize` 校验。
- 在 `FeatureElement` 中用 `FeatureContextExt::path/query`，用 `NavigationContextExt::navigate` 打开 Feature 或 Window。不要另设字符串参数通道。
- 需要构造子 Entity 时使用 `#[nexora(factory = Type::new)]`，否则让类型实现 `Default`。

独立原生窗口使用 `#[derive(nexora::Window)]` 和 `WindowElement`。它支持同样的 `path_params/query_params/factory`，不进入主导航或标签；可覆盖 `window_options`、`initialize` 和 `closing`。

## 自定义专用单例

Account 桌面端默认提供登录页。只有确实需要自定义时才声明一个 `LoginFeature`：

```rust
use nexora::{
    account::client,
    gpui::{Context, IntoElement, Render, Window, div, prelude::*},
};

#[derive(Default, nexora::LoginFeature)]
struct AppLogin {
    error: Option<String>,
}

impl Render for AppLogin {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let snapshot = client::login_snapshot(cx);
        let status = self
            .error
            .clone()
            .unwrap_or_else(|| snapshot.status.to_string());
        div()
            .child(status)
            .on_click(cx.listener(|this, _, _, cx| {
                this.error = client::start_login(cx)
                    .err()
                    .map(|error| error.to_string());
                cx.notify();
            }))
    }
}
```

- `LoginFeature` 只在 `account-client` 下可用，并直接使用 GPUI `Render`；未声明时使用框架默认页。
- 主窗口 Shell 已统一渲染透明 TitleBar。自定义登录页直接返回内容即可；复用 `ui::LoginGate` 时调用 `.title_bar(false)`，避免重复标题栏。
- 一个应用最多声明一个自定义 `LoginFeature`，重复声明应当修正，而不是依赖注册顺序。
- 未登录时 Shell 不创建业务 Feature，并拒绝打开普通业务 Window；固定的 `/settings` 仍可用于修正认证配置。退出会清空 Feature 缓存、Sidebar 插槽和已打开的业务 Window。
- 生成的 Account workspace 已在 `Application::initialize` 中调用 `nexora::account::client::install_authenticator(authenticator, cx)`。手写入口也必须先从根 Settings 构造 `client_config` 和 `AccountAuthenticator`，再安装一次。
- Account 运行时入口都在 `nexora::account::client`：`login_snapshot(&App) -> AccountLoginSnapshot`、`start_login(&mut App) -> Result<(), AccountLoginRuntimeError>`、`login_profile(&App) -> Option<&AccessProfileResponse>`、`login_session(&App) -> Option<&OidcSession>`、`sign_out(&mut App)`。登录流程由框架在后台完成，点击回调只发起它并处理同步错误。
- 不需要自定义构造时派生 `Default`；否则使用 `#[nexora(factory = AppLogin::new)]`，构造函数签名为 `fn new(&mut Window, &mut Context<Self>) -> Self`。
- 多个 Login 覆盖在 `AppRegistry::discover/build` 时返回 `RegistryError::DuplicateLoginFeature`，不会按链接顺序任选一个。

Nexora 桌面端也默认提供 `/settings` 设置窗口。需要替换完整设置体验时，只声明一个 `#[derive(nexora::SettingsWindow)]` 并实现现有的 `WindowElement`：

```rust
#[derive(Default, nexora::SettingsWindow)]
struct AppSettings;

impl nexora::WindowElement for AppSettings {
    fn render(
        &mut self,
        _window: &mut nexora::gpui::Window,
        _cx: &mut nexora::gpui::Context<Self>,
    ) -> impl nexora::gpui::IntoElement {
        nexora::gpui::div()
    }
}
```

派生宏会固定 `settings` ID 与 `/settings` 路径，并生成转发到 `WindowElement::render` 的 GPUI `Render`。不要同时再注册一个普通 `/settings` Window；多个专用覆盖会返回 `RegistryError::DuplicateSettingsWindow`。

- 可选 factory 与 Login 一致：`fn new(&mut Window, &mut Context<Self>) -> Self`。
- `WindowElement` 生命周期签名为 `window_options(&WindowRoute<Self::Path, Self::Query>, &App) -> WindowOptions`、`initialize(&mut self, &mut Window, &mut Context<Self>)` 和 `closing(&mut self, &mut Window, &mut App)`。
- 在 Feature 或其它 Entity 的 `Context` 中引入 `NavigationContextExt as _`，调用 `cx.navigate("/settings")?`；它返回 `Result<(), NavigationRequestError>`，并由 Shell 延迟打开独立窗口。

Sidebar Header 与 Footer 也采用自动发现，类型直接实现 GPUI `Render`：

```rust
use nexora::gpui::prelude::*;

#[derive(Default, nexora::SidebarFooter)]
struct AppSidebarFooter;

impl nexora::gpui::Render for AppSidebarFooter {
    fn render(
        &mut self,
        _window: &mut nexora::gpui::Window,
        _cx: &mut nexora::gpui::Context<Self>,
    ) -> impl nexora::gpui::IntoElement {
        nexora::gpui::div().child("当前账号")
    }
}
```

每个应用最多各声明一个 `SidebarHeader` 和 `SidebarFooter`；可选 factory 仍使用 `fn new(&mut Window, &mut Context<Self>) -> Self`。不要为插槽修改框架 Shell。

## 加载配置和 Account

根配置应同时派生 serde 与 Nexora Settings：

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api_endpoint: String,
    #[nexora(account_client)]
    account: nexora::account::client::Settings,
}

let settings: Settings = nexora::config::initialize(None)?;
```

手写 Account 桌面入口在调用 `run` 前完成可能失败的构造，再在同步 `initialize` 生命周期
中安装已经创建的协调器；`install_authenticator` 返回 `()`：

```rust
use nexora::account::client;

let config = client::client_config(&settings)?;
let authenticator = client::AccountAuthenticator::new(&config)?;

struct DesktopApplication {
    authenticator: client::AccountAuthenticator,
}

impl nexora::Application for DesktopApplication {
    fn initialize(&mut self, cx: &mut nexora::gpui::App) {
        client::install_authenticator(self.authenticator.clone(), cx);
    }
}
```

- `initialize(None)` 依次尝试进程第一个参数与 `config/<package>.toml`；显式路径使用 `initialize(Some(path))`。
- 桌面端标记 `#[nexora(account_client)]`，服务端标记 `#[nexora(account_server)]`；不要在一个根配置中混用两端配置。
- `account-server` 不启动 HTTP 服务。宿主创建共享 `PgPool`，调用 `account::server::migrate`、`dependencies` 和 `Account::new`，再将 `account.routers::<AppState>()` merge 到自己的 Axum Router。
- Account 初始化应与宿主引导流程组合：查询 `initialization_status`，再调用 `initialize(nexora::account::AccountInitialization { super_admin: identity })`，其中 `identity` 是宿主已经验证的 `ExternalIdentity`。不要让 Account 替宿主决定页面或业务初始化顺序。

## 使用 CLI 和验证

```text
nexora create my-app --layout single
nexora create my-app --layout workspace --features account
nexora init . --layout workspace
nexora doctor
nexora lint --workspace . --deny-warnings
```

- Account 需要桌面端和服务端，因此只用 workspace 布局。
- 先运行受影响 package 的 `cargo fmt`、`cargo check` 和 `cargo test`，再运行 `nexora lint --workspace . --deny-warnings`。
- 修改路由或注册项时，同时验证重复 ID、重复路径、动态参数与专用单例重复声明的失败路径。
