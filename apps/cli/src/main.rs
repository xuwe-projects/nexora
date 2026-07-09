//! `xuwecli` 命令行入口。

fn main() {
    if let Err(error) = xuwecli::run(std::env::args_os()) {
        eprintln!("xuwecli: {error}");
        std::process::exit(1);
    }
}
