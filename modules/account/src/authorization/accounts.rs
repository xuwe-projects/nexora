//! 账号、用户、角色与权限资源的权限标记。

use crate::permission;

use super::RequiredPermission;

/// 查看用户资源所需的权限标记。
pub struct ReadUsers;

impl RequiredPermission for ReadUsers {
    const KEY: &'static str = permission::USERS_READ;
}

/// 管理用户角色所需的权限标记。
pub struct WriteUserRoles;

impl RequiredPermission for WriteUserRoles {
    const KEY: &'static str = permission::USERS_ROLES_WRITE;
}

/// 管理用户状态所需的权限标记。
pub struct WriteUserStatus;

impl RequiredPermission for WriteUserStatus {
    const KEY: &'static str = permission::USERS_STATUS_WRITE;
}

/// 查看角色资源所需的权限标记。
pub struct ReadRoles;

impl RequiredPermission for ReadRoles {
    const KEY: &'static str = permission::ROLES_READ;
}

/// 管理角色及角色权限所需的权限标记。
pub struct WriteRoles;

impl RequiredPermission for WriteRoles {
    const KEY: &'static str = permission::ROLES_WRITE;
}

/// 查看权限目录所需的权限标记。
pub struct ReadPermissions;

impl RequiredPermission for ReadPermissions {
    const KEY: &'static str = permission::PERMISSIONS_READ;
}
