//! 应用配置加载、跨平台路径定位与用户配置持久化能力。
//!
//! 服务端配置可以通过 [`LayeredConfigLoader`] 按“文件、无前缀环境变量”的顺序合并；
//! 桌面用户偏好通过 [`UserConfigStore`] 保存到操作系统约定的应用配置目录。

mod error;
mod loader;
mod store;

pub use error::ConfigurationError;
pub use loader::LayeredConfigLoader;
pub use store::{UserConfigStore, VersionedConfiguration};
