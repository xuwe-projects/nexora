//! 用户、角色、权限、OIDC 认证与 RBAC HTTP 能力。
//!
//! 模块通过私有运行状态直接持有服务端创建的共享 [`sqlx::PgPool`] 句柄，内部完成
//! HTTP 与 PostgreSQL store 的装配；宿主服务只负责创建外部依赖并合并路由。

use std::{collections::BTreeSet, sync::Arc};

use axum::{Router, extract::FromRef};
pub use kernel::Page;
use kernel::ValidationError;
use sqlx::PgPool;

/// 提供 Bearer access token 验证端口及 OIDC/JWKS 实现。
pub mod authentication;
/// 提供 Bearer 认证与权限门禁使用的类型和编译期权限标记。
pub mod authorization;
/// 提供账号首次引导所需的外部身份目录适配器。
#[cfg(feature = "zitadel")]
pub mod directory;
mod entities;
mod errors;
#[cfg(feature = "zitadel")]
mod generated;
mod handlers;
mod routers;
mod stores;

pub(crate) use api::ApiError;
use authentication::AccessTokenVerifier;

pub use entities::account::{
    AccessProfile, ExternalIdentity, Permission, PermissionDefinition, PermissionKey, Role,
    SystemRole, User, UserStatus,
};
pub use errors::{AccountError, StoreError};

/// 宿主服务创建账号模块时必须提供的外部依赖。
///
/// 该结构只描述账号模块运行所需的已初始化对象。创建连接池、执行数据库迁移以及发现
/// OIDC Provider 等带 I/O 的启动工作仍由宿主服务负责。宿主还必须先通过
/// [`Account::bind_identity_issuer`] 绑定或核对部署 issuer。
#[derive(Clone)]
pub struct AccountDependencies {
    /// 宿主服务创建并与其他业务模块共享的 PostgreSQL 连接池句柄。
    pub pool: PgPool,
    /// 用于验证 HTTP Bearer access token 并提取可信身份声明的验证器。
    pub token_verifier: Arc<dyn AccessTokenVerifier>,
}

/// 账号模块首次初始化需要的可信输入。
///
/// `super_admin` 必须来自服务端已经验证的身份目录或 token，不能由浏览器提交的裸用户 ID
/// 直接构造。账号模块会再次执行字段格式校验，但不会替宿主证明身份来源是否可信。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInitialization {
    /// 要绑定为系统唯一超级管理员的可信外部身份。
    pub super_admin: ExternalIdentity,
}

/// 账号模块当前的一次性初始化状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountInitializationStatus {
    /// 尚未选择超级管理员，宿主可以展示或继续自己的初始化流程。
    Required,
    /// 账号模块已经完成初始化，且超级管理员身份永久不可替换。
    Completed {
        /// 完成初始化时绑定的本地超级管理员用户。
        super_admin: User,
    },
}

/// 账号模块执行初始化后的幂等结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountInitializationOutcome {
    /// 本次调用创建或提升用户，并首次完成账号模块初始化。
    Initialized {
        /// 本次绑定的本地超级管理员用户。
        super_admin: User,
    },
    /// 相同可信身份此前已经完成初始化，本次调用按幂等成功处理。
    AlreadyInitialized {
        /// 此前已经绑定的本地超级管理员用户。
        super_admin: User,
    },
}

/// 部署级 OIDC issuer 执行原子绑定或一致性核对后的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityIssuerBindingOutcome {
    /// 当前调用首次把 issuer 绑定到该部署，后续不可替换。
    Bound,
    /// 当前 issuer 与该部署此前绑定的值一致，没有修改数据库。
    Verified,
}

/// 可由服务端装配并合并进顶层 Router 的账号业务模块。
#[derive(Clone)]
pub struct Account {
    state: AccountState,
}

