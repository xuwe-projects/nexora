# Nexora

Nexora 是一个正在形成中的 Rust 桌面全栈框架。本仓库同时包含框架 crate、GPUI 桌面端、
Axum 服务端和独立业务模块，用于共同验证框架边界与真实应用集成。

> [!WARNING]
> Nexora 目前处于 **early alpha**。公开 API、生成项目结构和兼容策略仍可能发生破坏性
> 变化，暂不建议直接用于生产环境。当前仓库内容不代表已经发布到 crates.io 的稳定版本。

## 当前能力

以下能力已经存在于源码和测试中：

- 同一个 `nexora` Cargo package 同时提供框架库和 `nexora` 命令行程序；
- 同一个 CLI 提供 `create`、`init`、`build`、`doctor` 与 workspace `lint`，不再维护
  第二个工具二进制；
- `#[derive(nexora::Feature)]` 自动注册 Feature 元数据与 Entity 工厂，
  `#[derive(nexora::Window)]` 自动注册独立窗口、强类型参数与原生窗口工厂；
- `#[derive(nexora::SidebarHeader)]` 与 `#[derive(nexora::SidebarFooter)]` 自动发现并挂载
  应用自定义的 Sidebar 顶部、底部 GPUI View；
- `#[derive(nexora::LoginFeature)]` 与 `#[derive(nexora::SettingsWindow)]` 提供最多一个应用级
  覆盖；没有覆盖时使用框架默认登录门禁和 `/settings` 设置窗口；
- `Application` 自动发现注册项、校验首路由并启动带导航和标签的通用 GPUI Shell；
- 在同一注册表中校验 Feature 与 Window 的路径、标识、父子导航关系和路由冲突；
- 匹配静态路径、`:name` 动态参数、查询字符串与 custom scheme URI；
- 使用 serde 将动态路径和 query 在 Feature 创建前绑定为强类型
  `Path<T>` 与 `Query<T>`；
- 由 `FeatureElement::render` 定义完整 Panel 内容，派生宏自动生成 GPUI 原生
  `Render` 转发；
- 在 Feature Entity 上分发 `initialize`、`activated`、`deactivated`、`route_changed` 和
  `closing`，未使用的生命周期无需覆盖；
- `#[derive(nexora::Settings)]` 驱动强类型 TOML 配置加载与框架模块配置校验；
- 按 Cargo feature 启用 Account 客户端或服务端能力；服务端只交付可组合
  Router、用户/角色/权限与认证授权用例、可重试的一次性初始化 API，不接管宿主服务。

## 最小桌面应用

应用只需让 Feature 模块进入编译，并实现最小 `Application` 契约：

```rust
mod features;

use nexora::Application as _;

struct DesktopApplication;

impl nexora::Application for DesktopApplication {}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
```

框架默认打开 `/`，创建 `900 × 640` 主窗口。需要自定义首路由、尺寸或语言时，
覆盖 `Application::options`；需要注册 Global、Action 或应用服务时，覆盖
`Application::initialize`。应用不需要自行组装 RootView。

最小的 Feature 示例：

```rust
use nexora::{
    FeatureElement,
    gpui::{Context, IntoElement, Window, div, prelude::*},
};

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "工作台",
    icon = "layout-dashboard",
    order = 0
)]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().child("Hello Nexora")
    }
}
```

`FeatureElement` 的约束是 `Feature + Sized + Render`，并要求每个页面实现没有默认值的
`render`。`#[derive(Feature)]` 会为具体页面生成 GPUI `Render` 实现并转发到该方法，
因此应用不需要维护两份渲染代码。返回值会原样成为 Panel 内容；框架不会擅自添加
内边距、卡片或滚动容器。动态路径必须声明
`path_params = T` 并设置 `navigation = false`；查询结构可通过 `query_params = Q` 声明，
随后使用 `FeatureContextExt` 从 `cx` 读取已经校验的参数，并可通过
`NavigationContextExt::navigate` 打开其他 Feature 或 Window。

