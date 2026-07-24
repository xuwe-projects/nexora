//! 角色及角色权限数据访问函数。

use std::collections::HashMap;

use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    SYSTEM_ROLE_OWNER, StoreError,
    entities::account::{Permission, Role, RolePermissionRow, RoleRow, SystemRole},
};

/// 返回首次初始化需要同步到认证授权 Project 的全部系统角色。
pub(crate) async fn query_system(pool: &PgPool) -> Result<Vec<SystemRole>, StoreError> {
    let roles = sqlx::query_as::<_, SystemRole>(
        r#"
        SELECT key, name
        FROM account.roles
        WHERE owner = $1 AND is_system
        ORDER BY key
        "#,
    )
    .bind(SYSTEM_ROLE_OWNER)
    .fetch_all(pool)
    .await?;
    if roles.is_empty() {
        return Err(StoreError::InvalidData("系统角色目录"));
    }
    Ok(roles)
}

/// 返回后台系统范围内的全部角色及其直接权限。
pub(crate) async fn query_all(pool: &PgPool) -> Result<Vec<Role>, StoreError> {
    query_all_for_owner(SYSTEM_ROLE_OWNER, pool).await
}

/// 按 owner 返回全部角色及其直接权限。
pub(crate) async fn query_all_for_owner(
    owner: &str,
    pool: &PgPool,
) -> Result<Vec<Role>, StoreError> {
    let rows = sqlx::query_as::<_, RoleRow>(
        r#"
        SELECT id, owner, key, name, description, is_system, created_at, updated_at
        FROM account.roles
        WHERE owner = $1
        ORDER BY is_system DESC, key
        "#,
    )
    .bind(owner)
    .fetch_all(pool)
    .await?;
    attach_permissions(rows, pool).await
}

/// 按角色 ID 返回后台系统范围内的角色及其直接权限。
pub(crate) async fn query_by_id(role_id: i64, pool: &PgPool) -> Result<Option<Role>, StoreError> {
    query_by_id_for_owner(SYSTEM_ROLE_OWNER, role_id, pool).await
}

/// 按 owner 与角色 ID 返回角色及其直接权限。
pub(crate) async fn query_by_id_for_owner(
    owner: &str,
    role_id: i64,
    pool: &PgPool,
) -> Result<Option<Role>, StoreError> {
    let rows = sqlx::query_as::<_, RoleRow>(
        r#"
        SELECT id, owner, key, name, description, is_system, created_at, updated_at
        FROM account.roles
        WHERE owner = $1 AND id = $2
        "#,
    )
    .bind(owner)
    .bind(role_id)
    .fetch_all(pool)
    .await?;
    Ok(attach_permissions(rows, pool).await?.pop())
}

/// 返回用户直接关联的全部角色及其权限。
pub(crate) async fn query_for_user(user_id: &str, pool: &PgPool) -> Result<Vec<Role>, StoreError> {
    let rows = sqlx::query_as::<_, RoleRow>(
        r#"
        SELECT roles.id, roles.owner, roles.key, roles.name, roles.description, roles.is_system,
               roles.created_at, roles.updated_at
        FROM account.roles
        JOIN account.user_roles ON user_roles.role_id = roles.id
        WHERE user_roles.user_id = $1
        ORDER BY roles.owner, roles.key
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    attach_permissions(rows, pool).await
}

/// 按 owner 创建自定义角色并写入初始权限集合。
pub(crate) async fn create_for_owner(
    owner: &str,
    key: &str,
    name: &str,
    description: Option<&str>,
    permission_ids: &[i64],
    pool: &PgPool,
) -> Result<Role, StoreError> {
    let mut transaction = pool.begin().await?;
    let role_id = match sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO account.roles (key, owner, name, description)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
    )
    .bind(key)
    .bind(owner)
    .bind(name.trim())
    .bind(normalized_optional(description))
    .fetch_one(&mut *transaction)
    .await
    {
        Ok(role_id) => role_id,
        Err(error) => return Err(map_insert_error(error)),
    };
    insert_permissions(role_id, permission_ids, &mut transaction).await?;
    transaction.commit().await?;
    query_by_id_for_owner(owner, role_id, pool)
        .await?
        .ok_or(StoreError::NotFound("角色"))
}

