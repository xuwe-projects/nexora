#![cfg(all(feature = "desktop", feature = "derive"))]

use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    thread::{self, JoinHandle},
};

use contracts::patch::PatchField;
use nexora::desktop::{
    AccountClient, AccountClientError, AccountOidcSettings, AccountSettings, ApiSettings,
    client_config,
    contract::{
        CreateRoleRequest, ProvisionUserRequest, ReplaceRolePermissionsRequest,
        ReplaceUserRolesRequest, UpdateRoleRequest, UpdateUserStatusRequest, UserStatus,
    },
};
use serde::Deserialize;

const USER_JSON: &str = r#"{
    "id":"User0001",
    "identity_id":"subject-1",
    "username":"tester",
    "email":"user@example.com",
    "display_name":"测试用户",
    "avatar_url":null,
    "status":"active",
    "is_super_admin":false,
    "created_at":1,
    "updated_at":2,
    "last_login_at":3
}"#;

const ROLE_JSON: &str = r#"{
    "id":42,
    "key":"factory_manager",
    "name":"工厂管理员",
    "description":"管理工厂",
    "is_system":false,
    "permissions":[],
    "created_at":4,
    "updated_at":5
}"#;

const ACCESS_PROFILE_JSON: &str = r#"{
    "user":{
        "id":"User0001",
        "identity_id":"subject-1",
        "username":"tester",
        "email":"user@example.com",
        "display_name":"测试用户",
        "avatar_url":null,
        "status":"active",
        "is_super_admin":false,
        "created_at":1,
        "updated_at":2,
        "last_login_at":3
    },
    "roles":[],
    "permissions":["factories:read"]
}"#;

#[derive(Debug, Deserialize, nexora::Settings)]
struct DesktopSettings {
    api: ApiSettings,
    #[nexora(account_client)]
    account: AccountSettings,
}

