#![cfg(all(feature = "desktop", feature = "derive"))]

use gpui::{Context, Empty, IntoElement, Window, px, size};
use nexora::{
    Application as _, ApplicationError, ApplicationLogo, ApplicationOptions, FeatureElement,
    WindowElement,
};

#[derive(Default, nexora::Feature)]
#[nexora(title = "首页", path = "/")]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default, nexora::SettingsWindow)]
struct SettingsWindow;

impl WindowElement for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct DefaultApplication;

impl nexora::Application for DefaultApplication {}

struct ConfiguredApplication {
    initial_path: &'static str,
}

impl nexora::Application for ConfiguredApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new().initial_path(self.initial_path)
    }
}

#[test]
fn default_options_are_immediately_usable() {
    let options = DefaultApplication.options();

    assert!(options.activate);
    assert!(!options.daemon_mode);
    assert_eq!(options.application_name, "Nexora");
    assert_eq!(
        options.application_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert!(options.application_logo.is_none());
    assert_eq!(
        options.sidebar_subtitle.as_deref(),
        Some("Desktop workspace")
    );
    assert_eq!(options.locale, "zh-CN");
    assert_eq!(options.initial_path, "/");
    assert_eq!(options.window_size, Some(size(px(900.0), px(640.0))));
    assert_eq!(options.window_min_size, Some(size(px(640.0), px(480.0))));
    assert!(
        options
            .window_options
            .as_ref()
            .and_then(|options| options.titlebar.as_ref())
            .is_some()
    );
}

#[test]
fn option_builders_replace_framework_defaults() {
    let options = ApplicationOptions::new()
        .application_name("Nexora Studio")
        .application_version("2.0.0")
        .application_logo(ApplicationLogo::png(b"png"))
        .sidebar_subtitle("Project workspace")
        .initial_path("/users")
        .locale("en")
        .window_size(1280.0, 800.0)
        .window_min_size(720.0, 480.0)
        .activate(false)
        .daemon_mode(true)
        .startup_display_uuid("display-1");

    assert_eq!(options.application_name, "Nexora Studio");
    assert_eq!(options.application_version.as_deref(), Some("2.0.0"));
    assert!(options.application_logo.is_some());
    assert_eq!(
        options.sidebar_subtitle.as_deref(),
        Some("Project workspace")
    );
    assert_eq!(options.initial_path, "/users");
    assert_eq!(options.locale, "en");
    assert_eq!(options.window_size, Some(size(px(1280.0), px(800.0))));
    assert_eq!(options.window_min_size, Some(size(px(720.0), px(480.0))));
    assert!(!options.activate);
    assert!(options.daemon_mode);
    assert_eq!(options.startup_display_uuid.as_deref(), Some("display-1"));
}

#[test]
fn validation_rejects_missing_initial_feature_before_startup() {
    let error = ConfiguredApplication {
        initial_path: "/missing",
    }
    .validate()
    .expect_err("不存在的首路由应当在启动前失败");

    assert!(matches!(
        error,
        ApplicationError::InitialRoute { ref path, .. } if path == "/missing"
    ));
}

#[test]
fn validation_rejects_window_as_main_content() {
    let error = ConfiguredApplication {
        initial_path: "/settings",
    }
    .validate()
    .expect_err("独立窗口不能作为主窗口的首 Feature");

    assert_eq!(
        error,
        ApplicationError::InitialRouteIsWindow {
            path: "/settings".to_owned(),
            id: "settings",
        }
    );
}

#[test]
fn validation_accepts_discovered_initial_feature() {
    ConfiguredApplication { initial_path: "/" }
        .validate()
        .expect("派生 Feature 应当可以由 Application 自动发现");
}
