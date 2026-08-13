//! 桌面应用主题管理。
//!
//! 该 crate 负责校验并注册应用内置主题、保存当前主题选择，并通过 `gpui-component`
//! 的全局 `Theme` 与 `ThemeRegistry` 统一应用浅色、深色和跟随系统外观模式。

use std::{collections::HashSet, rc::Rc};

use gpui::{App, Global, SharedString, Window, px};
use gpui_component::{
    Size, Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet, scroll::ScrollbarMode,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const NEXORA_THEME_SET: &str = include_str!("../themes/nexora.json");
/// Nexora 内置主题使用的稳定预设 ID。
pub const NEXORA_THEME_PRESET_ID: &str = "nexora";
const NEUTRAL_THEME_SET: &str = include_str!("../themes/neutral.json");
/// 内置中性无彩主题使用的稳定预设 ID。
pub const NEUTRAL_THEME_PRESET_ID: &str = "neutral";
const LEGACY_XUWE_THEME_PRESET_ID: &str = "xuwe";
/// 应用界面默认基础字号，单位为逻辑像素。
pub const DEFAULT_FONT_SIZE: u16 = 14;
/// 应用设置允许选择的最小基础字号，单位为逻辑像素。
pub const MIN_FONT_SIZE: u16 = 12;
/// 应用设置允许选择的最大基础字号，单位为逻辑像素。
pub const MAX_FONT_SIZE: u16 = 20;
/// 支持 `with_size` 的应用组件默认使用标准尺寸。
pub const DEFAULT_COMPONENT_SIZE: Size = Size::Medium;

/// 下游应用在编译期提供的一组浅色与深色主题预设。
///
/// `id` 是写入用户偏好的稳定身份，`label` 用于设置页展示，`json` 必须是恰好包含
/// 一个浅色主题和一个深色主题的 `gpui-component` `ThemeSet` JSON。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePresetSource {
    id: &'static str,
    label: &'static str,
    json: &'static str,
}

impl ThemePresetSource {
    /// 创建一组随应用二进制发布的主题预设。
    ///
    /// 本函数不立即解析 JSON；[`ThemeCatalog::new`] 会集中校验 ID、显示名称和主题配对。
    pub const fn new(id: &'static str, label: &'static str, json: &'static str) -> Self {
        Self { id, label, json }
    }

    /// 返回用于持久化和运行时选择的稳定预设 ID。
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// 返回设置页展示的预设名称。
    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// 返回编译进应用的原始 `ThemeSet` JSON。
    pub const fn json(&self) -> &'static str {
        self.json
    }
}

/// 设置页和自定义客户端 UI 可读取的主题预设元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePresetMetadata {
    id: SharedString,
    label: SharedString,
}

impl ThemePresetMetadata {
    /// 返回用于持久化和运行时选择的稳定预设 ID。
    pub fn id(&self) -> &str {
        self.id.as_ref()
    }

    /// 返回适合直接展示在设置页中的预设名称。
    pub fn label(&self) -> &str {
        self.label.as_ref()
    }
}

#[derive(Debug, Clone)]
struct RegisteredThemePreset {
    metadata: ThemePresetMetadata,
    light: Rc<ThemeConfig>,
    dark: Rc<ThemeConfig>,
}

/// 已完成严格校验、可在 GPUI 启动阶段安装的应用主题目录。
///
/// 目录始终把内置 Nexora 主题放在首位，并按下游应用调用 `theme_preset(...)` 的顺序
/// 保存其他预设。其内部主题名称已经规范化，不受各 JSON 文件原始 `name` 冲突影响。
#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    presets: Vec<RegisteredThemePreset>,
    default_preset_id: SharedString,
    registry_json: String,
}

