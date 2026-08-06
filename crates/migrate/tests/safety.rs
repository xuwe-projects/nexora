const IDENTITY_UP: &str =
    include_str!("../migrations/20260806042552_account_identity_baseline.up.sql");
const IDENTITY_DOWN: &str =
    include_str!("../migrations/20260806042552_account_identity_baseline.down.sql");
const AUTHORIZATION_UP: &str =
    include_str!("../migrations/20260806042557_account_authorization_baseline.up.sql");
const AUTHORIZATION_DOWN: &str =
    include_str!("../migrations/20260806042557_account_authorization_baseline.down.sql");
const PROTECTION_UP: &str =
    include_str!("../migrations/20260806042602_account_protection_baseline.up.sql");
const PROTECTION_DOWN: &str =
    include_str!("../migrations/20260806042602_account_protection_baseline.down.sql");
const CATALOG_UP: &str =
    include_str!("../migrations/20260806042607_account_catalog_baseline.up.sql");
const CATALOG_DOWN: &str =
    include_str!("../migrations/20260806042607_account_catalog_baseline.down.sql");

#[test]
fn identity_baseline_owns_schema_users_and_initialization() {
    assert!(IDENTITY_UP.contains("CREATE SCHEMA account"));
    assert!(IDENTITY_UP.contains("CREATE TYPE account.user_status"));
    assert!(IDENTITY_UP.contains("CREATE TABLE account.users"));
    assert!(IDENTITY_UP.contains("CREATE TABLE account.system_initialization"));
}

#[test]
fn authorization_baseline_owns_rbac_objects() {
    for object in [
        "account.roles",
        "account.permissions",
        "account.user_roles",
        "account.role_permissions",
        "account.permission_implications",
    ] {
        assert!(AUTHORIZATION_UP.contains(&format!("CREATE TABLE {object}")));
    }
}

#[test]
fn protection_baseline_owns_all_functions_and_triggers() {
    assert_eq!(PROTECTION_UP.matches("CREATE FUNCTION").count(), 3);
    assert_eq!(PROTECTION_UP.matches("CREATE TRIGGER").count(), 3);
    assert_eq!(PROTECTION_UP.matches("COMMENT ON FUNCTION").count(), 3);
    assert_eq!(PROTECTION_UP.matches("COMMENT ON TRIGGER").count(), 3);
}

#[test]
fn catalog_baseline_contains_only_current_framework_data() {
    for key in [
        "users:read",
        "users:roles.write",
        "users:status.write",
        "users:provision",
        "roles:read",
        "roles:write",
        "permissions:read",
    ] {
        assert!(CATALOG_UP.contains(key));
    }
    assert!(!CATALOG_UP.contains("users:avatar.write"));
}

#[test]
fn reversible_baselines_drop_objects_in_dependency_order() {
    assert!(CATALOG_DOWN.starts_with("DELETE FROM account.role_permissions"));
    assert!(PROTECTION_DOWN.starts_with("DROP TRIGGER system_initialization_protect"));
    assert!(AUTHORIZATION_DOWN.starts_with("DROP TABLE account.permission_implications"));
    assert!(IDENTITY_DOWN.ends_with("DROP SCHEMA account;\n"));
}

#[test]
fn baseline_ddl_documents_every_object_category() {
    for category in [
        "COMMENT ON SCHEMA",
        "COMMENT ON TYPE",
        "COMMENT ON TABLE",
        "COMMENT ON COLUMN",
        "COMMENT ON CONSTRAINT",
        "COMMENT ON INDEX",
    ] {
        assert!(
            IDENTITY_UP.contains(category) || AUTHORIZATION_UP.contains(category),
            "缺少 {category}"
        );
    }
    assert!(PROTECTION_UP.contains("COMMENT ON FUNCTION"));
    assert!(PROTECTION_UP.contains("COMMENT ON TRIGGER"));
}
