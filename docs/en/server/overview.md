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

`Server` uses the application's PostgreSQL pool to run pending migrations, initializes OIDC,
Account and ZITADEL, and prepares Setup routes.

`Server::new` owns neither a pool, listener, nor Axum State. The application owns connection policy,
listen address, TLS, logging, and shutdown behavior. `server.initialize(..., &pool, ...)` borrows the
pool during framework initialization, while `server.migrate(&pool)` is also available for
upgrade-only workflows. `server.routers()` returns only Nexora's routes and adapts to the
application's own Axum State, so the application owns merge order and middleware boundaries. Omit
`.merge(server.routers())` when Nexora/Account HTTP routes are not needed.

After binding, `server.setup_url(listener.local_addr()?)` can be used to log the pending Setup URL.
It only formats the already-bound address and never owns the listener or service lifecycle.

Import server configuration and setup extension points from `nexora::server`, including
`AccountSettings`, `Setup`, `SetupUnlockRequest`, `SetupCompletionRequest`, and
`setup_routes_with`. The default setup flow validates the secret, lists human ZITADEL users,
selects the super administrator, and enforces a one-time token. Custom implementations only replace
request field mapping and the `IntoResponse` presentation layer.
