//! 服务账号资料的数据访问函数。

use sqlx::{PgPool, Postgres, Transaction};

use crate::{ServiceAccountIdentity, StoreError, entities::account::User};

const USER_ID_GENERATION_ATTEMPTS: usize = 16;

pub(crate) async fn provision(
    identity: &ServiceAccountIdentity,
    role_ids: &[i64],
    granted_by: &str,
    pool: &PgPool,
) -> Result<User, StoreError> {
    let mut transaction = pool.begin().await?;
    ensure_operator(granted_by, &mut transaction).await?;
    if service_account_identity_exists(
        identity.identity_id.as_str(),
        identity.username.as_str(),
        &mut transaction,
    )
    .await?
    {
        return Err(StoreError::Conflict("service_account_already_exists"));
    }

    for _ in 0..USER_ID_GENERATION_ATTEMPTS {
        let user_id = super::identities::generate_user_id()?;
        let inserted = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO account.users (
                id, identity_id, username, email, display_name, user_type, description
            )
            VALUES ($1, $2, $3, NULL, $4, 'service_account', $5)
            ON CONFLICT DO NOTHING
            RETURNING id, identity_id, username, email, display_name, status, user_type,
                      description, is_super_admin, created_at, updated_at, last_login_at
            "#,
        )
        .bind(user_id)
        .bind(identity.identity_id.as_str())
        .bind(identity.username.as_str())
        .bind(identity.display_name.as_str())
        .bind(identity.description.as_deref())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(user) = inserted {
            super::users::grant_initial_service_account_roles(
                user.id.as_str(),
                role_ids,
                granted_by,
                &mut transaction,
            )
            .await?;
            transaction.commit().await?;
            return Ok(user);
        }
        if service_account_identity_exists(
            identity.identity_id.as_str(),
            identity.username.as_str(),
            &mut transaction,
        )
        .await?
        {
            return Err(StoreError::Conflict("service_account_already_exists"));
        }
    }
    Err(StoreError::Database(sqlx::Error::Protocol(
        "无法在限定次数内生成唯一的 8 位服务账号 ID".to_owned(),
    )))
}

pub(crate) async fn update_profile(
    user_id: &str,
    display_name: &str,
    description: Option<&str>,
    pool: &PgPool,
) -> Result<User, StoreError> {
    sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET display_name = $2, description = $3, updated_at = NOW()
        WHERE id = $1 AND user_type = 'service_account'
        RETURNING id, identity_id, username, email, display_name, status, user_type,
                  description, is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(user_id)
    .bind(display_name)
    .bind(description)
    .fetch_optional(pool)
    .await?
    .ok_or(StoreError::NotFound("服务账号"))
}

async fn ensure_operator(
    user_id: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM account.users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(&mut **transaction)
            .await?;
    if exists {
        Ok(())
    } else {
        Err(StoreError::NotFound("授权操作者"))
    }
}

async fn service_account_identity_exists(
    identity_id: &str,
    username: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM account.users
            WHERE identity_id = $1 OR lower(username) = lower($2)
        )
        "#,
    )
    .bind(identity_id)
    .bind(username)
    .fetch_one(&mut **transaction)
    .await
}
