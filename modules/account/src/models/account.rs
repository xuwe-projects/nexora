//! 账号领域模型与公开 HTTP 契约之间的显式映射。
//!
//! 契约类型由 `contracts` crate 持有；本模块只在 API 边界执行转换，避免 `contracts`
//! 反向依赖包含 SQLx store 的 `accounts` crate。

use crate::{
    AccessProfile, CreateRole, Page, Permission, Role, UpdateRole, User,
    UserStatus as DomainUserStatus,
};

use contracts::account::{
    AccessProfileResponse, CreateRoleRequest, PermissionResponse, RoleResponse, UpdateRoleRequest,
    UserPageResponse, UserResponse, UserStatus,
};
use contracts::{pagination::PageMetadata, patch::PatchField};
pub(crate) fn create_role_input(request: CreateRoleRequest) -> CreateRole {
    CreateRole {
        key: request.key,
        name: request.name,
        description: request.description,
        permission_ids: request.permission_ids,
    }
}

pub(crate) fn update_role_input(request: UpdateRoleRequest) -> UpdateRole {
    UpdateRole {
        name: request.name,
        description: match request.description {
            PatchField::Missing => None,
            PatchField::Null => Some(None),
            PatchField::Value(value) => Some(Some(value)),
        },
    }
}

pub(crate) fn domain_user_status(status: UserStatus) -> DomainUserStatus {
    match status {
        UserStatus::Active => DomainUserStatus::Active,
        UserStatus::Suspended => DomainUserStatus::Suspended,
    }
}

pub(crate) fn access_profile_response(profile: AccessProfile) -> AccessProfileResponse {
    AccessProfileResponse {
        user: user_response(profile.user),
        roles: profile.roles.into_iter().map(role_response).collect(),
        permissions: profile.permissions.into_iter().collect(),
    }
}

pub(crate) fn user_response(user: User) -> UserResponse {
    UserResponse {
        id: user.id,
        issuer: user.issuer,
        subject: user.subject,
        email: user.email,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        status: match user.status {
            DomainUserStatus::Active => UserStatus::Active,
            DomainUserStatus::Suspended => UserStatus::Suspended,
        },
        is_super_admin: user.is_super_admin,
        created_at: user.created_at.timestamp(),
        updated_at: user.updated_at.timestamp(),
        last_login_at: user.last_login_at.timestamp(),
    }
}

pub(crate) fn role_response(role: Role) -> RoleResponse {
    RoleResponse {
        id: role.id,
        key: role.key,
        name: role.name,
        description: role.description,
        is_system: role.is_system,
        permissions: role
            .permissions
            .into_iter()
            .map(permission_response)
            .collect(),
        created_at: role.created_at.timestamp(),
        updated_at: role.updated_at.timestamp(),
    }
}

pub(crate) fn permission_response(permission: Permission) -> PermissionResponse {
    PermissionResponse {
        id: permission.id,
        key: permission.key,
        name: permission.name,
        description: permission.description,
    }
}

pub(crate) fn user_page_response(page: Page<User>) -> UserPageResponse {
    let (items, total, request) = page.into_parts();
    UserPageResponse {
        items: items.into_iter().map(user_response).collect(),
        page: PageMetadata {
            number: request.number(),
            size: request.size(),
            total,
        },
    }
}