impl ThemeCatalog {
    /// 校验下游主题并构建应用主题目录。
    ///
    /// `default_preset_id` 为 `None` 时使用内置 `nexora`。显式默认 ID 必须指向本次目录中
    /// 已注册的预设。
    ///
    /// # Errors
    ///
    /// ID 非法、使用保留 ID、显示名称为空、JSON 无效、浅色与深色主题未严格配对、
    /// ID 重复，或默认 ID 未注册时返回 [`ThemeCatalogError`]。
    pub fn new(
        sources: &[ThemePresetSource],
        default_preset_id: Option<&str>,
    ) -> Result<Self, ThemeCatalogError> {
        let mut presets = vec![
            parse_theme_preset(NEXORA_THEME_PRESET_ID, "Nexora", NEXORA_THEME_SET)?,
            parse_theme_preset(NEUTRAL_THEME_PRESET_ID, "中性", NEUTRAL_THEME_SET)?,
        ];
        let mut ids = HashSet::from([NEXORA_THEME_PRESET_ID, NEUTRAL_THEME_PRESET_ID]);

        for source in sources {
            validate_custom_preset_id(source.id)?;
            if source.label.trim().is_empty() {
                return Err(ThemeCatalogError::EmptyLabel {
                    id: source.id.to_owned(),
                });
            }
            if !ids.insert(source.id) {
                return Err(ThemeCatalogError::DuplicateId {
                    id: source.id.to_owned(),
                });
            }
            presets.push(parse_theme_preset(source.id, source.label, source.json)?);
        }

        let default_preset_id = default_preset_id.unwrap_or(NEXORA_THEME_PRESET_ID);
        if !ids.contains(default_preset_id) {
            return Err(ThemeCatalogError::UnknownDefaultPreset {
                id: default_preset_id.to_owned(),
            });
        }

        let registry_set = ThemeSet {
            name: "Nexora Application Themes".into(),
            author: Some("Nexora".into()),
            url: None,
            themes: presets
                .iter()
                .flat_map(|preset| [preset.light.as_ref().clone(), preset.dark.as_ref().clone()])
                .collect(),
        };
        let registry_json = serde_json::to_string(&registry_set)
            .map_err(|source| ThemeCatalogError::Serialize { source })?;

        Ok(Self {
            presets,
            default_preset_id: default_preset_id.into(),
            registry_json,
        })
    }

    /// 返回设置页可展示的全部主题预设，内置 Nexora 始终位于首项。
    pub fn presets(&self) -> impl ExactSizeIterator<Item = &ThemePresetMetadata> {
        self.presets.iter().map(|preset| &preset.metadata)
    }

    /// 返回应用声明的默认主题预设 ID。
    pub fn default_preset_id(&self) -> &str {
        self.default_preset_id.as_ref()
    }

    /// 判断目录是否包含指定稳定 ID。
    pub fn contains(&self, preset_id: &str) -> bool {
        self.preset(preset_id).is_some()
    }

    /// 按启动优先级把可选的持久化预设 ID 解析为目录中的稳定 ID。
    ///
    /// 没有持久化值或值已失效时返回应用默认预设；历史 `xuwe` 值迁移为内置
    /// `nexora`。仍然有效的持久化值（包括显式选择的 `nexora`）保持不变。
    pub fn resolve_preset_id(&self, persisted_id: Option<&str>) -> &str {
        match persisted_id {
            Some(LEGACY_XUWE_THEME_PRESET_ID) => NEXORA_THEME_PRESET_ID,
            Some(preset_id) => self
                .preset(preset_id)
                .map(|preset| preset.metadata.id())
                .unwrap_or_else(|| self.default_preset_id()),
            None => self.default_preset_id(),
        }
    }

    fn preset(&self, preset_id: &str) -> Option<&RegisteredThemePreset> {
        self.presets
            .iter()
            .find(|preset| preset.metadata.id() == preset_id)
    }

    fn themes(&self, preset_id: &str) -> Option<(Rc<ThemeConfig>, Rc<ThemeConfig>)> {
        self.preset(preset_id)
            .map(|preset| (preset.light.clone(), preset.dark.clone()))
    }
}

impl Default for ThemeCatalog {
    fn default() -> Self {
        Self::new(&[], None).expect("内置 Nexora 主题必须通过主题目录校验")
    }
}

