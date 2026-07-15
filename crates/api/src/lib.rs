//! 业务模块共享的 Axum HTTP 协议基础设施。
//!
//! 本 crate 只提供统一错误、请求 extractor、请求 ID、trace 与安全中间件，不持有业务
//! State，也不声明业务路由。各业务模块负责自己的 handler、Router 和数据库行为。

mod error;
mod extract;
mod request_id;
mod router;

pub use error::ApiError;
pub use extract::{ApiJson, ApiPath, ApiQuery};
pub use router::with_http_layers;
