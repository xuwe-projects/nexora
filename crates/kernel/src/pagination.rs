//! 领域和 application 层共享的页码分页值对象。

use std::num::NonZeroU32;

/// 已经过 application 层边界校验的页码分页请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    number: NonZeroU32,
    size: NonZeroU32,
}

impl PageRequest {
    /// 尝试使用从一开始的页码和实际页大小创建分页请求。
    ///
    /// 页码或页大小为零时返回 `None`。最大页大小属于具体业务用例，应在调用前完成限制。
    pub const fn new(number: u32, size: u32) -> Option<Self> {
        let Some(number) = NonZeroU32::new(number) else {
            return None;
        };
        let Some(size) = NonZeroU32::new(size) else {
            return None;
        };
        Some(Self { number, size })
    }

    /// 返回从一开始的页码。
    pub const fn number(self) -> u32 {
        self.number.get()
    }

    /// 返回 application 层限制后的实际页大小。
    pub const fn size(self) -> u32 {
        self.size.get()
    }
}

/// application 层返回的通用分页结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    items: Vec<T>,
    total: i64,
    request: PageRequest,
}

impl<T> Page<T> {
    /// 使用当前页数据、总记录数和分页请求创建结果。
    pub fn new(items: Vec<T>, total: i64, request: PageRequest) -> Self {
        Self {
            items,
            total,
            request,
        }
    }

    /// 返回当前页的只读项目切片。
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// 返回当前筛选条件下的总记录数。
    pub const fn total(&self) -> i64 {
        self.total
    }

    /// 返回生成当前结果所使用的分页请求。
    pub const fn request(&self) -> PageRequest {
        self.request
    }

    /// 消费分页结果并返回项目、总数和分页请求，供协议边界执行显式映射。
    pub fn into_parts(self) -> (Vec<T>, i64, PageRequest) {
        (self.items, self.total, self.request)
    }
}
