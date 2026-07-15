//! 用户状态与管理员不变式查询辅助函数。

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{StoreError, entities::account::DatabaseUserStatus};

pub(super) async fn protect_active_administrator(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    administrator_role_id: Uuid,
) -> Result<(), StoreError> {
    let is_administrator = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM account.user_roles WHERE user_id = $1 AND role_id = $2)",
    )
    .bind(user_id)
    .bind(administrator_role_id)
    .fetch_one(&mut **transaction)
    .await?;
    if is_administrator
        && lock_active_administrators(transaction, administrator_role_id)
            .await?
            .len()
            <= 1
    {
        return Err(StoreError::LastAdministrator);
    }
    Ok(())
}

pub(super) async fn lock_active_administrators(
    transaction: &mut Transaction<'_, Postgres>,
    administrator_role_id: Uuid,
) -> Result<Vec<Uuid>, StoreError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT users.id
        FROM account.users
        JOIN account.user_roles ON user_roles.user_id = users.id
        WHERE user_roles.role_id = $1 AND users.status = $2
        ORDER BY users.id
        FOR UPDATE OF users
        "#,
    )
    .bind(administrator_role_id)
    .bind(DatabaseUserStatus::Active)
    .fetch_all(&mut **transaction)
    .await?)
}
