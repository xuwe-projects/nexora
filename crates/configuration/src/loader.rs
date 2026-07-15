//! 基于 `config-rs` 的配置源分层加载器。

use std::{marker::PhantomData, path::PathBuf};

use config_rs::{Config, Environment, File};
use serde::de::DeserializeOwned;

use crate::ConfigurationError;

/// 按固定优先级加载文件和无前缀环境变量的配置加载器。
///
/// 默认启用环境变量源，不添加组织或项目名前缀。嵌套字段使用双下划线表达，
/// 例如 `SERVER__PORT` 会覆盖配置中的 `server.port`。
#[derive(Debug, Clone)]
pub struct LayeredConfigLoader<T> {
    files: Vec<(PathBuf, bool)>,
    include_environment: bool,
    environment_list_separator: Option<String>,
    environment_list_keys: Vec<String>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Default for LayeredConfigLoader<T> {
    /// 创建默认启用无前缀环境变量源的加载器。
    fn default() -> Self {
        Self {
            files: Vec::new(),
            include_environment: true,
            environment_list_separator: None,
            environment_list_keys: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<T> LayeredConfigLoader<T>
where
    T: DeserializeOwned,
{
    /// 创建默认配置加载器。
    ///
    /// 默认没有文件源，并启用无前缀环境变量源。调用方可以继续指定必需或可选文件，
    /// 也可以为桌面用户配置关闭环境变量覆盖。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个必须存在的配置文件。
    ///
    /// 文件内容会先于环境变量加载，因此同名环境变量拥有更高优先级。
    pub fn with_required_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push((path.into(), true));
        self
    }

    /// 添加一个允许不存在的配置文件。
    ///
    /// 适合具有完整代码默认值、但允许部署环境提供覆盖文件的程序。
    pub fn with_optional_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push((path.into(), false));
        self
    }

    /// 将一个环境变量字段按指定分隔符解析为字符串列表。
    ///
    /// `key` 使用反序列化后的点分字段名，例如 `features.enabled`。多次调用可以注册多个列表字段，但这些字段
    /// 共享最后一次传入的分隔符；普通字符串字段不会受影响。
    pub fn with_environment_list(
        mut self,
        key: impl Into<String>,
        separator: impl Into<String>,
    ) -> Self {
        self.environment_list_keys.push(key.into());
        self.environment_list_separator = Some(separator.into());
        self
    }

    /// 关闭环境变量配置源。
    ///
    /// 桌面用户偏好通常只应由用户设置文件控制，避免进程环境意外覆盖界面选择。
    pub fn without_environment(mut self) -> Self {
        self.include_environment = false;
        self
    }

    /// 合并已配置的来源并反序列化为目标配置类型。
    ///
    /// 环境变量不使用统一前缀，双下划线表示嵌套层级，并尝试把布尔值和数字解析为
    /// 对应类型。目标类型可以使用 `#[serde(default)]` 提供代码默认值。
    ///
    /// # Errors
    ///
    /// 必需文件不存在、文件格式无效、环境变量类型不匹配或目标类型反序列化失败时，
    /// 返回 [`ConfigurationError`]。
    pub fn load(self) -> Result<T, ConfigurationError> {
        let mut builder = Config::builder();
        for (path, required) in self.files {
            builder = builder.add_source(File::from(path).required(required));
        }
        if self.include_environment {
            let mut environment = Environment::default()
                .separator("__")
                .try_parsing(true)
                .ignore_empty(true);
            if let Some(separator) = self.environment_list_separator.as_deref() {
                environment = environment.list_separator(separator);
                for key in &self.environment_list_keys {
                    environment = environment.with_list_parse_key(key);
                }
            }
            builder = builder.add_source(environment);
        }

        Ok(builder.build()?.try_deserialize()?)
    }
}
