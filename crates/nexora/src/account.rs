//! Nexora Account 客户端与服务端能力的统一公开入口。
//!
//! Cargo feature 只决定编译哪一侧能力：桌面程序使用 `account-client`，服务端使用
//! `account-server`。服务端 Account 只提供依赖装配、业务用例和可合并 Router，不负责
//! 监听端口或启动宿主应用。

#[cfg(feature = "account-server")]
pub use crate::account_module::{
    Account, AccountDependencies, AccountError, AccountInitialization,
    AccountInitializationOutcome, AccountInitializationStatus, ExternalIdentity,
    IdentityIssuerBindingOutcome, User,
    authentication::{AccessTokenVerifier, VerifiedIdentity},
};

/// 桌面端 OIDC 登录与账号 HTTP 契约。
#[cfg(feature = "account-client")]
pub mod client;

/// 服务端 Account Router 与 OIDC Bearer verifier 装配能力。
#[cfg(feature = "account-server")]
pub mod server {
    use std::sync::Arc;

    use ::migrate as migration;
    use serde::Deserialize;
    use sqlx::PgPool;
    use thiserror::Error;
    use url::{Host, Url};

    use crate::{
        account_module::{
            Account, AccountDependencies, AccountError,
            authentication::{OidcAccessTokenVerifier, VerificationError},
        },
        config::{__private::ProvidesAccountServerSettings, AccountServerSection, ConfigError},
    };

    pub use migration::{MigrationError, MigrationOptions, MigrationReport};

    /// Account 资源服务器运行所需的标准配置段。
    #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Settings {
        /// 服务端验证 Bearer access token 使用的 OIDC 参数。
        pub oidc: OidcSettings,
    }

    /// OIDC resource server 的 issuer 与 audience 配置。
    #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct OidcSettings {
        /// Provider 的规范 HTTPS issuer URL；本地开发可使用 loopback HTTP。
        pub issuer_url: String,
        /// Access token 的 `aud` claim 必须包含的资源服务标识。
        pub audience: String,
    }

    impl AccountServerSection for Settings {
        fn validate_account_server(&self) -> Result<(), ConfigError> {
            validate_oidc(&self.oidc)
        }
    }

    /// 根据强类型根配置发现 OIDC Provider 并创建 Account 外部依赖。
    ///
    /// 该函数不会执行迁移、启动 HTTP Server 或合并 Router；宿主仍需把返回值传给
    /// `Account::new`，并显式调用 `account.routers::<AppState>()`。
    ///
    /// # Errors
    ///
    /// OIDC discovery、Provider 元数据或 JWKS 初始化失败，数据库无法绑定部署 issuer，
    /// 或配置 issuer 与该部署首次绑定值不一致时返回结构化错误。根配置没有使用
    /// `#[nexora(account_server)]` 标记标准 [`Settings`] 字段时会在编译期失败。
    pub async fn dependencies<S>(
        pool: PgPool,
        settings: &S,
    ) -> Result<AccountDependencies, AccountServerInitializationError>
    where
        S: ProvidesAccountServerSettings<AccountServerSettings = Settings>,
    {
        let settings = settings.account_server_settings();
        let verifier = OidcAccessTokenVerifier::discover(
            settings.oidc.issuer_url.trim(),
            settings.oidc.audience.trim().to_owned(),
        )
        .await?;
        Account::bind_identity_issuer(&pool, settings.oidc.issuer_url.trim()).await?;

        Ok(AccountDependencies {
            pool,
            token_verifier: Arc::new(verifier),
        })
    }

    /// 对共享 PostgreSQL 连接池执行 Nexora 集中维护的安全向前迁移。
    ///
    /// 该函数不会创建连接池或启动 HTTP 服务。首次安装空数据库时，宿主必须通过
    /// [`MigrationOptions::initialize_empty_database`] 显式授权；已有未知 schema、失败迁移
    /// 或核心表缺失时会 fail closed。
    ///
    /// # Errors
    ///
    /// 数据库状态检查、安全约束或任一向前迁移失败时返回 [`MigrationError`]。
    pub async fn migrate(
        pool: &PgPool,
        options: MigrationOptions,
    ) -> Result<MigrationReport, MigrationError> {
        migration::prepare(pool, options).await?.run(pool).await
    }

    /// Account 服务端依赖装配失败原因。
    #[derive(Debug, Error)]
    pub enum AccountServerInitializationError {
        /// OIDC discovery、Provider 元数据或 JWKS 无法用于验证 access token。
        #[error("无法初始化 Account OIDC access token 验证器")]
        Oidc(
            /// Account 模块保留的 OIDC 验证器初始化错误。
            #[from]
            VerificationError,
        ),
        /// 配置 issuer 无效、数据库不可访问，或与部署首次绑定的 issuer 不一致。
        #[error("无法绑定或核对 Account 部署级 OIDC issuer")]
        IdentityIssuer(
            /// Account 模块返回的部署 issuer 绑定错误。
            #[from]
            AccountError,
        ),
    }

    fn validate_oidc(settings: &OidcSettings) -> Result<(), ConfigError> {
        let issuer = Url::parse(settings.issuer_url.trim()).map_err(|_| {
            ConfigError::invalid_section("account.server", "oidc.issuer_url 不是有效 URL")
        })?;
        let secure_transport = issuer.scheme() == "https"
            || (issuer.scheme() == "http"
                && match issuer.host() {
                    Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
                    Some(Host::Ipv4(address)) => address.is_loopback(),
                    Some(Host::Ipv6(address)) => address.is_loopback(),
                    None => false,
                });
        let valid_issuer = secure_transport
            && issuer.host_str().is_some()
            && issuer.username().is_empty()
            && issuer.password().is_none()
            && issuer.query().is_none()
            && issuer.fragment().is_none();
        if !valid_issuer {
            return Err(ConfigError::invalid_section(
                "account.server",
                "oidc.issuer_url 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP，且不能包含凭据、query 或 fragment",
            ));
        }
        if settings.oidc_audience().is_empty() {
            return Err(ConfigError::invalid_section(
                "account.server",
                "oidc.audience 不能为空",
            ));
        }
        Ok(())
    }

    impl OidcSettings {
        fn oidc_audience(&self) -> &str {
            self.audience.trim()
        }
    }
}
