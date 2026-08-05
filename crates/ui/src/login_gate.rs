//! 可复用的桌面应用认证门禁。
//!
//! 该组件只负责统一的登录视觉、明暗主题素材和交互入口，不读取任何 OIDC 配置，
//! 也不持有应用认证状态。宿主应用通过属性与回调接入自己的认证和设置流程。

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, ClickEvent, Image, ImageFormat, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, img, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex,
};

const LOGO_BYTES: &[u8] = include_bytes!("../../../assets/logos/logo-icon-128.png");
const NETWORK_BYTES: &[u8] = include_bytes!("../assets/login-network.png");
const NETWORK_DARK_BYTES: &[u8] = include_bytes!("../assets/login-network-dark.png");

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type RememberLoginHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// 无业务导航的全窗口登录门禁。
///
/// 组件根据当前主题自动选择明暗网络素材，并提供主登录、设置、隐私和帮助入口。
/// 宿主应用仍负责认证状态机、浏览器跳转和设置窗口生命周期。
#[derive(IntoElement)]
pub struct LoginGate {
    product_name: SharedString,
    version: SharedString,
    logo: Arc<Image>,
    configured: bool,
    busy: bool,
    remember_login: bool,
    remember_login_enabled: bool,
    can_retry_recovery: bool,
    status: Option<SharedString>,
    login_label: SharedString,
    protection_label: SharedString,
    busy_label: SharedString,
    on_login: ClickHandler,
    on_settings: ClickHandler,
    on_check_updates: Option<ClickHandler>,
    on_remember_login: Option<RememberLoginHandler>,
    on_retry_recovery: Option<ClickHandler>,
    on_login_other_account: Option<ClickHandler>,
    privacy_url: SharedString,
    help_url: SharedString,
    title_bar: bool,
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
        let product_name = product_name.into();
        Self {
            login_label: format!("使用 {product_name} 账户登录").into(),
            protection_label: format!("由 {product_name} 统一身份认证保护").into(),
            product_name,
            version: version.into(),
            logo: default_application_logo(),
            configured: true,
            busy: false,
            remember_login: true,
            remember_login_enabled: true,
            can_retry_recovery: false,
            status: None,
            busy_label: "正在连接认证服务...".into(),
            on_login: Rc::new(on_login),
            on_settings: Rc::new(on_settings),
            on_check_updates: None,
            on_remember_login: None,
            on_retry_recovery: None,
            on_login_other_account: None,
            privacy_url: "https://github.com/xuwe-projects/nexora".into(),
            help_url: "https://github.com/xuwe-projects/nexora/issues".into(),
            title_bar: true,
        }
    }

    /// 覆盖登录页左上角展示的应用 Logo。
    ///
    /// 调用方可以使用 `Image::from_bytes` 加载编译进应用的品牌资源；未调用时继续使用
    /// Nexora 内置 Logo，因此只定制应用名称也能获得完整的默认登录体验。
    pub fn logo(mut self, logo: Arc<Image>) -> Self {
        self.logo = logo;
        self
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

    /// 设置受控“保持登录状态”复选框的当前值。
    pub fn remember_login(mut self, remember_login: bool) -> Self {
        self.remember_login = remember_login;
        self
    }

    /// 设置“保持登录状态”是否可用；Linux 可传入 `false` 以显示禁用说明。
    pub fn remember_login_enabled(mut self, enabled: bool) -> Self {
        self.remember_login_enabled = enabled;
        self
    }

    /// 接收用户修改保持登录偏好的回调；回调参数是点击后的受控值。
    pub fn on_remember_login(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_remember_login = Some(Rc::new(handler));
        self
    }

    /// 设置是否显示恢复失败后的重试与账号切换入口。
    pub fn recovery_actions(mut self, can_retry: bool) -> Self {
        self.can_retry_recovery = can_retry;
        self
    }

    /// 设置“重试恢复”按钮的回调。
    pub fn on_retry_recovery(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_retry_recovery = Some(Rc::new(handler));
        self
    }

    /// 设置“使用其他账号登录”按钮的回调。
    pub fn on_login_other_account(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_login_other_account = Some(Rc::new(handler));
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

    /// 覆盖登录按钮下方的身份认证保护说明。
    pub fn protection_label(mut self, label: impl Into<SharedString>) -> Self {
        self.protection_label = label.into();
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

    /// 在登录页右上角增加“检查更新”入口。
    ///
    /// 未调用时不渲染该按钮，适合没有安装 updater 的应用；Nexora 默认登录页只会在公共
    /// updater 已成功安装后设置此回调。
    pub fn on_check_updates(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_check_updates = Some(Rc::new(handler));
        self
    }

    /// 设置登录门禁是否自行渲染透明窗口标题栏。
    ///
    /// 独立使用 `LoginGate` 时应保持默认值 `true`；当外层框架已经统一提供 TitleBar 时，
    /// 设置为 `false` 可以避免重复窗口拖拽区和窗口控制按钮。
    pub const fn title_bar(mut self, title_bar: bool) -> Self {
        self.title_bar = title_bar;
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
        let on_check_updates = self.on_check_updates.clone();
        let on_remember_login = self.on_remember_login.clone();
        let on_retry_recovery = self.on_retry_recovery.clone();
        let on_login_other_account = self.on_login_other_account.clone();

        div()
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.background)
            .child(
                h_flex()
                    .absolute()
                    .left(px(42.0))
                    .top(px(34.0))
                    .gap_3()
                    .items_center()
                    .child(img(self.logo).size(px(42.0)))
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
                    .absolute()
                    .right(px(34.0))
                    .top(px(38.0))
                    .gap_2()
                    .when_some(on_check_updates, |buttons, handler| {
                        buttons.child(
                            Button::new("login-check-updates")
                                .ghost()
                                .small()
                                .icon(IconName::CircleCheck)
                                .label("检查更新")
                                .on_click(move |event, window, cx| handler(event, window, cx)),
                        )
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
                                v_flex()
                                    .mt_4()
                                    .gap_1()
                                    .child(
                                        Checkbox::new("remember-login")
                                            .with_size(theme::component_size(cx))
                                            .checked(self.remember_login)
                                            .disabled(!self.remember_login_enabled || self.busy)
                                            .label("保持登录状态")
                                            .when_some(on_remember_login, |checkbox, handler| {
                                                checkbox.on_click(move |checked, window, cx| {
                                                    handler(checked, window, cx);
                                                })
                                            }),
                                    )
                                    .when(!self.remember_login_enabled, |this| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child("当前平台暂不支持保持登录状态"),
                                        )
                                    }),
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
                                    .child(self.protection_label),
                            )
                            .when_some(self.status, |this, status| {
                                this.child(
                                    div()
                                        .mt_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(status),
                                )
                            })
                            .when(self.can_retry_recovery, |this| {
                                this.child(
                                    h_flex()
                                        .mt_4()
                                        .gap_2()
                                        .child(
                                            Button::new("retry-account-recovery")
                                                .with_size(theme::component_size(cx))
                                                .disabled(self.busy)
                                                .label("重试恢复")
                                                .when_some(
                                                    on_retry_recovery.clone(),
                                                    |button, handler| {
                                                        button.on_click(move |event, window, cx| {
                                                            handler(event, window, cx);
                                                        })
                                                    },
                                                ),
                                        )
                                        .child(
                                            Button::new("login-other-account")
                                                .with_size(theme::component_size(cx))
                                                .disabled(self.busy)
                                                .ghost()
                                                .label("使用其他账号登录")
                                                .when_some(
                                                    on_login_other_account.clone(),
                                                    |button, handler| {
                                                        button.on_click(move |event, window, cx| {
                                                            handler(event, window, cx);
                                                        })
                                                    },
                                                ),
                                        ),
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
            .when(self.title_bar, |this| {
                this.child(
                    div()
                        .debug_selector(|| "login-gate-title-bar".into())
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .child(
                            TitleBar::new()
                                .border_b(px(0.0))
                                .bg(gpui::transparent_black()),
                        ),
                )
            })
    }
}

/// 返回 Nexora 默认应用 Logo 的可复用图片对象。
///
/// 框架 Sidebar Header 与登录门禁共享该资源，确保没有提供自定义品牌素材时保持一致。
pub fn default_application_logo() -> Arc<Image> {
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
