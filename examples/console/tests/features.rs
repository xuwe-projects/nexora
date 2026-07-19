use std::{
    fs,
    io::{Read as _, Write as _},
    net::TcpListener,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use actions::{
    account::{AccountActionKind, SignInAccount},
    settings::OpenSettings,
};
use configuration::UserConfigStore;
use desktop::{Application as _, centered_window_bounds};
use gpui::{
    AnyElement, AppContext as _, Axis, Context, Entity, Global, InteractiveElement as _,
    IntoElement, Modifiers, ParentElement as _, Render, TestAppContext, Window, div, px, size,
};
use gpui_component::{Size, notification::NotificationList, setting::SettingItem};
use serde::Deserialize;
use ui::layout::WorkspaceLayout;
#[path = "../src/account_api.rs"]
mod account_api;
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
    FeatureId, feature_catalog, feature_catalog_sections, feature_registry,
    home::{next_steps, virtual_form_rows, virtual_form_view_modes},
    projects::project_rows,
    roles::RolesFeature,
    root::RootView,
    settings::{current_console_changelog, settings_window_options, startup_display_setting_item},
    tasks::task_rows,
    virtual_scroll::virtual_scroll_stock_seeds,
};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

struct NotificationTestRoot {
    notifications: Entity<NotificationList>,
}

#[derive(Default)]
struct WindowRouteDispatchCount(usize);

impl Global for WindowRouteDispatchCount {}

impl Render for NotificationTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.notifications.clone()
    }
}

struct RolePanelTestRoot {
    roles: Entity<RolesFeature>,
    show_roles: bool,
}

impl Render for RolePanelTestRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (content, panel_overlay): (AnyElement, Option<AnyElement>) = if self.show_roles {
            (
                self.roles.clone().into_any_element(),
                Some(self.roles.read(cx).panel_dialog()),
            )
        } else {
            (div().child("其他页面").into_any_element(), None)
        };
        let sidebar = div()
            .id("role-test-sidebar")
            .debug_selector(|| "role-test-sidebar".into());
        let title_bar = div()
            .id("role-test-title-bar")
            .debug_selector(|| "role-test-title-bar".into());
        let layout =
            WorkspaceLayout::new(sidebar, title_bar, content).with_content_padding(px(0.0));
        let layout = if let Some(panel_overlay) = panel_overlay {
            layout.with_panel_overlay(panel_overlay)
        } else {
            layout
        };

        layout.render(window, cx)
    }
}

#[gpui::test]
fn role_create_panel_dialog_survives_feature_visibility_switch(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (root, cx) = cx.add_window_view(|window, cx| RolePanelTestRoot {
        roles: cx.new(|cx| RolesFeature::new(window, cx)),
        show_roles: true,
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let open_button = cx
        .debug_bounds("open-create-role-dialog")
        .expect("角色页面应当渲染创建角色按钮");

    cx.simulate_click(open_button.center(), Modifiers::none());
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let overlay = cx
        .debug_bounds("panel-dialog-overlay")
        .expect("角色创建弹窗应当渲染在 Panel overlay 中");
    let sidebar = cx
        .debug_bounds("role-test-sidebar")
        .expect("测试工作区应当渲染 Sidebar");
    let title_bar = cx
        .debug_bounds("role-test-title-bar")
        .expect("测试工作区应当渲染标签栏内容");

    assert!(overlay.left() >= sidebar.right());
    assert!(overlay.top() >= title_bar.bottom());

    root.update_in(cx, |root, _, cx| {
        root.show_roles = false;
        cx.notify();
    });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("panel-dialog-overlay").is_none());

    root.update_in(cx, |root, _, cx| {
        root.show_roles = true;
        cx.notify();
    });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("panel-dialog-overlay").is_some());
}

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
    let config = test_auth_config();
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
        auth::apply_session(session, None, cx);
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

#[gpui::test]
fn sign_in_action_is_reachable_without_a_window_or_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        auth::init(None, None, cx);
        app::register_account_actions(cx);

        cx.dispatch_action(&SignInAccount);

        assert_eq!(auth::snapshot(cx).status.as_ref(), "未配置 OIDC_ISSUER_URL");
    });
}

