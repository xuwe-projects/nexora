#![cfg(all(feature = "desktop", feature = "derive"))]

use desktop::ApplicationOptions as DesktopApplicationOptions;
use gpui::{Bounds, TestAppContext, WindowBounds, point, px, size};
use gpui_component::{Size, Theme, ThemeMode};
use nexora::__private::{
    AccountPreferences, MainWindowPlacement, PersistedWindowBounds, ShellAppearancePreferences,
    ShellPreferences, WindowSession, WindowSessionRole, WindowTabSession,
    restore_appearance_preferences, restore_main_window_options,
};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<gpui::Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

#[test]
fn shell_preferences_default_to_safe_values() {
    let preferences = ShellPreferences::default();

    assert!(preferences.pinned_tabs.is_empty());
    assert_eq!(
        preferences.appearance.theme_preset,
        ThemePreset::default().id()
    );
    assert_eq!(
        preferences.appearance.color_scheme,
        ColorScheme::default().id()
    );
    assert_eq!(
        preferences.appearance.font_size,
        i64::from(theme::DEFAULT_FONT_SIZE)
    );
    assert_eq!(
        preferences.appearance.component_size,
        theme::DEFAULT_COMPONENT_SIZE.as_str()
    );
    assert!(preferences.main_window.is_none());
    assert_eq!(preferences.account, AccountPreferences::default());
}

#[test]
fn old_preferences_use_safe_account_defaults() {
    let preferences: ShellPreferences = toml::from_str("pinned_tabs = []").unwrap();

    assert!(preferences.account.remember_login);
    assert!(!preferences.account.recovery_allowed);
    let serialized = toml::to_string(&preferences).unwrap();
    assert!(!serialized.contains("access_token"));
    assert!(!serialized.contains("refresh_token"));
}

#[test]
fn unknown_workspace_fields_survive_same_schema_round_trip() {
    let preferences: ShellPreferences = toml::from_str(
        r#"
        schema_version = 1
        future_scalar = "keep-me"

        [future_table]
        enabled = true
        "#,
    )
    .expect("同一 schema 的未知字段应当可以读取");

    let serialized = toml::to_string_pretty(&preferences).expect("偏好应当可以重新序列化");

    assert!(serialized.contains("future_scalar = \"keep-me\""));
    assert!(serialized.contains("[future_table]"));
    assert!(serialized.contains("enabled = true"));
}

#[test]
fn schema_zero_preferences_migrate_legacy_tabs_and_geometry_into_main_session() {
    let mut preferences: ShellPreferences = toml::from_str(
        r#"
        pinned_tabs = ["/", "/users?status=active"]

        [main_window]
        display_uuid = "display-legacy"

        [main_window.bounds]
        state = "windowed"
        x = 10
        y = 20
        width = 900
        height = 640
        "#,
    )
    .expect("历史偏好应当可反序列化");

    assert_eq!(preferences.schema_version, 0);
    assert!(preferences.migrate_to_current());
    assert_eq!(preferences.schema_version, 1);
    assert!(preferences.pinned_tabs.is_empty());
    assert!(preferences.main_window.is_none());
    let main = preferences
        .windows
        .iter()
        .find(|session| session.session_id == "main")
        .expect("迁移应当创建主窗口会话");
    assert_eq!(main.role, WindowSessionRole::MainShell);
    assert_eq!(main.display_uuid.as_deref(), Some("display-legacy"));
    assert_eq!(main.tabs.len(), 2);
    assert!(main.tabs.iter().all(|tab| tab.pinned));
}

#[test]
fn window_tab_move_is_clamped_to_its_pin_partition() {
    let mut session = WindowSession {
        session_id: "shell-1".to_owned(),
        tabs: [
            ("pinned-a", true),
            ("pinned-b", true),
            ("regular-a", false),
            ("regular-b", false),
        ]
        .into_iter()
        .map(|(route_id, pinned)| WindowTabSession {
            route_id: route_id.to_owned(),
            location: format!("/{route_id}"),
            pinned,
        })
        .collect(),
        ..WindowSession::default()
    };

    assert!(session.move_tab_within_partition(3, 0));
    assert_eq!(
        session
            .tabs
            .iter()
            .map(|tab| tab.route_id.as_str())
            .collect::<Vec<_>>(),
        ["pinned-a", "pinned-b", "regular-b", "regular-a"]
    );
    assert!(session.move_tab_within_partition(0, 3));
    assert_eq!(
        session
            .tabs
            .iter()
            .map(|tab| tab.route_id.as_str())
            .collect::<Vec<_>>(),
        ["pinned-b", "pinned-a", "regular-b", "regular-a"]
    );
}

