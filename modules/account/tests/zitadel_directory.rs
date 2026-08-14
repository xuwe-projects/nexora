#![cfg(feature = "zitadel")]

use account::{
    __private::{
        inspect_add_client_secret_request, inspect_add_personal_access_token_request,
        inspect_create_human_user_request, inspect_create_service_account_request,
        inspect_remove_client_secret_request, inspect_remove_personal_access_token_request,
        inspect_service_account_status_mapping,
    },
    CreateHumanIdentity, CreateServiceAccountIdentity, ServiceAccountClientSecret,
    ServiceAccountDirectoryError, ServiceAccountPersonalAccessTokenSecret,
    directory::{DirectoryError, ZitadelUserDirectory},
};
use chrono::{TimeZone as _, Utc};
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

#[test]
fn create_human_user_request_marks_email_and_phone_verified() {
    let identity = human_identity("+15551234567").with_contact_phone("+15551234567");
    let inspection = inspect_create_human_user_request(
        TEST_ORGANIZATION_ID,
        &identity.identity,
        identity.contact_phone.as_deref(),
    );

    assert_eq!(inspection.organization_id, TEST_ORGANIZATION_ID);
    assert_eq!(inspection.username, "+15551234567");
    assert_eq!(inspection.email, "employee@example.com");
    assert!(inspection.email_is_verified);
    assert!(!inspection.email_send_code);
    assert_eq!(inspection.contact_phone.as_deref(), Some("+15551234567"));
    assert_eq!(inspection.phone_is_verified, Some(true));
    assert!(!inspection.phone_send_code);
}

#[test]
fn create_human_user_request_omits_phone_when_not_provided() {
    let identity = human_identity("employee-login");
    let inspection = inspect_create_human_user_request(TEST_ORGANIZATION_ID, &identity, None);

    assert!(inspection.email_is_verified);
    assert!(!inspection.email_send_code);
    assert_eq!(inspection.contact_phone, None);
    assert_eq!(inspection.phone_is_verified, None);
    assert!(!inspection.phone_send_code);
}

#[test]
fn create_service_account_request_uses_machine_user_with_jwt_access_tokens() {
    let inspection = inspect_create_service_account_request(
        TEST_ORGANIZATION_ID,
        &CreateServiceAccountIdentity {
            username: "dispenser-line-a".to_owned(),
            display_name: "A 线点料机".to_owned(),
            description: Some("一号车间".to_owned()),
        },
    );

    assert_eq!(inspection.organization_id, TEST_ORGANIZATION_ID);
    assert_eq!(inspection.username, "dispenser-line-a");
    assert_eq!(inspection.display_name, "A 线点料机");
    assert_eq!(inspection.description.as_deref(), Some("一号车间"));
    assert!(inspection.access_token_is_jwt);
}

#[test]
fn service_account_credential_requests_keep_subject_expiration_and_token_identity() {
    for inspection in [
        inspect_add_client_secret_request("machine-1"),
        inspect_remove_client_secret_request("machine-1"),
    ] {
        assert_eq!(inspection.user_id, "machine-1");
        assert_eq!(inspection.token_id, None);
        assert_eq!(inspection.expires_at, None);
    }

    let expires_at = Utc
        .with_ymd_and_hms(2030, 6, 1, 12, 30, 45)
        .single()
        .expect("测试到期时间必须有效");
    let expiring = inspect_add_personal_access_token_request("machine-1", Some(expires_at));
    assert_eq!(expiring.user_id, "machine-1");
    assert_eq!(expiring.expires_at, Some((expires_at.timestamp(), 0)));

    let permanent = inspect_add_personal_access_token_request("machine-1", None);
    assert_eq!(permanent.expires_at, None);

    let removal = inspect_remove_personal_access_token_request("machine-1", "pat-7");
    assert_eq!(removal.user_id, "machine-1");
    assert_eq!(removal.token_id.as_deref(), Some("pat-7"));
}

#[test]
fn service_account_provider_statuses_map_to_stable_categories() {
    assert!(matches!(
        inspect_service_account_status_mapping(StatusCodeError::AlreadyExists),
        ServiceAccountDirectoryError::Conflict
    ));
    assert!(matches!(
        inspect_service_account_status_mapping(StatusCodeError::NotFound),
        ServiceAccountDirectoryError::NotFound
    ));
    assert!(matches!(
        inspect_service_account_status_mapping(StatusCodeError::Unavailable),
        ServiceAccountDirectoryError::Unavailable
    ));
}

#[test]
fn service_account_secret_values_are_redacted_from_debug_output() {
    let now = Utc
        .timestamp_opt(1_800_000_000, 0)
        .single()
        .expect("测试时间戳必须有效");
    let client = ServiceAccountClientSecret {
        created_at: now,
        client_secret: "client-secret-must-not-leak".to_owned(),
    };
    let pat = ServiceAccountPersonalAccessTokenSecret {
        token_id: "pat-1".to_owned(),
        created_at: now,
        expires_at: None,
        token: "pat-must-not-leak".to_owned(),
    };

    assert!(!format!("{client:?}").contains("client-secret-must-not-leak"));
    assert!(!format!("{pat:?}").contains("pat-must-not-leak"));
}

fn human_identity(username: &str) -> CreateHumanIdentity {
    CreateHumanIdentity {
        username: username.to_owned(),
        given_name: "Test".to_owned(),
        family_name: "Employee".to_owned(),
        email: "employee@example.com".to_owned(),
        display_name: Some("Test Employee".to_owned()),
        initial_password: "correct horse battery staple".to_owned(),
        require_password_change: false,
    }
}
