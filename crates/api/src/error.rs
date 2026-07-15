//! API 统一错误响应。

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
    response::{IntoResponse, Response},
};
use contracts::error::{ErrorBody, ErrorEnvelope};
use serde_json::Value;

use crate::request_id;

/// HTTP handler、extractor 与业务模块共享的统一错误。
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Value,
}

impl ApiError {
    /// 创建不向客户端泄露内部实现细节的服务不可用错误。
    pub fn service_unavailable(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    /// 创建符合 Bearer 认证约定的未认证错误。
    pub fn unauthorized(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    /// 使用 HTTP 状态、稳定错误码和用户可读消息创建错误。
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: Value::Object(Default::default()),
        }
    }

    /// 为错误附加不包含敏感实现信息的结构化详情。
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = request_id::current();
        let status = self.status;
        let response = ErrorEnvelope {
            error: ErrorBody {
                code: self.code.to_owned(),
                message: self.message,
                details: self.details,
                request_id,
            },
        };
        let mut response = (status, Json(response)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}
