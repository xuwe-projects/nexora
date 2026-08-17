//! Account 桌面端的 OIDC 配置与强类型 HTTP 客户端。
//!
//! OIDC 仍负责取得短期 access token；[`AccountClient`] 只把该 token 放入内存中的
//! [`AccountSession`]，并使用共享 contracts 调用宿主合并后的 Account Router。

use std::time::Duration;

use contracts::{
    account::{
        AccessProfileResponse, CreateRoleRequest, CreateServiceAccountRequest, PermissionResponse,
        ProvisionUserRequest, ReplaceRolePermissionsRequest, ReplaceUserRolesRequest, RoleResponse,
        UpdateRoleRequest, UpdateServiceAccountRequest, UpdateUserStatusRequest, UserListQuery,
        UserPageResponse, UserResponse,
    },
    collection::ItemsResponse,
    error::ErrorEnvelope,
};
use reqwest::{
    Method,
    blocking::{Client, RequestBuilder, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use thiserror::Error;
use url::{Host, Url};

use crate::config::{__private::ProvidesAccountClientSettings, AccountClientSection, ConfigError};

#[path = "client/runtime.rs"]
mod runtime;

pub use contracts::account as contract;
pub use oidc::{
    OidcClient, OidcConfig, OidcError, OidcPrompt, OidcRevocationResult, OidcSession,
    OidcTokenCache, PendingOidcLogin,
};
pub(crate) use runtime::observe_authentication_in;
pub use runtime::{
    AccountAuthenticationScope, AccountLoginFailure, AccountLoginRuntimeError,
    AccountLoginSnapshot, api_session, authentication_scope, install_authenticator,
    is_authenticated, login_profile, login_session, login_snapshot, login_with_other_account,
    observe_authentication, retry_recovery, set_remember_login, sign_out, start_login,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// 桌面 Account 模块需要的 OIDC public client 配置。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// 标准 OIDC Authorization Code + PKCE 客户端配置。
    pub oidc: OidcSettings,
}

/// 桌面根配置中访问宿主 HTTP API 的配置。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApiSettings {
    /// API 根地址，例如 `http://127.0.0.1:3000` 或 `https://api.example.com`。
    pub endpoint: String,
    /// 是否显式允许连接非 loopback 的纯 HTTP API。
    ///
    /// 默认值为 `false`：HTTPS 始终允许，本机 loopback HTTP 仍可用于开发；其他 HTTP
    /// 地址会被拒绝。设为 `true` 时，调用方必须已经接受 Bearer Token 会以明文传输的
    /// 风险，仅应在内网或其他受控环境使用。
    #[serde(default)]
    pub allow_insecure_http: bool,
}

/// 桌面 public client 的 OIDC 参数。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OidcSettings {
    /// Provider 的规范 issuer URL。
    pub issuer_url: String,
    /// 桌面 public client 的 client ID。
    pub client_id: String,
    /// 授权请求需要申请的 scope；为空时仍会由 OIDC 客户端补充基础 scope。
    #[serde(default)]
    pub scopes: Vec<String>,
    /// 本地 loopback redirect URI 模板，例如 `http://127.0.0.1:0/auth/callback`。
    pub redirect_uri: String,
}

impl AccountClientSection for Settings {
    fn validate_account_client(&self) -> Result<(), ConfigError> {
        oidc_config_from(self)
            .map(|_| ())
            .map_err(|error| ConfigError::invalid_section("account.client", error.to_string()))
    }
}

/// 已校验、可直接创建 OIDC 与 Account HTTP 客户端的桌面配置。
#[derive(Clone, PartialEq, Eq)]
pub struct AccountClientConfig {
    oidc: OidcConfig,
    api_endpoint: Url,
}

impl std::fmt::Debug for AccountClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountClientConfig")
            .field("oidc", &"configured")
            .field("api_endpoint", &"redacted")
            .finish()
    }
}

impl AccountClientConfig {
    /// 返回 OIDC Authorization Code + PKCE 客户端配置。
    pub const fn oidc(&self) -> &OidcConfig {
        &self.oidc
    }

    /// 返回保证以 `/` 结尾且不包含凭据、query 或 fragment 的 API 根地址。
    pub const fn api_endpoint(&self) -> &Url {
        &self.api_endpoint
    }
}

