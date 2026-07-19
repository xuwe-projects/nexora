---
name: develop-nexora-apps
description: 使用 Nexora 设计、实现或审查 Rust 桌面全栈应用。适用于 Feature 页面、独立 Window、Account 登录页、Settings 窗口、Sidebar Header/Footer、强类型路由与配置、Account Axum 路由组合、Application 启动和 nexora CLI 工作流。
---

# 开发 Nexora 应用

## 先确认边界

- 先读取项目 `Cargo.toml` 和已有模块，确认是 single 还是 workspace 布局。
- 按端启用最小 Cargo feature：桌面端统一使用 `desktop,derive`，服务端统一使用 `server,derive`。Account 能力已经分别收进这两个端级 feature，不再由应用组合内部 feature。
- 让声明 Feature、Window 和专用单例的模块进入编译。Nexora 会自动发现注册项，不要再手写一份路由表。
- 把业务内容留在应用中；不要复制框架 Shell、登录流程、设置窗口调度或 Account Router。

## 实现页面和窗口

导航页面使用 `#[derive(nexora::Feature)]`，并实现 `FeatureElement`。`render` 返回的就是完整 Panel；长期 Entity、订阅和任务放到 `initialize` 等生命周期中。

```rust
use gpui::{Context, IntoElement, Window, div, prelude::*};
use nexora::{FeatureContextExt as _, FeatureElement};

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

- 静态可导航 Feature 设置 `title`、`path` 和可选的 `section/icon/order/group`。Feature 只表示
  页面；`group` 只能引用同 section 的 `NavigationGroup`，不再允许 Feature 充当父目录。
- `section` 是顶层大类；`#[derive(nexora::NavigationGroup)]` 声明没有 path/page/tab 的纯目录，
  它可用 `parent = "另一个目录 ID"` 递归嵌套。目录点击只展开/收起，叶子 Feature 才导航；
  不得用空白 Feature、占位页或第二套路由表冒充分组。
- 普通页面保持默认外层滚动；虚拟列表、编辑器或 DataTable 已自行管理滚动视口时，设置 `content_scrollable = false`，避免 Shell 产生双层滚动。
- 标准 CRUD 业务 Panel（标题/描述、刷新、可选筛选/操作区、表格/列表主体）默认使用
  `nexora::desktop::{CrudPanel, CrudPanelToolbar}`。顶部刷新只负责重新拉取当前数据；查询、创建、
  导入、导出和批量操作放入工具栏 action 区；没有筛选和操作时不要渲染第二块卡片。
- 标准 CRUD 表格优先使用 `#[derive(nexora::CrudTableRow)]` 标记行数据结构，并交给
  `nexora::desktop::CrudTableDelegate<T>` 接入 `gpui-component` 的 `DataTable`。字段属性只描述
  `Column` 能力、表头/正文对齐和自定义渲染；操作列通过 delegate 的 `action_column` 追加，
  复杂表格仍可直接手写原生 `TableDelegate`。
- CRUD DataTable 表头默认在 `TableDelegate::render_th` 中使用
  `nexora::desktop::TableHeaderCell`，让标题水平、垂直居中；需要按列语义覆盖时使用
  `.left()`、`.center()`、`.right()` 或完全自定义表头元素。
- CRUD DataTable 正文单元格优先在 `TableDelegate::render_td` 中使用
  `nexora::desktop::TableCell`。默认垂直居中、水平靠左；需要按列语义覆盖时使用
  `.left()`、`.center()`、`.right()` 和 `.top()`、`.middle()`、`.bottom()`。表格网格线优先使用
  `DataTable::bordered(true)` 等原生样式，不在单元格组件中重复实现。
- 主窗口顶部 Feature 标签默认使用 gpui-component 官方 `TabBar::segmented()` 样式；应用需要
  改成 underline、pill 或 outline 时，通过 `ApplicationOptions::tab_style(ApplicationTabStyle::...)`
  选择，不要重写 Shell 标签栏。
