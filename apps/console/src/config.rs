//! Console 桌面应用用户偏好配置。

use configuration::{ConfigurationError, UserConfigStore, VersionedConfiguration};
use gpui::{App, Context, Global, Subscription, UpdateGlobal as _};
use serde::{Deserialize, Serialize};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

/// Console 用户配置当前 schema 版本。
const CONSOLE_SCHEMA_VERSION: u32 = 2;

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
    /// 窗口创建和启动位置相关用户选择。
    window: WindowPreferences,
}

impl Default for ConsolePreferences {
    /// 创建使用铉微主题并跟随系统颜色模式的默认用户偏好。
    fn default() -> Self {
        Self {
            schema_version: CONSOLE_SCHEMA_VERSION,
            appearance: AppearancePreferences::default(),
            window: WindowPreferences::default(),
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

    /// 返回主窗口启动及后续新窗口优先使用的显示器稳定 UUID。
    ///
    /// 返回 `None` 表示跟随操作系统当前主显示器。
    pub fn startup_display_uuid(&self) -> Option<&str> {
        self.window.startup_display_uuid.as_deref()
    }

    /// 更新主窗口启动及后续新窗口优先使用的显示器稳定 UUID。
    ///
    /// 传入 `None` 会恢复为跟随操作系统主显示器；该设置影响后续创建的窗口。
    pub fn set_startup_display_uuid(&mut self, display_uuid: Option<String>) {
        self.schema_version = CONSOLE_SCHEMA_VERSION;
        self.window.startup_display_uuid = display_uuid;
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

/// Console 窗口类用户偏好。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
struct WindowPreferences {
    /// 主窗口启动及后续新窗口使用的显示器稳定 UUID；为空时跟随系统主显示器。
    startup_display_uuid: Option<String>,
}

#[derive(Debug, Default)]
struct PreferencesState {
    preferences: ConsolePreferences,
    store: Option<ConsolePreferencesStore>,
}

impl Global for PreferencesState {}

/// 创建 Console 在当前操作系统标准配置目录中的用户偏好存储。
///
/// # Errors
///
/// 当前平台无法确定应用配置目录时返回 [`ConfigurationError`]。
pub fn preferences_store() -> Result<ConsolePreferencesStore, ConfigurationError> {
    UserConfigStore::for_local_application("com", "Xuwe", "Console", "settings.toml")
}

/// 在应用启动阶段加载当前操作系统用户的本地偏好并注册全局状态。
///
/// Windows 使用 `%LOCALAPPDATA%`，macOS 与 Linux 使用各自的平台标准配置目录。
/// 旧版 Windows 漫游目录中的设置会在本地文件尚不存在时复制到新位置。
pub fn init(cx: &mut App) {
    if cx.has_global::<PreferencesState>() {
        return;
    }

    let state = load_preferences_state();
    cx.set_global(state);
}

/// 返回当前内存中恢复的完整主题选择。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn theme_selection(cx: &App) -> ThemeSelection {
    cx.global::<PreferencesState>()
        .preferences
        .theme_selection()
}

/// 返回主窗口启动及后续新窗口优先使用的显示器稳定 UUID。
///
/// `None` 表示跟随操作系统主显示器；返回值来自启动时加载的内存状态，不会在渲染期间访问文件。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn startup_display_uuid(cx: &App) -> Option<&str> {
    cx.global::<PreferencesState>()
        .preferences
        .startup_display_uuid()
}

/// 观察用户偏好变化，并在变化时通知当前 Entity 局部重渲染。
///
/// 返回的订阅必须由 Entity 持有；Entity 销毁时订阅会同步失效。
pub(crate) fn observe_preferences<T>(cx: &mut Context<T>) -> Subscription
where
    T: 'static,
{
    cx.observe_global::<PreferencesState>(|_, cx| cx.notify())
}

/// 更新内存中的主题选择并立即保存到当前用户的本地配置文件。
///
/// 文件保存失败时保留本次运行中的选择，并把错误输出到标准错误，避免中断设置交互。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_theme_selection(selection: ThemeSelection, cx: &mut App) {
    PreferencesState::update_global(cx, |state, _| {
        state.preferences.set_theme_selection(selection);
        if let Some(store) = state.store.as_ref()
            && let Err(error) = store.save(&state.preferences)
        {
            eprintln!("无法保存 Console 用户主题配置: {error}");
        }
    });
}

/// 更新窗口默认显示器并立即保存到当前用户的本地配置文件。
///
/// 传入 `None` 表示改为跟随系统主显示器。文件保存失败时保留本次运行中的选择，
/// 并把错误输出到标准错误。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_startup_display_uuid(display_uuid: Option<String>, cx: &mut App) {
    PreferencesState::update_global(cx, |state, _| {
        state.preferences.set_startup_display_uuid(display_uuid);
        if let Some(store) = state.store.as_ref()
            && let Err(error) = store.save(&state.preferences)
        {
            eprintln!("无法保存 Console 启动显示器配置: {error}");
        }
    });
}

fn load_preferences_state() -> PreferencesState {
    let store = match preferences_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("无法确定 Console 用户配置目录: {error}");
            return PreferencesState::default();
        }
    };

    if let Err(error) = migrate_legacy_preferences(&store) {
        eprintln!("无法迁移 Console 旧版用户配置: {error}");
    }

    match store.load_versioned_or_default() {
        Ok(preferences) => PreferencesState {
            preferences,
            store: Some(store),
        },
        Err(error) => {
            eprintln!("无法加载 Console 用户配置: {error}");
            PreferencesState::default()
        }
    }
}

fn migrate_legacy_preferences(
    destination: &ConsolePreferencesStore,
) -> Result<(), ConfigurationError> {
    let source = UserConfigStore::for_application("com", "Xuwe", "Console", "settings.toml")?;
    if source.path() == destination.path() || destination.path().exists() || !source.path().exists()
    {
        return Ok(());
    }

    destination.save(&source.load_versioned_or_default()?)
}
