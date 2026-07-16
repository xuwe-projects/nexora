//! Console 桌面应用用户偏好配置。

use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use crate::features::FeatureId;
use configuration::{ConfigurationError, UserConfigStore, VersionedConfiguration};
use gpui::{App, Context, Global, Subscription, UpdateGlobal as _};
use gpui_component::Size;
use serde::{Deserialize, Serialize};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

/// Console 用户配置当前 schema 版本。
const CONSOLE_SCHEMA_VERSION: u32 = 3;

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
    /// 工作区标签和导航相关用户选择。
    workspace: WorkspacePreferences,
}

impl Default for ConsolePreferences {
    /// 创建使用铉微主题并跟随系统颜色模式的默认用户偏好。
    fn default() -> Self {
        Self {
            schema_version: CONSOLE_SCHEMA_VERSION,
            appearance: AppearancePreferences::default(),
            window: WindowPreferences::default(),
            workspace: WorkspacePreferences::default(),
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

    /// 返回应用界面的基础字体大小，单位为逻辑像素。
    pub fn font_size(&self) -> u16 {
        self.appearance
            .font_size
            .clamp(theme::MIN_FONT_SIZE, theme::MAX_FONT_SIZE)
    }

    /// 更新应用界面的基础字体大小，并限制在设置页允许的范围内。
    pub fn set_font_size(&mut self, font_size: u16) {
        self.schema_version = CONSOLE_SCHEMA_VERSION;
        self.appearance.font_size = font_size.clamp(theme::MIN_FONT_SIZE, theme::MAX_FONT_SIZE);
    }

    /// 返回支持 `with_size` 的组件应使用的统一尺寸语义。
    pub fn component_size(&self) -> Size {
        self.appearance.component_size
    }

    /// 更新支持 `with_size` 的组件应使用的统一尺寸语义。
    pub fn set_component_size(&mut self, component_size: Size) {
        self.schema_version = CONSOLE_SCHEMA_VERSION;
        self.appearance.component_size = component_size;
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

    /// 返回使用具体逻辑路径表达的置顶标签。
    ///
    /// 新版本直接保存 `/users/details/42` 等具体路径；读取旧版本保存的 FeatureId 时会
    /// 自动转换为对应静态路径，从而保持已有用户配置可继续使用。
    pub fn pinned_tab_paths(&self) -> Vec<String> {
        self.workspace
            .pinned_tabs
            .iter()
            .filter_map(|value| {
                if value.starts_with('/') {
                    Some(value.clone())
                } else {
                    FeatureId::from_id(value).map(|feature| feature.path().to_owned())
                }
            })
            .filter(|path| !path.contains(':'))
            .fold(Vec::new(), |mut paths, path| {
                if !paths.contains(&path) {
                    paths.push(path);
                }
                paths
            })
    }

    /// 使用具体逻辑路径更新需要在下次启动时恢复的置顶标签。
    pub fn set_pinned_tab_paths(&mut self, pinned_tabs: &[String]) {
        self.schema_version = CONSOLE_SCHEMA_VERSION;
        self.workspace.pinned_tabs = pinned_tabs.iter().fold(Vec::new(), |mut paths, path| {
            if !paths.contains(path) {
                paths.push(path.clone());
            }
            paths
        });
    }
}

impl VersionedConfiguration for ConsolePreferences {
    const CURRENT_SCHEMA_VERSION: u32 = CONSOLE_SCHEMA_VERSION;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

/// Console 外观类用户偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct AppearancePreferences {
    /// 当前选择的主题预设。
    theme_preset: ThemePreset,
    /// 当前选择的浅色、深色或跟随系统模式。
    color_scheme: ColorScheme,
    /// 应用界面的基础字体大小，单位为逻辑像素。
    font_size: u16,
    /// 支持 `with_size` 的组件统一使用的尺寸语义。
    component_size: Size,
}

impl Default for AppearancePreferences {
    fn default() -> Self {
        Self {
            theme_preset: ThemePreset::default(),
            color_scheme: ColorScheme::default(),
            font_size: theme::DEFAULT_FONT_SIZE,
            component_size: theme::DEFAULT_COMPONENT_SIZE,
        }
    }
}

/// Console 窗口类用户偏好。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
struct WindowPreferences {
    /// 主窗口启动及后续新窗口使用的显示器稳定 UUID；为空时跟随系统主显示器。
    startup_display_uuid: Option<String>,
}

/// Console 工作区标签相关用户偏好。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
struct WorkspacePreferences {
    /// 按标签栏展示顺序保存的置顶功能区稳定标识。
    pinned_tabs: Vec<String>,
}

#[derive(Debug)]
struct PreferencesWriter {
    sender: Sender<PreferencesWriteCommand>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug)]
enum PreferencesWriteCommand {
    Persist(ConsolePreferences),
    Shutdown,
}

impl PreferencesWriter {
    fn start(store: ConsolePreferencesStore) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("xuwe-preferences".to_owned())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    let PreferencesWriteCommand::Persist(mut preferences) = command else {
                        break;
                    };
                    let mut shutdown = false;
                    while let Ok(command) = receiver.try_recv() {
                        match command {
                            PreferencesWriteCommand::Persist(latest) => preferences = latest,
                            PreferencesWriteCommand::Shutdown => {
                                shutdown = true;
                                break;
                            }
                        }
                    }

                    if let Err(error) = store.save(&preferences) {
                        tracing::error!(error = %error, "无法保存 Console 用户配置");
                    }
                    if shutdown {
                        break;
                    }
                }
            })?;

        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    fn persist(&self, preferences: &ConsolePreferences) {
        if self
            .sender
            .send(PreferencesWriteCommand::Persist(preferences.clone()))
            .is_err()
        {
            tracing::error!("Console 用户配置后台写入线程已经停止");
        }
    }
}

