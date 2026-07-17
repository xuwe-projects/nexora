//! Account 与 ZITADEL 组合使用的一次性系统初始化页面。

use std::{
    error::Error,
    fmt::Write as _,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use axum::{
    Form, Router,
    extract::State,
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, X_CONTENT_TYPE_OPTIONS},
    },
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use rand::{TryRngCore as _, rngs::OsRng};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::account_module::{
    Account, AccountError, AccountInitialization, AccountInitializationOutcome,
    directory::{DirectoryUser, ZitadelUserDirectory},
};

const SETUP_TOKEN_BYTES: usize = 32;
const SETUP_SESSION_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// `/setup` 解锁请求必须提供的字段契约。
///
/// 应用可以用自己的 Form 字段结构实现该 trait，但必须返回用户提交的 setup secret，
/// 以便框架执行固定的哈希校验。
pub trait SetupUnlockRequest: serde::de::DeserializeOwned + Send + 'static {
    /// 返回本次请求提交的 setup secret。
    fn setup_secret(&self) -> &str;
}

/// `/setup/complete` 完成请求必须提供的字段契约。
///
/// 框架使用一次性 token 防止绕过解锁步骤，并根据 identity ID 回查 ZITADEL 人类用户；
/// 自定义请求模型不能直接提交未经目录验证的用户资料。
pub trait SetupCompletionRequest: serde::de::DeserializeOwned + Send + 'static {
    /// 返回解锁成功后由框架签发的一次性页面 token。
    fn setup_token(&self) -> &str;

    /// 返回要绑定为系统超级管理员的 ZITADEL 用户 identity ID。
    fn super_admin_identity_id(&self) -> &str;
}

/// Account 一次性初始化流程的请求模型与 Axum 响应定制接口。
///
/// 框架始终负责 secret 校验、短期 setup 会话、ZITADEL 用户二次核对、系统角色同步和
/// 超级管理员数据库事务。实现者只定义请求字段映射与表现层响应，因而可以返回 HTML、JSON
/// 或重定向等任意 [`IntoResponse`]，但不能绕过初始化所需输入和业务不变量。
pub trait Setup: Clone + Send + Sync + 'static {
    /// 解锁 `/setup` 时使用的 Form 请求类型。
    type UnlockRequest: SetupUnlockRequest;
    /// 提交超级管理员选择时使用的 Form 请求类型。
    type CompletionRequest: SetupCompletionRequest;

    /// 返回输入 setup secret 的初始或校验失败响应内容。
    fn unlock_response(&self, error: Option<&str>) -> impl IntoResponse;

    /// 返回包含可信 ZITADEL 人类用户列表与一次性 setup token 的选择响应内容。
    fn selection_response(&self, users: &[DirectoryUser], setup_token: &str) -> impl IntoResponse;

    /// 返回系统初始化成功后的响应内容。
    fn completed_response(&self, super_admin: &DirectoryUser) -> impl IntoResponse;

    /// 返回框架内部或外部依赖暂时失败时的响应内容。
    fn error_response(&self) -> impl IntoResponse;

    /// 返回系统已经初始化、setup 路由永久关闭后的响应内容。
    fn not_found_response(&self) -> impl IntoResponse;
}

/// Nexora 内置的 HTML setup 流程。
///
/// 默认页面先收集 setup secret，再展示通过 PAT 从 ZITADEL 读取的启用状态人类用户，
/// 最后提交一次性 token 与选中的 identity ID。
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultSetup;

/// 默认 HTML setup 流程的解锁 Form。
#[derive(Deserialize)]
pub struct DefaultSetupUnlockRequest {
    secret: String,
}

impl SetupUnlockRequest for DefaultSetupUnlockRequest {
    fn setup_secret(&self) -> &str {
        self.secret.as_str()
    }
}

