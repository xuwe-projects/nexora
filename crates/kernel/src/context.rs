//! 与传输协议无关的调用执行上下文。

use chrono::{DateTime, Utc};

use crate::{Clock, RequestId};

/// 在一次调用链中传播的基础执行上下文。
///
/// 当前只包含关联 ID 和开始时间；用户、租户与权限仍由各业务模块定义，避免 kernel 依赖
/// accounts 或某一种认证模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    request_id: RequestId,
    started_at: DateTime<Utc>,
}

impl ExecutionContext {
    /// 使用指定时钟记录一次调用的开始上下文。
    pub fn start(request_id: RequestId, clock: &(impl Clock + ?Sized)) -> Self {
        Self {
            request_id,
            started_at: clock.now(),
        }
    }

    /// 使用已知开始时间恢复一个调用上下文。
    pub const fn new(request_id: RequestId, started_at: DateTime<Utc>) -> Self {
        Self {
            request_id,
            started_at,
        }
    }

    /// 返回当前调用链使用的请求关联 ID。
    pub const fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    /// 返回当前调用开始时记录的 UTC 时间。
    pub const fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }
}