/// 构建应用主题目录时可能发生的结构化配置错误。
#[derive(Debug, Error)]
pub enum ThemeCatalogError {
    /// 下游预设 ID 不符合稳定 ASCII snake_case 约束。
    #[error("主题预设 ID `{id}` 必须匹配 [a-z][a-z0-9_]*")]
    InvalidId {
        /// 校验失败的原始预设 ID。
        id: String,
    },
    /// 下游预设使用了框架或历史迁移保留 ID。
    #[error("主题预设 ID `{id}` 由 Nexora 保留")]
    ReservedId {
        /// 发生冲突的预设 ID。
        id: String,
    },
    /// 同一个应用注册了重复预设 ID。
    #[error("主题预设 ID `{id}` 重复注册")]
    DuplicateId {
        /// 重复出现的预设 ID。
        id: String,
    },
    /// 预设没有可供设置页展示的名称。
    #[error("主题预设 `{id}` 的显示名称不能为空")]
    EmptyLabel {
        /// 显示名称为空的预设 ID。
        id: String,
    },
    /// 预设 JSON 不是合法的 gpui-component ThemeSet。
    #[error("主题预设 `{id}` 的 JSON 无效：{source}")]
    InvalidJson {
        /// JSON 所属的预设 ID。
        id: String,
        /// serde 返回的解析错误。
        #[source]
        source: serde_json::Error,
    },
    /// 预设包含的主题数量不是严格的浅色与深色两项。
    #[error("主题预设 `{id}` 必须恰好包含 2 个主题，实际为 {actual}")]
    InvalidThemeCount {
        /// 配对失败的预设 ID。
        id: String,
        /// JSON 中实际声明的主题数量。
        actual: usize,
    },
    /// 预设为同一颜色模式声明了多个主题。
    #[error("主题预设 `{id}` 重复声明 `{mode}` 模式")]
    DuplicateMode {
        /// 配对失败的预设 ID。
        id: String,
        /// 重复出现的模式名称。
        mode: &'static str,
    },
    /// 预设缺少浅色或深色模式。
    #[error("主题预设 `{id}` 缺少 `{mode}` 模式")]
    MissingMode {
        /// 配对失败的预设 ID。
        id: String,
        /// 缺少的模式名称。
        mode: &'static str,
    },
    /// 应用默认主题没有出现在已注册目录中。
    #[error("应用默认主题预设 `{id}` 未注册")]
    UnknownDefaultPreset {
        /// 无法解析的默认预设 ID。
        id: String,
    },
    /// 已校验的主题配置无法序列化为 gpui-component 注册表输入。
    #[error("无法构建 gpui-component 主题注册表：{source}")]
    Serialize {
        /// serde 返回的序列化错误。
        #[source]
        source: serde_json::Error,
    },
}

impl ThemeCatalogError {
    /// 返回与错误直接相关的主题预设 ID；注册表序列化错误没有单一预设 ID。
    pub fn preset_id(&self) -> Option<&str> {
        match self {
            Self::InvalidId { id }
            | Self::ReservedId { id }
            | Self::DuplicateId { id }
            | Self::EmptyLabel { id }
            | Self::InvalidJson { id, .. }
            | Self::InvalidThemeCount { id, .. }
            | Self::DuplicateMode { id, .. }
            | Self::MissingMode { id, .. }
            | Self::UnknownDefaultPreset { id } => Some(id),
            Self::Serialize { .. } => None,
        }
    }
}

fn validate_custom_preset_id(id: &str) -> Result<(), ThemeCatalogError> {
    if !is_valid_preset_id(id) {
        return Err(ThemeCatalogError::InvalidId { id: id.to_owned() });
    }
    if matches!(
        id,
        NEXORA_THEME_PRESET_ID | NEUTRAL_THEME_PRESET_ID | LEGACY_XUWE_THEME_PRESET_ID
    ) {
        return Err(ThemeCatalogError::ReservedId { id: id.to_owned() });
    }
    Ok(())
}

fn is_valid_preset_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn parse_theme_preset(
    id: &str,
    label: &str,
    json: &str,
) -> Result<RegisteredThemePreset, ThemeCatalogError> {
    let ThemeSet { themes, .. } =
        serde_json::from_str(json).map_err(|source| ThemeCatalogError::InvalidJson {
            id: id.to_owned(),
            source,
        })?;
    if themes.len() != 2 {
        return Err(ThemeCatalogError::InvalidThemeCount {
            id: id.to_owned(),
            actual: themes.len(),
        });
    }

    let mut light = None;
    let mut dark = None;
    for mut config in themes {
        let (slot, mode_name) = match config.mode {
            ThemeMode::Light => (&mut light, "light"),
            ThemeMode::Dark => (&mut dark, "dark"),
        };
        if slot.is_some() {
            return Err(ThemeCatalogError::DuplicateMode {
                id: id.to_owned(),
                mode: mode_name,
            });
        }
        config.name = internal_theme_name(id, config.mode).into();
        *slot = Some(Rc::new(config));
    }

    Ok(RegisteredThemePreset {
        metadata: ThemePresetMetadata {
            id: id.to_owned().into(),
            label: label.to_owned().into(),
        },
        light: light.ok_or_else(|| ThemeCatalogError::MissingMode {
            id: id.to_owned(),
            mode: "light",
        })?,
        dark: dark.ok_or_else(|| ThemeCatalogError::MissingMode {
            id: id.to_owned(),
            mode: "dark",
        })?,
    })
}