impl Drop for PreferencesWriter {
    fn drop(&mut self) {
        _ = self.sender.send(PreferencesWriteCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            _ = worker.join();
        }
    }
}

#[derive(Debug, Default)]
struct PreferencesState {
    preferences: ConsolePreferences,
    writer: Option<PreferencesWriter>,
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

/// 返回当前内存中恢复的应用基础字体大小。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn font_size(cx: &App) -> u16 {
    cx.global::<PreferencesState>().preferences.font_size()
}

/// 返回当前内存中恢复的统一组件尺寸。
///
/// 表格、设置控件等支持 `gpui-component::Sizable` 的组件应把该值传给 `with_size`，
/// 避免各页面分别维护密度常量。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn component_size(cx: &App) -> Size {
    cx.global::<PreferencesState>().preferences.component_size()
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

/// 返回启动时恢复且仍能由 Nexora 注册表解析的置顶标签路径。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn pinned_tab_paths(cx: &App) -> Vec<String> {
    cx.global::<PreferencesState>()
        .preferences
        .pinned_tab_paths()
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

/// 更新内存中的主题选择，并把最新快照交给后台线程保存。
///
/// 文件保存失败时保留本次运行中的选择，并记录错误日志，避免中断设置交互。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_theme_selection(selection: ThemeSelection, cx: &mut App) {
    update_preferences(cx, |preferences| {
        preferences.set_theme_selection(selection);
    });
}

/// 更新应用基础字体大小，并把最新快照交给后台线程保存。
///
/// 字号会被限制在设置页允许的范围内；保存失败时仍保留本次运行中的选择。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_font_size(font_size: u16, cx: &mut App) {
    update_preferences(cx, |preferences| {
        preferences.set_font_size(font_size);
    });
}

/// 更新统一组件尺寸，并把最新快照交给后台线程保存。
///
/// 该函数只负责更新持久化偏好；调用方应先通过 `theme::set_component_size` 应用运行时状态。
/// 保存失败时仍保留本次运行中的选择。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_component_size(new_component_size: Size, cx: &mut App) {
    if component_size(cx) == new_component_size {
        return;
    }

    update_preferences(cx, |preferences| {
        preferences.set_component_size(new_component_size);
    });
}

/// 更新窗口默认显示器，并把最新快照交给后台线程保存。
///
/// 传入 `None` 表示改为跟随系统主显示器。文件保存失败时保留本次运行中的选择并记录错误日志。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_startup_display_uuid(display_uuid: Option<String>, cx: &mut App) {
    update_preferences(cx, |preferences| {
        preferences.set_startup_display_uuid(display_uuid);
    });
}

/// 更新具体路径形式的置顶标签顺序，并把最新快照交给后台线程保存。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为偏好全局状态尚未注册而 panic。
pub fn persist_pinned_tab_paths(pinned_tabs: &[String], cx: &mut App) {
    update_preferences(cx, |preferences| {
        preferences.set_pinned_tab_paths(pinned_tabs);
    });
}

fn update_preferences(cx: &mut App, update: impl FnOnce(&mut ConsolePreferences)) {
    PreferencesState::update_global(cx, |state, _| {
        update(&mut state.preferences);
        if let Some(writer) = state.writer.as_ref() {
            writer.persist(&state.preferences);
        }
    });
}

fn load_preferences_state() -> PreferencesState {
    let store = match preferences_store() {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(error = %error, "无法确定 Console 用户配置目录");
            return PreferencesState::default();
        }
    };

    if let Err(error) = migrate_legacy_preferences(&store) {
        tracing::error!(error = %error, "无法迁移 Console 旧版用户配置");
    }

    match store.load_versioned_or_default() {
        Ok(preferences) => {
            let writer = match PreferencesWriter::start(store) {
                Ok(writer) => Some(writer),
                Err(error) => {
                    tracing::error!(error = %error, "无法启动 Console 用户配置后台写入线程");
                    None
                }
            };
            PreferencesState {
                preferences,
                writer,
            }
        }
        Err(error) => {
            tracing::error!(error = %error, "无法加载 Console 用户配置");
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
