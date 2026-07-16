#![cfg(feature = "database-tests")]

use std::sync::Arc;

use account::{
    Account, AccountDependencies, AccountError, AccountInitialization,
    AccountInitializationOutcome, AccountInitializationStatus, ExternalIdentity,
    IdentityIssuerBindingOutcome, User,
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
    AccessProfileResponse, ProvisionUserRequest, ReplaceUserRolesRequest, UpdateUserStatusRequest,
    UserStatus,
};
use contracts::error::ErrorEnvelope;
use sqlx::PgPool;
use tower::ServiceExt as _;

const TEST_IDENTITY_ISSUER: &str = "https://id.example.com/";
const OTHER_IDENTITY_ISSUER: &str = "https://other-id.example.com/";

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn system_roles_expose_every_initialized_role_for_provider_sync(pool: PgPool) {
    let roles = test_account(pool)
        .await
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
    let account = test_account(pool.clone()).await;
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
    let account = test_account(pool).await;
    let profile = current_profile(&account, "ordinary-user").await;

    assert_eq!(profile.user.id, "User0001");
    assert_eq!(profile.user.identity_id, "ordinary-user");
    assert!(profile.roles.is_empty());
    assert!(profile.permissions.is_empty());
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn initialization_promotes_existing_user_and_removes_all_roles(pool: PgPool) {
    let account = test_account(pool.clone()).await;
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

    let outcome = account
        .initialize(initialization("existing-super-admin"))
        .await
        .expect("已有用户应当可以设为超级管理员");
    let AccountInitializationOutcome::Initialized { super_admin } = outcome else {
        panic!("首次初始化应返回 Initialized");
    };
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
    let account = test_account(pool.clone()).await;
    initialize_account(&account, "super-admin").await;
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
    let account = test_account(pool.clone()).await;
    assert!(matches!(
        account
            .initialization_status()
            .await
            .expect("应读取初始化状态"),
        AccountInitializationStatus::Required
    ));
    let outcome = account
        .initialize(initialization("super-admin"))
        .await
        .expect("首次初始化应当成功");
    let AccountInitializationOutcome::Initialized { super_admin } = outcome else {
        panic!("首次初始化应返回 Initialized");
    };
    let repeated_same = account
        .initialize(initialization("super-admin"))
        .await
        .expect("相同身份重复初始化应按幂等成功处理");
    assert!(matches!(
        repeated_same,
        AccountInitializationOutcome::AlreadyInitialized {
            super_admin: ref repeated
        } if repeated.id == super_admin.id
    ));
    let repeated = account
        .initialize(initialization("another-super-admin"))
        .await
        .expect_err("初始化完成后不应允许替换超级管理员");
    assert!(matches!(
        repeated,
        AccountError::Conflict {
            code: "system_already_initialized",
            ..
        }
    ));
    assert!(matches!(
        account
            .initialization_status()
            .await
            .expect("应读取完成后的初始化状态"),
        AccountInitializationStatus::Completed {
            super_admin: ref initialized
        } if initialized.id == super_admin.id
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

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn concurrent_same_identity_initialization_is_idempotent(pool: PgPool) {
    let account = test_account(pool).await;
    let first_account = account.clone();
    let second_account = account.clone();
    let (first, second) = tokio::join!(
        first_account.initialize(initialization("concurrent-super-admin")),
        second_account.initialize(initialization("concurrent-super-admin")),
    );
    let first = first.expect("第一个并发初始化请求应当成功");
    let second = second.expect("第二个并发初始化请求应当幂等成功");

    assert!(matches!(
        (&first, &second),
        (
            AccountInitializationOutcome::Initialized { super_admin: first },
            AccountInitializationOutcome::AlreadyInitialized {
                super_admin: second
            }
        ) | (
            AccountInitializationOutcome::AlreadyInitialized { super_admin: first },
            AccountInitializationOutcome::Initialized {
                super_admin: second
            }
        ) if first.id == second.id
    ));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn deployment_issuer_binding_is_idempotent_and_immutable(pool: PgPool) {
    let first = Account::bind_identity_issuer(&pool, TEST_IDENTITY_ISSUER)
        .await
        .expect("首次部署 issuer 绑定应当成功");
    assert_eq!(first, IdentityIssuerBindingOutcome::Bound);

    let repeated = Account::bind_identity_issuer(&pool, "https://id.example.com")
        .await
        .expect("规范化后的同一 issuer 应当可以安全重试");
    assert_eq!(repeated, IdentityIssuerBindingOutcome::Verified);

    let replacement = Account::bind_identity_issuer(&pool, OTHER_IDENTITY_ISSUER)
        .await
        .expect_err("部署 issuer 首次绑定后不能替换");
    assert!(matches!(replacement, AccountError::IdentityIssuerMismatch));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn concurrent_different_issuer_binding_has_exactly_one_winner(pool: PgPool) {
    let (first, second) = tokio::join!(
        Account::bind_identity_issuer(&pool, TEST_IDENTITY_ISSUER),
        Account::bind_identity_issuer(&pool, OTHER_IDENTITY_ISSUER),
    );

    assert!(matches!(
        (&first, &second),
        (
            Ok(IdentityIssuerBindingOutcome::Bound),
            Err(AccountError::IdentityIssuerMismatch)
        ) | (
            Err(AccountError::IdentityIssuerMismatch),
            Ok(IdentityIssuerBindingOutcome::Bound)
        )
    ));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn token_from_another_issuer_is_rejected_as_authentication_failure(pool: PgPool) {
    let account = test_account(pool).await;
    account
        .provision_user(identity("known-user"))
        .await
        .expect("应当可以预先开通测试用户");

    let response = router(&account)
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, "Bearer other:known-user")
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("路由应当返回认证错误");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("认证错误正文应当可读取");
    let error: ErrorEnvelope = serde_json::from_slice(&body).expect("错误响应应符合公共契约");
    assert_eq!(error.error.code, "invalid_identity_issuer");
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn authorized_administrator_can_provision_user_then_me_syncs_existing(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    initialize_account(&account, "super-admin").await;
    let administrator = account
        .provision_user(identity("administrator"))
        .await
        .expect("管理员身份应当可以预先开通");
    let administrator_role_id = system_role_id("admin", &pool).await;
    sqlx::query("INSERT INTO account.user_roles (user_id, role_id) VALUES ($1, $2)")
        .bind(administrator.id.as_str())
        .bind(administrator_role_id)
        .execute(&pool)
        .await
        .expect("应当可以授予系统管理员角色");
    account
        .provision_user(identity("ordinary-member"))
        .await
        .expect("普通成员身份应当可以预先开通");

    let request = ProvisionUserRequest {
        identity_id: "provisioned-user".to_owned(),
        email: Some("provisioned-user@example.com".to_owned()),
        display_name: "已开通用户".to_owned(),
        avatar_url: None,
    };
    let forbidden = request_json(
        &account,
        Method::POST,
        "/users".to_owned(),
        "ordinary-member",
        &request,
    )
    .await;
    assert_eq!(forbidden, StatusCode::FORBIDDEN);
    let response = request_json_response(
        &account,
        Method::POST,
        "/users".to_owned(),
        "administrator",
        &request,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get("location")
        .expect("创建响应应当包含 Location")
        .to_str()
        .expect("Location 应当是 ASCII");
    assert!(location.starts_with("/users/"));

    let profile = current_profile(&account, "provisioned-user").await;
    assert_eq!(profile.user.identity_id, "provisioned-user");

    let repeated = request_json(
        &account,
        Method::POST,
        "/users".to_owned(),
        "administrator",
        &request,
    )
    .await;
    assert_eq!(repeated, StatusCode::CONFLICT);
}

async fn test_account(pool: PgPool) -> Account {
    Account::bind_identity_issuer(&pool, TEST_IDENTITY_ISSUER)
        .await
        .expect("测试部署 issuer 应当可以绑定或核对");
    Account::new(AccountDependencies {
        pool,
        token_verifier: Arc::new(TokenIdentityVerifier),
    })
}

fn router(account: &Account) -> Router {
    with_http_layers(account.routers::<()>())
}

async fn initialize_account(account: &Account, identity_id: &str) -> User {
    match account
        .initialize(initialization(identity_id))
        .await
        .expect("账号模块初始化应当成功")
    {
        AccountInitializationOutcome::Initialized { super_admin }
        | AccountInitializationOutcome::AlreadyInitialized { super_admin } => super_admin,
    }
}

fn initialization(identity_id: &str) -> AccountInitialization {
    AccountInitialization {
        super_admin: identity(identity_id),
    }
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
    request_json_response(account, method, uri, token, body)
        .await
        .status()
}

async fn request_json_response<T: serde::Serialize>(
    account: &Account,
    method: Method,
    uri: String,
    token: &str,
    body: &T,
) -> axum::response::Response {
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
        let (issuer, subject) = token
            .strip_prefix("other:")
            .map_or((TEST_IDENTITY_ISSUER, token), |subject| {
                (OTHER_IDENTITY_ISSUER, subject)
            });
        Ok(VerifiedIdentity {
            issuer: issuer.to_owned(),
            subject: subject.to_owned(),
            email: Some(format!("{token}@example.com")),
            display_name: token.to_owned(),
            avatar_url: None,
        })
    }
}
