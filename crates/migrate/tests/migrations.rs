use std::collections::BTreeSet;

const SQLX_CONFIG: &str = include_str!("../../../sqlx.toml");
const IDENTITY_UP: &str =
    include_str!("../migrations/20260807034412_account_identity_baseline.up.sql");
const AUTHORIZATION_UP: &str =
    include_str!("../migrations/20260807034430_account_authorization_baseline.up.sql");
const PROTECTION_UP: &str =
    include_str!("../migrations/20260807034446_account_protection_baseline.up.sql");
const CATALOG_UP: &str =
    include_str!("../migrations/20260807034500_account_catalog_baseline.up.sql");
const SERVICE_ACCOUNTS_UP: &str =
    include_str!("../migrations/20260814075639_account_service_accounts.up.sql");
const SERVICE_ACCOUNTS_DOWN: &str =
    include_str!("../migrations/20260814075639_account_service_accounts.down.sql");
const INTERNAL_SERVICE_ACCOUNTS_UP: &str =
    include_str!("../migrations/20260817060744_account_internal_service_accounts.up.sql");
const INTERNAL_SERVICE_ACCOUNTS_DOWN: &str =
    include_str!("../migrations/20260817060744_account_internal_service_accounts.down.sql");

#[test]
fn baseline_contains_four_reversible_timestamp_migrations() {
    let migration_names = std::fs::read_dir(format!("{}/migrations", env!("CARGO_MANIFEST_DIR")))
        .expect("迁移目录应当存在")
        .map(|entry| {
            entry
                .expect("迁移目录项应当可读取")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(migration_names.len(), 12);
    for version in [
        "20260807034412_account_identity_baseline",
        "20260807034430_account_authorization_baseline",
        "20260807034446_account_protection_baseline",
        "20260807034500_account_catalog_baseline",
    ] {
        assert!(migration_names.contains(&format!("{version}.up.sql")));
        assert!(migration_names.contains(&format!("{version}.down.sql")));
    }
    assert!(migration_names.contains("20260814075639_account_service_accounts.up.sql"));
    assert!(migration_names.contains("20260814075639_account_service_accounts.down.sql"));
    assert!(migration_names.contains("20260817060744_account_internal_service_accounts.up.sql"));
    assert!(migration_names.contains("20260817060744_account_internal_service_accounts.down.sql"));
}

#[test]
fn service_account_migration_is_reversible_and_never_stores_secrets() {
    assert!(SERVICE_ACCOUNTS_UP.contains("CREATE TYPE account.user_type"));
    assert!(SERVICE_ACCOUNTS_UP.contains("CREATE TABLE account.service_account_credentials"));
    assert!(SERVICE_ACCOUNTS_UP.contains("service_accounts:credentials.write"));
    assert!(SERVICE_ACCOUNTS_UP.contains("users_service_account_delete_forbidden"));
    for forbidden in [
        "client_secret TEXT",
        "personal_access_token TEXT",
        "token TEXT",
    ] {
        assert!(
            !SERVICE_ACCOUNTS_UP
                .to_lowercase()
                .contains(&forbidden.to_lowercase()),
            "凭据元数据迁移不得保存敏感字段: {forbidden}"
        );
    }
    assert!(SERVICE_ACCOUNTS_DOWN.contains("DROP TABLE account.service_account_credentials"));
    assert!(SERVICE_ACCOUNTS_DOWN.contains("DROP TYPE account.user_type"));
}

#[test]
fn internal_service_account_migration_is_nullable_but_keeps_human_identity_required() {
    assert!(INTERNAL_SERVICE_ACCOUNTS_UP.contains("identity_id DROP NOT NULL"));
    assert!(INTERNAL_SERVICE_ACCOUNTS_UP.contains("users_human_identity_required"));
    assert!(INTERNAL_SERVICE_ACCOUNTS_UP.contains("user_type <> 'human'"));
    assert!(INTERNAL_SERVICE_ACCOUNTS_DOWN.contains("identity_id IS NULL"));
    assert!(INTERNAL_SERVICE_ACCOUNTS_DOWN.contains("identity_id SET NOT NULL"));
}

#[test]
fn sqlx_configuration_uses_framework_history_and_reversible_timestamps() {
    assert!(SQLX_CONFIG.contains("migrations-dir = \"crates/migrate/migrations\""));
    assert!(SQLX_CONFIG.contains("create-schemas = [\"nexora\"]"));
    assert!(SQLX_CONFIG.contains("table-name = \"nexora._sqlx_migrations\""));
    assert!(SQLX_CONFIG.contains("migration-type = \"reversible\""));
    assert!(SQLX_CONFIG.contains("migration-versioning = \"timestamp\""));
}

#[test]
fn baseline_describes_only_the_current_account_structure() {
    assert!(IDENTITY_UP.contains("identity_id TEXT NOT NULL"));
    assert!(IDENTITY_UP.contains("username TEXT"));
    assert!(!IDENTITY_UP.contains("avatar_url"));
    assert!(!IDENTITY_UP.contains("subject TEXT"));

    assert!(AUTHORIZATION_UP.contains("id BIGSERIAL NOT NULL"));
    assert!(AUTHORIZATION_UP.contains("owner TEXT NOT NULL DEFAULT 'IMES'"));
    assert!(PROTECTION_UP.contains("CREATE FUNCTION account.protect_system_initialization()"));
    assert!(CATALOG_UP.contains("'users:provision'"));
    assert!(!CATALOG_UP.contains("users:avatar.write"));

    for sql in [IDENTITY_UP, AUTHORIZATION_UP, PROTECTION_UP, CATALOG_UP] {
        assert!(!sql.contains("_sqlx_migrations"));
        assert!(!sql.contains("ALTER TABLE"));
    }
}

#[test]
fn baseline_uses_postgresql_17_compatible_not_null_columns() {
    for sql in [IDENTITY_UP, AUTHORIZATION_UP, PROTECTION_UP, CATALOG_UP] {
        for line in sql.lines() {
            assert!(
                !(line.contains("CONSTRAINT") && line.contains("NOT NULL")),
                "PostgreSQL 17 不会把命名 NOT NULL 列约束注册为可注释的 pg_constraint: {line}"
            );
            assert!(
                !(line.contains("COMMENT ON CONSTRAINT") && line.contains("_not_null")),
                "PostgreSQL 17 无法注释命名 NOT NULL 列约束: {line}"
            );
        }
    }

    assert!(IDENTITY_UP.contains("id VARCHAR(8) NOT NULL"));
    assert!(AUTHORIZATION_UP.contains("id BIGSERIAL NOT NULL"));
}
