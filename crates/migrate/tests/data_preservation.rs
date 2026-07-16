#![cfg(feature = "database-tests")]

use sqlx::PgPool;

const MIGRATION_1: &str = include_str!("../migrations/0001_account_create_rbac.up.sql");
const MIGRATION_2: &str = include_str!("../migrations/0002_account_add_super_admin.up.sql");
const MIGRATION_3: &str =
    include_str!("../migrations/0003_account_rework_system_initialization.up.sql");
const MIGRATION_4: &str = include_str!("../migrations/0004_account_change_identifier_types.up.sql");
const MIGRATION_5: &str = include_str!("../migrations/0005_account_bind_identity_issuer.up.sql");

#[sqlx::test(migrations = false)]
async fn identifier_type_upgrade_preserves_existing_business_data(pool: PgPool) {
    apply(&pool, MIGRATION_1).await;
    apply(&pool, MIGRATION_2).await;
    apply(&pool, MIGRATION_3).await;

    sqlx::query(
        r#"
        INSERT INTO account.users (
            identity_id,
            email,
            display_name,
            is_super_admin
        )
        VALUES
            ('identity-super-admin', 'owner@example.com', '超级管理员', TRUE),
            ('identity-member', 'member@example.com', '普通成员', FALSE)
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备迁移前用户");
    sqlx::query(
        r#"
        INSERT INTO account.user_roles (user_id, role_id, granted_by)
        SELECT member.id, roles.id, super_admin.id
        FROM account.users AS member
        CROSS JOIN account.roles AS roles
        CROSS JOIN account.users AS super_admin
        WHERE member.identity_id = 'identity-member'
          AND roles.key = 'member'
          AND super_admin.identity_id = 'identity-super-admin'
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备迁移前角色关系");
    sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET is_initialized = TRUE,
            super_admin_user_id = users.id,
            initialized_at = NOW()
        FROM account.users AS users
        WHERE account.system_initialization.id = 1
          AND users.identity_id = 'identity-super-admin'
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备迁移前初始化状态");

    let before_counts = business_counts(&pool).await;
    apply(&pool, MIGRATION_4).await;
    let after_counts = business_counts(&pool).await;

    assert_eq!(after_counts, before_counts);

    let users = sqlx::query_as::<_, (String, String, bool, i32, bool)>(
        r#"
        SELECT
            identity_id,
            display_name,
            is_super_admin,
            LENGTH(id)::INTEGER,
            id ~ '^[A-Za-z0-9]{8}$'
        FROM account.users
        ORDER BY identity_id
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("迁移后应当可以读取用户");
    assert_eq!(
        users,
        vec![
            (
                "identity-member".to_owned(),
                "普通成员".to_owned(),
                false,
                8,
                true,
            ),
            (
                "identity-super-admin".to_owned(),
                "超级管理员".to_owned(),
                true,
                8,
                true,
            ),
        ]
    );

    let assignment = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT roles.key, grantor.identity_id
        FROM account.user_roles AS user_roles
        JOIN account.users AS users ON users.id = user_roles.user_id
        JOIN account.roles AS roles ON roles.id = user_roles.role_id
        LEFT JOIN account.users AS grantor ON grantor.id = user_roles.granted_by
        WHERE users.identity_id = 'identity-member'
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("迁移后用户角色关系应当保留");
    assert_eq!(
        assignment,
        ("member".to_owned(), Some("identity-super-admin".to_owned()))
    );

    let initialization = sqlx::query_as::<_, (bool, Option<String>)>(
        r#"
        SELECT initialization.is_initialized, users.identity_id
        FROM account.system_initialization AS initialization
        LEFT JOIN account.users AS users ON users.id = initialization.super_admin_user_id
        WHERE initialization.id = 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("迁移后初始化引用应当保留");
    assert_eq!(
        initialization,
        (true, Some("identity-super-admin".to_owned()))
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
    .expect("迁移后应当可以读取主键类型");
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
async fn initialized_deployment_can_bind_issuer_once_after_upgrade(pool: PgPool) {
    apply(&pool, MIGRATION_1).await;
    apply(&pool, MIGRATION_2).await;
    apply(&pool, MIGRATION_3).await;
    apply(&pool, MIGRATION_4).await;
    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name, is_super_admin)
        VALUES ('Legacy01', 'legacy-super-admin', '遗留超级管理员', TRUE)
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备遗留超级管理员");
    sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET is_initialized = TRUE,
            super_admin_user_id = 'Legacy01',
            initialized_at = NOW()
        WHERE id = 1
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以准备已完成初始化的旧部署");

    apply(&pool, MIGRATION_5).await;

    let unbound = sqlx::query_scalar::<_, Option<String>>(
        "SELECT identity_issuer FROM account.system_initialization WHERE id = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("迁移后应当可以读取部署 issuer");
    assert_eq!(unbound, None);

    sqlx::query(
        r#"
        UPDATE account.system_initialization
        SET identity_issuer = 'https://recovered.example.com/'
        WHERE id = 1 AND identity_issuer IS NULL
        "#,
    )
    .execute(&pool)
    .await
    .expect("已初始化旧部署也应当可以首次绑定 issuer");
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
async fn deployment_issuer_is_required_and_user_subject_stays_globally_unique(pool: PgPool) {
    apply(&pool, MIGRATION_1).await;
    apply(&pool, MIGRATION_2).await;
    apply(&pool, MIGRATION_3).await;
    apply(&pool, MIGRATION_4).await;
    apply(&pool, MIGRATION_5).await;

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
        UPDATE account.system_initialization
        SET identity_issuer = 'https://id.example.com/'
        WHERE id = 1
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以首次绑定部署 issuer");

    sqlx::query(
        r#"
        INSERT INTO account.users (id, identity_id, display_name)
        VALUES ('Member01', 'shared-subject', '第一个用户')
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以创建第一个 subject");
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

async fn apply(pool: &PgPool, sql: &'static str) {
    sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .expect("迁移脚本应当执行成功");
}

async fn business_counts(pool: &PgPool) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM account.users),
            (SELECT COUNT(*) FROM account.roles),
            (SELECT COUNT(*) FROM account.permissions),
            (SELECT COUNT(*) FROM account.user_roles),
            (SELECT COUNT(*) FROM account.role_permissions)
        "#,
    )
    .fetch_one(pool)
    .await
    .expect("应当可以读取业务数据计数")
}
