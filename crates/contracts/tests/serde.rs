use contracts::{
    account::{
        CreateRoleRequest, CreateServiceAccountCredentialRequest,
        CreateServiceAccountCredentialResponse, CreateServiceAccountRequest, PermissionResponse,
        ProvisionUserRequest, ReplaceRolePermissionsRequest, ReplaceUserRolesRequest, RoleResponse,
        SYSTEM_ROLE_OWNER, ServiceAccountCredentialResponse, ServiceAccountCredentialSecret,
        ServiceAccountCredentialSource, ServiceAccountCredentialStatus,
        ServiceAccountCredentialType, UpdateRoleRequest, UserListQuery, UserResponse, UserStatus,
        UserType,
    },
    pagination::{PageMetadata, PageQuery, PageResponse},
    patch::PatchField,
};
use serde_json::json;

#[test]
fn update_role_request_preserves_missing_null_and_value_states() {
    let missing = UpdateRoleRequest {
        name: Some("开发者".to_owned()),
        description: PatchField::Missing,
    };
    assert_eq!(
        serde_json::to_value(&missing).expect("缺省 PATCH 请求应当可以序列化"),
        json!({ "name": "开发者" })
    );

    let null: UpdateRoleRequest = serde_json::from_value(json!({
        "name": null,
        "description": null
    }))
    .expect("显式 null 应当可以反序列化");
    assert_eq!(null.description, PatchField::Null);

    let value: UpdateRoleRequest = serde_json::from_value(json!({
        "name": null,
        "description": "可以发布应用"
    }))
    .expect("说明值应当可以反序列化");
    assert_eq!(
        value.description,
        PatchField::Value("可以发布应用".to_owned())
    );
}

#[test]
fn role_owner_defaults_to_system_scope_in_requests() {
    let create: CreateRoleRequest = serde_json::from_value(json!({
        "key": "customer_manager",
        "name": "客户管理员",
        "description": null,
        "permission_ids": [1]
    }))
    .expect("缺省 owner 的创建角色请求应当兼容旧客户端");
    assert_eq!(create.owner, SYSTEM_ROLE_OWNER);

    let replace_permissions: ReplaceRolePermissionsRequest = serde_json::from_value(json!({
        "permission_ids": [1, 2]
    }))
    .expect("缺省 owner 的替换权限请求应当兼容旧客户端");
    assert_eq!(replace_permissions.owner, SYSTEM_ROLE_OWNER);

    let replace_roles: ReplaceUserRolesRequest = serde_json::from_value(json!({
        "role_ids": [3, 4]
    }))
    .expect("缺省 owner 的替换用户角色请求应当兼容旧客户端");
    assert_eq!(replace_roles.owner, SYSTEM_ROLE_OWNER);

    let scoped = CreateRoleRequest {
        owner: "customer-1".to_owned(),
        key: "customer_manager".to_owned(),
        name: "客户管理员".to_owned(),
        description: None,
        permission_ids: Vec::new(),
    };
    let json = serde_json::to_value(scoped).expect("owner 作用域请求应当可以序列化");
    assert_eq!(json["owner"], "customer-1");
}

