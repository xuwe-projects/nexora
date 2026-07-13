//! Console 桌面应用认证状态。
//!
//! 该模块只负责把可复用 `oidc` crate 接入 GPUI：读取 Console 的 OIDC 环境变量、
//! 使用系统凭据库持久化 refresh token、维护应用级登录状态，并为根视图提供展示快照。

use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use configuration::{ConfigurationError, LayeredConfigLoader, UserConfigStore};
use gpui::{App, AppContext as _, Global, ReadGlobal as _, SharedString, UpdateGlobal as _};
use oidc::{OidcClient, OidcConfig, OidcError, OidcSession, OidcTokenCache};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const DEFAULT_DESKTOP_CONFIG_PATH: &str = "config/desktop.toml";
const DEFAULT_OIDC_SCOPES: &str = "openid profile email offline_access";

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
        match self.entry()?.get_password() {
            Ok(token) => return Ok(non_empty(token)),
            Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(error.into()),
        }

        self.migrate_legacy_token()
    }

    fn save_tokens(&self, tokens: &OidcTokenCache) -> Result<(), AuthTokenStoreError> {
        let Some(refresh_token) = tokens.refresh_token.as_deref().and_then(non_empty_str) else {
            return self.clear();
        };
        self.entry()?.set_password(refresh_token)?;
        Ok(())
    }

    fn clear(&self) -> Result<(), AuthTokenStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
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

        let entry = self.entry()?;
        entry.set_password(refresh_token.as_str())?;
        if entry.get_password()? != refresh_token {
            return Err(AuthTokenStoreError::Verification);
        }
        fs::remove_file(self.legacy.path()).map_err(ConfigurationError::from)?;
        Ok(Some(refresh_token))
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
}

