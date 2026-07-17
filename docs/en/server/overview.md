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

Trusted host code can also call `nexora::server::{create_user, create_permissions, create_role,
replace_role_permissions, replace_user_roles}` with the application's one `PgPool`. User creation
provisions an already-confirmed `ExternalIdentity` and does not add a local password model. The two
`replace_*` functions atomically replace the complete relation set rather than appending entries.

After binding, `server.setup_url(listener.local_addr()?)` can be used to log the pending Setup URL.
It only formats the already-bound address and never owns the listener or service lifecycle.

Import server configuration and setup extension points from `nexora::server`, including
`AccountSettings`, `Setup`, `SetupUnlockRequest`, `SetupCompletionRequest`, and
`setup_routes_with`. The default setup flow validates the secret, lists human ZITADEL users,
selects the super administrator, and enforces a one-time token. Custom implementations only replace
request field mapping and the `IntoResponse` presentation layer.
