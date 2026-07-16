//! 一次性系统初始化状态与超级管理员写入。

use sqlx::{FromRow, PgPool, Postgres};

use crate::{
    StoreError,
    entities::account::{ExternalIdentity, User, UserStatus},
    stores::identities,
};

const SUPER_ADMIN_LOCK_KEY: i64 = 0x5855_5745_5355_5045;

pub(crate) enum InitializationOutcome {
    Initialized(User),
    AlreadyInitialized(User),
}

pub(crate) enum IdentityIssuerBindingOutcome {
    Bound,
    Verified,
}

#[derive(FromRow)]
struct InitializationRecord {
    is_initialized: bool,
    super_admin_user_id: Option<String>,
}

/// 在单例初始化记录上原子绑定部署 issuer，或验证它与首次绑定值一致。
pub(crate) async fn bind_or_verify_identity_issuer(
    identity_issuer: &str,
    pool: &PgPool,
) -> Result<IdentityIssuerBindingOutcome, StoreError> {
    let mut transaction = pool.begin().await.map_err(|source| {
        StoreError::database_operation("identity_issuer.begin_transaction", source)
    })?;
    let current = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT identity_issuer
        FROM account.system_initialization
        WHERE id = 1
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| StoreError::database_operation("identity_issuer.lock_state", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    let outcome = match current {
        Some(current) if current == identity_issuer => IdentityIssuerBindingOutcome::Verified,
        Some(_) => return Err(StoreError::IdentityIssuerMismatch),
        None => {
            let updated = sqlx::query(
                r#"
                UPDATE account.system_initialization
                SET identity_issuer = $1
                WHERE id = 1 AND identity_issuer IS NULL
                "#,
            )
            .bind(identity_issuer)
            .execute(&mut *transaction)
            .await
            .map_err(|source| StoreError::database_operation("identity_issuer.bind", source))?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidData("部署 OIDC issuer"));
            }
            IdentityIssuerBindingOutcome::Bound
        }
    };
    transaction.commit().await.map_err(|source| {
        StoreError::database_operation("identity_issuer.commit_transaction", source)
    })?;
    Ok(outcome)
}

/// 验证可信 token 的 issuer 与当前部署首次绑定值一致。
pub(crate) async fn verify_identity_issuer(
    identity_issuer: &str,
    pool: &PgPool,
) -> Result<(), StoreError> {
    let current = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT identity_issuer
        FROM account.system_initialization
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|source| StoreError::database_operation("identity_issuer.verify", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    match current {
        Some(current) if current == identity_issuer => Ok(()),
        Some(_) => Err(StoreError::IdentityIssuerMismatch),
        None => Err(StoreError::IdentityIssuerNotBound),
    }
}

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

/// 返回初始化完成时绑定的超级管理员；尚未初始化时返回 `None`。
pub(crate) async fn query_status(pool: &PgPool) -> Result<Option<User>, StoreError> {
    let record = sqlx::query_as::<_, InitializationRecord>(
        r#"
        SELECT is_initialized, super_admin_user_id
        FROM account.system_initialization
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|source| StoreError::database_operation("system_initialization.query_status", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    completed_super_admin(record, pool).await
}

/// 原子清空所选用户角色、设置超级管理员并完成可重试的一次性初始化。
pub(crate) async fn initialize(
    identity: &ExternalIdentity,
    pool: &PgPool,
) -> Result<InitializationOutcome, StoreError> {
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

    let record = sqlx::query_as::<_, InitializationRecord>(
        r#"
        SELECT is_initialized, super_admin_user_id
        FROM account.system_initialization
        WHERE id = 1
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| StoreError::database_operation("system_initialization.lock_state", source))?
    .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    if record.is_initialized {
        let super_admin = completed_super_admin(record, &mut *transaction)
            .await?
            .ok_or(StoreError::InvalidData("系统初始化状态"))?;
        let same_identity = super_admin.identity_id == identity.identity_id;
        transaction.commit().await.map_err(|source| {
            StoreError::database_operation("system_initialization.commit_existing", source)
        })?;
        return if same_identity {
            Ok(InitializationOutcome::AlreadyInitialized(super_admin))
        } else {
            Err(StoreError::SystemAlreadyInitialized)
        };
    }
    if record.super_admin_user_id.is_some() {
        return Err(StoreError::InvalidData("系统初始化状态"));
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
    Ok(InitializationOutcome::Initialized(user))
}

async fn completed_super_admin<'e, E>(
    record: InitializationRecord,
    executor: E,
) -> Result<Option<User>, StoreError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    if !record.is_initialized {
        return if record.super_admin_user_id.is_none() {
            Ok(None)
        } else {
            Err(StoreError::InvalidData("系统初始化状态"))
        };
    }
    let user_id = record
        .super_admin_user_id
        .ok_or(StoreError::InvalidData("系统初始化状态"))?;
    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT id, identity_id, email, display_name, avatar_url, status,
               is_super_admin, created_at, updated_at, last_login_at
        FROM account.users
        WHERE id = $1 AND is_super_admin
        "#,
    )
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(|source| {
        StoreError::database_operation("system_initialization.query_super_admin", source)
    })?
    .ok_or(StoreError::InvalidData("系统初始化超级管理员"))?;
    Ok(Some(user))
}
