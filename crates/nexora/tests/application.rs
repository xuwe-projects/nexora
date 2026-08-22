#![cfg(all(feature = "desktop", feature = "derive"))]

use std::borrow::Cow;

use gpui::{AssetSource, Context, Empty, IntoElement, SharedString, Window, px, size};
use nexora::__private::window_requires_authentication;
use nexora::{
    Application as _, ApplicationError, ApplicationLogo, ApplicationOptions, ApplicationTabStyle,
    ApplicationThemePreset, FeatureElement, WindowElement,
};

const CUSTOM_THEME: &str = r##"{
    "themes": [
        { "name": "Light", "mode": "light", "colors": { "background": "#ffffff" } },
        { "name": "Dark", "mode": "dark", "colors": { "background": "#000000" } }
    ]
}"##;

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

#[derive(Default, nexora::Window)]
#[nexora(id = "public-info", title = "公共信息", path = "/public-info")]
struct PublicInfoWindow;

impl WindowElement for PublicInfoWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct TestAssets;

impl AssetSource for TestAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "icons/app.svg" => Ok(Some(Cow::Borrowed(b"<svg/>"))),
            _ => Ok(None),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(["icons/app.svg"]
            .into_iter()
            .filter(|asset| asset.starts_with(path))
            .map(Into::into)
            .collect())
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

struct WindowAccessApplication {
    window_ids: Vec<&'static str>,
}

