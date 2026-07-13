//! Console 桌面应用设置功能模块。
//!
//! 该模块展示桌面应用常见设置项的页面结构，用于承载后续偏好配置和运行时开关。

use std::collections::HashSet;

use crate::config;
use actions::settings::OpenSettings;
use changelog::{ChangelogEntry, ChangelogError, EmbeddedChangelogRepository};
use gpui::{
    AnyElement, App, Context, Global, IntoElement, ParentElement as _, Render, SharedString,
    StyleRefinement, Subscription, Window, WindowBounds, WindowHandle, WindowOptions, div,
    prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
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
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(860.0), px(680.0)), cx)),
        window_min_size: Some(size(px(680.0), px(520.0))),
        titlebar: Some(TitleBar::title_bar_options()),
        ..Default::default()
    };

    match cx.open_window(window_options, |window, cx| {
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

/// 应用设置功能视图。
///
/// 当前实现包含可交互且会本地持久化的主题、启动显示器设置，以及只读模板配置和更新信息。
pub struct SettingsFeature;

impl SettingsFeature {
    /// 渲染设置页面。
    ///
    /// 页面首先展示可立即生效的主题设置与下次启动显示器，随后展示后台模式、打包配置和更新信息。
    /// 用户偏好从应用级内存状态读取，交互变更会立即写入当前操作系统用户的本地配置文件。
    pub fn render<T>(cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let header_style = settings_header_style();
        let updater_config = cx.global::<SettingsWindowState>().updater_config.clone();

        Settings::new("console-settings")
            .header_style(&header_style)
            .pages(
                std::iter::once(theme_setting_page())
                    .chain(std::iter::once(window_setting_page(cx)))
                    .chain(
                        setting_groups()
                            .iter()
                            .copied()
                            .filter(|group| group.title() != "窗口")
                            .map(setting_page),
                    )
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

/// 设置页中的一个静态分组。
///
/// 真实应用可以把该类型替换为持久化配置、运行时状态或偏好设置模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingGroupData {
    title: &'static str,
    items: &'static [(&'static str, &'static str)],
}

impl SettingGroupData {
    /// 返回设置分组标题。
    ///
    /// 标题会显示在分组容器顶部，用于区分窗口、运行和发布等配置域。
    pub fn title(self) -> &'static str {
        self.title
    }

    /// 返回该分组中的设置项。
    ///
    /// 每个元组的第一项是设置名称，第二项是当前展示值。
    pub fn items(self) -> &'static [(&'static str, &'static str)] {
        self.items
    }
}

/// 返回设置页默认展示的模板配置分组。
///
/// 返回值顺序就是页面渲染顺序，用于稳定展示窗口、运行和发布三个配置区域。
pub fn setting_groups() -> &'static [SettingGroupData] {
    static WINDOW_ITEMS: [(&str, &str); 3] = [
        ("默认尺寸", "900 x 640"),
        ("最小尺寸", "900 x 640"),
        ("启动激活", "开启"),
    ];
    static RUNTIME_ITEMS: [(&str, &str); 3] = [
        ("守护模式", "关闭"),
        ("默认目标", "aarch64-apple-darwin"),
        ("打包输出", "dist/"),
    ];
    static RELEASE_ITEMS: [(&str, &str); 3] = [
        ("本地签名", "ad-hoc"),
        ("公证 profile", "xuwe"),
        ("校验文件", ".sha256"),
    ];
    static GROUPS: [SettingGroupData; 3] = [
        SettingGroupData {
            title: "窗口",
            items: &WINDOW_ITEMS,
        },
        SettingGroupData {
            title: "运行",
            items: &RUNTIME_ITEMS,
        },
        SettingGroupData {
            title: "发布",
            items: &RELEASE_ITEMS,
        },
    ];

    &GROUPS
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
                .description("同时定义应用在浅色和深色模式下使用的颜色 token。"),
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
    let window_group = setting_groups()
        .iter()
        .copied()
        .find(|group| group.title() == "窗口")
        .expect("设置模板必须包含窗口分组");

    SettingPage::new("窗口")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::LayoutDashboard))
        .description("设置主窗口下次启动时使用的显示器，并查看窗口默认参数。")
        .default_open(true)
        .resettable(true)
        .group(
            SettingGroup::new().title("启动位置").item(
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
                            let display_uuid = (value.as_ref() != SYSTEM_PRIMARY_DISPLAY)
                                .then(|| value.to_string());
                            config::persist_startup_display_uuid(display_uuid, cx);
                        },
                    )
                    .default_value(SharedString::from(SYSTEM_PRIMARY_DISPLAY)),
                )
                .description("保存后在下次启动生效；显示器未连接时会临时回退到系统主显示器。"),
            ),
        )
        .group(
            SettingGroup::new()
                .title("当前配置")
                .items(window_group.items().iter().map(|(label, value)| {
                    let value = *value;
                    SettingItem::new(
                        *label,
                        SettingField::render(move |options, _, _| {
                            Tag::secondary()
                                .with_size(options.size)
                                .outline()
                                .child(value)
                        }),
                    )
                })),
        )
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