未声明 `factory` 时，派生宏使用 `Default` 创建 Feature。需要在构造阶段创建子 Entity，或类型
本身无法实现 `Default` 时，可以声明 `factory = Type::new`；工厂接收 `&mut Window` 和
`&mut Context<Self>` 并返回 `Self`。

Feature 动态路径使用 `:name`，并必须关闭导航入口。例如：

```rust
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
```

在 `FeatureElement` 中引入 `FeatureContextExt` 后，可以直接使用 `cx.path()` 和
`cx.query()` 得到 `Path<UserPath>` 与 `Query<UserQuery>`；字段名、字段类型与缺失值
都交给 serde 一次性校验，没有另一套字符串参数 API。

`#[derive(nexora::Window)]` 支持与 Feature 一致的 `path_params`、`query_params` 和可选
`factory`。应用实现 `WindowElement::render` 后，框架会完成强类型参数绑定、原生窗口创建、
`gpui_component::Root` 挂载和生命周期清理。Window 不进入主导航或标签；Feature 中调用
`cx.navigate("/settings")` 会直接打开对应原生窗口。

Sidebar Header 与 Footer 不需要另一套 Nexora 渲染 trait。应用直接实现 GPUI
`Render`，派生宏只负责自动注册和 Entity 构造：

```rust
use nexora::gpui::{Context, IntoElement, Render, Window, div, prelude::*};

#[derive(Default, nexora::SidebarHeader)]
struct AppSidebarHeader;

impl Render for AppSidebarHeader {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().child("My application")
    }
}
```

每个应用最多注册一个 Header 和一个 Footer，重复定义会在注册表发现阶段失败。未声明
`factory` 时使用 `Default`；需要创建子 Entity 时可使用
`#[nexora(factory = Type::new)]`。

## 登录门禁与设置窗口

启用 `account-client` 后，Nexora Shell 会在未认证时显示框架默认登录门禁，并推迟创建
业务 Feature Entity。登录成功后才初始化当前 Feature；退出登录会关闭全部缓存页面，避免
不同用户复用上一会话的页面状态，并关闭已经打开的业务 Window；未登录时只有
`/settings` 仍允许打开。应用需要完全自定义登录体验时，可以声明且只能声明一个：

```rust
#[derive(Default, nexora::LoginFeature)]
struct AppLogin;

impl nexora::gpui::Render for AppLogin {
    fn render(
        &mut self,
        _window: &mut nexora::gpui::Window,
        cx: &mut nexora::gpui::Context<Self>,
    ) -> impl nexora::gpui::IntoElement {
        let status = nexora::account::client::login_snapshot(cx).status;
        nexora::gpui::div().child(status)
    }
}
```

登录交互使用 `nexora::account::client::start_login`，退出使用 `sign_out`。派生类型直接实现
GPUI `Render`，不会进入路由、导航或标签。多个自定义登录页会返回确定的
`DuplicateLoginFeature` 错误。

桌面端始终提供一个固定 ID 为 `settings`、路径为 `/settings` 的独立设置窗口。默认内容包含
主题预设、颜色模式、字号和组件密度；重复打开会激活已有原生窗口。应用可以用
`#[derive(nexora::SettingsWindow)]` 与
现有 `WindowElement` 完整替换它，但不能再注册普通 `/settings` Window，也不能定义多个
Settings Window 覆盖。

## 强类型配置

根配置可以包含任意应用字段：

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api_endpoint: String,
}

let settings: Settings = nexora::config::initialize(None)?;
```

`initialize::<T>` 按“显式传入的路径 → 进程的第一个命令行参数 →
`config/<CARGO_PKG_NAME>.toml`”选择配置文件，之后允许环境变量覆盖，嵌套键使用
双下划线。开启 Account 时，桌面与服务端分别使用
`#[nexora(account_client)]` 和 `#[nexora(account_server)]` 标记各自的标准配置段。
桌面 Account 配置同时包含 `api.endpoint` 与 OIDC public client 参数；
`AccountAuthenticator` 组合 Authorization Code + PKCE、loopback callback、token 刷新与
`/me` 业务门禁。框架默认 Login Feature 负责打开浏览器和展示状态；自定义 Login Feature
也可以直接复用同一套 Account 客户端运行时。

