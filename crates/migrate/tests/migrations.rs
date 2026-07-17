use std::collections::BTreeSet;

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
        BTreeSet::from([1, 2, 3, 4, 5])
    );

    first.pop();
    assert_eq!(first.len() + 1, second.len());
}
