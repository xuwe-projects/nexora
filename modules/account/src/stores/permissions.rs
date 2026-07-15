//! 权限目录查询。

use sqlx::PgPool;

use crate::{
    StoreError,
    entities::account::{Permission, PermissionRow},
};

/// 按稳定权限键返回完整权限目录。
pub(crate) async fn query_all(pool: &PgPool) -> Result<Vec<Permission>, StoreError> {
    sqlx::query_as::<_, PermissionRow>(
        r#"
        SELECT id, key, name, description
        FROM account.permissions
        ORDER BY key
        "#,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Permission::try_from)
    .collect()
}