#[gpui::test]
fn sign_in_action_does_not_restart_an_authenticated_session(cx: &mut TestAppContext) {
    let config = test_auth_config();
    let session = oidc::OidcSession::from_token_cache(oidc::OidcTokenCache {
        access_token: "access-token".to_owned(),
        profile: Some(oidc::OidcUserProfile {
            subject: "user-1".to_owned(),
            ..oidc::OidcUserProfile::default()
        }),
        ..oidc::OidcTokenCache::default()
    })
    .unwrap();

    cx.update(|cx| {
        auth::init(Some(config), None, cx);
        auth::apply_session(session, None, cx);
        app::register_account_actions(cx);

        cx.dispatch_action(&SignInAccount);

        let snapshot = auth::snapshot(cx);
        assert!(snapshot.authenticated);
        assert!(!snapshot.busy);
        assert_eq!(snapshot.status.as_ref(), "已登录");
    });
}

#[test]
fn desktop_login_validates_access_through_real_me_endpoint() {
    let body = r#"{
        "user": {
            "id": "User0001",
            "identity_id": "user-1",
            "email": "user@example.com",
            "display_name": "测试用户",
            "avatar_url": null,
            "status": "active",
            "is_super_admin": false,
            "created_at": 1,
            "updated_at": 2,
            "last_login_at": 3
        },
        "roles": [],
        "permissions": []
    }"#;
    let (base_url, server) = serve_single_api_response("200 OK", body);
    let config = auth::AuthConfig::new(test_oidc_config(), base_url.as_str())
        .expect("loopback 业务 API 配置应当有效");

    let profile = auth::validate_session_access(&config, &test_oidc_session())
        .expect("已有本地用户应当通过业务登录门禁");
    let request = server.join().expect("测试业务服务线程应当结束");

    assert_eq!(profile.user.id, "User0001");
    assert!(request.starts_with("GET /me HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer access-token\r\n")
    );
}

#[test]
fn desktop_login_preserves_account_not_registered_error() {
    let body = r#"{
        "error": {
            "code": "account_not_registered",
            "message": "当前账号尚未在系统中开通，禁止登录",
            "details": {},
            "request_id": "req_test_unknown_user"
        }
    }"#;
    let (base_url, server) = serve_single_api_response("403 Forbidden", body);
    let config = auth::AuthConfig::new(test_oidc_config(), base_url.as_str())
        .expect("loopback 业务 API 配置应当有效");

    let error = auth::validate_session_access(&config, &test_oidc_session())
        .expect_err("不存在的本地用户必须被拒绝");
    _ = server.join().expect("测试业务服务线程应当结束");

    assert!(matches!(
        error,
        auth::AuthError::ApiRejected {
            status: 403,
            ref code,
            ref request_id,
            ..
        } if code == "account_not_registered" && request_id == "req_test_unknown_user"
    ));
}

#[gpui::test]
fn login_failure_request_id_copy_action_does_not_reenter_notification(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (root, cx) = cx.add_window_view(|window, cx| NotificationTestRoot {
        notifications: cx.new(|cx| NotificationList::new(window, cx)),
    });
    let notifications = root.read_with(cx, |root, _| root.notifications.clone());
    let error = auth::AuthError::ApiRejected {
        status: 403,
        code: "account_not_registered".to_owned(),
        message: "当前账号尚未在系统中开通，禁止登录".to_owned(),
        request_id: "req_test_copy_request_id".to_owned(),
    };

    notifications.update_in(cx, |notifications, window, cx| {
        notifications.push(auth::login_failure_notification(&error), window, cx);
    });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let copy_button = cx
        .debug_bounds("copy-login-request-id")
        .expect("登录失败通知应当渲染请求 ID 复制按钮");

    cx.simulate_click(copy_button.center(), Modifiers::none());

    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("req_test_copy_request_id".to_owned())
    );
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

#[test]
fn desktop_login_rejects_insecure_remote_api() {
    let error = auth::AuthConfig::new(test_oidc_config(), "http://api.example.com")
        .expect_err("远程业务 API 必须使用 HTTPS");

    assert!(error.to_string().contains("必须使用 HTTPS"));
}

#[test]
fn account_api_lists_users_with_bearer_auth_and_pagination() {
    let body = r#"{
        "items": [{
            "id": "User0001",
            "identity_id": "identity-1",
            "email": "user@example.com",
            "display_name": "测试用户",
            "avatar_url": null,
            "status": "active",
            "is_super_admin": false,
            "created_at": 1,
            "updated_at": 2,
            "last_login_at": 3
        }],
        "page": { "number": 2, "size": 25, "total": 30 }
    }"#;
    let (base_url, server) = serve_single_api_response("200 OK", body);
    let api = test_account_api(base_url.as_str());

    let page = api.list_users(2, 25).expect("用户分页请求应当成功");
    let request = server.join().expect("测试业务服务线程应当结束");

    assert_eq!(page.items[0].id, "User0001");
    assert_eq!(page.page.total, 30);
    assert!(request.starts_with("GET /users?page=2&page_size=25 HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer access-token\r\n")
    );
}