/// 创建桌面 Account 配置失败的原因。
#[derive(Debug, Error)]
pub enum AccountClientConfigError {
    /// OIDC issuer、client ID 或 loopback redirect 配置无效。
    #[error(transparent)]
    Oidc(
        /// OIDC crate 返回的具体配置错误。
        #[from]
        OidcError,
    ),
    /// 业务 API 根地址无法安全用于 Account 请求。
    #[error("Account API endpoint 无效: {0}")]
    InvalidApiEndpoint(
        /// 不包含 endpoint 原值或其他敏感配置的约束说明。
        &'static str,
    ),
}

/// 从根 API 配置与派生宏标记的桌面 Account 配置段创建完整客户端配置。
///
/// # Errors
///
/// 根配置没有使用 `#[nexora(account_client)]` 标记标准 [`Settings`] 字段时无法通过编译；
/// OIDC 或 API endpoint 无效时返回 [`AccountClientConfigError`]。
pub fn client_config<S>(
    settings: &S,
    api: &ApiSettings,
) -> Result<AccountClientConfig, AccountClientConfigError>
where
    S: ProvidesAccountClientSettings<AccountClientSettings = Settings>,
{
    client_config_from(settings.account_client_settings(), api)
}

/// 从派生宏标记的桌面 Account 配置段创建已校验的 [`OidcConfig`]。
///
/// # Errors
///
/// 根配置没有使用 `#[nexora(account_client)]` 标记标准 [`Settings`] 字段时无法通过
/// 编译；issuer、client ID、scope 或 loopback redirect 无效时返回 [`OidcError`]。
pub fn oidc_config<S>(settings: &S) -> Result<OidcConfig, OidcError>
where
    S: ProvidesAccountClientSettings<AccountClientSettings = Settings>,
{
    oidc_config_from(settings.account_client_settings())
}

fn client_config_from(
    settings: &Settings,
    api: &ApiSettings,
) -> Result<AccountClientConfig, AccountClientConfigError> {
    Ok(AccountClientConfig {
        oidc: oidc_config_from(settings)?,
        api_endpoint: validated_api_endpoint(api.endpoint.as_str(), api.allow_insecure_http)?,
    })
}

fn oidc_config_from(settings: &Settings) -> Result<OidcConfig, OidcError> {
    let mut scopes = settings.oidc.scopes.clone();
    if !scopes
        .iter()
        .flat_map(|scope| scope.split_whitespace())
        .any(|scope| scope == "offline_access")
    {
        scopes.push("offline_access".to_owned());
    }
    OidcConfig::new(
        settings.oidc.issuer_url.trim(),
        settings.oidc.client_id.trim().to_owned(),
        scopes,
        settings.oidc.redirect_uri.trim(),
    )
}

/// 可复用的 Account HTTP 客户端。
///
/// 该类型不保存 access token；每次登录或刷新完成后，使用 [`Self::session`] 创建只存在于
/// 内存的认证会话。同步请求应放到 GPUI 后台执行器或专用工作线程中运行。
#[derive(Clone)]
pub struct AccountClient {
    http: Client,
    api_endpoint: Url,
}

impl std::fmt::Debug for AccountClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountClient")
            .finish_non_exhaustive()
    }
}

impl AccountClient {
    /// 使用已校验配置和统一 15 秒超时创建客户端。
    ///
    /// # Errors
    ///
    /// 当前平台无法构造 Reqwest 客户端时返回
    /// [`AccountClientError::ClientInitialization`]。
    pub fn new(config: &AccountClientConfig) -> Result<Self, AccountClientError> {
        Ok(Self {
            http: Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .map_err(|_| AccountClientError::ClientInitialization)?,
            api_endpoint: config.api_endpoint.clone(),
        })
    }

    /// 使用当前短期 access token 创建一个认证 API 会话。
    pub fn session(&self, access_token: impl Into<String>) -> AccountSession {
        AccountSession {
            http: self.http.clone(),
            api_endpoint: self.api_endpoint.clone(),
            access_token: access_token.into(),
        }
    }
}

/// 把 OIDC Authorization Code + PKCE 与 Account `/me` 门禁组合成一次登录流程。
///
/// UI 仍由应用决定如何打开系统浏览器和展示进度；该类型负责 Provider discovery、loopback
/// callback、token 交换以及业务账号存在性/状态校验，避免每个桌面应用重复拼装协议步骤。
#[derive(Clone)]
pub struct AccountAuthenticator {
    oidc: OidcClient,
    account: AccountClient,
}

