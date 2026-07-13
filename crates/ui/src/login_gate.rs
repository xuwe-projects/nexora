//! 可复用的桌面应用认证门禁。
//!
//! 该组件只负责统一的登录视觉、明暗主题素材和交互入口，不读取任何 OIDC 配置，
//! 也不持有应用认证状态。宿主应用通过属性与回调接入自己的认证和设置流程。

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, ClickEvent, Image, ImageFormat, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, img, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

const LOGO_BYTES: &[u8] = include_bytes!("../../../assets/logos/logo-icon-128.png");
const NETWORK_BYTES: &[u8] = include_bytes!("../assets/login-network.png");
const NETWORK_DARK_BYTES: &[u8] = include_bytes!("../assets/login-network-dark.png");

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 无业务导航的全窗口登录门禁。
///
/// 组件根据当前主题自动选择明暗网络素材，并提供主登录、设置、隐私和帮助入口。
/// 宿主应用仍负责认证状态机、浏览器跳转和设置窗口生命周期。
#[derive(IntoElement)]
pub struct LoginGate {
    product_name: SharedString,
    version: SharedString,
    configured: bool,
    busy: bool,
    status: Option<SharedString>,
    login_label: SharedString,
    busy_label: SharedString,
    on_login: ClickHandler,
    on_settings: ClickHandler,
    privacy_url: SharedString,
    help_url: SharedString,
}

impl LoginGate {
    /// 创建一个具备完整交互入口的登录门禁。
    ///
    /// `on_login` 与 `on_settings` 分别接入宿主应用的认证流程和设置窗口；版本文案通常传入
    /// `Console 0.1.0` 这类可直接展示的字符串。
    pub fn new(
        product_name: impl Into<SharedString>,
        version: impl Into<SharedString>,
        on_login: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_settings: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            product_name: product_name.into(),
            version: version.into(),
            configured: true,
            busy: false,
            status: None,
            login_label: "使用铉微账户登录".into(),
            busy_label: "正在连接认证服务...".into(),
            on_login: Rc::new(on_login),
            on_settings: Rc::new(on_settings),
            privacy_url: "https://xuwe.cc/privacy".into(),
            help_url: "https://github.com/xuwe-projects/desktop-template/issues".into(),
        }
    }

    /// 设置认证是否已经配置；未配置时主按钮保持禁用并提示先进入设置。
    pub fn configured(mut self, configured: bool) -> Self {
        self.configured = configured;
        self
    }

    /// 设置认证流程是否正在执行；忙碌状态会显示加载图标并防止重复登录。
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// 设置需要展示在安全说明下方的状态或错误；传入 `None` 时不占用额外空间。
    pub fn status(mut self, status: Option<impl Into<SharedString>>) -> Self {
        self.status = status.map(Into::into);
        self
    }

    /// 覆盖主登录按钮文案，便于其它桌面应用沿用布局但使用自己的产品名称。
    pub fn login_label(mut self, label: impl Into<SharedString>) -> Self {
        self.login_label = label.into();
        self
    }

    /// 覆盖隐私说明链接。
    pub fn privacy_url(mut self, url: impl Into<SharedString>) -> Self {
        self.privacy_url = url.into();
        self
    }

    /// 覆盖帮助与支持链接。
    pub fn help_url(mut self, url: impl Into<SharedString>) -> Self {
        self.help_url = url.into();
        self
    }
}

impl RenderOnce for LoginGate {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let network_image = if theme.is_dark() {
            network_dark_image()
        } else {
            network_image()
        };
        let primary_label = if self.busy {
            self.busy_label.clone()
        } else if self.configured {
            self.login_label.clone()
        } else {
            "请先配置身份认证".into()
        };
        let on_login = self.on_login.clone();
        let on_settings = self.on_settings.clone();

        div()
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.background)
            .child(
                TitleBar::new()
                    .h(px(76.0))
                    .border_b(px(0.0))
                    .bg(theme.background)
                    .child(
                        h_flex()
                            .size_full()
                            .pr_6()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .items_center()
                                    .child(img(logo_image()).size(px(42.0)))
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_semibold()
                                            .text_color(theme.foreground)
                                            .child(self.product_name),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(
                                        Button::new("login-settings")
                                            .ghost()
                                            .small()
                                            .icon(IconName::Settings2)
                                            .label("设置")
                                            .on_click(move |event, window, cx| {
                                                on_settings(event, window, cx);
                                            }),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(76.0))
                    .bottom_0()
                    .w_1_2()
                    .overflow_hidden()
                    .bg(theme.background)
                    .child(
                        img(network_image)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_1_2()
                    .right_0()
                    .top(px(76.0))
                    .bottom_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_12()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(440.0))
                            .items_start()
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .text_color(theme.primary)
                                    .child("安全工作台"),
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .text_size(px(42.0))
                                    .font_bold()
                                    .text_color(theme.foreground)
                                    .child("从这里开始"),
                            )
                            .child(
                                div()
                                    .mt_5()
                                    .text_base()
                                    .text_color(theme.muted_foreground)
                                    .child("登录以访问你的项目、任务和团队工作区。"),
                            )
                            .child(
                                Button::new("oidc-login")
                                    .mt_8()
                                    .w_full()
                                    .h(px(50.0))
                                    .large()
                                    .primary()
                                    .loading(self.busy)
                                    .disabled(!self.configured || self.busy)
                                    .label(primary_label)
                                    .on_click(move |event, window, cx| on_login(event, window, cx)),
                            )
                            .child(
                                h_flex()
                                    .mt_6()
                                    .gap_3()
                                    .items_center()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        Icon::new(IconName::CircleCheck)
                                            .size_5()
                                            .text_color(theme.primary),
                                    )
                                    .child("由铉微统一身份认证保护"),
                            )
                            .when_some(self.status, |this, status| {
                                this.child(
                                    div()
                                        .mt_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(status),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(42.0))
                    .bottom(px(28.0))
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(self.version),
            )
            .child(
                h_flex()
                    .absolute()
                    .right(px(34.0))
                    .bottom(px(20.0))
                    .gap_1()
                    .child(footer_link("login-privacy", "隐私", self.privacy_url))
                    .child(div().px_1().text_color(theme.muted_foreground).child("·"))
                    .child(footer_link("login-help", "帮助", self.help_url)),
            )
    }
}

fn logo_image() -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_BYTES.to_vec()))
}

fn network_image() -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, NETWORK_BYTES.to_vec()))
}

fn network_dark_image() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        NETWORK_DARK_BYTES.to_vec(),
    ))
}

fn footer_link(id: &'static str, label: &'static str, url: SharedString) -> Button {
    Button::new(id)
        .small()
        .text()
        .label(label)
        .on_click(move |_, _, cx| cx.open_url(url.as_ref()))
}
