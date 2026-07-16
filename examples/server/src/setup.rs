//! 一次性系统初始化 HTML 向导。
//!
//! 该模块负责 setup secret 验证、人类用户选择、系统角色 gRPC 同步和 HTML 响应。初始化状态
//! 与超级管理员不变式由账号实体校验、初始化 store 函数和 PostgreSQL 约束原子保证。

use std::{
    error::Error,
    fmt::Write as _,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use account::{
    Account, AccountError, AccountInitialization, AccountInitializationOutcome,
    directory::{DirectoryUser, ZitadelUserDirectory},
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

const SETUP_TOKEN_BYTES: usize = 32;
const SETUP_SESSION_LIFETIME: Duration = Duration::from_secs(15 * 60);
const SETUP_BUSINESS_OPERATION: &str = "system_setup";

/// 构建仅在系统未初始化时可用的 `/setup` HTML 路由。
pub(crate) fn routes<S>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/setup", get(show).post(unlock))
        .route("/setup/complete", post(complete))
        .with_state::<S>(SetupState::new(account, directory, setup_secret))
}

#[derive(Clone)]
struct SetupState {
    account: Account,
    directory: ZitadelUserDirectory,
    secret_hash: [u8; 32],
    session: Arc<Mutex<Option<SetupSession>>>,
}

impl SetupState {
    fn new(account: Account, directory: ZitadelUserDirectory, setup_secret: &str) -> Self {
        Self {
            account,
            directory,
            secret_hash: digest(setup_secret),
            session: Arc::new(Mutex::new(None)),
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
        let now = Instant::now();
        let token_hash = digest(token);
        self.session().as_ref().is_some_and(|session| {
            session.expires_at > now && constant_time_eq(&session.token_hash, &token_hash)
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

#[derive(Deserialize)]
struct UnlockForm {
    secret: String,
}

#[derive(Deserialize)]
struct CompleteForm {
    setup_token: String,
    identity_id: String,
}

async fn show(State(state): State<SetupState>) -> Response {
    match state.account.is_system_initialized().await {
        Ok(true) => not_found(),
        Ok(false) => html_response(StatusCode::OK, secret_page(None)),
        Err(error) => initialization_state_error(error),
    }
}

async fn unlock(State(state): State<SetupState>, Form(form): Form<UnlockForm>) -> Response {
    match state.account.is_system_initialized().await {
        Ok(true) => return not_found(),
        Ok(false) => {}
        Err(error) => return initialization_state_error(error),
    }

    if !state.verify_secret(form.secret.as_str()) {
        tracing::warn!(
            business_operation = SETUP_BUSINESS_OPERATION,
            stage = "verify_setup_secret",
            outcome = "rejected",
            "setup secret 校验未通过"
        );
        return html_response(
            StatusCode::UNAUTHORIZED,
            secret_page(Some("setup secret 不正确")),
        );
    }

    let users = match state.directory.list_active_human_users().await {
        Ok(users) if !users.is_empty() => users,
        Ok(_) => {
            tracing::warn!(
                business_operation = SETUP_BUSINESS_OPERATION,
                stage = "list_active_human_users",
                outcome = "empty",
                "认证授权用户目录没有可用于初始化的人类用户"
            );
            return html_response(
                StatusCode::SERVICE_UNAVAILABLE,
                secret_page(Some("认证授权服务中没有可选的启用状态人类用户")),
            );
        }
        Err(error) => {
            log_setup_error("list_active_human_users", None, &error);
            return html_response(
                StatusCode::BAD_GATEWAY,
                secret_page(Some("暂时无法读取认证授权用户目录，请稍后重试")),
            );
        }
    };
    let setup_token = match state.begin_session() {
        Ok(token) => token,
        Err(()) => {
            tracing::error!(
                business_operation = SETUP_BUSINESS_OPERATION,
                stage = "generate_setup_session_token",
                error = "operating_system_random_number_generator_unavailable",
                "setup 无法生成安全的一次性页面令牌"
            );
            return internal_error();
        }
    };

    html_response(
        StatusCode::OK,
        selection_page(users.as_slice(), setup_token.as_str(), None),
    )
}

async fn complete(State(state): State<SetupState>, Form(form): Form<CompleteForm>) -> Response {
    match state.account.is_system_initialized().await {
        Ok(true) => return not_found(),
        Ok(false) => {}
        Err(error) => return initialization_state_error(error),
    }

    if !state.valid_session(form.setup_token.as_str()) {
        tracing::warn!(
            business_operation = SETUP_BUSINESS_OPERATION,
            stage = "verify_setup_session",
            outcome = "rejected",
            "setup 页面会话无效或已过期"
        );
        return html_response(
            StatusCode::UNAUTHORIZED,
            secret_page(Some("初始化页面会话已失效，请重新输入 setup secret")),
        );
    }

    let selected = match state
        .directory
        .active_human_user(form.identity_id.as_str())
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            tracing::warn!(
                business_operation = SETUP_BUSINESS_OPERATION,
                stage = "verify_selected_user",
                identity_id = %form.identity_id,
                outcome = "not_eligible",
                "所选认证授权用户不可用于初始化"
            );
            return html_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                secret_page(Some("所选用户已不存在、未启用或不是人类用户，请重新选择")),
            );
        }
        Err(error) => {
            log_setup_error(
                "verify_selected_user",
                Some(form.identity_id.as_str()),
                &error,
            );
            return html_response(
                StatusCode::BAD_GATEWAY,
                secret_page(Some("暂时无法确认所选用户，请稍后重试")),
            );
        }
    };
    let display_name = selected.display_name.clone();
    let system_roles = match state.account.system_roles().await {
        Ok(roles) => roles,
        Err(error) => {
            log_setup_error("read_system_roles", Some(form.identity_id.as_str()), &error);
            return internal_error();
        }
    };
    tracing::info!(
        business_operation = SETUP_BUSINESS_OPERATION,
        stage = "synchronize_system_roles",
        identity_id = %form.identity_id,
        role_count = system_roles.len(),
        outcome = "started",
        "开始同步本地系统角色到认证授权 Project"
    );
    if let Err(error) = state
        .directory
        .ensure_project_roles(system_roles.as_slice())
        .await
    {
        log_setup_error(
            "synchronize_system_roles",
            Some(form.identity_id.as_str()),
            &error,
        );
        return html_response(
            StatusCode::BAD_GATEWAY,
            secret_page(Some(
                "系统角色暂时无法同步到认证授权服务，系统尚未完成初始化，请检查服务端日志后重试",
            )),
        );
    }
    tracing::info!(
        business_operation = SETUP_BUSINESS_OPERATION,
        stage = "synchronize_system_roles",
        identity_id = %form.identity_id,
        role_count = system_roles.len(),
        outcome = "succeeded",
        "本地系统角色已全部存在于认证授权 Project"
    );
    match state
        .account
        .initialize(AccountInitialization {
            super_admin: selected.into_external_identity(),
        })
        .await
    {
        Ok(outcome) => {
            let (user, outcome) = match outcome {
                AccountInitializationOutcome::Initialized { super_admin } => {
                    (super_admin, "succeeded")
                }
                AccountInitializationOutcome::AlreadyInitialized { super_admin } => {
                    (super_admin, "already_initialized")
                }
            };
            state.clear_session();
            tracing::info!(
                business_operation = SETUP_BUSINESS_OPERATION,
                stage = "complete_initialization",
                outcome,
                user_id = %user.id,
                identity_id = %user.identity_id,
                "账号模块初始化已完成"
            );
            html_response(StatusCode::OK, completed_page(display_name.as_str()))
        }
        Err(AccountError::Conflict {
            code: "system_already_initialized",
            ..
        }) => {
            state.clear_session();
            tracing::info!(
                business_operation = SETUP_BUSINESS_OPERATION,
                stage = "complete_initialization",
                identity_id = %form.identity_id,
                outcome = "already_initialized",
                "并发初始化请求发现系统已完成初始化"
            );
            not_found()
        }
        Err(error) => {
            log_setup_error(
                "complete_initialization",
                Some(form.identity_id.as_str()),
                &error,
            );
            internal_error()
        }
    }
}

fn initialization_state_error(error: AccountError) -> Response {
    log_setup_error("read_initialization_state", None, &error);
    internal_error()
}

fn log_setup_error(stage: &'static str, identity_id: Option<&str>, error: &(dyn Error + 'static)) {
    let error_chain = logging::format_error_chain(error);
    tracing::error!(
        business_operation = SETUP_BUSINESS_OPERATION,
        stage,
        identity_id = identity_id.unwrap_or("not_applicable"),
        error = %error_chain,
        "系统初始化业务操作失败"
    );
}

fn secret_page(error: Option<&str>) -> String {
    let error = error
        .map(|message| format!("<div class=\"error\">{}</div>", escape_html(message)))
        .unwrap_or_default();
    page(
        "系统初始化",
        format!(
            r#"
            <div class="eyebrow">系统初始化</div>
            <h1>验证 setup secret</h1>
            <p class="lead">输入服务端配置中的 <code>setup.secret</code> 后，才能进入一次性初始化向导。</p>
            {error}
            <form method="post" action="/setup">
              <label for="secret">setup secret</label>
              <input id="secret" name="secret" type="password" autocomplete="current-password" maxlength="1024" required autofocus>
              <button type="submit">继续</button>
            </form>
            <p class="hint">密钥不会写入 URL、数据库或日志。</p>
            "#,
        ),
    )
}

fn selection_page(users: &[DirectoryUser], setup_token: &str, error: Option<&str>) -> String {
    let error = error
        .map(|message| format!("<div class=\"error\">{}</div>", escape_html(message)))
        .unwrap_or_default();
    let mut options = String::new();
    for user in users {
        let email = user.email.as_deref().unwrap_or("无邮箱");
        let username = non_empty(user.username.as_str()).unwrap_or("无用户名");
        _ = write!(
            options,
            "<option value=\"{}\">{} — {} — {} — {}</option>",
            escape_html(user.identity_id.as_str()),
            escape_html(user.display_name.as_str()),
            escape_html(email),
            escape_html(username),
            escape_html(user.identity_id.as_str()),
        );
    }
    page(
        "初始化超级管理员",
        format!(
            r#"
            <div class="eyebrow">步骤 1 / 1</div>
            <h1>选择超级管理员</h1>
            <p class="lead">候选项由服务端使用 PAT 通过 gRPC 从认证授权用户目录读取，只包含启用状态的人类用户，不包含服务账户。</p>
            <div class="notice">超级管理员不挂载角色或权限，会在授权时直接绕过全部权限校验。</div>
            <div class="notice">提交后会先通过 ProjectService v2 gRPC 确保全部本地系统角色都已创建，再完成超级管理员初始化。</div>
            {error}
            <form method="post" action="/setup/complete">
              <input name="setup_token" type="hidden" value="{}">
              <label for="identity_id">认证授权用户</label>
              <select id="identity_id" name="identity_id" size="10" required>{options}</select>
              <button class="danger" type="submit">同步系统角色、设置超级管理员并完成初始化</button>
            </form>
            <p class="hint">初始化完成后 <code>/setup</code> 将永久返回 404。</p>
            "#,
            escape_html(setup_token),
        ),
    )
}

fn completed_page(display_name: &str) -> String {
    page(
        "系统初始化完成",
        format!(
            r#"
            <div class="success-mark">✓</div>
            <h1>系统初始化完成</h1>
            <p class="lead">系统角色已同步，<strong>{}</strong> 已设为超级管理员。</p>
            <div class="notice"><code>/setup</code> 已关闭，后续访问将返回 404。</div>
            "#,
            escape_html(display_name),
        ),
    )
}

fn not_found() -> Response {
    html_response(
        StatusCode::NOT_FOUND,
        page(
            "页面不存在",
            "<h1>页面不存在</h1><p class=\"lead\">请检查访问地址。</p>".to_owned(),
        ),
    )
}

fn internal_error() -> Response {
    html_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        page(
            "暂时无法完成请求",
            "<h1>暂时无法完成请求</h1><p class=\"lead\">请检查服务端日志后重试。</p>".to_owned(),
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

fn page(title: &str, content: String) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>{}</title>
  <style>
    :root {{ color-scheme: light dark; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 32px 16px; background: #0b1020; color: #e8edf8; }}
    main {{ width: min(680px, 100%); padding: 36px; border: 1px solid #26314d; border-radius: 18px; background: #121a2d; box-shadow: 0 24px 80px #0007; }}
    h1 {{ margin: 8px 0 12px; font-size: clamp(28px, 5vw, 42px); line-height: 1.12; }}
    .eyebrow {{ color: #8fb4ff; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }}
    .lead {{ color: #b9c5da; line-height: 1.7; }}
    form {{ display: grid; gap: 12px; margin-top: 24px; }}
    label {{ font-weight: 700; }}
    input, select {{ width: 100%; border: 1px solid #3a4869; border-radius: 10px; padding: 12px 14px; background: #0c1426; color: #f7f9ff; font: inherit; }}
    select {{ min-height: 260px; }}
    button {{ margin-top: 4px; border: 0; border-radius: 10px; padding: 13px 16px; background: #4c7dff; color: white; font: inherit; font-weight: 800; cursor: pointer; }}
    button:hover {{ background: #6791ff; }}
    button.danger {{ background: #c9405b; }}
    button.danger:hover {{ background: #df5871; }}
    .notice, .error {{ margin-top: 18px; border-radius: 10px; padding: 13px 15px; line-height: 1.55; }}
    .notice {{ border: 1px solid #345184; background: #182849; color: #c7d8ff; }}
    .error {{ border: 1px solid #873b4a; background: #381923; color: #ffc4cf; }}
    .hint {{ margin: 18px 0 0; color: #8997b2; font-size: 14px; }}
    code {{ padding: 2px 6px; border-radius: 6px; background: #091020; color: #b9d0ff; }}
    .success-mark {{ width: 54px; height: 54px; display: grid; place-items: center; border-radius: 50%; background: #1d714b; color: white; font-size: 30px; font-weight: 900; }}
    @media (max-width: 560px) {{ main {{ padding: 24px; }} }}
  </style>
</head>
<body><main>{content}</main></body>
</html>"#,
        escape_html(title),
    )
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
    let mut token = String::with_capacity(SETUP_TOKEN_BYTES * 2);
    for byte in bytes {
        _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            value => escaped.push(value),
        }
    }
    escaped
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}