#[test]
fn provision_user_posts_initial_role_ids_and_accepts_created_response() {
    let (endpoint, server) =
        spawn_mock("201 Created", USER_JSON, &[("Location", "/users/User0001")]);
    let user = session(endpoint)
        .provision_user(&ProvisionUserRequest {
            identity_id: "subject-1".to_owned(),
            username: Some("tester".to_owned()),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
            role_ids: vec![7, 9],
        })
        .expect("201 响应应按 User 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(user.id, "User0001");
    assert_request(&request, "POST", "/users");
    assert_eq!(
        request_body(&request),
        r#"{"identity_id":"subject-1","username":"tester","email":"user@example.com","display_name":"测试用户","avatar_url":null,"role_ids":[7,9]}"#
    );
}

#[test]
fn update_user_status_patches_snake_case_status() {
    let (endpoint, server) = spawn_mock("200 OK", USER_JSON, &[]);
    let user = session(endpoint)
        .update_user_status(
            "User0001",
            &UpdateUserStatusRequest {
                status: UserStatus::Suspended,
            },
        )
        .expect("200 响应应按 User 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(user.identity_id, "subject-1");
    assert_request(&request, "PATCH", "/users/User0001");
    assert_eq!(request_body(&request), r#"{"status":"suspended"}"#);
}

#[test]
fn replace_user_roles_puts_complete_role_id_set() {
    let (endpoint, server) = spawn_mock("200 OK", ACCESS_PROFILE_JSON, &[]);
    let profile = session(endpoint)
        .replace_user_roles(
            "User0001",
            &ReplaceUserRolesRequest {
                role_ids: vec![7, 9],
            },
        )
        .expect("200 响应应按 AccessProfile 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(profile.permissions, ["factories:read"]);
    assert_request(&request, "PUT", "/users/User0001/roles");
    assert_eq!(request_body(&request), r#"{"role_ids":[7,9]}"#);
}

#[test]
fn create_role_posts_initial_permission_ids_and_accepts_created_response() {
    let (endpoint, server) = spawn_mock("201 Created", ROLE_JSON, &[("Location", "/roles/42")]);
    let role = session(endpoint)
        .create_role(&CreateRoleRequest {
            key: "factory_manager".to_owned(),
            name: "工厂管理员".to_owned(),
            description: Some("管理工厂".to_owned()),
            permission_ids: vec![11, 12],
        })
        .expect("201 响应应按 Role 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(role.id, 42);
    assert_request(&request, "POST", "/roles");
    assert_eq!(
        request_body(&request),
        r#"{"key":"factory_manager","name":"工厂管理员","description":"管理工厂","permission_ids":[11,12]}"#
    );
}

#[test]
fn update_role_patches_metadata_without_collapsing_explicit_null() {
    let (endpoint, server) = spawn_mock("200 OK", ROLE_JSON, &[]);
    let role = session(endpoint)
        .update_role(
            42,
            &UpdateRoleRequest {
                name: Some("工厂主管".to_owned()),
                description: PatchField::Null,
            },
        )
        .expect("200 响应应按 Role 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(role.key, "factory_manager");
    assert_request(&request, "PATCH", "/roles/42");
    assert_eq!(
        request_body(&request),
        r#"{"name":"工厂主管","description":null}"#
    );
}

#[test]
fn delete_role_uses_resource_path_and_accepts_no_content() {
    let (endpoint, server) = spawn_mock("204 No Content", "", &[]);
    session(endpoint)
        .delete_role(42)
        .expect("204 响应应视为删除成功");
    let request = server.join().expect("测试服务线程应结束");

    assert_request(&request, "DELETE", "/roles/42");
    assert!(request_body(&request).is_empty());
}

#[test]
fn replace_role_permissions_puts_complete_permission_id_set() {
    let (endpoint, server) = spawn_mock("200 OK", ROLE_JSON, &[]);
    let role = session(endpoint)
        .replace_role_permissions(
            42,
            &ReplaceRolePermissionsRequest {
                permission_ids: vec![11, 12],
            },
        )
        .expect("200 响应应按 Role 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(role.id, 42);
    assert_request(&request, "PUT", "/roles/42/permissions");
    assert_eq!(request_body(&request), r#"{"permission_ids":[11,12]}"#);
}

#[test]
fn list_permissions_gets_items_envelope() {
    let body =
        r#"{"items":[{"id":11,"key":"factories:read","name":"查看工厂","description":null}]}"#;
    let (endpoint, server) = spawn_mock("200 OK", body, &[]);
    let permissions = session(endpoint)
        .list_permissions()
        .expect("200 响应应按权限 Items 契约解码");
    let request = server.join().expect("测试服务线程应结束");

    assert_eq!(permissions[0].key, "factories:read");
    assert_request(&request, "GET", "/permissions");
}

#[test]
fn account_error_preserves_envelope_code_message_and_request_id() {
    let body = r#"{
        "error":{
            "code":"permission_denied",
            "message":"没有执行该操作的权限",
            "details":{},
            "request_id":"req_contract_01"
        }
    }"#;
    let (endpoint, server) = spawn_mock(
        "403 Forbidden",
        body,
        &[("X-Request-Id", "req_header_fallback")],
    );
    let error = session(endpoint)
        .list_permissions()
        .expect_err("403 响应必须返回结构化客户端错误");
    let request = server.join().expect("测试服务线程应结束");

    assert_request(&request, "GET", "/permissions");
    let AccountClientError::Rejected {
        status,
        code,
        message,
        request_id,
    } = error
    else {
        panic!("统一错误正文应解码为 Rejected")
    };
    assert_eq!(status, 403);
    assert_eq!(code, "permission_denied");
    assert_eq!(message, "没有执行该操作的权限");
    assert_eq!(request_id, "req_contract_01");
}

fn session(endpoint: String) -> nexora::desktop::AccountSession {
    let settings = DesktopSettings {
        api: ApiSettings { endpoint },
        account: AccountSettings {
            oidc: AccountOidcSettings {
                issuer_url: "https://identity.example.com".to_owned(),
                client_id: "desktop-client".to_owned(),
                scopes: vec!["openid".to_owned()],
                redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
            },
        },
    };
    let config = client_config(&settings, &settings.api).expect("测试 Account 客户端配置应有效");
    AccountClient::new(&config)
        .expect("应能创建 Account 客户端")
        .session("contract-access-token")
}

fn spawn_mock(status: &str, body: &str, headers: &[(&str, &str)]) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应能监听 loopback 测试端口");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("测试端口应有地址")
    );
    let extra_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("客户端应连接测试服务");
        let request = read_request(&mut stream);
        stream
            .write_all(response.as_bytes())
            .expect("应能写入测试响应");
        request
    });
    (endpoint, server)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    loop {
        let size = stream.read(&mut buffer).expect("应能读取测试请求");
        assert_ne!(size, 0, "HTTP 请求在正文完整前意外结束");
        request.extend_from_slice(&buffer[..size]);
        if request_is_complete(&request) {
            break;
        }
    }
    String::from_utf8(request).expect("测试请求必须是 UTF-8 HTTP 文本")
}

fn request_is_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    request.len() >= header_end + 4 + content_length
}

fn assert_request(request: &str, method: &str, path: &str) {
    assert!(request.starts_with(&format!("{method} {path} HTTP/1.1\r\n")));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer contract-access-token\r\n"),
        "每个 Account 请求都必须携带当前会话 Bearer token"
    );
}

fn request_body(request: &str) -> &str {
    request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP 请求必须包含 header/body 分隔符")
}
