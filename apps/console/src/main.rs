//! 控制台桌面应用入口。

mod app;
mod auth;
mod config;
mod features;

use app::Console;
use desktop::Application as _;

/// 启动控制台桌面应用。
///
/// 入口函数创建控制台应用实例，并交给统一桌面运行器执行。
fn main() {
    Console::new().run();
}
