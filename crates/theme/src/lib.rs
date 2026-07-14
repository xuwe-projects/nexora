//! 桌面应用主题管理。
//!
//! 该 crate 负责注册应用内置主题、保存当前主题选择，并通过 `gpui-component`
//! 的全局 `Theme` 与 `ThemeRegistry` 统一应用浅色、深色和跟随系统外观模式。

use gpui::{App, Global, Window, px};
use gpui_component::{Size, Theme, ThemeMode, ThemeRegistry, scroll::ScrollbarShow};
use serde::{Deserialize, Serialize};

const XUWE_THEME_SET: &str = include_str!("../themes/xuwe.json");
const XUWE_LIGHT_THEME_NAME: &str = "Xuwe Light";
const XUWE_DARK_THEME_NAME: &str = "Xuwe Dark";
/// 应用界面默认基础字号，单位为逻辑像素。
pub const DEFAULT_FONT_SIZE: u16 = 14;
/// 应用设置允许选择的最小基础字号，单位为逻辑像素。
pub const MIN_FONT_SIZE: u16 = 12;
/// 应用设置允许选择的最大基础字号，单位为逻辑像素。
pub const MAX_FONT_SIZE: u16 = 20;
/// 支持 `with_size` 的应用组件默认使用标准尺寸。
pub const DEFAULT_COMPONENT_SIZE: Size = Size::Medium;

/// 应用颜色模式。
///
/// 颜色模式和具体主题预设相互独立：模式决定当前使用浅色还是深色主题，
/// 预设则决定两种模式分别对应哪一组颜色 token。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// 跟随操作系统当前的窗口外观，并在系统外观变化时自动同步。
    #[default]
    System,
    /// 始终使用当前主题预设中的浅色主题。
    Light,
    /// 始终使用当前主题预设中的深色主题。
    Dark,
}

impl ColorScheme {
    /// 设置界面可以展示的全部颜色模式。
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// 返回用于配置存储和下拉选项值的稳定标识。
    pub const fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// 返回适合直接展示在中文设置界面中的名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Light => "浅色",
            Self::Dark => "深色",
        }
    }

    /// 根据稳定标识解析颜色模式。
    ///
    /// 无法识别的标识会返回 `None`，调用方可以保留当前设置而不改变主题。
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scheme| scheme.id() == id)
    }
}

/// 应用内置的主题预设。
///
/// 每个预设同时声明一套浅色主题和一套深色主题，切换颜色模式时会自动选择同一预设
/// 中对应的主题，避免把两个不相关的主题意外组合在一起。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    /// 模板提供的铉微主题，使用中性色作为界面基底，并使用蓝色作为主要交互强调色。
    #[default]
    Xuwe,
}

impl ThemePreset {
    /// 设置界面可以选择的全部内置主题预设。
    pub const ALL: [Self; 1] = [Self::Xuwe];

    /// 返回用于配置存储和下拉选项值的稳定标识。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Xuwe => "xuwe",
        }
    }

    /// 返回适合直接展示在中文设置界面中的主题名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Xuwe => "铉微",
        }
    }

    /// 根据稳定标识解析主题预设。
    ///
    /// 无法识别的标识会返回 `None`，调用方可以保留当前设置而不改变主题。
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.id() == id)
    }
}

/// 当前应用的完整主题选择。
///
/// 该值适合由未来的配置模块持久化；恢复配置后可以调用 [`set_selection`] 一次性应用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ThemeSelection {
    preset: ThemePreset,
    color_scheme: ColorScheme,
}

impl ThemeSelection {
    /// 使用指定主题预设和颜色模式创建一个完整选择。
    pub const fn new(preset: ThemePreset, color_scheme: ColorScheme) -> Self {
        Self {
            preset,
            color_scheme,
        }
    }

    /// 返回当前选择的主题预设。
    pub const fn preset(self) -> ThemePreset {
        self.preset
    }

    /// 返回当前选择的颜色模式。
    pub const fn color_scheme(self) -> ColorScheme {
        self.color_scheme
    }
}

#[derive(Debug, Clone, Copy)]
struct ThemeState {
    selection: ThemeSelection,
    font_size: u16,
    component_size: Size,
}

impl ThemeState {
    fn new() -> Self {
        Self {
            selection: ThemeSelection::default(),
            font_size: DEFAULT_FONT_SIZE,
            component_size: DEFAULT_COMPONENT_SIZE,
        }
    }
}

impl Global for ThemeState {}

/// 初始化应用主题。
///
/// 调用方必须先执行 `gpui_component::init`，本函数随后会把编译进应用的 Xuwe 主题
/// 注册到组件主题表，创建默认主题状态，并立即按照系统外观应用主题。
///
/// # Panics
///
/// 当本函数早于 `gpui_component::init` 调用，或内置主题 JSON 不符合
/// `gpui-component` 的 `ThemeSet` 格式时会 panic。这两种情况都属于应用构建错误，
/// 并由主题 crate 的集成测试提前校验。
pub fn init(cx: &mut App) {
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(XUWE_THEME_SET)
        .expect("内置 Xuwe 主题必须符合 gpui-component ThemeSet 格式");

    if !cx.has_global::<ThemeState>() {
        cx.set_global(ThemeState::new());
    }

    apply_active_theme(None, cx);
}

