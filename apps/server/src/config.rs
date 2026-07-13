//! API 服务启动配置。

use std::{net::SocketAddr, path::Path};

use configuration::{ConfigurationError, LayeredConfigLoader};
use serde::Deserialize;

/// 服务端默认配置目录。
///
/// 当命令行没有通过 `--config` 指定精确配置文件时，服务端会按照配置名称从该目录加载
/// `<profile>.toml`，例如默认的 `local` 会对应 `config/local.toml`。
pub const DEFAULT_CONFIG_DIRECTORY: &str = "config";

/// 服务端本地开发默认配置名称。
///
/// 该名称用于无参数本地启动场景，使 `cargo run -p server` 能直接读取
/// `config/local.toml` 中的示例配置。
pub const DEFAULT_CONFIG_PROFILE: &str = "local";

/// API 服务完整运行时配置。
///
/// 缺少配置文件时使用代码默认值；默认本地启动会读取 `config/local.toml`。
/// 环境变量通过双下划线表达层级，例如 `SERVER__HOST=0.0.0.0` 和
/// `SERVER__PORT=8080`。
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    /// HTTP 监听相关配置。
    pub server: ServerSettings,
}

impl ServerConfig {
    /// 按配置名称加载 `config/<profile>.toml`，并使用无统一前缀的环境变量覆盖同名字段。
    ///
    /// 该方法适合应用默认启动路径。配置文件允许不存在，以便测试、容器或极简部署继续依赖
    /// 代码默认值与环境变量启动。
    ///
    /// # Errors
    ///
    /// TOML 无效、环境变量类型不匹配或最终配置无法反序列化时返回错误。
    pub fn load_profile(profile: &str) -> Result<Self, ConfigurationError> {
        let loader = LayeredConfigLoader::new();
        loader.with_optional_file(profile_path(profile)).load()
    }

    /// 加载指定 TOML 文件，并使用无统一前缀的环境变量覆盖同名字段。
    ///
    /// 该方法适合生产部署、测试和命令行显式指定配置文件的场景。与配置名称加载不同，指定文件
    /// 必须存在，避免拼错路径时静默回退到默认配置。
    ///
    /// # Errors
    ///
    /// 指定文件不存在、TOML 无效、环境变量类型不匹配或最终配置无法反序列化时返回错误。
    pub fn load_file(path: &Path) -> Result<Self, ConfigurationError> {
        LayeredConfigLoader::new().with_required_file(path).load()
    }

    /// 返回可直接交给 Tokio TCP listener 的监听地址。
    pub fn bind_address(&self) -> SocketAddr {
        SocketAddr::new(self.server.host, self.server.port)
    }
}

/// API 服务 HTTP 监听配置。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerSettings {
    /// HTTP 服务监听 IP；默认只接受本机连接。
    pub host: std::net::IpAddr,
    /// HTTP 服务监听端口。
    pub port: u16,
}

impl Default for ServerSettings {
    /// 创建默认监听配置 `127.0.0.1:3000`。
    fn default() -> Self {
        Self {
            host: std::net::Ipv4Addr::LOCALHOST.into(),
            port: 3000,
        }
    }
}

fn profile_path(profile: &str) -> std::path::PathBuf {
    Path::new(DEFAULT_CONFIG_DIRECTORY).join(format!("{profile}.toml"))
}
