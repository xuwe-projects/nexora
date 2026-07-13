//! 配置基础设施的结构化错误。

use std::path::PathBuf;

use thiserror::Error;

/// 配置读取、反序列化、路径定位或持久化阶段可能产生的错误。
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// `config-rs` 合并配置源或反序列化目标类型失败。
    #[error("配置加载失败: {0}")]
    Load(
        /// `config-rs` 返回的具体错误。
        #[from]
        config_rs::ConfigError,
    ),
    /// 用户配置无法序列化为 TOML。
    #[error("配置序列化失败: {0}")]
    Serialize(
        /// TOML 序列化器返回的具体错误。
        #[from]
        toml::ser::Error,
    ),
    /// 配置文件或目录操作失败。
    #[error("配置文件操作失败: {0}")]
    Io(
        /// 标准库返回的具体 I/O 错误。
        #[from]
        std::io::Error,
    ),
    /// 当前平台无法确定应用配置目录。
    #[error("无法确定应用 `{application}` 的系统配置目录")]
    ConfigDirectoryUnavailable {
        /// 调用方提供的应用名称。
        application: String,
    },
    /// 用户配置文件名包含目录或特殊路径组件。
    #[error("用户配置文件名 `{0}` 必须是单个普通文件名")]
    InvalidFileName(
        /// 调用方提供、但不满足单文件名约束的路径。
        PathBuf,
    ),
    /// 配置文件使用了当前程序尚不支持的 schema 版本。
    #[error("不支持配置 schema 版本 {actual}，当前版本为 {expected}")]
    UnsupportedSchema {
        /// 当前程序能够读取的 schema 版本。
        expected: u32,
        /// 配置文件实际声明的 schema 版本。
        actual: u32,
    },
}
