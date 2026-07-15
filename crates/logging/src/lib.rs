//! 服务端和桌面应用共享的日志初始化能力。
//!
//! 本 crate 只负责安装进程级 tracing subscriber 与格式化错误链，不依赖具体应用框架。

use std::error::Error;

use thiserror::Error;
use tracing_subscriber::EnvFilter;

/// 初始化进程级日志订阅器时返回的错误。
#[derive(Debug, Error)]
pub enum LoggingError {
    /// 进程中已经存在全局 subscriber，或 tracing 无法安装新的 subscriber。
    #[error("无法安装全局日志订阅器")]
    Initialize(
        /// tracing-subscriber 返回的底层初始化错误。
        #[source]
        Box<dyn Error + Send + Sync + 'static>,
    ),
}

/// 从 `RUST_LOG` 初始化向标准错误输出的格式化日志订阅器。
///
/// 环境变量缺失或内容无效时使用 `default_filter`。该函数使用 `try_init`，因此重复初始化
/// 不会 panic，而是返回 [`LoggingError`]。
///
/// # Errors
///
/// 进程已经安装其他全局 subscriber，或 tracing-subscriber 初始化失败时返回错误。
pub fn initialize(default_filter: &str) -> Result<(), LoggingError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(LoggingError::Initialize)
}

/// 将顶层错误及其所有 source 连接为适合日志输出的完整诊断文本。
pub fn format_error_chain(error: &(dyn Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        message.push_str(": ");
        let source_message = error.to_string();
        message.push_str(&source_message);
        source = error.source();
    }
    message
}
