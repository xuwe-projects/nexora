---
title: Server 与 Router
order: 1
---

# Server 与 Router

相关参考：

- [HTTP API 完整参考](./http-api)：逐接口说明路径、权限、入参、出参、错误码和示例；
- [Rust 服务端 API 完整参考](./rust-api)：说明 `nexora::server` 的生命周期、facade、
  pool-first 函数、extractor 与 Setup 扩展点；
- [OpenAPI 3.1](../openapi.yaml)：机器可读 HTTP 契约。

生成的服务端入口只负责加载配置和组合业务 Router：

```rust
use axum::Router;
use nexora::Server;
use sqlx::postgres::PgPoolOptions;

let pool = PgPoolOptions::new()
    .max_connections(settings.database.max_connections)
    .connect(settings.database.url.as_str())
    .await?;
let migrations = nexora::server::migrations();
sqlx::migrate::Migrator::with_migrations(migrations)
    .run(&pool)
    .await?;
let mut server = Server::new();
server
    .initialize(&settings, &pool, settings.setup.secret()?)
    .await?;

let app = Router::new()
    .merge(server.routers())
    .merge(routes::routers())
    .with_state(pool);
let listener =
    tokio::net::TcpListener::bind((settings.server.ip, settings.server.port)).await?;
axum::serve(listener, app).await?;
```

`Server` 默认完成：

1. 初始化 OIDC、Account 与 ZITADEL 目录；
2. 构建可由应用选择合并的默认 Setup 与 Account Router。

`Server::new` 不创建或持有连接池、监听器和 Axum State；连接方式、连接数、监听地址、
TLS、日志与关闭策略均由应用决定。
`nexora::server::migrations()` 返回全部框架迁移；应用必须先将其与业务迁移合并，拒绝跨
来源版本冲突，再构造唯一的 SQLx `Migrator` 执行一次。`server.initialize(..., &pool, ...)`
随后只装配框架模块，不再隐式迁移数据库。`server.routers()` 只返回 Nexora 自带路由，且
能适配应用自己的 Axum State；顶层组合顺序和中间件仍由应用决定。不需要 Nexora/Account
HTTP 路由时，不调用 `.merge(server.routers())` 即可。

可信宿主还可以直接使用 `nexora::server::{create_user, create_user_with_roles,
create_permissions, create_role, replace_role_permissions, replace_user_roles}` 管理 Account
表。所有函数接收宿主唯一 `&PgPool`；创建用户使用已确认的 `ExternalIdentity`，不会引入
本地密码模型。`create_user_with_roles` 额外接收初始业务角色和本地 `granted_by` 用户 ID，
在同一事务中创建用户、保留内置 `member` 角色并写入角色关联。两个 `replace_*` 函数表示
原子替换完整关联集合，而不是增量追加。这些 pool-first API 不执行当前请求授权，只能在已经
完成认证授权的可信宿主边界调用。

## 在应用 State 中复用 Account

业务 Router 需要复用 Nexora 认证授权时，在 `initialize` 成功后调用 `server.account()`，把
返回的句柄放入最终 State，并实现 `FromRef<AppState> for Account`：

```rust
use axum::extract::FromRef;
use nexora::server::{Account, Authorized, PermissionKey, RequiredPermission};
use sqlx::PgPool;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    account: Account,
}

impl FromRef<AppState> for Account {
    fn from_ref(state: &AppState) -> Self {
        state.account.clone()
    }
}

struct ReadFactories;

impl RequiredPermission for ReadFactories {
    const KEY: PermissionKey = PermissionKey::from_static("factories:read");
}

async fn list_factories(authorization: Authorized<ReadFactories>) {
    let current_user_id = authorization.profile().user.id.as_str();
    // 使用 current_user_id 写入业务审计字段。
}

server
    .initialize(&settings, &pool, settings.setup.secret()?)
    .await?;
let account = server.account().expect("Server 已完成初始化");
let state = AppState {
    pool: pool.clone(),
    account,
};

let app = Router::new()
    .merge(server.routers())
    .merge(application_routes())
    .with_state(state);
```

`Server::account()` 在初始化前返回 `None`；克隆 Account 句柄仍只复用同一个连接池。自定义
handler 可以直接提取 `AuthenticatedUser`，或使用 `Authorized<P>` 声明权限。两者都会复用
框架的 bearer token 校验、本地用户状态和权限合并规则，不向业务代码暴露 token。

默认 `POST /users` 在 `role_ids` 为空时只要求 `users:provision`；非空时还要求
`users:roles.write`。请求可以携带可选 `username` 作为身份提供方登录用户名；稳定认证绑定、
冲突判断与登录查询仍只使用 `identity_id`。可信宿主直接调用 pool-first API 时必须自行执行
等价授权。

初始化后可用 `server.setup_url(listener.local_addr()?)` 判断是否需要输出 Setup 提示；它只
根据已经绑定的地址生成 URL，不接管监听器或服务生命周期。

服务端配置与初始化扩展统一从 `nexora::server` 导入，例如 `AccountSettings`、`Setup`、
`SetupUnlockRequest`、`SetupCompletionRequest` 和 `setup_routes_with`。默认 Setup 负责 secret
校验、从 ZITADEL 获取人类用户、选择超级管理员与一次性 token；自定义实现只替换请求
字段映射和 `IntoResponse` 表现层，不能绕过这些初始化约束。
