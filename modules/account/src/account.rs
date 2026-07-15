//! 用户、角色、权限、OIDC 认证与 RBAC HTTP 能力。
//!
//! 模块通过 [`AccountState`] 直接持有服务端创建的共享 [`sqlx::PgPool`] 句柄，内部完成
//! HTTP、应用用例和 PostgreSQL store 的装配；宿主服务只负责创建外部依赖并合并路由。

use std::sync::Arc;

use axum::Router;
use sqlx::PgPool;

mod application;
/// 提供 Bearer access token 验证端口及 OIDC/JWKS 实现。
pub mod authentication;
mod authorization;
/// 提供账号首次引导所需的外部身份目录适配器。
pub mod directory;
mod domain;
mod entities;
mod errors;
mod handlers;
mod models;
mod routers;
mod stores;

pub(crate) use api::ApiError;
use authentication::AccessTokenVerifier;

pub use application::AccountApplication;
pub use domain::account::{
    AccessProfile, CreateRole, ExternalIdentity, Permission, Role, UpdateRole, User, UserStatus,
    permission,
};
pub use errors::{AccountError, StoreError};
pub use kernel::{Page, PageRequest};
pub use stores::accounts::{AccountsStore, PostgresAccountsStore};

/// 可由服务端装配并合并进顶层 Router 的账号业务模块。
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
    application: AccountApplication,
    token_verifier: Arc<dyn AccessTokenVerifier>,
}

impl Account {
    /// 使用共享连接池与 token verifier 构造账号模块。
    ///
    /// 该构造函数只装配内存对象，不连接数据库、不执行迁移，也不会发起 OIDC 网络请求。
    pub fn new(pool: PgPool, token_verifier: Arc<dyn AccessTokenVerifier>) -> Self {
        let store = Arc::new(PostgresAccountsStore::new(pool.clone()));
        Self::with_store(pool, store, token_verifier)
    }

    /// 使用调用方提供的持久化端口构造账号模块。
    ///
    /// 该入口适用于集成测试或替换 PostgreSQL store 的部署，同时仍要求提供服务端共享的
    /// `PgPool`，确保模块 State 形状与生产环境一致。
    pub fn with_store(
        pool: PgPool,
        store: Arc<dyn AccountsStore>,
        token_verifier: Arc<dyn AccessTokenVerifier>,
    ) -> Self {
        Self {
            state: AccountState {
                pool,
                application: AccountApplication::new(store),
                token_verifier,
            },
        }
    }

    /// 返回账号模块持有的共享 PostgreSQL 连接池句柄。
    pub fn pool(&self) -> &PgPool {
        &self.state.pool
    }

    /// 返回当前唯一内置超级管理员；首次引导尚未完成时返回 `None`。
    ///
    /// # Errors
    ///
    /// 数据库不可访问或持久化数据无效时返回 [`AccountError`]。
    pub async fn super_admin(&self) -> Result<Option<User>, AccountError> {
        self.state.application.super_admin().await
    }

    /// 把经过身份目录确认的用户绑定为唯一内置超级管理员。
    ///
    /// # Errors
    ///
    /// 身份字段无效、系统已绑定其他超级管理员或数据库事务失败时返回 [`AccountError`]。
    pub async fn bind_super_admin(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<User, AccountError> {
        self.state.application.bind_super_admin(identity).await
    }

    /// 注入账号模块自己的 State，并返回仍等待宿主 State `S` 的路由。
    ///
    /// 宿主应把返回值合并进顶层 `Router<S>`，最后只在服务端边界调用一次 `with_state`。
    pub fn routers<S>(self) -> Router<S> {
        routers::initialize().with_state::<S>(self.state)
    }
}

impl AccountState {
    pub(crate) fn application(&self) -> &AccountApplication {
        &self.application
    }

    pub(crate) fn token_verifier(&self) -> &dyn AccessTokenVerifier {
        self.token_verifier.as_ref()
    }
}
