//! PostgreSQL 数据库结构迁移程序。
//!
//! 该程序复用同包的 Nexora 框架迁移入口，不包含业务查询或 HTTP 路由。

use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use configuration::LayeredConfigLoader;
use migrate::migrate;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;

const DEFAULT_CONFIG_FILE: &str = "config/server.toml";

#[derive(Debug, Parser)]
#[command(name = "migrate", version, about = "安全地向前应用 PostgreSQL 迁移")]
struct Arguments {
    /// 指定数据库配置文件；默认读取 `config/server.toml`。
    #[arg(value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct MigrationConfig {
    database: DatabaseConfig,
}

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    url: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("数据库迁移失败: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse();
    let config_path = arguments
        .config
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE));
    let config = LayeredConfigLoader::<MigrationConfig>::new()
        .with_required_file(config_path)
        .load()?;
    let pool = PgPoolOptions::new().connect(&config.database.url).await?;
    migrate(&pool).await?;
    eprintln!("Nexora 数据库迁移完成");
    Ok(())
}
