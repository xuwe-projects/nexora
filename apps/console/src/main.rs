//! 控制台桌面应用入口。

mod app;
mod auth;
mod config;
mod features;

use app::Console;
use desktop::Application as _;

/// 启动控制台桌面应用。
///
/// 入口函数先安装共享日志订阅器，再创建控制台应用并交给统一桌面运行器执行。
///
/// # Errors
///
/// 进程无法安装全局日志订阅器时返回错误。
fn main() -> Result<(), logging::LoggingError> {
    logging::initialize("info")?;
    tracing::info!("控制台桌面应用启动");
    Console::new().run();
    Ok(())
}
