//! Nexora 命令行入口。

#[path = "nexora/cli.rs"]
mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nexora: {error}");
            ExitCode::FAILURE
        }
    }
}
