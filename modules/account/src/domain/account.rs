//! 账号、身份、角色与权限领域模型。

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 账号模块使用的稳定权限键。
pub mod permission {
    /// 查看用户列表、用户详情及其角色。
    pub const USERS_READ: &str = "users:read";
    /// 为用户授予或撤销角色。
    pub const USERS_ROLES_WRITE: &str = "users:roles.write";
    /// 启用或停用用户访问。
    pub const USERS_STATUS_WRITE: &str = "users:status.write";
    /// 查看角色及角色包含的权限。
    pub const ROLES_READ: &str = "roles:read";
    /// 创建、修改、删除自定义角色并配置权限。
    pub const ROLES_WRITE: &str = "roles:write";
    /// 查看系统支持的权限目录。
    pub const PERMISSIONS_READ: &str = "permissions:read";
}

/// 已通过外部身份提供方验证、等待同步到本地账号的数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    /// 身份提供方的规范 OIDC issuer URL。
    pub issuer: String,
    /// issuer 内稳定且唯一的用户 subject。
    pub subject: String,
    /// 身份提供方返回的可选邮箱。
    pub email: Option<String>,
    /// 面向用户界面展示的名称。
    pub display_name: String,
    /// 身份提供方返回的可选头像 URL。
    pub avatar_url: Option<String>,
}

/// 本地用户是否允许访问受保护资源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    /// 用户可以正常认证并参与授权判断。
    Active,
    /// 用户记录继续保留，但不能访问受保护资源。
    Suspended,
}

/// 与外部身份绑定的本地用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// 本地生成的稳定用户 ID。
    pub id: Uuid,
    /// 身份提供方的规范 OIDC issuer URL。
    pub issuer: String,
    /// issuer 内稳定且唯一的用户 subject。
    pub subject: String,
    /// 可选展示邮箱。
    pub email: Option<String>,
    /// 用户展示名称。
    pub display_name: String,
    /// 可选头像 URL。
    pub avatar_url: Option<String>,
    /// 用户当前访问状态。
    pub status: UserStatus,
    /// 是否为系统唯一且不可变的内置超级管理员。
    pub is_super_admin: bool,
    /// 本地用户首次创建时间。
    pub created_at: DateTime<Utc>,
    /// 本地用户资料最后更新时间。
    pub updated_at: DateTime<Utc>,
    /// 最近一次成功认证并同步身份的时间。
    pub last_login_at: DateTime<Utc>,
}

/// 可被角色授予的细粒度权限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// 权限的稳定 ID。
    pub id: Uuid,
    /// 授权判断使用的稳定权限键。
    pub key: String,
    /// 面向管理界面展示的权限名称。
    pub name: String,
    /// 可选的权限用途说明。
    pub description: Option<String>,
}

/// 一组可授予用户的权限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// 角色的稳定 ID。
    pub id: Uuid,
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

/// 创建自定义角色时使用的领域输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRole {
    /// 新角色的稳定业务键。
    pub key: String,
    /// 新角色的展示名称。
    pub name: String,
    /// 新角色的可选用途说明。
    pub description: Option<String>,
    /// 创建时直接授予角色的权限 ID。
    pub permission_ids: Vec<Uuid>,
}

/// 局部修改自定义角色元数据时使用的领域输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRole {
    /// 可选的新展示名称；`None` 表示保持原值。
    pub name: Option<String>,
    /// 角色说明的三态更新值：外层 `None` 保持原值，`Some(None)` 清空，
    /// `Some(Some(value))` 设置新值。
    pub description: Option<Option<String>>,
}

/// 用户、直接角色和合并后权限组成的授权快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessProfile {
    /// 当前本地用户。
    pub user: User,
    /// 直接授予当前用户的角色。
    pub roles: Vec<Role>,
    /// 当前用户最终拥有的去重权限键。
    pub permissions: BTreeSet<String>,
}

impl AccessProfile {
    /// 判断授权快照是否包含指定稳定权限键。
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }
}
