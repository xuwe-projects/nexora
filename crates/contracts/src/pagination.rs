//! HTTP API 共享的页码分页请求与响应契约。

use serde::{Deserialize, Serialize};

/// 后台集合资源使用的页码分页查询参数。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PageQuery {
    /// 从一开始的页码。
    pub page: u32,
    /// 单页期望返回的资源数量；服务端会应用具体业务的最大值限制。
    pub page_size: u32,
}

impl Default for PageQuery {
    /// 使用第一页和每页二十五条记录创建默认分页参数。
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 25,
        }
    }
}

/// 页码分页集合的统一响应包装。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PageResponse<T> {
    /// 当前页返回的资源集合。
    pub items: Vec<T>,
    /// 当前页码、页大小和总记录数。
    pub page: PageMetadata,
}

/// 页码分页响应的元数据。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct PageMetadata {
    /// 从一开始的当前页码。
    pub number: u32,
    /// 服务端实际使用的页大小。
    pub size: u32,
    /// 当前筛选条件下的总记录数。
    pub total: i64,
}
