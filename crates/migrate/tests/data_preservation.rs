#![cfg(feature = "database-tests")]

use sqlx::PgPool;

const MIGRATION_1: &str = include_str!("../migrations/0001_account_create_rbac.up.sql");
const MIGRATION_2: &str = include_str!("../migrations/0002_account_add_super_admin.up.sql");
const MIGRATION_3: &str =
    include_str!("../migrations/0003_account_rework_system_initialization.up.sql");
const MIGRATION_4: &str = include_str!("../migrations/0004_account_change_identifier_types.up.sql");

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
