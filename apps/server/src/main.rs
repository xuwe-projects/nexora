//! 服务端程序入口。

mod bootstrap;
mod config;
mod routers;
mod super_admin;

use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use tokio::net::TcpListener;

use crate::config::{ServerConfig, default_config_file};

/// 服务端启动参数。
#[derive(Debug, Parser)]
#[command(name = "server", version, about = "启动 API 服务")]
struct Arguments {
    /// 指定服务端 TOML 配置文件；默认读取 `config/server.toml`。
    #[arg(value_name = "FILE")]
    config: Option<PathBuf>,

    /// 仅加载并验证配置后退出，不连接数据库或启动监听器。
    #[arg(long)]
    check_config: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = logging::initialize("info") {
        eprintln!("{}", logging::format_error_chain(&error));
        return ExitCode::FAILURE;
    }

    if let Err(error) = run().await {
        tracing::error!(
            error = %format_startup_error(error.as_ref()),
            "服务端启动失败"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn format_startup_error(error: &(dyn Error + 'static)) -> String {
    error
        .downcast_ref::<config::ServerConfigError>()
        .map(config::ServerConfigError::safe_diagnostic)
        .unwrap_or_else(|| logging::format_error_chain(error))
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse();
    let config_path = arguments.config.unwrap_or_else(default_config_file);
    let config = ServerConfig::load_file(&config_path)?;

    if arguments.check_config {
        tracing::info!(address = %config.bind_address(), "服务端配置已加载");
        return Ok(());
    }

    let bind_address = config.bind_address();
    let initialized = bootstrap::initialize(&config).await?;
    let listener = TcpListener::bind(bind_address).await?;
    tracing::info!(address = %bind_address, "API 服务已启动");
    let app = routers::initialize(initialized.account).with_state(initialized.state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    wait_for_unix_shutdown().await;

    #[cfg(not(unix))]
    wait_for_ctrl_c().await;
}

async fn wait_for_ctrl_c() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!(signal = "ctrl_c", "收到服务关闭信号"),
        Err(error) => tracing::warn!(error = ?error, "无法监听 Ctrl-C 关闭信号"),
    }
}

#[cfg(unix)]
async fn wait_for_unix_shutdown() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(terminate) => terminate,
        Err(error) => {
            tracing::warn!(error = ?error, "无法监听 SIGTERM，继续等待 Ctrl-C");
            wait_for_ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        result = tokio::signal::ctrl_c() => match result {
            Ok(()) => tracing::info!(signal = "ctrl_c", "收到服务关闭信号"),
            Err(error) => tracing::warn!(error = ?error, "无法监听 Ctrl-C 关闭信号"),
        },
        received = terminate.recv() => {
            if received.is_some() {
                tracing::info!(signal = "SIGTERM", "收到服务关闭信号");
            } else {
                tracing::warn!("SIGTERM 信号流意外结束");
            }
        },
    }
}