impl std::fmt::Debug for AccountAuthenticator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountAuthenticator")
            .finish_non_exhaustive()
    }
}

impl AccountAuthenticator {
    /// 从同一份已校验配置创建 OIDC 与 Account HTTP 客户端。
    ///
    /// # Errors
    ///
    /// 当前平台无法创建 OIDC 或 Account HTTP client 时返回结构化错误。
    pub fn new(config: &AccountClientConfig) -> Result<Self, AccountAuthenticationError> {
        Ok(Self {
            oidc: OidcClient::new(config.oidc.clone())?,
            account: AccountClient::new(config)?,
        })
    }

    /// 启动一次 OIDC Authorization Code + PKCE 登录。
    ///
    /// 调用方应打开返回值的授权 URL，并在后台线程调用 [`PendingAccountLogin::finish`]。
    ///
    /// # Errors
    ///
    /// Provider discovery、loopback listener 或 PKCE 随机值初始化失败时返回错误。
    pub fn begin_login(&self) -> Result<PendingAccountLogin, AccountAuthenticationError> {
        self.begin_login_with_prompt(None)
    }

    /// 启动一次可选账号选择器的 OIDC 登录。
    ///
    /// `prompt` 为 [`OidcPrompt::SelectAccount`] 时，Provider 会展示已有账号并允许用户
    /// 选择其他账号；传入 `None` 时与 [`Self::begin_login`] 相同。
    ///
    /// # Errors
    ///
    /// Provider discovery、loopback listener 或 PKCE 随机值初始化失败时返回错误。
    pub fn begin_login_with_prompt(
        &self,
        prompt: Option<OidcPrompt>,
    ) -> Result<PendingAccountLogin, AccountAuthenticationError> {
        Ok(PendingAccountLogin {
            pending: self.oidc.begin_login_with_prompt(prompt)?,
            account: self.account.clone(),
        })
    }

    /// 使用 refresh token 恢复 OIDC 会话，并再次通过 Account `/me` 门禁。
    ///
    /// # Errors
    ///
    /// refresh token 被拒绝、OIDC 响应无效，或本地账号不存在/被停用时返回错误。
    pub fn refresh(
        &self,
        tokens: &OidcTokenCache,
    ) -> Result<AccountLogin, AccountAuthenticationError> {
        let session = self.oidc.refresh(tokens)?;
        validate_account_login(&self.account, session)
    }

    /// 撤销内存中的 refresh token，不影响浏览器中的 Provider SSO 会话。
    ///
    /// # Errors
    ///
    /// Provider discovery、网络请求或撤销响应失败时返回 [`AccountAuthenticationError`]；
    /// Provider 未声明撤销端点时返回 `ProviderUnsupported` 结果而不是错误。
    pub fn revoke_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<OidcRevocationResult, AccountAuthenticationError> {
        Ok(self.oidc.revoke_refresh_token(refresh_token)?)
    }

    /// 对已有的 OIDC 会话重新执行 Account `/me` 业务门禁。
    ///
    /// # Errors
    ///
    /// access token 无法访问 Account API，或本地用户不存在/已停用时返回错误。
    pub fn validate(
        &self,
        session: OidcSession,
    ) -> Result<AccountLogin, AccountAuthenticationError> {
        validate_account_login(&self.account, session)
    }
}

/// 已启动、等待系统浏览器回调的 Account 登录流程。
pub struct PendingAccountLogin {
    pending: PendingOidcLogin,
    account: AccountClient,
}

impl PendingAccountLogin {
    /// 返回需要由桌面应用交给系统浏览器打开的 OIDC 授权 URL。
    pub fn authorization_url(&self) -> &str {
        self.pending.authorization_url()
    }

    /// 等待 loopback callback、交换 token，并请求 Account `/me`。
    ///
    /// 该阻塞操作应在 GPUI 后台执行器或专用工作线程中运行。
    ///
    /// # Errors
    ///
    /// OIDC 登录任一步骤失败，或业务服务拒绝当前账号时返回错误。
    pub fn finish(self) -> Result<AccountLogin, AccountAuthenticationError> {
        let session = self.pending.finish()?;
        validate_account_login(&self.account, session)
    }

