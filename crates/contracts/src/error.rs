//! API 失败响应的稳定机器可读契约。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 所有 API 失败状态码使用的顶层响应包装。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ErrorEnvelope {
    /// 具体的错误码、消息、上下文和请求追踪标识。
    pub error: ErrorBody,
}

/// SDK 可以稳定解析的 API 错误正文。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ErrorBody {
    /// 供程序分支判断使用的稳定错误码。
    pub code: String,
    /// 面向调用方展示且不包含内部敏感信息的错误消息。
    pub message: String,
    /// 与具体错误有关的结构化补充信息；没有详情时为空对象。
    pub details: Value,
    /// 服务端生成或透传的请求追踪 ID。
    pub request_id: String,
}