## Account 组合边界

`account-server` 不启动端口、不创建第二个数据库连接池，也不接管宿主路由。
宿主创建共享 `PgPool` 后显式组合：

```rust
let migration_options = nexora::account::server::MigrationOptions::new()
    .initialize_empty_database(false);
nexora::account::server::migrate(&pool, migration_options).await?;

let dependencies = nexora::account::server::dependencies(pool.clone(), &settings).await?;
let account = nexora::account::Account::new(dependencies);

let app = axum::Router::new()
    .merge(account.routers::<AppState>())
    .merge(application_routes)
    .with_state(app_state);
```

宿主可以通过 `account.initialization_status()` 把 Account 初始化状态组合到自己的引导
流程，然后将服务端已验证的 `ExternalIdentity` 传给 `account.initialize(...)`。
相同身份重试会幂等成功，另一个身份不能替换已绑定的超级管理员。Account
不提供强制的初始化页面，也不执行宿主自己的初始化业务。

普通 OIDC 身份不会在第一次携带 token 时自动注册。超级管理员或拥有
`users:provision` 权限的管理员必须通过 `Account::provision_user` 或 `POST /users`
显式开通 subject；开通后 `/me` 才会同步资料并允许进入系统。

一个 Nexora 部署只允许使用一个 OIDC issuer。Account 服务端依赖初始化时会把配置中的
issuer 原子绑定到 `account.system_initialization` 单例记录：尚未绑定时写入一次，已经绑定
时只能使用完全相同的值，任何更换都会拒绝启动。每次 Bearer token 验证后还会再次确认
claim 中的 issuer 与该部署值一致。issuer 因而不是用户字段，也不会出现在用户 API DTO 中；
`identity_id`（OIDC subject）在这个部署范围内唯一。

## 开始开发

需要支持 Rust 2024 edition 的稳定工具链。克隆仓库后可先验证 Nexora 核心 crate：

```bash
cargo test -p nexora -p nexora-macros
cargo run -p nexora -- --help
```

Cargo 依赖与可执行文件都叫 `nexora`。尚未从 registry 发布时，可在本仓库
使用 `cargo install --path crates/nexora --no-default-features --features cli` 安装 CLI。
直接运行 `nexora create` 时，CLI 会像
Vite 一样交互询问项目名称、single/workspace 结构以及是否启用完整 Account；所有问题都有
默认值。脚本和 CI 仍可显式传入参数：

```bash
nexora create
nexora create my-app --layout single
nexora create my-app --layout workspace
nexora init . --layout workspace
nexora create my-fullstack-app --layout workspace --features account
```

开启 `account` 脚手架时会同时生成 `apps/desktop`、`apps/server` 与可直接被默认路径发现的
桌面端/服务端配置，因此使用 workspace 布局。交互模式选择 Account 后会自动调整为
workspace；同时显式
传入 `--layout single --features account` 时会返回清晰错误。生成的桌面端会加载强类型配置并把
`AccountAuthenticator` 安装到 Nexora 登录运行时；服务端会使用共享 `PgPool` 显式执行安全迁移，再完成
`Account::new`、Router merge 和宿主自定义路由。空数据库初始化默认关闭，必须由使用方
确认目标后显式设置 `initialize_empty_database = true`。

`create` 与 `init` 还会把仓库 `.agents/skills` 中的全部开发规范写入生成项目，并附带
`develop-nexora-apps` 框架 Skill；已有同名 Skill 时初始化会整体拒绝，不会覆盖用户内容。

