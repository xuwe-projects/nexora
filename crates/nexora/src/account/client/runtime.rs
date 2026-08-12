//! Account 桌面登录流程与 GPUI 应用状态协调。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Global, SharedString,
    Subscription, Window,
};
use gpui_component::{
    IconName, Sizable as _, WindowExt as _, button::Button, notification::Notification,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use super::{
    AccountAuthenticationError, AccountAuthenticator, AccountClientError, AccountLogin,
    AccountSession, PendingAccountLogin,
};
use contracts::account::AccessProfileResponse;
use oidc::{OidcSession, OidcTokenCache, OidcUserProfile};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use keyring::Entry;

/// Account 登录门禁可以安全读取的状态快照。
///
/// 快照不包含 access token、refresh token 或完整 OIDC 响应，可以直接交给桌面 UI
/// 决定按钮状态和提示文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLoginSnapshot {
    /// 是否已经向框架安装可用的 Account 认证协调器。
    pub configured: bool,
    /// 当前是否持有已经通过 OIDC 与 Account `/me` 门禁的会话。
    pub authenticated: bool,
    /// 是否正在创建登录请求、等待浏览器回调或校验业务账号。
    pub busy: bool,
    /// 是否正在从安全存储静默恢复会话。
    pub restoring: bool,
    /// 当前平台是否支持安全凭据持久化；Linux 始终为 `false`。
    pub secure_storage_supported: bool,
    /// 最近一次安全存储操作是否成功可用。
    pub secure_storage_available: bool,
    /// 当前偏好是否允许下次启动尝试恢复。
    pub remember_login: bool,
    /// 是否已经提交了“安全凭据可恢复”标记。
    pub recovery_allowed: bool,
    /// 当前恢复失败是否可以由用户手动重试。
    pub can_retry_recovery: bool,
    /// 适合直接显示在登录门禁中的当前状态或最近一次错误。
    pub status: SharedString,
    /// 最近一次登录失败的结构化信息；成功或开始下一次登录后为 `None`。
    pub failure: Option<AccountLoginFailure>,
}

/// 当前 Account 会话可以安全交给宿主业务状态使用的作用域标识。
///
/// 该快照不包含 OIDC token 或 Provider 资料。宿主可以在发起异步业务请求时保存快照，
/// 并在响应写回前与最新值比较；退出、重新登录或替换认证器后，旧响应将因 revision 不同
/// 而被丢弃。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountAuthenticationScope {
    /// 当前进程内认证作用域的递增版本；该值只用于内存态并发隔离，不能持久化或跨进程比较。
    pub revision: u64,
    /// 当前登录用户在 `account.users(id)` 中的本地 ID；未登录时为 `None`。
    pub user_id: Option<String>,
}

/// 仅通过受保护进程 IPC 传输的 Account 会话快照。
///
/// 登录变体包含短期 token，因此该类型刻意不实现 `Debug`，也不得写入磁盘或日志。
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum AccountProcessState {
    /// 当前窗口组已经退出登录。
    SignedOut,
    /// 当前窗口组共享的已认证会话。
    SignedIn {
        /// OIDC 短期 token 与 Provider 用户资料。
        tokens: Box<OidcTokenCache>,
        /// Account 服务确认的用户、角色和权限快照。
        profile: Box<AccessProfileResponse>,
    },
}

/// 可以安全交给桌面 UI 的 Account 登录失败信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLoginFailure {
    /// 不包含 token、内部错误链或数据库信息的用户可读说明。
    pub message: SharedString,
    /// 服务端返回的可选请求 ID，可用于日志检索和一键复制。
    pub request_id: Option<SharedString>,
}

struct AccountLoginState {
    authenticator: AccountAuthenticator,
    credential_service: Option<String>,
    login: Option<AccountLogin>,
    busy: bool,
    status: SharedString,
    failure: Option<AccountLoginFailure>,
    tokens: Option<OidcTokenCache>,
    restoring: bool,
    remember_login: bool,
    recovery_allowed: bool,
    secure_storage_supported: bool,
    secure_storage_available: bool,
    can_retry_recovery: bool,
    refresh_scheduled: bool,
    refresh_in_flight: bool,
    generation: u64,
    authentication_revision: u64,
    cancellation: Option<Arc<AtomicBool>>,
    login_window: Option<AnyWindowHandle>,
}

impl Global for AccountLoginState {}

/// 启动 Account 登录流程时可以同步发现的错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AccountLoginRuntimeError {
    /// 应用尚未在 [`install_authenticator`] 中安装认证协调器。
    #[error("Account 登录尚未初始化，请先安装认证协调器")]
    NotInstalled,
    /// 已经有一次浏览器认证流程正在执行。
    #[error("Account 登录正在进行，请稍候")]
    LoginInProgress,
}

