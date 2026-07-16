mod config;
mod routes;

use axum::Router;
use nexora::account::{Account, server::MigrationOptions};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

#[derive(Clone)]
pub(crate) struct AppState {
    pool: PgPool,
}

impl AppState {
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings: config::Settings = nexora::config::initialize(None)?;
    let pool = PgPoolOptions::new()
        .max_connections(settings.database.max_connections)
        .connect(settings.database.url.as_str())
        .await?;
    nexora::account::server::migrate(
        &pool,
        MigrationOptions::new()
            .initialize_empty_database(settings.database.initialize_empty_database),
    )
    .await?;
    let dependencies = nexora::account::server::dependencies(pool.clone(), &settings).await?;
    let account = Account::new(dependencies);

    let app = Router::new()
        .merge(account.routers::<AppState>())
        .merge(routes::routers())
        .with_state(AppState { pool });
    let listener = TcpListener::bind(settings.server.bind).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("应当可以监听 Ctrl-C 关闭信号");
}
