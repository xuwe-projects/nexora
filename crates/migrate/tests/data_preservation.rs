#![cfg(feature = "database-tests")]

use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn internal_service_principal_can_omit_provider_identity(pool: PgPool) {
    migrate::migrate(&pool).await.expect("Account 迁移应当成功");

    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name, user_type)
        VALUES ('SysAudit', NULL, '内部审计主体', 'service_account')
        "#,
    )
    .execute(&pool)
    .await
    .expect("内部服务主体应当允许不绑定 Provider identity");

    let invalid_human = sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name, user_type)
        VALUES ('HumanNil', NULL, '缺少身份的人员', 'human')
        "#,
    )
    .execute(&pool)
    .await;
    assert!(
        invalid_human.is_err(),
        "人员账号仍必须绑定 Provider identity"
    );
}

#[sqlx::test(migrations = false)]
async fn account_baseline_allows_optional_username_binding(pool: PgPool) {
    migrate::migrate(&pool)
        .await
        .expect("Account 基线迁移应当成功");
    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name)
        VALUES ('Member01', 'member-subject', '测试用户')
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以创建没有用户名的用户");

    let username = sqlx::query_scalar::<_, Option<String>>(
        "SELECT username FROM account.users WHERE id = 'Member01'",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以读取可选用户名");
    assert_eq!(username, None);

    sqlx::query("UPDATE account.users SET username = 'member-user' WHERE id = 'Member01'")
        .execute(&pool)
        .await
        .expect("应当可以绑定登录用户名");
    let username = sqlx::query_scalar::<_, Option<String>>(
        "SELECT username FROM account.users WHERE id = 'Member01'",
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以读取已绑定用户名");
    assert_eq!(username.as_deref(), Some("member-user"));
}

#[sqlx::test(migrations = false)]
async fn account_baseline_supports_final_identifiers_and_relations(pool: PgPool) {
    migrate::migrate(&pool)
        .await
        .expect("Account 基线迁移应当成功");
    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, email, display_name, is_super_admin)
        VALUES
            ('Admin001', 'identity-super-admin', 'owner@example.com', '超级管理员', TRUE),
            ('Member01', 'identity-member', 'member@example.com', '普通成员', FALSE)
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以创建最终标识符类型的用户");
    sqlx::query(
        r#"
        INSERT INTO account.user_roles (user_id, role_id, granted_by)
        SELECT 'Member01', roles.id, 'Admin001'
        FROM account.roles AS roles
        WHERE roles.key = 'member'
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以创建用户角色关系");
    sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET identity_issuer = 'https://id.example.com/',
            is_initialized = TRUE,
            super_admin_user_id = 'Admin001',
            initialized_at = NOW()
        WHERE id = 1
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以完成系统初始化");

    let assignment = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT roles.key, user_roles.granted_by
        FROM account.user_roles
        JOIN account.roles ON account.roles.id = account.user_roles.role_id
        WHERE account.user_roles.user_id = 'Member01'
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以读取用户角色关系");
    assert_eq!(
        assignment,
        ("member".to_owned(), Some("Admin001".to_owned()))
    );

    let id_types = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT
            pg_catalog.format_type(users.atttypid, users.atttypmod),
            pg_catalog.format_type(roles.atttypid, roles.atttypmod),
            pg_catalog.format_type(permissions.atttypid, permissions.atttypmod)
        FROM pg_attribute AS users
        CROSS JOIN pg_attribute AS roles
        CROSS JOIN pg_attribute AS permissions
        WHERE users.attrelid = 'account.users'::REGCLASS
          AND users.attname = 'id'
          AND roles.attrelid = 'account.roles'::REGCLASS
          AND roles.attname = 'id'
          AND permissions.attrelid = 'account.permissions'::REGCLASS
          AND permissions.attname = 'id'
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以读取最终主键类型");
    assert_eq!(
        id_types,
        (
            "character varying(8)".to_owned(),
            "bigint".to_owned(),
            "bigint".to_owned(),
        )
    );
}

#[sqlx::test(migrations = false)]
async fn deployment_issuer_can_only_be_bound_once(pool: PgPool) {
    migrate::migrate(&pool)
        .await
        .expect("Account 基线迁移应当成功");

    sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET identity_issuer = 'https://id.example.com/'
        WHERE id = 1 AND identity_issuer IS NULL
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以首次绑定部署 issuer");
    let second_rebind = sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET identity_issuer = 'https://another.example.com/'
        WHERE id = 1
        "#,
    )
    .execute(&pool)
    .await;

    assert!(second_rebind.is_err(), "部署 issuer 首次绑定后必须冻结");
}

#[sqlx::test(migrations = false)]
async fn deployment_issuer_is_required_and_identity_id_stays_globally_unique(pool: PgPool) {
    migrate::migrate(&pool)
        .await
        .expect("Account 基线迁移应当成功");
    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name, is_super_admin)
        VALUES ('Admin001', 'admin-subject', '超级管理员', TRUE)
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备超级管理员");
    let initialize_without_issuer = sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET is_initialized = TRUE,
            super_admin_user_id = 'Admin001',
            initialized_at = NOW()
        WHERE id = 1
        "#,
    )
    .execute(&pool)
    .await;
    assert!(
        initialize_without_issuer.is_err(),
        "绑定部署 issuer 前不能完成初始化"
    );

    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name)
        VALUES ('Member01', 'shared-subject', '第一个用户')
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以创建第一个 identity ID");
    let duplicate = sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name)
        VALUES ('Member02', 'shared-subject', '重复用户')
        "#,
    )
    .execute(&pool)
    .await;
    assert!(duplicate.is_err(), "identity ID 必须在部署内保持唯一");

    let administrator_can_provision = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM account.roles
            JOIN account.role_permissions ON role_permissions.role_id = roles.id
            JOIN account.permissions ON permissions.id = role_permissions.permission_id
            WHERE roles.key = 'admin' AND permissions.key = 'users:provision'
        )
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以检查新开通权限");
    assert!(administrator_can_provision);
}
