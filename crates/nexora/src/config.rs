//! Nexora 应用的强类型配置加载与模块配置契约。
//!
//! 应用通过 `#[derive(nexora::Settings)]` 声明根配置类型，再调用
//! `nexora::config::initialize` 按“显式路径、首个用户位置参数、正式安装包冻结配置、开发配置”
//! 的优先级加载 TOML 文件；sidecar 注入的 updater 健康确认参数不参与路径选择。Account
//! 客户端与服务端配置段由派生宏分别标记，避免在同一个 workspace 中因 Cargo feature 合并
//! 而混淆两端配置。

use std::path::PathBuf;

pub use configuration::ConfigurationError;
use configuration::LayeredConfigLoader;
use serde::de::DeserializeOwned;
use thiserror::Error;

#[path = "config/path.rs"]
mod path;

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
    /// 定位或读取正式安装包发布元数据时，当前可执行文件位置、文件读取或校验失败。
    #[error(transparent)]
    ReleaseMetadata(
        /// updater 通用发布元数据加载器返回的结构化错误。
        #[from]
        updater::ReleaseMetadataError,
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
    /// 声明配置所属 package 的清单目录，用于从子目录启动时定位 workspace 配置。
    const MANIFEST_DIR: &'static str = ".";

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
/// 2. 忽略 updater 健康确认参数对后的首个用户位置参数；
/// 3. 当前可执行文件位置的合法 `nexora-release.json` 所标识资源目录中的
///    `config/<T::APP_NAME>.toml`；
/// 4. 当前目录或 package 清单目录祖先中的 `config/<T::APP_NAME>.toml`。
///
/// 第 3 项代表正式发布边界：一旦发现并校验通过 `nexora-release.json`，对应 bundle 配置
/// 就是唯一文件来源。文件缺失、不可读或 TOML 无效都会直接失败，不会回退到源码仓库。
/// macOS 资源目录为 `.app/Contents/Resources`，Windows 为主 EXE 同级目录。普通
/// `cargo run`/`cargo test` 没有发布元数据时才使用第 4 项开发回退。
///
/// 文件加载后，无前缀环境变量仍可覆盖同名字段；嵌套字段使用双下划线分隔，这一行为
/// 与 [`LayeredConfigLoader`] 保持一致。
///
/// # Examples
///
/// ```no_run
/// use serde::Deserialize;
///
/// # #[cfg(feature = "derive")]
/// # fn main() -> Result<(), nexora::config::ConfigError> {
/// #[derive(Deserialize, nexora::Settings)]
/// struct ApplicationSettings {
///     endpoint: String,
/// }
///
/// let settings: ApplicationSettings = nexora::config::initialize(None)?;
/// println!("{}", settings.endpoint);
/// # Ok(())
/// # }
/// # #[cfg(not(feature = "derive"))]
/// # fn main() {}
/// ```
///
/// # Errors
///
/// 当前可执行文件位置或正式发布元数据无效、选中的配置文件不存在、TOML 无效、环境变量
/// 无法转换、目标类型反序列化失败，或派生宏标记的框架模块配置段校验失败时返回
/// [`ConfigError`]。
pub fn initialize<T>(config_path: Option<PathBuf>) -> Result<T, ConfigError>
where
    T: Settings,
{
    let config_path = path::resolve_with_release_loader(
        config_path,
        std::env::args_os(),
        T::APP_NAME,
        T::MANIFEST_DIR,
        updater::load_current_release_metadata,
    )?;
    let settings = LayeredConfigLoader::<T>::new()
        .with_required_file(config_path)
        .load()?;
    settings.validate()?;
    Ok(settings)
}

/// 派生宏和可选业务模块之间共享的隐藏配置契约。
#[doc(hidden)]
pub mod __private {
    use std::{ffi::OsString, path::PathBuf};

    use super::{AccountClientSection, AccountServerSection, Settings};

    /// 从完整进程参数中提取可选配置文件路径，并忽略 updater 内部参数。
    ///
    /// 该函数仅供 Nexora 配置加载器和集成测试共享；sidecar 使用
    /// `--nexora-updater-health-session` 启动新版本时必须继续使用应用默认配置，而不能把内部
    /// 参数及其值误判为 TOML 路径。
    #[doc(hidden)]
    pub fn config_path_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
        super::path::from_args(args)
    }

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
