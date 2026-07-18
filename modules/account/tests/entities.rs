use std::collections::BTreeSet;

use account::{AccessProfile, PermissionKey, User, UserStatus};
use chrono::Utc;
use sqlx::{Postgres, Type, TypeInfo};

#[test]
fn permission_keys_support_built_in_and_application_defined_values() {
    let cases = [
        (PermissionKey::UsersRead, "users:read"),
        (PermissionKey::UsersRolesWrite, "users:roles.write"),
        (PermissionKey::UsersStatusWrite, "users:status.write"),
        (PermissionKey::UsersProvision, "users:provision"),
        (PermissionKey::RolesRead, "roles:read"),
        (PermissionKey::RolesWrite, "roles:write"),
        (PermissionKey::PermissionsRead, "permissions:read"),
    ];

    for (permission, key) in cases {
        assert_eq!(permission.as_str(), key);
        assert_eq!(PermissionKey::try_from(key), Ok(permission));
    }
    let custom = PermissionKey::try_from("projects:archive").expect("应用权限键应受支持");
    assert_eq!(custom.as_str(), "projects:archive");
    assert!(PermissionKey::try_from("Invalid Permission").is_err());
}

#[test]
fn user_status_uses_schema_qualified_postgres_type() {
    let type_info = <UserStatus as Type<Postgres>>::type_info();

    assert_eq!(type_info.name(), "account.user_status");
}

#[test]
fn access_profile_distinguishes_rbac_and_super_administrator() {
    let mut profile = profile([PermissionKey::RolesRead]);
    assert!(profile.allows(PermissionKey::RolesRead));
    assert!(!profile.allows(PermissionKey::RolesWrite));

    profile.user.is_super_admin = true;
    profile.permissions.clear();
    assert!(profile.roles.is_empty());
    assert!(profile.allows(PermissionKey::RolesWrite));
}

fn profile(permissions: impl IntoIterator<Item = PermissionKey>) -> AccessProfile {
    let now = Utc::now();
    AccessProfile {
        user: User {
            id: "Ab3xY9qP".to_owned(),
            identity_id: "test-user".to_owned(),
            username: Some("test-user".to_owned()),
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
        permissions: permissions.into_iter().collect::<BTreeSet<_>>(),
    }
}
