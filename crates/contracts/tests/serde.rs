use contracts::{
    account::{PermissionResponse, RoleResponse, UpdateRoleRequest, UserResponse, UserStatus},
    pagination::{PageMetadata, PageQuery, PageResponse},
    patch::PatchField,
};
use serde_json::json;
use uuid::Uuid;

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
        id: Uuid::parse_str("019f6046-8e3b-73d2-86c8-c56155c84259")
            .expect("固定测试 UUID 应当有效"),
        issuer: "https://id.example.com/".to_owned(),
        subject: "user-1".to_owned(),
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
    assert_eq!(json["status"], "suspended");
    assert_eq!(json["is_super_admin"], false);
    assert_eq!(json["created_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json.get("createdAt").is_none());
    let decoded: UserResponse =
        serde_json::from_value(json).expect("SDK 应当可以反序列化服务端用户响应");
    assert_eq!(decoded, response);

    let role = RoleResponse {
        id: Uuid::now_v7(),
        key: "project_manager".to_owned(),
        name: "项目管理员".to_owned(),
        description: None,
        is_system: false,
        permissions: Vec::<PermissionResponse>::new(),
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_value(role).expect("角色响应应当可以序列化");
    assert_eq!(json["created_at"], now);
    assert_eq!(json["updated_at"], now);
    assert!(json["created_at"].is_i64());
    assert!(json["updated_at"].is_i64());
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