/// 默认 HTML setup 流程的超级管理员选择 Form。
#[derive(Deserialize)]
pub struct DefaultSetupCompletionRequest {
    setup_token: String,
    identity_id: String,
}

impl SetupCompletionRequest for DefaultSetupCompletionRequest {
    fn setup_token(&self) -> &str {
        self.setup_token.as_str()
    }

    fn super_admin_identity_id(&self) -> &str {
        self.identity_id.as_str()
    }
}

impl Setup for DefaultSetup {
    type UnlockRequest = DefaultSetupUnlockRequest;
    type CompletionRequest = DefaultSetupCompletionRequest;

    fn unlock_response(&self, error: Option<&str>) -> impl IntoResponse {
        html_response(StatusCode::OK, secret_page(error))
    }

    fn selection_response(&self, users: &[DirectoryUser], setup_token: &str) -> impl IntoResponse {
        html_response(StatusCode::OK, selection_page(users, setup_token))
    }

    fn completed_response(&self, super_admin: &DirectoryUser) -> impl IntoResponse {
        html_response(
            StatusCode::OK,
            completed_page(super_admin.display_name.as_str()),
        )
    }

    fn error_response(&self) -> impl IntoResponse {
        internal_error()
    }

    fn not_found_response(&self) -> impl IntoResponse {
        not_found()
    }
}

/// 构建只在 Account 尚未初始化时可用的默认 `/setup` 页面路由。
///
/// 页面先验证宿主配置的 setup secret，再通过 ZITADEL 目录列出启用的人类用户。提交后会
/// 幂等同步系统角色，并把选中的可信目录身份设为唯一超级管理员。初始化完成后所有 setup
/// 路由永久返回 `404 Not Found`。
pub fn setup_routes<S>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    setup_routes_with(account, directory, setup_secret, DefaultSetup)
}

/// 使用应用提供的 [`Setup`] 实现构建一次性初始化路由。
///
/// 自定义实现控制 Form 请求字段结构和响应表现；框架仍会强制执行 secret、一次性 token、
/// ZITADEL 人类用户与超级管理员 identity ID 等必要校验。
pub fn setup_routes_with<S, T>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
    setup: T,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    T: Setup,
{
    Router::new()
        .route("/setup", get(show::<T>).post(unlock::<T>))
        .route("/setup/complete", post(complete::<T>))
        .with_state::<S>(SetupState::new(account, directory, setup_secret, setup))
}

#[derive(Clone)]
struct SetupState<T> {
    account: Account,
    directory: ZitadelUserDirectory,
    secret_hash: [u8; 32],
    session: Arc<Mutex<Option<SetupSession>>>,
    setup: T,
}

impl<T> SetupState<T> {
    fn new(
        account: Account,
        directory: ZitadelUserDirectory,
        setup_secret: &str,
        setup: T,
    ) -> Self {
        Self {
            account,
            directory,
            secret_hash: digest(setup_secret),
            session: Arc::new(Mutex::new(None)),
            setup,
        }
    }

    fn verify_secret(&self, candidate: &str) -> bool {
        constant_time_eq(&self.secret_hash, &digest(candidate))
    }

    fn begin_session(&self) -> Result<String, ()> {
        let token = random_token()?;
        *self.session() = Some(SetupSession {
            token_hash: digest(token.as_str()),
            expires_at: Instant::now() + SETUP_SESSION_LIFETIME,
        });
        Ok(token)
    }

    fn valid_session(&self, token: &str) -> bool {
        let token_hash = digest(token);
        self.session().as_ref().is_some_and(|session| {
            session.expires_at > Instant::now()
                && constant_time_eq(&session.token_hash, &token_hash)
        })
    }

    fn clear_session(&self) {
        *self.session() = None;
    }

