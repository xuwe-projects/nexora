//! Nexora 桌面应用的默认设置窗口。

use gpui::{
    App, Axis, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    StyleRefinement, Subscription, Window, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size, TitleBar,
    group_box::GroupBoxVariant,
    h_flex,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
};
use theme::{ColorScheme, ThemePreset};

use crate::{
    __private::{SettingsWindowRegistration, WindowRegistration},
    NoPath, NoQuery, Window as WindowDefinition, WindowElement, WindowInstance, WindowMetadata,
    WindowRoute, WindowRuntimeError,
};

#[cfg(target_os = "macos")]
const MACOS_TITLE_BAR_HEIGHT: gpui::Pixels = px(34.0);

const SETTINGS_METADATA: WindowMetadata =
    WindowMetadata::new("settings", "设置", "/settings", Some("settings"), 0);

/// 创建框架默认设置窗口的回退注册记录。
///
/// 该记录只在应用没有声明 `#[derive(SettingsWindow)]` 覆盖时注入统一 Window 路由，
/// 不会提交到全局 `inventory`。
pub(crate) const fn default_settings_window_registration() -> SettingsWindowRegistration {
    SettingsWindowRegistration::new(
        "nexora::defaults::DefaultSettingsWindow",
        WindowRegistration::new(
            SETTINGS_METADATA,
            create_default_settings_window,
            default_settings_window_options,
        ),
    )
}

/// Nexora 桌面应用自带的设置窗口。
///
/// 默认窗口只承载所有桌面应用都具备的运行时外观能力，不包含更新服务、changelog、
/// Console 置顶标签或其他具体产品配置。
struct DefaultSettingsWindow {
    font_size_slider: Entity<SliderState>,
    _font_size_subscription: Subscription,
}

impl DefaultSettingsWindow {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current_font_size = theme::font_size(cx);
        let font_size_slider = cx.new(|_| {
            SliderState::new()
                .min(f32::from(theme::MIN_FONT_SIZE))
                .max(f32::from(theme::MAX_FONT_SIZE))
                .default_value(f32::from(current_font_size))
                .step(1.0)
        });
        let font_size_subscription =
            cx.subscribe(&font_size_slider, |_, _, event: &SliderEvent, cx| {
                let value = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => value,
                };
                theme::set_font_size(font_size_from_slider(value), cx);
            });

        Self {
            font_size_slider,
            _font_size_subscription: font_size_subscription,
        }
    }
}

impl WindowDefinition for DefaultSettingsWindow {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: WindowMetadata = SETTINGS_METADATA;
}

impl Render for DefaultSettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        <Self as WindowElement>::render(self, window, cx)
    }
}

impl WindowElement for DefaultSettingsWindow {
    fn window_options(_route: &WindowRoute<Self::Path, Self::Query>, cx: &App) -> WindowOptions {
        settings_window_options(cx)
    }

    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(settings_content(&self.font_size_slider, cx));
        let layers = ui::window_layers(window, cx);

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
                .children(layers)
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
            .children(layers)
            .into_any_element()
    }
}

fn create_default_settings_window(
    route: crate::RouteMatch,
    window: &mut Window,
    cx: &mut App,
) -> Result<WindowInstance, WindowRuntimeError> {
    crate::__private::create_window::<DefaultSettingsWindow>(
        route,
        window,
        cx,
        DefaultSettingsWindow::new,
    )
}

fn default_settings_window_options(
    route: &crate::RouteMatch,
    cx: &App,
) -> Result<WindowOptions, WindowRuntimeError> {
    crate::__private::window_options::<DefaultSettingsWindow>(route, cx)
}

fn settings_window_options(cx: &App) -> WindowOptions {
    let mut options = WindowOptions {
        window_min_size: Some(size(px(680.0), px(520.0))),
        titlebar: Some(TitleBar::title_bar_options()),
        ..Default::default()
    };
    desktop::apply_window_display_preference(
        &mut options,
        None,
        Some(size(px(860.0), px(680.0))),
        cx,
    );
    options
}

fn settings_content<T>(
    font_size_slider: &Entity<SliderState>,
    cx: &mut Context<T>,
) -> impl IntoElement
where
    T: 'static,
{
    let header_style = settings_header_style();

    Settings::new("nexora-default-settings")
        .with_size(theme::component_size(cx))
        .header_style(&header_style)
        .with_group_variant(GroupBoxVariant::Outline)
        .pages([appearance_setting_page(font_size_slider.clone())])
}

fn appearance_setting_page(font_size_slider: Entity<SliderState>) -> SettingPage {
    SettingPage::new("外观")
        .header_style(&settings_header_style())
        .icon(Icon::new(IconName::Palette))
        .description("切换主题预设、显示模式、文字大小与组件密度。")
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
        .group(
            SettingGroup::new()
                .title("文字")
                .item(font_size_setting_item(font_size_slider)),
        )
        .group(
            SettingGroup::new()
                .title("组件")
                .item(component_size_setting_item()),
        )
}

fn font_size_setting_item(font_size_slider: Entity<SliderState>) -> SettingItem {
    let slider_for_render = font_size_slider.clone();
    let slider_for_reset = font_size_slider;

    SettingItem::new(
        "文字大小",
        SettingField::render(move |options, _window, cx| {
            h_flex()
                .w_full()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.0))
                        .child(Slider::new(&slider_for_render).disabled(options.disabled)),
                )
                .child(
                    div()
                        .w(px(48.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}px", theme::font_size(cx))),
                )
        })
        .on_reset(
            |cx| theme::font_size(cx) != theme::DEFAULT_FONT_SIZE,
            move |window, cx| {
                theme::set_font_size(theme::DEFAULT_FONT_SIZE, cx);
                slider_for_reset.update(cx, |slider, cx| {
                    slider.set_value(f32::from(theme::DEFAULT_FONT_SIZE), window, cx);
                });
            },
        ),
    )
    .layout(Axis::Vertical)
    .description("调整应用界面的基础字号。")
    .keywords(["字号", "字体", "font size"])
}

fn component_size_setting_item() -> SettingItem {
    let options = [
        (Size::XSmall, "超紧凑"),
        (Size::Small, "紧凑"),
        (Size::Medium, "标准"),
        (Size::Large, "宽松"),
    ]
    .into_iter()
    .map(|(size, label)| (SharedString::from(size.as_str()), SharedString::from(label)))
    .collect();

    SettingItem::new(
        "组件尺寸",
        SettingField::dropdown(
            options,
            |cx: &App| SharedString::from(theme::component_size(cx).as_str()),
            |value: SharedString, cx: &mut App| {
                theme::set_component_size(Size::from_str(value.as_ref()), cx);
            },
        )
        .default_value(SharedString::from(theme::DEFAULT_COMPONENT_SIZE.as_str())),
    )
    .description("统一调整支持尺寸语义的组件密度。")
    .keywords(["组件尺寸", "界面密度", "size", "density"])
}

fn font_size_from_slider(value: &SliderValue) -> u16 {
    value.start().round() as u16
}

fn settings_header_style() -> StyleRefinement {
    #[cfg(target_os = "macos")]
    return StyleRefinement::default().pt(MACOS_TITLE_BAR_HEIGHT);

    #[cfg(not(target_os = "macos"))]
    StyleRefinement::default()
}
