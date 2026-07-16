#![cfg(feature = "desktop")]

use nexora::{AppRegistry, RouteTargetKind};

#[test]
fn default_settings_window_is_available_without_an_application_override() {
    let registry = AppRegistry::builder()
        .build()
        .expect("没有应用覆盖时应注入框架默认设置窗口");
    let settings = registry
        .resolve("/settings")
        .expect("默认设置窗口应参与统一路径解析");

    assert_eq!(settings.target().kind(), RouteTargetKind::Window);
    assert_eq!(settings.target().id(), "settings");
    assert_eq!(settings.target().title(), "设置");
}

#[cfg(feature = "account-client")]
#[gpui::test]
fn default_login_feature_creates_a_render_entity(cx: &mut gpui::TestAppContext) {
    use gpui::Empty;

    let registry = AppRegistry::builder()
        .build()
        .expect("没有应用覆盖时应选择框架默认登录页");
    let window = cx.add_window(|_, _| Empty);
    let _login = window
        .update(cx, |_, window, cx| {
            registry.create_login_feature(window, cx)
        })
        .expect("测试窗口应允许创建默认登录页 Entity");
}
