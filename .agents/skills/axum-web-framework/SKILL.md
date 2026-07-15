---
name: axum-web-framework
description: 使用 Axum 0.8、Tokio 与 Tower 在 Xuwe workspace 中实现或审查 HTTP API。适用于 Router/handler、extractor、中间件、认证授权、错误响应、应用状态、优雅关闭和集成测试，并遵循 Router/handlers、Application/Store 与 composition root 的项目分层。
---

# Xuwe Axum 开发

## 先遵守项目边界

在实现 HTTP 能力前先检查现有目录和依赖，不要另起一套架构。

| 职责 | 位置 | 约束 |
| --- | --- | --- |
| 可复用请求/响应契约 | `crates/contracts` | 不依赖 Axum、SQLx 或宿主程序 |
| HTTP 路由、handler、extractor、错误映射 | `crates/api` | 表达 HTTP 协议并调用 application；健康检查可读取通用 Database |
| 通用数据库连接池 | `crates/database` | 连接、健康检查和数据库错误上下文 |
| 迁移执行与 SQL 文件 | `crates/migrate` | 迁移集中在 `crates/migrate/migrations` |
| 业务模型、用例和持久化端口 | `modules/<business>` | 使用 `Application` 与 `Store` 命名 |
| 宿主启动与依赖装配 | `apps/server` | 唯一 composition root |

保持以下依赖方向：

```text
apps/server -> crates/api -> modules/<business>
     |             |                 |
     |             +-> database      +-> Store trait
     +-> database + migrate           -> store/postgres.rs (SQLx)
```

- 在 `router/*.rs` 声明路径和方法，在 `handlers/*.rs` 处理 HTTP。
- 在 `<Business>Application` 编排业务用例，在 `<Business>Store` 抽象持久化。
- 业务 SQL 只在业务模块的 `store` 边界执行；API state 可以为 `/health` 持有通用
  `Database`，但业务 handler 不直接访问连接池或写 SQL。
- 不新增 Java 风格的 `Repository`/`Service` 层。项目统一使用 `Application`/`Store`。
- 业务 crate 不初始化路由、连接池、配置或外部 Provider。

## 使用 composition root 装配

让 `apps/server` 显式构造依赖，再把完整状态传给 API。路由构建必须保持同步、无 I/O。

```rust
pub async fn initialize(config: &ServerConfig) -> Result<AppState, BootstrapError> {
    let database = Database::connect(
        config.database.url.as_str(),
        config.database.max_connections,
    )
    .await?;

    migrate::run(database.pool()).await?;

    let token_verifier = Arc::new(OidcAccessTokenVerifier::discover(
        config.oidc.issuer_url.as_str(),
        config.oidc.audience.clone(),
    ).await?);
    let accounts_store = Arc::new(PostgresAccountsStore::new(database.pool().clone()));
    let accounts = AccountApplication::new(
        accounts_store,
        config.authorization.bootstrap_admin_subjects.clone(),
    );

    Ok(AppState::new(database, accounts, token_verifier))
}
```

新增业务模块时重复同一装配步骤：构造 store、构造 application、加入 `AppState`、合并路由。不要引入通用 `Module` trait 或隐式注册表，除非多个真实模块已经证明需要相同生命周期协议。

## 组织 Router 与 handlers

项目默认直接从资源根路径提供 API，不增加 `/api/v1` 前缀：

```rust
// crates/api/src/router/accounts.rs
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(handlers::accounts::users::list))
        .route(
            "/users/{user_id}",
            get(handlers::accounts::users::get)
                .patch(handlers::accounts::users::update),
        )
}
```

Axum 0.8 路径参数使用 `{user_id}`，不要使用旧式 `:user_id`。除非项目明确决定公开新版本且有迁移方案，否则不要添加 `/api/v1`。

让 handler 保持薄：提取输入、调用 application、映射输出。不要在 handler 中构造 store、管理事务或实现可复用业务规则。

```rust
pub async fn get_user(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AppState>,
    ApiPath(user_id): ApiPath<Uuid>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    let profile = state.accounts().user_access(user_id).await?;
    Ok(Json(access_profile_response(profile)))
}
```

## 正确使用 extractor

- 路径参数用 `Path<T>`，查询参数用 `Query<T>`，JSON 正文用 `Json<T>`，共享状态用 `State<T>`。
- extractor 从左到右执行；会消费 body 的 extractor 放在最后，一个 handler 只放一个 body consumer。
- 认证、租户、请求上下文等只读取 request parts 的能力实现 `FromRequestParts`。
- Axum 0.8 可直接在 trait impl 中写原生 `async fn`，不要为 extractor 添加 `async_trait`。
- rejection 统一转换为 `ApiError`，不要让内置 rejection 泄露出多套错误格式。

```rust
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
};

pub struct AuthenticatedUser {
    profile: AccessProfile,
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        let identity = state.token_verifier().verify(token).await?;
        let profile = state
            .accounts()
            .authenticate(&ExternalIdentity {
                issuer: identity.issuer,
                subject: identity.subject,
                email: identity.email,
                display_name: identity.display_name,
                avatar_url: identity.avatar_url,
            })
            .await?;
        Ok(Self { profile })
    }
}
```

