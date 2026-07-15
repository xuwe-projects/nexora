//! Console 桌面应用认证状态。
//!
//! 该模块负责把可复用 `oidc` crate 和业务服务 `/me` 门禁接入 GPUI：读取认证与 API 配置、
//! 使用系统凭据库持久化 refresh token、维护应用级登录状态，并为根视图提供展示快照。

use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use configuration::{ConfigurationError, LayeredConfigLoader, UserConfigStore};
use contracts::{account::AccessProfileResponse, error::ErrorEnvelope};
use gpui::{
    AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Global, InteractiveElement as _,
    ReadGlobal as _, SharedString, Subscription, UpdateGlobal as _, Window,
};
use gpui_component::{IconName, WindowExt as _, button::Button, notification::Notification};
use oidc::{OidcClient, OidcConfig, OidcError, OidcSession, OidcTokenCache};
use reqwest::blocking::Client;
#[cfg(target_os = "macos")]
use security_framework::{
    base::Error as MacOsSecurityError,
    os::macos::keychain::{SecKeychain, SecPreferencesDomain},
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use url::{Host, Url};

const DEFAULT_DESKTOP_CONFIG_PATH: &str = "config/desktop.toml";
const DEFAULT_OIDC_SCOPES: &str = "openid profile email offline_access";
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(target_os = "macos")]
const MACOS_ERR_SEC_AUTH_FAILED: i32 = -25_293;
#[cfg(target_os = "macos")]
const MACOS_ERR_SEC_DUPLICATE_ITEM: i32 = -25_299;
#[cfg(target_os = "macos")]
const MACOS_ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25_308;

struct LoginFailureNotification;

/// Console 登录需要同时使用的 OIDC 客户端和业务 API 配置。
#[derive(Debug, Clone)]
pub struct AuthConfig {
    oidc: OidcConfig,
    api_base_url: Url,
}

impl AuthConfig {
    /// 使用 OIDC 配置和业务 API 根地址创建桌面登录配置。
    ///
    /// API 地址必须指向根路径；生产地址必须使用 HTTPS，仅 loopback 开发地址允许 HTTP。
    ///
    /// # Errors
    ///
    /// API 地址为空、不是绝对 URL、包含凭据/query/fragment/非根路径，或使用不安全的远程
    /// HTTP 时返回 [`AuthError`]。
    pub fn new(oidc: OidcConfig, api_base_url: &str) -> Result<Self, AuthError> {
        Ok(Self {
            oidc,
            api_base_url: validated_api_base_url(api_base_url)?,
        })
    }

    /// 返回系统凭据库隔离和 OIDC 登录使用的客户端配置。
    pub fn oidc(&self) -> &OidcConfig {
        &self.oidc
    }

    /// 使用当前 API 根地址和短期 access token 创建业务请求凭据快照。
    pub(crate) fn api_session(&self, access_token: impl Into<String>) -> ApiSession {
        ApiSession {
            api_base_url: self.api_base_url.clone(),
            access_token: access_token.into(),
        }
    }
}

/// 已登录桌面会话访问业务 API 所需的最小凭据快照。
///
/// 该值仅在 Console crate 内传给后台 HTTP 客户端；access token 不会写入日志或磁盘。
#[derive(Clone)]
pub(crate) struct ApiSession {
    api_base_url: Url,
    access_token: String,
}

impl ApiSession {
    /// 基于已校验的 API 根地址生成资源端点。
    pub(crate) fn endpoint(&self, path: &str) -> Url {
        self.api_base_url
            .join(path.trim_start_matches('/'))
            .expect("已校验的 API 根地址必须能够拼接相对资源路径")
    }

    /// 返回仅用于 Authorization Bearer 头的当前 access token。
    pub(crate) fn access_token(&self) -> &str {
        self.access_token.as_str()
    }
}

/// Console refresh token 的跨平台系统安全存储。
///
/// macOS、Windows 与 Linux 分别使用系统 Keychain、Credential Manager 与 Secret Service。
/// access token 和 ID Token 仅保留在内存中；旧版 `auth.toml` 会在首次成功迁移后删除。
#[derive(Debug, Clone)]
pub struct AuthTokenStore {
    service: String,
    user: String,
    legacy: UserConfigStore<OidcTokenCache>,
}

