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

impl ConfigurationError {
    /// 返回适合写入日志或终端的脱敏错误说明。
    ///
    /// `config-rs` 的 TOML 语法错误可能在默认展示中附带完整源码行；该方法只保留错误类别、
    /// 配置来源和字段名，不展示原始值，避免数据库 URL、访问令牌等配置秘密进入日志。
    pub fn safe_diagnostic(&self) -> String {
        match self {
            Self::Load(error) => format!("配置加载失败: {}", safe_config_error(error, None, None)),
            Self::Serialize(_) => "配置序列化失败".to_owned(),
            Self::Io(error) => format!("配置文件操作失败: {error}"),
            Self::ConfigDirectoryUnavailable { application } => {
                format!("无法确定应用 `{application}` 的系统配置目录")
            }
            Self::InvalidFileName(path) => {
                format!("用户配置文件名 `{}` 必须是单个普通文件名", path.display())
            }
            Self::UnsupportedSchema { expected, actual } => {
                format!("不支持配置 schema 版本 {actual}，当前版本为 {expected}")
            }
        }
    }
}

fn safe_config_error(
    error: &config_rs::ConfigError,
    inherited_origin: Option<&str>,
    inherited_key: Option<&str>,
) -> String {
    match error {
        config_rs::ConfigError::Frozen => "配置已经冻结".to_owned(),
        config_rs::ConfigError::NotFound(key) => format!("缺少字段 `{key}`"),
        config_rs::ConfigError::PathParse { .. } => "配置字段路径无效".to_owned(),
        config_rs::ConfigError::FileParse { uri, .. } => with_location(
            "配置文件语法无效",
            uri.as_deref().or(inherited_origin),
            inherited_key,
        ),
        config_rs::ConfigError::Type {
            origin,
            expected,
            key,
            ..
        } => with_location(
            format!("配置值类型无效，期望 {expected}"),
            origin.as_deref().or(inherited_origin),
            key.as_deref().or(inherited_key),
        ),
        config_rs::ConfigError::At { error, origin, key } => safe_config_error(
            error,
            origin.as_deref().or(inherited_origin),
            key.as_deref().or(inherited_key),
        ),
        config_rs::ConfigError::Message(_) => "配置内容无效".to_owned(),
        config_rs::ConfigError::Foreign(_) => "配置来源读取失败".to_owned(),
        _ => "配置加载失败".to_owned(),
    }
}

fn with_location(message: impl Into<String>, origin: Option<&str>, key: Option<&str>) -> String {
    let mut message = message.into();
    if let Some(origin) = origin {
        message.push_str(format!("（来源：{origin}").as_str());
        if let Some(key) = key {
            message.push_str(format!("，字段：{key}").as_str());
        }
        message.push('）');
    } else if let Some(key) = key {
        message.push_str(format!("（字段：{key}）").as_str());
    }
    message
}
