//! Nexora 服务端模块初始化与 Router 组合边界。
//!
//! 默认实现只负责 Account 装配与框架 Router；数据库连接池、顶层 Axum State、监听器、
//! 日志和关闭策略始终由应用创建和持有。

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::Router;
use sqlx::PgPool;
use thiserror::Error;

pub use crate::account::server::{
    AccountServerInitializationError, CreateZitadelOrganizationRequest, DefaultSetup,
    DefaultSetupCompletionRequest, DefaultSetupUnlockRequest, DirectoryError, DirectoryUser,
    OidcSettings as AccountOidcSettings, Settings as AccountSettings, Setup,
    SetupCompletionRequest, SetupUnlockRequest, ZitadelAuthorization, ZitadelAuthorizationOutcome,
    ZitadelAuthorizationRequest, ZitadelDeleteUserOutcome, ZitadelOrganization,
    ZitadelOrganizationState, ZitadelProjectGrant, ZitadelProjectGrantOutcome,
    ZitadelProjectGrantRequest, ZitadelProjectGrantState, ZitadelProvisioningClient,
    ZitadelProvisioningError, ZitadelUserDirectory, dependencies, provisioning_client,
    setup_routes, setup_routes_with, user_directory,
};
pub use crate::account::{
    AccessProfile, Account, AccountError, AuthenticatedUser, Authorized, BearerAccessToken,
    CreateHumanIdentity, ExternalIdentity, IdentityDirectory, IdentityDirectoryError,
    OidcAccessTokenVerifier, OidcResourceServer, Permission, PermissionDefinition, PermissionKey,
    RequiredPermission, Role, User, VerifiedBearerIdentity, VerifiedIdentity,
    VerifiedOrganizationContext, create_permissions, create_role, create_user,
    create_user_with_roles, replace_role_permissions, replace_user_roles,
};

/// 可组合 Nexora 默认模块与应用 Router 的服务端实例。
///
/// 应用创建自己的 [`PgPool`] 与 Axum State，在调用 [`Self::initialize`] 前先把
/// [`migrations`] 返回的框架迁移与业务迁移组合为一个 SQLx `Migrator` 并执行。随后通过
/// `Router::new().merge(server.routers())` 组合最终 Router，并使用标准 `axum::serve` 启动。
pub struct Server {
    account_routes: Option<AccountRoutes>,
    setup_required: bool,
}

struct AccountRoutes {
    account: crate::account::Account,
    directory: ZitadelUserDirectory,
    provisioning_client: ZitadelProvisioningClient,
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

    /// 使用应用提供的资源初始化 Nexora 服务端模块。
    ///
    /// 当前默认初始化只装配 Account、ZITADEL 与 Setup 路由，不执行数据库迁移。宿主必须
    /// 先把 [`migrations`] 与应用迁移组合成一个 SQLx `Migrator`，再使用同一个 `pool`
    /// 执行迁移并调用本方法。未来 OSS、缓存等框架模块也应收口在这一生命周期，而不是
    /// 重新塞回 [`Self::new`]。
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
        let directory = crate::account::server::user_directory(settings)?;
        let provisioning_client = crate::account::server::provisioning_client(settings)?;
        let mut dependencies = crate::account::server::dependencies(pool.clone(), settings).await?;
        dependencies.identity_directory = Some(Arc::new(directory.clone()));
        let account = crate::account::Account::new(dependencies);
        self.setup_required = !account.is_system_initialized().await?;
        if !self.setup_required {
            let roles = account.system_roles().await?;
            directory.ensure_project_roles(roles.as_slice()).await?;
        }
        self.account_routes = Some(AccountRoutes {
            account,
            directory,
            provisioning_client,
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

    /// 返回已经初始化的 Account 业务 facade 的可克隆句柄。
    ///
    /// 宿主可把返回值放入自己的 Axum State，并为 [`crate::account::Account`] 实现
    /// `axum::extract::FromRef<AppState>`，随后直接在自定义 handler 中使用
    /// [`crate::account::AuthenticatedUser`] 或 [`crate::account::Authorized`]。克隆句柄只会
    /// 复用初始化时传入的同一个 [`PgPool`]，不会创建第二个连接池或改变框架 Router 的认证
    /// 授权语义。
    ///
    /// [`Self::initialize`] 成功前返回 `None`。
    #[must_use]
    pub fn account(&self) -> Option<crate::account::Account> {
        self.account_routes
            .as_ref()
            .map(|routes| routes.account.clone())
    }

    /// 返回已经初始化的 ZITADEL provisioning/admin client。
    ///
    /// 该 client 不绑定默认 Account 的固定 Organization；宿主可把它放入自己的 State，为客户
    /// portal 动态创建 ZITADEL Organization、portal Project Grant、人类用户和用户 Project
    /// authorization。它不会把 portal 用户加入默认内部 Account `/users` 管理列表。
    ///
    /// [`Self::initialize`] 成功前返回 `None`。
    #[must_use]
    pub fn zitadel_provisioning_client(&self) -> Option<ZitadelProvisioningClient> {
        self.account_routes
            .as_ref()
            .map(|routes| routes.provisioning_client.clone())
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
    /// ZITADEL provisioning/admin client 初始化失败。
    #[cfg(feature = "server")]
    #[error("ZITADEL provisioning client 初始化失败")]
    Provisioning(
        /// ZITADEL provisioning client 返回的配置或 TLS 错误。
        #[from]
        crate::account::server::ZitadelProvisioningError,
    ),
}

/// 返回 Nexora 服务端模块维护的全部嵌入式 SQLx 迁移。
///
/// 宿主应把返回列表与自己的业务迁移合并，拒绝跨来源版本冲突，并构造唯一的 SQLx
/// `Migrator` 执行一次。该函数只克隆内嵌迁移元数据，不访问数据库，也不创建连接池。
#[must_use]
pub fn migrations() -> Vec<sqlx::migrate::Migration> {
    ::migrate::migrations()
}

fn setup_url(address: SocketAddr) -> String {
    let host = match address.ip() {
        address if address.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    format!("http://{host}:{}/setup", address.port())
}