    /// 等待 loopback callback，并允许宿主取消尚未完成的浏览器登录。
    ///
    /// # Errors
    ///
    /// 宿主取消、OIDC 登录失败，或业务服务拒绝当前账号时返回错误。
    pub fn finish_with_cancellation(
        self,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<AccountLogin, AccountAuthenticationError> {
        let session = self.pending.finish_with_cancellation(is_cancelled)?;
        validate_account_login(&self.account, session)
    }
}

/// 同时通过 OIDC 与 Account 业务门禁的登录结果。
///
/// 该类型不实现 `Debug`，避免 OIDC token 被调试日志意外输出。
pub struct AccountLogin {
    session: OidcSession,
    profile: AccessProfileResponse,
}

impl AccountLogin {
    /// 返回通过 Provider 验证的 OIDC 会话和 token 缓存。
    pub const fn session(&self) -> &OidcSession {
        &self.session
    }

    /// 返回业务服务确认后的本地用户、角色和权限快照。
    pub const fn profile(&self) -> &AccessProfileResponse {
        &self.profile
    }

    /// 拆分 OIDC 会话与 Account 权限快照，便于应用保存 token 或写入 Global。
    pub fn into_parts(self) -> (OidcSession, AccessProfileResponse) {
        (self.session, self.profile)
    }
}

/// 组合 OIDC 登录与 Account `/me` 门禁时的错误。
#[derive(Debug, Error)]
pub enum AccountAuthenticationError {
    /// Provider discovery、浏览器回调、token 交换或 refresh 失败。
    #[error(transparent)]
    Oidc(
        /// OIDC crate 返回的协议错误。
        #[from]
        OidcError,
    ),
    /// Account HTTP client 初始化或 `/me` 门禁失败。
    #[error(transparent)]
    Account(
        /// Account 客户端返回的结构化错误。
        #[from]
        AccountClientError,
    ),
}

fn validate_account_login(
    account: &AccountClient,
    session: OidcSession,
) -> Result<AccountLogin, AccountAuthenticationError> {
    let profile = account
        .session(session.tokens().access_token.clone())
        .me()?;
    Ok(AccountLogin { session, profile })
}

/// 携带当前短期 Bearer token 的 Account API 会话。
///
/// 该类型刻意不实现 `Debug`，防止 access token 被调试日志意外输出。
#[derive(Clone)]
pub struct AccountSession {
    http: Client,
    api_endpoint: Url,
    access_token: String,
}

impl AccountSession {
    /// 请求当前登录用户、角色和合并权限快照。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，或服务端拒绝当前 Bearer token/账号时返回错误。
    pub fn me(&self) -> Result<AccessProfileResponse, AccountClientError> {
        self.send_json(self.request(Method::GET, "me"))
    }

