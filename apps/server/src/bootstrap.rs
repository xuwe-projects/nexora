//! 服务端连接池与账号模块的启动装配。

use std::{sync::Arc, time::Duration};

use account::{
    Account,
    authentication::{OidcAccessTokenVerifier, VerificationError},
};
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

use crate::{
    config::ServerConfig,
    routers::AppState,
    super_admin::{self, SuperAdminSetupError},
};

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// 服务端启动完成后交给路由层的应用 State 与账号模块。
pub(crate) struct InitializedServer {
    pub(crate) state: AppState,
    pub(crate) account: Account,
}

/// 创建唯一 PostgreSQL 连接池、初始化 OIDC verifier 并装配账号模块。
///
/// 数据库结构由独立的 `migrate` 程序管理；本函数不会在服务启动时隐式修改 schema。
///
/// # Errors
///
/// PostgreSQL 连接、OIDC discovery 或首次超级管理员绑定失败时返回 [`BootstrapError`]。
pub async fn initialize(config: &ServerConfig) -> Result<InitializedServer, BootstrapError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect(config.database.url.as_str())
        .await
        .map_err(BootstrapError::Database)?;
    let verifier = OidcAccessTokenVerifier::discover(
        config.oidc.issuer_url.as_str(),
        config.oidc.audience.clone(),
    )
    .await?;
    let account = Account::new(pool.clone(), Arc::new(verifier));
    super_admin::ensure_super_admin(&account, config).await?;

    Ok(InitializedServer {
        state: AppState::new(pool),
        account,
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
    /// OIDC discovery 或 JWKS 初始化失败。
    #[error("无法初始化 OIDC access token 验证器")]
    Oidc(
        /// Token verifier 配置或 Provider 错误。
        #[from]
        VerificationError,
    ),
    /// 首次启动超级管理员引导失败。
    #[error(transparent)]
    SuperAdmin(
        /// 超级管理员选择、目录读取或持久化错误。
        #[from]
        SuperAdminSetupError,
    ),
}