## 管理状态与并发

- `AppState` 只保存已构造完成且可克隆的依赖，不读取配置或执行启动 I/O。
- `PgPool` 本身可廉价克隆，不需要再包一层 `Arc<PgPool>`。
- trait object 或 application 需要共享所有权时使用 `Arc`，但不要无条件给每个字段套 `Arc<Mutex<_>>`。
- 不使用全局静态连接池，也不要在同步初始化中调用 `Handle::block_on`。
- CPU 密集或阻塞库调用使用 `tokio::task::spawn_blocking`；数据库和网络 I/O 保持异步。

## 统一响应与错误

- 成功响应使用明确状态码；创建资源返回 `201 Created` 和 `Location`，删除通常返回 `204 No Content`。
- `ApiError` 实现 `IntoResponse`，集中映射稳定错误码、用户可读消息和 request ID。
- `401` 表示未认证，`403` 表示身份有效但无权限，`404` 表示资源不存在，`409` 表示状态冲突，`422` 表示字段校验失败。
- 对外响应不包含 SQL、堆栈、内部路径、令牌或底层 Provider 细节；完整 source chain 写入 tracing 日志。
- handler 返回具体 `Result<T, ApiError>`，不要用 `anyhow::Error` 作为公开 HTTP 错误。

## 中间件与 tracing

在顶层 Router 一次性挂载横切能力。连续调用 `Router::layer` 时，后添加的 layer 先看到请求；
`ServiceBuilder` 则按添加顺序执行，不要把两者混为一谈。

```rust
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(accounts::routes())
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(TraceLayer::new_for_http().make_span_with(http_request_span))
        .layer(SetSensitiveRequestHeadersLayer::new([AUTHORIZATION]))
        .layer(middleware::from_fn(request_id::assign))
        .with_state(state)
}
```

- 使用 `tracing`，不要用 `println!`/`eprintln!` 记录服务状态。
- 请求 span 至少记录 method、URI 或 matched path、request ID、状态码和耗时。
- 将 `Authorization`、Cookie 和其他凭据标记为敏感，不记录 token 或请求正文。
- `DefaultBodyLimit` 约束 `Json`、`Bytes` 等兼容 extractor；直接读取 `Body` 或使用不遵守该
  默认值的第三方 extractor 时，使用 `RequestBodyLimitLayer` 才能保证全局字节上限。
- 为请求超时、并发和上传设置边界；认证接口按部署环境增加限流。
- 生产流量使用 HTTPS；代理终止 TLS 时明确可信代理边界。

## 认证与授权

按顺序处理：验证凭据、同步/加载本地身份、检查账号状态、加载角色权限、执行权限判断。

- token verifier 只负责验证 issuer、audience、签名、有效期等认证事实。
- `AccountApplication` 负责本地用户与 RBAC 用例；权限检查不要散落成 handler 内的角色名比较。
- 使用项目既有的稳定权限字符串，例如 `users:read`；角色是权限集合，不是硬编码分支。
- 对资源级授权同时校验权限与资源归属，防止只换 ID 即越权。
- 授权被拒绝时记录主体 ID、所需权限和 request ID，但不记录原始 token。

## 优雅关闭

服务入口同时监听 Ctrl+C 与 Unix `SIGTERM`，把信号交给 `axum::serve(...).with_graceful_shutdown(...)`：

```rust
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("无法安装 Ctrl+C 信号处理器");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("无法安装 SIGTERM 信号处理器")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
```

关闭时停止接收新请求并等待在途请求完成，通过 tracing 记录开始与结束。部署要求硬性退出
期限时，由宿主额外协调超时与强制终止；不要把“无限等待”和“有期限排空”混写成同一保证。
不要在 handler 或业务 crate 中安装进程信号。

## 测试

将 API 集成测试放在对应 crate 的 `tests/` 目录，不把主要 handler 测试内联进源文件。

至少覆盖：

- 成功路径和响应契约；
- 未认证 `401`、权限不足 `403`；
- 参数校验、资源不存在、状态冲突；
- body 大小限制、fallback 与 method-not-allowed；
- 需要数据库的 store/application 行为以及事务回滚。

使用 `tower::ServiceExt::oneshot` 驱动完整 Router。测试通过显式构造的 `AppState` 注入 fake verifier 或测试 application；数据库测试使用隔离的 PostgreSQL 测试库，不在测试中连接生产配置。

```rust
#[tokio::test]
async fn unauthenticated_request_returns_401() {
    let response = test_router()
        .oneshot(Request::get("/users").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
```

## 完成检查

1. 确认新增路径没有 `/api/v1`，参数使用 Axum 0.8 的 `{id}`。
2. 确认 Router/handler、Application、Store、Postgres store 和 composition root 职责清晰。
3. 确认 SQL 只出现在数据库边界，handler 没有直接访问连接池。
4. 确认认证、授权、错误和 tracing 不泄露敏感信息。
5. 运行 `cargo fmt --all --check`、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets` 和 `cargo clippy --workspace --all-targets --all-features -- -D warnings`；如环境限制无法运行，明确说明。