    fn session(&self) -> MutexGuard<'_, Option<SetupSession>> {
        self.session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

struct SetupSession {
    token_hash: [u8; 32],
    expires_at: Instant,
}

async fn show<T>(State(state): State<SetupState<T>>) -> Response
where
    T: Setup,
{
    match state.account.is_system_initialized().await {
        Ok(true) => setup_response(StatusCode::NOT_FOUND, state.setup.not_found_response()),
        Ok(false) => setup_response(StatusCode::OK, state.setup.unlock_response(None)),
        Err(error) => setup_error(&state.setup, "read_initialization_state", &error),
    }
}

async fn unlock<T>(
    State(state): State<SetupState<T>>,
    Form(form): Form<T::UnlockRequest>,
) -> Response
where
    T: Setup,
{
    match state.account.is_system_initialized().await {
        Ok(true) => {
            return setup_response(StatusCode::NOT_FOUND, state.setup.not_found_response());
        }
        Ok(false) => {}
        Err(error) => return setup_error(&state.setup, "read_initialization_state", &error),
    }
    if !state.verify_secret(form.setup_secret()) {
        tracing::warn!(stage = "verify_setup_secret", "setup secret 校验未通过");
        return setup_response(
            StatusCode::UNAUTHORIZED,
            state.setup.unlock_response(Some("setup secret 不正确")),
        );
    }
    let users = match state.directory.list_active_human_users().await {
        Ok(users) if !users.is_empty() => users,
        Ok(_) => {
            return setup_response(
                StatusCode::SERVICE_UNAVAILABLE,
                state
                    .setup
                    .unlock_response(Some("ZITADEL 中没有可用于初始化的启用状态人类用户")),
            );
        }
        Err(error) => return setup_error(&state.setup, "list_active_human_users", &error),
    };
    let setup_token = match state.begin_session() {
        Ok(token) => token,
        Err(()) => {
            return setup_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                state.setup.error_response(),
            );
        }
    };
    setup_response(
        StatusCode::OK,
        state
            .setup
            .selection_response(users.as_slice(), setup_token.as_str()),
    )
}

async fn complete<T>(
    State(state): State<SetupState<T>>,
    Form(form): Form<T::CompletionRequest>,
) -> Response
where
    T: Setup,
{
    match state.account.is_system_initialized().await {
        Ok(true) => {
            return setup_response(StatusCode::NOT_FOUND, state.setup.not_found_response());
        }
        Ok(false) => {}
        Err(error) => return setup_error(&state.setup, "read_initialization_state", &error),
    }
    if !state.valid_session(form.setup_token()) {
        return setup_response(
            StatusCode::UNAUTHORIZED,
            state
                .setup
                .unlock_response(Some("初始化页面会话无效或已过期，请重新输入 setup secret")),
        );
    }
    let selected = match state
        .directory
        .active_human_user(form.super_admin_identity_id())
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            return setup_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                state
                    .setup
                    .unlock_response(Some("所选用户已不存在、未启用或不是人类用户")),
            );
        }
        Err(error) => return setup_error(&state.setup, "verify_selected_user", &error),
    };
    let roles = match state.account.system_roles().await {
        Ok(roles) => roles,
        Err(error) => return setup_error(&state.setup, "read_system_roles", &error),
    };
    if let Err(error) = state.directory.ensure_project_roles(roles.as_slice()).await {
        return setup_error(&state.setup, "synchronize_system_roles", &error);
    }
    let completed_user = selected.clone();
    match state
        .account
        .initialize(AccountInitialization {
            super_admin: selected.into_external_identity(),
        })
        .await
    {
        Ok(
            AccountInitializationOutcome::Initialized { .. }
            | AccountInitializationOutcome::AlreadyInitialized { .. },
        ) => {
            state.clear_session();
            setup_response(
                StatusCode::OK,
                state.setup.completed_response(&completed_user),
            )
        }
        Err(AccountError::Conflict {
            code: "system_already_initialized",
            ..
        }) => {
            state.clear_session();
            setup_response(StatusCode::NOT_FOUND, state.setup.not_found_response())
        }
        Err(error) => setup_error(&state.setup, "complete_initialization", &error),
    }
}