/// 把已经校验配置的 Account 认证协调器安装到 GPUI 应用。
///
/// 应用通常在 [`crate::Application::initialize`] 中调用一次。再次调用会安全替换旧状态
/// 并清除已有会话，适合开发期间重新加载配置。
pub fn install_authenticator(authenticator: AccountAuthenticator, cx: &mut App) {
    let (generation, authentication_revision) =
        if let Some(state) = cx.try_global::<AccountLoginState>() {
            if let Some(cancellation) = state.cancellation.as_ref() {
                cancellation.store(true, Ordering::Release);
            }
            (
                state.generation.wrapping_add(1),
                state.authentication_revision.wrapping_add(1),
            )
        } else {
            (0, 0)
        };
    let preferences = crate::application::shell_preferences_snapshot(cx);
    let credential_service = credential_service(cx);
    let secure_storage_supported = secure_storage_supported() && credential_service.is_some();
    let state = AccountLoginState {
        authenticator,
        credential_service,
        login: None,
        tokens: None,
        busy: false,
        restoring: false,
        status: "未登录".into(),
        failure: None,
        remember_login: secure_storage_supported && preferences.account.remember_login,
        recovery_allowed: secure_storage_supported && preferences.account.recovery_allowed,
        secure_storage_supported,
        secure_storage_available: secure_storage_supported,
        can_retry_recovery: false,
        refresh_scheduled: false,
        refresh_in_flight: false,
        generation,
        authentication_revision,
        cancellation: None,
        login_window: None,
    };
    if cx.has_global::<AccountLoginState>() {
        *cx.global_mut::<AccountLoginState>() = state;
    } else {
        cx.set_global(state);
    }
    refresh_login_windows(cx);
    start_recovery(cx);
}

/// 返回当前 Account 登录状态的无敏感信息快照。
pub fn login_snapshot(cx: &App) -> AccountLoginSnapshot {
    if !cx.has_global::<AccountLoginState>() {
        return AccountLoginSnapshot {
            configured: false,
            authenticated: false,
            busy: false,
            restoring: false,
            secure_storage_supported: secure_storage_supported(),
            secure_storage_available: false,
            remember_login: secure_storage_supported(),
            recovery_allowed: false,
            can_retry_recovery: false,
            status: "未配置 Account 登录".into(),
            failure: None,
        };
    }

    let state = cx.global::<AccountLoginState>();
    AccountLoginSnapshot {
        configured: true,
        authenticated: state.login.is_some(),
        busy: state.busy,
        restoring: state.restoring,
        secure_storage_supported: state.secure_storage_supported,
        secure_storage_available: state.secure_storage_available,
        remember_login: state.remember_login,
        recovery_allowed: state.recovery_allowed,
        can_retry_recovery: state.can_retry_recovery,
        status: state.status.clone(),
        failure: state.failure.clone(),
    }
}

/// 返回当前业务请求所属的 Account 认证作用域。
///
/// 未安装认证器或尚未登录时，`user_id` 为 `None`。revision 只在当前进程内用于识别
/// 退出、重新登录和认证器替换，不代表数据库版本或 token 内容。
pub fn authentication_scope(cx: &App) -> AccountAuthenticationScope {
    let Some(state) = cx.try_global::<AccountLoginState>() else {
        return AccountAuthenticationScope::default();
    };
    AccountAuthenticationScope {
        revision: state.authentication_revision,
        user_id: state
            .login
            .as_ref()
            .map(AccountLogin::profile)
            .map(|profile| profile.user.id.clone()),
    }
}

/// 生成当前进程可交给受保护 IPC 的账号会话快照。
pub(crate) fn process_state(cx: &App) -> AccountProcessState {
    let Some(state) = cx.try_global::<AccountLoginState>() else {
        return AccountProcessState::SignedOut;
    };
    let Some(login) = state.login.as_ref() else {
        return AccountProcessState::SignedOut;
    };
    AccountProcessState::SignedIn {
        tokens: Box::new(login.session().tokens().clone()),
        profile: Box::new(login.profile().clone()),
    }
}

/// 应用来自受保护 IPC 的账号状态，并刷新当前进程全部登录门禁。
///
/// 主进程作为 `authoritative` 接收子进程登录时负责安全存储和后续 token 刷新；普通
/// 窗口子进程只消费主进程广播，避免多个进程并发轮换 refresh token。
pub(crate) fn apply_process_state(
    process_state: AccountProcessState,
    authoritative: bool,
    cx: &mut App,
) {
    match process_state {
        AccountProcessState::SignedOut => {
            let snapshot = login_snapshot(cx);
            if snapshot.authenticated
                || snapshot.busy
                || snapshot.restoring
                || snapshot.recovery_allowed
            {
                sign_out(cx);
            }
        }
        AccountProcessState::SignedIn { tokens, profile } => {
            let tokens = *tokens;
            let Some(session) = OidcSession::from_token_cache(tokens.clone()) else {
                return;
            };
            if !cx.has_global::<AccountLoginState>() {
                return;
            }
            let state = cx.global_mut::<AccountLoginState>();
            if let Some(cancellation) = state.cancellation.take() {
                cancellation.store(true, Ordering::Release);
            }
            state.generation = state.generation.wrapping_add(1);
            let generation = state.generation;
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.login = Some(AccountLogin {
                session,
                profile: *profile,
            });
            state.tokens = Some(tokens.clone());
            state.busy = false;
            state.restoring = false;
            state.refresh_scheduled = false;
            state.refresh_in_flight = false;
            state.can_retry_recovery = false;
            state.status = "登录状态已从主进程同步".into();
            state.failure = None;
            if authoritative {
                persist_session(tokens, generation, true, cx);
            }
            refresh_login_windows(cx);
        }
    }
}

