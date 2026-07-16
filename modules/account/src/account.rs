//! 用户、角色、权限、OIDC 认证与 RBAC HTTP 能力。
//!
//! 模块通过私有运行状态直接持有服务端创建的共享 [`sqlx::PgPool`] 句柄，内部完成
//! HTTP 与 PostgreSQL store 的装配；宿主服务只负责创建外部依赖并合并路由。

use std::sync::Arc;

use axum::Router;
use sqlx::PgPool;

/// 提供 Bearer access token 验证端口及 OIDC/JWKS 实现。
pub mod authentication;
mod authorization;
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
    AccessProfile, ExternalIdentity, Permission, PermissionKey, Role, SystemRole, User, UserStatus,
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
        self.state.provision_user(identity).await
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

impl AccountState {
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) fn token_verifier(&self) -> &dyn AccessTokenVerifier {
        self.token_verifier.as_ref()
    }

    pub(crate) async fn provision_user(
        &self,
        identity: ExternalIdentity,
    ) -> Result<User, AccountError> {
        let identity = identity.normalized().map_err(|_| {
            kernel::ValidationError::new("identity", "identity ID 或展示资料不符合约束")
        })?;
        Ok(stores::identities::provision(&identity, &self.pool).await?)
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
