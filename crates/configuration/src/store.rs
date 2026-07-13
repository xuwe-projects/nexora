//! 桌面用户配置的跨平台路径和原子持久化。

use std::{
    fs::{self, File},
    io::Write as _,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Serialize, de::DeserializeOwned};

use crate::{ConfigurationError, LayeredConfigLoader};

/// 可声明并校验配置 schema 版本的用户配置类型。
///
/// 新版本程序读取旧 schema 时可以在具体应用层先执行迁移；读取比程序更新的 schema 时，
/// [`UserConfigStore::load_versioned_or_default`] 会拒绝继续解析，避免覆盖未知字段。
pub trait VersionedConfiguration {
    /// 当前程序能够读写的 schema 版本。
    const CURRENT_SCHEMA_VERSION: u32;

    /// 返回当前配置实例声明的 schema 版本。
    fn schema_version(&self) -> u32;
}

/// 保存某个桌面应用用户偏好的 TOML 配置存储。
///
/// 默认路径由 [`ProjectDirs`] 按当前操作系统规则计算；测试或迁移工具也可以通过
/// [`UserConfigStore::at_path`] 显式指定文件位置。
#[derive(Debug, Clone)]
pub struct UserConfigStore<T> {
    path: PathBuf,
    _marker: PhantomData<fn() -> T>,
}

impl<T> UserConfigStore<T>
where
    T: Default + DeserializeOwned + Serialize,
{
    /// 为指定组织和应用创建系统标准目录中的用户配置存储。
    ///
    /// `qualifier` 通常使用反向域名顶级部分，例如 `com`；`organization` 和
    /// `application` 用于组成 macOS、Windows 与 Linux 各自约定的配置目录。
    ///
    /// # Errors
    ///
    /// 当前平台无法提供配置目录，或 `file_name` 不是单个普通文件名时返回错误。
    pub fn for_application(
        qualifier: &str,
        organization: &str,
        application: &str,
        file_name: impl AsRef<Path>,
    ) -> Result<Self, ConfigurationError> {
        let file_name = file_name.as_ref();
        validate_file_name(file_name)?;
        let project_dirs =
            ProjectDirs::from(qualifier, organization, application).ok_or_else(|| {
                ConfigurationError::ConfigDirectoryUnavailable {
                    application: application.to_owned(),
                }
            })?;

        Ok(Self::at_path(project_dirs.config_dir().join(file_name)))
    }

    /// 为指定组织和应用创建系统本机配置目录中的用户配置存储。
    ///
    /// 该构造函数使用 [`ProjectDirs::config_local_dir`] 定位不需要随用户漫游的配置：
    /// Windows 会使用当前用户的 Local AppData，Linux 与 macOS 则遵循各自的平台约定。
    /// `qualifier`、`organization` 和 `application` 的含义与
    /// [`UserConfigStore::for_application`] 相同。
    ///
    /// # Errors
    ///
    /// 当前平台无法提供本机配置目录，或 `file_name` 不是单个普通文件名时返回错误。
    pub fn for_local_application(
        qualifier: &str,
        organization: &str,
        application: &str,
        file_name: impl AsRef<Path>,
    ) -> Result<Self, ConfigurationError> {
        let file_name = file_name.as_ref();
        validate_file_name(file_name)?;
        let project_dirs =
            ProjectDirs::from(qualifier, organization, application).ok_or_else(|| {
                ConfigurationError::ConfigDirectoryUnavailable {
                    application: application.to_owned(),
                }
            })?;

        Ok(Self::at_path(
            project_dirs.config_local_dir().join(file_name),
        ))
    }

    /// 使用调用方提供的完整文件路径创建用户配置存储。
    ///
    /// 该构造函数适合测试、迁移工具以及已有固定配置位置的应用。
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            _marker: PhantomData,
        }
    }

    /// 返回该存储实际读写的 TOML 文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取用户配置；文件不存在时返回目标类型的默认值。
    ///
    /// 用户配置不会读取环境变量，防止部署环境覆盖用户在界面中做出的选择。
    ///
    /// # Errors
    ///
    /// 文件无法读取、TOML 格式无效或字段无法反序列化时返回错误。
    pub fn load_or_default(&self) -> Result<T, ConfigurationError> {
        if !self.path.exists() {
            return Ok(T::default());
        }

        LayeredConfigLoader::new()
            .with_required_file(&self.path)
            .without_environment()
            .load()
    }

    /// 把用户配置写入临时文件，并在写入成功后替换正式配置文件。
    ///
    /// 父目录不存在时会自动创建。写入失败不会使用不完整的临时内容覆盖原配置。
    ///
    /// # Errors
    ///
    /// TOML 序列化、目录创建、文件写入、同步或替换失败时返回错误。
    pub fn save(&self, value: &T) -> Result<(), ConfigurationError> {
        let content = toml::to_string_pretty(value)?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let temporary_path = temporary_path(&self.path);
        let mut temporary_file = File::create(&temporary_path)?;
        temporary_file.write_all(content.as_bytes())?;
        temporary_file.sync_all()?;
        replace_file(&temporary_path, &self.path)?;
        Ok(())
    }
}

impl<T> UserConfigStore<T>
where
    T: Default + DeserializeOwned + Serialize + VersionedConfiguration,
{
    /// 读取用户配置并确认其 schema 版本不高于当前程序支持版本。
    ///
    /// # Errors
    ///
    /// 除普通读取错误外，当配置 schema 高于 [`VersionedConfiguration::CURRENT_SCHEMA_VERSION`]
    /// 时返回 [`ConfigurationError::UnsupportedSchema`]。
    pub fn load_versioned_or_default(&self) -> Result<T, ConfigurationError> {
        let value = self.load_or_default()?;
        if value.schema_version() > T::CURRENT_SCHEMA_VERSION {
            return Err(ConfigurationError::UnsupportedSchema {
                expected: T::CURRENT_SCHEMA_VERSION,
                actual: value.schema_version(),
            });
        }

        Ok(value)
    }
}

fn validate_file_name(file_name: &Path) -> Result<(), ConfigurationError> {
    let mut components = file_name.components();
    let valid =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if valid {
        return Ok(());
    }

    Err(ConfigurationError::InvalidFileName(file_name.to_path_buf()))
}

fn temporary_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!("{extension}.tmp"))
        .unwrap_or_else(|| "tmp".to_owned());
    path.with_extension(extension)
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary_path: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::rename(temporary_path, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(temporary_path: &Path, destination: &Path) -> Result<(), std::io::Error> {
    if !destination.exists() {
        return fs::rename(temporary_path, destination);
    }

    let backup = destination.with_extension("backup");
    _ = fs::remove_file(&backup);
    fs::rename(destination, &backup)?;
    match fs::rename(temporary_path, destination) {
        Ok(()) => {
            _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            _ = fs::rename(backup, destination);
            Err(error)
        }
    }
}
