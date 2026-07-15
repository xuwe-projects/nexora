//! 账号与 RBAC 用例编排。

use std::sync::Arc;

use url::Url;
use uuid::Uuid;

use kernel::{Page, PageRequest, ValidationError};

use crate::{
    AccessProfile, AccountError, AccountsStore, CreateRole, ExternalIdentity, Permission, Role,
    UpdateRole, User, UserStatus,
};

const MAX_PAGE_SIZE: u32 = 100;
const MAX_ISSUER_LENGTH: usize = 2_048;
const MAX_SUBJECT_LENGTH: usize = 255;
const MAX_IDENTITY_NAME_LENGTH: usize = 200;
const MAX_EMAIL_LENGTH: usize = 320;
const MAX_AVATAR_URL_LENGTH: usize = 2_048;
const MAX_ROLE_NAME_LENGTH: usize = 100;
const MAX_DESCRIPTION_LENGTH: usize = 1_000;
const MAX_ROLE_PERMISSIONS: usize = 256;
const MAX_USER_ROLES: usize = 64;

/// 账号、身份同步和 RBAC 管理用例的应用层门面。
///
/// 该类型只持有抽象 store，不直接执行 SQL 或管理连接池。
#[derive(Clone)]
pub struct AccountApplication {
    store: Arc<dyn AccountsStore>,
}

impl AccountApplication {
    /// 使用持久化端口创建 application 门面。
    pub fn new(store: Arc<dyn AccountsStore>) -> Self {
        Self { store }
    }