#[test]
fn account_responses_use_snake_case_and_unix_second_timestamps() {
    let now = 1_784_044_800;
    let response = UserResponse {
        id: "Ab3xY9qP".to_owned(),
        identity_id: "user-1".to_owned(),
        username: Some("tester".to_owned()),
        email: Some("user@example.com".to_owned()),
        display_name: "测试用户".to_owned(),
        description: None,
        status: UserStatus::Suspended,
        user_type: UserType::Human,
        is_super_admin: false,
        created_at: now,
        updated_at: now,
        last_login_at: now,
    };

    let json = serde_json::to_value(&response).expect("用户响应应当可以序列化");
    assert_eq!(json["id"], "Ab3xY9qP");
    assert_eq!(json["status"], "suspended");
    assert_eq!(json["user_type"], "human");
    assert_eq!(json["identity_id"], "user-1");
    assert_eq!(json["username"], "tester");
    assert_eq!(json["is_super_admin"], false);
    assert_eq!(json["created_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json.get("avatar_url").is_none());
    assert!(json.get("createdAt").is_none());
    let decoded: UserResponse =
        serde_json::from_value(json).expect("SDK 应当可以反序列化服务端用户响应");
    assert_eq!(decoded, response);

    let role = RoleResponse {
        id: 42,
        owner: "IMES".to_owned(),
        key: "project_manager".to_owned(),
        name: "项目管理员".to_owned(),
        description: None,
        is_system: false,
        permissions: vec![PermissionResponse {
            id: 7,
            key: "users:read".to_owned(),
            name: "查看用户".to_owned(),
            description: None,
        }],
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_value(role).expect("角色响应应当可以序列化");
    assert_eq!(json["id"], 42);
    assert_eq!(json["owner"], "IMES");
    assert_eq!(json["permissions"][0]["id"], 7);
    assert_eq!(json["created_at"], now);
    assert_eq!(json["updated_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json["updated_at"].is_i64());
    let decoded: RoleResponse =
        serde_json::from_value(json).expect("SDK 应当可以反序列化服务端角色响应");
    assert_eq!(decoded.owner, "IMES");
}

#[test]
fn provision_user_request_uses_profile_fields_and_snake_case() {
    let request = ProvisionUserRequest {
        username: "tester".to_owned(),
        given_name: "Test".to_owned(),
        family_name: "User".to_owned(),
        email: "user@example.com".to_owned(),
        display_name: Some("测试用户".to_owned()),
        initial_password: "imes13800000000.".to_owned(),
        require_password_change: false,
        role_ids: vec![7, 11],
    };

    let json = serde_json::to_value(&request).expect("用户开通请求应当可以序列化");
    assert_eq!(json["username"], "tester");
    assert_eq!(json["given_name"], "Test");
    assert_eq!(json["family_name"], "User");
    assert_eq!(json["initial_password"], "imes13800000000.");
    assert_eq!(json["require_password_change"], false);
    assert_eq!(json["role_ids"], json!([7, 11]));
    assert!(json.get("givenName").is_none());
    assert!(json.get("initialPassword").is_none());
    assert!(json.get("avatarUrl").is_none());
    assert!(json.get("avatar_url").is_none());
    assert!(json.get("identity_id").is_none());
    let debug = format!("{request:?}");
    assert!(!debug.contains("imes13800000000."));
    assert!(debug.contains("<redacted>"));
    assert_eq!(
        serde_json::from_value::<ProvisionUserRequest>(json).expect("用户开通请求应当可以反序列化"),
        request
    );

    assert!(
        serde_json::from_value::<ProvisionUserRequest>(json!({
            "identity_id": "legacy-user",
            "username": "legacy-user",
            "email": "legacy@example.com",
            "initial_password": "imes13800000000.",
            "display_name": "旧客户端用户"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ProvisionUserRequest>(json!({
            "username": "avatar-user",
            "given_name": "Avatar",
            "family_name": "User",
            "email": "avatar@example.com",
            "display_name": "旧头像字段用户",
            "avatar_url": "https://cdn.example.com/avatar.png",
            "initial_password": "imes13800000000.",
            "require_password_change": false
        }))
        .is_err()
    );

    let empty_roles = ProvisionUserRequest {
        username: "user-with-default-role".to_owned(),
        given_name: "Default".to_owned(),
        family_name: "Member".to_owned(),
        email: "member@example.com".to_owned(),
        display_name: Some("默认成员".to_owned()),
        initial_password: "imes13800000001.".to_owned(),
        require_password_change: false,
        role_ids: Vec::new(),
    };
    let empty_roles_json = serde_json::to_value(empty_roles).expect("空初始角色请求应当可以序列化");
    assert!(empty_roles_json.get("role_ids").is_none());
}

#[test]
fn shared_pagination_contract_keeps_defaults_and_response_shape() {
    let query: PageQuery = serde_json::from_value(json!({})).expect("空分页查询应当使用公共默认值");
    assert_eq!(query, PageQuery::default());
    assert!(serde_json::from_value::<PageQuery>(json!({ "offset": 1 })).is_err());

    let response = PageResponse {
        items: vec!["one", "two"],
        page: PageMetadata {
            number: 2,
            size: 2,
            total: 7,
        },
    };
    assert_eq!(
        serde_json::to_value(response).expect("公共分页响应应当可以序列化"),
        json!({
            "items": ["one", "two"],
            "page": { "number": 2, "size": 2, "total": 7 }
        })
    );
}

#[test]
fn user_list_query_keeps_snake_case_filters_and_page_defaults() {
    let query: UserListQuery = serde_json::from_value(json!({
        "page": 2,
        "page_size": 50,
        "keyword": "管理员",
        "status": "suspended",
        "user_type": "service_account"
    }))
    .expect("用户列表筛选查询应当可以反序列化");

    assert_eq!(query.page.page, 2);
    assert_eq!(query.page.page_size, 50);
    assert_eq!(query.keyword.as_deref(), Some("管理员"));
    assert_eq!(query.status, Some(UserStatus::Suspended));
    assert_eq!(query.user_type, Some(UserType::ServiceAccount));

    let encoded = serde_json::to_value(query).expect("用户列表筛选查询应当可以序列化");
    assert_eq!(encoded["page"], 2);
    assert_eq!(encoded["page_size"], 50);
    assert_eq!(encoded["user_type"], "service_account");
    assert!(encoded.get("userType").is_none());
    assert!(serde_json::from_value::<UserListQuery>(json!({ "unknown": true })).is_err());
}

#[test]
fn service_account_requests_reject_unknown_fields_and_keep_snake_case() {
    let request: CreateServiceAccountRequest = serde_json::from_value(json!({
        "username": "dispenser-line-a",
        "display_name": "A 线点料机",
        "description": "一号车间",
        "role_ids": [12]
    }))
    .expect("服务账号请求应当可以反序列化");
    assert_eq!(request.username, "dispenser-line-a");
    assert_eq!(
        serde_json::to_value(request).expect("服务账号请求应当可以序列化")["display_name"],
        "A 线点料机"
    );
    assert!(
        serde_json::from_value::<CreateServiceAccountRequest>(json!({
            "username": "machine-a",
            "display_name": "Machine A",
            "unknown": true
        }))
        .is_err()
    );

    let immutable_update = contracts::account::UpdateServiceAccountRequest {
        username: Some("machine-b".to_owned()),
        display_name: None,
        description: PatchField::Missing,
    };
    assert_eq!(
        serde_json::to_value(immutable_update).expect("稳定标识更新请求应可传递给服务端拒绝"),
        json!({ "username": "machine-b", "display_name": null })
    );

    let pat: CreateServiceAccountCredentialRequest = serde_json::from_value(json!({
        "credential_type": "personal_access_token",
        "name": "控制器",
        "expires_at": null
    }))
    .expect("nullable PAT 到期时间应当可以反序列化");
    assert_eq!(
        pat.credential_type,
        ServiceAccountCredentialType::PersonalAccessToken
    );
    assert_eq!(pat.expires_at, None);
    let client = serde_json::to_value(CreateServiceAccountCredentialRequest {
        credential_type: ServiceAccountCredentialType::ClientCredentials,
        name: "OAuth 客户端".to_owned(),
        expires_at: None,
    })
    .expect("Client Credentials 请求应当可以序列化");
    assert_eq!(client["credential_type"], "client_credentials");
    assert!(client.get("expires_at").is_none());
    assert!(
        serde_json::from_value::<CreateServiceAccountCredentialRequest>(json!({
            "credential_type": "personal_access_token",
            "name": "PAT",
            "expiresAt": 1_800_000_000
        }))
        .is_err()
    );
    let invalid: CreateServiceAccountCredentialRequest = serde_json::from_value(json!({
        "credential_type": "ssh_key",
        "name": "unsupported"
    }))
    .expect("未知凭据类型应进入 handler 并返回稳定业务错误码");
    assert_eq!(
        invalid.credential_type,
        ServiceAccountCredentialType::Invalid
    );
}

#[test]
fn service_account_credential_response_uses_unix_seconds_and_redacts_secret_debug() {
    let metadata = ServiceAccountCredentialResponse {
        id: 7,
        service_account_id: "SaA1b2C3".to_owned(),
        credential_type: ServiceAccountCredentialType::PersonalAccessToken,
        name: "A 线控制器".to_owned(),
        provider_credential_id: Some("provider-token-1".to_owned()),
        created_by: Some("Admin001".to_owned()),
        created_at: 1_800_000_000,
        expires_at: None,
        status: ServiceAccountCredentialStatus::Active,
        source: ServiceAccountCredentialSource::Nexora,
        revoked_by: None,
        revoked_at: None,
        last_synchronized_at: 1_800_000_010,
    };
    let response = CreateServiceAccountCredentialResponse {
        credential: metadata,
        secret: ServiceAccountCredentialSecret::PersonalAccessToken {
            token: "only-visible-once".to_owned(),
        },
    };
    let json = serde_json::to_value(&response).expect("一次性凭据响应应当可以序列化");
    assert_eq!(
        json["credential"]["credential_type"],
        "personal_access_token"
    );
    assert_eq!(json["credential"]["created_at"], 1_800_000_000_i64);
    assert!(json["credential"]["expires_at"].is_null());
    assert_eq!(json["secret"]["token"], "only-visible-once");

    let debug = format!("{response:?}");
    assert!(!debug.contains("only-visible-once"));
    assert!(debug.contains("<redacted>"));
}
