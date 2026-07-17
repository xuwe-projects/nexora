//! Account 桌面客户端的默认登录门禁。

use gpui::{AppContext as _, Context, IntoElement, Render, Window, prelude::FluentBuilder as _};
use ui::LoginGate;

use crate::{__private::LoginFeatureRegistration, NavigationContextExt as _};

/// 创建框架默认登录页的回退注册记录。
///
/// 该记录只由应用注册表在没有发现用户 `#[derive(LoginFeature)]` 覆盖时使用，不会提交
/// 到全局 `inventory`，因此不会与应用自定义登录页形成重复注册。
pub(crate) const fn default_login_registration() -> LoginFeatureRegistration {
    LoginFeatureRegistration::new(
        "nexora::defaults::DefaultLoginFeature",
        create_default_login_feature,
    )
}

/// Nexora Account 桌面客户端自带的登录页面。
///
/// 页面本身不保存认证状态，而是每次渲染读取 Account 登录运行时的无敏感信息快照；
/// 登录协议、浏览器回调和业务 `/me` 校验继续由 Account 客户端运行时负责。
struct DefaultLoginFeature;

impl Render for DefaultLoginFeature {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = crate::account::client::login_snapshot(cx);
        let branding = crate::application::application_branding(cx);
        let status = visible_status(snapshot.status.as_ref()).then_some(snapshot.status);
        let settings_target = cx.entity().downgrade();

        LoginGate::new(
            branding.application_name.clone(),
            branding.application_version.clone().map_or_else(
                || branding.application_name.clone(),
                |version| format!("{} {version}", branding.application_name),
            ),
            |_, _, cx| {
                _ = crate::account::client::start_login(cx);
            },
            move |_, _, cx| {
                _ = settings_target.update(cx, |_, cx| {
                    _ = cx.navigate("/settings");
                });
            },
        )
        .configured(snapshot.configured)
        .busy(snapshot.busy)
        .status(status)
        .login_label(format!("使用 {} 账户登录", branding.application_name))
        .when_some(branding.logo, |gate, logo| gate.logo(logo.image()))
        .title_bar(false)
    }
}

fn create_default_login_feature(_window: &mut Window, cx: &mut gpui::App) -> gpui::AnyView {
    cx.new(|_| DefaultLoginFeature).into()
}

fn visible_status(status: &str) -> bool {
    !status.is_empty()
        && status != "未登录"
        && status != "未配置 Account 登录"
        && status != "已退出登录"
}
