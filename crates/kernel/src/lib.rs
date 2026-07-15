//! 跨业务模块共享且不依赖 HTTP、SQLx 或具体应用宿主的稳定应用原语。
//!
//! API 请求与响应 DTO 不属于 kernel，应继续放在 `contracts` crate。业务模块只在确实共享
//! 语义时依赖这里的类型，避免把领域规则集中成万能公共层。

/// 提供请求关联信息等与传输协议无关的执行上下文。
pub mod context;
/// 提供跨业务模块复用的基础校验错误。
pub mod error;
/// 提供稳定标识值及其格式校验。
pub mod id;
/// 提供领域/application 层使用的分页值对象。
pub mod pagination;
/// 提供可替换的当前时间来源。
pub mod time;

pub use context::ExecutionContext;
pub use error::ValidationError;
pub use id::{InvalidRequestId, RequestId};
pub use pagination::{Page, PageRequest};
pub use time::{Clock, SystemClock};
