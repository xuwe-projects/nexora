//! Account 桌面登录流程与 GPUI 应用状态协调。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Global, SharedString,
    Subscription, Window,
};
use gpui_component::{
    IconName, Sizable as _, WindowExt as _, button::Button, notification::Notification,
};
use thiserror::Error;

use super::{
    AccountAuthenticationError, AccountAuthenticator, AccountClientError, AccountLogin,
    AccountSession, PendingAccountLogin,
};
use contracts::account::AccessProfileResponse;
use oidc::OidcSession;

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
    /// 适合直接显示在登录门禁中的当前状态或最近一次错误。
    pub status: SharedString,
    /// 最近一次登录失败的结构化信息；成功或开始下一次登录后为 `None`。
    pub failure: Option<AccountLoginFailure>,
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
    login: Option<AccountLogin>,
    busy: bool,
    status: SharedString,
    failure: Option<AccountLoginFailure>,
    generation: u64,
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
    let generation = if let Some(state) = cx.try_global::<AccountLoginState>() {
        if let Some(cancellation) = state.cancellation.as_ref() {
            cancellation.store(true, Ordering::Release);
        }
        state.generation.wrapping_add(1)
    } else {
        0
    };
    let state = AccountLoginState {
        authenticator,
        login: None,
        busy: false,
        status: "未登录".into(),
        failure: None,
        generation,
        cancellation: None,
        login_window: None,
    };
    if cx.has_global::<AccountLoginState>() {
        *cx.global_mut::<AccountLoginState>() = state;
    } else {
        cx.set_global(state);
    }
    refresh_login_windows(cx);
}

/// 返回当前 Account 登录状态的无敏感信息快照。
pub fn login_snapshot(cx: &App) -> AccountLoginSnapshot {
    if !cx.has_global::<AccountLoginState>() {
        return AccountLoginSnapshot {
            configured: false,
            authenticated: false,
            busy: false,
            status: "未配置 Account 登录".into(),
            failure: None,
        };
    }

    let state = cx.global::<AccountLoginState>();
    AccountLoginSnapshot {
        configured: true,
        authenticated: state.login.is_some(),
        busy: state.busy,
        status: state.status.clone(),
        failure: state.failure.clone(),
    }
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
    let begin_task = cx.background_spawn(async move { authenticator.begin_login() });
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
/// 当前实现不会持久化 refresh token，因此退出只需要释放内存中的 OIDC 与业务资料。
/// 尚未完成的浏览器登录结果会被作废，不能在退出后重新写回会话。
pub fn sign_out(cx: &mut App) {
    if !cx.has_global::<AccountLoginState>() {
        return;
    }
    let state = cx.global_mut::<AccountLoginState>();
    if let Some(cancellation) = state.cancellation.take() {
        cancellation.store(true, Ordering::Release);
    }
    state.generation = state.generation.wrapping_add(1);
    state.login = None;
    state.busy = false;
    state.status = "已退出登录".into();
    state.failure = None;
    state.login_window = None;
    refresh_login_windows(cx);
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
            let state = cx.global_mut::<AccountLoginState>();
            state.busy = false;
            state.cancellation = None;
            state.login_window = None;
            state.login = Some(login);
            state.status = "登录成功".into();
            state.failure = None;
        }
        Err(error) => {
            let failure = login_failure(&error);
            let displayed = push_login_failure_notification(&failure, cx);
            let state = cx.global_mut::<AccountLoginState>();
            state.busy = false;
            state.cancellation = None;
            state.login_window = None;
            state.login = None;
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
