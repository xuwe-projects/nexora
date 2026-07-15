//! 不分页集合资源共享的 HTTP 响应契约。

use serde::{Deserialize, Serialize};

/// 简单集合资源的统一响应包装。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ItemsResponse<T> {
    /// 当前响应返回的资源集合。
    pub items: Vec<T>,
}