fn internal_theme_name(id: &str, mode: ThemeMode) -> String {
    format!("__nexora_{id}_{}", mode.name())
}

/// 应用颜色模式。
///
/// 颜色模式和具体主题预设相互独立：模式决定当前使用浅色还是深色主题，预设则决定
/// 两种模式分别对应哪一组主题 token。
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
    /// 无法识别的标识会返回 `None`，调用方可以安全回退到跟随系统。
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scheme| scheme.id() == id)
    }
}

/// 当前应用的完整主题选择。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThemeSelection {
    preset_id: SharedString,
    color_scheme: ColorScheme,
}

impl ThemeSelection {
    /// 使用指定稳定预设 ID 和颜色模式创建完整选择。
    pub fn new(preset_id: impl Into<SharedString>, color_scheme: ColorScheme) -> Self {
        Self {
            preset_id: preset_id.into(),
            color_scheme,
        }
    }

    /// 返回当前选择的稳定主题预设 ID。
    pub fn preset_id(&self) -> &str {
        self.preset_id.as_ref()
    }

    /// 返回当前选择的颜色模式。
    pub const fn color_scheme(&self) -> ColorScheme {
        self.color_scheme
    }
}

impl Default for ThemeSelection {
    fn default() -> Self {
        Self::new(NEXORA_THEME_PRESET_ID, ColorScheme::System)
    }
}

/// 运行时切换主题预设时可能发生的错误。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThemeSelectionError {
    /// 请求的稳定 ID 没有注册到当前应用主题目录。
    #[error("主题预设 `{id}` 未注册")]
    UnknownPreset {
        /// 调用方请求切换到的原始主题预设 ID。
        id: String,
    },
}

#[derive(Debug, Clone)]
struct ThemeState {
    catalog: ThemeCatalog,
    selection: ThemeSelection,
    font_size: u16,
    component_size: Size,
}

impl ThemeState {
    fn new(catalog: ThemeCatalog) -> Self {
        let selection = ThemeSelection::new(catalog.default_preset_id(), ColorScheme::System);
        Self {
            catalog,
            selection,
            font_size: DEFAULT_FONT_SIZE,
            component_size: DEFAULT_COMPONENT_SIZE,
        }
    }
}

impl Global for ThemeState {}

/// 使用内置 Nexora 主题初始化应用主题。
///
/// 调用方必须先执行 `gpui_component::init`。需要安装下游主题时改用
/// [`init_with_catalog`]。
pub fn init(cx: &mut App) {
    init_with_catalog(ThemeCatalog::default(), cx);
}

/// 使用已校验的应用主题目录初始化主题运行时。
///
/// 调用方必须先执行 `gpui_component::init`。本函数会把目录中的规范化主题注册到
/// gpui-component，安装唯一主题状态，并立即应用目录声明的默认主题。
///
/// # Panics
///
/// 早于 `gpui_component::init` 调用，或已校验目录无法装载到 ThemeRegistry 时会 panic；
/// 后一种情况表示框架内部序列化契约被破坏。
pub fn init_with_catalog(catalog: ThemeCatalog, cx: &mut App) {
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(&catalog.registry_json)
        .expect("已校验的应用主题目录必须能够装载到 gpui-component");

    let state = ThemeState::new(catalog);
    if cx.has_global::<ThemeState>() {
        *cx.global_mut::<ThemeState>() = state;
    } else {
        cx.set_global(state);
    }

    apply_active_theme(None, cx);
}

/// 把主题管理器附着到一个新创建的窗口。
///
/// 该函数会先按窗口的实际外观同步一次主题，再监听后续系统外观变化。只有颜色模式为
/// [`ColorScheme::System`] 时，系统外观变化才会触发当前预设的浅色与深色主题切换。
///
/// # Panics
///
/// 在 [`init`] 或 [`init_with_catalog`] 之前调用时会因为全局主题状态尚未创建而 panic。
pub fn attach_window(window: &mut Window, cx: &mut App) {
    apply_active_theme(Some(window), cx);

    window
        .observe_window_appearance(|window, cx| {
            if selection(cx).color_scheme() == ColorScheme::System {
                apply_active_theme(Some(window), cx);
            }
        })
        // nexora-lint: allow(nexora::detached_lifecycle) reason="监听由窗口持有并随窗口销毁"
        .detach();
}

/// 返回当前应用的完整主题选择。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn selection(cx: &App) -> ThemeSelection {
    cx.global::<ThemeState>().selection.clone()
}

