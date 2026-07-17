//! 账号模块的数据库实体、持久化枚举与组合查询结果。

use std::{borrow::Cow, collections::BTreeSet, fmt};

use chrono::{DateTime, Utc};
use sqlx::{FromRow, Type};
use url::{Host, Url};

use crate::{AccountError, StoreError};

const MAX_IDENTITY_ID_LENGTH: usize = 255;
const MAX_IDENTITY_ISSUER_LENGTH: usize = 2_048;
const MAX_IDENTITY_NAME_LENGTH: usize = 200;
const MAX_EMAIL_LENGTH: usize = 320;
const MAX_AVATAR_URL_LENGTH: usize = 2_048;

/// 已通过认证授权服务验证、等待同步到本地用户表的身份资料。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    /// 当前部署绑定的 OIDC issuer 中与用户对应的稳定唯一 ID（subject）。
    pub identity_id: String,
    /// 身份服务返回的可选邮箱。
    pub email: Option<String>,
    /// 面向用户界面展示的名称。
    pub display_name: String,
    /// 身份服务返回的可选头像 URL。
    pub avatar_url: Option<String>,
}

impl ExternalIdentity {
    /// 去除稳定字段两侧空白并验证数据库字段长度约束。
    ///
    /// # Errors
    ///
    /// identity ID 或展示名为空，或任一字段超过数据库允许长度时返回
    /// [`AccountError::InvalidIdentity`]。
    pub(crate) fn normalized(&self) -> Result<Self, AccountError> {
        let identity_id = self.identity_id.trim();
        let display_name = self.display_name.trim();
        let valid = !identity_id.is_empty()
            && identity_id.len() <= MAX_IDENTITY_ID_LENGTH
            && !display_name.is_empty()
            && display_name.chars().count() <= MAX_IDENTITY_NAME_LENGTH
            && self
                .email
                .as_deref()
                .is_none_or(|email| email.len() <= MAX_EMAIL_LENGTH)
            && self
                .avatar_url
                .as_deref()
                .is_none_or(|url| url.len() <= MAX_AVATAR_URL_LENGTH);
        if !valid {
            return Err(AccountError::InvalidIdentity);
        }
        Ok(Self {
            identity_id: identity_id.to_owned(),
            email: self.email.clone(),
            display_name: display_name.to_owned(),
            avatar_url: self.avatar_url.clone(),
        })
    }
}

/// 账号模块使用的稳定权限键。
///
/// 内置权限通过关联常量提供；应用也可以注册符合相同命名规则的自定义权限，例如
/// `projects:read` 或 `invoices:approval.write`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionKey(Cow<'static, str>);

#[allow(
    non_upper_case_globals,
    reason = "保留早期公开枚举变体风格的源码兼容名称"
)]
impl PermissionKey {
    /// 从应用代码中的静态字符串声明权限标记。
    ///
    /// 该构造函数用于实现 [`crate::authorization::RequiredPermission`] 的关联常量；应用仍应
    /// 通过 [`crate::create_permissions`] 注册同名权限，并遵守小写资源与操作键格式。
    pub const fn from_static(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// 查看用户列表、用户详情及其角色。
    pub const UsersRead: Self = Self(Cow::Borrowed("users:read"));
    /// 为用户授予或撤销角色。
    pub const UsersRolesWrite: Self = Self(Cow::Borrowed("users:roles.write"));
    /// 启用或停用用户访问。
    pub const UsersStatusWrite: Self = Self(Cow::Borrowed("users:status.write"));
    /// 把经过管理员确认的外部身份显式开通为本地用户。
    pub const UsersProvision: Self = Self(Cow::Borrowed("users:provision"));
    /// 查看角色及角色包含的权限。
    pub const RolesRead: Self = Self(Cow::Borrowed("roles:read"));
    /// 创建、修改、删除自定义角色并配置权限。
    pub const RolesWrite: Self = Self(Cow::Borrowed("roles:write"));
    /// 查看系统支持的权限目录。
    pub const PermissionsRead: Self = Self(Cow::Borrowed("permissions:read"));

    /// 返回数据库和授权日志使用的稳定权限键。
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl TryFrom<&str> for PermissionKey {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim();
        let valid_segment = |segment: &str| {
            let mut characters = segment.chars();
            (2..=64).contains(&segment.len())
                && characters
                    .next()
                    .is_some_and(|character| character.is_ascii_lowercase())
                && characters.all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || matches!(character, '.' | '_' | '-')
                })
        };
        let valid = value.split_once(':').is_some_and(|(resource, action)| {
            !action.contains(':') && valid_segment(resource) && valid_segment(action)
        });
        if valid {
            Ok(Self(Cow::Owned(value.to_owned())))
        } else {
            Err(())
        }
    }
}

impl fmt::Display for PermissionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 应用注册一个权限目录项时提供的稳定定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDefinition {
    /// 授权判断使用的稳定键，例如 `projects:read`。
    pub key: String,
    /// 面向管理界面展示的权限名称。
    pub name: String,
    /// 可选的权限用途说明。
    pub description: Option<String>,
}

