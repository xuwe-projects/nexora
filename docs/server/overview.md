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

1. 使用应用创建的 PostgreSQL 连接池执行待执行迁移；
2. 初始化 OIDC、Account 与 ZITADEL 目录；
3. 构建可由应用选择合并的默认 Setup 与 Account Router。

`Server::new` 不创建或持有连接池、监听器和 Axum State；连接方式、连接数、监听地址、
TLS、日志与关闭策略均由应用决定。
`server.initialize(..., &pool, ...)` 借用该连接池完成框架初始化，独立升级场景也可以直接
调用 `server.migrate(&pool)`。`server.routers()` 只返回 Nexora 自带路由，且能适配应用
自己的 Axum State；顶层组合顺序和中间件仍由应用决定。不需要 Nexora/Account HTTP 路由
时，不调用 `.merge(server.routers())` 即可。

初始化后可用 `server.setup_url(listener.local_addr()?)` 判断是否需要输出 Setup 提示；它只
根据已经绑定的地址生成 URL，不接管监听器或服务生命周期。

服务端配置与初始化扩展统一从 `nexora::server` 导入，例如 `AccountSettings`、`Setup`、
`SetupUnlockRequest`、`SetupCompletionRequest` 和 `setup_routes_with`。默认 Setup 负责 secret
校验、从 ZITADEL 获取人类用户、选择超级管理员与一次性 token；自定义实现只替换请求
字段映射和 `IntoResponse` 表现层，不能绕过这些初始化约束。
