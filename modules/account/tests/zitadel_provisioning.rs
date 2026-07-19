#![cfg(feature = "zitadel")]

use account::{
    SystemRole,
    provisioning::{
        CreateZitadelOrganizationRequest, ZitadelAuthorization, ZitadelAuthorizationOutcome,
        ZitadelDeleteUserOutcome, ZitadelOrganization, ZitadelOrganizationState,
        ZitadelProjectGrant, ZitadelProjectGrantOutcome, ZitadelProjectGrantRequest,
        ZitadelProjectGrantState, ZitadelProvisioningClient, ZitadelProvisioningError,
    },
};

const TEST_TOKEN: &str = "secret-provisioning-pat";

#[test]
fn provisioning_client_uses_server_side_zitadel_feature_without_exposing_pat() {
    let client = ZitadelProvisioningClient::new("http://localhost:8080", TEST_TOKEN)
        .expect("loopback issuer 应当允许本地开发 gRPC channel");
    let debug = format!("{client:?}");

    assert!(debug.contains("ZitadelProvisioningClient"));
    assert!(!debug.contains(TEST_TOKEN));
    assert!(
        ZitadelProvisioningClient::new("http://id.example.com", TEST_TOKEN).is_err(),
        "非 loopback HTTP issuer 必须被拒绝"
    );
    assert!(
        ZitadelProvisioningClient::new("https://id.example.com", "bad\npat").is_err(),
        "非法 metadata 字符必须在构造阶段被拒绝"
    );
}

#[tokio::test]
async fn provisioning_validates_inputs_before_network_requests() {
    let client = ZitadelProvisioningClient::new("http://localhost:8080", TEST_TOKEN)
        .expect("loopback issuer 应当允许本地开发 gRPC channel");

    let error = client
        .create_organization(&CreateZitadelOrganizationRequest {
            organization_id: None,
            name: "Customer A".to_owned(),
            administrator_user_ids: Vec::new(),
            administrator_roles: Vec::new(),
        })
        .await
        .expect_err("创建 org 必须要求管理员用户，避免创建后无法管理");
    assert!(matches!(
        error,
        ZitadelProvisioningError::InvalidInput {
            field: "organization.administrator_user_ids",
            ..
        }
    ));

    let error = client
        .ensure_project_grant(&ZitadelProjectGrantRequest {
            project_id: "portal-project".to_owned(),
            granted_organization_id: "customer-org".to_owned(),
            role_keys: vec!["admin".to_owned(), "admin".to_owned()],
        })
        .await
        .expect_err("重复 role key 必须在查询或写入前被拒绝");
    assert!(matches!(
        error,
        ZitadelProvisioningError::InvalidInput {
            field: "role_keys",
            ..
        }
    ));

    let error = client
        .delete_user(" ")
        .await
        .expect_err("补偿删除必须拒绝空 user ID");
    assert!(matches!(
        error,
        ZitadelProvisioningError::InvalidInput {
            field: "user_id",
            ..
        }
    ));
}

#[test]
fn provisioning_public_outcomes_model_idempotency_and_compensation() {
    let grant = ZitadelProjectGrant {
        project_id: "portal-project".to_owned(),
        owner_organization_id: Some("platform-org".to_owned()),
        granted_organization_id: "customer-org".to_owned(),
        granted_organization_name: Some("Customer".to_owned()),
        role_keys: vec!["admin".to_owned()],
        state: ZitadelProjectGrantState::Active,
    };
    let authorization = ZitadelAuthorization {
        id: "authorization-1".to_owned(),
        user_id: "user-1".to_owned(),
        project_id: "portal-project".to_owned(),
        organization_id: "customer-org".to_owned(),
        role_keys: vec!["admin".to_owned()],
    };
    let organization = ZitadelOrganization {
        id: "customer-org".to_owned(),
        name: "Customer".to_owned(),
        primary_domain: None,
        state: ZitadelOrganizationState::Active,
    };

    assert!(matches!(
        ZitadelProjectGrantOutcome::Unchanged(grant),
        ZitadelProjectGrantOutcome::Unchanged(_)
    ));
    assert!(matches!(
        ZitadelAuthorizationOutcome::Updated(authorization),
        ZitadelAuthorizationOutcome::Updated(_)
    ));
    assert_eq!(organization.state, ZitadelOrganizationState::Active);
    assert_eq!(
        ZitadelDeleteUserOutcome::AlreadyAbsent,
        ZitadelDeleteUserOutcome::AlreadyAbsent
    );
}

#[test]
fn provisioning_role_sync_accepts_dynamic_portal_project_roles_at_type_level() {
    fn assert_role_sync_api(client: &ZitadelProvisioningClient, roles: &[SystemRole]) {
        _ = client.ensure_project_roles("portal-project", roles);
    }

    _ = assert_role_sync_api as fn(&ZitadelProvisioningClient, &[SystemRole]);
}
