//! 将 Axum 请求提取失败统一映射为 API 错误格式。
//!
//! 正文大小由路由层的 `DefaultBodyLimit` 统一限制，本模块只负责反序列化和拒绝映射。

use axum::extract::{FromRequest, FromRequestParts, Json, Path, Query, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::ApiError;

/// 将 Axum JSON rejection 映射为统一错误格式的正文 extractor。
pub struct ApiJson<T>(
    /// 已成功反序列化的 JSON 请求值。
    pub T,
);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    // nexora-lint: allow(nexora::raw_axum_request) reason="自定义 JSON extractor 必须接收请求后委托 Axum Json 完成正文解析"
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| {
                ApiError::new(rejection.status(), "invalid_json_body", "JSON 请求正文无效")
            })
    }
}

/// 将 Axum 路径参数 rejection 映射为统一错误格式的 extractor。
pub struct ApiPath<T>(
    /// 已成功反序列化的路径参数值。
    pub T,
);

impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|rejection| {
                ApiError::new(rejection.status(), "invalid_path_parameter", "路径参数无效")
            })
    }
}

/// 将 Axum 查询参数 rejection 映射为统一错误格式的 extractor。
pub struct ApiQuery<T>(
    /// 已成功反序列化的查询参数值。
    pub T,
);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|rejection| {
                ApiError::new(
                    rejection.status(),
                    "invalid_query_parameter",
                    "查询参数无效",
                )
            })
    }
}