/// 返回当前应用已注册的全部主题预设元数据。
///
/// 返回顺序与默认设置页一致：内置 Nexora 在前，下游主题按注册顺序排列。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn presets(cx: &App) -> impl ExactSizeIterator<Item = &ThemePresetMetadata> {
    cx.global::<ThemeState>().catalog.presets()
}

/// 返回当前应用声明的默认主题预设 ID。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn default_preset_id(cx: &App) -> &str {
    cx.global::<ThemeState>().catalog.default_preset_id()
}

/// 判断当前应用主题目录是否包含指定稳定 ID。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn contains_preset(preset_id: &str, cx: &App) -> bool {
    cx.global::<ThemeState>().catalog.contains(preset_id)
}

/// 按启动优先级把可选的持久化预设 ID 解析为当前目录中的稳定 ID。
///
/// `None` 表示首次启动并使用应用默认值；有效的已保存值优先于应用默认值，历史
/// `xuwe` 值迁移为内置 `nexora`，其他未知值回退到应用默认值。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn resolve_preset_id<'a>(persisted_id: Option<&str>, cx: &'a App) -> &'a str {
    cx.global::<ThemeState>()
        .catalog
        .resolve_preset_id(persisted_id)
}

/// 返回当前应用界面的基础字号，单位为逻辑像素。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn font_size(cx: &App) -> u16 {
    cx.global::<ThemeState>().font_size
}

/// 返回支持 `with_size` 的应用组件当前应使用的统一尺寸。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn component_size(cx: &App) -> Size {
    cx.global::<ThemeState>().component_size
}

/// 一次性更新主题预设与颜色模式并刷新全部窗口。
///
/// 当新选择与当前选择相同时不会重复应用主题。
///
/// # Errors
///
/// `new_selection` 的预设 ID 没有注册到当前应用主题目录时返回
/// [`ThemeSelectionError::UnknownPreset`]，当前主题保持不变。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn set_selection(
    new_selection: ThemeSelection,
    cx: &mut App,
) -> Result<(), ThemeSelectionError> {
    if !contains_preset(new_selection.preset_id(), cx) {
        return Err(ThemeSelectionError::UnknownPreset {
            id: new_selection.preset_id().to_owned(),
        });
    }
    if selection(cx) == new_selection {
        return Ok(());
    }

    cx.global_mut::<ThemeState>().selection = new_selection;
    apply_active_theme(None, cx);
    Ok(())
}

/// 更新主题预设，并保留当前颜色模式。
///
/// # Errors
///
/// `preset_id` 没有注册到当前应用主题目录时返回结构化错误，当前主题保持不变。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn set_preset(preset_id: &str, cx: &mut App) -> Result<(), ThemeSelectionError> {
    let current = selection(cx);
    set_selection(
        ThemeSelection::new(preset_id.to_owned(), current.color_scheme()),
        cx,
    )
}

/// 更新颜色模式，并保留当前主题预设。
///
/// 选择跟随系统时会读取平台当前外观；选择浅色或深色时会固定使用当前预设对应的主题。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
pub fn set_color_scheme(color_scheme: ColorScheme, cx: &mut App) {
    let current = selection(cx);
    set_selection(
        ThemeSelection::new(current.preset_id().to_owned(), color_scheme),
        cx,
    )
    .expect("当前主题选择必须来自已注册目录");
}

/// 更新应用界面的基础字号，并刷新所有窗口。
///
/// 传入值会被限制在 [`MIN_FONT_SIZE`] 和 [`MAX_FONT_SIZE`] 之间。该设置只修改当前
/// 运行时主题状态，调用方需要自行决定是否持久化到用户配置。
///
/// # Panics
///
/// 在主题运行时初始化前调用时会 panic。
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
/// 在主题运行时初始化前调用时会 panic。
pub fn set_component_size(new_component_size: Size, cx: &mut App) {
    if component_size(cx) == new_component_size {
        return;
    }

    cx.global_mut::<ThemeState>().component_size = new_component_size;
    cx.refresh_windows();
}

fn apply_active_theme(window: Option<&mut Window>, cx: &mut App) {
    let selection = selection(cx);
    let (light_theme, dark_theme) = cx
        .global::<ThemeState>()
        .catalog
        .themes(selection.preset_id())
        .expect("当前主题选择必须来自已注册目录");

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

    Theme::set_scrollbar_mode(ScrollbarMode::Hover, cx);
    apply_font_size(cx);
    cx.refresh_windows();
}

fn apply_font_size(cx: &mut App) {
    Theme::global_mut(cx).font_size = px(f32::from(font_size(cx)));
}