- 需要覆盖内容区与 Panel Header、但保留 Sidebar 和窗口 TitleBar 的对话框时，实现 `FeatureElement::panel_overlay`。浮层必须是在 `initialize` 中创建并长期持有的 Entity，hook 始终返回同一个 `AnyView`；显示、隐藏和内容变化由浮层 Entity 自己管理，不要根据 Feature 临时状态在 `Some` 与 `None` 之间切换。
- 带 `:name` 的动态路径必须声明 `path_params = T` 并设置 `navigation = false`。查询字段用 `query_params = Q`；`T` 和 `Q` 均通过 `serde::Deserialize` 校验。
- 在 `FeatureElement` 中用 `FeatureContextExt::path/query`，用 `NavigationContextExt::navigate` 打开 Feature 或 Window。不要另设字符串参数通道。
- 需要构造子 Entity 时使用 `#[nexora(factory = Type::new)]`，否则让类型实现 `Default`。
- 保持 Feature 轻量：它只协调路由状态、生命周期与顶层布局。列表、筛选、创建、编辑、详情
  和确认弹层等独立区域放入 `features/<name>/components/*.rs`，不要让单个 Feature 承担完整
  CRUD 页面。
- 页面私有轻量组件使用 `#[derive(IntoElement)] + RenderOnce`；有输入状态、异步请求或订阅
  的组件使用独立 Entity 并实现 `Render`，由 Feature 在 `initialize` 创建后组合。禁止通过拆
  文件但继续把全部状态和 handler 留在 Feature 的方式制造假组件化。
- 创建和编辑资源默认使用 `nexora::desktop::{FormDialog, FormDialogState}`。对话框 Entity 在
  `initialize` 创建并由 `panel_overlay` 稳定返回，只遮罩当前 Feature Panel；输入变化写入
  字段草稿，默认取消自动确认未保存内容，业务必须提供 `on_submit`。非表单确认或工厂选择
  Popover 不强制使用 FormDialog。

独立原生窗口使用 `#[derive(nexora::Window)]` 和 `WindowElement`。它支持同样的 `path_params/query_params/factory`，不进入主导航或标签；可覆盖 `window_options`、`initialize` 和 `closing`。

## 自定义专用单例

Account 桌面端默认提供登录页。只有确实需要自定义时才声明一个 `LoginFeature`：

```rust
use gpui::{Context, IntoElement, Render, Window, div, prelude::*};
use nexora::desktop;

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
        let snapshot = desktop::login_snapshot(cx);
        let status = self
            .error
            .clone()
            .unwrap_or_else(|| snapshot.status.to_string());
        div()
            .child(status)
            .on_click(cx.listener(|this, _, _, cx| {
                this.error = desktop::start_login(cx)
                    .err()
                    .map(|error| error.to_string());
                cx.notify();
            }))
    }
}
```

- `LoginFeature` 由桌面端 `desktop` feature 提供，并直接使用 GPUI `Render`；未声明时使用框架默认页。
- 主窗口 Shell 已统一渲染透明 TitleBar。自定义登录页直接返回内容即可；复用 `ui::LoginGate` 时调用 `.title_bar(false)`，避免重复标题栏。
- 一个应用最多声明一个自定义 `LoginFeature`，重复声明应当修正，而不是依赖注册顺序。
- 默认登录页从 `ApplicationOptions` 读取 `application_name`、`application_version` 和可选
  `ApplicationLogo`，并把应用名用于顶部品牌、登录按钮与认证保护说明。仅替换 Logo 时使用
  `ApplicationLogo::png(include_bytes!(...))`；只有需要重做完整布局时才覆盖 `LoginFeature`。
- 登录失败由 Account 运行时推送 `Notification`；服务端返回 `request_id` 时通知提供复制
  Action。自定义登录页仍可从 `login_snapshot().failure` 读取结构化失败信息。