    /// 分页读取本地用户目录。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，或当前用户没有 `users:read` 权限时返回错误。
    pub fn list_users(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<UserPageResponse, AccountClientError> {
        self.list_users_filtered(UserListQuery {
            page: contracts::pagination::PageQuery { page, page_size },
            ..UserListQuery::default()
        })
    }

    /// 按分页和用户管理筛选条件读取本地用户目录。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，或当前用户没有 `users:read` 权限时返回错误。
    pub fn list_users_filtered(
        &self,
        query: UserListQuery,
    ) -> Result<UserPageResponse, AccountClientError> {
        let request = self.request(Method::GET, "users").query(&query);
        self.send_json(request)
    }

    /// 显式开通一个已经由管理员确认的 OIDC 外部身份，并原子授予请求中的初始角色。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，身份已存在，或当前用户没有 `users:provision` 权限时
    /// 返回错误。
    pub fn provision_user(
        &self,
        request: &ProvisionUserRequest,
    ) -> Result<UserResponse, AccountClientError> {
        self.send_json(self.request(Method::POST, "users").json(request))
    }

    /// 创建一个不带默认 `member` 角色且不自动生成凭据的服务账号。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败、username 冲突，或当前用户缺少服务账号开通权限时返回错误。
    pub fn create_service_account(
        &self,
        request: &CreateServiceAccountRequest,
    ) -> Result<UserResponse, AccountClientError> {
        self.send_json(self.request(Method::POST, "service-accounts").json(request))
    }

    /// 局部修改服务账号展示名称或说明。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败、目标不存在，或当前用户缺少资料管理权限时返回错误。
    pub fn update_service_account(
        &self,
        service_account_id: &str,
        request: &UpdateServiceAccountRequest,
    ) -> Result<UserResponse, AccountClientError> {
        self.send_json(
            self.request(
                Method::PATCH,
                format!("service-accounts/{service_account_id}"),
            )
            .json(request),
        )
    }

    /// 读取指定用户及其直接角色和合并权限。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，用户不存在，或当前用户没有 `users:read` 权限时返回错误。
    pub fn get_user(&self, user_id: &str) -> Result<AccessProfileResponse, AccountClientError> {
        self.send_json(self.request(Method::GET, format!("users/{user_id}")))
    }

    /// 修改指定用户的访问状态。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，目标用户不可修改，或当前用户没有 `users:status.write`
    /// 权限时返回错误。
    pub fn update_user_status(
        &self,
        user_id: &str,
        request: &UpdateUserStatusRequest,
    ) -> Result<UserResponse, AccountClientError> {
        self.send_json(
            self.request(Method::PATCH, format!("users/{user_id}"))
                .json(request),
        )
    }

    /// 原子替换指定用户的直接角色集合。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，用户或角色不存在，或当前用户没有 `users:roles.write`
    /// 权限时返回错误。
    pub fn replace_user_roles(
        &self,
        user_id: &str,
        request: &ReplaceUserRolesRequest,
    ) -> Result<AccessProfileResponse, AccountClientError> {
        self.send_json(
            self.request(Method::PUT, format!("users/{user_id}/roles"))
                .json(request),
        )
    }

    /// 读取全部角色及其直接权限。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，或当前用户没有 `roles:read` 权限时返回错误。
    pub fn list_roles(&self) -> Result<Vec<RoleResponse>, AccountClientError> {
        let response: ItemsResponse<RoleResponse> =
            self.send_json(self.request(Method::GET, "roles"))?;
        Ok(response.items)
    }

    /// 创建自定义角色。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，角色键冲突，或当前用户没有 `roles:write` 权限时返回错误。
    pub fn create_role(
        &self,
        request: &CreateRoleRequest,
    ) -> Result<RoleResponse, AccountClientError> {
        self.send_json(self.request(Method::POST, "roles").json(request))
    }

    /// 修改指定自定义角色的名称或说明。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，角色不存在/不可修改，或当前用户没有 `roles:write` 权限时
    /// 返回错误。
    pub fn update_role(
        &self,
        role_id: i64,
        request: &UpdateRoleRequest,
    ) -> Result<RoleResponse, AccountClientError> {
        self.send_json(
            self.request(Method::PATCH, format!("roles/{role_id}"))
                .json(request),
        )
    }

    /// 删除指定自定义角色。
    ///
    /// # Errors
    ///
    /// 网络、响应处理失败，角色不存在/不可删除，或当前用户没有 `roles:write` 权限时返回
    /// 错误。
    pub fn delete_role(&self, role_id: i64) -> Result<(), AccountClientError> {
        self.send_empty(self.request(Method::DELETE, format!("roles/{role_id}")))
    }

    /// 原子替换指定角色的直接权限集合。
    ///
    /// # Errors
    ///
    /// 网络、请求/响应处理失败，角色或权限不存在，或当前用户没有 `roles:write` 权限时
    /// 返回错误。
    pub fn replace_role_permissions(
        &self,
        role_id: i64,
        request: &ReplaceRolePermissionsRequest,
    ) -> Result<RoleResponse, AccountClientError> {
        self.send_json(
            self.request(Method::PUT, format!("roles/{role_id}/permissions"))
                .json(request),
        )
    }

    /// 读取系统支持的完整权限目录。
    ///
    /// # Errors
    ///
    /// 网络、响应解析失败，或当前用户没有 `permissions:read` 权限时返回错误。
    pub fn list_permissions(&self) -> Result<Vec<PermissionResponse>, AccountClientError> {
        let response: ItemsResponse<PermissionResponse> =
            self.send_json(self.request(Method::GET, "permissions"))?;
        Ok(response.items)
    }

    fn request(&self, method: Method, path: impl AsRef<str>) -> RequestBuilder {
        self.http
            .request(method, self.endpoint(path.as_ref()))
            .bearer_auth(self.access_token.as_str())
    }

    fn endpoint(&self, path: &str) -> Url {
        self.api_endpoint
            .join(path.trim_start_matches('/'))
            .expect("已校验的 API 根地址必须能够拼接相对资源路径")
    }

    fn send_json<T>(&self, request: RequestBuilder) -> Result<T, AccountClientError>
    where
        T: DeserializeOwned,
    {
        let response = request.send().map_err(classify_request_error)?;
        if response.status().is_success() {
            let status = response.status().as_u16();
            let request_id = response_request_id(&response);
            let body = response
                .bytes()
                .map_err(|_| AccountClientError::ResponseRead {
                    status,
                    request_id: request_id.clone(),
                })?;
            return serde_json::from_slice(&body)
                .map_err(|_| AccountClientError::Contract { status, request_id });
        }
        Err(rejected(response))
    }

    fn send_empty(&self, request: RequestBuilder) -> Result<(), AccountClientError> {
        let response = request.send().map_err(classify_request_error)?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(rejected(response))
    }
}

/// Account API 配置、网络或服务端拒绝错误。
#[derive(Error)]
pub enum AccountClientError {
    /// 当前平台无法初始化 HTTP client。
    #[error("无法初始化 Account HTTP 客户端")]
    ClientInitialization,
    /// 请求未能与 Account 服务建立连接。
    #[error("无法连接 Account 服务")]
    Connection,
    /// Account 请求超过客户端截止时间。
    #[error("Account 服务请求超时")]
    Timeout,
    /// 请求在连接成功前因其他传输原因失败。
    #[error("Account 服务请求失败")]
    Request,
    /// 已收到 HTTP 响应头，但无法完整读取响应体。
    #[error("Account 服务响应读取失败（status={status}）")]
    ResponseRead {
        /// HTTP 状态码。
        status: u16,
        /// 响应头中可用的请求 ID。
        request_id: Option<String>,
    },
    /// 成功 HTTP 响应不符合当前 Account 契约。
    #[error("Account 服务成功响应与客户端契约不兼容（status={status}）")]
    Contract {
        /// HTTP 状态码。
        status: u16,
        /// 响应头中可用的请求 ID。
        request_id: Option<String>,
    },
    /// 服务端使用统一错误契约拒绝请求。
    #[error("Account API 拒绝请求: {message}（code={code}, request_id={request_id}）")]
    Rejected {
        /// HTTP 状态码。
        status: u16,
        /// 服务端稳定错误码。
        code: String,
        /// 适合展示给当前用户的错误说明。
        message: String,
        /// 用于服务端日志检索的请求 ID。
        request_id: String,
    },
    /// 服务端返回了非结构化错误响应。
    #[error("Account 服务返回无法识别的错误响应（status={status}）")]
    UnexpectedResponse {
        /// HTTP 状态码。
        status: u16,
        /// 响应头中可用的请求 ID。
        request_id: Option<String>,
    },
}

impl std::fmt::Debug for AccountClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountClientError")
            .field("category", &self.category())
            .field("status", &self.status())
            .field("request_id", &self.request_id())
            .finish()
    }
}

