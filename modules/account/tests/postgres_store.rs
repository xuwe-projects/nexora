#![cfg(feature = "database-tests")]

use account::{
    AccountApplication, AccountsStore, CreateRole, ExternalIdentity, PostgresAccountsStore,
    StoreError, UserStatus,
};
use sqlx::PgPool;
use std::{collections::BTreeSet, sync::Arc};

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn identity_sync_assigns_only_the_default_member_role(pool: PgPool) {
    let store = Arc::new(PostgresAccountsStore::new(pool));
    let application = AccountApplication::new(store);

    let profile = application
        .authenticate(&identity("ordinary-user"))
        .await
        .expect("普通身份应当同步成功");

    assert!(profile.roles.iter().any(|role| role.key == "member"));
    assert!(!profile.roles.iter().any(|role| role.key == "administrator"));
    assert!(!profile.has_permission("roles:write"));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn last_active_administrator_cannot_be_suspended_or_demoted(pool: PgPool) {
    let store = Arc::new(PostgresAccountsStore::new(pool));
    let application = AccountApplication::new(store.clone());
    let member_profile = application
        .authenticate(&identity("administrator"))
        .await
        .expect("管理员的本地身份应当同步成功");
    let administrator_role_id = store
        .list_roles()
        .await
        .expect("应当可以读取角色目录")
        .into_iter()
        .find(|role| role.key == "administrator")
        .expect("系统管理员角色应当存在")
        .id;
    let profile = store
        .replace_user_roles(
            member_profile.user.id,
            &[administrator_role_id],
            member_profile.user.id,
        )
        .await
        .expect("应当可以显式授予首个管理员角色");

    let suspend_error = store
        .set_user_status(profile.user.id, UserStatus::Suspended)
        .await
        .expect_err("最后一个管理员不应被停用");
    assert!(matches!(suspend_error, StoreError::LastAdministrator));

    let member_role_id = profile
        .roles
        .iter()
        .find(|role| role.key == "member")
        .expect("默认成员角色应当存在")
        .id;
    let demote_error = store
        .replace_user_roles(profile.user.id, &[member_role_id], profile.user.id)
        .await
        .expect_err("最后一个管理员不应被降级");
    assert!(matches!(demote_error, StoreError::LastAdministrator));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn binding_existing_user_replaces_previous_roles_with_reserved_roles(pool: PgPool) {
    let store = Arc::new(PostgresAccountsStore::new(pool));
    let selected_identity = identity("existing-super-admin");
    let existing_user = store
        .sync_identity(&selected_identity)
        .await
        .expect("已有管理员身份应当同步成功");
    let administrator_role_id = store
        .list_roles()
        .await
        .expect("应当可以读取角色目录")
        .into_iter()
        .find(|role| role.key == "administrator")
        .expect("系统管理员角色应当存在")
        .id;
    let custom_role = store
        .create_role(&CreateRole {
            key: "project-manager".to_owned(),
            name: "项目管理员".to_owned(),
            description: Some("绑定超级管理员前已有的自定义角色".to_owned()),
            permission_ids: Vec::new(),
        })
        .await
        .expect("应当可以创建自定义角色");
    let previous_profile = store
        .replace_user_roles(
            existing_user.id,
            &[administrator_role_id, custom_role.id],
            existing_user.id,
        )
        .await
        .expect("应当可以为已有用户授予系统管理员和自定义角色");
    assert_eq!(
        previous_profile
            .roles
            .iter()
            .map(|role| role.key.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["administrator", "member", "project-manager"]),
    );

    let super_admin = store
        .bind_super_admin(&selected_identity)
        .await
        .expect("已有用户应当可以绑定为超级管理员");
    let repeated = store
        .bind_super_admin(&selected_identity)
        .await
        .expect("同一身份重复绑定应当幂等");
    assert_eq!(repeated.id, super_admin.id);

    let profile = store
        .access_profile(super_admin.id)
        .await
        .expect("应当可以读取超级管理员授权快照");
    assert_eq!(
        profile
            .roles
            .iter()
            .map(|role| role.key.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["member", "super-administrator"]),
    );

    let replace_roles_error = store
        .replace_user_roles(super_admin.id, &[custom_role.id], super_admin.id)
        .await
        .expect_err("超级管理员角色不应允许再次变更");
    assert!(matches!(
        replace_roles_error,
        StoreError::SuperAdministratorImmutable
    ));
}

#[sqlx::test(migrations = "../../crates/migrate/migrations")]
async fn super_administrator_is_unique_immutable_and_receives_future_permissions(pool: PgPool) {
    let store = Arc::new(PostgresAccountsStore::new(pool.clone()));
    let selected_identity = identity("super-admin");
    let super_admin = store
        .bind_super_admin(&selected_identity)
        .await
        .expect("首次绑定超级管理员应当成功");
    let repeated = store
        .bind_super_admin(&selected_identity)
        .await
        .expect("同一身份重复绑定应当幂等");
    assert_eq!(repeated.id, super_admin.id);
    assert!(repeated.is_super_admin);
    store
        .sync_identity(&selected_identity)
        .await
        .expect("超级管理员登录同步不应改变固定角色");

    let different_identity = store
        .bind_super_admin(&identity("another-super-admin"))
        .await
        .expect_err("超级管理员身份不应允许替换");
    assert!(matches!(
        different_identity,
        StoreError::SuperAdministratorAlreadyBound
    ));

    sqlx::query(
        r#"
        INSERT INTO account.permissions (key, name, description)
        VALUES ('reports:export', '导出报表', '超级管理员绑定后新增的权限')
        "#,
    )
    .execute(&pool)
    .await
    .expect("应当可以新增未来权限");
    let profile = store
        .access_profile(super_admin.id)
        .await
        .expect("应当可以读取超级管理员授权快照");
    assert!(
        profile
            .roles
            .iter()
            .any(|role| role.key == "super-administrator")
    );
    assert!(!profile.roles.iter().any(|role| role.key == "administrator"));
    let permission_keys = store
        .list_permissions()
        .await
        .expect("应当可以读取完整权限目录")
        .into_iter()
        .map(|permission| permission.key)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(profile.permissions, permission_keys);
    assert!(profile.has_permission("reports:export"));

    let suspend_error = store
        .set_user_status(super_admin.id, UserStatus::Suspended)
        .await
        .expect_err("超级管理员不应允许停用");
    assert!(matches!(
        suspend_error,
        StoreError::SuperAdministratorImmutable
    ));
    let member_role_id = profile
        .roles
        .iter()
        .find(|role| role.key == "member")
        .expect("超级管理员应当保留成员角色")
        .id;
    let replace_roles_error = store
        .replace_user_roles(super_admin.id, &[member_role_id], super_admin.id)
        .await
        .expect_err("超级管理员角色不应允许替换");
    assert!(matches!(
        replace_roles_error,
        StoreError::SuperAdministratorImmutable
    ));

    let ordinary_user = store
        .sync_identity(&identity("ordinary-user"))
        .await
        .expect("普通用户身份应当同步成功");
    let super_role_id = profile
        .roles
        .iter()
        .find(|role| role.key == "super-administrator")
        .expect("超级管理员保留角色应当存在")
        .id;
    let reserved_role_error = store
        .replace_user_roles(ordinary_user.id, &[super_role_id], super_admin.id)
        .await
        .expect_err("保留角色不应允许授予普通用户");
    assert!(matches!(
        reserved_role_error,
        StoreError::SuperAdministratorRoleReserved
    ));

    assert!(
        sqlx::query("INSERT INTO account.user_roles (user_id, role_id) VALUES ($1, $2)")
            .bind(ordinary_user.id)
            .bind(super_role_id)
            .execute(&pool)
            .await
            .is_err(),
        "直接 SQL 也不应能把保留角色授予普通用户"
    );
    let administrator_role_id = store
        .list_roles()
        .await
        .expect("应当可以读取角色目录")
        .into_iter()
        .find(|role| role.key == "administrator")
        .expect("系统管理员角色应当存在")
        .id;
    assert!(
        sqlx::query("INSERT INTO account.user_roles (user_id, role_id) VALUES ($1, $2)")
            .bind(super_admin.id)
            .bind(administrator_role_id)
            .execute(&pool)
            .await
            .is_err(),
        "直接 SQL 也不应能给超级管理员追加角色"
    );

    assert!(
        sqlx::query("UPDATE account.users SET subject = 'replaced' WHERE id = $1")
            .bind(super_admin.id)
            .execute(&pool)
            .await
            .is_err(),
        "直接 SQL 也不应能替换超级管理员身份"
    );
    assert!(
        sqlx::query("DELETE FROM account.user_roles WHERE user_id = $1")
            .bind(super_admin.id)
            .execute(&pool)
            .await
            .is_err(),
        "直接 SQL 也不应能删除超级管理员角色"
    );
    assert!(
        sqlx::query("DELETE FROM account.users WHERE id = $1")
            .bind(super_admin.id)
            .execute(&pool)
            .await
            .is_err(),
        "直接 SQL 也不应能删除超级管理员"
    );
}

fn identity(subject: &str) -> ExternalIdentity {
    ExternalIdentity {
        issuer: "https://id.example.com/".to_owned(),
        subject: subject.to_owned(),
        email: Some(format!("{subject}@example.com")),
        display_name: subject.to_owned(),
        avatar_url: None,
    }
}
