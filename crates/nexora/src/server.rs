//! Nexora 服务端模块初始化与 Router 组合边界。
//!
//! 默认实现只负责 Account 装配与框架 Router；数据库连接池、顶层 Axum State、监听器、
//! 日志和关闭策略始终由应用创建和持有。

use std::net::{IpAddr, SocketAddr};

use axum::Router;
use sqlx::PgPool;
use thiserror::Error;

pub use crate::account::server::{
    AccountServerInitializationError, DefaultSetup, DefaultSetupCompletionRequest,
    DefaultSetupUnlockRequest, DirectoryError, DirectoryUser, MigrationError, MigrationReport,
    OidcSettings as AccountOidcSettings, Settings as AccountSettings, Setup,
    SetupCompletionRequest, SetupUnlockRequest, ZitadelUserDirectory, dependencies, migrate,
    setup_routes, setup_routes_with, user_directory,
};

/// 可组合 Nexora 默认模块与应用 Router 的服务端实例。
///
/// 应用创建自己的 [`PgPool`] 与 Axum State，再调用 [`Self::initialize`] 完成迁移和当前
/// 框架模块装配；只执行数据库升级时可以单独调用 [`Self::migrate`]。随后通过
/// `Router::new().merge(server.routers())` 组合最终 Router，并使用标准
/// `axum::serve(listener, app)` 启动服务。
pub struct Server {
    account_routes: Option<AccountRoutes>,
    setup_required: bool,
}

struct AccountRoutes {
    account: crate::account::Account,
    directory: ZitadelUserDirectory,
    setup_secret: String,
}

impl Server {
    /// 创建不持有数据库连接池、监听器或应用 State 的服务端组合器。
    pub const fn new() -> Self {
        Self {
            account_routes: None,
            setup_required: false,
        }
    }

    /// 使用应用创建的 PostgreSQL 连接池执行全部待执行迁移。
    ///
    /// SQLx 根据 `_sqlx_migrations` 自动跳过已成功执行的版本；本方法不会创建、替换或
    /// 关闭连接池。
    ///
    /// # Errors
    ///
    /// 数据库状态检查失败、安全约束不满足或迁移执行失败时返回错误。
    pub async fn migrate(&self, pool: &PgPool) -> Result<MigrationReport, ServerError> {
        Ok(crate::account::server::migrate(pool).await?)
    }

    /// 使用应用提供的资源初始化 Nexora 服务端模块。
    ///
    /// 当前默认初始化依次执行数据库迁移以及 Account、ZITADEL、Setup 路由装配。未来 OSS、
    /// 缓存等框架模块也应收口在这一生命周期，而不是重新塞回 [`Self::new`]。
    ///
    /// 初始化已经完成时会同步 ZITADEL Project 系统角色；未完成时记录等待 Setup 的状态，
    /// 供宿主在监听成功后通过 [`Self::setup_url`] 输出可访问的 `/setup` 地址。
    ///
    /// # Errors
    ///
    /// OIDC discovery、issuer 绑定、ZITADEL 客户端或系统角色同步失败时返回错误。
    #[cfg(feature = "server")]
    pub async fn initialize<S>(
        &mut self,
        settings: &S,
        pool: &PgPool,
        setup_secret: &str,
    ) -> Result<(), ServerError>
    where
        S: crate::config::__private::ProvidesAccountServerSettings<
                AccountServerSettings = crate::account::server::Settings,
            >,
    {
        self.migrate(pool).await?;
        self.initialize_account(settings, pool, setup_secret).await
    }

    async fn initialize_account<S>(
        &mut self,
        settings: &S,
        pool: &PgPool,
        setup_secret: &str,
    ) -> Result<(), ServerError>
    where
        S: crate::config::__private::ProvidesAccountServerSettings<
                AccountServerSettings = crate::account::server::Settings,
            >,
    {
        let dependencies = crate::account::server::dependencies(pool.clone(), settings).await?;
        let account = crate::account::Account::new(dependencies);
        let directory = crate::account::server::user_directory(settings)?;
        self.setup_required = !account.is_system_initialized().await?;
        if !self.setup_required {
            let roles = account.system_roles().await?;
            directory.ensure_project_roles(roles.as_slice()).await?;
        }
        self.account_routes = Some(AccountRoutes {
            account,
            directory,
            setup_secret: setup_secret.to_owned(),
        });
        Ok(())
    }

    /// 返回仍等待应用 State 的 Nexora 框架 Router。
    ///
    /// 应用负责把返回值合并进自己的顶层 Router，这样路由组合顺序和宿主中间件边界始终
    /// 由应用掌控。构建 Router 不会创建新的数据库连接池或重复初始化 Account。
    pub fn routers<S>(&self) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        let Some(routes) = self.account_routes.as_ref() else {
            return Router::new();
        };
        Router::new()
            .merge(crate::account::server::setup_routes(
                routes.account.clone(),
                routes.directory.clone(),
                routes.setup_secret.as_str(),
            ))
            .merge(routes.account.routers::<S>())
    }

    /// 当系统尚未完成初始化时，根据宿主已经绑定的地址返回 Setup 页面 URL。
    ///
    /// 对外监听未指定地址时，URL 使用本机回环地址，避免把 `0.0.0.0` 或 `[::]` 作为不可
    /// 直接访问的主机名展示给操作者；已经初始化完成时返回 `None`。
    pub fn setup_url(&self, address: SocketAddr) -> Option<String> {
        self.setup_required.then(|| setup_url(address))
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

/// Nexora 默认服务端初始化失败原因。
#[derive(Debug, Error)]
pub enum ServerError {
    /// Nexora 集中迁移失败。
    #[cfg(feature = "server")]
    #[error("服务端数据库迁移失败")]
    Migration(
        /// 迁移安全检查或 SQLx 执行过程返回的原始错误。
        #[from]
        crate::account::server::MigrationError,
    ),
    /// Account OIDC 与部署 issuer 依赖装配失败。
    #[cfg(feature = "server")]
    #[error("Account 服务端依赖初始化失败")]
    AccountInitialization(
        /// OIDC discovery、token verifier 或 issuer 绑定过程返回的原始错误。
        #[from]
        crate::account::server::AccountServerInitializationError,
    ),
    /// Account 初始化状态或系统角色读取失败。
    #[cfg(feature = "server")]
    #[error("Account 服务端操作失败")]
    Account(
        /// Account 领域用例或持久化边界返回的原始错误。
        #[from]
        crate::account::AccountError,
    ),
    /// ZITADEL 用户目录或 Project 角色同步失败。
    #[cfg(feature = "server")]
    #[error("ZITADEL 目录操作失败")]
    Directory(
        /// ZITADEL 用户目录或角色管理接口返回的原始错误。
        #[from]
        crate::account::server::DirectoryError,
    ),
}

fn setup_url(address: SocketAddr) -> String {
    let host = match address.ip() {
        address if address.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    format!("http://{host}:{}/setup", address.port())
}