fn setting_page(group: SettingGroupData) -> SettingPage {
    let (icon, description) = match group.title() {
        "窗口" => (
            IconName::LayoutDashboard,
            "窗口尺寸、激活行为和原生窗口配置。",
        ),
        "运行" => (IconName::SquareTerminal, "后台常驻、构建目标和产物目录。"),
        "发布" => (IconName::Globe, "签名、公证和产物校验配置。"),
        _ => (IconName::Settings2, "应用运行时配置。"),
    };

    SettingPage::new(group.title())
        .header_style(&settings_header_style())
        .icon(Icon::new(icon))
        .description(description)
        .default_open(group.title() == "窗口")
        .resettable(false)
        .group(
            SettingGroup::new()
                .title("当前配置")
                .items(group.items().iter().map(|(label, value)| {
                    let value = *value;
                    SettingItem::new(
                        *label,
                        SettingField::render(move |options, _, _| {
                            Tag::secondary()
                                .with_size(options.size)
                                .outline()
                                .child(value)
                        }),
                    )
                })),
        )
}

fn update_setting_page(updater_config: Option<UpdateConfig>) -> SettingPage {
    let changelog_item = match current_console_changelog() {
        Ok(Some(entry)) => SettingItem::render(move |_, _, _| {
            TextView::markdown("settings-current-changelog", entry.markdown()).selectable(true)
        })
        .keywords(["更新日志", "版本记录", "changelog"]),
        Ok(None) => SettingItem::render(|_, _, _| {
            gpui_component::alert::Alert::warning(
                "settings-current-changelog-missing",
                "当前版本缺少更新日志，请添加 changelogs/<version>/console/zh-CN.md。",
            )
        })
        .keywords(["更新日志", "版本记录", "changelog"]),
        Err(error) => {
            let message = error.to_string();
            SettingItem::render(move |_, _, _| {
                gpui_component::alert::Alert::error(
                    "settings-current-changelog-invalid",
                    message.clone(),
                )
            })
            .keywords(["更新日志", "版本记录", "changelog"])
        }
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
    let channel_label = updater_config
        .as_ref()
        .map(|config| match config.channel() {
            updater::UpdateChannel::Stable => "稳定版",
            updater::UpdateChannel::Beta => "测试版",
            updater::UpdateChannel::Nightly => "每日构建",
        })
        .unwrap_or("未配置");
    let check_config = updater_config.clone();
    let check_item = SettingItem::new(
        "检查更新",
        SettingField::render(move |options, _window, _cx| {
            let config = check_config.clone();
            Button::new("settings-check-update")
                .with_size(options.size)
                .label("检查更新")
                .primary()
                .disabled(config.is_none())
                .on_click(move |_, window, cx| {
                    if let Some(config) = config.clone() {
                        updater::open_update_dialog(config, window, cx);
                    }
                })
        }),
    )
    .description(if updater_config.is_some() {
        "检查当前更新通道的最新版本，并在应用内完成下载和安装。"
    } else {
        "发布构建需要设置 UPDATE_MANIFEST_URL 后才能启用在线更新。"
    });

    SettingPage::new("更新")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::BookOpen))
        .description("查看当前版本、更新通道和随应用发布的更新日志。")
        .default_open(false)
        .resettable(false)
        .group(
            SettingGroup::new().title("版本信息").items([
                SettingItem::new(
                    "当前版本",
                    SettingField::render(move |options, _, _| {
                        Tag::secondary()
                            .with_size(options.size)
                            .outline()
                            .child(version_label.clone())
                    }),
                ),
                SettingItem::new(
                    "更新通道",
                    SettingField::render(move |options, _, _| {
                        Tag::secondary()
                            .with_size(options.size)
                            .outline()
                            .child(channel_label)
                    }),
                )
                .description("稳定版、测试版和每日构建分别使用独立的 latest.json。"),
                check_item,
            ]),
        )
        .group(
            SettingGroup::new()
                .title("当前版本更新日志")
                .description("内容来自 changelogs/<version>/console/zh-CN.md。")
                .item(changelog_item),
        )
}

fn settings_header_style() -> StyleRefinement {
    #[cfg(target_os = "macos")]
    return StyleRefinement::default().pt(MACOS_TITLE_BAR_HEIGHT);

    #[cfg(not(target_os = "macos"))]
    StyleRefinement::default()
}
