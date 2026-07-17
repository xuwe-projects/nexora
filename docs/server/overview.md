---
title: Server 与 Router
order: 1
---

# Server 与 Router

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

可信宿主还可以直接使用 `nexora::server::{create_user, create_permissions, create_role,
replace_role_permissions, replace_user_roles}` 管理 Account 表。所有函数接收宿主唯一
`&PgPool`；创建用户使用已确认的 `ExternalIdentity`，不会引入本地密码模型。两个
`replace_*` 函数表示原子替换完整关联集合，而不是增量追加。

初始化后可用 `server.setup_url(listener.local_addr()?)` 判断是否需要输出 Setup 提示；它只
根据已经绑定的地址生成 URL，不接管监听器或服务生命周期。

服务端配置与初始化扩展统一从 `nexora::server` 导入，例如 `AccountSettings`、`Setup`、
`SetupUnlockRequest`、`SetupCompletionRequest` 和 `setup_routes_with`。默认 Setup 负责 secret
校验、从 ZITADEL 获取人类用户、选择超级管理员与一次性 token；自定义实现只替换请求
字段映射和 `IntoResponse` 表现层，不能绕过这些初始化约束。