#[test]
fn account_api_replaces_user_roles_through_real_resource_endpoint() {
    let body = r#"{
        "user": {
            "id": "User0001",
            "identity_id": "identity-1",
            "email": "user@example.com",
            "display_name": "测试用户",
            "avatar_url": null,
            "status": "active",
            "is_super_admin": false,
            "created_at": 1,
            "updated_at": 2,
            "last_login_at": 3
        },
        "roles": [],
        "permissions": []
    }"#;
    let (base_url, server) = serve_single_api_response("200 OK", body);
    let api = test_account_api(base_url.as_str());

    api.replace_user_roles(
        "User0001",
        &contracts::account::ReplaceUserRolesRequest {
            role_ids: vec![1, 3],
        },
    )
    .expect("用户角色替换请求应当成功");
    let request = server.join().expect("测试业务服务线程应当结束");

    assert!(request.starts_with("PUT /users/User0001/roles HTTP/1.1\r\n"));
    assert!(request.contains(r#"{"role_ids":[1,3]}"#));
}

#[test]
fn account_api_replaces_role_permissions_through_real_resource_endpoint() {
    let body = r#"{
        "id": 7,
        "key": "project_manager",
        "name": "项目经理",
        "description": null,
        "is_system": false,
        "permissions": [],
        "created_at": 1,
        "updated_at": 2
    }"#;
    let (base_url, server) = serve_single_api_response("200 OK", body);
    let api = test_account_api(base_url.as_str());

    api.replace_role_permissions(
        7,
        &contracts::account::ReplaceRolePermissionsRequest {
            permission_ids: vec![2, 5],
        },
    )
    .expect("角色权限替换请求应当成功");
    let request = server.join().expect("测试业务服务线程应当结束");

    assert!(request.starts_with("PUT /roles/7/permissions HTTP/1.1\r\n"));
    assert!(request.contains(r#"{"permission_ids":[2,5]}"#));
}

#[test]
fn account_api_updates_user_status_and_creates_custom_role() {
    let user_body = r#"{
        "id": "User0001",
        "identity_id": "identity-1",
        "email": null,
        "display_name": "测试用户",
        "avatar_url": null,
        "status": "suspended",
        "is_super_admin": false,
        "created_at": 1,
        "updated_at": 2,
        "last_login_at": 3
    }"#;
    let (base_url, status_server) = serve_single_api_response("200 OK", user_body);
    let api = test_account_api(base_url.as_str());
    api.update_user_status(
        "User0001",
        &contracts::account::UpdateUserStatusRequest {
            status: contracts::account::UserStatus::Suspended,
        },
    )
    .expect("用户状态更新请求应当成功");
    let status_request = status_server.join().expect("测试业务服务线程应当结束");
    assert!(status_request.starts_with("PATCH /users/User0001 HTTP/1.1\r\n"));
    assert!(status_request.contains(r#"{"status":"suspended"}"#));

    let role_body = r#"{
        "id": 8,
        "key": "auditor",
        "name": "审计员",
        "description": "只读审计角色",
        "is_system": false,
        "permissions": [],
        "created_at": 1,
        "updated_at": 2
    }"#;
    let (base_url, role_server) = serve_single_api_response("201 Created", role_body);
    let api = test_account_api(base_url.as_str());
    api.create_role(&contracts::account::CreateRoleRequest {
        key: "auditor".to_owned(),
        name: "审计员".to_owned(),
        description: Some("只读审计角色".to_owned()),
        permission_ids: Vec::new(),
    })
    .expect("自定义角色创建请求应当成功");
    let role_request = role_server.join().expect("测试业务服务线程应当结束");
    assert!(role_request.starts_with("POST /roles HTTP/1.1\r\n"));
    assert!(role_request.contains(r#""key":"auditor""#));
}

#[test]
fn account_api_updates_and_deletes_custom_role() {
    let role_body = r#"{
        "id": 8,
        "key": "auditor",
        "name": "高级审计员",
        "description": "负责安全审计",
        "is_system": false,
        "permissions": [],
        "created_at": 1,
        "updated_at": 3
    }"#;
    let (base_url, update_server) = serve_single_api_response("200 OK", role_body);
    let api = test_account_api(base_url.as_str());
    api.update_role(
        8,
        &contracts::account::UpdateRoleRequest {
            name: Some("高级审计员".to_owned()),
            description: contracts::patch::PatchField::Value("负责安全审计".to_owned()),
        },
    )
    .expect("自定义角色更新请求应当成功");
    let update_request = update_server.join().expect("测试业务服务线程应当结束");
    assert!(update_request.starts_with("PATCH /roles/8 HTTP/1.1\r\n"));
    assert!(update_request.contains(r#""name":"高级审计员""#));
    assert!(update_request.contains(r#""description":"负责安全审计""#));

    let (base_url, delete_server) = serve_single_api_response("204 No Content", "");
    let api = test_account_api(base_url.as_str());
    api.delete_role(8).expect("自定义角色删除请求应当成功");
    let delete_request = delete_server.join().expect("测试业务服务线程应当结束");
    assert!(delete_request.starts_with("DELETE /roles/8 HTTP/1.1\r\n"));
}

#[test]
fn account_api_loads_role_and_permission_catalogs() {
    let roles_body = r#"{
        "items": [{
            "id": 1,
            "key": "admin",
            "name": "管理员",
            "description": "系统内置管理员",
            "is_system": true,
            "permissions": [],
            "created_at": 1,
            "updated_at": 2
        }]
    }"#;
    let (base_url, roles_server) = serve_single_api_response("200 OK", roles_body);
    let api = test_account_api(base_url.as_str());
    let roles = api.list_roles().expect("角色目录请求应当成功");
    let roles_request = roles_server.join().expect("测试业务服务线程应当结束");
    assert_eq!(roles[0].key, "admin");
    assert!(roles[0].is_system);
    assert!(roles_request.starts_with("GET /roles HTTP/1.1\r\n"));

    let permissions_body = r#"{
        "items": [{
            "id": 1,
            "key": "users:read",
            "name": "查看用户",
            "description": "查看用户列表与详情"
        }]
    }"#;
    let (base_url, permissions_server) = serve_single_api_response("200 OK", permissions_body);
    let api = test_account_api(base_url.as_str());
    let permissions = api.list_permissions().expect("权限目录请求应当成功");
    let permissions_request = permissions_server.join().expect("测试业务服务线程应当结束");
    assert_eq!(permissions[0].key, "users:read");
    assert!(permissions_request.starts_with("GET /permissions HTTP/1.1\r\n"));
}

fn test_auth_config() -> auth::AuthConfig {
    test_auth_config_for_base_url("http://127.0.0.1:3000")
}

fn test_auth_config_for_base_url(base_url: &str) -> auth::AuthConfig {
    auth::AuthConfig::new(test_oidc_config(), base_url).expect("业务 API 测试配置应当有效")
}

fn test_account_api(base_url: &str) -> account_api::AccountApi {
    let config = test_auth_config_for_base_url(base_url);
    account_api::AccountApi::new(config.api_session("access-token"))
        .expect("测试账号 API 客户端应当创建成功")
}

fn test_oidc_config() -> oidc::OidcConfig {
    oidc::OidcConfig::with_default_scopes(
        "https://id.example.com",
        "console",
        "http://127.0.0.1:0/auth/callback",
    )
    .expect("OIDC 测试配置应当有效")
}

fn test_oidc_session() -> oidc::OidcSession {
    oidc::OidcSession::from_token_cache(oidc::OidcTokenCache {
        access_token: "access-token".to_owned(),
        profile: Some(oidc::OidcUserProfile {
            subject: "user-1".to_owned(),
            ..oidc::OidcUserProfile::default()
        }),
        ..oidc::OidcTokenCache::default()
    })
    .expect("OIDC 测试会话应当有效")
}

fn serve_single_api_response(status: &str, body: &str) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以监听 loopback 测试端口");
    let address = listener.local_addr().expect("测试端口应当有本地地址");
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nx-request-id: req_test_unknown_user\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("测试业务服务应当收到连接");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2_048];
        while !http_request_complete(&request) {
            let read = stream.read(&mut buffer).expect("应当可以读取测试请求");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        stream
            .write_all(response.as_bytes())
            .expect("应当可以写入测试响应");
        String::from_utf8(request).expect("HTTP 测试请求应当是 UTF-8")
    });
    (format!("http://{address}"), server)
}

fn http_request_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    request.len() >= header_end + 4 + content_length
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
            FeatureId::Users,
            FeatureId::Roles,
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
    assert_eq!(FeatureId::Users.id(), "users");
    assert_eq!(FeatureId::Users.title(), "用户管理");
    assert_eq!(FeatureId::Roles.id(), "roles");
    assert_eq!(FeatureId::Roles.title(), "角色管理");
    assert_eq!(FeatureId::from_id("users"), Some(FeatureId::Users));
    assert_eq!(FeatureId::from_id("roles"), Some(FeatureId::Roles));
    assert_eq!(FeatureId::VirtualScroll.title(), "虚拟滚动");
}

#[test]
fn sidebar_navigation_groups_include_access_control() {
    let sections = feature_catalog_sections()
        .map(|(section, items)| {
            (
                section,
                items.iter().map(|feature| feature.id()).collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        sections[1],
        ("访问控制", vec![FeatureId::Users, FeatureId::Roles])
    );
}

#[test]
fn settings_load_current_console_changelog() {
    let entry = current_console_changelog().unwrap().unwrap();

    assert_eq!(entry.component(), "console");
    assert_eq!(entry.version().to_string(), env!("CARGO_PKG_VERSION"));
    assert_eq!(entry.locale(), "zh-CN");
    assert!(entry.markdown().contains("服务端客户门户能力"));
    assert!(entry.markdown().contains("VerifiedIdentity"));
    assert!(!entry.markdown().contains("CrudTableRow"));
    assert!(!entry.markdown().contains("DMG"));
}

#[test]
fn console_preferences_keep_theme_selection() {
    let mut preferences = ConsolePreferences::default();
    let selection = ThemeSelection::new(ThemePreset::Nexora, ColorScheme::Dark);

    preferences.set_theme_selection(selection);

    assert_eq!(preferences.theme_selection(), selection);
}

#[test]
fn console_preferences_round_trip_theme_display_font_size_component_size_and_pinned_tabs() {
    let directory = temporary_directory("preferences");
    let path = directory.join("settings.toml");
    let store = UserConfigStore::<ConsolePreferences>::at_path(&path);
    let mut preferences = ConsolePreferences::default();
    let selection = ThemeSelection::new(ThemePreset::Nexora, ColorScheme::Dark);
    preferences.set_theme_selection(selection);
    preferences.set_startup_display_uuid(Some("display-uuid".to_owned()));
    preferences.set_font_size(18);
    preferences.set_component_size(Size::Large);
    preferences.set_pinned_tab_paths(&["/projects".to_owned(), "/tasks".to_owned()]);

    store.save(&preferences).expect("用户偏好应当可以保存");
    let loaded = store
        .load_versioned_or_default()
        .expect("用户偏好应当可以重新加载");

    assert_eq!(loaded.theme_selection(), selection);
    assert_eq!(loaded.startup_display_uuid(), Some("display-uuid"));
    assert_eq!(loaded.font_size(), 18);
    assert_eq!(loaded.component_size(), Size::Large);
    assert_eq!(loaded.pinned_tab_paths(), vec!["/projects", "/tasks"]);
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
    assert_eq!(loaded.font_size(), theme::DEFAULT_FONT_SIZE);
    assert_eq!(loaded.component_size(), theme::DEFAULT_COMPONENT_SIZE);
    assert!(loaded.pinned_tab_paths().is_empty());
    assert_eq!(
        loaded.theme_selection(),
        ThemeSelection::new(ThemePreset::Nexora, ColorScheme::Dark)
    );
    _ = fs::remove_dir_all(directory);
}

#[test]
fn console_preferences_ignore_unknown_and_duplicate_pinned_tabs() {
    let directory = temporary_directory("unknown-pinned-tabs");
    let path = directory.join("settings.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试配置目录");
    fs::write(
        &path,
        "schema_version = 3\n\n[workspace]\npinned_tabs = [\"projects\", \"removed-feature\", \"projects\", \"tasks\"]\n",
    )
    .expect("应当可以写入包含过期标签的用户配置");
    let store = UserConfigStore::<ConsolePreferences>::at_path(&path);

    let loaded = store
        .load_versioned_or_default()
        .expect("过期标签标识不应阻止其他偏好加载");

    assert_eq!(loaded.pinned_tab_paths(), vec!["/projects", "/tasks"]);
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

    assert_eq!(names, vec!["Console", "Desktop Runtime", "Nexora CLI"]);
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
            "nexora build --mode local",
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
    assert_eq!(
        actions[1].shortcut(),
        Some(actions::settings::shortcut_label())
    );
}

#[test]
fn root_view_can_select_active_feature() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);

    assert_eq!(view.active_feature(), FeatureId::Tasks);
}

#[test]
fn nexora_registry_keeps_hidden_feature_and_window_routable() {
    let registry = feature_registry();

    assert!(
        !registry
            .navigation_features()
            .any(|metadata| metadata.id() == "user-details")
    );
    assert_eq!(
        registry
            .resolve("/users/details/42")
            .unwrap()
            .target()
            .kind(),
        nexora::RouteTargetKind::Feature
    );
    assert_eq!(
        registry.resolve("/settings").unwrap().target().kind(),
        nexora::RouteTargetKind::Window
    );
}

#[test]
fn root_view_uses_concrete_paths_for_dynamic_tabs_and_history() {
    let mut view = RootView::new();

    view.select_path("/users/details/1").unwrap();
    view.select_path("/users/details/2").unwrap();
    view.select_path("/users/details/1").unwrap();

    assert_eq!(view.active_feature(), FeatureId::UserDetails);
    assert_eq!(view.active_path(), "/users/details/1");
    assert_eq!(
        view.opened_paths(),
        ["/", "/users/details/1", "/users/details/2"]
    );
    assert_eq!(
        view.navigation_history_paths(),
        [
            "/",
            "/users/details/1",
            "/users/details/2",
            "/users/details/1"
        ]
    );

    view.toggle_pin_path("/users/details/2");
    assert_eq!(view.pinned_paths(), ["/users/details/2"]);
    view.close_path("/users/details/1");
    assert_eq!(view.opened_paths(), ["/users/details/2", "/"]);
}

#[test]
fn root_view_canonicalizes_path_aliases_and_refreshes_query_state() {
    #[derive(Deserialize)]
    struct TabQuery {
        tab: String,
    }

    let mut view = RootView::new();

    view.select_path("/users/details/%41?tab=summary").unwrap();
    view.select_path("/users/details/A?tab=roles").unwrap();

    assert_eq!(view.opened_paths(), ["/", "/users/details/A"]);
    let nexora::Query(query): nexora::Query<TabQuery> = view.active_route().query().unwrap();
    assert_eq!(query.tab, "roles");

    view.navigate_back();
    assert_eq!(view.active_path(), "/");
    view.navigate_forward();
    assert_eq!(view.active_path(), "/users/details/A");
    let nexora::Query(query): nexora::Query<TabQuery> = view.active_route().query().unwrap();
    assert_eq!(query.tab, "roles");
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

#[gpui::test]
fn root_view_releases_feature_runtime_with_its_window(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        auth::init(None, None, cx);
    });
    let (root, cx) = cx.add_window_view(|window, cx| {
        let mut root = RootView::new();
        root.initialize_feature_state(window, cx);
        root
    });
    let weak_root = root.downgrade();

    drop(root);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();

    assert!(weak_root.upgrade().is_none());
}