#[derive(Debug, Default)]
struct AuthState {
    config: Option<OidcConfig>,
    store: Option<AuthTokenStore>,
    session: Option<OidcSession>,
    busy: bool,
    status: String,
    refresh_generation: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DesktopAuthConfig {
    oidc: OidcSettings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OidcSettings {
    issuer_url: Option<String>,
    client_id: Option<String>,
    scopes: Option<String>,
    redirect_uri: Option<String>,
}

impl Global for AuthState {}

/// 初始化 Console 认证状态。
///
/// 应用启动时调用一次；它会注册 GPUI Global，并在后台使用系统凭据库中的 refresh token
/// 换取一组经过验证的新 token。恢复完成前保持未认证门禁并展示加载状态。
pub fn init(config: Option<OidcConfig>, store: Option<AuthTokenStore>, cx: &mut App) {
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
/// `OIDC_ISSUER_URL`、`OIDC_CLIENT_ID`、`OIDC_SCOPES`、`OIDC_REDIRECT_URI`。
///
/// # Errors
///
/// 配置文件格式无效或 OIDC 参数校验失败时返回 [`AuthError`]。
pub fn config_from_environment() -> Result<Option<OidcConfig>, AuthError> {
    config_from_sources(Path::new(DEFAULT_DESKTOP_CONFIG_PATH))
}

fn config_from_sources(path: &Path) -> Result<Option<OidcConfig>, AuthError> {
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

    OidcConfig::new(
        issuer_url,
        client_id,
        scopes.split_whitespace().map(str::to_owned),
        redirect_uri,
    )
    .map(Some)
    .map_err(AuthError::from)
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
pub fn token_store(config: &OidcConfig) -> Result<AuthTokenStore, ConfigurationError> {
    AuthTokenStore::new(config)
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
        status: state.status.clone().into(),
    }
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
    eprintln!("Console OIDC: 收到登录请求");
    if AuthState::global(cx).busy {
        return Err(AuthError::LoginInProgress);
    }
    let config = AuthState::global(cx)
        .config
        .clone()
        .ok_or(AuthError::MissingIssuer)?;

    AuthState::update_global(cx, |auth, cx| {
        auth.busy = true;
        auth.status = "正在连接认证服务...".to_owned();
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证门禁与主工作区切换影响整个窗口"
        cx.refresh_windows();
    });

    let begin_task = cx.background_spawn(async move {
        let client = OidcClient::new(config)?;
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
                eprintln!("Console OIDC: Discovery 完成，正在打开系统浏览器");
                cx.open_url(authorization_url.as_str());
                wait_for_login(pending, cx);
            }
            Err(error) => {
                eprintln!("Console OIDC: 创建登录请求失败: {error}");
                complete_login(Err(error), cx);
            }
        });
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="浏览器登录流程属于应用级认证 Global 生命周期"
    .detach();

    Ok(())
}

fn wait_for_login(pending: oidc::PendingOidcLogin, cx: &mut App) {
    let store = AuthState::global(cx).store.clone();
    let login_task = cx.background_spawn(async move {
        let session = pending.finish().map_err(AuthError::from)?;
        let storage_warning = store
            .as_ref()
            .and_then(|store| store.save_tokens(session.tokens()).err())
            .map(|error| error.to_string());
        Ok::<_, AuthError>((session, storage_warning))
    });
    cx.spawn(async move |cx| {
        let result = login_task.await;
        cx.update(|cx| match result {
            Ok((session, warning)) => apply_session(session, warning, cx),
            Err(error) => complete_login(Err(error), cx),
        });
    })
    // xuwe-lint: allow(xuwe::detached_lifecycle) reason="loopback 回调等待属于应用级认证 Global 生命周期"
    .detach();
}

/// 把后台登录结果写回认证全局状态。
pub fn complete_login(result: Result<OidcSession, AuthError>, cx: &mut App) {
    match result {
        Ok(session) => apply_session(session, None, cx),
        Err(error) => AuthState::update_global(cx, |auth, cx| {
            auth.busy = false;
            auth.status = error.to_string();
            // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证失败状态需要刷新整个登录门禁"
            cx.refresh_windows();
        }),
    }
}

/// 清除当前登录态并在后台删除系统凭据库中的 refresh token。
pub fn sign_out(cx: &mut App) {
    let store = AuthState::global(cx).store.clone();
    AuthState::update_global(cx, |auth, cx| {
        auth.session = None;
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
    config: OidcConfig,
    store: AuthTokenStore,
) -> Result<Option<OidcSession>, AuthError> {
    let Some(refresh_token) = store.load_refresh_token()? else {
        return Ok(None);
    };
    let client = OidcClient::new(config)?;
    let tokens = OidcTokenCache {
        refresh_token: Some(refresh_token),
        ..OidcTokenCache::default()
    };
    match client.refresh(&tokens) {
        Ok(session) => {
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

fn apply_session(session: OidcSession, storage_warning: Option<String>, cx: &mut App) {
    let expires_at = session.tokens().expires_at;
    AuthState::update_global(cx, |auth, cx| {
        auth.busy = false;
        auth.status = storage_warning
            .map(|warning| format!("已登录，但无法保持登录: {warning}"))
            .unwrap_or_else(|| "已登录".to_owned());
        auth.session = Some(session);
        auth.refresh_generation = auth.refresh_generation.wrapping_add(1);
        // xuwe-lint: allow(xuwe::global_refresh_scope) reason="认证成功需要把登录门禁切换为完整工作区"
        cx.refresh_windows();
    });
    let generation = AuthState::global(cx).refresh_generation;
    schedule_session_refresh(expires_at, generation, cx);
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
        let client = OidcClient::new(config)?;
        match client.refresh(&tokens) {
            Ok(session) => {
                let warning = store
                    .as_ref()
                    .and_then(|store| store.save_tokens(session.tokens()).err())
                    .map(|error| error.to_string());
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
    result: Result<(OidcSession, Option<String>), AuthError>,
    cx: &mut App,
) {
    if AuthState::global(cx).refresh_generation != generation {
        return;
    }
    match result {
        Ok((session, warning)) => apply_session(session, warning, cx),
        Err(error) if refresh_token_rejected(&error) => {
            AuthState::update_global(cx, |auth, cx| {
                auth.session = None;
                auth.status = "登录已过期，请重新登录".to_owned();
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
