//! 权限目录查询。

use std::collections::BTreeSet;

use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::{
    SYSTEM_ROLE_OWNER, StoreError,
    entities::account::{
        Permission, PermissionCatalogDefinition, PermissionDefinition, PermissionRow,
    },
};

/// 幂等注册或更新应用声明的权限目录项。
pub(crate) async fn register(
    definitions: &[PermissionDefinition],
    pool: &PgPool,
) -> Result<Vec<Permission>, StoreError> {
    let definitions = definitions
        .iter()
        .cloned()
        .map(PermissionCatalogDefinition::from)
        .collect::<Vec<_>>();
    register_catalog(definitions.as_slice(), pool).await
}

/// 幂等注册或更新应用声明的权限目录项及其蕴含关系。
pub(crate) async fn register_catalog(
    definitions: &[PermissionCatalogDefinition],
    pool: &PgPool,
) -> Result<Vec<Permission>, StoreError> {
    if definitions.is_empty() {
        return Ok(Vec::new());
    }
    let mut transaction = pool.begin().await?;
    let administrator_role_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM account.roles
        WHERE key = 'admin' AND owner = $1 AND is_system = TRUE
        FOR SHARE
        "#,
    )
    .bind(SYSTEM_ROLE_OWNER)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(StoreError::NotFound("系统管理员角色"))?;
    let mut query =
        QueryBuilder::<Postgres>::new("INSERT INTO account.permissions (key, name, description) ");
    query.push_values(definitions, |mut row, definition| {
        row.push_bind(definition.permission.key.trim())
            .push_bind(definition.permission.name.trim())
            .push_bind(
                definition
                    .permission
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
    replace_implications(definitions, permissions.as_slice(), &mut transaction).await?;
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

async fn replace_implications(
    definitions: &[PermissionCatalogDefinition],
    permissions: &[Permission],
    transaction: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let permission_ids = permissions
        .iter()
        .map(|permission| permission.id)
        .collect::<Vec<_>>();
    sqlx::query("DELETE FROM account.permission_implications WHERE permission_id = ANY($1)")
        .bind(&permission_ids)
        .execute(&mut **transaction)
        .await?;

    for definition in definitions {
        if definition.implies.is_empty() {
            continue;
        }
        let permission_id = permissions
            .iter()
            .find(|permission| permission.key.as_str() == definition.permission.key.trim())
            .map(|permission| permission.id)
            .ok_or(StoreError::NotFound("权限"))?;
        let implied_keys = definition
            .implies
            .iter()
            .map(|key| key.trim())
            .collect::<BTreeSet<_>>();
        for implied_key in implied_keys {
            let implied_permission_id =
                sqlx::query_scalar::<_, i64>("SELECT id FROM account.permissions WHERE key = $1")
                    .bind(implied_key)
                    .fetch_optional(&mut **transaction)
                    .await?
                    .ok_or(StoreError::NotFound("权限蕴含"))?;
            sqlx::query(
                r#"
                INSERT INTO account.permission_implications (permission_id, implied_permission_id)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(permission_id)
            .bind(implied_permission_id)
            .execute(&mut **transaction)
            .await?;
        }
    }

    Ok(())
}
