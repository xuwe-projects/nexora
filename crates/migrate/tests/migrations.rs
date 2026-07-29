use std::collections::BTreeSet;

const CURRENT_IMES_SYSTEM_DICTIONARY_SEED_VERSION: i64 = 202607180006;
const ACCOUNT_AVATAR_REMOVAL_VERSION: i64 = 202607290001;

#[test]
fn exported_migrations_include_all_framework_versions_and_are_independent() {
    let mut first = migrate::migrations();
    let second = migrate::migrations();

    assert_eq!(first.len(), second.len());
    assert_eq!(
        second
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .map(|migration| migration.version)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            ACCOUNT_AVATAR_REMOVAL_VERSION,
        ])
    );

    first.pop();
    assert_eq!(first.len() + 1, second.len());
}

#[test]
fn account_avatar_removal_migration_runs_after_current_imes_seed_migration() {
    let versions = migrate::migrations()
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| migration.version)
        .collect::<BTreeSet<_>>();

    let avatar_removal_version = versions
        .iter()
        .copied()
        .find(|version| *version == ACCOUNT_AVATAR_REMOVAL_VERSION)
        .expect("Account 头像移除迁移必须导出");
    assert!(avatar_removal_version > CURRENT_IMES_SYSTEM_DICTIONARY_SEED_VERSION);
}

#[test]
fn account_avatar_removal_migration_drops_column_and_permission_without_checksum_repairs() {
    let up_sql = include_str!("../migrations/202607290001_account_remove_avatar_capability.up.sql");

    assert!(up_sql.contains("DROP COLUMN IF EXISTS avatar_url"));
    assert!(up_sql.contains("users:avatar.write"));
    assert!(up_sql.contains("DELETE FROM account.role_permissions"));
    assert!(up_sql.contains("DELETE FROM account.permissions"));
    assert!(!up_sql.contains("_sqlx_migrations"));
}
