//! 桌面用户配置的跨平台路径和原子持久化。

use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(not(target_os = "windows"))]
use std::fs::File;

use directories::{BaseDirs, ProjectDirs};
use serde::{Serialize, de::DeserializeOwned};

use crate::{ConfigurationError, LayeredConfigLoader};

/// 用户配置迁移的只读判定或完成结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// 新配置已经存在，因此保留它并跳过旧文件读取。
    CurrentFileExists,
    /// 旧配置存在且已成功写入新配置；旧文件仍保留在原位置。
    Migrated,
    /// 新旧配置都不存在，调用方将使用类型默认值。
    NoSource,
}

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

/// 保存某个桌面应用用户偏好的配置存储。
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

    /// 为 Cargo package 创建 `~/.xuwe/<package>/settings.json` 用户设置存储。
    ///
    /// 路径通过平台主目录 API 与 [`PathBuf::join`] 构造，不依赖路径分隔符文本。`package`
    /// 必须是单个普通路径段，通常直接传入 `env!("CARGO_PKG_NAME")`。
    ///
    /// # Errors
    ///
    /// 平台无法提供用户主目录，或 package 不是安全路径段时返回错误。
    pub fn for_xuwe_application(package: &str) -> Result<Self, ConfigurationError> {
        validate_application_name(package)?;
        let base_dirs =
            BaseDirs::new().ok_or_else(|| ConfigurationError::ConfigDirectoryUnavailable {
                application: package.to_owned(),
            })?;
        Ok(Self::at_path(
            base_dirs
                .home_dir()
                .join(".xuwe")
                .join(package)
                .join("settings.json"),
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

        if is_json_path(&self.path) {
            let bytes = fs::read(&self.path)?;
            return Ok(serde_json::from_slice(&bytes)?);
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
    /// JSON/TOML 序列化、目录创建、文件写入、同步或替换失败时返回错误。
    pub fn save(&self, value: &T) -> Result<(), ConfigurationError> {
        let content = if is_json_path(&self.path) {
            let mut bytes = serde_json::to_vec_pretty(value)?;
            bytes.push(b'\n');
            bytes
        } else {
            toml::to_string_pretty(value)?.into_bytes()
        };
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let temporary_path = temporary_path(&self.path);
        let result = (|| {
            let mut temporary_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)?;
            temporary_file.write_all(&content)?;
            temporary_file.sync_all()?;
            replace_file(&temporary_path, &self.path)?;
            sync_parent_directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            _ = fs::remove_file(temporary_path);
        }
        result
    }

    /// 在新文件缺失时把旧用户配置迁移到当前存储。
    ///
    /// 新文件已存在时不会读取或覆盖；旧文件不存在时不写入；迁移成功后仅创建新文件，
    /// 不删除、不重命名也不修改旧文件。序列化格式由两个路径的扩展名分别决定，因此可将
    /// 旧 TOML 安全迁移为 JSON。
    ///
    /// # Errors
    ///
    /// 旧配置读取失败，或新配置无法原子写入时返回错误。错误不包含配置原始值。
    pub fn migrate_from(&self, legacy: &Self) -> Result<MigrationOutcome, ConfigurationError> {
        if self.path.is_file() {
            return Ok(MigrationOutcome::CurrentFileExists);
        }
        if !legacy.path.is_file() {
            return Ok(MigrationOutcome::NoSource);
        }
        let value = legacy.load_or_default()?;
        self.save(&value)?;
        Ok(MigrationOutcome::Migrated)
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

fn validate_application_name(application: &str) -> Result<(), ConfigurationError> {
    let path = Path::new(application);
    let mut components = path.components();
    let valid = !application.trim().is_empty()
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();
    if valid {
        Ok(())
    } else {
        Err(ConfigurationError::InvalidApplicationName(
            application.to_owned(),
        ))
    }
}

fn is_json_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

fn temporary_path(path: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(
            || format!("{}-{unique}.tmp", std::process::id()),
            |extension| format!("{extension}.{}-{unique}.tmp", std::process::id()),
        );
    path.with_extension(extension)
}

#[cfg(not(target_os = "windows"))]
fn sync_parent_directory(parent: &Path) -> Result<(), std::io::Error> {
    File::open(parent)?.sync_all()
}

#[cfg(target_os = "windows")]
fn sync_parent_directory(_parent: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary_path: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::rename(temporary_path, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(temporary_path: &Path, destination: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let temporary = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: 两个 UTF-16 缓冲区均以 NUL 结尾，并在调用返回前保持存活。
    unsafe {
        MoveFileExW(
            PCWSTR(temporary.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| std::io::Error::last_os_error())
}