/// 观察当前进程账号会话变化，供应用级进程协调器发布内存快照。
pub(crate) fn observe_process_state(
    cx: &mut App,
    mut observer: impl FnMut(AccountProcessState, &mut App) + 'static,
) -> Subscription {
    cx.observe_global::<AccountLoginState>(move |cx| observer(process_state(cx), cx))
}

/// 返回当前应用是否已经通过 Account 登录门禁。
pub fn is_authenticated(cx: &App) -> bool {
    cx.has_global::<AccountLoginState>() && cx.global::<AccountLoginState>().login.is_some()
}

/// 返回当前登录用户的业务账号、角色和权限快照。
///
/// 未安装认证协调器或尚未登录时返回 `None`。返回引用不包含 OIDC token。
pub fn login_profile(cx: &App) -> Option<&AccessProfileResponse> {
    cx.has_global::<AccountLoginState>()
        .then(|| cx.global::<AccountLoginState>().login.as_ref())
        .flatten()
        .map(AccountLogin::profile)
}

/// 返回当前已经通过业务门禁的 OIDC 会话。
///
/// 该接口会暴露短期 token，只应在受控的业务请求边界中读取，不要写入日志或普通配置。
pub fn login_session(cx: &App) -> Option<&OidcSession> {
    cx.has_global::<AccountLoginState>()
        .then(|| cx.global::<AccountLoginState>().login.as_ref())
        .flatten()
        .map(AccountLogin::session)
}

/// 使用当前短期 access token 创建 Account 业务 API 会话。
///
/// 默认用户与角色管理 Feature 使用该接口；自定义 Feature 也可以直接复用全部公开的
/// 用户、角色和权限方法，而无需自行读取或复制 Bearer token。
pub fn api_session(cx: &App) -> Option<AccountSession> {
    let state = cx.try_global::<AccountLoginState>()?;
    let login = state.login.as_ref()?;
    Some(
        state
            .authenticator
            .account
            .session(login.session().tokens().access_token.clone()),
    )
}

