use std::error::Error;

use axum::{Router, routing::get};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server = nexora::Server::new();

    // 完整 Account 初始化需要宿主提供 PostgreSQL、配置和 setup secret；本示例只展示
    // 零配置 Router 组合方式，因此不执行迁移、Account 初始化或外部服务请求。
    let router = Router::new()
        .merge(server.routers())
        .route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    println!("server_basic listening on http://{address}");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health() -> &'static str {
    "ok\n"
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to install Ctrl+C handler: {error}");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                if signal.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            }
            Err(error) => {
                eprintln!("failed to install SIGTERM handler: {error}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
