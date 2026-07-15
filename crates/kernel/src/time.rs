//! 可替换的当前时间来源。

use chrono::{DateTime, Utc};

/// application 和领域代码读取当前 UTC 时间的最小端口。
///
/// 需要确定性时间的测试可以提供固定实现，生产环境使用 [`SystemClock`]。
pub trait Clock: Send + Sync {
    /// 返回当前 UTC 时间。
    fn now(&self) -> DateTime<Utc>;
}

/// 从操作系统时钟读取当前 UTC 时间的生产实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