两种布局都由 `templates/scaffold/` 下的独立 Askama 模板生成。
Cargo 会强制排除包含子 `Cargo.toml` 的目录，因此清单模板使用语义后缀以便进入
发布包；实际生成文件仍为标准 `Cargo.toml`。

桌面端首次构建可能需要较长时间。Account 服务端的运行与数据库测试还需要
本地 PostgreSQL 和有效 OIDC 配置。

## 项目状态与路线

当前优先事项是继续验证通用 Feature/Window Shell 与 Account 组合边界，收敛操作系统
deeplink scheme 注册、公开 API 和发布兼容策略。

如果你愿意参与，请先阅读 [贡献指南](CONTRIBUTING.md) 和
[社区行为准则](CODE_OF_CONDUCT.md)。安全问题请按照 [安全策略](SECURITY.md) 私下报告。

## Workspace 现有组成

框架实现位于 `crates/` 与 `modules/`；可运行产品不再作为 `apps/` 主体，而是放在
`examples/` 中验证真实桌面端与服务端组合。

## 后台结构

```text
.
├── examples/
│   ├── console/                # GPUI 桌面端完整集成示例
│   └── server/                 # Axum、PgPool 与 Account 组合示例
├── config/
│   └── example.server.toml      # 可提交的服务端配置示例
├── crates/
│   ├── api/                     # 通用 HTTP 错误、extractor 和中间件
│   ├── configuration/           # config-rs 分层加载
│   ├── contracts/               # 跨进程 JSON 契约
│   └── migrate/                 # 全局 SQLx 迁移程序、迁移和测试数据
└── modules/
    └── account/                 # 账号模块 State、Router、handler、实体和 store
```

服务端只创建一个 `PgPool`。宿主把它和 token verifier 交给
`Account::new(AccountDependencies)`，Account 内部保存连接池的廉价克隆句柄并返回可与
`Router<AppState>` 合并的路由。只有 `examples/server` 在最外层调用一次
`.with_state(app_state)`。

依赖方向保持单向：

```text
examples/server ──> modules/account ──> crates/api + crates/contracts + SQLx
                └─> crates/configuration

crates/migrate ──> crates/configuration + SQLx
```

业务模块不依赖 `examples/server` 或服务端的 `AppState`。业务 SQL 只出现在模块的 `stores`
边界；表结构、索引、约束和必要基础数据统一位于 `crates/migrate/migrations`。

## 本地运行

先创建不会被 Git 跟踪的本地配置：

```bash
cp config/example.server.toml config/server.toml
```

首次安装空数据库时执行：

```bash
cargo run -p migrate -- --initialize-empty-database config/server.toml
cargo run -p server -- config/server.toml
```

以后发布升级只执行 `cargo run -p migrate -- config/server.toml`。普通升级命令拒绝空数据库和
迁移历史与核心表不一致的目标，不会自动清库或重新初始化。

服务端默认也会读取 `config/server.toml`，因此配置就位后可以省略路径。迁移程序与服务端都在
文件配置之后加载环境变量，嵌套字段使用双下划线，例如 `DATABASE__URL`。

## 新增业务模块

新模块使用单数 crate 名和初始化类型、复数资源路由与表名。模块至少包含
`entities`、`errors`、`handlers`、`routers`、`stores` 和模块 State；服务端只新增 workspace
依赖、构造模块并合并 `module.routers::<AppState>()`。数据库变更以全局唯一版本追加到
`crates/migrate/migrations/` 一级目录，测试数据按模块放入 `crates/migrate/seeds/<module>/`。

详细约定见 [modules/README.md](modules/README.md) 与
[crates/migrate/README.md](crates/migrate/README.md)。

## 质量检查

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p nexora -- lint --workspace . --deny-warnings
```

## 许可证

本项目采用 [Apache License 2.0](LICENSE-APACHE) 或 [MIT License](LICENSE-MIT) 双许可证，
使用者可任选其一。
