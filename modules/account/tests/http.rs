use std::sync::Arc;

use account::{
    Account,
    authentication::{AccessTokenVerifier, VerificationError, VerifiedIdentity},
};
use api::with_http_layers;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use contracts::error::ErrorEnvelope;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

#[tokio::test]
async fn protected_resource_rejects_missing_bearer_token_before_database_access() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:5432/test")
        .expect("惰性测试连接池配置应当有效");
    let router = with_http_layers(Account::new(pool, Arc::new(StaticVerifier)).routers::<()>());
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
}

struct StaticVerifier;

#[async_trait]
impl AccessTokenVerifier for StaticVerifier {
    async fn verify(&self, _token: &str) -> Result<VerifiedIdentity, VerificationError> {
        Ok(VerifiedIdentity {
            issuer: "https://id.example.com/".to_owned(),
            subject: "test-user".to_owned(),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
        })
    }
}
