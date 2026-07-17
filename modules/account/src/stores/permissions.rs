//! 权限目录查询。

use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::{
    StoreError,
    entities::account::{Permission, PermissionDefinition, PermissionRow},
};

/// 幂等注册或更新应用声明的权限目录项。
pub(crate) async fn register(
    definitions: &[PermissionDefinition],
    pool: &PgPool,
) -> Result<Vec<Permission>, StoreError> {
    if definitions.is_empty() {
        return Ok(Vec::new());
    }
    let mut query =
        QueryBuilder::<Postgres>::new("INSERT INTO account.permissions (key, name, description) ");
    query.push_values(definitions, |mut row, definition| {
        row.push_bind(definition.key.trim())
            .push_bind(definition.name.trim())
            .push_bind(
                definition
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|description| !description.is_empty()),
            );
    });
    query.push(
        r#"
        ON CONFLICT (key) DO UPDATE
        SET name = EXCLUDED.name,
            description = EXCLUDED.description
        RETURNING id, key, name, description
        "#,
    );
    query
        .build_query_as::<PermissionRow>()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(Permission::try_from)
        .collect()
}

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
