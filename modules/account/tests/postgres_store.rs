#![cfg(feature = "database-tests")]

use std::sync::Arc;

use account::{
    Account, AccountError, ExternalIdentity,
    authentication::{AccessTokenVerifier, VerificationError, VerifiedIdentity},
};
use api::with_http_layers;
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header::AUTHORIZATION},
};
use contracts::account::{
    AccessProfileResponse, ReplaceUserRolesRequest, UpdateUserStatusRequest, UserStatus,
};
use contracts::error::ErrorEnvelope;
use sqlx::PgPool;
use tower::ServiceExt as _;

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn system_roles_expose_every_initialized_role_for_provider_sync(pool: PgPool) {
    let roles = test_account(pool)
        .system_roles()
        .await
        .expect("应当可以读取初始化系统角色");

    assert_eq!(
        roles
            .into_iter()
            .map(|role| (role.key, role.name))
            .collect::<Vec<_>>(),
        vec![
            ("admin".to_owned(), "系统管理员".to_owned()),
            ("auditor".to_owned(), "审计员".to_owned()),
            ("member".to_owned(), "普通成员".to_owned()),
        ]
    );
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn unknown_identity_is_denied_without_creating_local_user(pool: PgPool) {
    let account = test_account(pool.clone());
    let response = router(&account)
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, "Bearer unknown-user")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("错误响应正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应符合公共契约");
    assert_eq!(error.error.code, "account_not_registered");
    let user_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.users")
        .fetch_one(&pool)
        .await
        .expect("应当可以读取用户数量");
    assert_eq!(user_count, 0);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn existing_identity_authenticates_without_automatic_role_grant(pool: PgPool) {
    insert_user("User0001", "ordinary-user", &pool).await;
    let account = test_account(pool);
    let profile = current_profile(&account, "ordinary-user").await;

    assert_eq!(profile.user.id, "User0001");
    assert_eq!(profile.user.identity_id, "ordinary-user");
    assert!(profile.roles.is_empty());
    assert!(profile.permissions.is_empty());
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn initialization_promotes_existing_user_and_removes_all_roles(pool: PgPool) {
    let account = test_account(pool.clone());
    insert_user("Exist001", "existing-super-admin", &pool).await;
    let existing = current_profile(&account, "existing-super-admin").await;
    let administrator_role_id = system_role_id("admin", &pool).await;
    let custom_role_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO account.roles (key, name, description)
        VALUES ('project-manager', '项目管理员', '初始化前已有的自定义角色')
        RETURNING id
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以创建测试角色");
    sqlx::query(
        r#"
        INSERT INTO account.user_roles (user_id, role_id)
        VALUES ($1, $2), ($1, $3)
        "#,
    )
    .bind(existing.user.id.as_str())
    .bind(administrator_role_id)
    .bind(custom_role_id)
    .execute(&pool)
    .await
    .expect("应当可以准备已有角色关系");

    let super_admin = account
        .initialize_super_admin(&identity("existing-super-admin"))
        .await
        .expect("已有用户应当可以设为超级管理员");
    assert_eq!(super_admin.id, existing.user.id);
    assert!(super_admin.is_super_admin);
    assert!(
        account
            .is_system_initialized()
            .await
            .expect("应读取初始化状态")
    );

    let role_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account.user_roles WHERE user_id = $1")
            .bind(super_admin.id)
            .fetch_one(&pool)
            .await
            .expect("应当可以读取角色数量");
    assert_eq!(role_count, 0);
    let profile = current_profile(&account, "existing-super-admin").await;
    assert!(profile.roles.is_empty());
    assert!(profile.permissions.is_empty());
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn last_active_administrator_cannot_be_suspended_or_demoted(pool: PgPool) {
    let account = test_account(pool.clone());
    account
        .initialize_super_admin(&identity("super-admin"))
        .await
        .expect("系统初始化应当成功");
    insert_user("Admin001", "administrator", &pool).await;
    let administrator = current_profile(&account, "administrator").await;
    let administrator_role_id = system_role_id("admin", &pool).await;
    sqlx::query("INSERT INTO account.user_roles (user_id, role_id) VALUES ($1, $2)")
        .bind(administrator.user.id.as_str())
        .bind(administrator_role_id)
        .execute(&pool)
        .await
        .expect("应当可以准备系统管理员");

    let suspend = request_json(
        &account,
        Method::PATCH,
        format!("/users/{}", administrator.user.id),
        "super-admin",
        &UpdateUserStatusRequest {
            status: UserStatus::Suspended,
        },
    )
    .await;
    assert_eq!(suspend, StatusCode::CONFLICT);

    let member_role_id = system_role_id("member", &pool).await;
    let demote = request_json(
        &account,
        Method::PUT,
        format!("/users/{}/roles", administrator.user.id),
        "super-admin",
        &ReplaceUserRolesRequest {
            role_ids: vec![member_role_id],
        },
    )
    .await;
    assert_eq!(demote, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn super_administrator_is_unique_immutable_and_has_no_grants(pool: PgPool) {
    let account = test_account(pool.clone());
    assert!(
        !account
            .is_system_initialized()
            .await
            .expect("应读取初始化状态")
    );
    let super_admin = account
        .initialize_super_admin(&identity("super-admin"))
        .await
        .expect("首次初始化应当成功");
    let repeated = account
        .initialize_super_admin(&identity("another-super-admin"))
        .await
        .expect_err("初始化完成后不应允许替换超级管理员");
    assert!(matches!(
        repeated,
        AccountError::Conflict {
            code: "system_already_initialized",
            ..
        }
    ));

    let profile = current_profile(&account, "super-admin").await;
    assert!(profile.user.is_super_admin);
    assert!(profile.roles.is_empty());
    assert!(profile.permissions.is_empty());

    let suspend = request_json(
        &account,
        Method::PATCH,
        format!("/users/{}", super_admin.id),
        "super-admin",
        &UpdateUserStatusRequest {
            status: UserStatus::Suspended,
        },
    )
    .await;
    assert_eq!(suspend, StatusCode::CONFLICT);

    let administrator_role_id = system_role_id("admin", &pool).await;
    assert!(
        sqlx::query("INSERT INTO account.user_roles (user_id, role_id) VALUES ($1, $2)")
            .bind(super_admin.id.as_str())
            .bind(administrator_role_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE account.users SET identity_id = 'replaced' WHERE id = $1")
            .bind(super_admin.id.as_str())
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM account.users WHERE id = $1")
            .bind(super_admin.id.as_str())
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            r#"
            UPDATE account.system_initialization
            SET is_initialized = FALSE,
                super_admin_user_id = NULL,
                initialized_at = NULL
            WHERE id = 1
            "#,
        )
        .execute(&pool)
        .await
        .is_err()
    );
}

fn test_account(pool: PgPool) -> Account {
    Account::new(pool, Arc::new(TokenIdentityVerifier))
}

fn router(account: &Account) -> Router {
    with_http_layers(account.clone().routers::<()>())
}

async fn current_profile(account: &Account, token: &str) -> AccessProfileResponse {
    let response = router(account)
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应正文应当可读取");
    serde_json::from_slice(&body).expect("响应应当符合授权快照契约")
}

async fn request_json<T: serde::Serialize>(
    account: &Account,
    method: Method,
    uri: String,
    token: &str,
    body: &T,
) -> StatusCode {
    router(account)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(body).expect("请求契约应当可以序列化"),
                ))
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回响应")
        .status()
}

async fn system_role_id(key: &str, pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT id FROM account.roles WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("系统角色应当存在")
}

async fn insert_user(id: &str, identity_id: &str, pool: &PgPool) {
    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, email, display_name)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(identity_id)
    .bind(format!("{identity_id}@example.com"))
    .bind(identity_id)
    .execute(pool)
    .await
    .expect("应当可以准备已有本地用户");
}

fn identity(identity_id: &str) -> ExternalIdentity {
    ExternalIdentity {
        identity_id: identity_id.to_owned(),
        email: Some(format!("{identity_id}@example.com")),
        display_name: identity_id.to_owned(),
        avatar_url: None,
    }
}

struct TokenIdentityVerifier;

#[async_trait]
impl AccessTokenVerifier for TokenIdentityVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError> {
        Ok(VerifiedIdentity {
            issuer: "https://id.example.com/".to_owned(),
            subject: token.to_owned(),
            email: Some(format!("{token}@example.com")),
            display_name: token.to_owned(),
            avatar_url: None,
        })
    }
}
