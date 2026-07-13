//! 服务端程序入口。

mod config;

use std::{error::Error, path::PathBuf};

use clap::Parser;

use crate::config::{DEFAULT_CONFIG_PROFILE, ServerConfig};

/// 服务端启动参数。
#[derive(Debug, Parser)]
#[command(name = "server", version, about = "启动 API 服务")]
struct Arguments {
    /// 指定服务端 TOML 配置文件；文件配置会被同名环境变量覆盖。
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// 指定 `config/<profile>.toml` 配置名称；未传入时默认加载本地配置。
    #[arg(long, default_value = DEFAULT_CONFIG_PROFILE, value_name = "NAME")]
    profile: String,
}

/// 启动服务端程序。
///
/// 当前入口完成启动参数和分层配置加载；后续 Axum 服务应使用这里得到的类型化配置创建监听器。
///
/// # Errors
///
/// 命令行参数、必需配置文件、环境变量或类型化反序列化无效时返回错误并终止启动。
fn main() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse();
    let config = match arguments.config.as_deref() {
        Some(path) => ServerConfig::load_file(path)?,
        None => ServerConfig::load_profile(&arguments.profile)?,
    };

    println!("服务端配置已加载：{}", config.bind_address());
    Ok(())
}
