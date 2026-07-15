//! 跨协议传播的稳定标识值。

use std::{fmt, str::FromStr};

use thiserror::Error;
use uuid::Uuid;

const MAX_REQUEST_ID_LENGTH: usize = 128;

/// 一次调用链中用于日志、错误响应和下游传播的请求关联 ID。
///
/// 该值允许接收符合安全字符集的上游 ID；没有上游值时可使用 [`RequestId::generate`] 创建
/// 带 `req_` 前缀的 UUID v7。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(String);

impl RequestId {
    /// 生成按时间大致有序且可以安全写入 HTTP header 的请求 ID。
    pub fn generate() -> Self {
        Self(format!("req_{}", Uuid::now_v7().simple()))
    }

    /// 校验并保留上游传入的请求 ID。
    ///
    /// # Errors
    ///
    /// 值为空、超过 128 字节或包含非字母数字、连字符、下划线、点号的字符时返回错误。
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidRequestId> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_REQUEST_ID_LENGTH
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        valid.then_some(Self(value)).ok_or(InvalidRequestId)
    }

    /// 返回适合写入日志、错误响应或 header 的字符串表示。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RequestId {
    type Err = InvalidRequestId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// 上游请求 ID 不符合安全传播约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("请求 ID 格式无效")]
pub struct InvalidRequestId;