/// 系统凭据库与旧 token 文件迁移阶段的错误。
#[derive(Debug, Error)]
pub enum AuthTokenStoreError {
    /// 系统凭据库创建、读取、写入或删除失败。
    #[error("系统凭据库操作失败: {0}")]
    Keyring(
        /// 系统凭据后端返回的结构化错误。
        #[from]
        keyring::Error,
    ),
    /// 旧版明文 token 文件读取或删除失败。
    #[error(transparent)]
    Configuration(
        /// 旧配置文件读取、迁移或删除时返回的错误。
        #[from]
        ConfigurationError,
    ),
    /// 写入系统凭据库后无法读回相同 refresh token。
    #[error("系统凭据库写入校验失败")]
    Verification,
    /// macOS 登录钥匙串需要由用户解锁，但系统解锁流程没有完成。
    #[cfg(target_os = "macos")]
    #[error("macOS 登录钥匙串未解锁: {0}")]
    MacKeychainUnlock(
        /// Security.framework 返回的钥匙串解锁错误。
        #[from]
        MacOsSecurityError,
    ),
    /// macOS 登录钥匙串在系统解锁后仍拒绝当前进程访问。
    #[cfg(target_os = "macos")]
    #[error("macOS 登录钥匙串仍拒绝当前构建访问: {0}")]
    MacKeychainLocked(
        /// 系统凭据后端在重试时返回的错误。
        #[source]
        keyring::Error,
    ),
    /// macOS 登录钥匙串中已有当前进程无权更新的同名凭据。
    #[cfg(target_os = "macos")]
    #[error("macOS 登录钥匙串中的旧凭据不允许当前构建更新: {0}")]
    MacKeychainAccessDenied(
        /// 系统凭据后端在重试时返回的重复条目错误。
        #[source]
        keyring::Error,
    ),
}

impl AuthTokenStoreError {
    /// 返回适合显示在账户栏有限空间内的错误处理提示。
    pub(crate) fn user_message(&self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacKeychainUnlock(_) | Self::MacKeychainLocked(_) => "请解锁 macOS 登录钥匙串",
            #[cfg(target_os = "macos")]
            Self::MacKeychainAccessDenied(_) => "请清理旧的 macOS 登录凭据后重试",
            _ => "系统凭据库保存失败",
        }
    }
}

impl AuthTokenStore {
    fn new(config: &OidcConfig) -> Result<Self, ConfigurationError> {
        let identity = format!("{}\0{}", config.issuer_url(), config.client_id());
        let digest = Sha256::digest(identity.as_bytes());
        let user = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        Ok(Self {
            service: "com.xuwe.console.oidc".to_owned(),
            user: format!("active-session:{user}"),
            legacy: UserConfigStore::for_application("com", "Xuwe", "Console", "auth.toml")?,
        })
    }

