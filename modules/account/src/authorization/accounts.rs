//! 账号、用户、角色与权限资源的权限标记。

use crate::PermissionKey;

use super::RequiredPermission;

/// 查看用户资源所需的权限标记。
pub struct ReadUsers;

impl RequiredPermission for ReadUsers {
    const KEY: PermissionKey = PermissionKey::UsersRead;
}

/// 管理用户角色所需的权限标记。
pub struct WriteUserRoles;

impl RequiredPermission for WriteUserRoles {
    const KEY: PermissionKey = PermissionKey::UsersRolesWrite;
}

/// 管理用户状态所需的权限标记。
pub struct WriteUserStatus;

impl RequiredPermission for WriteUserStatus {
    const KEY: PermissionKey = PermissionKey::UsersStatusWrite;
}

/// 显式开通可信外部身份所需的权限标记。
/// 绠＄悊鐢ㄦ埛澶村儚鎵€闇€鐨勬潈闄愭爣璁般€?
pub struct WriteUserAvatar;

impl RequiredPermission for WriteUserAvatar {
    const KEY: PermissionKey = PermissionKey::UsersAvatarWrite;
}

pub struct ProvisionUsers;

impl RequiredPermission for ProvisionUsers {
    const KEY: PermissionKey = PermissionKey::UsersProvision;
}

/// 查看角色资源所需的权限标记。
pub struct ReadRoles;

impl RequiredPermission for ReadRoles {
    const KEY: PermissionKey = PermissionKey::RolesRead;
}

/// 管理角色及角色权限所需的权限标记。
pub struct WriteRoles;

impl RequiredPermission for WriteRoles {
    const KEY: PermissionKey = PermissionKey::RolesWrite;
}

/// 查看权限目录所需的权限标记。
pub struct ReadPermissions;

impl RequiredPermission for ReadPermissions {
    const KEY: PermissionKey = PermissionKey::PermissionsRead;
}