impl AccountClientError {
    /// 返回不包含 endpoint、token 或响应正文的稳定阶段名称。
    pub const fn category(&self) -> &'static str {
        match self {
            Self::ClientInitialization => "client_initialization",
            Self::Connection => "connection",
            Self::Timeout => "timeout",
            Self::Request => "request",
            Self::ResponseRead { .. } => "response_read",
            Self::Contract { .. } => "contract",
            Self::Rejected { .. } => "rejected",
            Self::UnexpectedResponse { .. } => "unexpected_response",
        }
    }

    /// 返回已收到响应的 HTTP 状态码。
    pub const fn status(&self) -> Option<u16> {
        match self {
            Self::ResponseRead { status, .. }
            | Self::Contract { status, .. }
            | Self::Rejected { status, .. }
            | Self::UnexpectedResponse { status, .. } => Some(*status),
            Self::ClientInitialization | Self::Connection | Self::Timeout | Self::Request => None,
        }
    }

    /// 返回服务端传回的安全请求 ID。
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::ResponseRead { request_id, .. }
            | Self::Contract { request_id, .. }
            | Self::UnexpectedResponse { request_id, .. } => request_id.as_deref(),
            Self::Rejected { request_id, .. }
                if !request_id.trim().is_empty() && request_id != "unknown" =>
            {
                Some(request_id)
            }
            Self::ClientInitialization
            | Self::Connection
            | Self::Timeout
            | Self::Request
            | Self::Rejected { .. } => None,
        }
    }

    /// 返回该错误是否明确表示当前 Account 恢复凭据已永久失效。
    ///
    /// 连接、超时、临时服务拒绝、响应读取和契约不兼容均返回 `false`；
    /// 只有服务端稳定错误码已明确证明本地账号不可继续使用时才返回 `true`。
    pub fn is_permanent_authentication_failure(&self) -> bool {
        matches!(
            self,
            Self::Rejected { code, .. }
                if matches!(
                    code.as_str(),
                    "account_suspended" | "account_not_registered"
                )
        )
    }

    /// 返回适合在桌面界面中展示且不包含 token 的错误信息。
    pub fn user_message(&self) -> String {
        match self {
            Self::Rejected {
                message,
                request_id,
                ..
            } if request_id != "unknown" => format!("{message}（请求 ID：{request_id}）"),
            Self::Rejected { message, .. } => message.clone(),
            Self::ClientInitialization => "无法初始化 Account 客户端，请重试".to_owned(),
            Self::Connection => "无法连接 Account 服务，请检查网络或稍后重试".to_owned(),
            Self::Timeout => "Account 服务请求超时，请重试".to_owned(),
            Self::Request => "Account 服务请求未完成，请重试".to_owned(),
            Self::ResponseRead { .. } => "读取 Account 服务响应失败，请重试".to_owned(),
            Self::Contract { .. } => "客户端与 Account 服务版本或响应契约不兼容".to_owned(),
            Self::UnexpectedResponse { .. } => "Account 服务返回了无法识别的错误响应".to_owned(),
        }
    }
}

