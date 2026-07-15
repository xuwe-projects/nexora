//! PostgreSQL 账号 store。
//!
//! 本模块是账号业务中唯一允许持有 `PgPool`、执行 SQLx 查询和管理事务的位置。

mod identity;
mod roles;
mod users;

use std::collections::BTreeSet;

use async_trait::async_trait;
use kernel::{Page, PageRequest};
use sqlx::PgPool;
use uuid::Uuid;

use self::{
    identity::{grant_system_role, remove_non_super_admin_roles, upsert_identity},
    roles::{
        classify_role_mutation, ensure_mutable_role, insert_role_permissions,
        is_foreign_key_violation, map_role_insert_error, normalized_optional, roles_for_user,
        roles_with_permissions, system_role_id,
    },
    users::{lock_active_administrators, protect_active_administrator},
};
use crate::{
    AccessProfile, AccountsStore, CreateRole, ExternalIdentity, Permission, Role, StoreError,
    UpdateRole, User, UserStatus,
    entities::account::{DatabaseUserStatus, PermissionRow, UserRow},
};

const SUPER_ADMIN_LOCK_KEY: i64 = 0x5855_5745_5355_5045;

/// 使用 SQLx 连接池实现的 PostgreSQL 账号 store。
#[derive(Debug, Clone)]
pub struct PostgresAccountsStore {
    pool: PgPool,
}

impl PostgresAccountsStore {
    /// 使用应用状态已经建立的 PostgreSQL 连接池创建 store。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AccountsStore for PostgresAccountsStore {
    async fn sync_identity(&self, identity: &ExternalIdentity) -> Result<User, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = upsert_identity(&mut transaction, identity).await?;

