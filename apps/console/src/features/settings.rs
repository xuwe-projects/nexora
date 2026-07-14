//! Console 桌面应用设置功能模块。
//!
//! 该模块展示桌面应用常见设置项的页面结构，用于承载后续偏好配置和运行时开关。

use std::collections::HashSet;

use crate::config;
use actions::settings::OpenSettings;
use changelog::{ChangelogEntry, ChangelogError, EmbeddedChangelogRepository};
use gpui::{
    AnyElement, App, Axis, Context, Global, IntoElement, ParentElement as _, Render, SharedString,
    StyleRefinement, Subscription, Window, WindowHandle, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    group_box::GroupBoxVariant,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    tag::Tag,
    text::TextView,
};
use theme::{ColorScheme, ThemePreset};
use updater::UpdateConfig;

#[cfg(target_os = "macos")]
const MACOS_TITLE_BAR_HEIGHT: gpui::Pixels = px(34.0);

const SYSTEM_PRIMARY_DISPLAY: &str = "system-primary-display";
const NIL_DISPLAY_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// 应用级设置窗口状态。
///
/// 该状态通过 GPUI `Global` 保存当前设置窗口的弱生命周期句柄，让菜单和快捷键无论从哪个窗口触发，
/// 都能激活同一个设置窗口；窗口关闭后句柄会自然失效，下一次触发时再创建新窗口。
#[derive(Default)]
struct SettingsWindowState {
    window: Option<WindowHandle<gpui_component::Root>>,
    updater_config: Option<UpdateConfig>,
}

impl Global for SettingsWindowState {}

/// 初始化独立设置窗口的全局 action 处理器。
///
/// 该函数应在应用创建主窗口时调用一次。处理器使用 `App::on_action` 监听 `OpenSettings`，
/// 因此 Sidebar 弹出菜单和跨平台快捷键会进入完全相同的窗口打开流程。
pub fn init(updater_config: Option<UpdateConfig>, cx: &mut App) {
    if cx.has_global::<SettingsWindowState>() {
        return;
    }

    cx.set_global(SettingsWindowState {
        window: None,
        updater_config,
    });
    cx.on_action(|_: &OpenSettings, cx| open_settings_window(cx));
}

fn open_settings_window(cx: &mut App) {
    let existing_window = cx.global::<SettingsWindowState>().window;
    if let Some(existing_window) = existing_window
        && existing_window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return;
    }

    cx.global_mut::<SettingsWindowState>().window = None;
    let window_options = settings_window_options(config::startup_display_uuid(cx), cx);
    #[cfg(target_os = "windows")]
    let target_display_id = window_options
        .display_id
        .or_else(|| cx.primary_display().map(|display| display.id()));

    match cx.open_window(window_options, move |window, cx| {
        #[cfg(target_os = "windows")]
        if let Some(target_display_id) = target_display_id
            && let Err(error) = desktop::center_window_on_display(window, target_display_id)
        {
            eprintln!("无法在目标显示器上居中设置窗口: {error}");
        }
        let settings = cx.new(SettingsWindow::new);
        let root = cx.new(|cx| gpui_component::Root::new(settings, window, cx));
        theme::attach_window(window, cx);
        root
    }) {
        Ok(settings_window) => {
            cx.global_mut::<SettingsWindowState>().window = Some(settings_window);
            cx.activate(true);
            _ = settings_window.update(cx, |_, window, _| window.activate_window());
        }
        Err(error) => eprintln!("无法打开设置窗口: {error:#}"),
    }
}

/// 根据当前显示器偏好创建设置窗口的原生选项。
///
/// 提供稳定 UUID 时，窗口会在对应显示器上以 `860 × 680` 居中；显示器不可用时回退
/// 系统主显示器。该函数也集中维护设置窗口的最小尺寸和标题栏配置。
pub(crate) fn settings_window_options(display_uuid: Option<&str>, cx: &App) -> WindowOptions {
    let window_size = size(px(860.0), px(680.0));
    let mut window_options = WindowOptions {
        window_min_size: Some(size(px(680.0), px(520.0))),
        titlebar: Some(TitleBar::title_bar_options()),
        ..Default::default()
    };
    desktop::apply_window_display_preference(
        &mut window_options,
        display_uuid,
        Some(window_size),
        cx,
    );
    window_options
}

/// 应用设置功能视图。
///
/// 当前实现包含可交互且会本地持久化的主题、启动显示器设置，以及面向用户的版本和更新信息。
pub struct SettingsFeature;

impl SettingsFeature {
    /// 渲染设置页面。
    ///
    /// 页面首先展示可立即生效的主题设置与新窗口默认显示器，随后展示版本信息和本次更新内容。
    /// 用户偏好从应用级内存状态读取，交互变更会立即写入当前操作系统用户的本地配置文件。
    pub fn render<T>(cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let header_style = settings_header_style();
        let updater_config = cx.global::<SettingsWindowState>().updater_config.clone();

        Settings::new("console-settings")
            .header_style(&header_style)
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(
                std::iter::once(theme_setting_page())
                    .chain(std::iter::once(window_setting_page(cx)))
                    .chain(std::iter::once(update_setting_page(updater_config))),
            )
            .into_any_element()
    }
}

