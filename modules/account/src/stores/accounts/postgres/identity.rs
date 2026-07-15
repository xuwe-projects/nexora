//! 外部身份同步与系统角色授予查询。

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{ExternalIdentity, StoreError, entities::account::UserRow};

pub(super) async fn upsert_identity(
    transaction: &mut Transaction<'_, Postgres>,
    identity: &ExternalIdentity,
) -> Result<UserRow, StoreError> {
    Ok(sqlx::query_as::<_, UserRow>(
        r#"
        INSERT INTO account.users (issuer, subject, email, display_name, avatar_url)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (issuer, subject) DO UPDATE SET
            email = COALESCE(EXCLUDED.email, users.email),
            display_name = EXCLUDED.display_name,
            avatar_url = COALESCE(EXCLUDED.avatar_url, users.avatar_url),
            updated_at = NOW(),
            last_login_at = NOW()
        RETURNING id, issuer, subject, email, display_name, avatar_url, status,
                  is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(identity.issuer.trim())
    .bind(identity.subject.trim())
    .bind(identity.email.as_deref())
    .bind(identity.display_name.trim())
    .bind(identity.avatar_url.as_deref())
    .fetch_one(&mut **transaction)
    .await?)
}

pub(super) async fn grant_system_role(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    role_key: &str,
) -> Result<(), StoreError> {
    let result = sqlx::query(
        r#"
        INSERT INTO account.user_roles (user_id, role_id)
        SELECT $1, roles.id
        FROM account.roles
        WHERE roles.key = $2
          AND roles.is_system
          AND NOT EXISTS (
              SELECT 1 FROM account.user_roles
              WHERE user_roles.user_id = $1 AND user_roles.role_id = roles.id
          )
        ON CONFLICT (user_id, role_id) DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(role_key)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() == 0 {
        let assigned = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM account.user_roles
                JOIN account.roles ON roles.id = user_roles.role_id
                WHERE user_roles.user_id = $1 AND roles.key = $2
            )
            "#,
        )
        .bind(user_id)
        .bind(role_key)
        .fetch_one(&mut **transaction)
        .await?;
        if !assigned {
            return Err(StoreError::InvalidData("系统角色"));
        }
    }
    Ok(())
}

pub(super) async fn remove_non_super_admin_roles(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), StoreError> {
    sqlx::query(
        r#"
        DELETE FROM account.user_roles
        USING account.roles AS roles
        WHERE user_roles.user_id = $1
          AND user_roles.role_id = roles.id
          AND roles.key NOT IN ('member', 'super-administrator')
        "#,
    )
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