/// PostgreSQL `account.user_status` 对应的用户访问状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Type)]
#[sqlx(type_name = "account.user_status", rename_all = "snake_case")]
pub enum UserStatus {
    /// 用户可以正常认证并参与授权判断。
    Active,
    /// 用户记录继续保留，但不能访问受保护资源。
    Suspended,
}

/// `account.users` 查询返回的本地用户实体。
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct User {
    /// 本地生成的 8 位大小写字母与数字用户 ID。
    pub id: String,
    /// 当前部署绑定的 OIDC issuer 中与用户对应的稳定唯一 ID（subject）。
    pub identity_id: String,
    /// 可选展示邮箱。
    pub email: Option<String>,
    /// 用户展示名称。
    pub display_name: String,
    /// 可选头像 URL。
    pub avatar_url: Option<String>,
    /// 用户当前访问状态。
    pub status: UserStatus,
    /// 是否为不挂载角色和权限、直接绕过权限校验的超级管理员。
    pub is_super_admin: bool,
    /// 本地用户首次创建时间。
    pub created_at: DateTime<Utc>,
    /// 本地用户资料最后更新时间。
    pub updated_at: DateTime<Utc>,
    /// 最近一次成功认证并同步身份的时间。
    pub last_login_at: DateTime<Utc>,
}

pub(crate) fn normalized_identity_issuer(value: &str) -> Result<String, AccountError> {
    let mut issuer = Url::parse(value.trim()).map_err(|_| AccountError::InvalidIdentityIssuer)?;
    if issuer.host().is_none()
        || !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
        || issuer.as_str().len() > MAX_IDENTITY_ISSUER_LENGTH
    {
        return Err(AccountError::InvalidIdentityIssuer);
    }
    let secure = issuer.scheme() == "https"
        || (issuer.scheme() == "http"
            && match issuer.host() {
                Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                None => false,
            });
    if !secure {
        return Err(AccountError::InvalidIdentityIssuer);
    }
    let path = issuer.path().trim_end_matches('/').to_owned();
    issuer.set_path(if path.is_empty() { "/" } else { path.as_str() });
    Ok(issuer.to_string())
}

/// `account.permissions` 查询返回的权限实体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// 数据库生成的 BIGSERIAL 权限 ID。
    pub id: i64,
    /// 授权判断使用的稳定权限枚举。
    pub key: PermissionKey,
    /// 面向管理界面展示的权限名称。
    pub name: String,
    /// 可选的权限用途说明。
    pub description: Option<String>,
}

/// 角色及其直接权限组成的查询实体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// 数据库生成的 BIGSERIAL 角色 ID。
    pub id: i64,
    /// 业务规则和授权配置使用的稳定角色键。
    pub key: String,
    /// 面向管理界面展示的角色名称。
    pub name: String,
    /// 可选的角色用途说明。
    pub description: Option<String>,
    /// 是否为数据库预置且不可修改的系统角色。
    pub is_system: bool,
    /// 当前角色直接包含的权限。
    pub permissions: Vec<Permission>,
    /// 角色创建时间。
    pub created_at: DateTime<Utc>,
    /// 角色最后更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 首次系统初始化时需要同步到认证授权 Project 的本地系统角色定义。
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct SystemRole {
    /// 认证授权 token claim 与本地 RBAC 共用的稳定角色键。
    pub key: String,
    /// 面向认证授权管理界面展示的角色名称。
    pub name: String,
}

/// 用户、直接角色和合并权限组成的授权查询结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessProfile {
    /// 当前本地用户。
    pub user: User,
    /// 直接授予当前用户的角色。
    pub roles: Vec<Role>,
    /// 当前用户最终拥有的去重权限。
    pub permissions: BTreeSet<PermissionKey>,
}

impl AccessProfile {
    /// 判断用户是否可执行要求指定权限的操作。
    ///
    /// 超级管理员不依赖角色或权限记录而直接返回 `true`；普通用户必须通过角色拥有该权限。
    pub fn allows(&self, permission: PermissionKey) -> bool {
        self.user.is_super_admin || self.permissions.contains(&permission)
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct PermissionRow {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

impl TryFrom<PermissionRow> for Permission {
    type Error = StoreError;

    fn try_from(row: PermissionRow) -> Result<Self, Self::Error> {
        let key = PermissionKey::try_from(row.key.as_str())
            .map_err(|()| StoreError::InvalidData("权限键"))?;
        Ok(Self {
            id: row.id,
            key,
            name: row.name,
            description: row.description,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct RoleRow {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) is_system: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
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
pub(crate) struct RolePermissionRow {
    pub(crate) role_id: i64,
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

impl RolePermissionRow {
    pub(crate) fn into_permission(self) -> Result<Permission, StoreError> {
        PermissionRow {
            id: self.id,
            key: self.key,
            name: self.name,
            description: self.description,
        }
        .try_into()
    }
}