/// 开始一次 Account Authorization Code + PKCE 登录。
///
/// OIDC discovery、loopback callback、token 交换和 `/me` 校验都在后台执行；授权 URL
/// 准备完成后由 GPUI 打开系统浏览器。异步失败会写入 [`login_snapshot`] 的状态文案。
///
/// # Errors
///
/// 尚未安装认证协调器，或已有登录流程正在执行时返回错误。
pub fn start_login(cx: &mut App) -> Result<(), AccountLoginRuntimeError> {
    if !cx.has_global::<AccountLoginState>() {
        return Err(AccountLoginRuntimeError::NotInstalled);
    }
    if cx.global::<AccountLoginState>().busy {
        return Err(AccountLoginRuntimeError::LoginInProgress);
    }

    let cancellation = Arc::new(AtomicBool::new(false));
    let login_window = cx.active_window();
    let (authenticator, generation) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.generation = state.generation.wrapping_add(1);
        state.busy = true;
        state.status = "正在连接认证服务...".into();
        state.failure = None;
        state.cancellation = Some(cancellation.clone());
        state.login_window = login_window;
        (state.authenticator.clone(), state.generation)
    };
    refresh_login_windows(cx);
    let begin_task = cx.background_spawn(async move {
        authenticator.begin_login_with_prompt(Some(oidc::OidcPrompt::SelectAccount))
    });
    cx.spawn(async move |cx| {
        let result = begin_task.await;
        cx.update(|cx| match result {
            Ok(pending) => open_authorization_url(pending, generation, cancellation, cx),
            Err(error) => {
                complete_login(Err(error), generation, cx);
            }
        });
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="登录任务属于应用级 Account Global 生命周期"
    .detach();
    Ok(())
}

/// 清除当前进程中的 Account 会话并重新显示登录门禁。
///
/// 退出会先作废当前 generation 和界面状态，再在后台禁止恢复、删除安全凭据并尽力
/// 撤销内存中的 refresh token；不会调用 Provider 全局退出或清除浏览器 SSO Cookie。
pub fn sign_out(cx: &mut App) {
    if !cx.has_global::<AccountLoginState>() {
        return;
    }
    let (authenticator, credential_service, refresh_token) = {
        let state = cx.global_mut::<AccountLoginState>();
        let refresh_token = state
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.refresh_token.clone());
        if let Some(cancellation) = state.cancellation.take() {
            cancellation.store(true, Ordering::Release);
        }
        state.generation = state.generation.wrapping_add(1);
        state.authentication_revision = state.authentication_revision.wrapping_add(1);
        state.login = None;
        state.tokens = None;
        state.busy = false;
        state.restoring = false;
        state.refresh_scheduled = false;
        state.refresh_in_flight = false;
        state.recovery_allowed = false;
        state.can_retry_recovery = false;
        state.status = "已退出登录".into();
        state.failure = None;
        state.login_window = None;
        (
            state.authenticator.clone(),
            state.credential_service.clone(),
            refresh_token,
        )
    };
    crate::application::update_shell_preferences(cx, |preferences| {
        preferences.account.recovery_allowed = false;
    });
    let preferences_flush = crate::application::shell_preferences_flush_signal(cx);
    refresh_login_windows(cx);

    let key = credential_key(&authenticator);
    let cleanup = cx.background_spawn(async move {
        if let Some(receiver) = preferences_flush {
            _ = receiver.recv();
        }
        if let Some(service) = credential_service {
            let _ = secure_delete(service.as_str(), key.as_str());
        }
        if let Some(refresh_token) = refresh_token {
            let _ = authenticator.revoke_refresh_token(refresh_token.as_str());
        }
    });
    cx.spawn(async move |_cx| {
        cleanup.await;
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="退出清理属于应用级 Account Global 生命周期"
    .detach();
}

/// 更新“保持登录状态”偏好。
///
/// Linux 上该调用会保持未选中且不会接入任何凭据存储。取消勾选会立即禁止恢复并在
/// 后台删除已有安全凭据。
pub fn set_remember_login(remember_login: bool, cx: &mut App) {
    let Some(state) = cx.try_global::<AccountLoginState>() else {
        return;
    };
    let remember_login = state.secure_storage_supported && remember_login;
    let (authenticator, credential_service) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.remember_login = remember_login;
        if !remember_login {
            state.recovery_allowed = false;
        }
        (
            state.authenticator.clone(),
            state.credential_service.clone(),
        )
    };
    crate::application::update_shell_preferences(cx, |preferences| {
        preferences.account.remember_login = remember_login;
        if !remember_login {
            preferences.account.recovery_allowed = false;
        }
    });
    if !remember_login {
        let preferences_flush = crate::application::shell_preferences_flush_signal(cx);
        let key = credential_key(&authenticator);
        cx.background_spawn(async move {
            if let Some(receiver) = preferences_flush {
                _ = receiver.recv();
            }
            if let Some(service) = credential_service {
                let _ = secure_delete(service.as_str(), key.as_str());
            }
        })
        // nexora-lint: allow(nexora::detached_lifecycle) reason="删除安全凭据必须在偏好写入完成后于后台执行，不能阻塞 GPUI 事件循环"
        .detach();
    }
    refresh_login_windows(cx);
}

/// 手动重试最近一次静默恢复，不会自动打开浏览器。
///
/// # Errors
///
/// 当 Account 认证器尚未安装时返回 [`AccountLoginRuntimeError::NotInstalled`]；已有登录
/// 或恢复任务运行时返回 [`AccountLoginRuntimeError::LoginInProgress`]。恢复请求本身的
/// OIDC 错误会通过登录状态暴露，不会由本函数同步返回。
pub fn retry_recovery(cx: &mut App) -> Result<(), AccountLoginRuntimeError> {
    if !cx.has_global::<AccountLoginState>() {
        return Err(AccountLoginRuntimeError::NotInstalled);
    }
    if cx.global::<AccountLoginState>().busy {
        return Err(AccountLoginRuntimeError::LoginInProgress);
    }
    start_recovery(cx);
    Ok(())
}

/// 放弃当前恢复许可并以账号选择模式重新打开交互式登录。
///
/// # Errors
///
/// 当 Account 认证器尚未安装或已有登录/恢复任务运行时，返回对应的
/// [`AccountLoginRuntimeError`]；浏览器登录过程及其 OIDC 错误会通过登录状态异步暴露。
pub fn login_with_other_account(cx: &mut App) -> Result<(), AccountLoginRuntimeError> {
    invalidate_current_attempt(cx);
    set_recovery_disabled(cx);
    start_login(cx)
}

/// 观察当前 Entity 所属应用中的 Account 认证作用域变化。
///
/// 回调只在 [`AccountAuthenticationScope`] 发生变化时触发，不会因登录状态文案或忙碌状态
/// 更新而触发。订阅不会主动发送初始值；构造 Entity 时应先调用 [`authentication_scope`]
/// 读取当前作用域，并把返回的 [`Subscription`] 保存在与 Entity 相同的生命周期中。
/// 丢弃订阅会立即停止观察。
pub fn observe_authentication<T>(
    cx: &mut Context<T>,
    mut observer: impl FnMut(&mut T, AccountAuthenticationScope, &mut Context<T>) + 'static,
) -> Subscription
where
    T: 'static,
{
    let mut previous = authentication_scope(cx);
    cx.observe_global::<AccountLoginState>(move |this, cx| {
        let current = authentication_scope(cx);
        if current == previous {
            return;
        }
        previous = current.clone();
        observer(this, current, cx);
    })
}

pub(crate) fn observe_authentication_in<T>(
    window: &Window,
    cx: &mut Context<T>,
    observer: impl FnMut(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> Subscription
where
    T: 'static,
{
    cx.observe_global_in::<AccountLoginState>(window, observer)
}

fn open_authorization_url(
    pending: PendingAccountLogin,
    generation: u64,
    cancellation: Arc<AtomicBool>,
    cx: &mut App,
) {
    let authorization_url = pending.authorization_url().to_owned();
    if !update_status(generation, true, "已打开浏览器，正在等待登录...", cx) {
        return;
    }
    cx.open_url(authorization_url.as_str());

    let login_task = cx.background_spawn(async move {
        pending.finish_with_cancellation(|| cancellation.load(Ordering::Acquire))
    });
    cx.spawn(async move |cx| {
        let result = login_task.await;
        cx.update(|cx| {
            let succeeded = result.is_ok();
            if complete_login(result, generation, cx) && succeeded {
                cx.activate(true);
            }
        });
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="loopback 回调等待属于应用级 Account Global 生命周期"
    .detach();
}

fn complete_login(
    result: Result<AccountLogin, AccountAuthenticationError>,
    generation: u64,
    cx: &mut App,
) -> bool {
    if !attempt_is_current(generation, cx) {
        return false;
    }
    match result {
        Ok(login) => {
            let tokens = login.session().tokens().clone();
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.busy = false;
            state.restoring = false;
            state.cancellation = None;
            state.login_window = None;
            state.can_retry_recovery = false;
            state.tokens = Some(tokens.clone());
            state.login = Some(login);
            state.status = "登录成功".into();
            state.failure = None;
            persist_session(tokens, generation, true, cx);
        }
        Err(error) => {
            let failure = login_failure(&error);
            let displayed = push_login_failure_notification(&failure, cx);
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.busy = false;
            state.restoring = false;
            state.cancellation = None;
            state.login_window = None;
            state.login = None;
            state.tokens = None;
            state.can_retry_recovery = false;
            state.status = if displayed {
                "未登录".into()
            } else {
                failure.message.clone()
            };
            state.failure = Some(failure);
        }
    }
    refresh_login_windows(cx);
    true
}

fn update_status(
    generation: u64,
    busy: bool,
    status: impl Into<SharedString>,
    cx: &mut App,
) -> bool {
    if !attempt_is_current(generation, cx) {
        return false;
    }
    let state = cx.global_mut::<AccountLoginState>();
    state.busy = busy;
    state.status = status.into();
    refresh_login_windows(cx);
    true
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCredential {
    version: u8,
    refresh_token: String,
    subject: String,
    profile: OidcUserProfile,
}

enum SecureStorageError {
    Unavailable,
    Corrupt,
}

fn secure_storage_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn credential_key(authenticator: &AccountAuthenticator) -> String {
    let config = authenticator.oidc.config();
    let material = format!("{}\0{}", config.issuer_url(), config.client_id());
    let digest = Sha256::digest(material.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn credential_service(cx: &App) -> Option<String> {
    crate::application::application_identity(cx)
        .map(|application_identity| format!("{application_identity}.account.oidc"))
}

fn stored_credential(tokens: &OidcTokenCache) -> Option<StoredCredential> {
    Some(StoredCredential {
        version: 1,
        refresh_token: tokens
            .refresh_token
            .clone()
            .filter(|token| !token.trim().is_empty())?,
        subject: tokens.profile.as_ref()?.subject.clone(),
        profile: tokens.profile.clone()?,
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn secure_load(service: &str, key: &str) -> Result<Option<StoredCredential>, SecureStorageError> {
    let entry = Entry::new(service, key).map_err(|_| SecureStorageError::Unavailable)?;
    let value = match entry.get_password() {
        Ok(value) => value,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_) => return Err(SecureStorageError::Unavailable),
    };
    let credential = serde_json::from_str::<StoredCredential>(value.as_str())
        .map_err(|_| SecureStorageError::Corrupt)?;
    if credential.version != 1
        || credential.refresh_token.trim().is_empty()
        || credential.subject.trim().is_empty()
        || credential.subject != credential.profile.subject
    {
        return Err(SecureStorageError::Corrupt);
    }
    Ok(Some(credential))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn secure_load(_service: &str, _key: &str) -> Result<Option<StoredCredential>, SecureStorageError> {
    Ok(None)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn secure_save(
    service: &str,
    key: &str,
    credential: &StoredCredential,
) -> Result<(), SecureStorageError> {
    let value = serde_json::to_string(credential).map_err(|_| SecureStorageError::Unavailable)?;
    let entry = Entry::new(service, key).map_err(|_| SecureStorageError::Unavailable)?;
    entry
        .set_password(value.as_str())
        .map_err(|_| SecureStorageError::Unavailable)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn secure_save(
    _service: &str,
    _key: &str,
    _credential: &StoredCredential,
) -> Result<(), SecureStorageError> {
    Err(SecureStorageError::Unavailable)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn secure_delete(service: &str, key: &str) -> Result<(), SecureStorageError> {
    let entry = Entry::new(service, key).map_err(|_| SecureStorageError::Unavailable)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecureStorageError::Unavailable),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn secure_delete(_service: &str, _key: &str) -> Result<(), SecureStorageError> {
    Ok(())
}

fn set_recovery_disabled(cx: &mut App) {
    if !cx.has_global::<AccountLoginState>() {
        return;
    }
    let (authenticator, credential_service) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.recovery_allowed = false;
        state.can_retry_recovery = false;
        (
            state.authenticator.clone(),
            state.credential_service.clone(),
        )
    };
    crate::application::update_shell_preferences(cx, |preferences| {
        preferences.account.recovery_allowed = false;
    });
    let preferences_flush = crate::application::shell_preferences_flush_signal(cx);
    let key = credential_key(&authenticator);
    cx.background_spawn(async move {
        if let Some(receiver) = preferences_flush {
            _ = receiver.recv();
        }
        if let Some(service) = credential_service {
            let _ = secure_delete(service.as_str(), key.as_str());
        }
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="删除安全凭据必须在偏好写入完成后于后台执行，不能阻塞 GPUI 事件循环"
    .detach();
}

fn invalidate_current_attempt(cx: &mut App) {
    if let Some(state) = cx.try_global::<AccountLoginState>()
        && let Some(cancellation) = state.cancellation.as_ref()
    {
        cancellation.store(true, Ordering::Release);
    }
    let state = cx.global_mut::<AccountLoginState>();
    state.generation = state.generation.wrapping_add(1);
    state.cancellation = None;
    state.busy = false;
    state.restoring = false;
    state.refresh_scheduled = false;
    state.refresh_in_flight = false;
}

fn start_recovery(cx: &mut App) {
    let Some(state) = cx.try_global::<AccountLoginState>() else {
        return;
    };
    if state
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.refresh_token.as_ref())
        .is_some()
        && (!state.recovery_allowed || !state.secure_storage_supported)
    {
        start_memory_recovery(cx);
        return;
    }
    if !state.secure_storage_supported || !state.remember_login || !state.recovery_allowed {
        return;
    }
    let cancellation = Arc::new(AtomicBool::new(false));
    let (authenticator, credential_service, generation) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.generation = state.generation.wrapping_add(1);
        state.busy = true;
        state.restoring = true;
        state.can_retry_recovery = false;
        state.status = "正在恢复登录状态…".into();
        state.failure = None;
        state.cancellation = Some(cancellation);
        (
            state.authenticator.clone(),
            state
                .credential_service
                .clone()
                .expect("安全存储恢复必须具有应用凭据命名空间"),
            state.generation,
        )
    };
    refresh_login_windows(cx);
    let key = credential_key(&authenticator);
    let load =
        cx.background_spawn(async move { secure_load(credential_service.as_str(), key.as_str()) });
    cx.spawn(async move |cx| {
        let result = load.await;
        cx.update(|cx| finish_recovery_load(result, authenticator, generation, cx));
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="静默恢复属于应用级 Account Global 生命周期"
    .detach();
}

fn start_memory_recovery(cx: &mut App) {
    let (authenticator, tokens, generation) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.generation = state.generation.wrapping_add(1);
        state.busy = true;
        state.restoring = true;
        state.refresh_in_flight = true;
        state.can_retry_recovery = false;
        state.status = "正在恢复登录状态…".into();
        state.failure = None;
        (
            state.authenticator.clone(),
            state
                .tokens
                .clone()
                .expect("memory recovery requires tokens"),
            state.generation,
        )
    };
    refresh_login_windows(cx);
    let refresh_tokens = tokens.clone();
    let refresh = cx.background_spawn(async move { authenticator.refresh(&refresh_tokens) });
    cx.spawn(async move |cx| {
        let result = refresh.await;
        cx.update(|cx| finish_refresh(result, generation, cx));
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="内存 refresh token 恢复属于应用级 Account Global 生命周期"
    .detach();
}

fn finish_recovery_load(
    result: Result<Option<StoredCredential>, SecureStorageError>,
    authenticator: AccountAuthenticator,
    generation: u64,
    cx: &mut App,
) {
    if !attempt_is_current(generation, cx) {
        return;
    }
    let credential = match result {
        Ok(credential) => credential,
        Err(SecureStorageError::Corrupt) => {
            set_recovery_failure(generation, "无法读取保存的登录状态，请重新登录", false, cx);
            set_recovery_disabled(cx);
            return;
        }
        Err(SecureStorageError::Unavailable) => {
            set_recovery_failure(generation, "暂时无法访问安全存储，请稍后重试", true, cx);
            return;
        }
    };
    let Some(credential) = credential else {
        crate::application::update_shell_preferences(cx, |preferences| {
            preferences.account.recovery_allowed = false;
        });
        let state = cx.global_mut::<AccountLoginState>();
        state.recovery_allowed = false;
        state.busy = false;
        state.restoring = false;
        state.cancellation = None;
        state.status = "未登录".into();
        refresh_login_windows(cx);
        return;
    };
    let tokens = OidcTokenCache {
        refresh_token: Some(credential.refresh_token),
        profile: Some(credential.profile),
        ..Default::default()
    };
    let refresh_tokens = tokens.clone();
    let refresh = cx.background_spawn(async move { authenticator.refresh(&refresh_tokens) });
    cx.spawn(async move |cx| {
        let result = refresh.await;
        cx.update(|cx| finish_recovery(result, tokens, generation, cx));
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="静默恢复刷新属于应用级 Account Global 生命周期"
    .detach();
}

fn finish_recovery(
    result: Result<AccountLogin, AccountAuthenticationError>,
    previous_tokens: OidcTokenCache,
    generation: u64,
    cx: &mut App,
) {
    if !attempt_is_current(generation, cx) {
        return;
    }
    match result {
        Ok(login) => {
            let tokens = login.session().tokens().clone();
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.busy = false;
            state.restoring = false;
            state.cancellation = None;
            state.login = Some(login);
            state.tokens = Some(tokens.clone());
            state.status = "登录状态已恢复".into();
            state.failure = None;
            persist_session(tokens, generation, true, cx);
        }
        Err(error) if is_permanent_failure(&error) => {
            set_recovery_disabled(cx);
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.busy = false;
            state.restoring = false;
            state.cancellation = None;
            state.login = None;
            state.tokens = None;
            state.can_retry_recovery = false;
            state.status = "保存的登录状态已失效，请重新登录".into();
            state.failure = Some(AccountLoginFailure {
                message: "保存的登录状态已失效，请重新登录".into(),
                request_id: None,
            });
            refresh_login_windows(cx);
        }
        Err(_) => {
            let state = cx.global_mut::<AccountLoginState>();
            state.busy = false;
            state.restoring = false;
            state.cancellation = None;
            state.tokens = Some(previous_tokens);
            state.can_retry_recovery = true;
            state.status = "暂时无法恢复登录状态，请重试".into();
            state.failure = Some(AccountLoginFailure {
                message: "暂时无法恢复登录状态，请重试".into(),
                request_id: None,
            });
            refresh_login_windows(cx);
        }
    }
}

fn set_recovery_failure(generation: u64, message: &str, retryable: bool, cx: &mut App) {
    if !attempt_is_current(generation, cx) {
        return;
    }
    let state = cx.global_mut::<AccountLoginState>();
    state.busy = false;
    state.restoring = false;
    state.cancellation = None;
    state.can_retry_recovery = retryable;
    state.status = message.into();
    state.failure = Some(AccountLoginFailure {
        message: message.into(),
        request_id: None,
    });
    refresh_login_windows(cx);
}

fn is_permanent_failure(error: &AccountAuthenticationError) -> bool {
    match error {
        AccountAuthenticationError::Oidc(error) => {
            error.is_refresh_token_rejected() || matches!(error, oidc::OidcError::SubjectMismatch)
        }
        AccountAuthenticationError::Account(AccountClientError::Rejected { code, .. }) => {
            matches!(
                code.as_str(),
                "account_suspended" | "account_not_registered"
            )
        }
        AccountAuthenticationError::Account(_) => false,
    }
}

fn persist_session(tokens: OidcTokenCache, generation: u64, force: bool, cx: &mut App) {
    if tokens.refresh_token.is_some() {
        schedule_refresh(generation, tokens.expires_at, cx);
    }
    let Some(state) = cx.try_global::<AccountLoginState>() else {
        return;
    };
    if !state.secure_storage_supported || !state.remember_login {
        return;
    }
    if !force && state.recovery_allowed {
        return;
    }
    let Some(credential) = stored_credential(&tokens) else {
        let state = cx.global_mut::<AccountLoginState>();
        state.recovery_allowed = false;
        state.status = "当前账号未提供可恢复凭据".into();
        crate::application::update_shell_preferences(cx, |preferences| {
            preferences.account.recovery_allowed = false;
        });
        refresh_login_windows(cx);
        return;
    };
    let authenticator = state.authenticator.clone();
    let credential_service = state
        .credential_service
        .clone()
        .expect("安全凭据持久化必须具有应用凭据命名空间");
    let key = credential_key(&authenticator);
    {
        let state = cx.global_mut::<AccountLoginState>();
        state.recovery_allowed = false;
    }
    crate::application::update_shell_preferences(cx, |preferences| {
        preferences.account.recovery_allowed = false;
    });
    let preferences_flush = crate::application::shell_preferences_flush_signal(cx);
    let save = cx.background_spawn(async move {
        if let Some(receiver) = preferences_flush {
            _ = receiver.recv();
        }
        secure_save(credential_service.as_str(), key.as_str(), &credential)
    });
    cx.spawn(async move |cx| {
        let result = save.await;
        cx.update(|cx| {
            if !attempt_is_current(generation, cx) {
                return;
            }
            match result {
                Ok(()) => {
                    let state = cx.global_mut::<AccountLoginState>();
                    state.recovery_allowed = true;
                    state.secure_storage_available = true;
                    crate::application::update_shell_preferences(cx, |preferences| {
                        preferences.account.recovery_allowed = true;
                    });
                }
                Err(_) => {
                    let state = cx.global_mut::<AccountLoginState>();
                    state.secure_storage_available = false;
                    state.status = "保持登录状态失败，当前会话仍可继续使用".into();
                    state.failure = Some(AccountLoginFailure {
                        message: "保持登录状态失败，当前会话仍可继续使用".into(),
                        request_id: None,
                    });
                }
            }
            refresh_login_windows(cx);
        });
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="安全凭据写入属于应用级 Account Global 生命周期"
    .detach();
}

fn schedule_refresh(generation: u64, expires_at: Option<u64>, cx: &mut App) {
    let Some(expires_at) = expires_at else {
        return;
    };
    if !cx.has_global::<AccountLoginState>() {
        return;
    }
    let state = cx.global_mut::<AccountLoginState>();
    if state.refresh_scheduled || state.refresh_in_flight {
        return;
    }
    state.refresh_scheduled = true;
    let delay_seconds = expires_at
        .saturating_sub(unix_seconds().saturating_add(60))
        .max(5);
    let timer = cx
        .background_executor()
        .timer(Duration::from_secs(delay_seconds));
    cx.spawn(async move |cx| {
        timer.await;
        cx.update(|cx| begin_refresh(generation, cx));
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="自动续期计时器属于应用级 Account Global 生命周期"
    .detach();
}

fn begin_refresh(generation: u64, cx: &mut App) {
    if !attempt_is_current(generation, cx) {
        return;
    }
    let (authenticator, tokens) = {
        let state = cx.global_mut::<AccountLoginState>();
        state.refresh_scheduled = false;
        if state.refresh_in_flight {
            return;
        }
        let Some(tokens) = state.tokens.clone() else {
            return;
        };
        state.refresh_in_flight = true;
        (state.authenticator.clone(), tokens)
    };
    let refresh = cx.background_spawn(async move { authenticator.refresh(&tokens) });
    cx.spawn(async move |cx| {
        let result = refresh.await;
        cx.update(|cx| finish_refresh(result, generation, cx));
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="自动续期请求属于应用级 Account Global 生命周期"
    .detach();
}

fn finish_refresh(
    result: Result<AccountLogin, AccountAuthenticationError>,
    generation: u64,
    cx: &mut App,
) {
    if !attempt_is_current(generation, cx) {
        return;
    }
    let previous_expired = cx
        .global::<AccountLoginState>()
        .tokens
        .as_ref()
        .is_some_and(OidcTokenCache::is_expired);
    cx.global_mut::<AccountLoginState>().refresh_in_flight = false;
    match result {
        Ok(login) => {
            let tokens = login.session().tokens().clone();
            let rotated = cx
                .global::<AccountLoginState>()
                .tokens
                .as_ref()
                .and_then(|old| old.refresh_token.as_ref())
                != tokens.refresh_token.as_ref();
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.login = Some(login);
            state.tokens = Some(tokens.clone());
            state.busy = false;
            state.restoring = false;
            state.status = "登录状态已更新".into();
            state.failure = None;
            persist_session(tokens, generation, rotated, cx);
        }
        Err(error) if is_permanent_failure(&error) => {
            set_recovery_disabled(cx);
            let state = cx.global_mut::<AccountLoginState>();
            state.authentication_revision = state.authentication_revision.wrapping_add(1);
            state.login = None;
            state.tokens = None;
            state.status = "登录状态已失效，请重新登录".into();
            state.failure = Some(AccountLoginFailure {
                message: "登录状态已失效，请重新登录".into(),
                request_id: None,
            });
            refresh_login_windows(cx);
        }
        Err(error) => {
            if previous_expired {
                let state = cx.global_mut::<AccountLoginState>();
                state.login = None;
                state.status = "登录已过期，请重试恢复".into();
                state.failure = Some(login_failure(&error));
                state.can_retry_recovery = true;
            } else {
                let state = cx.global_mut::<AccountLoginState>();
                state.status = "登录状态暂时无法更新，将自动重试".into();
                state.failure = Some(login_failure(&error));
            }
            schedule_refresh(
                generation,
                cx.global::<AccountLoginState>()
                    .tokens
                    .as_ref()
                    .and_then(|t| t.expires_at),
                cx,
            );
            refresh_login_windows(cx);
        }
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

struct LoginFailureNotification;

fn login_failure(error: &AccountAuthenticationError) -> AccountLoginFailure {
    match error {
        AccountAuthenticationError::Account(AccountClientError::Rejected {
            message,
            request_id,
            ..
        }) => AccountLoginFailure {
            message: message.clone().into(),
            request_id: (!request_id.trim().is_empty() && request_id != "unknown")
                .then(|| request_id.clone().into()),
        },
        AccountAuthenticationError::Account(error) => AccountLoginFailure {
            message: error.user_message().into(),
            request_id: None,
        },
        AccountAuthenticationError::Oidc(error) => AccountLoginFailure {
            message: error.to_string().into(),
            request_id: None,
        },
    }
}

fn push_login_failure_notification(failure: &AccountLoginFailure, cx: &mut App) -> bool {
    let window_handle = cx
        .global::<AccountLoginState>()
        .login_window
        .or_else(|| cx.active_window());
    let Some(window_handle) = window_handle else {
        return false;
    };
    let message = failure.request_id.as_ref().map_or_else(
        || failure.message.clone(),
        |request_id| format!("{}\n请求 ID：{request_id}", failure.message).into(),
    );
    let mut notification = Notification::error(message)
        .id::<LoginFailureNotification>()
        .title("登录失败");
    if let Some(request_id) = failure.request_id.clone() {
        notification = notification.action(move |_, _, cx| {
            let request_id = request_id.clone();
            Button::new("copy-account-login-request-id")
                .icon(IconName::Copy)
                .label("复制请求 ID")
                .small()
                .on_click(cx.listener(move |notification, _, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(request_id.to_string()));
                    notification.dismiss(window, cx);
                }))
        });
    }

    window_handle
        .update(cx, |_, window, cx| {
            window.push_notification(notification, cx);
        })
        .is_ok()
}

fn attempt_is_current(generation: u64, cx: &App) -> bool {
    cx.try_global::<AccountLoginState>()
        .is_some_and(|state| state.generation == generation)
}

fn refresh_login_windows(cx: &mut App) {
    // nexora-lint: allow(nexora::global_refresh_scope) reason="登录状态提示属于全窗口认证门禁"
    cx.refresh_windows();
}
