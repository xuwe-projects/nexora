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
    let mut transaction = pool.begin().await?;
    let administrator_role_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM account.roles WHERE key = 'admin' AND is_system = TRUE FOR SHARE",
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(StoreError::NotFound("系统管理员角色"))?;
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
    let permissions = query
        .build_query_as::<PermissionRow>()
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(Permission::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let permission_ids = permissions
        .iter()
        .map(|permission| permission.id)
        .collect::<Vec<_>>();
    sqlx::query(
        r#"
        INSERT INTO account.role_permissions (role_id, permission_id)
        SELECT $1, permission_id
        FROM UNNEST($2::bigint[]) AS requested(permission_id)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(administrator_role_id)
    .bind(permission_ids)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(permissions)
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