    /// 同步已验证外部身份并返回本地授权快照。
    ///
    /// # Errors
    ///
    /// issuer、subject 或展示名为空，用户已停用，或 store 操作失败时返回错误。
    pub async fn authenticate(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<AccessProfile, AccountError> {
        let identity = normalized_identity(identity)?;
        let user = self.store.sync_identity(&identity).await?;
        if user.status == UserStatus::Suspended {
            return Err(AccountError::UserSuspended);
        }
        Ok(self.store.access_profile(user.id).await?)
    }

    /// 返回当前唯一内置超级管理员；首次引导尚未完成时返回 `None`。
    ///
    /// # Errors
    ///
    /// Store 无法读取数据库或持久化数据无效时返回错误。
    pub async fn super_admin(&self) -> Result<Option<User>, AccountError> {
        Ok(self.store.super_admin().await?)
    }

    /// 把经过 ZITADEL 目录确认的外部身份绑定为唯一内置超级管理员。
    ///
    /// 绑定会移除该用户原有的普通管理员或自定义角色，只保留成员与超级管理员角色。
    /// 同一身份重复绑定是幂等的；一旦绑定成功，普通业务接口不能替换该身份、停用账号、
    /// 修改角色或删除账号。
    ///
    /// # Errors
    ///
    /// 身份字段无效、系统已绑定其他超级管理员或 store 事务失败时返回错误。
    pub async fn bind_super_admin(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<User, AccountError> {
        let identity = normalized_identity(identity)?;
        Ok(self.store.bind_super_admin(&identity).await?)
    }

    /// 检查授权快照是否包含指定权限。
    ///
    /// # Errors
    ///
    /// 缺少权限时返回 [`AccountError::Forbidden`]。
    pub fn require_permission(
        &self,
        profile: &AccessProfile,
        permission: &'static str,
    ) -> Result<(), AccountError> {
        profile
            .has_permission(permission)
            .then_some(())
            .ok_or(AccountError::Forbidden(permission))
    }

    /// 分页列出用户；页大小会被限制在 1 到 100。
    ///
    /// # Errors
    ///
    /// 页码为零或 store 操作失败时返回错误。
    pub async fn list_users(&self, page: u32, page_size: u32) -> Result<Page<User>, AccountError> {
        if page == 0 {
            return Err(invalid("page", "页码必须从 1 开始"));
        }
        let request = PageRequest::new(page, page_size.clamp(1, MAX_PAGE_SIZE))
            .ok_or_else(|| invalid("page", "分页参数必须大于零"))?;
        Ok(self.store.list_users(request).await?)
    }

    /// 返回指定用户及其授权快照。
    ///
    /// # Errors
    ///
    /// 用户不存在或 store 操作失败时返回错误。
    pub async fn user_access(&self, user_id: Uuid) -> Result<AccessProfile, AccountError> {
        Ok(self.store.access_profile(user_id).await?)
    }

    /// 修改用户状态。
    ///
    /// # Errors
    ///
    /// 用户不存在、操作会停用最后一个管理员或 store 操作失败时返回错误。
    pub async fn set_user_status(
        &self,
        user_id: Uuid,
        status: UserStatus,
    ) -> Result<User, AccountError> {
        Ok(self.store.set_user_status(user_id, status).await?)
    }

    /// 返回全部角色及其权限。
    ///
    /// # Errors
    ///
    /// Store 操作失败时返回错误。
    pub async fn list_roles(&self) -> Result<Vec<Role>, AccountError> {
        Ok(self.store.list_roles().await?)
    }

    /// 返回指定角色。
    ///
    /// # Errors
    ///
    /// 角色不存在或 store 操作失败时返回错误。
    pub async fn role(&self, role_id: Uuid) -> Result<Role, AccountError> {
        Ok(self.store.role(role_id).await?)
    }

    /// 创建一个自定义角色。
    ///
    /// # Errors
    ///
    /// 字段校验失败、角色键冲突、权限不存在或 store 操作失败时返回错误。
    pub async fn create_role(&self, mut input: CreateRole) -> Result<Role, AccountError> {
        validate_role_key(&input.key)?;
        validate_role_fields(&input.name, input.description.as_deref())?;
        deduplicate_ids(
            &mut input.permission_ids,
            MAX_ROLE_PERMISSIONS,
            "permission_ids",
        )?;
        Ok(self.store.create_role(&input).await?)
    }

    /// 修改自定义角色名称与说明。
    ///
    /// # Errors
    ///
    /// 字段校验失败、角色不存在、角色是系统角色或 store 操作失败时返回错误。
    pub async fn update_role(
        &self,
        role_id: Uuid,
        input: UpdateRole,
    ) -> Result<Role, AccountError> {
        if input.name.is_none() && input.description.is_none() {
            return Err(invalid("body", "至少需要提供一个要修改的角色字段"));
        }
        let current = self.store.role(role_id).await?;
        ensure_custom_role(&current)?;
        let final_name = input.name.as_deref().unwrap_or(current.name.as_str());
        let final_description = input
            .description
            .as_ref()
            .map(|value| value.as_deref())
            .unwrap_or(current.description.as_deref());
        validate_role_fields(final_name, final_description)?;
        Ok(self.store.update_role(role_id, &input).await?)
    }

    /// 删除自定义角色。
    ///
    /// # Errors
    ///
    /// 角色不存在、角色是系统角色、角色仍被用户引用或 store 操作失败时返回错误。
    pub async fn delete_role(&self, role_id: Uuid) -> Result<(), AccountError> {
        ensure_custom_role(&self.store.role(role_id).await?)?;
        Ok(self.store.delete_role(role_id).await?)
    }

    /// 原子替换自定义角色的权限集合。
    ///
    /// # Errors
    ///
    /// 角色不存在、角色是系统角色、权限数量超限、权限不存在或 store 操作失败时返回错误。
    pub async fn replace_role_permissions(
        &self,
        role_id: Uuid,
        mut permission_ids: Vec<Uuid>,
    ) -> Result<Role, AccountError> {
        ensure_custom_role(&self.store.role(role_id).await?)?;
        deduplicate_ids(&mut permission_ids, MAX_ROLE_PERMISSIONS, "permission_ids")?;
        Ok(self
            .store
            .replace_role_permissions(role_id, &permission_ids)
            .await?)
    }

    /// 返回完整权限目录。
    ///
    /// # Errors
    ///
    /// Store 操作失败时返回错误。
    pub async fn list_permissions(&self) -> Result<Vec<Permission>, AccountError> {
        Ok(self.store.list_permissions().await?)
    }

    /// 原子替换用户的直接角色集合。
    ///
    /// Store 会始终补回默认成员角色，并保护最后一个启用管理员。
    ///
    /// # Errors
    ///
    /// 角色数量超限、用户或角色不存在、操作会移除最后一个管理员，或 store 操作失败时返回错误。
    pub async fn replace_user_roles(
        &self,
        user_id: Uuid,
        mut role_ids: Vec<Uuid>,
        granted_by: Uuid,
    ) -> Result<AccessProfile, AccountError> {
        deduplicate_ids(&mut role_ids, MAX_USER_ROLES, "role_ids")?;
        Ok(self
            .store
            .replace_user_roles(user_id, &role_ids, granted_by)
            .await?)
    }
}

fn normalized_identity(identity: &ExternalIdentity) -> Result<ExternalIdentity, AccountError> {
    let mut issuer =
        Url::parse(identity.issuer.trim()).map_err(|_| AccountError::InvalidIdentity)?;
    if !matches!(issuer.scheme(), "http" | "https") || issuer.host_str().is_none() {
        return Err(AccountError::InvalidIdentity);
    }
    issuer.set_query(None);
    issuer.set_fragment(None);
    let path = issuer.path().trim_end_matches('/').to_owned();
    issuer.set_path(if path.is_empty() { "/" } else { path.as_str() });
    let issuer = issuer.to_string();
    let subject = identity.subject.trim();
    let display_name = identity.display_name.trim();
    let valid = issuer.len() <= MAX_ISSUER_LENGTH
        && !subject.is_empty()
        && subject.len() <= MAX_SUBJECT_LENGTH
        && !display_name.is_empty()
        && display_name.chars().count() <= MAX_IDENTITY_NAME_LENGTH
        && identity
            .email
            .as_deref()
            .is_none_or(|email| email.len() <= MAX_EMAIL_LENGTH)
        && identity
            .avatar_url
            .as_deref()
            .is_none_or(|url| url.len() <= MAX_AVATAR_URL_LENGTH);
    if !valid {
        return Err(AccountError::InvalidIdentity);
    }
    Ok(ExternalIdentity {
        issuer,
        subject: subject.to_owned(),
        email: identity.email.clone(),
        display_name: display_name.to_owned(),
        avatar_url: identity.avatar_url.clone(),
    })
}

fn validate_role_key(key: &str) -> Result<(), AccountError> {
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

fn validate_role_fields(name: &str, description: Option<&str>) -> Result<(), AccountError> {
    if name.trim().is_empty() || name.chars().count() > MAX_ROLE_NAME_LENGTH {
        return Err(invalid("name", "角色名称必须为 1 到 100 个字符"));
    }
    if description.is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_LENGTH) {
        return Err(invalid("description", "角色说明不能超过 1000 个字符"));
    }
    Ok(())
}

fn deduplicate_ids(
    ids: &mut Vec<Uuid>,
    maximum: usize,
    field: &'static str,
) -> Result<(), AccountError> {
    if ids.len() > maximum {
        return Err(invalid(field, "集合元素数量超过限制"));
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(())
}

fn ensure_custom_role(role: &Role) -> Result<(), AccountError> {
    (!role.is_system)
        .then_some(())
        .ok_or(AccountError::Conflict {
            code: "system_role_immutable",
            message: "系统角色不可修改或删除",
        })
}

fn invalid(field: &'static str, message: &'static str) -> AccountError {
    ValidationError::new(field, message).into()
}