/// 按 owner 创建由数据库序列参与生成全局唯一 key 的自定义角色。
pub(crate) async fn create_generated_for_owner(
    owner: &str,
    name: &str,
    description: Option<&str>,
    permission_ids: &[i64],
    pool: &PgPool,
) -> Result<Role, StoreError> {
    let mut transaction = pool.begin().await?;
    let role_id = match sqlx::query_scalar::<_, i64>(
        r#"
        WITH generated AS (
            SELECT nextval('account.roles_id_seq')::BIGINT AS id
        )
        INSERT INTO account.roles (id, key, owner, name, description)
        SELECT id, 'role_' || id::TEXT, $1, $2, $3
        FROM generated
        RETURNING id
        "#,
    )
    .bind(owner)
    .bind(name.trim())
    .bind(normalized_optional(description))
    .fetch_one(&mut *transaction)
    .await
    {
        Ok(role_id) => role_id,
        Err(error) => return Err(map_insert_error(error)),
    };
    insert_permissions(role_id, permission_ids, &mut transaction).await?;
    transaction.commit().await?;
    query_by_id_for_owner(owner, role_id, pool)
        .await?
        .ok_or(StoreError::NotFound("角色"))
}

/// 按 owner 修改自定义角色名称或说明。
pub(crate) async fn update_for_owner(
    owner: &str,
    role_id: i64,
    name: Option<&str>,
    description: Option<Option<&str>>,
    pool: &PgPool,
) -> Result<Role, StoreError> {
    let result = sqlx::query(
        r#"
        UPDATE account.roles
        SET name = COALESCE($3, name),
            description = CASE WHEN $4 THEN $5 ELSE description END,
            updated_at = NOW()
        WHERE owner = $1 AND id = $2 AND NOT is_system
        "#,
    )
    .bind(owner)
    .bind(role_id)
    .bind(name.map(str::trim))
    .bind(description.is_some())
    .bind(description.and_then(normalized_optional))
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(classify_mutation_for_owner(owner, role_id, pool).await?);
    }
    query_by_id_for_owner(owner, role_id, pool)
        .await?
        .ok_or(StoreError::NotFound("角色"))
}

/// 删除后台系统范围内未被用户引用的自定义角色。
pub(crate) async fn delete(role_id: i64, pool: &PgPool) -> Result<(), StoreError> {
    delete_for_owner(SYSTEM_ROLE_OWNER, role_id, pool).await
}

/// 按 owner 删除未被用户引用的自定义角色。
pub(crate) async fn delete_for_owner(
    owner: &str,
    role_id: i64,
    pool: &PgPool,
) -> Result<(), StoreError> {
    let result =
        sqlx::query("DELETE FROM account.roles WHERE owner = $1 AND id = $2 AND NOT is_system")
            .bind(owner)
            .bind(role_id)
            .execute(pool)
            .await;
    match result {
        Ok(result) if result.rows_affected() == 1 => Ok(()),
        Ok(_) => Err(classify_mutation_for_owner(owner, role_id, pool).await?),
        Err(error) if is_foreign_key_violation(&error) => Err(StoreError::Conflict("role_in_use")),
        Err(error) => Err(error.into()),
    }
}

/// 按 owner 原子替换自定义角色包含的权限集合。
pub(crate) async fn replace_permissions_for_owner(
    owner: &str,
    role_id: i64,
    permission_ids: &[i64],
    pool: &PgPool,
) -> Result<Role, StoreError> {
    let mut transaction = pool.begin().await?;
    ensure_mutable_for_owner(owner, role_id, &mut transaction).await?;
    replace_permissions_in_transaction(role_id, permission_ids, &mut transaction).await?;
    transaction.commit().await?;
    query_by_id_for_owner(owner, role_id, pool)
        .await?
        .ok_or(StoreError::NotFound("角色"))
}

/// 创建或更新后台系统角色，并按权限 key 重建权限集合。
pub(crate) async fn ensure_system_with_permissions(
    key: &str,
    name: &str,
    description: Option<&str>,
    permission_keys: &[String],
    pool: &PgPool,
) -> Result<Role, StoreError> {
    let mut transaction = pool.begin().await?;
    let role_id = match sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO account.roles (key, owner, name, description, is_system)
        VALUES ($1, $2, $3, $4, TRUE)
        ON CONFLICT (key) DO UPDATE
        SET owner = EXCLUDED.owner,
            name = EXCLUDED.name,
            description = EXCLUDED.description,
            is_system = TRUE,
            updated_at = NOW()
        RETURNING id
        "#,
    )
    .bind(key)
    .bind(SYSTEM_ROLE_OWNER)
    .bind(name.trim())
    .bind(normalized_optional(description))
    .fetch_one(&mut *transaction)
    .await
    {
        Ok(role_id) => role_id,
        Err(error) => return Err(map_insert_error(error)),
    };
    let permission_ids = permission_ids_for_keys(permission_keys, &mut transaction).await?;
    replace_permissions_in_transaction(role_id, permission_ids.as_slice(), &mut transaction)
        .await?;
    transaction.commit().await?;
    query_by_id_for_owner(SYSTEM_ROLE_OWNER, role_id, pool)
        .await?
        .ok_or(StoreError::NotFound("角色"))
}

