use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use actions::account::AccountActionKind;
use configuration::UserConfigStore;
use desktop::{Application as _, centered_window_bounds};
use gpui::{AppContext as _, Axis, TestAppContext, px, size};
use gpui_component::setting::SettingItem;
#[path = "../src/app.rs"]
mod app;
#[path = "../src/auth.rs"]
mod auth;
#[path = "../src/config.rs"]
mod config;
#[path = "../src/features.rs"]
mod features;

use app::Console;
use config::ConsolePreferences;
use features::{
    FeatureId, feature_catalog,
    home::{next_steps, virtual_form_rows, virtual_form_view_modes},
    projects::project_rows,
    root::RootView,
    settings::{current_console_changelog, settings_window_options, startup_display_setting_item},
    tasks::task_rows,
    virtual_scroll::virtual_scroll_stock_seeds,
};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

#[cfg(target_os = "macos")]
#[test]
fn macos_keychain_errors_expose_actionable_messages() {
    use security_framework::base::Error as MacOsSecurityError;

    let locked = auth::macos_keychain_error_after_retry(keyring::Error::PlatformFailure(Box::new(
        MacOsSecurityError::from_code(-25_293),
    )));
    let stale_entry = auth::macos_keychain_error_after_retry(keyring::Error::PlatformFailure(
        Box::new(MacOsSecurityError::from_code(-25_299)),
    ));

    assert_eq!(locked.user_message(), "请解锁 macOS 登录钥匙串");
    assert_eq!(
        stale_entry.user_message(),
        "请清理旧的 macOS 登录凭据后重试"
    );
}

#[gpui::test]
fn signing_out_clears_the_authenticated_session(cx: &mut TestAppContext) {
    let config = oidc::OidcConfig::with_default_scopes(
        "https://id.example.com",
        "console",
        "http://127.0.0.1:0/auth/callback",
    )
    .unwrap();
    let session = oidc::OidcSession::from_token_cache(oidc::OidcTokenCache {
        access_token: "access-token".to_owned(),
        profile: Some(oidc::OidcUserProfile {
            subject: "user-1".to_owned(),
            name: Some("测试用户".to_owned()),
            picture: Some("https://cdn.example.com/avatar.png".to_owned()),
            ..oidc::OidcUserProfile::default()
        }),
        ..oidc::OidcTokenCache::default()
    })
    .unwrap();

    cx.update(|cx| {
        auth::init(Some(config), None, cx);
        auth::complete_login(Ok(session), cx);
        let snapshot = auth::snapshot(cx);
        assert!(snapshot.authenticated);
        assert_eq!(
            snapshot.avatar_url.as_deref(),
            Some("https://cdn.example.com/avatar.png")
        );

        auth::sign_out(cx);
        let snapshot = auth::snapshot(cx);
        assert!(!snapshot.authenticated);
        assert!(snapshot.avatar_url.is_none());
    });
}

#[test]
fn feature_catalog_has_stable_navigation_order() {
    let ids = feature_catalog()
        .iter()
        .map(|feature| feature.id())
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        vec![
            FeatureId::Home,
            FeatureId::Projects,
            FeatureId::Tasks,
            FeatureId::VirtualScroll,
            FeatureId::Reports,
            FeatureId::Analytics,
            FeatureId::Releases,
            FeatureId::Secrets,
            FeatureId::Integrations,
            FeatureId::AuditLogs,
            FeatureId::Team,
            FeatureId::Automation,
            FeatureId::Notifications,
            FeatureId::Billing,
            FeatureId::HelpCenter,
            FeatureId::Experiments,
        ]
    );
}

#[test]
fn feature_ids_expose_display_metadata() {
    assert_eq!(FeatureId::default(), FeatureId::Home);
    assert_eq!(FeatureId::Projects.title(), "项目");
    assert_eq!(FeatureId::VirtualScroll.title(), "虚拟滚动");
}

