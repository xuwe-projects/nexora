#![cfg(feature = "database-tests")]

use std::sync::{Arc, Mutex};

use account::{
    Account, AccountDependencies, AccountError, AccountInitialization,
    AccountInitializationOutcome, AccountInitializationStatus, CreateHumanIdentity,
    ExternalIdentity, IdentityDirectory, IdentityDirectoryError, IdentityIssuerBindingOutcome,
    PORTAL_ADMIN_ROLE_KEY, PermissionDefinition, PermissionKey, SYSTEM_ROLE_OWNER, User,
    authentication::{AccessTokenVerifier, VerificationError, VerifiedIdentity},
    authorization::{AuthenticatedUser, Authorized, RequiredPermission},
    create_generated_role_for_owner, create_permissions, create_role, create_role_for_owner,
    create_user, create_user_with_roles, ensure_system_role_with_permissions, grant_user_role,
    replace_role_permissions, replace_role_permissions_for_owner, replace_user_roles,
    replace_user_roles_for_owner, roles_for_owner,
};
use api::with_http_layers;
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{FromRef, State},
    http::{Method, Request, StatusCode, header::AUTHORIZATION},
    routing::get,
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

#[derive(Clone)]
struct HostState {
    account: Account,
    pool: PgPool,
}

impl FromRef<HostState> for Account {
    fn from_ref(state: &HostState) -> Self {
        state.account.clone()
    }
}

struct ReadFactories;

