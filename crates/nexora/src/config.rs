//! Nexora 应用的强类型配置加载与模块配置契约。
//!
//! 应用通过 `#[derive(nexora::Settings)]` 声明根配置类型，再调用 [`initialize`] 按
//! “显式路径、首个命令行参数、包名默认路径”的优先级加载 TOML 文件。Account 客户端
//! 与服务端配置段由派生宏分别标记，避免在同一个 workspace 中因 Cargo feature 合并而
//! 混淆两端配置。

use std::path::PathBuf;

pub use configuration::ConfigurationError;
use configuration::LayeredConfigLoader;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Nexora 根配置加载或配置段校验失败时返回的错误。
#[derive(Debug, Error)]
pub enum ConfigError {
    /// 配置文件读取、来源合并或目标类型反序列化失败。
    #[error(transparent)]
    Load(
        /// 底层通用配置加载器返回的结构化错误。
        #[from]
        ConfigurationError,
    ),
    /// 已反序列化的模块配置不满足运行约束。
    #[error("配置段 `{section}` 无效: {message}")]
    InvalidSection {
        /// 校验失败的稳定配置段名称，例如 `account.client`。
        section: &'static str,
        /// 不应包含令牌、密码等秘密值的失败说明。
        message: String,
    },
}

impl ConfigError {
    /// 创建一个不包含秘密值的配置段校验错误。
    ///
    /// `section` 应使用稳定的点分名称，便于日志和命令行定位；`message` 只能描述失败
    /// 约束，不应拼接数据库密码、访问令牌或其他配置原值。
    pub fn invalid_section(section: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidSection {
            section,
            message: message.into(),
        }
    }
}

/// 可由 Nexora 统一加载的应用根配置。
///
/// 应用通常不需要手写此 trait，而是对可反序列化的根配置使用
/// `#[derive(nexora::Settings)]`。派生宏会在调用方 crate 中计算 [`Self::APP_NAME`]，
/// 因而默认文件名对应实际应用包名，而不是 `nexora`。
pub trait Settings: DeserializeOwned {
    /// 声明配置所属的 Cargo 包名，用于生成默认配置文件路径。
    const APP_NAME: &'static str;

    /// 校验框架模块关心的配置段。
    ///
    /// 派生宏会调用标记为 `account_client` 或 `account_server` 的字段校验。未启用这些
    /// 模块时，该方法直接成功；应用自己的额外业务配置可以在加载后继续执行专属校验。
    ///
    /// # Errors
    ///
    /// 任一框架模块配置段不满足运行约束时返回 [`ConfigError`]。
    fn validate(&self) -> Result<(), ConfigError>;
}

/// Account 桌面客户端配置段需要提供的内部校验契约。
///
/// 该契约由 Nexora 的 Account 客户端具体配置类型实现。应用只需把对应字段标记为
/// `#[nexora(account_client)]`，无需自行实现此 trait。
#[doc(hidden)]
pub trait AccountClientSection {
    /// 校验 OIDC 客户端、回调地址等桌面登录配置。
    ///
    /// # Errors
    ///
    /// 配置无法用于建立 Account 客户端时返回 [`ConfigError`]。
    fn validate_account_client(&self) -> Result<(), ConfigError>;
}

/// Account 服务端配置段需要提供的内部校验契约。
///
/// 该契约由 Nexora 的 Account 服务端具体配置类型实现。应用只需把对应字段标记为
/// `#[nexora(account_server)]`，无需自行实现此 trait。
#[doc(hidden)]
pub trait AccountServerSection {
    /// 校验 OIDC issuer、audience 等资源服务器配置。
    ///
    /// # Errors
    ///
    /// 配置无法用于建立 Account 服务端时返回 [`ConfigError`]。
    fn validate_account_server(&self) -> Result<(), ConfigError>;
}

/// 加载并校验调用方声明的 Nexora 根配置。
///
/// 配置文件按以下优先级选择：
///
/// 1. `config_path` 显式传入的路径；
/// 2. 当前进程第一个命令行参数；
/// 3. `config/<T::APP_NAME>.toml`。
///
/// 文件加载后，无前缀环境变量仍可覆盖同名字段；嵌套字段使用双下划线分隔，这一行为
/// 与 [`LayeredConfigLoader`] 保持一致。
///
/// # Examples
///
/// ```no_run
/// use serde::Deserialize;
///
/// #[derive(Deserialize, nexora::Settings)]
/// struct ApplicationSettings {
///     endpoint: String,
/// }
///
/// let settings: ApplicationSettings = nexora::config::initialize(None)?;
/// println!("{}", settings.endpoint);
/// # Ok::<(), nexora::config::ConfigError>(())
/// ```
///
/// # Errors
///
/// 选中的配置文件不存在、TOML 无效、环境变量无法转换、目标类型反序列化失败，或派生宏
/// 标记的框架模块配置段校验失败时返回 [`ConfigError`]。
pub fn initialize<T>(config_path: Option<PathBuf>) -> Result<T, ConfigError>
where
    T: Settings,
{
    let config_path = config_path
        .or_else(|| std::env::args_os().nth(1).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("config").join(format!("{}.toml", T::APP_NAME)));
    let settings = LayeredConfigLoader::<T>::new()
        .with_required_file(config_path)
        .load()?;
    settings.validate()?;
    Ok(settings)
}

/// 派生宏和可选业务模块之间共享的隐藏配置契约。
#[doc(hidden)]
pub mod __private {
    use super::{AccountClientSection, AccountServerSection, Settings};

    /// 表示根配置包含一个 Account 桌面客户端配置段。
    pub trait ProvidesAccountClientSettings: Settings {
        /// 派生宏标记的 Account 桌面客户端配置具体类型。
        type AccountClientSettings: AccountClientSection;

        /// 返回 Account 桌面客户端初始化所需的配置段。
        fn account_client_settings(&self) -> &Self::AccountClientSettings;
    }

    /// 表示根配置包含一个 Account 服务端配置段。
    pub trait ProvidesAccountServerSettings: Settings {
        /// 派生宏标记的 Account 服务端配置具体类型。
        type AccountServerSettings: AccountServerSection;

        /// 返回 Account 服务端依赖装配所需的配置段。
        fn account_server_settings(&self) -> &Self::AccountServerSettings;
    }
}
