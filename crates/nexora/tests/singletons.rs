#![cfg(all(feature = "desktop", feature = "derive"))]

#[cfg(feature = "desktop")]
use gpui::Render;
use gpui::{Context, Empty, IntoElement, Window};
use nexora::{AppRegistry, RegistryError, WindowElement};

#[derive(Default, nexora::SettingsWindow)]
struct AlphaSettingsWindow;

impl WindowElement for AlphaSettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default, nexora::SettingsWindow)]
struct ZuluSettingsWindow;

impl WindowElement for ZuluSettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default, nexora::Window)]
#[nexora(title = "普通设置路由", path = "/settings")]
struct GenericSettingsRoute;

impl WindowElement for GenericSettingsRoute {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[cfg(feature = "desktop")]
#[derive(Default, nexora::LoginFeature)]
struct AlphaLoginFeature;

#[cfg(feature = "desktop")]
impl Render for AlphaLoginFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[cfg(feature = "desktop")]
#[derive(Default, nexora::LoginFeature)]
struct ZuluLoginFeature;

#[cfg(feature = "desktop")]
impl Render for ZuluLoginFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[test]
fn settings_window_override_replaces_framework_default_before_route_validation() {
    let registry = AppRegistry::builder()
        .settings_window::<AlphaSettingsWindow>()
        .build()
        .expect("单个应用设置窗口应覆盖框架默认实现");

    assert_eq!(registry.windows().len(), 1);
    assert_eq!(registry.windows()[0].id(), "settings");
    assert_eq!(registry.windows()[0].path(), "/settings");
}

#[test]
fn generic_window_builder_routes_derived_settings_window_to_the_singleton_slot() {
    let registry = AppRegistry::builder()
        .window::<AlphaSettingsWindow>()
        .build()
        .expect("泛型 Window 注册流程也应识别专用 Settings Window");

    assert_eq!(registry.windows().len(), 1);
    assert_eq!(registry.windows()[0].id(), "settings");
}

#[test]
fn duplicate_settings_window_overrides_report_sorted_type_names() {
    let error = AppRegistry::builder()
        .settings_window::<ZuluSettingsWindow>()
        .settings_window::<AlphaSettingsWindow>()
        .build()
        .err()
        .expect("多个应用设置窗口覆盖必须失败");

    assert!(matches!(
        error,
        RegistryError::DuplicateSettingsWindow { first, duplicate }
            if first.ends_with("AlphaSettingsWindow")
                && duplicate.ends_with("ZuluSettingsWindow")
    ));
}

#[test]
fn inventory_discovery_enforces_settings_window_singleton() {
    let error = AppRegistry::discover()
        .err()
        .expect("自动发现的多个设置窗口覆盖必须失败");

    assert!(matches!(
        error,
        RegistryError::DuplicateSettingsWindow { first, duplicate }
            if first.ends_with("AlphaSettingsWindow")
                && duplicate.ends_with("ZuluSettingsWindow")
    ));
}

#[test]
fn ordinary_window_cannot_take_reserved_settings_path() {
    let error = AppRegistry::builder()
        .window::<GenericSettingsRoute>()
        .build()
        .err()
        .expect("普通 Window 与默认设置窗口的保留路径必须冲突");

    assert!(matches!(error, RegistryError::RouteConflict { .. }));
}

#[cfg(feature = "desktop")]
#[test]
fn one_login_feature_override_is_accepted() {
    AppRegistry::builder()
        .login_feature::<AlphaLoginFeature>()
        .build()
        .expect("单个应用登录页面应覆盖框架默认实现");
}

#[cfg(feature = "desktop")]
#[test]
fn duplicate_login_feature_overrides_report_sorted_type_names() {
    let error = AppRegistry::builder()
        .login_feature::<ZuluLoginFeature>()
        .login_feature::<AlphaLoginFeature>()
        .build()
        .err()
        .expect("多个应用登录页面覆盖必须失败");

    assert!(matches!(
        error,
        RegistryError::DuplicateLoginFeature { first, duplicate }
            if first.ends_with("AlphaLoginFeature")
                && duplicate.ends_with("ZuluLoginFeature")
    ));
}