fn setup_error<T>(setup: &T, stage: &'static str, error: &(dyn Error + 'static)) -> Response
where
    T: Setup,
{
    tracing::error!(stage, error = %error, "系统初始化操作失败");
    setup_response(StatusCode::INTERNAL_SERVER_ERROR, setup.error_response())
}

fn setup_response(status: StatusCode, response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    *response.status_mut() = status;
    response
}

fn secret_page(error: Option<&str>) -> String {
    let error = error
        .map(|message| format!("<p class=\"error\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    page(
        "系统初始化",
        format!(
            "<h1>系统初始化</h1><p>请输入服务端配置中的 <code>setup.secret</code>。</p>{error}<form method=\"post\" action=\"/setup\"><label for=\"secret\">setup secret</label><input id=\"secret\" name=\"secret\" type=\"password\" maxlength=\"1024\" required autofocus><button type=\"submit\">继续</button></form>",
        ),
    )
}

fn selection_page(users: &[DirectoryUser], setup_token: &str) -> String {
    let options = users.iter().fold(String::new(), |mut options, user| {
        _ = write!(
            options,
            "<option value=\"{}\">{} — {} — {}</option>",
            escape_html(user.identity_id.as_str()),
            escape_html(user.display_name.as_str()),
            escape_html(user.email.as_deref().unwrap_or("无邮箱")),
            escape_html(user.identity_id.as_str()),
        );
        options
    });
    page(
        "选择超级管理员",
        format!(
            "<h1>选择超级管理员</h1><p>候选用户由服务端通过 ZITADEL UserService 读取。提交后会先同步系统角色，再完成初始化。</p><form method=\"post\" action=\"/setup/complete\"><input name=\"setup_token\" type=\"hidden\" value=\"{}\"><label for=\"identity_id\">ZITADEL 用户</label><select id=\"identity_id\" name=\"identity_id\" size=\"10\" required>{options}</select><button type=\"submit\">完成初始化</button></form>",
            escape_html(setup_token),
        ),
    )
}

fn completed_page(display_name: &str) -> String {
    page(
        "初始化完成",
        format!(
            "<h1>初始化完成</h1><p><strong>{}</strong> 已设为超级管理员，后续访问 <code>/setup</code> 将返回 404。</p>",
            escape_html(display_name),
        ),
    )
}

fn page(title: &str, content: String) -> String {
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>:root{{color-scheme:light dark;font-family:system-ui,sans-serif}}body{{max-width:680px;margin:8vh auto;padding:24px}}form{{display:grid;gap:12px}}input,select,button{{font:inherit;padding:10px}}select{{min-height:240px}}.error{{color:#d33}}code{{font-family:ui-monospace,monospace}}</style></head><body>{content}</body></html>",
        escape_html(title),
    )
}

fn not_found() -> Response {
    html_response(
        StatusCode::NOT_FOUND,
        page("页面不存在", "<h1>页面不存在</h1>".to_owned()),
    )
}

fn internal_error() -> Response {
    html_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        page(
            "暂时无法完成请求",
            "<h1>暂时无法完成请求</h1><p>请检查服务端日志后重试。</p>".to_owned(),
        ),
    )
}

fn html_response(status: StatusCode, body: String) -> Response {
    let mut response = (status, Html(body)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn random_token() -> Result<String, ()> {
    let mut bytes = [0_u8; SETUP_TOKEN_BYTES];
    OsRng.try_fill_bytes(&mut bytes).map_err(|_| ())?;
    Ok(bytes.iter().fold(
        String::with_capacity(SETUP_TOKEN_BYTES * 2),
        |mut token, byte| {
            _ = write!(token, "{byte:02x}");
            token
        },
    ))
}

fn escape_html(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            value => escaped.push(value),
        }
        escaped
    })
}
