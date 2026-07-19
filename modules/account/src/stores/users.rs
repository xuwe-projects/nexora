//! 用户、用户状态与用户角色数据访问函数。

use std::collections::BTreeSet;

use kernel::{Page, PageRequest};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    StoreError,
    entities::account::{AccessProfile, PermissionKey, User, UserStatus},
    stores::roles,
};

/// 按用户 ID 返回本地用户实体。
pub(crate) async fn query_by_id(user_id: &str, pool: &PgPool) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, identity_id, username, email, display_name, avatar_url, status,
               is_super_admin, created_at, updated_at, last_login_at
        FROM account.users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// 按外部身份目录的稳定 identity ID 返回本地用户实体。
pub(crate) async fn query_by_identity_id(
    identity_id: &str,
    pool: &PgPool,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, identity_id, username, email, display_name, avatar_url, status,
               is_super_admin, created_at, updated_at, last_login_at
        FROM account.users
        WHERE identity_id = $1
        "#,
    )
    .bind(identity_id)
    .fetch_optional(pool)
    .await
}

/// 按页码返回本地用户实体。
pub(crate) async fn query_page(
    request: PageRequest,
    pool: &PgPool,
) -> Result<Page<User>, sqlx::Error> {
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.users")
        .fetch_one(pool)
        .await?;
    let offset = i64::from(request.number().saturating_sub(1)) * i64::from(request.size());
    let items = sqlx::query_as::<_, User>(
        r#"
        SELECT id, identity_id, username, email, display_name, avatar_url, status,
               is_super_admin, created_at, updated_at, last_login_at
        FROM account.users
        ORDER BY created_at DESC, id DESC
        LIMIT $1 OFFSET $2
        "#,
    )
    .bind(i64::from(request.size()))
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(Page::new(items, total, request))
}

/// 返回指定用户及其直接角色和合并权限。
pub(crate) async fn query_access_profile(
    user_id: &str,
    pool: &PgPool,
) -> Result<Option<AccessProfile>, StoreError> {
    let Some(user) = query_by_id(user_id, pool).await? else {
        return Ok(None);
    };
    let assigned_roles = roles::query_for_user(user_id, pool).await?;
    let permissions = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT permissions.key
        FROM account.permissions
        JOIN account.role_permissions ON role_permissions.permission_id = permissions.id
        JOIN account.user_roles ON user_roles.role_id = role_permissions.role_id
        WHERE user_roles.user_id = $1
        ORDER BY permissions.key
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|key| {
        PermissionKey::try_from(key.as_str()).map_err(|()| StoreError::InvalidData("权限键"))
    })
    .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(Some(AccessProfile {
        user,
        roles: assigned_roles,
        permissions,
    }))
}