#[gpui::test]
fn root_view_opens_window_routes_without_creating_tabs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(WindowRouteDispatchCount::default());
        cx.on_action(|_: &OpenSettings, cx| {
            cx.global_mut::<WindowRouteDispatchCount>().0 += 1;
        });
    });
    let root = cx.new(|_| RootView::new());

    let target = cx.update_entity(&root, |root, cx| root.open_path("/settings", cx).unwrap());

    assert_eq!(target, nexora::RouteTargetKind::Window);
    assert_eq!(cx.update(|cx| cx.global::<WindowRouteDispatchCount>().0), 1);
    assert_eq!(
        cx.read_entity(&root, |root, _| root
            .opened_paths()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()),
        ["/"]
    );

    cx.update_entity(&root, |root, cx| {
        root.open_path("nexora://settings", cx).unwrap();
    });
    assert_eq!(cx.update(|cx| cx.global::<WindowRouteDispatchCount>().0), 2);
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
fn root_view_restores_pinned_tabs_at_startup() {
    let view = RootView::with_pinned_tabs(vec![FeatureId::Tasks, FeatureId::Projects]);

    assert_eq!(view.pinned_tabs(), &[FeatureId::Tasks, FeatureId::Projects]);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Tasks, FeatureId::Projects, FeatureId::Home]
    );
    assert_eq!(view.active_feature(), FeatureId::Home);
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