impl RequiredPermission for ReadFactories {
    const KEY: PermissionKey = PermissionKey::from_static("factories:read");
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn host_pool_facade_manages_users_roles_and_permissions(pool: PgPool) {
    let permissions = create_permissions(
        &pool,
        &[PermissionDefinition {
            key: "projects:archive".to_owned(),
            name: "归档项目".to_owned(),
            description: Some("允许归档项目".to_owned()),
        }],
    )
    .await
    .expect("宿主应能注册应用权限");
    assert_eq!(permissions.len(), 1);
    assert_eq!(permissions[0].key.as_str(), "projects:archive");
    let admin_has_registered_permission = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM account.role_permissions role_permissions
            JOIN account.roles roles ON roles.id = role_permissions.role_id
            WHERE roles.key = 'admin'
              AND role_permissions.permission_id = $1
        )
        "#,
    )
    .bind(permissions[0].id)
    .fetch_one(&pool)
    .await
    .expect("应当可以核对系统管理员权限兜底");
    assert!(
        admin_has_registered_permission,
        "应用新注册的权限必须自动授予系统管理员角色"
    );

    let role = create_role(
        &pool,
        "project-manager",
        "项目管理员",
        Some("管理项目生命周期"),
        &[],
    )
    .await
    .expect("宿主应能创建自定义角色");
    let role = replace_role_permissions(&pool, role.id, &[permissions[0].id])
        .await
        .expect("宿主应能替换角色权限关联");
    assert_eq!(role.key, "project-manager");
    assert_eq!(role.permissions, permissions);

    let user = create_user(&pool, identity("host-managed-user"))
        .await
        .expect("宿主应能开通外部身份对应的本地用户");
    let profile = replace_user_roles(&pool, user.id.as_str(), &[role.id], user.id.as_str())
        .await
        .expect("宿主应能替换用户角色关联");
    assert_eq!(profile.user, user);
    assert!(profile.roles.iter().any(|assigned| assigned.id == role.id));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn owner_scoped_roles_crud_permissions_and_generated_keys(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let permissions = account
        .register_permissions(&[PermissionDefinition {
            key: "portal:read".to_owned(),
            name: "查看门户".to_owned(),
            description: None,
        }])
        .await
        .expect("宿主应当可以注册门户权限");
    let permission_id = permissions[0].id;

    let role = create_role_for_owner(
        &pool,
        "customer-1",
        "customer_manager",
        "客户管理员",
        Some("管理单个客户门户"),
        &[permission_id],
    )
    .await
    .expect("应当可以在客户 owner 下创建角色");
    assert_eq!(role.owner, "customer-1");
    assert_eq!(permission_keys(&role.permissions), ["portal:read"]);
    assert!(
        account
            .roles()
            .await
            .expect("默认后台角色查询应当成功")
            .iter()
            .all(|role| role.owner == SYSTEM_ROLE_OWNER)
    );

    let scoped_roles = roles_for_owner(&pool, "customer-1")
        .await
        .expect("应当可以按 owner 查询角色");
    assert_eq!(scoped_roles.len(), 1);
    assert_eq!(scoped_roles[0].id, role.id);

    let generated = create_generated_role_for_owner(
        &pool,
        "customer-1",
        "自动编码客户角色",
        None,
        &[permission_id],
    )
    .await
    .expect("应当可以创建数据库序列参与生成 key 的角色");
    assert_eq!(generated.owner, "customer-1");
    assert!(generated.key.starts_with("role_"));

    let duplicate = create_role_for_owner(
        &pool,
        "customer-2",
        "customer_manager",
        "另一个客户管理员",
        None,
        &[],
    )
    .await
    .expect_err("role key 应当保持全局唯一");
    assert!(matches!(
        duplicate,
        AccountError::Conflict {
            code: "role_key_exists",
            ..
        }
    ));

    let updated = account
        .update_role_for_owner("customer-1", role.id, Some("客户主管"), Some(None))
        .await
        .expect("应当可以按 owner 更新自定义角色");
    assert_eq!(updated.name, "客户主管");
    assert_eq!(updated.description, None);

    let replaced = replace_role_permissions_for_owner(&pool, "customer-1", role.id, &[])
        .await
        .expect("应当可以按 owner 替换权限集合");
    assert!(replaced.permissions.is_empty());

    let wrong_scope = account
        .role_for_owner(SYSTEM_ROLE_OWNER, role.id)
        .await
        .expect_err("后台默认 owner 不应读取客户角色");
    assert!(matches!(wrong_scope, AccountError::NotFound("角色")));

    account
        .delete_role_for_owner("customer-1", role.id)
        .await
        .expect("应当可以按 owner 删除未引用角色");
    let remaining = roles_for_owner(&pool, "customer-1")
        .await
        .expect("删除后仍应可查询客户 owner");
    assert_eq!(
        remaining
            .into_iter()
            .map(|role| role.id)
            .collect::<Vec<_>>(),
        vec![generated.id]
    );
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn replace_user_roles_for_owner_preserves_other_owner_roles(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let grantor = account
        .provision_user(identity("owner-scope-grantor"))
        .await
        .expect("授权人应当可以开通");
    let user = account
        .provision_user(identity("owner-scope-user"))
        .await
        .expect("目标用户应当可以开通");
    let backend_role = account
        .create_role("backend_reader", "后台查看员", None, &[])
        .await
        .expect("后台角色应当可以创建");
    let customer_a_role = account
        .create_role_for_owner(
            "customer-a",
            "customer_a_reader",
            "客户 A 查看员",
            None,
            &[],
        )
        .await
        .expect("客户 A 角色应当可以创建");
    let customer_b_role = account
        .create_role_for_owner(
            "customer-b",
            "customer_b_reader",
            "客户 B 查看员",
            None,
            &[],
        )
        .await
        .expect("客户 B 角色应当可以创建");

    replace_user_roles(
        &pool,
        user.id.as_str(),
        &[backend_role.id],
        grantor.id.as_str(),
    )
    .await
    .expect("后台角色替换应当成功");
    replace_user_roles_for_owner(
        &pool,
        "customer-a",
        user.id.as_str(),
        &[customer_a_role.id],
        grantor.id.as_str(),
    )
    .await
    .expect("客户 A 角色替换应当成功");
    replace_user_roles_for_owner(
        &pool,
        "customer-b",
        user.id.as_str(),
        &[customer_b_role.id],
        grantor.id.as_str(),
    )
    .await
    .expect("客户 B 角色替换应当成功");
    replace_user_roles_for_owner(
        &pool,
        "customer-a",
        user.id.as_str(),
        &[],
        grantor.id.as_str(),
    )
    .await
    .expect("清空客户 A 角色不应影响其他 owner");

    let profile = account
        .user_access(user.id.as_str())
        .await
        .expect("应当可以读取最终授权快照");
    let assigned = profile
        .roles
        .iter()
        .map(|role| (role.owner.as_str(), role.key.as_str()))
        .collect::<Vec<_>>();
    assert!(assigned.contains(&(SYSTEM_ROLE_OWNER, "backend_reader")));
    assert!(assigned.contains(&(SYSTEM_ROLE_OWNER, "member")));
    assert!(assigned.contains(&("customer-b", "customer_b_reader")));
    assert!(!assigned.contains(&("customer-a", "customer_a_reader")));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn system_role_sync_and_grant_user_role_are_immutable_and_idempotent(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    create_permissions(
        &pool,
        &[PermissionDefinition {
            key: "portal:admin".to_owned(),
            name: "管理门户".to_owned(),
            description: Some("允许管理客户门户".to_owned()),
        }],
    )
    .await
    .expect("门户管理员权限应当可以注册");

    let portal_role = ensure_system_role_with_permissions(
        &pool,
        PORTAL_ADMIN_ROLE_KEY,
        "门户管理员",
        Some("全局客户门户管理员"),
        &["portal:admin"],
    )
    .await
    .expect("宿主应当可以同步门户管理员系统角色");
    assert_eq!(portal_role.owner, SYSTEM_ROLE_OWNER);
    assert_eq!(portal_role.key, PORTAL_ADMIN_ROLE_KEY);
    assert!(portal_role.is_system);
    assert_eq!(permission_keys(&portal_role.permissions), ["portal:admin"]);

    let update_error = account
        .update_role(portal_role.id, Some("不可修改"), None)
        .await
        .expect_err("系统角色不可编辑");
    assert!(matches!(
        update_error,
        AccountError::Conflict {
            code: "system_role_immutable",
            ..
        }
    ));
    let delete_error = account
        .delete_role(portal_role.id)
        .await
        .expect_err("系统角色不可删除");
    assert!(matches!(
        delete_error,
        AccountError::Conflict {
            code: "system_role_immutable",
            ..
        }
    ));

    let grantor = account
        .provision_user(identity("portal-grantor"))
        .await
        .expect("授权人应当可以开通");
    let user = account
        .provision_user(identity("portal-admin-user"))
        .await
        .expect("门户管理员用户应当可以开通");
    let existing_role = account
        .create_role("ops_viewer", "运营查看员", None, &[])
        .await
        .expect("已有角色应当可以创建");
    account
        .replace_user_roles(user.id.as_str(), &[existing_role.id], grantor.id.as_str())
        .await
        .expect("预置已有角色应当成功");

    grant_user_role(&pool, user.id.as_str(), portal_role.id, grantor.id.as_str())
        .await
        .expect("首次追加门户管理员角色应当成功");
    let profile = grant_user_role(&pool, user.id.as_str(), portal_role.id, grantor.id.as_str())
        .await
        .expect("重复追加门户管理员角色应当幂等成功");
    let portal_grant_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM account.user_roles WHERE user_id = $1 AND role_id = $2",
    )
    .bind(user.id.as_str())
    .bind(portal_role.id)
    .fetch_one(&pool)
    .await
    .expect("应当可以核对门户管理员角色授权数量");
    assert_eq!(portal_grant_count, 1);
    assert!(profile.roles.iter().any(|role| role.id == portal_role.id));
    assert!(profile.roles.iter().any(|role| role.id == existing_role.id));

    let missing_grantor = grant_user_role(&pool, user.id.as_str(), portal_role.id, "Missing1")
        .await
        .expect_err("幂等重复追加也应校验授权人存在");
    assert!(matches!(missing_grantor, AccountError::NotFound("用户")));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn role_permissions_store_expanded_implied_permissions(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let permissions = account
        .register_permission_catalog(&[
            PermissionDefinition {
                key: "employees:read".to_owned(),
                name: "查看员工".to_owned(),
                description: None,
            }
            .into(),
            PermissionDefinition {
                key: "employees:write".to_owned(),
                name: "编辑员工".to_owned(),
                description: None,
            }
            .with_implies(["employees:read"]),
            PermissionDefinition {
                key: "employees:approve".to_owned(),
                name: "审批员工".to_owned(),
                description: None,
            }
            .with_implies(["employees:write", "employees:read"]),
        ])
        .await
        .expect("带蕴含关系的权限目录应当可以注册");
    let permission_id = |key: &str| {
        permissions
            .iter()
            .find(|permission| permission.key.as_str() == key)
            .map(|permission| permission.id)
            .expect("测试权限应当存在")
    };

    let role = account
        .create_role(
            "employee-editor",
            "员工编辑员",
            None,
            &[permission_id("employees:write")],
        )
        .await
        .expect("创建角色时应当展开写权限蕴含的读权限");
    assert_eq!(
        permission_keys(&role.permissions),
        ["employees:read", "employees:write"]
    );
    assert_eq!(
        stored_role_permission_keys(role.id, &pool).await,
        ["employees:read", "employees:write"]
    );

    let role = account
        .replace_role_permissions(role.id, &[permission_id("employees:approve")])
        .await
        .expect("替换角色权限时应当传递展开蕴含权限");
    assert_eq!(
        permission_keys(&role.permissions),
        ["employees:approve", "employees:read", "employees:write"]
    );
    assert_eq!(
        stored_role_permission_keys(role.id, &pool).await,
        ["employees:approve", "employees:read", "employees:write"]
    );

    let loop_permissions = account
        .register_permission_catalog(&[
            PermissionDefinition {
                key: "loops:a".to_owned(),
                name: "循环 A".to_owned(),
                description: None,
            }
            .with_implies(["loops:b"]),
            PermissionDefinition {
                key: "loops:b".to_owned(),
                name: "循环 B".to_owned(),
                description: None,
            }
            .with_implies(["loops:a"]),
        ])
        .await
        .expect("循环蕴含关系不应导致注册失败");
    let loop_a_id = loop_permissions
        .iter()
        .find(|permission| permission.key.as_str() == "loops:a")
        .map(|permission| permission.id)
        .expect("循环测试权限应当存在");
    let loop_role = account
        .create_role("loop-reader", "循环权限角色", None, &[loop_a_id])
        .await
        .expect("循环蕴含关系不应导致权限展开无限递归");
    assert_eq!(
        permission_keys(&loop_role.permissions),
        ["loops:a", "loops:b"]
    );

    let user = account
        .provision_user(identity("employee-authorized"))
        .await
        .expect("测试用户应当可以开通");
    account
        .replace_user_roles(user.id.as_str(), &[role.id], user.id.as_str())
        .await
        .expect("测试用户应当可以授予角色");

    let profile = current_profile(&account, "employee-authorized").await;
    assert_eq!(
        profile.permissions,
        ["employees:approve", "employees:read", "employees:write"]
    );
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn provisioning_with_initial_roles_is_atomic(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let grantor = account
        .provision_user(identity("grantor"))
        .await
        .expect("测试授权人应当可以开通");
    let role = account
        .create_role("factory-operator", "工厂操作员", None, &[])
        .await
        .expect("测试角色应当可以创建");

    let user = create_user_with_roles(
        &pool,
        identity("factory-user"),
        &[role.id],
        grantor.id.as_str(),
    )
    .await
    .expect("用户与初始角色应当在同一操作中创建");
    let profile = account
        .user_access(user.id.as_str())
        .await
        .expect("应当可以读取新用户授权快照");
    assert!(profile.roles.iter().any(|assigned| assigned.id == role.id));
    assert!(
        profile
            .roles
            .iter()
            .any(|assigned| assigned.key == "member")
    );
    let grantors = sqlx::query_scalar::<_, Option<String>>(
        "SELECT DISTINCT granted_by FROM account.user_roles WHERE user_id = $1",
    )
    .bind(user.id.as_str())
    .fetch_all(&pool)
    .await
    .expect("应当可以读取初始角色授权人");
    assert_eq!(grantors, vec![Some(grantor.id.clone())]);

    let error = account
        .provision_user_with_roles(identity("rollback-user"), &[i64::MAX], grantor.id.as_str())
        .await
        .expect_err("不存在的初始角色必须使整个开通操作失败");
    assert!(matches!(error, AccountError::NotFound("角色")));
    let user_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM account.users WHERE identity_id = 'rollback-user')",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以核对事务回滚结果");
    assert!(!user_exists);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn managed_user_with_initial_password_sets_directory_password(pool: PgPool) {
    let directory = Arc::new(RecordingIdentityDirectory::default());
    let account = test_account_with_directory(pool.clone(), directory.clone()).await;
    let grantor = account
        .provision_user(identity("password-grantor"))
        .await
        .expect("测试授权人应当可以开通");
    let role = account
        .create_role("employee", "员工", None, &[])
        .await
        .expect("测试角色应当可以创建");

    let user = account
        .create_managed_user_with_roles(
            password_identity("13800000000", "imes13800000000."),
            &[role.id],
            grantor.id.as_str(),
        )
        .await
        .expect("带初始密码的人类用户应当可以创建并绑定本地账号");

    assert_eq!(user.identity_id, "13800000000");
    let created = directory.created.lock().expect("测试目录记录应可读取");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].username, "13800000000");
    assert!(created[0].initial_password_matches("imes13800000000."));
    assert!(!created[0].require_password_change);
    assert_eq!(created[0].contact_phone, None);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn managed_user_with_contact_phone_passes_phone_to_directory(pool: PgPool) {
    let directory = Arc::new(RecordingIdentityDirectory::default());
    let account = test_account_with_directory(pool.clone(), directory.clone()).await;
    let grantor = account
        .provision_user(identity("phone-grantor"))
        .await
        .expect("测试授权人应当可以开通");

    let user = account
        .create_managed_user_with_roles(
            password_identity("13800000000", "imes13800000000.").with_contact_phone("13800000000"),
            &[],
            grantor.id.as_str(),
        )
        .await
        .expect("带联系手机号的人类用户应当可以创建并绑定本地账号");

    assert_eq!(user.identity_id, "13800000000");
    let created = directory.created.lock().expect("测试目录记录应可读取");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].username, "13800000000");
    assert_eq!(created[0].contact_phone.as_deref(), Some("13800000000"));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn managed_user_with_initial_password_compensates_local_binding_failure(pool: PgPool) {
    let directory = Arc::new(RecordingIdentityDirectory::default());
    let account = test_account_with_directory(pool.clone(), directory.clone()).await;
    let grantor = account
        .provision_user(identity("password-rollback-grantor"))
        .await
        .expect("测试授权人应当可以开通");

    let error = account
        .create_managed_user_with_roles(
            password_identity("rollback-password-user", "imes13800000001."),
            &[i64::MAX],
            grantor.id.as_str(),
        )
        .await
        .expect_err("本地初始角色无效时整体创建必须失败");

    assert!(matches!(error, AccountError::NotFound("角色")));
    let deleted = directory
        .deleted
        .lock()
        .expect("测试目录删除记录应可读取")
        .clone();
    assert_eq!(deleted.as_slice(), ["rollback-password-user"]);
    let user_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM account.users WHERE identity_id = 'rollback-password-user')",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以核对本地用户回滚结果");
    assert!(!user_exists);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn managed_user_with_initial_password_directory_conflict_does_not_bind_local_user(
    pool: PgPool,
) {
    let account =
        test_account_with_directory(pool.clone(), Arc::new(ConflictingIdentityDirectory)).await;
    let grantor = account
        .provision_user(identity("password-conflict-grantor"))
        .await
        .expect("测试授权人应当可以开通");

    let error = account
        .create_managed_user_with_roles(
            password_identity("conflict-password-user", "imes13800000002."),
            &[],
            grantor.id.as_str(),
        )
        .await
        .expect_err("目录冲突时应当直接返回冲突");

    assert!(matches!(
        error,
        AccountError::IdentityDirectory(IdentityDirectoryError::Conflict)
    ));
    let user_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM account.users WHERE identity_id = 'conflict-password-user')",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以核对目录冲突不会绑定本地用户");
    assert!(!user_exists);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn managed_user_with_initial_password_rejects_missing_or_invalid_password(pool: PgPool) {
    let directory = Arc::new(RecordingIdentityDirectory::default());
    let account = test_account_with_directory(pool, directory.clone()).await;

    let too_long_password = "x".repeat(201);
    for password in ["", "   ", too_long_password.as_str()] {
        let error = account
            .create_managed_user_with_roles(
                password_identity("invalid-password-user", password),
                &[],
                "grantor",
            )
            .await
            .expect_err("缺失或超长初始密码应当在调用目录前被拒绝");
        assert!(matches!(error, AccountError::InvalidInput(_)));
    }
    assert!(
        directory
            .created
            .lock()
            .expect("测试目录记录应可读取")
            .is_empty()
    );
}

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
            ("portal_admin".to_owned(), "门户管理员".to_owned()),
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
async fn host_state_extractors_share_account_authentication_and_authorization(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let user = account
        .provision_user(identity("host-user"))
        .await
        .expect("宿主测试用户应当可以开通");
    let app = host_router(&account, pool.clone());

    assert_eq!(
        get_with_token(app.clone(), "/host/profile", "host-user").await,
        StatusCode::OK
    );
    assert_eq!(
        get_with_token(app.clone(), "/me", "host-user").await,
        StatusCode::OK
    );
    assert_eq!(
        get_with_token(app.clone(), "/host/factories", "host-user").await,
        StatusCode::FORBIDDEN
    );

    let permissions = account
        .register_permissions(&[PermissionDefinition {
            key: "factories:read".to_owned(),
            name: "查看工厂".to_owned(),
            description: None,
        }])
        .await
        .expect("宿主应当可以注册业务权限");
    let role = account
        .create_role("factory-reader", "工厂查看者", None, &[permissions[0].id])
        .await
        .expect("宿主应当可以创建业务角色");
    account
        .replace_user_roles(user.id.as_str(), &[role.id], user.id.as_str())
        .await
        .expect("宿主应当可以授予业务角色");

    assert_eq!(
        get_with_token(app.clone(), "/host/factories", "host-user").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(get_without_token(app, "/host/health").await, StatusCode::OK);
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn subject_fallback_does_not_overwrite_existing_display_name(pool: PgPool) {
    insert_user("User0002", "identity-without-name", &pool).await;
    sqlx::query("UPDATE account.users SET display_name = '已有展示名' WHERE id = 'User0002'")
        .execute(&pool)
        .await
        .expect("应当可以准备已有展示名");

    let profile = current_profile(&test_account(pool).await, "identity-without-name").await;

    assert_eq!(profile.user.display_name, "已有展示名");
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
            owner: account::SYSTEM_ROLE_OWNER.to_owned(),
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
    let initial_role = account
        .create_role("production-planner", "生产计划员", None, &[])
        .await
        .expect("初始业务角色应当可以创建");

    let request = ProvisionUserRequest {
        username: "provisioned-user".to_owned(),
        given_name: "Provisioned".to_owned(),
        family_name: "User".to_owned(),
        email: "provisioned-user@example.com".to_owned(),
        display_name: Some("已开通用户".to_owned()),
        avatar_url: Some("https://cdn.example.com/provisioned-user.png".to_owned()),
        initial_password: "imes13800000003.".to_owned(),
        require_password_change: false,
        role_ids: vec![initial_role.id],
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
    assert_eq!(
        profile.user.avatar_url.as_deref(),
        Some("https://cdn.example.com/provisioned-user.png")
    );
    assert!(profile.roles.iter().any(|role| role.id == initial_role.id));
    let granted_by = sqlx::query_scalar::<_, Option<String>>(
        "SELECT granted_by FROM account.user_roles WHERE user_id = $1 AND role_id = $2",
    )
    .bind(profile.user.id.as_str())
    .bind(initial_role.id)
    .fetch_one(&pool)
    .await
    .expect("应当可以读取 HTTP 开通写入的角色授权人");
    assert_eq!(granted_by, Some(administrator.id.clone()));

    let invalid_request = ProvisionUserRequest {
        username: "rollback-http-user".to_owned(),
        given_name: "Rollback".to_owned(),
        family_name: "User".to_owned(),
        email: "rollback-http-user@example.com".to_owned(),
        display_name: Some("应回滚用户".to_owned()),
        avatar_url: None,
        initial_password: "imes13800000004.".to_owned(),
        require_password_change: false,
        role_ids: vec![i64::MAX],
    };
    let invalid = request_json(
        &account,
        Method::POST,
        "/users".to_owned(),
        "administrator",
        &invalid_request,
    )
    .await;
    assert_eq!(invalid, StatusCode::NOT_FOUND);
    let rollback_user_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM account.users WHERE identity_id = 'rollback-http-user'",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以核对 HTTP 开通事务回滚结果");
    assert_eq!(rollback_user_count, 0);

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

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn provisioning_initial_roles_requires_role_management_permission(pool: PgPool) {
    let account = test_account(pool.clone()).await;
    let provision_permission_id = permission_id("users:provision", &pool).await;
    let roles_write_permission_id = permission_id("users:roles.write", &pool).await;
    let provision_only_role = account
        .create_role(
            "user-provisioner",
            "用户开通员",
            None,
            &[provision_permission_id],
        )
        .await
        .expect("应当可以创建仅开通用户的测试角色");
    let user_manager_role = account
        .create_role(
            "user-manager",
            "用户管理员",
            None,
            &[provision_permission_id, roles_write_permission_id],
        )
        .await
        .expect("应当可以创建同时管理用户角色的测试角色");
    let initial_role = account
        .create_role("factory-reader", "工厂查看者", None, &[])
        .await
        .expect("应当可以创建待授予的初始角色");
    let provisioner = account
        .provision_user(identity("provision-only"))
        .await
        .expect("应当可以开通仅开通用户的操作者");
    account
        .replace_user_roles(
            provisioner.id.as_str(),
            &[provision_only_role.id],
            provisioner.id.as_str(),
        )
        .await
        .expect("应当可以授予用户开通权限");
    let user_manager = account
        .provision_user(identity("user-manager"))
        .await
        .expect("应当可以开通用户管理员");
    account
        .replace_user_roles(
            user_manager.id.as_str(),
            &[user_manager_role.id],
            user_manager.id.as_str(),
        )
        .await
        .expect("应当可以授予用户与角色管理权限");

    let empty_roles = ProvisionUserRequest {
        username: "empty-role-user".to_owned(),
        given_name: "Empty".to_owned(),
        family_name: "Role".to_owned(),
        email: "empty-role-user@example.com".to_owned(),
        display_name: Some("默认成员用户".to_owned()),
        avatar_url: None,
        initial_password: "imes13800000005.".to_owned(),
        require_password_change: false,
        role_ids: Vec::new(),
    };
    assert_eq!(
        request_json(
            &account,
            Method::POST,
            "/users".to_owned(),
            "provision-only",
            &empty_roles,
        )
        .await,
        StatusCode::CREATED
    );

    let denied_roles = ProvisionUserRequest {
        username: "denied-role-user".to_owned(),
        given_name: "Denied".to_owned(),
        family_name: "Role".to_owned(),
        email: "denied-role-user@example.com".to_owned(),
        display_name: Some("越权角色用户".to_owned()),
        avatar_url: None,
        initial_password: "imes13800000006.".to_owned(),
        require_password_change: false,
        role_ids: vec![initial_role.id],
    };
    let denied = request_json_response(
        &account,
        Method::POST,
        "/users".to_owned(),
        "provision-only",
        &denied_roles,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let denied_body = to_bytes(denied.into_body(), 16 * 1024)
        .await
        .expect("权限拒绝响应应当可以读取");
    let denied_error: ErrorEnvelope =
        serde_json::from_slice(&denied_body).expect("权限拒绝应当符合公共错误契约");
    assert_eq!(denied_error.error.code, "permission_denied");

    let allowed_roles = ProvisionUserRequest {
        username: "allowed-role-user".to_owned(),
        given_name: "Allowed".to_owned(),
        family_name: "Role".to_owned(),
        email: "allowed-role-user@example.com".to_owned(),
        display_name: Some("已授权角色用户".to_owned()),
        avatar_url: None,
        initial_password: "imes13800000007.".to_owned(),
        require_password_change: false,
        role_ids: vec![initial_role.id],
    };
    assert_eq!(
        request_json(
            &account,
            Method::POST,
            "/users".to_owned(),
            "user-manager",
            &allowed_roles,
        )
        .await,
        StatusCode::CREATED
    );
}

async fn test_account(pool: PgPool) -> Account {
    test_account_with_directory(pool, Arc::new(TestIdentityDirectory)).await
}

async fn test_account_with_directory(
    pool: PgPool,
    identity_directory: Arc<dyn IdentityDirectory>,
) -> Account {
    Account::bind_identity_issuer(&pool, TEST_IDENTITY_ISSUER)
        .await
        .expect("测试部署 issuer 应当可以绑定或核对");
    Account::new(AccountDependencies {
        pool,
        token_verifier: Arc::new(TokenIdentityVerifier),
        identity_directory: Some(identity_directory),
        avatar_storage: None,
    })
}

fn router(account: &Account) -> Router {
    with_http_layers(account.routers::<()>())
}

fn host_router(account: &Account, pool: PgPool) -> Router {
    with_http_layers(
        Router::new()
            .merge(account.routers::<HostState>())
            .route("/host/health", get(host_health))
            .route("/host/profile", get(host_profile))
            .route("/host/factories", get(host_factories)),
    )
    .with_state(HostState {
        account: account.clone(),
        pool,
    })
}

async fn host_profile(authenticated: AuthenticatedUser) -> StatusCode {
    if authenticated.profile().user.identity_id == "host-user" {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

async fn host_factories(_authorization: Authorized<ReadFactories>) -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn host_health(State(state): State<HostState>) -> StatusCode {
    match state.pool.acquire().await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn get_with_token(router: Router, uri: &str, token: &str) -> StatusCode {
    router
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("宿主路由应当返回响应")
        .status()
}

async fn get_without_token(router: Router, uri: &str) -> StatusCode {
    router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("测试请求应当有效"),
        )
        .await
        .expect("宿主路由应当返回响应")
        .status()
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

async fn permission_id(key: &str, pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT id FROM account.permissions WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("测试权限应当存在")
}

fn permission_keys(permissions: &[account::Permission]) -> Vec<&str> {
    permissions
        .iter()
        .map(|permission| permission.key.as_str())
        .collect()
}

async fn stored_role_permission_keys(role_id: i64, pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT permissions.key
        FROM account.role_permissions
        JOIN account.permissions ON permissions.id = role_permissions.permission_id
        WHERE role_permissions.role_id = $1
        ORDER BY permissions.key
        "#,
    )
    .bind(role_id)
    .fetch_all(pool)
    .await
    .expect("应当可以读取角色最终权限")
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
        username: Some(identity_id.to_owned()),
        email: Some(format!("{identity_id}@example.com")),
        display_name: identity_id.to_owned(),
        avatar_url: None,
    }
}

fn password_identity(username: &str, password: &str) -> CreateHumanIdentity {
    CreateHumanIdentity {
        username: username.to_owned(),
        given_name: "Test".to_owned(),
        family_name: "User".to_owned(),
        email: format!("{username}@example.com"),
        display_name: Some(username.to_owned()),
        initial_password: password.to_owned(),
        require_password_change: false,
        avatar_url: None,
    }
}

struct TokenIdentityVerifier;

struct TestIdentityDirectory;

#[async_trait]
impl IdentityDirectory for TestIdentityDirectory {
    async fn identity(
        &self,
        identity_id: &str,
    ) -> Result<Option<ExternalIdentity>, IdentityDirectoryError> {
        Ok(Some(identity(identity_id)))
    }

    async fn create_human_identity(
        &self,
        request: &CreateHumanIdentity,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        Ok(ExternalIdentity {
            identity_id: request.username.clone(),
            username: Some(request.username.clone()),
            email: Some(request.email.clone()),
            display_name: request
                .display_name
                .clone()
                .unwrap_or_else(|| format!("{} {}", request.given_name, request.family_name)),
            avatar_url: request.avatar_url.clone(),
        })
    }

    async fn delete_identity(&self, _identity_id: &str) -> Result<(), IdentityDirectoryError> {
        Ok(())
    }

    async fn update_identity_avatar(
        &self,
        _identity_id: &str,
        _avatar_url: Option<&str>,
    ) -> Result<(), IdentityDirectoryError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingIdentityDirectory {
    created: Mutex<Vec<RecordedDirectoryCreate>>,
    deleted: Mutex<Vec<String>>,
}

struct RecordedDirectoryCreate {
    username: String,
    initial_password: String,
    require_password_change: bool,
    contact_phone: Option<String>,
}

impl RecordedDirectoryCreate {
    fn initial_password_matches(&self, expected: &str) -> bool {
        self.initial_password == expected
    }
}

#[async_trait]
impl IdentityDirectory for RecordingIdentityDirectory {
    async fn identity(
        &self,
        identity_id: &str,
    ) -> Result<Option<ExternalIdentity>, IdentityDirectoryError> {
        Ok(Some(identity(identity_id)))
    }

    async fn create_human_identity(
        &self,
        request: &CreateHumanIdentity,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        self.record_create(request, None);
        Ok(directory_identity(request))
    }

    async fn create_human_identity_with_contact(
        &self,
        request: &CreateHumanIdentity,
        contact_phone: Option<&str>,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        self.record_create(request, contact_phone);
        Ok(directory_identity(request))
    }

    async fn delete_identity(&self, identity_id: &str) -> Result<(), IdentityDirectoryError> {
        self.deleted
            .lock()
            .expect("测试目录删除记录应可写入")
            .push(identity_id.to_owned());
        Ok(())
    }

    async fn update_identity_avatar(
        &self,
        _identity_id: &str,
        _avatar_url: Option<&str>,
    ) -> Result<(), IdentityDirectoryError> {
        Ok(())
    }
}

impl RecordingIdentityDirectory {
    fn record_create(&self, request: &CreateHumanIdentity, contact_phone: Option<&str>) {
        self.created
            .lock()
            .expect("测试目录创建记录应可写入")
            .push(RecordedDirectoryCreate {
                username: request.username.clone(),
                initial_password: request.initial_password.clone(),
                require_password_change: request.require_password_change,
                contact_phone: contact_phone.map(str::to_owned),
            });
    }
}

fn directory_identity(request: &CreateHumanIdentity) -> ExternalIdentity {
    ExternalIdentity {
        identity_id: request.username.clone(),
        username: Some(request.username.clone()),
        email: Some(request.email.clone()),
        display_name: request
            .display_name
            .clone()
            .unwrap_or_else(|| format!("{} {}", request.given_name, request.family_name)),
        avatar_url: request.avatar_url.clone(),
    }
}

struct ConflictingIdentityDirectory;

#[async_trait]
impl IdentityDirectory for ConflictingIdentityDirectory {
    async fn identity(
        &self,
        identity_id: &str,
    ) -> Result<Option<ExternalIdentity>, IdentityDirectoryError> {
        Ok(Some(identity(identity_id)))
    }

    async fn create_human_identity(
        &self,
        _request: &CreateHumanIdentity,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        Err(IdentityDirectoryError::Conflict)
    }

    async fn delete_identity(&self, _identity_id: &str) -> Result<(), IdentityDirectoryError> {
        Ok(())
    }

    async fn update_identity_avatar(
        &self,
        _identity_id: &str,
        _avatar_url: Option<&str>,
    ) -> Result<(), IdentityDirectoryError> {
        Ok(())
    }
}

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
            username: Some(token.to_owned()),
            email: Some(format!("{token}@example.com")),
            display_name: token.to_owned(),
            avatar_url: None,
            organization: None,
        })
    }
}