/// 修改用户状态，并保护超级管理员和最后一个启用管理员。
pub(crate) async fn update_status(
    user_id: &str,
    status: UserStatus,
    pool: &PgPool,
) -> Result<User, StoreError> {
    let mut transaction = pool.begin().await?;
    let administrator_role_id =
        roles::query_system_role_id("admin", true, &mut transaction).await?;
    let (current_status, is_super_admin, username, email) =
        sqlx::query_as::<_, (UserStatus, bool, Option<String>, Option<String>)>(
            r#"
        SELECT status, is_super_admin, username, email
        FROM account.users
        WHERE id = $1
        FOR UPDATE
        "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound("用户"))?;
    if is_service_account(is_super_admin, username.as_deref(), email.as_deref()) {
        return Err(StoreError::ServiceAccountImmutable);
    }
    if is_super_admin && current_status != status {
        return Err(StoreError::SuperAdministratorImmutable);
    }
    if current_status != status && status == UserStatus::Suspended {
        protect_active_administrator(user_id, administrator_role_id, &mut transaction).await?;
    }
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET status = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING id, identity_id, username, email, display_name, avatar_url, status,
                  is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(user_id)
    .bind(status)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(user)
}

/// 原子替换用户直接角色，并始终保留 `member` 角色。
pub(crate) async fn replace_roles(
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
    pool: &PgPool,
) -> Result<AccessProfile, StoreError> {
    let mut transaction = pool.begin().await?;
    let administrator_role_id =
        roles::query_system_role_id("admin", true, &mut transaction).await?;
    let (target_status, is_super_admin, username, email) =
        sqlx::query_as::<_, (UserStatus, bool, Option<String>, Option<String>)>(
            r#"
        SELECT status, is_super_admin, username, email
        FROM account.users
        WHERE id = $1
        FOR UPDATE
        "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound("用户"))?;
    if is_super_admin {
        return Err(StoreError::SuperAdministratorImmutable);
    }
    if is_service_account(is_super_admin, username.as_deref(), email.as_deref()) {
        return Err(StoreError::ServiceAccountImmutable);
    }
    let desired_role_ids = desired_role_ids(role_ids, &mut transaction).await?;
    let currently_administrator = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM account.user_roles
            WHERE user_id = $1 AND role_id = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(administrator_role_id)
    .fetch_one(&mut *transaction)
    .await?;
    if currently_administrator
        && target_status == UserStatus::Active
        && !role_ids.contains(&administrator_role_id)
        && lock_active_administrators(administrator_role_id, &mut transaction)
            .await?
            .len()
            <= 1
    {
        return Err(StoreError::LastAdministrator);
    }

    sqlx::query("DELETE FROM account.user_roles WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    insert_role_grants(
        user_id,
        desired_role_ids.as_slice(),
        granted_by,
        &mut transaction,
    )
    .await?;
    transaction.commit().await?;
    query_access_profile(user_id, pool)
        .await?
        .ok_or(StoreError::NotFound("用户"))
}

pub(super) async fn grant_initial_roles(
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let desired_role_ids = desired_role_ids(role_ids, transaction).await?;
    insert_role_grants(
        user_id,
        desired_role_ids.as_slice(),
        granted_by,
        transaction,
    )
    .await
}

async fn desired_role_ids(
    role_ids: &[i64],
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<i64>, StoreError> {
    let member_role_id = roles::query_system_role_id("member", false, transaction).await?;
    let existing_role_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.roles WHERE id = ANY($1)")
            .bind(role_ids)
            .fetch_one(&mut **transaction)
            .await?;
    if existing_role_count != role_ids.len() as i64 {
        return Err(StoreError::NotFound("角色"));
    }
    let mut desired_role_ids = role_ids.to_vec();
    desired_role_ids.push(member_role_id);
    desired_role_ids.sort_unstable();
    desired_role_ids.dedup();
    Ok(desired_role_ids)
}

async fn insert_role_grants(
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    sqlx::query(
        r#"
        INSERT INTO account.user_roles (user_id, role_id, granted_by)
        SELECT $1, requested.role_id, $3
        FROM UNNEST($2::bigint[]) AS requested(role_id)
        "#,
    )
    .bind(user_id)
    .bind(role_ids)
    .bind(granted_by)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn is_service_account(is_super_admin: bool, username: Option<&str>, email: Option<&str>) -> bool {
    !is_super_admin && username.is_none() && email.is_none()
}

async fn protect_active_administrator(
    user_id: &str,
    administrator_role_id: i64,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let is_administrator = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM account.user_roles
            WHERE user_id = $1 AND role_id = $2
        )
        "#,
    )
    .bind(user_id)
    .bind(administrator_role_id)
    .fetch_one(&mut **transaction)
    .await?;
    if is_administrator
        && lock_active_administrators(administrator_role_id, transaction)
            .await?
            .len()
            <= 1
    {
        return Err(StoreError::LastAdministrator);
    }
    Ok(())
}

async fn lock_active_administrators(
    administrator_role_id: i64,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
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
    .bind(UserStatus::Active)
    .fetch_all(&mut **transaction)
    .await
}
