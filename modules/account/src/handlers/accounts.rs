//! 账号、角色与权限 HTTP handlers 模块入口及契约映射。

use contracts::{
    account::{
        AccessProfileResponse, PermissionResponse, RoleResponse, UserPageResponse, UserResponse,
        UserStatus as ApiUserStatus,
    },
    pagination::PageMetadata,
};
use kernel::{Page, PageRequest, ValidationError};

use crate::{AccessProfile, AccountError, Permission, Role, User, UserStatus};

pub(crate) mod me;
pub(crate) mod permissions;
pub(crate) mod roles;
pub(crate) mod users;

const MAX_PAGE_SIZE: u32 = 100;
const MAX_ROLE_NAME_LENGTH: usize = 100;
const MAX_DESCRIPTION_LENGTH: usize = 1_000;
const MAX_ROLE_PERMISSIONS: usize = 256;
const MAX_USER_ROLES: usize = 64;

pub(crate) fn page_request(page: u32, page_size: u32) -> Result<PageRequest, AccountError> {
    if page == 0 {
        return Err(invalid("page", "页码必须从 1 开始"));
    }
    PageRequest::new(page, page_size.clamp(1, MAX_PAGE_SIZE))
        .ok_or_else(|| invalid("page", "分页参数必须大于零"))
}

pub(crate) fn validate_role_key(key: &str) -> Result<(), AccountError> {
    let valid_length = (2..=64).contains(&key.len());
    let mut characters = key.chars();
    let valid_first = characters
        .next()
        .is_some_and(|value| value.is_ascii_lowercase());
    let valid_rest = characters.all(|value| {
        value.is_ascii_lowercase() || value.is_ascii_digit() || matches!(value, '.' | '_' | '-')
    });
    if valid_length && valid_first && valid_rest {
        Ok(())
    } else {
        Err(invalid(
            "key",
            "角色键必须为 2 到 64 位小写字母、数字、点、下划线或连字符，并以字母开头",
        ))
    }
}

pub(crate) fn validate_role_fields(
    name: &str,
    description: Option<&str>,
) -> Result<(), AccountError> {
    if name.trim().is_empty() || name.chars().count() > MAX_ROLE_NAME_LENGTH {
        return Err(invalid("name", "角色名称必须为 1 到 100 个字符"));
    }
    if description.is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_LENGTH) {
        return Err(invalid("description", "角色说明不能超过 1000 个字符"));
    }
    Ok(())
}

pub(crate) fn role_permission_ids(ids: Vec<i64>) -> Result<Vec<i64>, AccountError> {
    deduplicate_ids(ids, MAX_ROLE_PERMISSIONS, "permission_ids")
}

pub(crate) fn user_role_ids(ids: Vec<i64>) -> Result<Vec<i64>, AccountError> {
    deduplicate_ids(ids, MAX_USER_ROLES, "role_ids")
}

pub(super) fn user_status(status: ApiUserStatus) -> UserStatus {
    match status {
        ApiUserStatus::Active => UserStatus::Active,
        ApiUserStatus::Suspended => UserStatus::Suspended,
    }
}

pub(super) fn access_profile_response(profile: AccessProfile) -> AccessProfileResponse {
    AccessProfileResponse {
        user: user_response(profile.user),
        roles: profile.roles.into_iter().map(role_response).collect(),
        permissions: profile
            .permissions
            .into_iter()
            .map(|permission| permission.as_str().to_owned())
            .collect(),
    }
}

pub(super) fn user_response(user: User) -> UserResponse {
    UserResponse {
        id: user.id,
        identity_id: user.identity_id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        status: match user.status {
            UserStatus::Active => ApiUserStatus::Active,
            UserStatus::Suspended => ApiUserStatus::Suspended,
        },
        is_super_admin: user.is_super_admin,
        created_at: user.created_at.timestamp(),
        updated_at: user.updated_at.timestamp(),
        last_login_at: user.last_login_at.timestamp(),
    }
}

pub(super) fn role_response(role: Role) -> RoleResponse {
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

pub(super) fn permission_response(permission: Permission) -> PermissionResponse {
    PermissionResponse {
        id: permission.id,
        key: permission.key.as_str().to_owned(),
        name: permission.name,
        description: permission.description,
    }
}

pub(super) fn user_page_response(page: Page<User>) -> UserPageResponse {
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

fn deduplicate_ids(
    mut ids: Vec<i64>,
    maximum: usize,
    field: &'static str,
) -> Result<Vec<i64>, AccountError> {
    if ids.len() > maximum {
        return Err(invalid(field, "集合元素数量超过限制"));
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn invalid(field: &'static str, message: &'static str) -> AccountError {
    ValidationError::new(field, message).into()
}