/// 把主题管理器附着到一个新创建的窗口。
///
/// 该函数会先按窗口的实际外观同步一次主题，再监听后续系统外观变化。
/// 只有颜色模式为 [`ColorScheme::System`] 时，系统外观变化才会触发主题切换。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn attach_window(window: &mut Window, cx: &mut App) {
    apply_active_theme(Some(window), cx);

    window
        .observe_window_appearance(|window, cx| {
            if selection(cx).color_scheme() == ColorScheme::System {
                apply_active_theme(Some(window), cx);
            }
        })
        // xuwe-lint: allow(xuwe::detached_lifecycle) reason="监听由窗口持有并随窗口销毁"
        .detach();
}

/// 返回当前应用的主题选择。
///
/// 返回值为轻量可复制结构，可以直接用于设置界面展示或交给配置模块持久化。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn selection(cx: &App) -> ThemeSelection {
    cx.global::<ThemeState>().selection
}

/// 返回当前应用界面的基础字号，单位为逻辑像素。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn font_size(cx: &App) -> u16 {
    cx.global::<ThemeState>().font_size
}

/// 返回支持 `with_size` 的应用组件当前应使用的统一尺寸。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn component_size(cx: &App) -> Size {
    cx.global::<ThemeState>().component_size
}

/// 一次性更新主题预设与颜色模式并刷新全部窗口。
///
/// 当新选择和当前选择相同时不会重复应用主题。
///
/// # Panics
///
/// 在 [`init`] 之前调用，或主题注册表缺少所选预设对应的主题时会 panic。
pub fn set_selection(new_selection: ThemeSelection, cx: &mut App) {
    if selection(cx) == new_selection {
        return;
    }

    cx.global_mut::<ThemeState>().selection = new_selection;
    apply_active_theme(None, cx);
}

/// 更新主题预设，并保留当前颜色模式。
///
/// 该操作会同时替换预设的浅色和深色主题，然后刷新全部窗口。
///
/// # Panics
///
/// 在 [`init`] 之前调用，或主题注册表缺少所选预设对应的主题时会 panic。
pub fn set_preset(preset: ThemePreset, cx: &mut App) {
    let current = selection(cx);
    set_selection(ThemeSelection::new(preset, current.color_scheme()), cx);
}

/// 更新颜色模式，并保留当前主题预设。
///
/// 选择跟随系统时会读取平台当前外观；选择浅色或深色时会固定使用对应主题。
///
/// # Panics
///
/// 在 [`init`] 之前调用，或主题注册表缺少当前预设对应的主题时会 panic。
pub fn set_color_scheme(color_scheme: ColorScheme, cx: &mut App) {
    let current = selection(cx);
    set_selection(ThemeSelection::new(current.preset(), color_scheme), cx);
}

/// 更新应用界面的基础字号，并刷新所有窗口。
///
/// 传入值会被限制在 [`MIN_FONT_SIZE`] 和 [`MAX_FONT_SIZE`] 之间。该设置只修改当前
/// 运行时主题状态，调用方需要自行决定是否持久化到用户配置。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn set_font_size(new_font_size: u16, cx: &mut App) {
    let new_font_size = new_font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    if font_size(cx) == new_font_size {
        return;
    }

    cx.global_mut::<ThemeState>().font_size = new_font_size;
    apply_font_size(cx);
    cx.refresh_windows();
}

/// 更新支持 `with_size` 的应用组件统一尺寸，并刷新所有窗口。
///
/// 该设置只修改当前运行时主题状态，调用方需要自行决定是否持久化到用户配置。
///
/// # Panics
///
/// 在 [`init`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn set_component_size(new_component_size: Size, cx: &mut App) {
    if component_size(cx) == new_component_size {
        return;
    }

    cx.global_mut::<ThemeState>().component_size = new_component_size;
    cx.refresh_windows();
}

fn apply_active_theme(window: Option<&mut Window>, cx: &mut App) {
    let selection = selection(cx);
    let (light_theme, dark_theme) = {
        let registry = ThemeRegistry::global(cx);

        match selection.preset() {
            ThemePreset::Xuwe => (
                registry
                    .themes()
                    .get(XUWE_LIGHT_THEME_NAME)
                    .cloned()
                    .expect("主题注册表必须包含 Xuwe Light"),
                registry
                    .themes()
                    .get(XUWE_DARK_THEME_NAME)
                    .cloned()
                    .expect("主题注册表必须包含 Xuwe Dark"),
            ),
        }
    };

    {
        let active_theme = Theme::global_mut(cx);
        active_theme.light_theme = light_theme;
        active_theme.dark_theme = dark_theme;
    }

    match selection.color_scheme() {
        ColorScheme::System => Theme::sync_system_appearance(window, cx),
        ColorScheme::Light => Theme::change(ThemeMode::Light, window, cx),
        ColorScheme::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }

    Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Hover;
    apply_font_size(cx);
    cx.refresh_windows();
}

fn apply_font_size(cx: &mut App) {
    Theme::global_mut(cx).font_size = px(f32::from(font_size(cx)));
}
