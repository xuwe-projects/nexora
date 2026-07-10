//! 控制台应用设置功能模块。
//!
//! 该模块展示桌面应用常见设置项的页面结构，用于承载后续偏好配置和运行时开关。

use actions::settings::OpenSettings;
use gpui::{
    AnyElement, App, Context, Global, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, StyleRefinement, Window, WindowBounds, WindowHandle, WindowOptions, div,
    prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    tag::Tag,
};
use theme::{ColorScheme, ThemePreset};

const MACOS_TITLE_BAR_HEIGHT: Pixels = px(34.0);

/// 应用级设置窗口状态。
///
/// 该状态通过 GPUI `Global` 保存当前设置窗口的弱生命周期句柄，让菜单和快捷键无论从哪个窗口触发，
/// 都能激活同一个设置窗口；窗口关闭后句柄会自然失效，下一次触发时再创建新窗口。
#[derive(Default)]
struct SettingsWindowState {
    window: Option<WindowHandle<gpui_component::Root>>,
}

impl Global for SettingsWindowState {}

/// 初始化独立设置窗口的全局 action 处理器。
///
/// 该函数应在应用创建主窗口时调用一次。处理器使用 `App::on_action` 监听 `OpenSettings`，
/// 因此 Sidebar 弹出菜单和跨平台快捷键会进入完全相同的窗口打开流程。
pub fn init(cx: &mut App) {
    if cx.has_global::<SettingsWindowState>() {
        return;
    }

    cx.set_global(SettingsWindowState::default());
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
        let settings = cx.new(|_| SettingsWindow::new());
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
/// 当前实现包含可交互的主题设置和静态模板配置，用于说明设置 feature 可以独立管理
/// 运行时偏好、分组说明和当前值。
pub struct SettingsFeature;

impl SettingsFeature {
    /// 渲染设置页面。
    ///
    /// 页面首先展示可立即生效的主题设置，随后展示窗口、后台模式和打包配置等模板级设置。
    /// 主题控件直接调用共享 `theme` crate；其余静态项后续可以接入真实持久化配置。
    pub fn render<T>(_cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let header_style = settings_header_style();

        Settings::new("console-settings")
            .header_style(&header_style)
            .pages(
                std::iter::once(theme_setting_page())
                    .chain(setting_groups().iter().copied().map(setting_page)),
            )
            .into_any_element()
    }
}

/// 独立原生窗口中使用的设置根视图。
///
/// 该视图只负责为 `SettingsFeature` 提供全窗口尺寸和主题背景，设置项本身仍由 feature 模块维护，
/// 并继续使用 `gpui-component` 的 `Settings`、`SettingPage` 和 `SettingField` 组件。
#[derive(Debug, Default)]
pub struct SettingsWindow;

impl SettingsWindow {
    /// 创建一个新的独立设置窗口视图。
    pub fn new() -> Self {
        Self
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
                        |value: SharedString, cx: &mut App| {
                            if let Some(preset) = ThemePreset::from_id(value.as_ref()) {
                                theme::set_preset(preset, cx);
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
                        |value: SharedString, cx: &mut App| {
                            if let Some(scheme) = ColorScheme::from_id(value.as_ref()) {
                                theme::set_color_scheme(scheme, cx);
                            }
                        },
                    )
                    .default_value(SharedString::from(ColorScheme::default().id())),
                )
                .description("跟随系统会在操作系统外观变化时自动切换浅色或深色主题。"),
            ]),
        )
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

fn settings_header_style() -> StyleRefinement {
    #[cfg(target_os = "macos")]
    return StyleRefinement::default().pt(MACOS_TITLE_BAR_HEIGHT);

    #[cfg(not(target_os = "macos"))]
    StyleRefinement::default()
}
