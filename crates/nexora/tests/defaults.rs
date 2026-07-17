#![cfg(feature = "desktop")]

use nexora::{AppRegistry, RouteTargetKind};

#[cfg(feature = "desktop")]
struct CustomUsersFeature;

#[cfg(feature = "desktop")]
impl nexora::Feature for CustomUsersFeature {
    type Path = nexora::NoPath;
    type Query = nexora::NoQuery;

    const METADATA: nexora::FeatureMetadata = nexora::FeatureMetadata::new(
        "custom-users",
        "自定义用户",
        "/users",
        Some("自定义"),
        None,
        None,
        1,
        true,
    );
}

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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[test]
fn account_client_adds_default_management_features() {
    let registry = AppRegistry::builder()
        .account_defaults(true)
        .build()
        .expect("Account 客户端应注入默认管理页面");

    assert_eq!(
        registry.resolve("/users").unwrap().target().title(),
        "用户管理"
    );
    assert_eq!(
        registry.resolve("/roles").unwrap().target().title(),
        "角色与权限"
    );
}

#[cfg(feature = "desktop")]
#[test]
fn application_feature_with_reserved_path_replaces_only_matching_default() {
    let registry = AppRegistry::builder()
        .account_defaults(true)
        .feature::<CustomUsersFeature>()
        .build()
        .expect("普通 Feature 应能覆盖对应 Account 默认页面");

    assert_eq!(
        registry.resolve("/users").unwrap().target().id(),
        "custom-users"
    );
    assert_eq!(registry.resolve("/roles").unwrap().target().id(), "roles");
}