#[test]
fn old_startup_display_config_reads_and_is_cleaned_on_next_save() {
    let preferences: ShellPreferences = toml::from_str(
        r#"
        pinned_tabs = ["/", "/users"]
        startup_display_uuid = "legacy-display"
        "#,
    )
    .expect("旧 workspace.toml 应当可以读取");

    assert_eq!(preferences.pinned_tabs, ["/", "/users"]);
    assert_eq!(
        preferences.appearance,
        ShellAppearancePreferences::default()
    );
    assert!(preferences.main_window.is_none());

    let serialized = toml::to_string_pretty(&preferences).expect("偏好应当可以重新序列化");
    assert!(!serialized.contains("startup_display_uuid"));
}

#[test]
fn shell_preferences_serialize_round_trip() {
    let mut preferences = ShellPreferences::default();
    preferences.pinned_tabs = vec!["/".to_owned(), "/reports".to_owned()];
    preferences.appearance = ShellAppearancePreferences {
        theme_preset: "nexora".to_owned(),
        color_scheme: "dark".to_owned(),
        font_size: 18,
        component_size: "large".to_owned(),
    };
    preferences.main_window = Some(MainWindowPlacement {
        display_uuid: "display-2".to_owned(),
        bounds: PersistedWindowBounds::Maximized {
            x: 200,
            y: 100,
            width: 1200,
            height: 800,
        },
    });

    let serialized = toml::to_string_pretty(&preferences).expect("偏好应当可以序列化");
    let decoded: ShellPreferences = toml::from_str(serialized.as_str()).expect("偏好应当可以读取");

    assert_eq!(decoded, preferences);
}

#[test]
fn preference_field_updates_preserve_unrelated_fields() {
    let original_appearance = ShellAppearancePreferences {
        theme_preset: "nexora".to_owned(),
        color_scheme: "light".to_owned(),
        font_size: 16,
        component_size: "small".to_owned(),
    };
    let original_window = MainWindowPlacement {
        display_uuid: "display-2".to_owned(),
        bounds: PersistedWindowBounds::Windowed {
            x: 20,
            y: 40,
            width: 900,
            height: 640,
        },
    };
    let mut preferences = ShellPreferences::default();
    preferences.appearance = original_appearance.clone();
    preferences.main_window = Some(original_window.clone());

    preferences.pinned_tabs = vec!["/users".to_owned()];

    assert_eq!(preferences.appearance, original_appearance);
    assert_eq!(preferences.main_window, Some(original_window));
}

#[test]
fn window_bounds_save_round_trips_all_states() {
    let raw_bounds = bounds(120.4, 240.5, 1000.2, 700.8);

    assert_eq!(
        PersistedWindowBounds::from_window_bounds(WindowBounds::Windowed(raw_bounds)),
        Some(PersistedWindowBounds::Windowed {
            x: 120,
            y: 241,
            width: 1000,
            height: 701,
        })
    );
    assert!(matches!(
        PersistedWindowBounds::from_window_bounds(WindowBounds::Maximized(raw_bounds)),
        Some(PersistedWindowBounds::Maximized { .. })
    ));
    assert!(matches!(
        PersistedWindowBounds::from_window_bounds(WindowBounds::Fullscreen(raw_bounds)),
        Some(PersistedWindowBounds::Fullscreen { .. })
    ));
}

#[gpui::test]
fn windowed_main_window_preferences_restore_display_and_bounds(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供显示器");
        let display_uuid = display.uuid().expect("测试显示器应当提供 UUID").to_string();
        let mut options = DesktopApplicationOptions {
            window_size: Some(size(px(900.0), px(640.0))),
            window_min_size: Some(size(px(640.0), px(480.0))),
            ..Default::default()
        };
        let mut preferences = ShellPreferences::default();
        preferences.main_window = Some(MainWindowPlacement {
            display_uuid,
            bounds: PersistedWindowBounds::Windowed {
                x: 100,
                y: 80,
                width: 1100,
                height: 720,
            },
        });

        assert!(restore_main_window_options(&mut options, &preferences, cx));

        assert_eq!(options.display_id(), Some(display.id()));
        assert!(options.window_size.is_none());
        assert_eq!(
            options.window_options.unwrap().window_bounds.unwrap(),
            WindowBounds::Windowed(bounds(100.0, 80.0, 1100.0, 720.0))
        );
    });
}