- 未登录时 Shell 不创建业务 Feature，并拒绝打开普通业务 Window；固定的 `/settings` 仍可用于修正认证配置。退出会清空 Feature 缓存、Sidebar 插槽和已打开的业务 Window。
- 生成的 Account workspace 已在 `Application::initialize` 中调用 `nexora::desktop::install_authenticator(authenticator, cx)`。手写入口也必须先从根 Settings 构造 `client_config` 和 `AccountAuthenticator`，再安装一次。
- 框架根据是否安装 `AccountAuthenticator` 自动启用登录门禁和默认 Account 页面；不要再在 `ApplicationOptions` 中增加重复的 `account_enabled` 布尔开关。
- Account 桌面运行时入口都在 `nexora::desktop`：`login_snapshot(&App) -> AccountLoginSnapshot`、`start_login(&mut App) -> Result<(), AccountLoginRuntimeError>`、`login_profile(&App) -> Option<&AccessProfileResponse>`、`login_session(&App) -> Option<&OidcSession>`、`sign_out(&mut App)`。登录流程由框架在后台完成，点击回调只发起它并处理同步错误。
- 不需要自定义构造时派生 `Default`；否则使用 `#[nexora(factory = AppLogin::new)]`，构造函数签名为 `fn new(&mut Window, &mut Context<Self>) -> Self`。
- 多个 Login 覆盖在 `AppRegistry::discover/build` 时返回 `RegistryError::DuplicateLoginFeature`，不会按链接顺序任选一个。
- Account 客户端默认注入 `/users` 与 `/roles` 管理 Feature，并放在“访问控制” section。
  用户页通过服务端 ZITADEL gRPC 创建人类用户并自动绑定本地账号、初始角色、状态和直接角色
  管理；`GET /me` 会从身份目录同步最新用户名与资料。角色页支持自定义角色
  创建、元数据编辑、完整权限替换和删除。应用声明相同 ID 或路径的普通 `Feature` 即可逐页
  替换默认实现，不需要新的专用派生宏。

Nexora 桌面端也默认提供 `/settings` 设置窗口。需要替换完整设置体验时，只声明一个 `#[derive(nexora::SettingsWindow)]` 并实现现有的 `WindowElement`：

```rust
#[derive(Default, nexora::SettingsWindow)]
struct AppSettings;

impl nexora::WindowElement for AppSettings {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}
```

派生宏会固定 `settings` ID 与 `/settings` 路径，并生成转发到 `WindowElement::render` 的 GPUI `Render`。不要同时再注册一个普通 `/settings` Window；多个专用覆盖会返回 `RegistryError::DuplicateSettingsWindow`。

- 可选 factory 与 Login 一致：`fn new(&mut Window, &mut Context<Self>) -> Self`。
- `WindowElement` 生命周期签名为 `window_options(&WindowRoute<Self::Path, Self::Query>, &App) -> WindowOptions`、`initialize(&mut self, &mut Window, &mut Context<Self>)` 和 `closing(&mut self, &mut Window, &mut App)`。
- 在 Feature 或其它 Entity 的 `Context` 中引入 `NavigationContextExt as _`，调用 `cx.navigate("/settings")?`；它返回 `Result<(), NavigationRequestError>`，并由 Shell 延迟打开独立窗口。

Sidebar Header 与 Footer 也采用自动发现，类型直接实现 GPUI `Render`：

```rust
use gpui::prelude::*;

#[derive(Default, nexora::SidebarFooter)]
struct AppSidebarFooter;

impl gpui::Render for AppSidebarFooter {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().child("当前账号")
    }
}
```

