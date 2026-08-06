//! 应用业务数据库对象的 SQLx 迁移入口。

use std::borrow::Cow;

use sqlx::{PgPool, migrate::Migrator};

static MIGRATOR: Migrator = Migrator {
    table_name: Cow::Borrowed("public._sqlx_migrations"),
    ..sqlx::migrate!("./migrations")
};

/// 在宿主创建的唯一连接池上执行全部待处理应用迁移。
///
/// 本函数只迁移应用业务对象，并把历史独立记录在 `public._sqlx_migrations`。服务端启动时
/// 必须先调用 `nexora::server::migrate(&pool)`，再调用本函数，之后才能初始化 Account 和
/// 接收流量。
///
/// # Errors
///
/// SQLx 无法创建应用迁移历史表、获取迁移锁、校验 checksum 或执行迁移时返回原始迁移错误。
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}
