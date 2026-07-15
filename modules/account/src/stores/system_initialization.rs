//! 一次性系统初始化状态与超级管理员写入。

use sqlx::PgPool;

use crate::{
    StoreError,
    entities::account::{ExternalIdentity, User, UserStatus},
    stores::identities,
};

const SUPER_ADMIN_LOCK_KEY: i64 = 0x5855_5745_5355_5045;

/// 返回数据库单例记录声明的系统初始化状态。
pub(crate) async fn query(pool: &PgPool) -> Result<bool, StoreError> {
    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT is_initialized
        FROM account.system_initialization
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|source| StoreError::database_operation("system_initialization.query_state", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))
}

/// 原子清空所选用户角色、设置超级管理员并完成一次性系统初始化。
pub(crate) async fn initialize_super_admin(
    identity: &ExternalIdentity,
    pool: &PgPool,
) -> Result<User, StoreError> {
    let mut transaction = pool.begin().await.map_err(|source| {
        StoreError::database_operation("system_initialization.begin_transaction", source)
    })?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(SUPER_ADMIN_LOCK_KEY)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            StoreError::database_operation("system_initialization.acquire_lock", source)
        })?;

    let initialized = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT is_initialized
        FROM account.system_initialization
        WHERE id = 1
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| StoreError::database_operation("system_initialization.lock_state", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    if initialized {
        return Err(StoreError::SystemAlreadyInitialized);
    }

    let user = identities::upsert(identity, &mut transaction)
        .await
        .map_err(|source| {
            StoreError::database_operation("system_initialization.upsert_user", source)
        })?;
    // 超级管理员标记生效后，数据库触发器会拒绝其任何角色变更。
    sqlx::query("DELETE FROM account.user_roles WHERE user_id = $1")
        .bind(user.id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            StoreError::database_operation("system_initialization.clear_user_roles", source)
        })?;
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET is_super_admin = TRUE, status = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING id, identity_id, email, display_name, avatar_url, status,
                  is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(user.id.as_str())
    .bind(UserStatus::Active)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|source| {
        StoreError::database_operation("system_initialization.mark_super_admin", source)
    })?;
    let initialization = sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET is_initialized = TRUE,
            super_admin_user_id = $1,
            initialized_at = NOW()
        WHERE id = 1 AND NOT is_initialized
        "#,
    )
    .bind(user.id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        StoreError::database_operation("system_initialization.complete_state", source)
    })?;
    if initialization.rows_affected() != 1 {
        return Err(StoreError::InvalidData("系统初始化状态"));
    }
    transaction.commit().await.map_err(|source| {
        StoreError::database_operation("system_initialization.commit_transaction", source)
    })?;
    Ok(user)
}