    fn entry(&self) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(self.service.as_str(), self.user.as_str())
    }

    fn load_refresh_token(&self) -> Result<Option<String>, AuthTokenStoreError> {
        match self.run_keyring_operation(keyring::Entry::get_password) {
            Ok(token) => return Ok(non_empty(token)),
            Err(AuthTokenStoreError::Keyring(keyring::Error::NoEntry)) => {}
            Err(error) => return Err(error),
        }

        self.migrate_legacy_token()
    }

    fn save_tokens(&self, tokens: &OidcTokenCache) -> Result<(), AuthTokenStoreError> {
        let Some(refresh_token) = tokens.refresh_token.as_deref().and_then(non_empty_str) else {
            return self.clear();
        };
        self.run_keyring_operation(|entry| entry.set_password(refresh_token))
    }

    fn clear(&self) -> Result<(), AuthTokenStoreError> {
        match self.run_keyring_operation(keyring::Entry::delete_credential) {
            Ok(()) | Err(AuthTokenStoreError::Keyring(keyring::Error::NoEntry)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn run_keyring_operation<T>(
        &self,
        mut operation: impl FnMut(&keyring::Entry) -> Result<T, keyring::Error>,
    ) -> Result<T, AuthTokenStoreError> {
        let entry = self.entry()?;
        match operation(&entry) {
            Ok(value) => Ok(value),
            #[cfg(target_os = "macos")]
            Err(error) if macos_keychain_error_needs_unlock(&error) => {
                unlock_macos_login_keychain()?;
                operation(&entry).map_err(macos_keychain_error_after_retry)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn migrate_legacy_token(&self) -> Result<Option<String>, AuthTokenStoreError> {
        if !self.legacy.path().exists() {
            return Ok(None);
        }

        let cache = self.legacy.load_or_default()?;
        let refresh_token = cache.refresh_token.and_then(non_empty);
        let Some(refresh_token) = refresh_token else {
            fs::remove_file(self.legacy.path()).map_err(ConfigurationError::from)?;
            return Ok(None);
        };

        self.run_keyring_operation(|entry| entry.set_password(refresh_token.as_str()))?;
        if self.run_keyring_operation(keyring::Entry::get_password)? != refresh_token {
            return Err(AuthTokenStoreError::Verification);
        }
        fs::remove_file(self.legacy.path()).map_err(ConfigurationError::from)?;
        Ok(Some(refresh_token))
    }
}

#[cfg(target_os = "macos")]
fn macos_keychain_error_code(error: &keyring::Error) -> Option<i32> {
    match error {
        keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source) => source
            .downcast_ref::<MacOsSecurityError>()
            .map(|error| error.code()),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn macos_keychain_error_needs_unlock(error: &keyring::Error) -> bool {
    matches!(
        macos_keychain_error_code(error),
        Some(
            MACOS_ERR_SEC_AUTH_FAILED
                | MACOS_ERR_SEC_DUPLICATE_ITEM
                | MACOS_ERR_SEC_INTERACTION_NOT_ALLOWED
        )
    )
}

#[cfg(target_os = "macos")]
fn unlock_macos_login_keychain() -> Result<(), MacOsSecurityError> {
    let mut keychain = SecKeychain::default_for_domain(SecPreferencesDomain::User)?;
    keychain.unlock(None)
}

/// 把 macOS 钥匙串重试错误转换为可由界面明确提示的认证存储错误。
#[cfg(target_os = "macos")]
pub(crate) fn macos_keychain_error_after_retry(error: keyring::Error) -> AuthTokenStoreError {
    match macos_keychain_error_code(&error) {
        Some(MACOS_ERR_SEC_DUPLICATE_ITEM) => AuthTokenStoreError::MacKeychainAccessDenied(error),
        Some(MACOS_ERR_SEC_AUTH_FAILED | MACOS_ERR_SEC_INTERACTION_NOT_ALLOWED) => {
            AuthTokenStoreError::MacKeychainLocked(error)
        }
        _ => AuthTokenStoreError::Keyring(error),
    }
}

/// 认证状态的只读快照。
///
/// UI 渲染层通过它读取当前是否已配置、是否登录、展示名和错误信息，避免直接修改全局状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSnapshot {
    /// 是否已经提供 OIDC issuer 和 client id。
    pub configured: bool,
    /// 当前是否存在有效登录用户资料。
    pub authenticated: bool,
    /// 认证流程是否正在后台执行。
    pub busy: bool,
    /// 侧边栏账户区展示的名称。
    pub display_name: SharedString,
    /// 已登录用户的邮箱。
    pub email: Option<String>,
    /// 已登录用户的远程头像地址。
    pub avatar_url: Option<SharedString>,
    /// 当前状态说明或最近一次错误。
    pub status: SharedString,
}

/// Console 认证配置或登录流程中的错误。
#[derive(Debug, Error)]
pub enum AuthError {
    /// 未配置 OIDC issuer。
    #[error("未配置 OIDC_ISSUER_URL")]
    MissingIssuer,
    /// 未配置 OIDC client id。
    #[error("未配置 OIDC_CLIENT_ID")]
    MissingClientId,
    /// 未配置 OIDC redirect URI。
    #[error("未配置 OIDC_REDIRECT_URI")]
    MissingRedirectUri,
    /// 未配置业务 API 根地址。
    #[error("未配置 API_BASE_URL")]
    MissingApiBaseUrl,
    /// 业务 API 根地址不满足桌面登录安全约束。
    #[error("业务 API 地址无效: {0}")]
    InvalidApiBaseUrl(
        /// 不包含凭据的稳定校验说明。
        &'static str,
    ),
    /// OIDC 登录流程失败。
    #[error(transparent)]
    Oidc(
        /// 可复用 OIDC 客户端返回的协议或网络错误。
        #[from]
        OidcError,
    ),
    /// 桌面配置目录不可用或旧 token 文件迁移失败。
    #[error(transparent)]
    Configuration(
        /// 桌面运行配置加载或旧 token 文件访问错误。
        #[from]
        ConfigurationError,
    ),
    /// refresh token 无法从系统凭据库读取或写入。
    #[error(transparent)]
    TokenStore(
        /// 系统凭据库或旧 token 迁移返回的错误。
        #[from]
        AuthTokenStoreError,
    ),
    /// 已经有认证或恢复流程正在执行。
    #[error("认证流程正在进行，请稍候")]
    LoginInProgress,
    /// 业务 API 网络请求或 JSON 响应读取失败。
    #[error("业务服务请求失败: {0}")]
    ApiRequest(
        /// Reqwest 返回且不会包含 Bearer token 的请求错误。
        #[from]
        reqwest::Error,
    ),
    /// 业务 API 使用稳定错误契约拒绝当前 OIDC 会话。
    #[error("业务服务拒绝登录: {message}（code={code}, request_id={request_id}）")]
    ApiRejected {
        /// HTTP 状态码。
        status: u16,
        /// 服务端返回的稳定错误码。
        code: String,
        /// 服务端返回的用户可读错误说明。
        message: String,
        /// 服务端请求追踪 ID。
        request_id: String,
    },
}

impl AuthError {
    fn rejects_local_access(&self) -> bool {
        matches!(
            self,
            Self::ApiRejected {
                status: 401 | 403,
                ..
            }
        )
    }
}

#[derive(Debug, Default)]
struct AuthState {
    config: Option<AuthConfig>,
    store: Option<AuthTokenStore>,
    session: Option<OidcSession>,
    login_window: Option<AnyWindowHandle>,
    busy: bool,
    status: String,
    refresh_generation: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DesktopAuthConfig {
    oidc: OidcSettings,
    api: ApiSettings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OidcSettings {
    issuer_url: Option<String>,
    client_id: Option<String>,
    scopes: Option<String>,
    redirect_uri: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ApiSettings {
    base_url: Option<String>,
}

impl Global for AuthState {}

/// 初始化 Console 认证状态。
///
/// 应用启动时调用一次；它会注册 GPUI Global，并在后台使用系统凭据库中的 refresh token
/// 换取一组经过验证的新 token。恢复完成前保持未认证门禁并展示加载状态。
pub fn init(config: Option<AuthConfig>, store: Option<AuthTokenStore>, cx: &mut App) {
    let can_restore = config.is_some() && store.is_some();
    let status = if config.is_some() {
        if can_restore {
            "正在恢复登录..."
        } else {
            "未登录"
        }
    } else {
        "未配置 OIDC"
    };

    AuthState::set_global(
        cx,
        AuthState {
            config: config.clone(),
            store: store.clone(),
            session: None,
            login_window: None,
            busy: can_restore,
            status: status.to_owned(),
            refresh_generation: 0,
        },
    );

    if let (Some(config), Some(store)) = (config, store) {
        let restore_task = cx.background_spawn(async move { restore_session(config, store) });
        cx.spawn(async move |cx| {
            let result = restore_task.await;
            cx.update(|cx| complete_restore(result, cx));
        })
        // xuwe-lint: allow(xuwe::detached_lifecycle) reason="认证恢复任务属于应用级 Global 生命周期"
        .detach();
    }
}

/// 从桌面配置文件、进程环境变量和编译期环境变量读取 OIDC 配置。
///
/// 默认读取可选的 `config/desktop.toml`。配置优先级为编译期环境变量、配置文件、
/// 进程环境变量；后提供的来源覆盖先提供的来源。支持的进程环境变量包括：
/// `OIDC_ISSUER_URL`、`OIDC_CLIENT_ID`、`OIDC_SCOPES`、`OIDC_REDIRECT_URI` 和
/// `API_BASE_URL`。
///
/// # Errors
///
/// 配置文件格式无效或 OIDC 参数校验失败时返回 [`AuthError`]。
pub fn config_from_environment() -> Result<Option<AuthConfig>, AuthError> {
    config_from_sources(Path::new(DEFAULT_DESKTOP_CONFIG_PATH))
}

fn config_from_sources(path: &Path) -> Result<Option<AuthConfig>, AuthError> {
    let file_config = load_desktop_auth_config(path)?;
    let issuer_url = environment_value("OIDC_ISSUER_URL")
        .or(file_config.oidc.issuer_url)
        .or_else(|| compiled_value(option_env!("OIDC_ISSUER_URL")));
    let client_id = environment_value("OIDC_CLIENT_ID")
        .or(file_config.oidc.client_id)
        .or_else(|| compiled_value(option_env!("OIDC_CLIENT_ID")));
    let Some(issuer_url) = issuer_url else {
        return Ok(None);
    };
    let Some(client_id) = client_id else {
        return Err(AuthError::MissingClientId);
    };
    let redirect_uri = environment_value("OIDC_REDIRECT_URI")
        .or(file_config.oidc.redirect_uri)
        .or_else(|| compiled_value(option_env!("OIDC_REDIRECT_URI")))
        .ok_or(AuthError::MissingRedirectUri)?;
    let scopes = environment_value("OIDC_SCOPES")
        .or(file_config.oidc.scopes)
        .or_else(|| compiled_value(option_env!("OIDC_SCOPES")))
        .unwrap_or_else(|| DEFAULT_OIDC_SCOPES.to_owned());
    let api_base_url = environment_value("API_BASE_URL")
        .or(file_config.api.base_url)
        .or_else(|| compiled_value(option_env!("API_BASE_URL")))
        .ok_or(AuthError::MissingApiBaseUrl)?;

    let oidc = OidcConfig::new(
        issuer_url,
        client_id,
        scopes.split_whitespace().map(str::to_owned),
        redirect_uri,
    )
    .map_err(AuthError::from)?;
    AuthConfig::new(oidc, api_base_url.as_str()).map(Some)
}

fn load_desktop_auth_config(path: &Path) -> Result<DesktopAuthConfig, ConfigurationError> {
    LayeredConfigLoader::<DesktopAuthConfig>::new()
        .with_optional_file(path)
        .without_environment()
        .load()
}

/// 创建 Console 默认 token 存储。
///
/// # Errors
///
/// 当前平台无法确定旧 token 文件目录时返回 [`ConfigurationError`]。
pub fn token_store(config: &AuthConfig) -> Result<AuthTokenStore, ConfigurationError> {
    AuthTokenStore::new(config.oidc())
}

/// 返回当前认证状态快照。
pub fn snapshot(cx: &App) -> AuthSnapshot {
    let state = AuthState::global(cx);
    let profile = state.session.as_ref().map(OidcSession::profile);
    let display_name =
        profile
            .map(|profile| profile.display_name())
            .unwrap_or(if state.config.is_some() {
                "未登录"
            } else {
                "未配置认证"
            });

    AuthSnapshot {
        configured: state.config.is_some(),
        authenticated: state.session.is_some(),
        busy: state.busy,
        display_name: display_name.into(),
        email: profile.and_then(|profile| profile.email.clone()),
        avatar_url: profile
            .and_then(|profile| profile.picture.as_deref())
            .and_then(non_empty_str)
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            .map(SharedString::from),
        status: state.status.clone().into(),
    }
}

/// 返回当前已认证会话访问业务 API 所需的短期凭据快照。
///
/// 未登录、认证配置缺失或 access token 为空时返回 `None`。
pub(crate) fn api_session(cx: &App) -> Option<ApiSession> {
    let state = AuthState::global(cx);
    let config = state.config.as_ref()?;
    let access_token = state.session.as_ref()?.tokens().access_token.trim();
    if access_token.is_empty() {
        return None;
    }
    Some(config.api_session(access_token.to_owned()))
}

/// 返回当前已认证身份的稳定 OIDC subject。
///
/// 该值仅用于隔离桌面页面的会话级缓存；access token 自动续期不会改变它。
pub(crate) fn session_identity(cx: &App) -> Option<String> {
    AuthState::global(cx)
        .session
        .as_ref()
        .map(|session| session.profile().subject.clone())
}

/// 在窗口上下文中观察认证状态变化。
///
/// 调用方仍应比较 [`session_identity`]，因为登录过程中的状态文案与忙碌状态也会通知观察者。
pub(crate) fn observe_session_in<T>(
    window: &Window,
    cx: &mut Context<T>,
    observer: impl FnMut(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> Subscription
where
    T: 'static,
{
    cx.observe_global_in::<AuthState>(window, observer)
}

/// 开始一次浏览器登录流程。
///
/// OIDC Discovery 和本地 listener 创建会在后台执行，避免网络请求阻塞 GPUI 事件线程；
/// 授权 URL 准备完成后会回到前台线程打开系统浏览器，然后继续在后台等待回调、交换 token
/// 并加载用户资料。
///
/// # Errors
///
/// 尚未配置 OIDC issuer 时返回 [`AuthError::MissingIssuer`]。
pub fn start_login(cx: &mut App) -> Result<(), AuthError> {
    tracing::info!("收到 Console OIDC 登录请求");
    if AuthState::global(cx).busy {
        return Err(AuthError::LoginInProgress);
    }
    let config = AuthState::global(cx)
        .config
        .clone()
        .ok_or(AuthError::MissingIssuer)?;
    let login_window = cx.active_window();

    AuthState::update_global(cx, |auth, cx| {
        auth.busy = true;
        auth.login_window = login_window;
        auth.status = "正在连接认证服务...".to_owned();
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证门禁与主工作区切换影响整个窗口"
        cx.refresh_windows();
    });

    let begin_task = cx.background_spawn(async move {
        let client = OidcClient::new(config.oidc.clone())?;
        client.begin_login().map_err(AuthError::from)
    });
    cx.spawn(async move |cx| {
        let result = begin_task.await;
        cx.update(|cx| match result {
            Ok(pending) => {
                let authorization_url = pending.authorization_url().to_owned();
                AuthState::update_global(cx, |auth, cx| {
                    auth.status = "已打开浏览器，正在等待登录...".to_owned();
                    // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证状态提示需要刷新应用级门禁"
                    cx.refresh_windows();
                });
                tracing::info!("Console OIDC Discovery 完成，正在打开系统浏览器");
                cx.open_url(authorization_url.as_str());
                wait_for_login(pending, cx);
            }
            Err(error) => {
                tracing::error!(error = %error, "Console OIDC 创建登录请求失败");
                complete_login(Err(error), cx);
            }
        });
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="浏览器登录流程属于应用级认证 Global 生命周期"
    .detach();

    Ok(())
}

fn wait_for_login(pending: oidc::PendingOidcLogin, cx: &mut App) {
    let login_task = cx.background_spawn(async move { pending.finish().map_err(AuthError::from) });
    cx.spawn(async move |cx| {
        let result = login_task.await;
        cx.update(|cx| {
            if result.is_ok() {
                cx.activate(true);
            }
            complete_login(result, cx);
        });
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="loopback 回调等待属于应用级认证 Global 生命周期"
    .detach();
}

fn validate_login_session(session: OidcSession, cx: &mut App) {
    let Some(config) = AuthState::global(cx).config.clone() else {
        complete_login(Err(AuthError::MissingIssuer), cx);
        return;
    };
    let store = AuthState::global(cx).store.clone();
    AuthState::update_global(cx, |auth, cx| {
        auth.status = "正在验证系统访问权限...".to_owned();
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证状态提示需要刷新应用级门禁"
        cx.refresh_windows();
    });
    let validation_task = cx.background_spawn(async move {
        match validate_session_access(&config, &session) {
            Ok(profile) => {
                let warning = persist_session_tokens(store.as_ref(), session.tokens());
                Ok((session, warning, profile))
            }
            Err(error) => {
                if error.rejects_local_access()
                    && let Some(store) = &store
                    && let Err(clear_error) = store.clear()
                {
                    tracing::warn!(
                        error = ?clear_error,
                        "业务服务拒绝登录后无法清理 Console refresh token"
                    );
                }
                Err(error)
            }
        }
    });
    cx.spawn(async move |cx| {
        let result = validation_task.await;
        cx.update(|cx| match result {
            Ok((session, warning, profile)) => {
                tracing::info!(
                    business_operation = "desktop_login",
                    local_user_id = %profile.user.id,
                    is_super_admin = profile.user.is_super_admin,
                    outcome = "authorized",
                    "Console 已通过业务服务登录门禁"
                );
                apply_session(session, warning, cx);
            }
            Err(error) => complete_login(Err(error), cx),
        });
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="业务登录门禁与凭据写入属于应用级认证 Global 生命周期"
    .detach();
}

/// 把后台登录结果写回认证全局状态。
pub fn complete_login(result: Result<OidcSession, AuthError>, cx: &mut App) {
    match result {
        Ok(session) => validate_login_session(session, cx),
        Err(error) => {
            tracing::warn!(error = %error, "Console 登录失败");
            let error_message = error.to_string();
            let notification_displayed = push_login_failure_notification(&error, cx);
            AuthState::update_global(cx, |auth, cx| {
                auth.busy = false;
                auth.session = None;
                auth.login_window = None;
                auth.status = if notification_displayed {
                    if auth.config.is_some() {
                        "未登录".to_owned()
                    } else {
                        "未配置 OIDC".to_owned()
                    }
                } else {
                    error_message
                };
                // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证失败状态需要刷新整个登录门禁和通知层"
                cx.refresh_windows();
            });
        }
    }
}

/// 将登录失败通知推送到发起登录的窗口；窗口不可用时返回 `false`。
pub(crate) fn push_login_failure_notification(error: &AuthError, cx: &mut App) -> bool {
    let login_window = AuthState::global(cx).login_window;
    let Some(window_handle) = login_window.or_else(|| cx.active_window()) else {
        return false;
    };
    let notification = login_failure_notification(error);

    if let Err(notification_error) = window_handle.update(cx, |_, window, cx| {
        window.push_notification(notification, cx);
    }) {
        tracing::warn!(
            error = %notification_error,
            "Console 登录失败通知无法推送到发起登录的窗口"
        );
        return false;
    }
    true
}

/// 构造带可选请求 ID 复制操作的登录失败通知。
pub(crate) fn login_failure_notification(error: &AuthError) -> Notification {
    let mut notification = Notification::error(login_failure_notification_message(error))
        .id::<LoginFailureNotification>()
        .title("登录失败");
    if let Some(request_id) = login_failure_request_id(error).map(str::to_owned) {
        notification = notification.action(move |_, _, cx| {
            let request_id = request_id.clone();
            Button::new("copy-login-request-id")
                .icon(IconName::Copy)
                .label("复制请求 ID")
                .debug_selector(|| "copy-login-request-id".to_owned())
                .on_click(cx.listener(move |notification, _, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(request_id.clone()));
                    notification.dismiss(window, cx);
                }))
        });
    }
    notification
}

fn login_failure_notification_message(error: &AuthError) -> String {
    match error {
        AuthError::ApiRejected { message, .. }
            if let Some(request_id) = login_failure_request_id(error) =>
        {
            format!("{message}\n请求 ID：{request_id}")
        }
        AuthError::ApiRejected { message, .. } => message.clone(),
        _ => error.to_string(),
    }
}

fn login_failure_request_id(error: &AuthError) -> Option<&str> {
    let AuthError::ApiRejected { request_id, .. } = error else {
        return None;
    };
    let request_id = request_id.trim();
    (!request_id.is_empty() && request_id != "unknown").then_some(request_id)
}

/// 清除当前登录态并在后台删除系统凭据库中的 refresh token。
pub fn sign_out(cx: &mut App) {
    let store = AuthState::global(cx).store.clone();
    AuthState::update_global(cx, |auth, cx| {
        auth.session = None;
        auth.login_window = None;
        auth.busy = false;
        auth.refresh_generation = auth.refresh_generation.wrapping_add(1);
        auth.status = if auth.config.is_some() {
            "已退出登录".to_owned()
        } else {
            "未配置 OIDC".to_owned()
        };
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="退出登录需要把完整工作区切换为认证门禁"
        cx.refresh_windows();
    });

    if let Some(store) = store {
        let clear_task = cx.background_spawn(async move { store.clear() });
        cx.spawn(async move |cx| {
            if let Err(error) = clear_task.await {
                cx.update(|cx| {
                    AuthState::update_global(cx, |auth, cx| {
                        auth.status = format!("已退出，但清除系统登录凭据失败: {error}");
                        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="系统凭据清理错误需要刷新登录门禁提示"
                        cx.refresh_windows();
                    });
                });
            }
        })
        // xuwe-lint: allow(xuwe::detached_lifecycle) reason="系统凭据清理属于应用级认证 Global 生命周期"
        .detach();
    }
}

fn restore_session(
    config: AuthConfig,
    store: AuthTokenStore,
) -> Result<Option<OidcSession>, AuthError> {
    let Some(refresh_token) = store.load_refresh_token()? else {
        return Ok(None);
    };
    let client = OidcClient::new(config.oidc.clone())?;
    let tokens = OidcTokenCache {
        refresh_token: Some(refresh_token),
        ..OidcTokenCache::default()
    };
    match client.refresh(&tokens) {
        Ok(session) => {
            if let Err(error) = validate_session_access(&config, &session) {
                if error.rejects_local_access() {
                    store.clear()?;
                }
                return Err(error);
            }
            store.save_tokens(session.tokens())?;
            Ok(Some(session))
        }
        Err(error) => {
            if error.is_refresh_token_rejected() {
                store.clear()?;
            }
            Err(error.into())
        }
    }
}

fn complete_restore(result: Result<Option<OidcSession>, AuthError>, cx: &mut App) {
    match result {
        Ok(Some(session)) => apply_session(session, None, cx),
        Ok(None) => AuthState::update_global(cx, |auth, cx| {
            auth.busy = false;
            auth.status = "未登录".to_owned();
            // xuwe-lint: allow(xuwe::global_refresh_scope) reason="登录恢复结束需要刷新整个认证门禁"
            cx.refresh_windows();
        }),
        Err(error) => AuthState::update_global(cx, |auth, cx| {
            auth.busy = false;
            auth.status = format!("登录态恢复失败: {error}");
            // xuwe-lint: allow(xuwe::global_refresh_scope) reason="登录恢复错误需要刷新整个认证门禁"
            cx.refresh_windows();
        }),
    }
}

/// 应用已经通过业务服务门禁的 OIDC 会话，并安排后续自动续期。
pub(crate) fn apply_session(
    session: OidcSession,
    storage_warning: Option<AuthTokenStoreError>,
    cx: &mut App,
) {
    let expires_at = session.tokens().expires_at;
    AuthState::update_global(cx, |auth, cx| {
        auth.busy = false;
        auth.login_window = None;
        auth.status = storage_warning
            .map(|warning| format!("已登录 · {}", warning.user_message()))
            .unwrap_or_else(|| "已登录".to_owned());
        auth.session = Some(session);
        auth.refresh_generation = auth.refresh_generation.wrapping_add(1);
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证成功需要把登录门禁切换为完整工作区"
        cx.refresh_windows();
    });
    let generation = AuthState::global(cx).refresh_generation;
    schedule_session_refresh(expires_at, generation, cx);
}

fn persist_session_tokens(
    store: Option<&AuthTokenStore>,
    tokens: &OidcTokenCache,
) -> Option<AuthTokenStoreError> {
    let warning = store.and_then(|store| store.save_tokens(tokens).err());
    if let Some(error) = &warning {
        tracing::warn!(error = ?error, "Console OIDC 无法保存 refresh token");
    }
    warning
}

fn schedule_session_refresh(expires_at: Option<u64>, generation: u64, cx: &mut App) {
    let Some(expires_at) = expires_at else {
        return;
    };
    let refresh_at = expires_at.saturating_sub(120);
    let delay = Duration::from_secs(refresh_at.saturating_sub(now_unix_seconds()).max(5));
    let timer = cx.background_executor().timer(delay);
    cx.spawn(async move |cx| {
        timer.await;
        cx.update(|cx| start_scheduled_refresh(generation, cx));
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="token 续期定时器属于应用级认证 Global 生命周期"
    .detach();
}

fn start_scheduled_refresh(generation: u64, cx: &mut App) {
    let state = AuthState::global(cx);
    if state.refresh_generation != generation || state.busy {
        return;
    }
    let (Some(config), Some(session)) = (state.config.clone(), state.session.clone()) else {
        return;
    };
    let tokens = session.tokens().clone();
    let store = state.store.clone();
    let refresh_task = cx.background_spawn(async move {
        let client = OidcClient::new(config.oidc.clone())?;
        match client.refresh(&tokens) {
            Ok(session) => {
                if let Err(error) = validate_session_access(&config, &session) {
                    if error.rejects_local_access()
                        && let Some(store) = &store
                        && let Err(clear_error) = store.clear()
                    {
                        tracing::warn!(
                            error = ?clear_error,
                            "会话被业务服务拒绝后无法清理 Console refresh token"
                        );
                    }
                    return Err(error);
                }
                let warning = persist_session_tokens(store.as_ref(), session.tokens());
                Ok((session, warning))
            }
            Err(error) => {
                if error.is_refresh_token_rejected()
                    && let Some(store) = &store
                {
                    store.clear()?;
                }
                Err(error.into())
            }
        }
    });
    cx.spawn(async move |cx| {
        let result = refresh_task.await;
        cx.update(|cx| complete_scheduled_refresh(generation, result, cx));
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="token 续期请求属于应用级认证 Global 生命周期"
    .detach();
}

fn complete_scheduled_refresh(
    generation: u64,
    result: Result<(OidcSession, Option<AuthTokenStoreError>), AuthError>,
    cx: &mut App,
) {
    if AuthState::global(cx).refresh_generation != generation {
        return;
    }
    match result {
        Ok((session, warning)) => apply_session(session, warning, cx),
        Err(error) if refresh_token_rejected(&error) || error.rejects_local_access() => {
            AuthState::update_global(cx, |auth, cx| {
                auth.session = None;
                auth.status = error.to_string();
                auth.refresh_generation = auth.refresh_generation.wrapping_add(1);
                // xuwe-lint: allow(xuwe::global_refresh_scope) reason="会话失效需要把完整工作区切换为登录门禁"
                cx.refresh_windows();
            });
        }
        Err(error) => {
            AuthState::update_global(cx, |auth, cx| {
                auth.status = format!("会话自动续期失败，将稍后重试: {error}");
                // xuwe-lint: allow(xuwe::global_refresh_scope) reason="自动续期错误需要刷新应用级认证提示"
                cx.refresh_windows();
            });
            let timer = cx.background_executor().timer(Duration::from_secs(60));
            cx.spawn(async move |cx| {
                timer.await;
                cx.update(|cx| start_scheduled_refresh(generation, cx));
            })
            // xuwe-lint: allow(xuwe::detached_lifecycle) reason="token 续期重试定时器属于应用级认证 Global 生命周期"
            .detach();
        }
    }
}

fn refresh_token_rejected(error: &AuthError) -> bool {
    matches!(error, AuthError::Oidc(error) if error.is_refresh_token_rejected())
}

/// 使用当前 access token 请求业务服务 `/me`，确认本地账号存在且允许登录。
///
/// 成功响应会按共享 [`AccessProfileResponse`] 契约解析；非成功响应只读取共享错误契约，
/// 不记录或返回 Bearer token。
///
/// # Errors
///
/// 网络失败、响应不符合契约，或业务服务以非成功状态拒绝当前用户时返回 [`AuthError`]。
pub(crate) fn validate_session_access(
    config: &AuthConfig,
    session: &OidcSession,
) -> Result<AccessProfileResponse, AuthError> {
    let response = Client::builder()
        .timeout(API_REQUEST_TIMEOUT)
        .build()?
        .get(
            config
                .api_base_url
                .join("me")
                .expect("API 根地址已经过校验"),
        )
        .bearer_auth(session.tokens().access_token.as_str())
        .send()?;
    let status = response.status();
    if status.is_success() {
        return Ok(response.json()?);
    }

    let response_request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| "unknown".to_owned());
    let error = response.json::<ErrorEnvelope>().ok();
    Err(AuthError::ApiRejected {
        status: status.as_u16(),
        code: error
            .as_ref()
            .map(|error| error.error.code.clone())
            .unwrap_or_else(|| format!("http_{}", status.as_u16())),
        message: error
            .as_ref()
            .map(|error| error.error.message.clone())
            .unwrap_or_else(|| "业务服务返回了无法识别的错误响应".to_owned()),
        request_id: error
            .map(|error| error.error.request_id)
            .filter(|request_id| !request_id.trim().is_empty())
            .unwrap_or(response_request_id),
    })
}

fn validated_api_base_url(api_base_url: &str) -> Result<Url, AuthError> {
    let url = Url::parse(api_base_url.trim())
        .map_err(|_| AuthError::InvalidApiBaseUrl("API_BASE_URL 不是有效绝对 URL"))?;
    if url.host().is_none() {
        return Err(AuthError::InvalidApiBaseUrl("API_BASE_URL 必须包含主机"));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthError::InvalidApiBaseUrl(
            "API_BASE_URL 不能包含凭据、query 或 fragment",
        ));
    }
    if url.path() != "/" {
        return Err(AuthError::InvalidApiBaseUrl(
            "API_BASE_URL 必须指向服务根路径",
        ));
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)) {
        return Err(AuthError::InvalidApiBaseUrl(
            "API_BASE_URL 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP",
        ));
    }
    Ok(url)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn non_empty_str(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

fn environment_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn compiled_value(value: Option<&'static str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}
