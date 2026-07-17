//! Nexora 桌面端认证、会话与 Account HTTP 客户端 facade。
//!
//! 应用通过本模块安装认证协调器、读取登录状态并调用用户、角色和权限 API，不需要了解
//! Account 客户端的内部模块层级。

pub use crate::account::client::OidcSettings as AccountOidcSettings;
pub use crate::account::client::{
    AccountAuthenticationError, AccountAuthenticator, AccountClient, AccountClientConfig,
    AccountClientConfigError, AccountClientError, AccountLogin, AccountLoginFailure,
    AccountLoginRuntimeError, AccountLoginSnapshot, AccountSession, ApiSettings, OidcClient,
    OidcConfig, OidcError, OidcSession, OidcTokenCache, PendingAccountLogin, PendingOidcLogin,
    Settings as AccountSettings, api_session, client_config, contract, install_authenticator,
    is_authenticated, login_profile, login_session, login_snapshot, oidc_config, sign_out,
    start_login,
};