#[test]
fn settings_load_current_console_changelog() {
    let entry = current_console_changelog().unwrap().unwrap();

    assert_eq!(entry.component(), "console");
    assert_eq!(entry.version().to_string(), env!("CARGO_PKG_VERSION"));
    assert_eq!(entry.locale(), "zh-CN");
    assert!(!entry.markdown().contains("Sidebar"));
    assert!(!entry.markdown().contains("TabBar"));
    assert!(!entry.markdown().contains("DataTable"));
    assert!(!entry.markdown().contains("DMG"));
}

#[test]
fn console_preferences_keep_theme_selection() {
    let mut preferences = ConsolePreferences::default();
    let selection = ThemeSelection::new(ThemePreset::Xuwe, ColorScheme::Dark);

    preferences.set_theme_selection(selection);

    assert_eq!(preferences.theme_selection(), selection);
}

#[test]
fn console_preferences_round_trip_theme_and_startup_display() {
    let directory = temporary_directory("preferences");
    let path = directory.join("settings.toml");
    let store = UserConfigStore::<ConsolePreferences>::at_path(&path);
    let mut preferences = ConsolePreferences::default();
    let selection = ThemeSelection::new(ThemePreset::Xuwe, ColorScheme::Dark);
    preferences.set_theme_selection(selection);
    preferences.set_startup_display_uuid(Some("display-uuid".to_owned()));

    store.save(&preferences).expect("用户偏好应当可以保存");
    let loaded = store
        .load_versioned_or_default()
        .expect("用户偏好应当可以重新加载");

    assert_eq!(loaded.theme_selection(), selection);
    assert_eq!(loaded.startup_display_uuid(), Some("display-uuid"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn version_one_preferences_default_to_system_primary_display() {
    let directory = temporary_directory("version-one-preferences");
    let path = directory.join("settings.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试配置目录");
    fs::write(
        &path,
        "schema_version = 1\n\n[appearance]\ntheme_preset = \"xuwe\"\ncolor_scheme = \"dark\"\n",
    )
    .expect("应当可以写入旧版用户配置");
    let store = UserConfigStore::<ConsolePreferences>::at_path(&path);

    let loaded = store
        .load_versioned_or_default()
        .expect("旧版用户偏好应当使用新字段默认值加载");

    assert_eq!(loaded.startup_display_uuid(), None);
    assert_eq!(
        loaded.theme_selection(),
        ThemeSelection::new(ThemePreset::Xuwe, ColorScheme::Dark)
    );
    _ = fs::remove_dir_all(directory);
}

#[gpui::test]
fn settings_window_uses_selected_display_and_expected_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供主显示器");
        let display_uuid = display.uuid().expect("测试显示器应当提供稳定 UUID");
        let window_size = size(px(860.0), px(680.0));

        let options = settings_window_options(Some(display_uuid.to_string().as_str()), cx);

        assert_eq!(options.display_id, Some(display.id()));
        assert_eq!(
            options
                .window_bounds
                .expect("应当生成设置窗口边界")
                .get_bounds(),
            centered_window_bounds(display.visible_bounds(), window_size)
        );
        assert_eq!(options.window_min_size, Some(size(px(680.0), px(520.0))));
    });
}

#[test]
fn startup_display_setting_uses_full_width_vertical_layout() {
    let item = startup_display_setting_item(vec![(
        "display-uuid".into(),
        "显示器 2（1920 × 1080）".into(),
    )]);
    let SettingItem::Item { layout, .. } = item else {
        panic!("默认显示器应当使用标准设置项");
    };

    assert_eq!(layout, Axis::Vertical);
}

#[test]
fn projects_navigation_exposes_child_routes() {
    let projects = feature_catalog()
        .iter()
        .find(|feature| feature.id() == FeatureId::Projects)
        .unwrap();
    let child_ids = projects
        .children()
        .iter()
        .map(|child| child.id())
        .collect::<Vec<_>>();

    assert_eq!(
        child_ids,
        vec![
            FeatureId::Projects,
            FeatureId::ProjectTemplates,
            FeatureId::ProjectEnvironments,
        ]
    );
    assert_eq!(projects.children()[1].title(), "模板项目");
    assert!(projects.contains(FeatureId::ProjectEnvironments));
}

