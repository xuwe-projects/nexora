mod config;
mod routes;

use axum::Router;
use nexora::Server;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let settings: config::Settings = nexora::config::initialize(None)?;
    let setup_secret = settings.setup.secret()?;
    let pool = PgPoolOptions::new()
        .max_connections(settings.database.max_connections)
        .connect(settings.database.url.as_str())
        .await?;
    let mut server = Server::new();
    server.initialize(&settings, &pool, setup_secret).await?;
    let app = Router::new()
        .merge(server.routers())
        .merge(routes::routers())
        .with_state(pool);
    let listener =
        tokio::net::TcpListener::bind((settings.server.ip, settings.server.port)).await?;
    let address = listener.local_addr()?;
    tracing::info!(%address, "服务端已启动");
    if let Some(setup_url) = server.setup_url(address) {
        tracing::warn!(%setup_url, "系统尚未完成初始化，请访问 Setup 页面");
    }
    axum::serve(listener, app).await?;
    Ok(())
}
