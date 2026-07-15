//! 角色、权限与系统角色查询辅助函数。

use std::collections::HashMap;

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    Permission, Role, StoreError,
    entities::account::{RolePermissionRow, RoleRow},
};

pub(super) async fn roles_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Role>, StoreError> {
    let rows = sqlx::query_as::<_, RoleRow>(
        r#"
        SELECT roles.id, roles.key, roles.name, roles.description, roles.is_system,
               roles.created_at, roles.updated_at
        FROM account.roles
        JOIN account.user_roles ON user_roles.role_id = roles.id
        WHERE user_roles.user_id = $1
        ORDER BY roles.key
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    attach_permissions(pool, rows).await
}

pub(super) async fn roles_with_permissions(
    pool: &PgPool,
    role_id: Option<Uuid>,
) -> Result<Vec<Role>, StoreError> {
    let rows = if let Some(role_id) = role_id {
        sqlx::query_as::<_, RoleRow>(
            r#"
            SELECT id, key, name, description, is_system, created_at, updated_at
            FROM account.roles
            WHERE id = $1
            ORDER BY key
            "#,
        )
        .bind(role_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, RoleRow>(
            r#"
            SELECT id, key, name, description, is_system, created_at, updated_at
            FROM account.roles
            ORDER BY is_system DESC, key
            "#,
        )
        .fetch_all(pool)
        .await?
    };
    attach_permissions(pool, rows).await
}

async fn attach_permissions(pool: &PgPool, rows: Vec<RoleRow>) -> Result<Vec<Role>, StoreError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let role_ids = rows.iter().map(|role| role.id).collect::<Vec<_>>();
    let permission_rows = sqlx::query_as::<_, RolePermissionRow>(
        r#"
        SELECT role_permissions.role_id, permissions.id, permissions.key,
               permissions.name, permissions.description
        FROM account.role_permissions
        JOIN account.permissions ON permissions.id = role_permissions.permission_id
        WHERE role_permissions.role_id = ANY($1)
        ORDER BY permissions.key
        "#,
    )
    .bind(&role_ids)
    .fetch_all(pool)
    .await?;
    let mut permissions_by_role = HashMap::<Uuid, Vec<Permission>>::new();
    for row in permission_rows {
        permissions_by_role
            .entry(row.role_id)
            .or_default()
            .push(Permission {
                id: row.id,
                key: row.key,
                name: row.name,
                description: row.description,
            });
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let permissions = permissions_by_role.remove(&row.id).unwrap_or_default();
            row.with_permissions(permissions)
        })
        .collect())
}

pub(super) async fn insert_role_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    permission_ids: &[Uuid],
) -> Result<(), StoreError> {
    if permission_ids.is_empty() {
        return Ok(());
    }
    let ids = permission_ids.to_vec();
    let result = sqlx::query(
        r#"
        INSERT INTO account.role_permissions (role_id, permission_id)
        SELECT $1, id FROM account.permissions WHERE id = ANY($2)
        "#,
    )
    .bind(role_id)
    .bind(&ids)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != ids.len() as u64 {
        return Err(StoreError::NotFound("权限"));
    }
    Ok(())
}

pub(super) async fn ensure_mutable_role(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
) -> Result<(), StoreError> {
    let is_system = sqlx::query_scalar::<_, bool>(
        "SELECT is_system FROM account.roles WHERE id = $1 FOR UPDATE",
    )
    .bind(role_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::NotFound("角色"))?;
    if is_system {
        Err(StoreError::SystemRole)
    } else {
        Ok(())
    }
}

pub(super) async fn classify_role_mutation(
    pool: &PgPool,
    role_id: Uuid,
) -> Result<StoreError, StoreError> {
    let is_system =
        sqlx::query_scalar::<_, bool>("SELECT is_system FROM account.roles WHERE id = $1")
            .bind(role_id)
            .fetch_optional(pool)
            .await?;
    Ok(match is_system {
        Some(true) => StoreError::SystemRole,
        Some(false) => StoreError::Conflict("role_not_modified"),
        None => StoreError::NotFound("角色"),
    })
}

pub(super) async fn system_role_id(
    transaction: &mut Transaction<'_, Postgres>,
    role_key: &str,
    lock: bool,
) -> Result<Uuid, StoreError> {
    let role_id = if lock {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM account.roles WHERE key = $1 AND is_system FOR UPDATE",
        )
        .bind(role_key)
        .fetch_optional(&mut **transaction)
        .await?
    } else {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM account.roles WHERE key = $1 AND is_system")
            .bind(role_key)
            .fetch_optional(&mut **transaction)
            .await?
    };
    role_id.ok_or(StoreError::InvalidData("系统角色"))
}

pub(super) fn map_role_insert_error(error: sqlx::Error) -> StoreError {
    if is_unique_violation(&error) {
        StoreError::Conflict("role_key_exists")
    } else {
        StoreError::Database(error)
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23505")
}

pub(super) fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23503")
}

pub(super) fn normalized_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