#[test]
fn feature_catalog_includes_scroll_overflow_examples() {
    let overflow_items = feature_catalog()
        .iter()
        .filter(|feature| feature.section() == "扩展示例")
        .collect::<Vec<_>>();
    let overflow_ids = overflow_items
        .iter()
        .map(|feature| feature.id())
        .collect::<Vec<_>>();

    assert!(overflow_items.len() >= 12);
    assert_eq!(overflow_ids[0], FeatureId::VirtualScroll);
    assert_eq!(overflow_ids[8], FeatureId::Automation);
    assert_eq!(overflow_ids[12], FeatureId::Experiments);
}

#[test]
fn virtual_scroll_feature_uses_stock_table_shape() {
    assert!(virtual_scroll_stock_seeds().len() >= 20);
    assert_eq!(virtual_scroll_stock_seeds()[0].symbol(), "AAPL");
    assert_eq!(virtual_scroll_stock_seeds()[0].market(), "US");
}

#[test]
fn home_next_steps_keep_template_order() {
    assert_eq!(
        next_steps(),
        [
            "把首页替换成真实工作台数据",
            "为项目、任务、设置补充独立 Entity",
            "把常用命令接入 actions 和快捷键",
        ]
    );
}

#[test]
fn home_virtual_form_keeps_table_sample_data() {
    let rows = virtual_form_rows();

    assert!(rows.len() >= 8);
    assert_eq!(rows[0].id(), "REQ-2401");
    assert_eq!(rows[0].owner(), "Jason Lee");
    assert_eq!(rows[0].status(), "待审核");
    assert_eq!(rows[0].priority(), "高");
    assert_eq!(rows[0].amount(), "$12,480");
    assert_eq!(
        virtual_form_view_modes(),
        ["全部记录", "只看待审核", "只看高优先级"]
    );
}

#[test]
fn project_rows_keep_template_projects() {
    let rows = project_rows();
    let names = rows.iter().map(|row| row.name()).collect::<Vec<_>>();

    assert_eq!(names, vec!["Console", "Desktop Runtime", "Xuwe CLI"]);
    assert_eq!(rows[0].status().label(), "active");
}

#[test]
fn task_rows_keep_pipeline_order() {
    let rows = task_rows();
    let commands = rows.iter().map(|row| row.command()).collect::<Vec<_>>();

    assert_eq!(
        commands,
        vec![
            "cargo check --workspace",
            "xuwecli build --mode local",
            "codesign + notarytool",
            "sha256 sidecar",
        ]
    );
    assert_eq!(rows[2].status().label(), "blocked");
}

#[test]
fn root_view_defaults_to_home_feature() {
    let view = RootView::default();

    assert_eq!(view.active_feature(), FeatureId::Home);
}

#[test]
fn console_default_keeps_application_window_defaults() {
    let application = Console::default();
    let options = application.options();

    assert!(options.activate);
    assert!(options.window_size.is_some());
    assert!(options.window_min_size.is_some());
    assert!(options.window_options.is_some());
    assert!(!options.daemon_mode);
}

#[test]
fn root_view_exposes_account_menu_actions() {
    let actions = actions::account::signed_out_menu_actions();

    assert_eq!(
        actions
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>(),
        vec!["登录", "设置"]
    );
    assert_eq!(actions[0].kind(), AccountActionKind::SignIn);
    assert_eq!(actions[0].shortcut(), Some("Cmd+Shift+L"));
    assert_eq!(actions[1].kind(), AccountActionKind::Settings);
    assert_eq!(actions[1].shortcut(), Some("Cmd+,"));
}

#[test]
fn root_view_can_select_active_feature() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);

    assert_eq!(view.active_feature(), FeatureId::Tasks);
}

#[gpui::test]
fn root_view_entity_updates_navigation_inside_gpui(cx: &mut TestAppContext) {
    let root = cx.new(|_| RootView::new());

    cx.update_entity(&root, |root, cx| {
        root.select_feature(FeatureId::Tasks);
        cx.notify();
    });

    assert_eq!(
        cx.read_entity(&root, |root, _| root.active_feature()),
        FeatureId::Tasks
    );
    assert_eq!(
        cx.read_entity(&root, |root, _| root.opened_tabs().to_vec()),
        [FeatureId::Home, FeatureId::Tasks]
    );
}

