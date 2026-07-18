use contracts::{
    account::{
        PermissionResponse, ProvisionUserRequest, RoleResponse, UpdateRoleRequest, UserResponse,
        UserStatus,
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
fn account_responses_use_snake_case_and_unix_second_timestamps() {
    let now = 1_784_044_800;
    let response = UserResponse {
        id: "Ab3xY9qP".to_owned(),
        identity_id: "user-1".to_owned(),
        username: Some("tester".to_owned()),
        email: Some("user@example.com".to_owned()),
        display_name: "测试用户".to_owned(),
        avatar_url: None,
        status: UserStatus::Suspended,
        is_super_admin: false,
        created_at: now,
        updated_at: now,
        last_login_at: now,
    };

    let json = serde_json::to_value(&response).expect("用户响应应当可以序列化");
    assert_eq!(json["id"], "Ab3xY9qP");
    assert_eq!(json["status"], "suspended");
    assert_eq!(json["identity_id"], "user-1");
    assert_eq!(json["username"], "tester");
    assert_eq!(json["is_super_admin"], false);
    assert_eq!(json["created_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json.get("createdAt").is_none());
    let decoded: UserResponse =
        serde_json::from_value(json).expect("SDK 应当可以反序列化服务端用户响应");
    assert_eq!(decoded, response);

    let role = RoleResponse {
        id: 42,
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
    assert_eq!(json["permissions"][0]["id"], 7);
    assert_eq!(json["created_at"], now);
    assert_eq!(json["updated_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json["updated_at"].is_i64());
}

#[test]
fn provision_user_request_uses_explicit_snake_case_identity_id() {
    let request = ProvisionUserRequest {
        identity_id: "user-1".to_owned(),
        username: Some("tester".to_owned()),
        email: Some("user@example.com".to_owned()),
        display_name: "测试用户".to_owned(),
        avatar_url: None,
        role_ids: vec![7, 11],
    };

    let json = serde_json::to_value(&request).expect("用户开通请求应当可以序列化");
    assert_eq!(json["identity_id"], "user-1");
    assert_eq!(json["username"], "tester");
    assert_eq!(json["role_ids"], json!([7, 11]));
    assert!(json.get("identityId").is_none());
    assert_eq!(
        serde_json::from_value::<ProvisionUserRequest>(json).expect("用户开通请求应当可以反序列化"),
        request
    );

    let compatible = serde_json::from_value::<ProvisionUserRequest>(json!({
        "identity_id": "legacy-user",
        "email": null,
        "display_name": "旧客户端用户",
        "avatar_url": null
    }))
    .expect("旧客户端省略初始角色时应当保持兼容");
    assert!(compatible.role_ids.is_empty());
    assert!(compatible.username.is_none());

    let empty_roles = ProvisionUserRequest {
        identity_id: "user-with-default-role".to_owned(),
        username: None,
        email: None,
        display_name: "默认成员".to_owned(),
        avatar_url: None,
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
