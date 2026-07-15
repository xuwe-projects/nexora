//! 用户、角色、权限、OIDC 认证与 RBAC HTTP 能力。
//!
//! 模块通过 [`AccountState`] 直接持有服务端创建的共享 [`sqlx::PgPool`] 句柄，内部完成
//! HTTP 与 PostgreSQL store 的装配；宿主服务只负责创建外部依赖并合并路由。

use std::sync::Arc;

use axum::Router;
use sqlx::PgPool;

/// 提供 Bearer access token 验证端口及 OIDC/JWKS 实现。
pub mod authentication;
mod authorization;
/// 提供账号首次引导所需的外部身份目录适配器。
pub mod directory;
mod entities;
mod errors;
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
    /// 使用共享连接池与 token verifier 构造账号模块。
    ///
    /// 该构造函数只装配内存对象，不连接数据库、不执行迁移，也不会发起 OIDC 网络请求。
    pub fn new(pool: PgPool, token_verifier: Arc<dyn AccessTokenVerifier>) -> Self {
        Self {
            state: AccountState {
                pool,
                token_verifier,
            },
        }
    }

    /// 返回系统是否已完成一次性初始化。
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

    /// 把经过身份目录确认的用户设为唯一超级管理员并完成系统初始化。
    ///
    /// # Errors
    ///
    /// 身份字段无效、系统已完成初始化或数据库事务失败时返回 [`AccountError`]。
    pub async fn initialize_super_admin(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<User, AccountError> {
        let identity = identity.normalized()?;
        Ok(
            stores::system_initialization::initialize_super_admin(&identity, &self.state.pool)
                .await?,
        )
    }

    /// 注入账号模块自己的 State，并返回仍等待宿主 State `S` 的路由。
    ///
    /// 宿主应把返回值合并进顶层 `Router<S>`，最后只在服务端边界调用一次 `with_state`。
    pub fn routers<S>(self) -> Router<S> {
        routers::initialize().with_state::<S>(self.state)
    }
}

impl AccountState {
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) fn token_verifier(&self) -> &dyn AccessTokenVerifier {
        self.token_verifier.as_ref()
    }
}