#[gpui::test]
fn maximized_main_window_preferences_restore_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供显示器");
        let display_uuid = display.uuid().expect("测试显示器应当提供 UUID").to_string();
        let mut options = DesktopApplicationOptions {
            window_min_size: Some(size(px(640.0), px(480.0))),
            ..Default::default()
        };
        let mut preferences = ShellPreferences::default();
        preferences.main_window = Some(MainWindowPlacement {
            display_uuid,
            bounds: PersistedWindowBounds::Maximized {
                x: 10,
                y: 20,
                width: 1200,
                height: 800,
            },
        });

        assert!(restore_main_window_options(&mut options, &preferences, cx));

        assert!(matches!(
            options.window_options.unwrap().window_bounds.unwrap(),
            WindowBounds::Maximized(_)
        ));
    });
}

#[gpui::test]
fn fullscreen_main_window_preferences_restore_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供显示器");
        let display_uuid = display.uuid().expect("测试显示器应当提供 UUID").to_string();
        let mut options = DesktopApplicationOptions::default();
        let mut preferences = ShellPreferences::default();
        preferences.main_window = Some(MainWindowPlacement {
            display_uuid,
            bounds: PersistedWindowBounds::Fullscreen {
                x: 10,
                y: 20,
                width: 1200,
                height: 800,
            },
        });

        assert!(restore_main_window_options(&mut options, &preferences, cx));

        assert!(matches!(
            options.window_options.unwrap().window_bounds.unwrap(),
            WindowBounds::Fullscreen(_)
        ));
    });
}

#[gpui::test]
fn missing_display_restores_safely_without_mutating_original_uuid(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let primary = cx.primary_display().expect("测试平台应当提供显示器");
        let original_uuid = "disconnected-display".to_owned();
        let mut options = DesktopApplicationOptions {
            window_min_size: Some(size(px(640.0), px(480.0))),
            ..Default::default()
        };
        let mut preferences = ShellPreferences::default();
        preferences.main_window = Some(MainWindowPlacement {
            display_uuid: original_uuid.clone(),
            bounds: PersistedWindowBounds::Windowed {
                x: 100_000,
                y: -100_000,
                width: 0,
                height: 720,
            },
        });

        assert!(!restore_main_window_options(&mut options, &preferences, cx));

        preferences.main_window = Some(MainWindowPlacement {
            display_uuid: original_uuid.clone(),
            bounds: PersistedWindowBounds::Windowed {
                x: 100_000,
                y: -100_000,
                width: 1100,
                height: 720,
            },
        });

        assert!(restore_main_window_options(&mut options, &preferences, cx));
        assert_eq!(
            preferences.main_window.as_ref().unwrap().display_uuid,
            original_uuid
        );
        assert_eq!(options.display_id(), None);
        let restored = options
            .window_options
            .unwrap()
            .window_bounds
            .unwrap()
            .get_bounds();
        let visible = primary.visible_bounds();
        assert!(restored.origin.x >= visible.origin.x);
        assert!(restored.origin.y >= visible.origin.y);
        assert!(restored.origin.x + restored.size.width <= visible.origin.x + visible.size.width);
        assert!(restored.origin.y + restored.size.height <= visible.origin.y + visible.size.height);
    });
}

#[gpui::test]
fn appearance_preferences_restore_theme_font_and_component_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(cx);
        let mut preferences = ShellPreferences::default();
        preferences.appearance = ShellAppearancePreferences {
            theme_preset: "nexora".to_owned(),
            color_scheme: "dark".to_owned(),
            font_size: 18,
            component_size: "large".to_owned(),
        };

        restore_appearance_preferences(&preferences, cx);

        assert_eq!(
            theme::selection(cx),
            ThemeSelection::new(ThemePreset::Nexora, ColorScheme::Dark)
        );
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark);
        assert_eq!(theme::font_size(cx), 18);
        assert_eq!(theme::component_size(cx), Size::Large);
    });
}

#[gpui::test]
fn invalid_appearance_preferences_fall_back_to_defaults(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(cx);
        let mut preferences = ShellPreferences::default();
        preferences.appearance = ShellAppearancePreferences {
            theme_preset: "future-theme".to_owned(),
            color_scheme: "future-mode".to_owned(),
            font_size: 100,
            component_size: "huge".to_owned(),
        };

        restore_appearance_preferences(&preferences, cx);

        assert_eq!(theme::selection(cx), ThemeSelection::default());
        assert_eq!(theme::font_size(cx), theme::MAX_FONT_SIZE);
        assert_eq!(theme::component_size(cx), theme::DEFAULT_COMPONENT_SIZE);
    });
}

trait DesktopOptionsExt {
    fn display_id(&self) -> Option<gpui::DisplayId>;
}

impl DesktopOptionsExt for DesktopApplicationOptions {
    fn display_id(&self) -> Option<gpui::DisplayId> {
        self.window_options
            .as_ref()
            .and_then(|options| options.display_id)
    }
}