/// 独立原生窗口中使用的设置根视图。
///
/// 该视图只负责为 `SettingsFeature` 提供全窗口尺寸和主题背景，设置项本身仍由 feature 模块维护，
/// 并继续使用 `gpui-component` 的 `Settings`、`SettingPage` 和 `SettingField` 组件。
pub struct SettingsWindow {
    _preferences_subscription: Subscription,
}

impl SettingsWindow {
    /// 创建独立设置窗口视图，并观察后续用户偏好变化以局部刷新当前窗口。
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _preferences_subscription: config::observe_preferences(cx),
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(SettingsFeature::render(cx));

        if cfg!(target_os = "macos") {
            return div()
                .relative()
                .flex()
                .flex_col()
                .size_full()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .bg(cx.theme().tokens.background)
                .child(content)
                .when(!window.is_fullscreen(), |this| {
                    this.child(
                        TitleBar::new()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .border_b(px(0.0))
                            .bg(gpui::transparent_black()),
                    )
                })
                .into_any_element();
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(cx.theme().tokens.background)
            .child(TitleBar::new())
            .child(content)
            .into_any_element()
    }
}

/// 返回当前 Console 版本对应的中文更新日志。
///
/// 查询使用 workspace 包版本、`console` 组件标识和 `zh-CN` 语言标识。找到日志时返回
/// 克隆后的轻量条目；当前版本尚未添加 Markdown 时返回 `Ok(None)`。
///
/// # Errors
///
/// 当编译进应用的任意更新日志路径、版本号或 UTF-8 内容不符合 `changelog` crate 约定时，
/// 返回 [`ChangelogError`]。
pub fn current_console_changelog() -> Result<Option<ChangelogEntry>, ChangelogError> {
    let repository = EmbeddedChangelogRepository::load()?;

    Ok(repository
        .entries()
        .iter()
        .find(|entry| {
            entry.component() == "console"
                && entry.locale() == "zh-CN"
                && entry.version().to_string() == env!("CARGO_PKG_VERSION")
        })
        .cloned())
}

fn theme_setting_page() -> SettingPage {
    SettingPage::new("外观")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::Palette))
        .description("切换应用主题预设与浅色、深色显示模式。")
        .default_open(true)
        .resettable(true)
        .group(
            SettingGroup::new().title("主题").items([
                SettingItem::new(
                    "主题预设",
                    SettingField::dropdown(
                        ThemePreset::ALL
                            .into_iter()
                            .map(|preset| {
                                (
                                    SharedString::from(preset.id()),
                                    SharedString::from(preset.label()),
                                )
                            })
                            .collect(),
                        |cx: &App| SharedString::from(theme::selection(cx).preset().id()),
                        move |value: SharedString, cx: &mut App| {
                            if let Some(preset) = ThemePreset::from_id(value.as_ref()) {
                                theme::set_preset(preset, cx);
                                config::persist_theme_selection(theme::selection(cx), cx);
                            }
                        },
                    )
                    .default_value(SharedString::from(ThemePreset::default().id())),
                )
                .description("决定应用在浅色和深色模式下使用的配色风格。"),
                SettingItem::new(
                    "颜色模式",
                    SettingField::dropdown(
                        ColorScheme::ALL
                            .into_iter()
                            .map(|scheme| {
                                (
                                    SharedString::from(scheme.id()),
                                    SharedString::from(scheme.label()),
                                )
                            })
                            .collect(),
                        |cx: &App| SharedString::from(theme::selection(cx).color_scheme().id()),
                        move |value: SharedString, cx: &mut App| {
                            if let Some(scheme) = ColorScheme::from_id(value.as_ref()) {
                                theme::set_color_scheme(scheme, cx);
                                config::persist_theme_selection(theme::selection(cx), cx);
                            }
                        },
                    )
                    .default_value(SharedString::from(ColorScheme::default().id())),
                )
                .description("跟随系统会在操作系统外观变化时自动切换浅色或深色主题。"),
            ]),
        )
}

fn window_setting_page(cx: &App) -> SettingPage {
    let display_options = startup_display_options(cx);

    SettingPage::new("窗口")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::LayoutDashboard))
        .description("设置主窗口启动与新窗口打开时使用的显示器。")
        .default_open(true)
        .resettable(true)
        .group(
            SettingGroup::new()
                .title("启动位置")
                .item(startup_display_setting_item(display_options)),
        )
}