每个应用最多各声明一个 `SidebarHeader` 和 `SidebarFooter`；可选 factory 仍使用 `fn new(&mut Window, &mut Context<Self>) -> Self`。不要为插槽修改框架 Shell。
自定义类型只提供插槽内容；Shell 只保留 Header/Footer 结构边界、间距和分隔线，不向自定义
内容注入 hover、selected、圆角、cursor 或点击语义。品牌与应用 Context 需要独立命中区域时
使用 `nexora::desktop::SidebarRegion::new("稳定唯一 ID")`，由每个区域自行声明 hover 和点击，
不要用负 margin 或背景覆盖抵消 Shell。

## 加载配置和 Account

根配置应同时派生 serde 与 Nexora Settings：

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api: nexora::desktop::ApiSettings,
    #[nexora(account_client)]
    account: nexora::desktop::AccountSettings,
}

let settings: Settings = nexora::config::initialize(None)?;
```

手写 Account 桌面入口在调用 `run` 前完成可能失败的构造，再在同步 `initialize` 生命周期
中安装已经创建的协调器；`install_authenticator` 返回 `()`：

```rust
use nexora::desktop;

let config = desktop::client_config(&settings, &settings.api)?;
let authenticator = desktop::AccountAuthenticator::new(&config)?;

struct DesktopApplication {
    authenticator: desktop::AccountAuthenticator,
}

impl nexora::Application for DesktopApplication {
    fn initialize(&mut self, cx: &mut gpui::App) {
        desktop::install_authenticator(self.authenticator.clone(), cx);
    }
}
```

- `initialize(None)` 依次尝试进程第一个参数、当前目录及 package 清单目录祖先中的 `config/<package>.toml`；显式路径使用 `initialize(Some(path))`。
- 桌面端标记 `#[nexora(account_client)]`，服务端标记 `#[nexora(account_server)]`；不要在一个根配置中混用两端配置。
- 应用自行创建并持有唯一 `PgPool`；`Server::new()` 不连接数据库。先取得
  `nexora::server::migrations()`，与应用迁移合并并拒绝跨来源版本冲突，再构造唯一 SQLx
  `Migrator` 执行一次。随后调用 `server.initialize(&settings, &pool, setup_secret).await?`
  装配 Account/ZITADEL；该初始化不执行迁移。应用再用
  `Router::new().merge(server.routers()).merge(application_routers).with_state(application_state)`
  组合最终 Router，自行创建 `TcpListener` 并调用 `axum::serve(listener, app)`。`Server` 只
  装配框架模块，不接管监听、TLS、日志或关闭策略。
- 生成模板直接使用 `PgPool` 作为最小 Axum State；业务依赖更多时由应用定义自己的可克隆
  State，`server.routers()` 会适配该 State。业务 Router 需要 Account extractor 时，在
  `initialize` 成功后用 `server.account()` 取得 Account 句柄，放入最终 State，并实现
  `FromRef<AppState> for Account`；随后直接使用 `AuthenticatedUser` 或 `Authorized<P>`。
  系统尚未初始化时，应用可在监听成功后使用
  `server.setup_url(listener.local_addr()?)` 输出 `/setup` URL。
- 默认初始化页面使用 `nexora::server::setup_routes`；需要自定义请求字段与响应时实现 `nexora::server::Setup`，再调用 `setup_routes_with`。关联请求类型必须分别实现 `SetupUnlockRequest` 与 `SetupCompletionRequest`，框架固定校验 secret、一次性 token 和超级管理员 identity ID。
- 可信宿主通过 `nexora::server::{create_user, create_user_with_roles, create_permissions, create_role, replace_role_permissions, replace_user_roles}` 和唯一 `PgPool` 管理 Account 数据；两个用户创建函数都接收已确认的 `ExternalIdentity`，不创建本地密码。`create_user_with_roles` 还接收初始业务角色与本地 `granted_by` 用户 ID，并原子写入用户及角色关系。从 HTTP 调用这些管理函数前必须自行完成授权。
- SQLx 会让空数据库执行全部迁移，并根据 `_sqlx_migrations` 只升级待执行版本；不要增加需要人工切换的首次初始化布尔配置。

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
