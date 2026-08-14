#![cfg(feature = "desktop")]

use nexora::{AppRegistry, RouteTargetKind};

#[cfg(feature = "desktop")]
use gpui::{AnyView, Context, Modifiers, Render, TestAppContext, Window, div, prelude::*};

#[cfg(feature = "desktop")]
const ROLE_CREATE_DIALOG_SOURCE: &str =
    include_str!("../src/defaults/account/roles/components/create.rs");
#[cfg(feature = "desktop")]
const ROLE_EDITOR_SOURCE: &str = include_str!("../src/defaults/account/roles/components/editor.rs");
#[cfg(feature = "desktop")]
const USER_TABLE_SOURCE: &str = include_str!("../src/defaults/account/users/components/table.rs");
#[cfg(feature = "desktop")]
const USER_PAGE_SOURCE: &str = include_str!("../src/defaults/account/users/components/page.rs");
#[cfg(feature = "desktop")]
const USER_PROVISION_DIALOG_SOURCE: &str =
    include_str!("../src/defaults/account/users/components/provision.rs");
#[cfg(feature = "desktop")]
const SERVICE_ACCOUNT_DIALOG_SOURCE: &str =
    include_str!("../src/defaults/account/users/components/service_account.rs");
#[cfg(feature = "desktop")]
const SERVICE_ACCOUNT_CREDENTIALS_SOURCE: &str =
    include_str!("../src/defaults/account/users/components/credentials.rs");
#[cfg(feature = "desktop")]
const CREDENTIAL_SECRET_SOURCE: &str =
    include_str!("../src/defaults/account/users/components/credential_secret.rs");
#[cfg(feature = "desktop")]
const DEFAULT_SETTINGS_SOURCE: &str = include_str!("../src/defaults/settings.rs");
#[cfg(feature = "desktop")]
const DEFAULT_LOGIN_SOURCE: &str = include_str!("../src/defaults/login.rs");

#[cfg(feature = "desktop")]
struct CustomUsersFeature;

#[cfg(feature = "desktop")]
struct DefaultAccountFeatureTestRoot {
    content: AnyView,
    overlay: AnyView,
}

