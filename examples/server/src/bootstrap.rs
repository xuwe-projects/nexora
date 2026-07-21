//! 服务端连接池与账号模块的启动装配。

use std::{sync::Arc, time::Duration};

use account::{
    Account, AccountDependencies,
    authentication::{OidcAccessTokenVerifier, VerificationError},
    directory::{DirectoryError, ZitadelUserDirectory},
};
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

use crate::{config::ServerConfig, routers::AppState};

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// 服务端启动完成后交给路由层的应用 State 与账号模块。
pub(crate) struct InitializedServer {
    pub(crate) state: AppState,
    pub(crate) account: Account,
    pub(crate) directory: ZitadelUserDirectory,
    pub(crate) system_initialized: bool,
}

/// 创建唯一 PostgreSQL 连接池、执行统一迁移、初始化 OIDC verifier 并装配账号模块。
///
/// 本示例只包含 Nexora 自身迁移，因此直接用完整框架列表构造一个 SQLx `Migrator`。宿主
/// 应用还必须先合并自己的业务迁移并检查版本冲突，不能依次运行两个 Migrator。
///
/// # Errors
///
/// PostgreSQL 连接、OIDC discovery、部署 issuer 核对、用户目录或账号模块状态读取失败时
/// 返回 [`BootstrapError`]。
pub async fn initialize(config: &ServerConfig) -> Result<InitializedServer, BootstrapError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect(config.database.url.as_str())
        .await
        .map_err(BootstrapError::Database)?;
    sqlx::migrate::Migrator::with_migrations(migrate::migrations())
        .run(&pool)
        .await
        .map_err(BootstrapError::Migration)?;
    let verifier = OidcAccessTokenVerifier::discover(
        config.oidc.issuer_url.as_str(),
        config.oidc.audience.clone(),
    )
    .await?;
    Account::bind_identity_issuer(&pool, config.oidc.issuer_url.as_str()).await?;
    let directory = ZitadelUserDirectory::new(
        &config.oidc.issuer_url,
        config.oidc.personal_access_token(),
        config.oidc.organization_id(),
        config.oidc.project_id(),
    )?;
    let account = Account::new(AccountDependencies {
        pool: pool.clone(),
        token_verifier: Arc::new(verifier),
        identity_directory: Some(Arc::new(directory.clone())),
        avatar_storage: None,
    });
    let system_initialized = account.is_system_initialized().await?;
    if system_initialized {
        let system_roles = account.system_roles().await?;
        tracing::info!(
            business_operation = "system_role_reconciliation",
            stage = "synchronize_initialized_system_roles",
            role_count = system_roles.len(),
            outcome = "started",
            "开始为已初始化系统补齐认证授权 Project 角色"
        );
        directory
            .ensure_project_roles(system_roles.as_slice())
            .await?;
        tracing::info!(
            business_operation = "system_role_reconciliation",
            stage = "synchronize_initialized_system_roles",
            role_count = system_roles.len(),
            outcome = "succeeded",
            "已初始化系统的认证授权 Project 角色已全部存在"
        );
    }

    Ok(InitializedServer {
        state: AppState::new(pool),
        account,
        directory,
        system_initialized,
    })
}

/// 服务端启动装配阶段可能产生的错误。
#[derive(Debug, Error)]
pub enum BootstrapError {
    /// SQLx 无法使用配置建立 PostgreSQL 连接池。
    #[error("无法连接 PostgreSQL，请检查数据库服务、网络地址、端口和凭据")]
    Database(
        /// SQLx 返回的底层连接错误，仅用于脱敏后的错误链诊断。
        #[source]
        sqlx::Error,
    ),
    /// SQLx 无法使用统一迁移清单升级数据库结构。
    #[error("无法应用服务端数据库迁移")]
    Migration(
        /// SQLx Migrator 返回的底层错误，仅用于启动诊断。
        #[source]
        sqlx::migrate::MigrateError,
    ),
    /// OIDC discovery 或 JWKS 初始化失败。
    #[error("无法初始化 OIDC access token 验证器")]
    Oidc(
        /// Token verifier 配置或 Provider 错误。
        #[from]
        VerificationError,
    ),
    /// 认证授权 gRPC 配置、channel 或系统角色同步失败。
    #[error("认证授权 gRPC 客户端初始化或系统角色同步失败")]
    Directory(
        /// 用户目录或 Project 角色同步返回的底层错误。
        #[from]
        DirectoryError,
    ),
    /// 账号模块无法核对部署 issuer、读取初始化状态或加载系统角色。
    #[error("账号模块初始化、部署 issuer 核对或状态读取失败")]
    Account(
        /// 账号领域返回的底层错误。
        #[from]
        account::AccountError,
    ),
}
