//! Console 桌面应用用户偏好配置。

use configuration::{ConfigurationError, UserConfigStore, VersionedConfiguration};
use serde::{Deserialize, Serialize};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

/// Console 用户配置当前 schema 版本。
const CONSOLE_SCHEMA_VERSION: u32 = 1;

/// Console 用户偏好的跨平台配置存储类型。
pub type ConsolePreferencesStore = UserConfigStore<ConsolePreferences>;

/// Console 持久化到本地 TOML 文件的用户偏好。
///
/// 该类型只保存用户可以在界面中修改的设置，不包含 API 地址、更新地址、签名身份等
/// 构建期或受信任引导配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsolePreferences {
    /// 配置文件 schema 版本，用于后续迁移和拒绝未知的新格式。
    schema_version: u32,
    /// 外观和主题相关用户选择。
    appearance: AppearancePreferences,
}

impl Default for ConsolePreferences {
    /// 创建使用铉微主题并跟随系统颜色模式的默认用户偏好。
    fn default() -> Self {
        Self {
            schema_version: CONSOLE_SCHEMA_VERSION,
            appearance: AppearancePreferences::default(),
        }
    }
}

impl ConsolePreferences {
    /// 返回配置中保存的完整主题选择。
    pub fn theme_selection(&self) -> ThemeSelection {
        ThemeSelection::new(self.appearance.theme_preset, self.appearance.color_scheme)
    }

    /// 使用当前界面主题更新用户偏好，并同步写入当前 schema 版本。
    pub fn set_theme_selection(&mut self, selection: ThemeSelection) {
        self.schema_version = CONSOLE_SCHEMA_VERSION;
        self.appearance.theme_preset = selection.preset();
        self.appearance.color_scheme = selection.color_scheme();
    }
}

impl VersionedConfiguration for ConsolePreferences {
    const CURRENT_SCHEMA_VERSION: u32 = CONSOLE_SCHEMA_VERSION;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

/// Console 外观类用户偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
struct AppearancePreferences {
    /// 当前选择的主题预设。
    theme_preset: ThemePreset,
    /// 当前选择的浅色、深色或跟随系统模式。
    color_scheme: ColorScheme,
}

/// 创建 Console 在当前操作系统标准配置目录中的用户偏好存储。
///
/// # Errors
///
/// 当前平台无法确定应用配置目录时返回 [`ConfigurationError`]。
pub fn preferences_store() -> Result<ConsolePreferencesStore, ConfigurationError> {
    UserConfigStore::for_application("com", "Xuwe", "Console", "settings.toml")
}