#[test]
fn root_view_tracks_opened_tabs_without_duplicates() {
    let mut view = RootView::new();

    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.select_feature(FeatureId::Tasks);

    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::VirtualScroll]
    );
}

#[test]
fn root_view_navigates_browser_like_history() {
    let mut view = RootView::new();

    assert_eq!(view.navigation_history(), &[FeatureId::Home]);
    assert!(!view.can_navigate_back());
    assert!(!view.can_navigate_forward());

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);

    assert_eq!(
        view.navigation_history(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);
    assert!(view.can_navigate_back());
    assert!(!view.can_navigate_forward());

    view.navigate_back();
    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert!(view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.navigate_back();
    assert_eq!(view.active_feature(), FeatureId::Home);
    assert!(!view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.navigate_forward();
    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert!(view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.select_feature(FeatureId::Reports);
    assert_eq!(
        view.navigation_history(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::Reports]
    );
    assert_eq!(view.active_feature(), FeatureId::Reports);
    assert!(!view.can_navigate_forward());
}

#[test]
fn root_view_closes_tabs_with_active_fallback() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.close_tab(FeatureId::Tasks);

    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Home, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);

    view.close_tab(FeatureId::VirtualScroll);
    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);
    assert_eq!(view.active_feature(), FeatureId::Home);

    view.close_tab(FeatureId::Home);
    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);
    assert_eq!(view.active_feature(), FeatureId::Home);
}

#[test]
fn root_view_bulk_closes_tabs_while_preserving_pinned_tabs() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Projects);
    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Projects);

    assert_eq!(view.pinned_tabs(), &[FeatureId::Projects]);
    assert_eq!(
        view.opened_tabs(),
        &[
            FeatureId::Projects,
            FeatureId::Home,
            FeatureId::Tasks,
            FeatureId::VirtualScroll,
        ]
    );

    view.close_tabs_to_left(FeatureId::VirtualScroll);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Projects, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::Reports);
    view.close_tabs_to_right(FeatureId::Tasks);
    assert_eq!(
        view.opened_tabs(),
        &[
            FeatureId::Projects,
            FeatureId::VirtualScroll,
            FeatureId::Tasks
        ]
    );

    view.close_other_tabs(FeatureId::Tasks);
    assert_eq!(view.opened_tabs(), &[FeatureId::Projects, FeatureId::Tasks]);
    assert_eq!(view.active_feature(), FeatureId::Tasks);
}

#[test]
fn root_view_toggles_pinned_tabs_at_the_front() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Tasks);

    assert!(view.is_tab_pinned(FeatureId::VirtualScroll));
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::VirtualScroll, FeatureId::Tasks, FeatureId::Home]
    );
    assert_eq!(
        view.pinned_tabs(),
        &[FeatureId::VirtualScroll, FeatureId::Tasks]
    );

    view.toggle_pin_tab(FeatureId::VirtualScroll);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Tasks, FeatureId::VirtualScroll, FeatureId::Home]
    );
    assert_eq!(view.pinned_tabs(), &[FeatureId::Tasks]);
}

#[test]
fn root_view_keeps_pinned_tabs_out_of_regular_scroll_tabs() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Projects);
    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Projects);
    view.toggle_pin_tab(FeatureId::VirtualScroll);

    assert_eq!(
        view.pinned_tabs(),
        &[FeatureId::Projects, FeatureId::VirtualScroll]
    );
    assert_eq!(view.regular_tabs(), &[FeatureId::Home, FeatureId::Tasks]);

    view.toggle_pin_tab(FeatureId::Projects);

    assert_eq!(view.pinned_tabs(), &[FeatureId::VirtualScroll]);
    assert_eq!(
        view.regular_tabs(),
        &[FeatureId::Projects, FeatureId::Home, FeatureId::Tasks]
    );
}

fn temporary_directory(label: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "console-{label}-{}-{timestamp}",
        std::process::id()
    ))
}