        if !row.is_super_admin {
            grant_system_role(&mut transaction, row.id, "member").await?;
        }
        transaction.commit().await?;
        row.try_into()
    }

    async fn super_admin(&self) -> Result<Option<User>, StoreError> {
        sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, issuer, subject, email, display_name, avatar_url, status,
                   is_super_admin, created_at, updated_at, last_login_at
            FROM account.users
            WHERE is_super_admin
            "#,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(User::try_from)
        .transpose()
    }

    async fn bind_super_admin(&self, identity: &ExternalIdentity) -> Result<User, StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(SUPER_ADMIN_LOCK_KEY)
            .execute(&mut *transaction)
            .await?;

        let existing = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, issuer, subject, email, display_name, avatar_url, status,
                   is_super_admin, created_at, updated_at, last_login_at
            FROM account.users
            WHERE is_super_admin
            FOR UPDATE
            "#,
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if existing.as_ref().is_some_and(|user| {
            user.issuer != identity.issuer.trim() || user.subject != identity.subject.trim()
        }) {
            return Err(StoreError::SuperAdministratorAlreadyBound);
        }

        let row = upsert_identity(&mut transaction, identity).await?;
        // 角色触发器会在超级管理员标记生效后冻结授权，因此必须先收敛旧角色。
        remove_non_super_admin_roles(&mut transaction, row.id).await?;
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            UPDATE account.users
            SET is_super_admin = TRUE, status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, issuer, subject, email, display_name, avatar_url, status,
                      is_super_admin, created_at, updated_at, last_login_at
            "#,
        )
        .bind(row.id)
        .bind(DatabaseUserStatus::Active)
        .fetch_one(&mut *transaction)
        .await?;
        grant_system_role(&mut transaction, row.id, "member").await?;
        grant_system_role(&mut transaction, row.id, "super-administrator").await?;
        transaction.commit().await?;
        row.try_into()
    }

    async fn access_profile(&self, user_id: Uuid) -> Result<AccessProfile, StoreError> {
        let user = self.user(user_id).await?;
        let roles = roles_for_user(&self.pool, user_id).await?;
        let permissions = if user.is_super_admin {
            sqlx::query_scalar::<_, String>("SELECT key FROM account.permissions ORDER BY key")
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_scalar::<_, String>(
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
            .fetch_all(&self.pool)
            .await?
        }
        .into_iter()
        .collect::<BTreeSet<_>>();
        Ok(AccessProfile {
            user,
            roles,
            permissions,
        })
    }

    async fn list_users(&self, request: PageRequest) -> Result<Page<User>, StoreError> {
        let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.users")
            .fetch_one(&self.pool)
            .await?;
        let offset = i64::from(request.number().saturating_sub(1)) * i64::from(request.size());
        let rows = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, issuer, subject, email, display_name, avatar_url, status,
                   is_super_admin, created_at, updated_at, last_login_at
            FROM account.users
            ORDER BY created_at DESC, id DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(i64::from(request.size()))
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(User::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::new(items, total, request))
    }

    async fn user(&self, user_id: Uuid) -> Result<User, StoreError> {
        sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, issuer, subject, email, display_name, avatar_url, status,
                   is_super_admin, created_at, updated_at, last_login_at
            FROM account.users
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound("用户"))?
        .try_into()
    }

    async fn set_user_status(&self, user_id: Uuid, status: UserStatus) -> Result<User, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let administrator_role_id = system_role_id(&mut transaction, "administrator", true).await?;
        let (current_status, is_super_admin) = sqlx::query_as::<_, (DatabaseUserStatus, bool)>(
            "SELECT status, is_super_admin FROM account.users WHERE id = $1 FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound("用户"))?;
        let database_status = DatabaseUserStatus::from(status);

        if is_super_admin && current_status != database_status {
            return Err(StoreError::SuperAdministratorImmutable);
        }

        if current_status != database_status && status == UserStatus::Suspended {
            protect_active_administrator(&mut transaction, user_id, administrator_role_id).await?;
        }

        let row = sqlx::query_as::<_, UserRow>(
            r#"
            UPDATE account.users
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, issuer, subject, email, display_name, avatar_url, status,
                      is_super_admin, created_at, updated_at, last_login_at
            "#,
        )
        .bind(user_id)
        .bind(database_status)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        row.try_into()
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        roles_with_permissions(&self.pool, None).await
    }

    async fn role(&self, role_id: Uuid) -> Result<Role, StoreError> {
        roles_with_permissions(&self.pool, Some(role_id))
            .await?
            .pop()
            .ok_or(StoreError::NotFound("角色"))
    }

    async fn create_role(&self, input: &CreateRole) -> Result<Role, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let role_id = match sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO account.roles (key, name, description)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(input.key.as_str())
        .bind(input.name.trim())
        .bind(normalized_optional(input.description.as_deref()))
        .fetch_one(&mut *transaction)
        .await
        {
            Ok(role_id) => role_id,
            Err(error) => return Err(map_role_insert_error(error)),
        };
        insert_role_permissions(&mut transaction, role_id, &input.permission_ids).await?;
        transaction.commit().await?;
        self.role(role_id).await
    }

    async fn update_role(&self, role_id: Uuid, input: &UpdateRole) -> Result<Role, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE account.roles
            SET name = COALESCE($2, name),
                description = CASE WHEN $3 THEN $4 ELSE description END,
                updated_at = NOW()
            WHERE id = $1 AND NOT is_system
            "#,
        )
        .bind(role_id)
        .bind(input.name.as_deref().map(str::trim))
        .bind(input.description.is_some())
        .bind(
            input
                .description
                .as_ref()
                .and_then(|value| normalized_optional(value.as_deref())),
        )
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(classify_role_mutation(&self.pool, role_id).await?);
        }
        self.role(role_id).await
    }

    async fn delete_role(&self, role_id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM account.roles WHERE id = $1 AND NOT is_system")
            .bind(role_id)
            .execute(&self.pool)
            .await;
        match result {
            Ok(result) if result.rows_affected() == 1 => Ok(()),
            Ok(_) => Err(classify_role_mutation(&self.pool, role_id).await?),
            Err(error) if is_foreign_key_violation(&error) => {
                Err(StoreError::Conflict("role_in_use"))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn replace_role_permissions(
        &self,
        role_id: Uuid,
        permission_ids: &[Uuid],
    ) -> Result<Role, StoreError> {
        let mut transaction = self.pool.begin().await?;
        ensure_mutable_role(&mut transaction, role_id).await?;
        sqlx::query("DELETE FROM account.role_permissions WHERE role_id = $1")
            .bind(role_id)
            .execute(&mut *transaction)
            .await?;
        insert_role_permissions(&mut transaction, role_id, permission_ids).await?;
        sqlx::query("UPDATE account.roles SET updated_at = NOW() WHERE id = $1")
            .bind(role_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        self.role(role_id).await
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        let rows = sqlx::query_as::<_, PermissionRow>(
            "SELECT id, key, name, description FROM account.permissions ORDER BY key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Permission::from).collect())
    }

    async fn replace_user_roles(
        &self,
        user_id: Uuid,
        role_ids: &[Uuid],
        granted_by: Uuid,
    ) -> Result<AccessProfile, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let administrator_role_id = system_role_id(&mut transaction, "administrator", true).await?;
        let (target_status, is_super_admin) = sqlx::query_as::<_, (DatabaseUserStatus, bool)>(
            "SELECT status, is_super_admin FROM account.users WHERE id = $1 FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound("用户"))?;
        if is_super_admin {
            return Err(StoreError::SuperAdministratorImmutable);
        }
        let member_role_id = system_role_id(&mut transaction, "member", false).await?;

        let requested = role_ids.to_vec();
        let existing_role_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.roles WHERE id = ANY($1)")
                .bind(&requested)
                .fetch_one(&mut *transaction)
                .await?;
        if existing_role_count != requested.len() as i64 {
            return Err(StoreError::NotFound("角色"));
        }
        let contains_reserved_role = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM account.roles
                WHERE id = ANY($1) AND key = 'super-administrator'
            )
            "#,
        )
        .bind(&requested)
        .fetch_one(&mut *transaction)
        .await?;
        if contains_reserved_role {
            return Err(StoreError::SuperAdministratorRoleReserved);
        }

        let currently_administrator = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM account.user_roles WHERE user_id = $1 AND role_id = $2
            )
            "#,
        )
        .bind(user_id)
        .bind(administrator_role_id)
        .fetch_one(&mut *transaction)
        .await?;
        if currently_administrator
            && target_status == DatabaseUserStatus::Active
            && !requested.contains(&administrator_role_id)
        {
            let active_administrators =
                lock_active_administrators(&mut transaction, administrator_role_id).await?;
            if active_administrators.len() <= 1 {
                return Err(StoreError::LastAdministrator);
            }
        }

        let mut desired = requested;
        desired.push(member_role_id);
        desired.sort_unstable();
        desired.dedup();

        sqlx::query("DELETE FROM account.user_roles WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO account.user_roles (user_id, role_id, granted_by)
            SELECT $1, requested.role_id, $3
            FROM UNNEST($2::uuid[]) AS requested(role_id)
            "#,
        )
        .bind(user_id)
        .bind(&desired)
        .bind(granted_by)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.access_profile(user_id).await
    }
}
