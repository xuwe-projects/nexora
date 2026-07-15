//! PostgreSQL 数据库结构迁移程序。
//!
//! 该程序是 `crates/migrate/migrations` 的唯一执行入口，不包含业务查询或 HTTP 路由。

mod safety;

use std::{collections::HashSet, error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use configuration::LayeredConfigLoader;
use serde::Deserialize;
use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};

use crate::safety::{DatabaseState, validate_migration_safety};

const DEFAULT_CONFIG_FILE: &str = "config/server.toml";
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Parser)]
#[command(name = "migrate", version, about = "安全地向前应用 PostgreSQL 迁移")]
struct Arguments {
    /// 指定数据库配置文件；默认读取 `config/server.toml`。
    #[arg(value_name = "FILE")]
    config: Option<PathBuf>,

    /// 明确允许在没有迁移历史和业务 schema 的空数据库上执行首次安装。
    #[arg(long)]
    initialize_empty_database: bool,
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
    let target = migration_target(&pool).await?;
    let state = database_state(&pool).await?;
    validate_migration_safety(&state, arguments.initialize_empty_database)?;

    let applied = state
        .applied_migrations
        .iter()
        .map(|(version, _)| *version)
        .collect::<HashSet<_>>();
    let pending = MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .filter(|migration| !applied.contains(&migration.version))
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    eprintln!(
        "数据库迁移目标: database={}, server={}:{}, 已应用={}，待应用={pending:?}",
        target.database,
        target.server_address.as_deref().unwrap_or("local"),
        target
            .server_port
            .map_or_else(|| "default".to_owned(), |port| port.to_string()),
        state.applied_migrations.len(),
    );

    MIGRATOR.run(&pool).await?;
    eprintln!("数据库迁移完成: database={}", target.database);
    Ok(())
}

struct MigrationTarget {
    database: String,
    server_address: Option<String>,
    server_port: Option<i32>,
}

async fn migration_target(pool: &PgPool) -> Result<MigrationTarget, sqlx::Error> {
    let (database, server_address, server_port) =
        sqlx::query_as::<_, (String, Option<String>, Option<i32>)>(
            r#"
        SELECT current_database(), inet_server_addr()::TEXT, inet_server_port()
        "#,
        )
        .fetch_one(pool)
        .await?;
    Ok(MigrationTarget {
        database,
        server_address,
        server_port,
    })
}

async fn database_state(pool: &PgPool) -> Result<DatabaseState, sqlx::Error> {
    let migrations_table_exists =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(pool)
            .await?;
    let applied_migrations = if migrations_table_exists {
        sqlx::query_as::<_, (i64, bool)>(
            "SELECT version, success FROM public._sqlx_migrations ORDER BY version",
        )
        .fetch_all(pool)
        .await?
    } else {
        Vec::new()
    };
    let account_schema_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname = 'account')",
    )
    .fetch_one(pool)
    .await?;
    let (
        users_exists,
        roles_exists,
        permissions_exists,
        role_permissions_exists,
        user_roles_exists,
        system_initialization_exists,
    ) = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool)>(
        r#"
        SELECT
            to_regclass('account.users') IS NOT NULL,
            to_regclass('account.roles') IS NOT NULL,
            to_regclass('account.permissions') IS NOT NULL,
            to_regclass('account.role_permissions') IS NOT NULL,
            to_regclass('account.user_roles') IS NOT NULL,
            to_regclass('account.system_initialization') IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(DatabaseState {
        applied_migrations,
        account_schema_exists,
        users_exists,
        roles_exists,
        permissions_exists,
        role_permissions_exists,
        user_roles_exists,
        system_initialization_exists,
    })
}
