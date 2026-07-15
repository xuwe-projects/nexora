//! PostgreSQL 数据库结构迁移程序。
//!
//! 该程序是 `crates/migrate/migrations` 的唯一执行入口，不包含业务查询或 HTTP 路由。

use std::{error::Error, path::PathBuf};

use configuration::LayeredConfigLoader;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;

const DEFAULT_CONFIG_FILE: &str = "config/server.toml";

#[derive(Debug, Deserialize)]
struct MigrationConfig {
    database: DatabaseConfig,
}

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    url: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE));
    let config = LayeredConfigLoader::<MigrationConfig>::new()
        .with_required_file(config_path)
        .load()?;
    let pool = PgPoolOptions::new().connect(&config.database.url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(())
}
