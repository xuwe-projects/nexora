//! 跨业务模块共享的基础错误值。

use thiserror::Error;

/// application 层输入没有满足字段约束时使用的结构化错误。
///
/// 具体业务错误可以透明包装该类型，再由 HTTP、CLI 或消息消费端映射成各自协议错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("字段 {field} 无效: {message}")]
pub struct ValidationError {
    field: &'static str,
    message: &'static str,
}

impl ValidationError {
    /// 使用稳定字段名和不包含敏感信息的说明创建校验错误。
    pub const fn new(field: &'static str, message: &'static str) -> Self {
        Self { field, message }
    }

    /// 返回发生校验失败的稳定字段名。
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// 返回适合映射到调用方错误响应的校验说明。
    pub const fn message(&self) -> &'static str {
        self.message
    }
}