pub(super) async fn query_system_role_id(
    role_key: &str,
    lock: bool,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<i64, StoreError> {
    let role_id = if lock {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id
            FROM account.roles
            WHERE key = $1 AND owner = $2 AND is_system
            FOR UPDATE
            "#,
        )
        .bind(role_key)
        .bind(SYSTEM_ROLE_OWNER)
        .fetch_optional(&mut **transaction)
        .await?
    } else {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id
            FROM account.roles
            WHERE key = $1 AND owner = $2 AND is_system
            "#,
        )
        .bind(role_key)
        .bind(SYSTEM_ROLE_OWNER)
        .fetch_optional(&mut **transaction)
        .await?
    };
    role_id.ok_or(StoreError::InvalidData("系统角色"))
}

async fn attach_permissions(rows: Vec<RoleRow>, pool: &PgPool) -> Result<Vec<Role>, StoreError> {
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
    let mut permissions_by_role = HashMap::<i64, Vec<Permission>>::new();
    for row in permission_rows {
        let role_id = row.role_id;
        permissions_by_role
            .entry(role_id)
            .or_default()
            .push(row.into_permission()?);
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let permissions = permissions_by_role.remove(&row.id).unwrap_or_default();
            row.with_permissions(permissions)
        })
        .collect())
}

async fn replace_permissions_in_transaction(
    role_id: i64,
    permission_ids: &[i64],
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM account.role_permissions WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut **transaction)
        .await?;
    insert_permissions(role_id, permission_ids, transaction).await?;
    sqlx::query("UPDATE account.roles SET updated_at = NOW() WHERE id = $1")
        .bind(role_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn insert_permissions(
    role_id: i64,
    permission_ids: &[i64],
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let permission_ids = canonical_permission_ids(permission_ids, transaction).await?;
    if permission_ids.is_empty() {
        return Ok(());
    }
    let result = sqlx::query(
        r#"
        INSERT INTO account.role_permissions (role_id, permission_id)
        SELECT $1, id
        FROM account.permissions
        WHERE id = ANY($2)
        "#,
    )
    .bind(role_id)
    .bind(&permission_ids)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != permission_ids.len() as u64 {
        return Err(StoreError::NotFound("权限"));
    }
    Ok(())
}

async fn canonical_permission_ids(
    permission_ids: &[i64],
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<i64>, StoreError> {
    if permission_ids.is_empty() {
        return Ok(Vec::new());
    }

    let existing_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.permissions WHERE id = ANY($1)")
            .bind(permission_ids)
            .fetch_one(&mut **transaction)
            .await?;
    if existing_count != permission_ids.len() as i64 {
        return Err(StoreError::NotFound("权限"));
    }

    let expanded = sqlx::query_scalar::<_, i64>(
        r#"
        WITH RECURSIVE expanded(permission_id, path) AS (
            SELECT id, ARRAY[id]
            FROM account.permissions
            WHERE id = ANY($1)

            UNION

            SELECT implication.implied_permission_id,
                   expanded.path || implication.implied_permission_id
            FROM expanded
            JOIN account.permission_implications implication
              ON implication.permission_id = expanded.permission_id
            WHERE NOT implication.implied_permission_id = ANY(expanded.path)
        )
        SELECT DISTINCT permission_id
        FROM expanded
        ORDER BY permission_id
        "#,
    )
    .bind(permission_ids)
    .fetch_all(&mut **transaction)
    .await?;

    Ok(expanded)
}

async fn permission_ids_for_keys(
    permission_keys: &[String],
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<i64>, StoreError> {
    if permission_keys.is_empty() {
        return Ok(Vec::new());
    }
    let permission_ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM account.permissions
        WHERE key = ANY($1)
        ORDER BY key
        "#,
    )
    .bind(permission_keys)
    .fetch_all(&mut **transaction)
    .await?;
    if permission_ids.len() != permission_keys.len() {
        return Err(StoreError::NotFound("权限"));
    }
    Ok(permission_ids)
}

async fn ensure_mutable_for_owner(
    owner: &str,
    role_id: i64,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let is_system = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT is_system
        FROM account.roles
        WHERE owner = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(owner)
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

async fn classify_mutation_for_owner(
    owner: &str,
    role_id: i64,
    pool: &PgPool,
) -> Result<StoreError, StoreError> {
    let row = sqlx::query_as::<_, (String, bool)>(
        r#"
        SELECT owner, is_system
        FROM account.roles
        WHERE id = $1
        "#,
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((stored_owner, true)) if stored_owner == owner => StoreError::SystemRole,
        Some((stored_owner, false)) if stored_owner == owner => {
            StoreError::Conflict("role_not_modified")
        }
        Some(_) | None => StoreError::NotFound("角色"),
    })
}

fn map_insert_error(error: sqlx::Error) -> StoreError {
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

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23503")
}

fn normalized_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
