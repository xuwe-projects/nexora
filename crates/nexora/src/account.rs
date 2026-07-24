//! Nexora Account 服务端业务能力的公开入口。
//!
//! 桌面认证与 Account HTTP 客户端统一从 [`crate::desktop`] 使用；本模块保留服务端领域
//! facade。默认 [`crate::Server`] 负责常用模块装配，宿主仍自行组合 Router 和启动 Axum。

#[cfg(feature = "server")]
pub use crate::account_module::{
    AccessProfile, Account, AccountDependencies, AccountError, AccountInitialization,
    AccountInitializationOutcome, AccountInitializationStatus, AvatarStorage, AvatarStorageError,
    AvatarUpload, CreateHumanIdentity, CreateHumanIdentityProvision, ExternalIdentity,
    IdentityDirectory, IdentityDirectoryError, IdentityIssuerBindingOutcome, LocalAvatarStorage,
    PORTAL_ADMIN_ROLE_KEY, Page, Permission, PermissionCatalogDefinition, PermissionDefinition,
    PermissionKey, Role, SYSTEM_ROLE_OWNER, SystemRole, User, UserStatus,
    authentication::{
        AccessTokenVerifier, BearerAccessToken, OidcAccessTokenVerifier, OidcResourceServer,
        VerifiedBearerIdentity, VerifiedIdentity, VerifiedOrganizationContext,
    },
    authorization::{AuthenticatedUser, Authorized, RequiredPermission},
    create_generated_role_for_owner, create_permission_catalog, create_permissions, create_role,
    create_role_for_owner, create_user, create_user_with_roles,
    ensure_system_role_with_permissions, grant_user_role, replace_role_permissions,
    replace_role_permissions_for_owner, replace_user_roles, replace_user_roles_for_owner,
    role_for_owner, roles_for_owner,
};

#[cfg(feature = "desktop")]
pub(crate) mod client;

#[cfg(feature = "server")]
#[path = "account/setup.rs"]
mod setup;

/// 服务端 Account Router 与 OIDC Bearer verifier 的内部装配能力。
///
/// 应用应使用 [`crate::Server`] 和 [`crate::server`] 暴露的公共服务端 API。
#[cfg(feature = "server")]
pub(crate) mod server {
    use std::{fmt, sync::Arc};

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

    #[cfg(feature = "server")]
    pub use super::setup::{
        DefaultSetup, DefaultSetupCompletionRequest, DefaultSetupUnlockRequest, Setup,
        SetupCompletionRequest, SetupUnlockRequest, setup_routes, setup_routes_with,
    };
    #[cfg(feature = "server")]
    pub use crate::account_module::directory::{
        DirectoryError, DirectoryUser, ZitadelUserDirectory,
    };
    #[cfg(feature = "server")]
    pub use crate::account_module::provisioning::{
        CreateZitadelOrganizationRequest, ZitadelAuthorization, ZitadelAuthorizationOutcome,
        ZitadelAuthorizationRequest, ZitadelDeleteUserOutcome, ZitadelOrganization,
        ZitadelOrganizationState, ZitadelProjectGrant, ZitadelProjectGrantOutcome,
        ZitadelProjectGrantRequest, ZitadelProjectGrantState, ZitadelProvisioningClient,
        ZitadelProvisioningError,
    };

    /// Account 资源服务器运行所需的标准配置段。
    #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Settings {
        /// 服务端验证 Bearer access token 使用的 OIDC 参数。
        pub oidc: OidcSettings,
    }

    /// OIDC resource server 与可选 ZITADEL 管理客户端配置。
    #[derive(Clone, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct OidcSettings {
        /// Provider 的规范 HTTPS issuer URL；本地开发可使用 loopback HTTP。
        pub issuer_url: String,
        /// Access token 的 `aud` claim 必须包含的资源服务标识。
        pub audience: String,
        /// ZITADEL 中创建人类用户的 Organization ID。
        #[cfg(feature = "server")]
        pub organization_id: String,
        /// ZITADEL 中承载本系统角色的 Project ID，与 API Application Client ID 含义不同。
        #[cfg(feature = "server")]
        pub project_id: String,
        /// 服务端调用 ZITADEL UserService 与 ProjectService 使用的服务账号 PAT。
        ///
        /// 生产部署应通过 `ACCOUNT__OIDC__PERSONAL_ACCESS_TOKEN` 等密钥注入方式提供，
        /// 不应把真实令牌提交到配置模板或版本库。
        #[cfg(feature = "server")]
        pub personal_access_token: String,
    }

    impl fmt::Debug for OidcSettings {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mut debug = formatter.debug_struct("OidcSettings");
            debug
                .field("issuer_url", &self.issuer_url)
                .field("audience", &self.audience);
            #[cfg(feature = "server")]
            debug
                .field("organization_id", &self.organization_id)
                .field("project_id", &self.project_id)
                .field("personal_access_token", &"[REDACTED]");
            debug.finish()
        }
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
            identity_directory: None,
            avatar_storage: None,
        })
    }

    /// 根据强类型根配置创建 ZITADEL 用户目录与 Project 角色管理客户端。
    ///
    /// # Errors
    ///
    /// issuer、Project ID、PAT 或 TLS 配置无法用于建立 ZITADEL gRPC 客户端时返回
    /// [`DirectoryError`]。
    #[cfg(feature = "server")]
    pub fn user_directory<S>(settings: &S) -> Result<ZitadelUserDirectory, DirectoryError>
    where
        S: ProvidesAccountServerSettings<AccountServerSettings = Settings>,
    {
        let settings = settings.account_server_settings();
        ZitadelUserDirectory::new(
            settings.oidc.issuer_url.as_str(),
            settings.oidc.personal_access_token.as_str(),
            settings.oidc.organization_id.as_str(),
            settings.oidc.project_id.as_str(),
        )
    }

    /// 根据强类型根配置创建 ZITADEL provisioning/admin client。
    ///
    /// 该 client 使用与默认 Account 目录相同的 issuer、PAT、TLS 与 loopback 安全规则，但不绑定
    /// 固定 Organization 或 Project。宿主可用它为 customer portal 动态创建 Organization、
    /// Project Grant、人类用户和 Project role authorization。
    ///
    /// # Errors
    ///
    /// issuer、PAT 或 TLS 配置无法用于建立 ZITADEL gRPC 客户端时返回
    /// [`ZitadelProvisioningError`]。
    #[cfg(feature = "server")]
    pub fn provisioning_client<S>(
        settings: &S,
    ) -> Result<ZitadelProvisioningClient, ZitadelProvisioningError>
    where
        S: ProvidesAccountServerSettings<AccountServerSettings = Settings>,
    {
        let settings = settings.account_server_settings();
        ZitadelProvisioningClient::new(
            settings.oidc.issuer_url.as_str(),
            settings.oidc.personal_access_token.as_str(),
        )
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
        #[cfg(feature = "server")]
        if settings.organization_id.trim().is_empty() {
            return Err(ConfigError::invalid_section(
                "account.server",
                "oidc.organization_id 不能为空",
            ));
        }
        #[cfg(feature = "server")]
        if settings.project_id.trim().is_empty() {
            return Err(ConfigError::invalid_section(
                "account.server",
                "oidc.project_id 不能为空",
            ));
        }
        #[cfg(feature = "server")]
        if settings.personal_access_token.trim().is_empty() {
            return Err(ConfigError::invalid_section(
                "account.server",
                "oidc.personal_access_token 不能为空",
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
