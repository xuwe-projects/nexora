//! 为每个请求建立唯一且可传播的关联 ID。
//!
//! 路由层已经使用 `DefaultBodyLimit` 限制正文；本模块作为 middleware 只传递请求，不解析正文。

use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use kernel::{ExecutionContext, RequestId, SystemClock};

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static CURRENT_CONTEXT: ExecutionContext;
}

// xuwe-lint: allow(xuwe::raw_axum_request) reason="请求 ID middleware 必须包装完整请求并把同一 ID 传播到响应"
pub(crate) async fn assign(mut request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| RequestId::parse(value).ok())
        .unwrap_or_else(RequestId::generate);
    let header_value = HeaderValue::from_str(request_id.as_str())
        .unwrap_or_else(|_| HeaderValue::from_static("req_invalid"));
    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), header_value.clone());

    let context = ExecutionContext::start(request_id, &SystemClock);
    let mut response = CURRENT_CONTEXT.scope(context, next.run(request)).await;
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), header_value);
    response
}

pub(crate) fn current() -> String {
    CURRENT_CONTEXT
        .try_with(|context| context.request_id().to_string())
        .unwrap_or_else(|_| RequestId::generate().to_string())
}