fn validated_api_endpoint(
    endpoint: &str,
    allow_insecure_http: bool,
) -> Result<Url, AccountClientConfigError> {
    let mut endpoint = Url::parse(endpoint.trim())
        .map_err(|_| AccountClientConfigError::InvalidApiEndpoint("endpoint 必须是有效绝对 URL"))?;
    if endpoint.host().is_none() {
        return Err(AccountClientConfigError::InvalidApiEndpoint(
            "endpoint 必须包含主机",
        ));
    }
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(AccountClientConfigError::InvalidApiEndpoint(
            "endpoint 不能包含凭据、query 或 fragment",
        ));
    }
    if endpoint.path() != "/" {
        return Err(AccountClientConfigError::InvalidApiEndpoint(
            "endpoint 必须指向服务根路径",
        ));
    }
    match endpoint.scheme() {
        "https" => {}
        "http" if is_loopback(&endpoint) || allow_insecure_http => {}
        "http" => {
            return Err(AccountClientConfigError::InvalidApiEndpoint(
                "远程 HTTP endpoint 需要显式启用 allow_insecure_http；纯 HTTP 会明文传输 Bearer Token",
            ));
        }
        _ => {
            return Err(AccountClientConfigError::InvalidApiEndpoint(
                "endpoint scheme 必须是 http 或 https",
            ));
        }
    }
    endpoint.set_path("/");
    Ok(endpoint)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn rejected(response: Response) -> AccountClientError {
    let status = response.status();
    let header_request_id = response_request_id(&response);
    let body = match response.bytes() {
        Ok(body) => body,
        Err(_) => {
            return AccountClientError::ResponseRead {
                status: status.as_u16(),
                request_id: header_request_id,
            };
        }
    };
    let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(&body) else {
        return AccountClientError::UnexpectedResponse {
            status: status.as_u16(),
            request_id: header_request_id,
        };
    };
    AccountClientError::Rejected {
        status: status.as_u16(),
        code: envelope.error.code,
        message: envelope.error.message,
        request_id: (!envelope.error.request_id.trim().is_empty())
            .then_some(envelope.error.request_id)
            .or(header_request_id)
            .unwrap_or_else(|| "unknown".to_owned()),
    }
}

fn response_request_id(response: &Response) -> Option<String> {
    response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn classify_request_error(reason: reqwest::Error) -> AccountClientError {
    if reason.is_timeout() {
        AccountClientError::Timeout
    } else if reason.is_connect() {
        AccountClientError::Connection
    } else {
        AccountClientError::Request
    }
}
