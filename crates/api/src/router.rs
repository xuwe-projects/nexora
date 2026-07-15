//! API 路由组合与通用 HTTP 中间件。

use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, MatchedPath},
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware,
};
use tower_http::{
    LatencyUnit,
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::{DefaultOnResponse, TraceLayer},
};

use crate::{ApiError, request_id};

const MAX_JSON_BODY_BYTES: usize = 64 * 1024;

/// 为已组合的业务路由挂载 fallback、请求 ID、trace、敏感头保护与正文上限。
pub fn with_http_layers<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(http_request_span)
                .on_response(
                    DefaultOnResponse::new()
                        .level(tracing::Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                ),
        )
        .layer(SetSensitiveRequestHeadersLayer::new([AUTHORIZATION]))
        .layer(middleware::from_fn(request_id::assign))
}

fn http_request_span(request: &Request<Body>) -> tracing::Span {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    let matched_path = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched");
    tracing::info_span!(
        "http_request",
        method = %request.method(),
        path = %request.uri().path(),
        matched_path = %matched_path,
        request_id = %request_id,
    )
}

async fn route_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "route_not_found", "接口不存在")
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "HTTP 方法不受支持",
    )
}
