//! API 服务启动配置。

use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use configuration::{ConfigurationError, LayeredConfigLoader};
use serde::Deserialize;
use thiserror::Error;
use url::{Host, Url};

/// 返回由当前 Cargo 包名推导出的默认配置文件路径。
pub fn default_config_file() -> PathBuf {
    PathBuf::from("config").join(format!("{}.toml", env!("CARGO_PKG_NAME")))
}

/// API 服务完整运行时配置。
///
/// 默认启动会读取调用目录下的 `config/server.toml`。
/// 环境变量通过双下划线表达层级，例如 `SERVER__HOST=0.0.0.0` 和
/// `SERVER__PORT=8080`。
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    /// HTTP 监听相关配置。
    pub server: ServerSettings,
    /// PostgreSQL 连接池配置。
    pub database: DatabaseSettings,
    /// OIDC resource server 配置。
    pub oidc: OidcSettings,
    /// 一次性系统初始化页面配置。
    pub setup: SetupSettings,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerConfig")
            .field("server", &self.server)
            .field("database", &self.database)
            .field("oidc", &self.oidc)
            .field("setup", &self.setup)
            .finish()
    }
}

impl ServerConfig {
    /// 加载指定 TOML 文件，并使用无统一前缀的环境变量覆盖同名字段。
    ///
    /// 该方法适合生产部署、测试和命令行显式指定配置文件的场景。与配置名称加载不同，指定文件
    /// 必须存在，避免拼错路径时静默回退到默认配置。
    ///
    /// # Errors
    ///
    /// 指定文件不存在、TOML 无效、环境变量类型不匹配、最终配置无法反序列化或配置语义无效时
    /// 返回错误。
    pub fn load_file(path: &Path) -> Result<Self, ServerConfigError> {
        let config = config_loader().with_required_file(path).load()?;
        Self::prepare(config)
    }

    /// 检查服务监听、PostgreSQL 与 OIDC 配置是否满足运行约束。
    ///
    /// 该检查不连接数据库或身份提供方，因此可用于命令行 `--check-config` 和部署探针。
    ///
    /// # Errors
    ///
    /// 端口、数据库连接 URL、OIDC issuer、audience、PAT 或 setup secret 无效时返回错误。
    pub fn validate(&self) -> Result<(), ServerConfigError> {
        if self.server.port == 0 {
            return Err(ServerConfigError::Invalid("server.port 不能为 0"));
        }
        validate_database(&self.database)?;
        validate_oidc(&self.oidc)?;
        validate_setup(&self.setup)?;
        Ok(())
    }

    /// 返回可直接交给 Tokio TCP listener 的监听地址。
    pub fn bind_address(&self) -> SocketAddr {
        SocketAddr::new(self.server.host, self.server.port)
    }

    fn prepare(config: Self) -> Result<Self, ServerConfigError> {
        config.validate()?;
        Ok(config)
    }
}

/// 服务端配置加载或语义校验错误。
#[derive(Debug, Error)]
pub enum ServerConfigError {
    /// 配置来源读取、合并或反序列化失败。
    #[error(transparent)]
    Load(
        /// 底层配置加载器返回并保留 source chain 的具体错误。
        #[from]
        ConfigurationError,
    ),
    /// 最终配置不满足服务端运行约束。
    #[error("服务端配置无效: {0}")]
    Invalid(
        /// 不包含数据库凭据或访问令牌的错误说明。
        &'static str,
    ),
}