#[cfg(feature = "desktop")]
impl Render for DefaultAccountFeatureTestRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
            .relative()
            .size_full()
            .child(self.content.clone())
            .child(self.overlay.clone())
    }
}

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
#[test]
fn default_settings_no_longer_exposes_startup_display_picker() {
    assert!(!DEFAULT_SETTINGS_SOURCE.contains("默认显示器"));
    assert!(!DEFAULT_SETTINGS_SOURCE.contains("startup_display"));
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
fn default_login_only_connects_update_button_through_installed_updater() {
    assert!(DEFAULT_LOGIN_SOURCE.contains("crate::desktop::updater_available(cx)"));
    assert!(DEFAULT_LOGIN_SOURCE.contains("gate.on_check_updates"));
    assert!(DEFAULT_LOGIN_SOURCE.contains("crate::desktop::check_for_updates(window, cx)"));
}

#[cfg(feature = "desktop")]
#[test]
fn default_login_formats_general_release_identity_without_channel_or_build_zero_fallback() {
    assert!(DEFAULT_LOGIN_SOURCE.contains("crate::desktop::application_info(cx)"));
    assert!(DEFAULT_LOGIN_SOURCE.contains("· Build {build_number}"));
    assert!(DEFAULT_LOGIN_SOURCE.contains("application_info.build_number().map_or_else"));
    assert!(!DEFAULT_LOGIN_SOURCE.contains("Build 0"));
    assert!(!DEFAULT_LOGIN_SOURCE.contains("application_info.channel()"));
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

#[cfg(feature = "desktop")]
#[gpui::test]
fn default_users_feature_keeps_panel_form_dialog_stable_and_blocks_unprivileged_creation(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    cx.update(theme::init);
    let registry = AppRegistry::builder()
        .account_defaults(true)
        .build()
        .expect("Account 默认用户页面应当可以注册");
    let route = registry
        .resolve("/users")
        .expect("默认用户页面应当可以解析");
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let instance = registry
            .create_feature(route, window, cx)
            .expect("默认用户页面应当可以创建");
        let overlay = instance
            .panel_overlay(cx)
            .expect("创建用户与角色管理应提供内容区 FormDialog 层");
        let same_overlay = instance
            .panel_overlay(cx)
            .expect("重复读取仍应返回同一个内容区 FormDialog 层");
        assert_eq!(overlay.entity_id(), same_overlay.entity_id());
        DefaultAccountFeatureTestRoot {
            content: instance.view(),
            overlay,
        }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let open_button = cx
        .debug_bounds("open-default-account-user-dialog")
        .expect("默认用户页面应当渲染开通按钮");
    cx.simulate_click(open_button.center(), Modifiers::none());
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    assert!(
        cx.debug_bounds("panel-dialog-overlay").is_none(),
        "未登录或没有 users:provision 权限时不能打开创建用户 FormDialog"
    );

    let service_account_button = cx
        .debug_bounds("open-default-service-account-dialog")
        .expect("默认用户页面应当渲染独立的服务账号创建按钮");
    cx.simulate_click(service_account_button.center(), Modifiers::none());
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(
        cx.debug_bounds("panel-dialog-overlay").is_none(),
        "未登录或没有 service_accounts:provision 权限时不能打开服务账号 FormDialog"
    );
}

#[cfg(feature = "desktop")]
#[gpui::test]
fn default_roles_feature_keeps_overlay_stable_and_blocks_unprivileged_creation(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    cx.update(theme::init);
    let registry = AppRegistry::builder()
        .account_defaults(true)
        .build()
        .expect("Account 默认角色页面应当可以注册");
    let route = registry
        .resolve("/roles")
        .expect("默认角色页面应当可以解析");
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let instance = registry
            .create_feature(route, window, cx)
            .expect("默认角色页面应当可以创建");
        let overlay = instance
            .panel_overlay(cx)
            .expect("默认角色页面应当始终保留创建对话框 Entity");
        let same_overlay = instance
            .panel_overlay(cx)
            .expect("重复读取仍应返回创建对话框 Entity");
        assert_eq!(overlay.entity_id(), same_overlay.entity_id());
        DefaultAccountFeatureTestRoot {
            content: instance.view(),
            overlay,
        }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let open_button = cx
        .debug_bounds("open-default-account-role-dialog")
        .expect("默认角色页面应当渲染创建按钮");
    cx.simulate_click(open_button.center(), Modifiers::none());
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    assert!(
        cx.debug_bounds("panel-dialog-overlay").is_none(),
        "未登录或没有 roles:write 权限时不能打开角色创建弹窗"
    );
}

#[cfg(feature = "desktop")]
#[test]
fn default_role_forms_hide_role_key_from_operators() {
    assert!(!ROLE_CREATE_DIALOG_SOURCE.contains("FormItem::new(\"角色键\")"));
    assert!(!ROLE_EDITOR_SOURCE.contains("FormItem::new(\"角色键\")"));
    assert!(ROLE_CREATE_DIALOG_SOURCE.contains("generated_role_key("));
}

#[cfg(feature = "desktop")]
#[test]
fn default_user_management_keeps_visual_avatar_without_image_source_or_avatar_url_input() {
    assert!(USER_TABLE_SOURCE.contains("Avatar::new().name(row.avatar_name.clone()).small()"));
    assert!(
        !USER_TABLE_SOURCE.contains(".src("),
        "默认用户列表只能显示首字母/默认 Avatar，不再读取图片 URL"
    );
    assert!(
        !USER_PROVISION_DIALOG_SOURCE.contains("头像 URL"),
        "默认用户创建表单不应再暴露头像 URL 输入"
    );
}

#[cfg(feature = "desktop")]
#[test]
fn default_user_table_keeps_official_column_handles_and_copy_control() {
    assert!(USER_TABLE_SOURCE.contains("CrudListState::create_with_delegate"));
    assert!(
        USER_TABLE_SOURCE.contains("Clipboard::new(format!(\"copy-default-user-id-{}\", row.id))")
    );
    assert!(!USER_TABLE_SOURCE.contains("resizable = false"));
    assert!(USER_TABLE_SOURCE.contains("status,"));
}

#[cfg(feature = "desktop")]
#[test]
fn default_user_panel_does_not_duplicate_crud_panel_actions() {
    assert!(USER_PAGE_SOURCE.contains("CrudPanel::new"));
    assert!(!USER_PAGE_SOURCE.contains("refresh-default-account-users"));
}

#[cfg(feature = "desktop")]
#[test]
fn service_account_ui_keeps_required_component_and_lifecycle_boundaries() {
    for choice in [
        "Client Credentials（推荐）",
        "Personal Access Token",
        "暂不生成凭据",
    ] {
        assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains(choice));
    }
    assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains("open_alert_dialog"));
    assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains("service_accounts:credentials.write"));
    assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains("!can_create_credential"));
    assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains("超长期 PAT"));
    assert!(SERVICE_ACCOUNT_DIALOG_SOURCE.contains("凭据生成失败"));
    assert!(SERVICE_ACCOUNT_CREDENTIALS_SOURCE.contains("DataTable::new"));
    assert!(SERVICE_ACCOUNT_CREDENTIALS_SOURCE.contains("confirm_revoke"));
    assert!(SERVICE_ACCOUNT_CREDENTIALS_SOURCE.contains("open_alert_dialog"));
    assert!(CREDENTIAL_SECRET_SOURCE.contains("Clipboard::new"));
    assert!(CREDENTIAL_SECRET_SOURCE.contains("self.secret = None"));
    assert!(USER_TABLE_SOURCE.contains(".when(is_service_account"));
    assert!(USER_TABLE_SOURCE.contains("page.manage_service_account"));
}
