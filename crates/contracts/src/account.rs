//! 账号、角色、权限和授权快照的请求与响应契约。

use serde::{Deserialize, Serialize};

use crate::{pagination::PageResponse, patch::PatchField};

/// 管理员在身份目录创建人类用户并绑定本地账号的请求正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProvisionUserRequest {
    /// 身份提供方中的登录用户名，在组织内必须唯一。
    pub username: String,
    /// 用户名字；由身份提供方用于建立人类用户资料。
    pub given_name: String,
    /// 用户姓氏；由身份提供方用于建立人类用户资料。
    pub family_name: String,
    /// 登录与验证使用的主邮箱。
    pub email: String,
    /// 可选展示名称；省略时使用名字与姓氏组合。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 创建用户时直接授予的角色 ID 集合；省略时使用 Account 的默认成员角色。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_ids: Vec<i64>,
}

/// 创建自定义角色的请求正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateRoleRequest {
    /// 在 API 和授权规则中使用的稳定角色键。
    pub key: String,
    /// 面向管理界面展示的角色名称。
    pub name: String,
    /// 可选的角色用途说明。
    pub description: Option<String>,
    /// 创建时直接授予角色的权限 ID 集合。
    #[serde(default)]
    pub permission_ids: Vec<i64>,
}

/// 局部更新自定义角色元数据的请求正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateRoleRequest {
    /// 可选的新角色名称；字段缺失时保持原值。
    pub name: Option<String>,
    /// 角色说明的三态更新值，区分保持、清空和设置。
    #[serde(default, skip_serializing_if = "PatchField::is_missing")]
    pub description: PatchField<String>,
}

/// 完整替换角色权限集合的请求正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplaceRolePermissionsRequest {
    /// 替换后角色应当直接包含的权限 ID 集合。
    pub permission_ids: Vec<i64>,
}

/// 完整替换用户直接角色集合的请求正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplaceUserRolesRequest {
    /// 替换后用户应当直接拥有的角色 ID 集合。
    pub role_ids: Vec<i64>,
}

/// 用户是否允许访问受保护 API 的公开状态。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    /// 用户可以正常认证并参与授权判断。
    Active,
    /// 用户记录继续保留，但受保护 API 会拒绝其访问。
    Suspended,
}

/// 修改用户访问状态的请求正文。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateUserStatusRequest {
    /// 请求设置的目标用户状态。
    pub status: UserStatus,
}

/// 当前用户及其合并后角色和权限的响应。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AccessProfileResponse {
    /// 当前本地用户摘要。
    pub user: UserResponse,
    /// 直接授予当前用户的角色。
    pub roles: Vec<RoleResponse>,
    /// 当前用户最终拥有的去重权限键。
    pub permissions: Vec<String>,
}

/// API 对外公开的用户表示。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct UserResponse {
    /// 本地生成的 8 位大小写字母与数字用户 ID。
    pub id: String,
    /// 当前部署绑定的 OIDC issuer 中与用户对应的稳定唯一 ID（subject）。
    pub identity_id: String,
    /// 身份提供方中的可选登录用户名。
    #[serde(default)]
    pub username: Option<String>,
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
    /// 本地用户首次创建时间的 Unix 秒时间戳。
    pub created_at: i64,
    /// 本地资料最后更新时间的 Unix 秒时间戳。
    pub updated_at: i64,
    /// 最近一次成功认证并同步身份的 Unix 秒时间戳。
    pub last_login_at: i64,
}

/// API 对外公开的角色及其直接权限表示。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RoleResponse {
    /// 数据库生成的 BIGSERIAL 角色 ID。
    pub id: i64,
    /// 在授权判断中使用的稳定角色键。
    pub key: String,
    /// 面向管理界面展示的角色名称。
    pub name: String,
    /// 可选的角色用途说明。
    pub description: Option<String>,
    /// 是否为不可编辑和删除的系统角色。
    pub is_system: bool,
    /// 当前角色直接包含的权限。
    pub permissions: Vec<PermissionResponse>,
    /// 角色创建时间的 Unix 秒时间戳。
    pub created_at: i64,
    /// 角色最后更新时间的 Unix 秒时间戳。
    pub updated_at: i64,
}

/// API 对外公开的细粒度权限表示。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PermissionResponse {
    /// 数据库生成的 BIGSERIAL 权限 ID。
    pub id: i64,
    /// 权限稳定键，例如 `users:read`。
    pub key: String,
    /// 面向管理界面展示的权限名称。
    pub name: String,
    /// 可选的权限用途说明。
    pub description: Option<String>,
}

/// 后台用户列表的页码分页响应。
pub type UserPageResponse = PageResponse<UserResponse>;
