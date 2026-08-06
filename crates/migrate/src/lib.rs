//! Nexora 框架数据库对象的 PostgreSQL 迁移入口。
//!
//! 本 crate 只拥有 Nexora 自身的 schema 与对象。宿主通过 [`migrate`] 借用自己创建的唯一
//! `PgPool` 执行框架迁移；应用业务迁移由宿主在其后独立执行。

use std::borrow::Cow;

use sqlx::{PgPool, migrate::Migrator};
use thiserror::Error;

const MIGRATION_SCHEMA: &str = "nexora";
const MIGRATION_TABLE: &str = "nexora._sqlx_migrations";

static MIGRATOR: Migrator = Migrator {
    table_name: Cow::Borrowed(MIGRATION_TABLE),
    create_schemas: Cow::Borrowed(&[Cow::Borrowed(MIGRATION_SCHEMA)]),
    ..sqlx::migrate!("./migrations")
};

/// Nexora 框架数据库迁移失败的原因。
///
/// 该错误保留 SQLx 原始迁移错误作为 source，调用方可以在日志边界输出完整错误链，同时避免
/// 把数据库内部细节直接暴露给客户端。
#[derive(Debug, Error)]
pub enum MigrationError {
    /// SQLx 创建迁移 schema、维护迁移历史或执行框架 DDL 时失败。
    #[error("执行 Nexora 数据库迁移失败")]
    Apply(
        /// SQLx 0.9 原生 `Migrator` 返回的底层错误。
        #[source]
        sqlx::migrate::MigrateError,
    ),
}

/// 在宿主提供的连接池上执行全部待处理 Nexora 框架迁移。
///
/// 该函数借用宿主在 composition root 中创建的唯一 [`PgPool`]，不会建立第二连接池。迁移使用
/// SQLx 0.9 原生 [`Migrator`]，自动创建 `nexora` schema，并把独立历史固定记录在
/// `nexora._sqlx_migrations`。重复调用时 SQLx 只校验已应用迁移并执行仍待处理的版本。
///
/// 宿主必须在调用成功后再运行自己的应用 Migrator，随后才能初始化 Account、构造 Router 并
/// 接收流量。Nexora 迁移与应用迁移拥有独立历史，不应合并、重编号或协调 checksum。
///
/// # Errors
///
/// 无法创建 `nexora` schema 或迁移历史表、无法获取迁移锁、已应用迁移 checksum 不一致，或
/// 任一框架迁移执行失败时返回 [`MigrationError`]，并保留 SQLx 原始错误作为 source。
pub async fn migrate(pool: &PgPool) -> Result<(), MigrationError> {
    MIGRATOR.run(pool).await.map_err(MigrationError::Apply)
}