/// 账号模块的私有运行状态。
///
/// `PgPool` 是指向服务端唯一底层连接池的廉价克隆句柄；模块不会创建第二个连接池，也不会
/// 使用 `Arc<PgPool>` 再次包装它。
#[derive(Clone)]
pub(crate) struct AccountState {
    pool: PgPool,
    token_verifier: Arc<dyn AccessTokenVerifier>,
}

impl FromRef<AccountState> for Account {
    fn from_ref(state: &AccountState) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

/// 使用宿主共享连接池幂等创建或更新应用权限目录。
///
/// 相同权限键再次提交时会更新展示名称和说明。该函数复用 Account 的字段校验和 PostgreSQL
/// Store，不创建连接池，也不执行当前用户授权；只应从可信宿主启动或管理边界调用。
///
/// # Errors
///
/// 权限键、名称、说明或集合数量不符合约束，批次包含重复键，或数据库写入失败时返回
/// [`AccountError`]。
pub async fn create_permissions(
    pool: &PgPool,
    definitions: &[PermissionDefinition],
) -> Result<Vec<Permission>, AccountError> {
    validate_permission_definitions(definitions)?;
    Ok(stores::permissions::register(definitions, pool).await?)
}

/// 使用宿主共享连接池创建一个可由应用管理的自定义角色。
///
/// `permission_ids` 是角色创建后直接包含的完整权限集合；角色与权限写入由 Account Store
/// 在同一事务中完成。该函数不执行当前用户授权，不允许创建系统角色。
///
/// # Errors
///
/// 角色键、名称、说明或权限 ID 集合无效，角色键已存在，权限不存在，或数据库写入失败时
/// 返回 [`AccountError`]。
pub async fn create_role(
    pool: &PgPool,
    key: &str,
    name: &str,
    description: Option<&str>,
    permission_ids: &[i64],
) -> Result<Role, AccountError> {
    handlers::accounts::validate_role_key(key)?;
    handlers::accounts::validate_role_fields(name, description)?;
    let permission_ids = handlers::accounts::role_permission_ids(permission_ids.to_vec())?;
    Ok(stores::roles::create(key, name, description, permission_ids.as_slice(), pool).await?)
}

/// 使用宿主共享连接池原子替换自定义角色的直接权限集合。
///
/// `permission_ids` 表示替换后的完整集合，而不是增量添加列表。空集合会清除该自定义角色的
/// 全部直接权限；系统角色始终不可修改。
///
/// # Errors
///
/// 权限数量超限、权限或角色不存在、目标为系统角色，或数据库事务失败时返回
/// [`AccountError`]。
pub async fn replace_role_permissions(
    pool: &PgPool,
    role_id: i64,
    permission_ids: &[i64],
) -> Result<Role, AccountError> {
    let permission_ids = handlers::accounts::role_permission_ids(permission_ids.to_vec())?;
    Ok(stores::roles::replace_permissions(role_id, permission_ids.as_slice(), pool).await?)
}

/// 使用宿主共享连接池创建一个已经由宿主确认的外部身份对应的本地用户。
///
/// 本函数只在 `account.users` 中开通用户，不创建本地密码，也不会替宿主验证身份来源。
/// 调用方必须先从当前部署绑定的身份目录、管理员操作或其他可信规则获得
/// [`ExternalIdentity`]。
///
/// # Errors
///
/// 身份字段不符合约束、同一 identity ID 已经开通，或数据库事务失败时返回
/// [`AccountError`]。
pub async fn create_user(pool: &PgPool, identity: ExternalIdentity) -> Result<User, AccountError> {
    let identity = identity
        .normalized()
        .map_err(|_| ValidationError::new("identity", "identity ID 或展示资料不符合约束"))?;
    Ok(stores::identities::provision(&identity, pool).await?)
}

/// 使用宿主共享连接池原子创建本地用户并授予初始角色。
///
/// `role_ids` 表示创建时直接授予的业务角色，Account 会自动补充内置 `member` 角色；
/// `granted_by` 必须是已经由宿主认证授权的本地操作者用户 ID，并写入每条用户角色关系。
/// 本函数不执行当前请求授权，只应从可信宿主启动或管理边界调用。
///
/// # Errors
///
/// 身份字段或角色数量无效、身份已经开通、任一角色或授权人不存在，或事务中的任一写入
/// 失败时返回 [`AccountError`]；失败不会留下用户或部分角色关系。
pub async fn create_user_with_roles(
    pool: &PgPool,
    identity: ExternalIdentity,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<User, AccountError> {
    let identity = identity
        .normalized()
        .map_err(|_| ValidationError::new("identity", "identity ID 或展示资料不符合约束"))?;
    let role_ids = handlers::accounts::user_role_ids(role_ids.to_vec())?;
    Ok(
        stores::identities::provision_with_roles(&identity, role_ids.as_slice(), granted_by, pool)
            .await?,
    )
}

/// 使用宿主共享连接池原子替换普通用户的直接角色集合。
///
/// `role_ids` 表示替换后的完整业务角色集合；Account 会按既有规则保留内置 `member` 角色。
/// `granted_by` 必须是已经由宿主认证授权的本地操作者用户 ID。该函数不自行验证当前请求，
/// 因此不可信入口必须先完成用户角色管理授权。
///
/// # Errors
///
/// 用户、角色或操作者不存在，角色数量超限，目标为超级管理员，操作会移除最后一个管理员，
/// 或数据库事务失败时返回 [`AccountError`]。
pub async fn replace_user_roles(
    pool: &PgPool,
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<AccessProfile, AccountError> {
    let role_ids = handlers::accounts::user_role_ids(role_ids.to_vec())?;
    Ok(stores::users::replace_roles(user_id, role_ids.as_slice(), granted_by, pool).await?)
}

impl Account {
    /// 原子绑定当前部署唯一允许使用的 OIDC issuer，或核对既有绑定。
    ///
    /// 首次调用会把规范化后的 issuer 写入 `account.system_initialization`；相同 issuer
    /// 可以安全重试，任何不同 issuer 都会被永久拒绝。宿主应在创建 HTTP Router 前调用，
    /// 通常由 Nexora 的 Account 服务端依赖装配函数完成。
    ///
    /// # Errors
    ///
    /// issuer 不是安全的 HTTPS URL（loopback 开发地址可使用 HTTP）、与部署既有绑定不一致，
    /// 或数据库不可访问时返回 [`AccountError`]。
    pub async fn bind_identity_issuer(
        pool: &PgPool,
        identity_issuer: &str,
    ) -> Result<IdentityIssuerBindingOutcome, AccountError> {
        let identity_issuer = entities::account::normalized_identity_issuer(identity_issuer)?;
        Ok(
            match stores::system_initialization::bind_or_verify_identity_issuer(
                identity_issuer.as_str(),
                pool,
            )
            .await?
            {
                stores::system_initialization::IdentityIssuerBindingOutcome::Bound => {
                    IdentityIssuerBindingOutcome::Bound
                }
                stores::system_initialization::IdentityIssuerBindingOutcome::Verified => {
                    IdentityIssuerBindingOutcome::Verified
                }
            },
        )
    }

    /// 使用宿主提供的依赖构造账号模块。
    ///
    /// 该构造函数只装配内存对象，不连接数据库、不执行迁移、不启动 HTTP 服务，也不会发起
    /// OIDC 网络请求。宿主必须先完成 [`Self::bind_identity_issuer`]，随后才能安全地把
    /// [`Self::routers`] 合并进自己的 Axum Router；未绑定部署 issuer 的认证请求会失败。
    pub fn new(dependencies: AccountDependencies) -> Self {
        let AccountDependencies {
            pool,
            token_verifier,
        } = dependencies;
        Self {
            state: AccountState {
                pool,
                token_verifier,
            },
        }
    }

    /// 返回账号模块当前的一次性初始化状态及已绑定的超级管理员。
    ///
    /// 宿主可以把该状态与自己的业务初始化状态组合，而无需采用账号模块提供的固定页面或
    /// 启动流程。
    ///
    /// # Errors
    ///
    /// 数据库不可访问，或初始化记录与超级管理员数据不一致时返回 [`AccountError`]。
    pub async fn initialization_status(&self) -> Result<AccountInitializationStatus, AccountError> {
        Ok(
            match stores::system_initialization::query_status(&self.state.pool).await? {
                Some(super_admin) => AccountInitializationStatus::Completed { super_admin },
                None => AccountInitializationStatus::Required,
            },
        )
    }

    /// 返回系统是否已完成一次性初始化。
    ///
    /// 需要同时获得超级管理员资料时应使用 [`Self::initialization_status`]。
    ///
    /// # Errors
    ///
    /// 数据库不可访问或初始化状态记录无效时返回 [`AccountError`]。
    pub async fn is_system_initialized(&self) -> Result<bool, AccountError> {
        Ok(stores::system_initialization::query(&self.state.pool).await?)
    }

    /// 返回首次初始化时必须同步到认证授权 Project 的全部本地系统角色。
    ///
    /// # Errors
    ///
    /// 数据库不可访问或系统角色目录为空时返回 [`AccountError`]。
    pub async fn system_roles(&self) -> Result<Vec<SystemRole>, AccountError> {
        Ok(stores::roles::query_system(&self.state.pool).await?)
    }

    /// 验证 Bearer access token，并返回已开通且未停用用户的完整授权快照。
    ///
    /// 宿主的自定义 Axum 路由可以把可克隆的 `Account` 放入自己的 `AppState`，从请求头提取
    /// token 后调用本方法，从而复用 Account Router 相同的 issuer 绑定、身份同步和用户状态
    /// 规则。
    ///
    /// # Errors
    ///
    /// token 无效或认证服务不可用、issuer 不属于当前部署、身份尚未开通、用户已停用，
    /// 或数据库访问失败时返回错误。
    pub async fn authenticate(&self, access_token: &str) -> Result<AccessProfile, AccountError> {
        self.state.authenticate(access_token).await
    }

    /// 验证 Bearer token，并要求当前用户拥有指定内置或应用自定义权限。
    ///
    /// 超级管理员按 Account 既有规则直接通过；普通用户必须通过角色拥有该权限。应用应先用
    /// [`Self::register_permissions`] 注册自定义权限，并使用 [`PermissionKey::from_static`]
    /// 声明稳定权限键。
    ///
    /// # Errors
    ///
    /// 认证失败或当前用户缺少 `permission` 时返回错误。
    pub async fn authorize(
        &self,
        access_token: &str,
        permission: PermissionKey,
    ) -> Result<AccessProfile, AccountError> {
        let profile = self.authenticate(access_token).await?;
        if profile.allows(permission.clone()) {
            Ok(profile)
        } else {
            Err(AccountError::Forbidden(permission))
        }
    }

    /// 幂等注册应用定义的权限目录，并返回本次注册后的权限实体。
    ///
    /// 相同权限键再次注册会更新展示名称和说明，便于应用升级时同步权限元数据。该方法属于
    /// 可信宿主启动/管理边界，不执行当前用户授权；从 HTTP 请求调用前必须由宿主自行完成
    /// 管理权限校验。
    ///
    /// # Errors
    ///
    /// 权限键、名称、说明或集合数量不符合约束，存在重复键，或数据库写入失败时返回错误。
    pub async fn register_permissions(
        &self,
        definitions: &[PermissionDefinition],
    ) -> Result<Vec<Permission>, AccountError> {
        create_permissions(&self.state.pool, definitions).await
    }

    /// 返回内置权限与应用自定义权限组成的完整目录。
    ///
    /// # Errors
    ///
    /// 数据库不可访问或权限记录不符合稳定权限键约束时返回错误。
    pub async fn permissions(&self) -> Result<Vec<Permission>, AccountError> {
        Ok(stores::permissions::query_all(&self.state.pool).await?)
    }

    /// 返回系统角色与应用创建的自定义角色，并附带各自直接权限。
    ///
    /// # Errors
    ///
    /// 数据库不可访问或角色、权限记录无效时返回错误。
    pub async fn roles(&self) -> Result<Vec<Role>, AccountError> {
        Ok(stores::roles::query_all(&self.state.pool).await?)
    }

    /// 按数据库 ID 返回一个角色及其直接权限。
    ///
    /// # Errors
    ///
    /// 角色不存在、数据库不可访问或角色数据无效时返回错误。
    pub async fn role(&self, role_id: i64) -> Result<Role, AccountError> {
        stores::roles::query_by_id(role_id, &self.state.pool)
            .await?
            .ok_or(AccountError::NotFound("角色"))
    }

    /// 创建一个可由应用管理的自定义角色。
    ///
    /// 该方法不会执行当前用户授权；HTTP 或其他不可信入口必须先验证调用者拥有角色管理权限。
    ///
    /// # Errors
    ///
    /// 角色键、名称、说明、权限 ID 集合无效，角色键已存在，权限不存在或数据库失败时返回错误。
    pub async fn create_role(
        &self,
        key: &str,
        name: &str,
        description: Option<&str>,
        permission_ids: &[i64],
    ) -> Result<Role, AccountError> {
        create_role(&self.state.pool, key, name, description, permission_ids).await
    }

    /// 修改一个自定义角色的名称或说明。
    ///
    /// `description` 为 `None` 时保持原值，`Some(None)` 清空说明，`Some(Some(value))` 设置
    /// 新说明。系统角色始终不可修改。
    ///
    /// # Errors
    ///
    /// 没有提供任何变更、字段无效、角色不存在、目标是系统角色或数据库失败时返回错误。
    pub async fn update_role(
        &self,
        role_id: i64,
        name: Option<&str>,
        description: Option<Option<&str>>,
    ) -> Result<Role, AccountError> {
        if name.is_none() && description.is_none() {
            return Err(ValidationError::new("role", "至少需要提供一个要修改的角色字段").into());
        }
        let current = self.role(role_id).await?;
        if current.is_system {
            return Err(AccountError::Conflict {
                code: "system_role_immutable",
                message: "系统角色不可修改或删除",
            });
        }
        let final_name = name.unwrap_or(current.name.as_str());
        let final_description = description.unwrap_or(current.description.as_deref());
        handlers::accounts::validate_role_fields(final_name, final_description)?;
        Ok(stores::roles::update(role_id, name, description, &self.state.pool).await?)
    }

    /// 删除一个尚未被用户引用的自定义角色。
    ///
    /// # Errors
    ///
    /// 角色不存在、角色是系统角色、仍被用户引用或数据库失败时返回错误。
    pub async fn delete_role(&self, role_id: i64) -> Result<(), AccountError> {
        Ok(stores::roles::delete(role_id, &self.state.pool).await?)
    }

    /// 原子替换一个自定义角色直接包含的权限集合。
    ///
    /// # Errors
    ///
    /// 权限数量超限、权限或角色不存在、目标是系统角色或数据库失败时返回错误。
    pub async fn replace_role_permissions(
        &self,
        role_id: i64,
        permission_ids: &[i64],
    ) -> Result<Role, AccountError> {
        replace_role_permissions(&self.state.pool, role_id, permission_ids).await
    }

    /// 分页返回本地用户目录。
    ///
    /// 页码从 1 开始，页大小会限制到 1 至 100。该方法不执行当前用户授权。
    ///
    /// # Errors
    ///
    /// 页码无效或数据库查询失败时返回错误。
    pub async fn users(&self, page: u32, page_size: u32) -> Result<Page<User>, AccountError> {
        let request = handlers::accounts::page_request(page, page_size)?;
        Ok(stores::users::query_page(request, &self.state.pool)
            .await
            .map_err(StoreError::from)?)
    }

    /// 返回指定用户及其直接角色、合并权限组成的授权快照。
    ///
    /// # Errors
    ///
    /// 用户不存在、数据库不可访问或授权数据无效时返回错误。
    pub async fn user_access(&self, user_id: &str) -> Result<AccessProfile, AccountError> {
        stores::users::query_access_profile(user_id, &self.state.pool)
            .await?
            .ok_or(AccountError::NotFound("用户"))
    }

    /// 更新一个普通用户的访问状态。
    ///
    /// # Errors
    ///
    /// 用户不存在、目标是超级管理员、操作会停用最后一个管理员或数据库失败时返回错误。
    pub async fn update_user_status(
        &self,
        user_id: &str,
        status: UserStatus,
    ) -> Result<User, AccountError> {
        Ok(stores::users::update_status(user_id, status, &self.state.pool).await?)
    }

    /// 原子替换一个普通用户的直接角色集合，并保留内置 `member` 角色。
    ///
    /// `granted_by` 必须是已经通过宿主认证授权的本地操作者用户 ID。该方法本身不执行当前
    /// 用户授权，HTTP 或其他不可信入口必须先验证角色管理权限。
    ///
    /// # Errors
    ///
    /// 用户或角色不存在、角色数量超限、目标是超级管理员、操作会移除最后一个管理员，
    /// 或数据库失败时返回错误。
    pub async fn replace_user_roles(
        &self,
        user_id: &str,
        role_ids: &[i64],
        granted_by: &str,
    ) -> Result<AccessProfile, AccountError> {
        replace_user_roles(&self.state.pool, user_id, role_ids, granted_by).await
    }

    /// 把经过宿主服务确认的外部身份显式开通为普通本地用户。
    ///
    /// 该方法不会验证 access token，也不会自行授予角色。调用方必须先通过可信身份目录、
    /// 管理员操作或其他业务规则确认 [`ExternalIdentity`]，再调用本方法。HTTP 路由中的同等
    /// 能力额外要求当前用户拥有 `users:provision` 权限。
    ///
    /// # Errors
    ///
    /// 身份字段无效、同一 identity ID 已经开通，或数据库不可访问时返回 [`AccountError`]。
    pub async fn provision_user(&self, identity: ExternalIdentity) -> Result<User, AccountError> {
        create_user(&self.state.pool, identity).await
    }

    /// 在同一数据库事务中开通外部身份并授予初始角色。
    ///
    /// `role_ids` 表示创建时直接授予的业务角色；Account 仍会自动加入内置 `member` 角色。
    /// `granted_by` 必须是已经通过宿主认证授权的本地操作者用户 ID，并会写入每条角色关系的
    /// 授权审计字段。该方法本身不校验当前请求权限，不可信入口应先要求
    /// `users:provision`。
    ///
    /// # Errors
    ///
    /// 身份字段或角色数量无效、身份已经开通、任一角色或授权人不存在，或事务中的任一写入
    /// 失败时返回错误；失败不会留下用户或部分角色关系。
    pub async fn provision_user_with_roles(
        &self,
        identity: ExternalIdentity,
        role_ids: &[i64],
        granted_by: &str,
    ) -> Result<User, AccountError> {
        create_user_with_roles(&self.state.pool, identity, role_ids, granted_by).await
    }

    /// 把经过身份目录或认证流程确认的用户设为唯一超级管理员。
    ///
    /// 初始化与超级管理员写入在同一个数据库事务中完成。相同身份重复调用会返回
    /// [`AccountInitializationOutcome::AlreadyInitialized`]；另一个身份试图替换已经绑定的超级
    /// 管理员时返回 `system_already_initialized` 冲突。该方法不提供页面、不启动服务，也不
    /// 执行宿主自己的初始化逻辑。
    ///
    /// # Errors
    ///
    /// 身份字段无效、其他身份已经完成初始化，或数据库事务失败时返回 [`AccountError`]。
    pub async fn initialize(
        &self,
        request: AccountInitialization,
    ) -> Result<AccountInitializationOutcome, AccountError> {
        let identity = request.super_admin.normalized()?;
        match stores::system_initialization::initialize(&identity, &self.state.pool).await? {
            stores::system_initialization::InitializationOutcome::Initialized(super_admin) => {
                Ok(AccountInitializationOutcome::Initialized { super_admin })
            }
            stores::system_initialization::InitializationOutcome::AlreadyInitialized(
                super_admin,
            ) => Ok(AccountInitializationOutcome::AlreadyInitialized { super_admin }),
        }
    }

    /// 注入账号模块自己的 State，并返回仍等待宿主 State `S` 的路由。
    ///
    /// 宿主应把返回值合并进顶层 `Router<S>`，最后只在服务端边界调用一次 `with_state`。
    pub fn routers<S>(&self) -> Router<S> {
        routers::initialize().with_state::<S>(self.state.clone())
    }
}

fn validate_permission_definitions(
    definitions: &[PermissionDefinition],
) -> Result<(), AccountError> {
    if definitions.len() > 256 {
        return Err(ValidationError::new("permissions", "权限定义数量不能超过 256").into());
    }
    let mut keys = BTreeSet::new();
    for definition in definitions {
        let key = PermissionKey::try_from(definition.key.as_str()).map_err(|()| {
            ValidationError::new(
                "key",
                "权限键必须使用 resource:action 格式；两段都必须为 2 到 64 位小写字母、数字、点、下划线或连字符，并以字母开头",
            )
        })?;
        if !keys.insert(key) {
            return Err(ValidationError::new("key", "同一批权限定义不能包含重复键").into());
        }
        if definition.name.trim().is_empty() || definition.name.chars().count() > 100 {
            return Err(ValidationError::new("name", "权限名称必须为 1 到 100 个字符").into());
        }
        if definition
            .description
            .as_deref()
            .is_some_and(|description| description.chars().count() > 1_000)
        {
            return Err(ValidationError::new("description", "权限说明不能超过 1000 个字符").into());
        }
    }
    Ok(())
}

impl AccountState {
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) async fn authenticate(
        &self,
        access_token: &str,
    ) -> Result<AccessProfile, AccountError> {
        let identity = self.token_verifier.verify(access_token).await?;
        self.verify_identity_issuer(identity.issuer.as_str())
            .await?;
        let identity = ExternalIdentity {
            identity_id: identity.subject,
            email: identity.email,
            display_name: identity.display_name,
            avatar_url: identity.avatar_url,
        }
        .normalized()?;
        let user = stores::identities::sync_existing(&identity, self.pool())
            .await?
            .ok_or_else(|| {
                tracing::warn!(
                    business_operation = "authenticate_local_account",
                    identity_id = %identity.identity_id,
                    outcome = "not_registered",
                    "认证身份没有对应的本地用户，拒绝访问"
                );
                AccountError::UserNotRegistered
            })?;
        if user.status == UserStatus::Suspended {
            return Err(AccountError::UserSuspended);
        }
        stores::users::query_access_profile(user.id.as_str(), self.pool())
            .await?
            .ok_or(AccountError::NotFound("用户"))
    }

    pub(crate) async fn verify_identity_issuer(
        &self,
        identity_issuer: &str,
    ) -> Result<(), AccountError> {
        let identity_issuer = entities::account::normalized_identity_issuer(identity_issuer)
            .map_err(|_| AccountError::IdentityIssuerMismatch)?;
        Ok(stores::system_initialization::verify_identity_issuer(
            identity_issuer.as_str(),
            &self.pool,
        )
        .await?)
    }
}
