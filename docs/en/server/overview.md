---
title: Server and Routers
order: 1
---

# Server and Routers

The generated server entry point only loads configuration and composes application routers:

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

`Server` initializes OIDC, Account and ZITADEL, and prepares Setup routes. It does not run
database migrations.

`Server::new` owns neither a pool, listener, nor Axum State. The application owns connection policy,
listen address, TLS, logging, and shutdown behavior. `nexora::server::migrations()` returns all
framework migrations. The application combines them with its own migrations, rejects version
collisions across sources, and runs one SQLx `Migrator` before calling
`server.initialize(..., &pool, ...)`. `server.routers()` returns only Nexora's routes and adapts to
the application's own Axum State, so the application owns merge order and middleware boundaries.
Omit `.merge(server.routers())` when Nexora/Account HTTP routes are not needed.

Trusted host code can also call `nexora::server::{create_user, create_user_with_roles,
create_permissions, create_role, replace_role_permissions, replace_user_roles}` with the
application's one `PgPool`. User creation provisions an already-confirmed `ExternalIdentity` and
does not add a local password model. `create_user_with_roles` additionally accepts initial business
roles and a local `granted_by` user ID, then creates the user, retains the built-in `member` role,
and writes role grants in one transaction. The two `replace_*` functions atomically replace the
complete relation set rather than appending entries. These pool-first APIs do not authorize the
current request and must only be called from an already-authorized trusted host boundary.

## Reuse Account from application State

When application routers need Nexora authentication or authorization, call `server.account()`
after successful initialization, store the handle in the final State, and implement
`FromRef<AppState> for Account`:

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
    // Use current_user_id for business audit columns.
}

server
    .initialize(&settings, &pool, settings.setup.secret()?)
    .await?;
let account = server.account().expect("Server has been initialized");
let state = AppState {
    pool: pool.clone(),
    account,
};

let app = Router::new()
    .merge(server.routers())
    .merge(application_routes())
    .with_state(state);
```

`Server::account()` returns `None` before initialization. Cloning the Account handle continues to
reuse the same pool. Custom handlers can extract `AuthenticatedUser` directly or declare a
permission with `Authorized<P>`. Both reuse framework bearer-token verification, local user status,
and merged permissions without exposing the token to business code.

The default `POST /users` requires only `users:provision` when `role_ids` is empty and additionally
requires `users:roles.write` when it is non-empty. Trusted hosts calling pool-first APIs must enforce
equivalent authorization themselves.

After binding, `server.setup_url(listener.local_addr()?)` can be used to log the pending Setup URL.
It only formats the already-bound address and never owns the listener or service lifecycle.

Import server configuration and setup extension points from `nexora::server`, including
`AccountSettings`, `Setup`, `SetupUnlockRequest`, `SetupCompletionRequest`, and
`setup_routes_with`. The default setup flow validates the secret, lists human ZITADEL users,
selects the super administrator, and enforces a one-time token. Custom implementations only replace
request field mapping and the `IntoResponse` presentation layer.
