use std::{collections::BTreeSet, sync::Arc};

use account::{
    AccessProfile, Account, AccountsStore, CreateRole, ExternalIdentity, Page, PageRequest,
    Permission, Role, StoreError, UpdateRole, User, UserStatus,
    authentication::{AccessTokenVerifier, VerificationError, VerifiedIdentity},
    permission,
};
use api::with_http_layers;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::AUTHORIZATION},
};
use chrono::Utc;
use contracts::{
    account::{
        AccessProfileResponse, UpdateRoleRequest, UpdateUserStatusRequest,
        UserStatus as ApiUserStatus,
    },
    error::ErrorEnvelope,
    patch::PatchField,
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;
use uuid::Uuid;

#[tokio::test]
async fn protected_resource_rejects_missing_bearer_token() {
    let response = test_router(profile_with_permissions([]))
        .oneshot(
            Request::builder()
                .uri("/me")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()["www-authenticate"],
        "Bearer",
        "401 响应应提示标准 Bearer scheme"
    );
    let response_request_id = response.headers()["x-request-id"]
        .to_str()
        .expect("请求 ID 响应头应当是 ASCII")
        .to_owned();
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应当符合公共错误契约");
    assert_eq!(error.error.code, "missing_access_token");
    assert_eq!(error.error.request_id, response_request_id);
}

#[tokio::test]
async fn permission_extractor_returns_forbidden_before_handler() {
    let response = test_router(profile_with_permissions([]))
        .oneshot(authenticated_request("/roles"))
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn permission_extractor_allows_authorized_request() {
    let response = test_router(profile_with_permissions([permission::ROLES_READ]))
        .oneshot(authenticated_request("/roles"))
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn current_user_response_marks_the_built_in_super_administrator() {
    let mut profile = profile_with_permissions([]);
    profile.user.is_super_admin = true;
    let response = test_router(profile)
        .oneshot(authenticated_request("/me"))
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("当前用户响应正文应当可读取");
    let profile: AccessProfileResponse =
        serde_json::from_slice(&body).expect("响应应当符合授权快照公共契约");
    assert!(profile.user.is_super_admin);
}

#[tokio::test]
async fn invalid_path_parameter_uses_unified_error_response() {
    let response = test_router(profile_with_permissions([permission::USERS_STATUS_WRITE]))
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/users/not-a-uuid")
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdateUserStatusRequest {
                        status: ApiUserStatus::Active,
                    })
                    .expect("公共用户状态请求应当可以序列化"),
                ))
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应当符合公共错误契约");
    assert_eq!(error.error.code, "invalid_path_parameter");
}

#[tokio::test]
async fn invalid_json_body_uses_unified_error_response() {
    let response = test_router(profile_with_permissions([permission::ROLES_WRITE]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/roles")
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应当符合公共错误契约");
    assert_eq!(error.error.code, "invalid_json_body");
}

#[tokio::test]
async fn role_patch_distinguishes_missing_description_from_explicit_null() {
    let mut profile = profile_with_permissions([permission::ROLES_WRITE]);
    let role_id = Uuid::now_v7();
    let now = Utc::now();
    profile.roles.push(Role {
        id: role_id,
        key: "developer".to_owned(),
        name: "开发者".to_owned(),
        description: Some("旧说明".to_owned()),
        is_system: false,
        permissions: Vec::new(),
        created_at: now,
        updated_at: now,
    });
    let response = test_router(profile)
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/roles/{role_id}"))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&UpdateRoleRequest {
                        name: None,
                        description: PatchField::Null,
                    })
                    .expect("公共角色更新请求应当可以序列化"),
                ))
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::OK);
}

fn test_router(profile: AccessProfile) -> axum::Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:5432/test")
        .expect("惰性测试连接池配置应当有效");
    let store = Arc::new(StaticStore { profile });
    let account = Account::with_store(pool, store, Arc::new(StaticVerifier));
    with_http_layers(account.routers::<()>())
}

fn authenticated_request(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, "Bearer test-token")
        .body(Body::empty())
        .expect("测试请求应当有效")
}

struct StaticVerifier;

#[async_trait]
impl AccessTokenVerifier for StaticVerifier {
    async fn verify(&self, _token: &str) -> Result<VerifiedIdentity, VerificationError> {
        Ok(VerifiedIdentity {
            issuer: "https://id.example.com/".to_owned(),
            subject: "test-user".to_owned(),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
        })
    }
}

struct StaticStore {
    profile: AccessProfile,
}

#[async_trait]
impl AccountsStore for StaticStore {
    async fn sync_identity(&self, _identity: &ExternalIdentity) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn super_admin(&self) -> Result<Option<User>, StoreError> {
        Ok(self
            .profile
            .user
            .is_super_admin
            .then(|| self.profile.user.clone()))
    }

    async fn bind_super_admin(&self, _identity: &ExternalIdentity) -> Result<User, StoreError> {
        let mut user = self.profile.user.clone();
        user.is_super_admin = true;
        Ok(user)
    }

    async fn access_profile(&self, _user_id: Uuid) -> Result<AccessProfile, StoreError> {
        Ok(self.profile.clone())
    }

    async fn list_users(&self, request: PageRequest) -> Result<Page<User>, StoreError> {
        Ok(Page::new(vec![self.profile.user.clone()], 1, request))
    }

    async fn user(&self, _user_id: Uuid) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn set_user_status(
        &self,
        _user_id: Uuid,
        _status: UserStatus,
    ) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        Ok(self.profile.roles.clone())
    }

    async fn role(&self, _role_id: Uuid) -> Result<Role, StoreError> {
        self.profile
            .roles
            .first()
            .cloned()
            .ok_or(StoreError::NotFound("角色"))
    }

    async fn create_role(&self, _input: &CreateRole) -> Result<Role, StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn update_role(&self, _role_id: Uuid, _input: &UpdateRole) -> Result<Role, StoreError> {
        self.profile
            .roles
            .first()
            .cloned()
            .ok_or(StoreError::NotFound("角色"))
    }

    async fn delete_role(&self, _role_id: Uuid) -> Result<(), StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn replace_role_permissions(
        &self,
        _role_id: Uuid,
        _permission_ids: &[Uuid],
    ) -> Result<Role, StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        Ok(Vec::new())
    }

    async fn replace_user_roles(
        &self,
        _user_id: Uuid,
        _role_ids: &[Uuid],
        _granted_by: Uuid,
    ) -> Result<AccessProfile, StoreError> {
        Ok(self.profile.clone())
    }
}

fn profile_with_permissions(permissions: impl IntoIterator<Item = &'static str>) -> AccessProfile {
    let now = Utc::now();
    AccessProfile {
        user: User {
            id: Uuid::now_v7(),
            issuer: "https://id.example.com/".to_owned(),
            subject: "test-user".to_owned(),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
            status: UserStatus::Active,
            is_super_admin: false,
            created_at: now,
            updated_at: now,
            last_login_at: now,
        },
        roles: Vec::new(),
        permissions: permissions
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
    }
}
