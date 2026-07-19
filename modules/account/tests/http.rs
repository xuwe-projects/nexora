use std::sync::Arc;

use account::{
    Account, AccountDependencies, AccountError, ExternalIdentity,
    authentication::{
        AccessTokenVerifier, OidcResourceServer, VerificationError, VerifiedBearerIdentity,
        VerifiedIdentity,
    },
};
use api::{ApiError, with_http_layers};
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::FromRef,
    http::{Request, StatusCode},
    response::IntoResponse as _,
    routing::get,
};
use contracts::error::ErrorEnvelope;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

#[tokio::test]
async fn protected_resource_rejects_missing_bearer_token_before_database_access() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:5432/test")
        .expect("惰性测试连接池配置应当有效");
    let account = Account::new(AccountDependencies {
        pool,
        token_verifier: Arc::new(StaticVerifier),
        identity_directory: None,
    });
    let router = with_http_layers(account.routers::<()>());
    let reusable_router = with_http_layers(account.routers::<()>());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/me")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["www-authenticate"], "Bearer");
    let response_request_id = response.headers()["x-request-id"]
        .to_str()
        .expect("请求 ID 响应头应当是 ASCII")
        .to_owned();
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应当符合公共错误契约");
    assert_eq!(error.error.code, "missing_access_token");
    assert_eq!(error.error.request_id, response_request_id);

    let reusable_response = reusable_router
        .oneshot(
            Request::builder()
                .uri("/me")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("同一个 Account 应当可以再次构建路由");
    assert_eq!(reusable_response.status(), StatusCode::UNAUTHORIZED);

    let invalid_identity = account
        .provision_user(ExternalIdentity {
            identity_id: " ".to_owned(),
            username: None,
            email: None,
            display_name: "测试用户".to_owned(),
            avatar_url: None,
        })
        .await
        .expect_err("应用层不能开通缺少 identity ID 的身份");
    assert!(matches!(invalid_identity, AccountError::InvalidInput(_)));
}

#[tokio::test]
async fn deployment_issuer_mismatch_is_a_stable_authentication_failure() {
    let response = ApiError::from(AccountError::IdentityIssuerMismatch).into_response();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["www-authenticate"], "Bearer");
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("认证错误响应应当可以读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应符合公共契约");
    assert_eq!(error.error.code, "invalid_identity_issuer");
}

#[tokio::test]
async fn resource_server_authentication_error_does_not_echo_access_token() {
    let state = PortalState {
        resource_server: OidcResourceServer::new(Arc::new(RejectingVerifier)),
    };
    let router = with_http_layers(
        Router::new()
            .route("/portal", get(portal_handler))
            .with_state(state),
    );
    let response = router
        .oneshot(
            Request::builder()
                .uri("/portal")
                .header("authorization", "Bearer secret-portal-token")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let body = String::from_utf8(body.to_vec()).expect("错误响应应为 UTF-8 JSON");
    assert!(!body.contains("secret-portal-token"));
    let error: ErrorEnvelope = serde_json::from_str(body.as_str()).expect("错误响应应符合公共契约");
    assert_eq!(error.error.code, "invalid_access_token");
}

#[derive(Clone)]
struct PortalState {
    resource_server: OidcResourceServer,
}

impl FromRef<PortalState> for OidcResourceServer {
    fn from_ref(state: &PortalState) -> Self {
        state.resource_server.clone()
    }
}

async fn portal_handler(_identity: VerifiedBearerIdentity) -> StatusCode {
    StatusCode::OK
}

struct RejectingVerifier;

#[async_trait]
impl AccessTokenVerifier for RejectingVerifier {
    async fn verify(&self, _token: &str) -> Result<VerifiedIdentity, VerificationError> {
        Err(VerificationError::InvalidToken)
    }
}

struct StaticVerifier;

#[async_trait]
impl AccessTokenVerifier for StaticVerifier {
    async fn verify(&self, _token: &str) -> Result<VerifiedIdentity, VerificationError> {
        Ok(VerifiedIdentity {
            issuer: "https://id.example.com/".to_owned(),
            subject: "test-user".to_owned(),
            username: Some("test-user".to_owned()),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
            organization: None,
        })
    }
}