/// 创建使用纵向布局的默认显示器设置项。
///
/// 纵向布局让官方下拉控件独占整行，避免显示器名称和分辨率较长时被设置项右边界裁切。
pub(crate) fn startup_display_setting_item(
    display_options: Vec<(SharedString, SharedString)>,
) -> SettingItem {
    SettingItem::new(
        "默认显示器",
        SettingField::dropdown(
            display_options,
            |cx: &App| {
                SharedString::from(
                    config::startup_display_uuid(cx).unwrap_or(SYSTEM_PRIMARY_DISPLAY),
                )
            },
            |value: SharedString, cx: &mut App| {
                let display_uuid =
                    (value.as_ref() != SYSTEM_PRIMARY_DISPLAY).then(|| value.to_string());
                config::persist_startup_display_uuid(display_uuid, cx);
            },
        )
        .default_value(SharedString::from(SYSTEM_PRIMARY_DISPLAY)),
    )
    .layout(Axis::Vertical)
    .description("用于主窗口下次启动及之后新打开的窗口；显示器未连接时会临时回退到系统主显示器。")
}

fn startup_display_options(cx: &App) -> Vec<(SharedString, SharedString)> {
    let primary_display_id = cx.primary_display().map(|display| display.id());
    let mut known_uuids = HashSet::new();
    let mut options = vec![(
        SharedString::from(SYSTEM_PRIMARY_DISPLAY),
        SharedString::from("跟随系统主显示器"),
    )];

    options.extend(
        cx.displays()
            .into_iter()
            .enumerate()
            .filter_map(|(index, display)| {
                let uuid = display.uuid().ok()?.to_string();
                if uuid == NIL_DISPLAY_UUID || !known_uuids.insert(uuid.clone()) {
                    return None;
                }

                let bounds = display.bounds();
                let primary_suffix = if primary_display_id == Some(display.id()) {
                    " · 当前主显示器"
                } else {
                    ""
                };
                let label = format!(
                    "显示器 {}（{} × {}）{}",
                    index + 1,
                    u32::from(bounds.size.width),
                    u32::from(bounds.size.height),
                    primary_suffix,
                );
                Some((SharedString::from(uuid), SharedString::from(label)))
            }),
    );

    if let Some(saved_uuid) = config::startup_display_uuid(cx)
        && !known_uuids.contains(saved_uuid)
    {
        options.push((
            SharedString::from(saved_uuid.to_owned()),
            SharedString::from("上次选择的显示器（当前未连接）"),
        ));
    }

    options
}

fn update_setting_page(updater_config: Option<UpdateConfig>) -> SettingPage {
    let changelog_item = match current_console_changelog() {
        Ok(Some(entry)) => SettingItem::render(move |_, _, _| {
            TextView::markdown("settings-current-changelog", entry.markdown()).selectable(true)
        })
        .keywords(["更新日志", "版本记录", "changelog"]),
        Ok(None) | Err(_) => SettingItem::render(|_, _, cx| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("当前版本暂无更新说明。")
        })
        .keywords(["更新日志", "版本记录"]),
    };

    let version_label = updater_config
        .as_ref()
        .map(|config| {
            format!(
                "v{} ({})",
                config.current_version(),
                config.current_bundle_version()
            )
        })
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    let online_update_items = updater_config.map(|config| {
        let label = match config.channel() {
            updater::UpdateChannel::Stable => "稳定版",
            updater::UpdateChannel::Beta => "测试版",
            updater::UpdateChannel::Nightly => "预览版",
        };
        let check_config = config.clone();

        [
            SettingItem::new(
                "更新通道",
                SettingField::render(move |options, _, _| {
                    Tag::secondary()
                        .with_size(options.size)
                        .outline()
                        .child(label)
                }),
            ),
            SettingItem::new(
                "检查更新",
                SettingField::render(move |options, _window, _cx| {
                    let config = check_config.clone();
                    Button::new("settings-check-update")
                        .with_size(options.size)
                        .label("检查更新")
                        .primary()
                        .on_click(move |_, window, cx| {
                            updater::open_update_dialog(config.clone(), window, cx);
                        })
                }),
            )
            .description("检查当前更新通道的最新版本，并在应用内完成下载和安装。"),
        ]
    });
    let version_items = std::iter::once(SettingItem::new(
        "当前版本",
        SettingField::render(move |options, _, _| {
            Tag::secondary()
                .with_size(options.size)
                .outline()
                .child(version_label.clone())
        }),
    ))
    .chain(online_update_items.into_iter().flatten());

    SettingPage::new("更新")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::BookOpen))
        .description("查看当前版本、检查更新并了解本次版本的新变化。")
        .default_open(false)
        .resettable(false)
        .group(SettingGroup::new().title("版本信息").items(version_items))
        .group(SettingGroup::new().title("本次更新").item(changelog_item))
}

fn settings_header_style() -> StyleRefinement {
    #[cfg(target_os = "macos")]
    return StyleRefinement::default().pt(MACOS_TITLE_BAR_HEIGHT);

    #[cfg(not(target_os = "macos"))]
    StyleRefinement::default()
}