impl nexora::Application for WindowAccessApplication {
    fn options(&self) -> ApplicationOptions {
        self.window_ids
            .iter()
            .fold(ApplicationOptions::new(), |options, window_id| {
                options.unauthenticated_window(*window_id)
            })
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
    assert!(options.application_assets.is_none());
    assert_eq!(
        options.sidebar_subtitle.as_deref(),
        Some("Desktop workspace")
    );
    assert_eq!(options.locale, "zh-CN");
    assert_eq!(options.initial_path, "/");
    assert!(options.unauthenticated_window_ids.is_empty());
    assert_eq!(options.tab_style, ApplicationTabStyle::Tab);
    assert!(!options.sidebar_search);
    assert!(options.tray_enabled);
    assert!(options.application_identity_override.is_none());
    assert!(options.theme_presets.is_empty());
    assert!(options.default_theme_preset.is_none());
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
        .application_assets(TestAssets)
        .sidebar_subtitle("Project workspace")
        .initial_path("/users")
        .unauthenticated_window("public-info")
        .tab_style(ApplicationTabStyle::Underline)
        .sidebar_search(true)
        .locale("en")
        .window_size(1280.0, 800.0)
        .window_min_size(720.0, 480.0)
        .activate(false)
        .daemon_mode(true)
        .tray_enabled(false)
        .application_identity("com.example.nexora-studio")
        .startup_display_uuid("display-1")
        .theme_preset(ApplicationThemePreset::new(
            "studio",
            "Studio",
            CUSTOM_THEME,
        ))
        .default_theme_preset("studio");

    assert_eq!(options.application_name, "Nexora Studio");
    assert_eq!(options.application_version.as_deref(), Some("2.0.0"));
    assert!(options.application_logo.is_some());
    assert_eq!(
        options.sidebar_subtitle.as_deref(),
        Some("Project workspace")
    );
    assert!(options.application_assets.is_some());
    assert_eq!(options.initial_path, "/users");
    assert_eq!(options.unauthenticated_window_ids, ["public-info"]);
    assert_eq!(options.tab_style, ApplicationTabStyle::Underline);
    assert!(options.sidebar_search);
    assert_eq!(options.locale, "en");
    assert_eq!(options.window_size, Some(size(px(1280.0), px(800.0))));
    assert_eq!(options.window_min_size, Some(size(px(720.0), px(480.0))));
    assert!(!options.activate);
    assert!(options.daemon_mode);
    assert!(!options.tray_enabled);
    assert_eq!(
        options.application_identity_override.as_deref(),
        Some("com.example.nexora-studio")
    );
    assert_eq!(options.startup_display_uuid.as_deref(), Some("display-1"));
    assert_eq!(options.theme_presets[0].id(), "studio");
    assert_eq!(options.default_theme_preset.as_deref(), Some("studio"));
}

#[test]
fn strict_single_process_window_mode_keeps_window_group_features_enabled() {
    let options = ApplicationOptions::new();

    assert!(options.tray_enabled);
}

#[test]
fn native_window_title_uses_framework_default_only_when_application_did_not_set_one() {
    let options = ApplicationOptions::new().default_native_window_title("安装元数据名称");
    assert_eq!(
        options
            .window_options
            .as_ref()
            .and_then(|options| options.titlebar.as_ref())
            .and_then(|titlebar| titlebar.title.as_deref()),
        Some("安装元数据名称")
    );

    let mut explicit = ApplicationOptions::new();
    explicit
        .window_options
        .as_mut()
        .unwrap()
        .titlebar
        .as_mut()
        .unwrap()
        .title = Some("应用显式标题".into());
    let explicit = explicit.default_native_window_title("安装元数据名称");
    assert_eq!(
        explicit
            .window_options
            .as_ref()
            .and_then(|options| options.titlebar.as_ref())
            .and_then(|titlebar| titlebar.title.as_deref()),
        Some("应用显式标题")
    );
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

#[test]
fn validation_accepts_registered_unauthenticated_window_id() {
    WindowAccessApplication {
        window_ids: vec!["public-info"],
    }
    .validate()
    .expect("已注册 Window 的稳定 ID 应当通过启动校验");
}

#[test]
fn validation_rejects_unknown_or_non_window_unauthenticated_ids() {
    for window_id in ["missing-window", "home", "PUBLIC-INFO"] {
        let error = WindowAccessApplication {
            window_ids: vec![window_id],
        }
        .validate()
        .expect_err("不存在、Feature 或大小写不匹配的 ID 都必须在启动前失败");

        assert_eq!(
            error,
            ApplicationError::UnknownUnauthenticatedWindow {
                id: window_id.to_owned(),
            }
        );
    }
}

#[test]
fn validation_rejects_duplicate_unauthenticated_window_ids() {
    let error = WindowAccessApplication {
        window_ids: vec!["public-info", "public-info"],
    }
    .validate()
    .expect_err("重复 Window ID 不应静默去重");

    assert_eq!(
        error,
        ApplicationError::DuplicateUnauthenticatedWindow {
            id: "public-info".to_owned(),
        }
    );
}

#[test]
fn window_authentication_policy_uses_account_state_settings_and_exact_ids() {
    let configured_ids = vec!["public-info".to_owned()];

    assert!(!window_requires_authentication(
        false,
        false,
        &configured_ids,
        "private-window",
    ));
    assert!(!window_requires_authentication(
        true,
        true,
        &configured_ids,
        "private-window",
    ));
    assert!(!window_requires_authentication(
        true,
        false,
        &configured_ids,
        "settings",
    ));
    assert!(!window_requires_authentication(
        true,
        false,
        &configured_ids,
        "public-info",
    ));
    assert!(window_requires_authentication(
        true,
        false,
        &configured_ids,
        "PUBLIC-INFO",
    ));
    assert!(window_requires_authentication(
        true,
        false,
        &configured_ids,
        "private-window",
    ));
}

struct InvalidThemeApplication;

impl nexora::Application for InvalidThemeApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .theme_preset(ApplicationThemePreset::new("acme", "Acme", "{"))
            .default_theme_preset("acme")
    }
}

#[test]
fn validation_rejects_invalid_theme_before_startup() {
    let error = InvalidThemeApplication
        .validate()
        .expect_err("非法主题必须在进入 GPUI 事件循环前失败");

    assert!(matches!(
        error,
        ApplicationError::InvalidThemeConfiguration {
            preset_id: Some(ref preset_id),
            ..
        } if preset_id == "acme"
    ));
}
