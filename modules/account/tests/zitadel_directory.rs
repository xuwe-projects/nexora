#![cfg(feature = "zitadel")]

use account::directory::{DirectoryError, ZitadelUserDirectory};
use grpc::{StatusCodeError, StatusError};

const TEST_TOKEN: &str = "test-bootstrap-pat";
const TEST_ORGANIZATION_ID: &str = "test-organization-id";
const TEST_PROJECT_ID: &str = "test-project-id";

#[test]
fn directory_requires_https_except_for_loopback_development() {
    assert!(
        ZitadelUserDirectory::new(
            "http://id.example.com",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_err()
    );
    assert!(
        ZitadelUserDirectory::new(
            "https://id.example.com",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_ok()
    );
    assert!(
        ZitadelUserDirectory::new(
            "http://localhost:8080",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_ok()
    );
    assert!(
        ZitadelUserDirectory::new(
            "http://127.0.0.1:8080",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_ok()
    );
}

#[test]
fn directory_rejects_invalid_issuer_and_pat() {
    assert!(
        ZitadelUserDirectory::new(
            "not-an-url",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_err()
    );
    assert!(
        ZitadelUserDirectory::new(
            "https://id.example.com?tenant=1",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_err()
    );
    assert!(
        ZitadelUserDirectory::new(
            "https://id.example.com",
            "  ",
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_err()
    );
    assert!(
        ZitadelUserDirectory::new(
            "https://id.example.com",
            "invalid\npat",
            TEST_ORGANIZATION_ID,
            TEST_PROJECT_ID,
        )
        .is_err()
    );
    assert!(
        ZitadelUserDirectory::new("https://id.example.com", TEST_TOKEN, "  ", TEST_PROJECT_ID,)
            .is_err()
    );
    assert!(
        ZitadelUserDirectory::new(
            "https://id.example.com",
            TEST_TOKEN,
            TEST_ORGANIZATION_ID,
            "  ",
        )
        .is_err()
    );
}

#[tokio::test]
async fn explicit_identity_id_must_not_be_empty() {
    let directory = ZitadelUserDirectory::new(
        "http://localhost:8080",
        TEST_TOKEN,
        TEST_ORGANIZATION_ID,
        TEST_PROJECT_ID,
    )
    .expect("loopback gRPC 目录应当可以创建");

    assert!(directory.active_human_user("  ").await.is_err());
}

#[test]
fn grpc_project_role_error_keeps_project_role_and_status_context() {
    let error = DirectoryError::ProjectRoleRequest {
        project_id: "project-1".to_owned(),
        role_key: "admin".to_owned(),
        code: StatusCodeError::PermissionDenied,
        message: "caller has no project.role.write permission".to_owned(),
    };

    assert_eq!(
        error.to_string(),
        "ZITADEL ProjectService v2 AddProjectRole gRPC 请求失败（project_id=project-1, role_key=admin, code=PermissionDenied, message=caller has no project.role.write permission）"
    );
}

#[test]
fn grpc_directory_error_keeps_status_code_and_message() {
    let error = DirectoryError::from(StatusError::new(
        StatusCodeError::PermissionDenied,
        "caller has no permission to list users",
    ));

    assert_eq!(
        error.to_string(),
        "ZITADEL UserService v2 gRPC 请求失败（code=PermissionDenied, message=caller has no permission to list users）"
    );
}
