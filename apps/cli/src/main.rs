//! `xuwecli` 命令行入口。

/// 团队 CLI 的命令定义与执行流程。
pub mod commands;

fn main() {
    if let Err(error) = commands::run(std::env::args_os()) {
        eprintln!("xuwecli: {error}");
        std::process::exit(1);
    }
}
