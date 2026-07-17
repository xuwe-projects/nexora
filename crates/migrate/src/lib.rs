//! Nexora 工作区的集中式 PostgreSQL 迁移运行时。
//!
//! 迁移文件仍由本 crate 统一拥有。服务端应用既可以使用同包的 `migrate` 命令，也可以
//! 通过 [`prepare`] 在自己的 composition root 中执行完全相同的安全检查和向前迁移。

mod safety;

use std::{collections::HashSet, io};

use sqlx::{PgPool, migrate::Migrator};
use thiserror::Error;

use crate::safety::{DatabaseState, validate_migration_safety};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 已通过 fail-closed 安全检查、可以执行的迁移计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    target: MigrationTarget,
    applied_count: usize,
    pending_versions: Vec<i64>,
}

impl MigrationPlan {
    /// 返回不包含凭据的数据库目标信息。
    pub const fn target(&self) -> &MigrationTarget {
        &self.target
    }

    /// 返回数据库中已有的成功迁移数量。
    pub const fn applied_count(&self) -> usize {
        self.applied_count
    }

    /// 返回尚待执行的向前迁移版本。
    pub fn pending_versions(&self) -> &[i64] {
        self.pending_versions.as_slice()
    }

    /// 执行计划中的向前迁移。
    ///
    /// # Errors
    ///
    /// SQLx 无法创建迁移表、获取迁移锁或执行任一迁移时返回错误。
    pub async fn run(self, pool: &PgPool) -> Result<MigrationReport, MigrationError> {
        MIGRATOR.run(pool).await.map_err(MigrationError::Apply)?;
        Ok(MigrationReport {
            target: self.target,
            applied_count: self.applied_count,
            applied_versions: self.pending_versions,
        })
    }
}

/// 一次成功迁移的非敏感结果摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    target: MigrationTarget,
    applied_count: usize,
    applied_versions: Vec<i64>,
}

impl MigrationReport {
    /// 返回本次迁移连接的数据库目标。
    pub const fn target(&self) -> &MigrationTarget {
        &self.target
    }

    /// 返回执行前已经存在的成功迁移数量。
    pub const fn previous_applied_count(&self) -> usize {
        self.applied_count
    }

    /// 返回本次计划并成功应用的向前迁移版本。
    pub fn applied_versions(&self) -> &[i64] {
        self.applied_versions.as_slice()
    }
}

/// 不包含连接凭据的数据库目标信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationTarget {
    database: String,
    server_address: Option<String>,
    server_port: Option<i32>,
}

impl MigrationTarget {
    /// 返回 PostgreSQL 当前数据库名。
    pub fn database(&self) -> &str {
        self.database.as_str()
    }

    /// 返回 PostgreSQL 服务端地址；本地 Unix socket 连接时可能为空。
    pub fn server_address(&self) -> Option<&str> {
        self.server_address.as_deref()
    }

    /// 返回 PostgreSQL 服务端端口；平台无法报告时可能为空。
    pub const fn server_port(&self) -> Option<i32> {
        self.server_port
    }
}

/// 准备或执行集中式迁移失败的原因。
#[derive(Debug, Error)]
pub enum MigrationError {
    /// 读取数据库目标或 schema 状态失败。
    #[error("读取数据库迁移状态失败（{operation}）")]
    Inspect {
        /// 失败的稳定操作名称。
        operation: &'static str,
        /// SQLx 返回的数据库错误。
        #[source]
        source: sqlx::Error,
    },
    /// 目标数据库未通过 fail-closed 安全检查。
    #[error("数据库迁移安全检查失败")]
    Safety(
        /// 不包含数据库凭据的安全检查说明。
        #[source]
        io::Error,
    ),
    /// SQLx 执行向前迁移失败。
    #[error("应用数据库迁移失败")]
    Apply(
        /// SQLx 迁移运行器返回的错误。
        #[source]
        sqlx::migrate::MigrateError,
    ),
}

/// 检查目标数据库并创建一个可执行的向前迁移计划。
///
/// 空数据库会作为首次安装自动执行全部迁移；已有 account schema 却没有迁移历史、
/// 存在失败记录或核心表缺失时始终拒绝。
///
/// # Errors
///
/// 无法读取数据库状态，或目标数据库未通过安全检查时返回错误。
pub async fn prepare(pool: &PgPool) -> Result<MigrationPlan, MigrationError> {
    let target = migration_target(pool).await?;
    let state = database_state(pool).await?;
    validate_migration_safety(&state).map_err(MigrationError::Safety)?;

    let applied = state
        .applied_migrations
        .iter()
        .map(|(version, _)| *version)
        .collect::<HashSet<_>>();
    let pending_versions = MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .filter(|migration| !applied.contains(&migration.version))
        .map(|migration| migration.version)
        .collect();

    Ok(MigrationPlan {
        target,
        applied_count: state.applied_migrations.len(),
        pending_versions,
    })
}

async fn migration_target(pool: &PgPool) -> Result<MigrationTarget, MigrationError> {
    let (database, server_address, server_port) =
        sqlx::query_as::<_, (String, Option<String>, Option<i32>)>(
            r#"
            SELECT current_database(), inet_server_addr()::TEXT, inet_server_port()
            "#,
        )
        .fetch_one(pool)
        .await
        .map_err(|source| MigrationError::Inspect {
            operation: "migration_target",
            source,
        })?;
    Ok(MigrationTarget {
        database,
        server_address,
        server_port,
    })
}

async fn database_state(pool: &PgPool) -> Result<DatabaseState, MigrationError> {
    let migrations_table_exists =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(pool)
            .await
            .map_err(|source| MigrationError::Inspect {
                operation: "migration_history_exists",
                source,
            })?;
    let applied_migrations = if migrations_table_exists {
        sqlx::query_as::<_, (i64, bool)>(
            "SELECT version, success FROM public._sqlx_migrations ORDER BY version",
        )
        .fetch_all(pool)
        .await
        .map_err(|source| MigrationError::Inspect {
            operation: "migration_history",
            source,
        })?
    } else {
        Vec::new()
    };
    let account_schema_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname = 'account')",
    )
    .fetch_one(pool)
    .await
    .map_err(|source| MigrationError::Inspect {
        operation: "account_schema",
        source,
    })?;
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
    .await
    .map_err(|source| MigrationError::Inspect {
        operation: "account_tables",
        source,
    })?;
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
