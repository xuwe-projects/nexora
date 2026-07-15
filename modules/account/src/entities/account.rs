//! PostgreSQL 查询实体与账号领域模型的映射。

use chrono::{DateTime, Utc};
use sqlx::{FromRow, Type};
use uuid::Uuid;

use crate::{Permission, Role, StoreError, User, UserStatus};

/// PostgreSQL `account.user_status` 对应的持久化枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Type)]
#[sqlx(type_name = "user_status", rename_all = "snake_case")]
pub(crate) enum DatabaseUserStatus {
    /// 用户可以正常认证并参与授权判断。
    Active,
    /// 用户记录保留，但不能访问受保护资源。
    Suspended,
}

impl From<DatabaseUserStatus> for UserStatus {
    fn from(status: DatabaseUserStatus) -> Self {
        match status {
            DatabaseUserStatus::Active => Self::Active,
            DatabaseUserStatus::Suspended => Self::Suspended,
        }
    }
}

impl From<UserStatus> for DatabaseUserStatus {
    fn from(status: UserStatus) -> Self {
        match status {
            UserStatus::Active => Self::Active,
            UserStatus::Suspended => Self::Suspended,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) issuer: String,
    pub(crate) subject: String,
    email: Option<String>,
    display_name: String,
    avatar_url: Option<String>,
    status: DatabaseUserStatus,
    pub(crate) is_super_admin: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_login_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = StoreError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            issuer: row.issuer,
            subject: row.subject,
            email: row.email,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            status: row.status.into(),
            is_super_admin: row.is_super_admin,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_login_at: row.last_login_at,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct RoleRow {
    pub(crate) id: Uuid,
    key: String,
    name: String,
    description: Option<String>,
    is_system: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RoleRow {
    pub(crate) fn with_permissions(self, permissions: Vec<Permission>) -> Role {
        Role {
            id: self.id,
            key: self.key,
            name: self.name,
            description: self.description,
            is_system: self.is_system,
            permissions,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct PermissionRow {
    id: Uuid,
    key: String,
    name: String,
    description: Option<String>,
}

impl From<PermissionRow> for Permission {
    fn from(row: PermissionRow) -> Self {
        Self {
            id: row.id,
            key: row.key,
            name: row.name,
            description: row.description,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct RolePermissionRow {
    pub(crate) role_id: Uuid,
    pub(crate) id: Uuid,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}