impl ServerConfigError {
    /// 返回不会包含配置源码行或秘密值的启动错误说明。
    pub(crate) fn safe_diagnostic(&self) -> String {
        match self {
            Self::Load(error) => error.safe_diagnostic(),
            Self::Invalid(message) => format!("服务端配置无效: {message}"),
        }
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

/// API 服务 PostgreSQL 连接池配置。
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DatabaseSettings {
    /// PostgreSQL 连接 URL。
    ///
    /// 生产环境应通过 `DATABASE__URL` 或部署平台密钥注入，不应提交真实密码。
    pub url: String,
    /// SQLx 连接池最大连接数。
    pub max_connections: u32,
}

impl fmt::Debug for DatabaseSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseSettings")
            .field("url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

impl Default for DatabaseSettings {
    /// 创建本地开发数据库默认配置。
    fn default() -> Self {
        Self {
            url: "postgres://postgres:postgres@127.0.0.1:5432/xuwe".to_owned(),
            max_connections: 10,
        }
    }
}

/// API 服务验证 Bearer access token 所需的 OIDC 配置。
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OidcSettings {
    /// 身份提供方的 issuer URL。
    pub issuer_url: String,
    /// Access token 必须包含的 API audience。
    pub audience: String,
    /// 保存并签发本系统角色的认证授权 Project ID。
    ///
    /// 该值与可能是 API Client ID 的 `audience` 含义不同，初始化时作为
    /// ProjectService v2 gRPC 请求的 `project_id`。
    pub project_id: String,
    /// 服务端调用 ZITADEL Management API 使用的服务账号 Personal Access Token。
    ///
    /// 生产环境应通过 `OIDC__PERSONAL_ACCESS_TOKEN` 或部署平台密钥注入，不应提交真实令牌。
    personal_access_token: String,
}

impl fmt::Debug for OidcSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OidcSettings")
            .field("issuer_url", &self.issuer_url)
            .field("audience", &self.audience)
            .field("project_id", &self.project_id)
            .field("personal_access_token", &"[REDACTED]")
            .finish()
    }
}

impl Default for OidcSettings {
    /// 创建不绑定具体部署的 OIDC 示例配置；PAT 仍必须由部署显式提供。
    fn default() -> Self {
        Self {
            issuer_url: "https://id.example.com".to_owned(),
            audience: "xuwe-api".to_owned(),
            project_id: String::new(),
            personal_access_token: String::new(),
        }
    }
}

/// 一次性系统初始化页面配置。
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SetupSettings {
    /// 进入 `/setup` 超级管理员选择步骤前必须提交的密钥。
    ///
    /// 生产环境应通过 `SETUP__SECRET` 或部署平台密钥注入。
    secret: String,
}

impl fmt::Debug for SetupSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SetupSettings")
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl Default for SetupSettings {
    /// 创建不包含任何部署密钥的默认配置。
    fn default() -> Self {
        Self {
            secret: String::new(),
        }
    }
}

impl SetupSettings {
    /// 返回进入一次性初始化流程的密钥；调用方不得记录该值。
    pub(crate) fn secret(&self) -> &str {
        self.secret.as_str()
    }
}

impl OidcSettings {
    /// 返回调用 ZITADEL UserService 与 ProjectService v2 gRPC 使用的 PAT；调用方不得记录该密钥。
    pub(crate) fn personal_access_token(&self) -> &str {
        self.personal_access_token.trim()
    }

    /// 返回初始化角色所属的认证授权 Project ID。
    pub(crate) fn project_id(&self) -> &str {
        self.project_id.trim()
    }
}

fn config_loader() -> LayeredConfigLoader<ServerConfig> {
    LayeredConfigLoader::new()
}

fn validate_database(database: &DatabaseSettings) -> Result<(), ServerConfigError> {
    if database.max_connections == 0 {
        return Err(ServerConfigError::Invalid(
            "database.max_connections 必须大于 0",
        ));
    }
    let url = Url::parse(database.url.trim())
        .map_err(|_| ServerConfigError::Invalid("database.url 不是有效 URL"))?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(ServerConfigError::Invalid(
            "database.url 必须使用 postgres 或 postgresql scheme",
        ));
    }
    if url.host().is_none() {
        return Err(ServerConfigError::Invalid("database.url 必须包含主机"));
    }
    if url.path().trim_matches('/').is_empty() {
        return Err(ServerConfigError::Invalid(
            "database.url 必须包含数据库名称",
        ));
    }
    Ok(())
}

fn validate_oidc(oidc: &OidcSettings) -> Result<(), ServerConfigError> {
    if oidc.audience.trim().is_empty() {
        return Err(ServerConfigError::Invalid("oidc.audience 不能为空"));
    }
    if oidc.project_id().is_empty() {
        return Err(ServerConfigError::Invalid("oidc.project_id 不能为空"));
    }
    if oidc.personal_access_token().is_empty() {
        return Err(ServerConfigError::Invalid(
            "oidc.personal_access_token 不能为空",
        ));
    }
    let issuer = Url::parse(oidc.issuer_url.trim())
        .map_err(|_| ServerConfigError::Invalid("oidc.issuer_url 不是有效 URL"))?;
    if issuer.host().is_none() {
        return Err(ServerConfigError::Invalid("oidc.issuer_url 必须包含主机"));
    }
    if !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
    {
        return Err(ServerConfigError::Invalid(
            "oidc.issuer_url 不能包含凭据、query 或 fragment",
        ));
    }
    if issuer.scheme() != "https" && !(issuer.scheme() == "http" && is_loopback(&issuer)) {
        return Err(ServerConfigError::Invalid(
            "oidc.issuer_url 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP",
        ));
    }
    Ok(())
}

fn validate_setup(setup: &SetupSettings) -> Result<(), ServerConfigError> {
    if setup.secret.trim().is_empty() {
        return Err(ServerConfigError::Invalid("setup.secret 不能为空"));
    }
    if setup.secret.len() > 1_024 {
        return Err(ServerConfigError::Invalid(
            "setup.secret 长度不能超过 1024 字节",
        ));
    }
    Ok(())
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}
