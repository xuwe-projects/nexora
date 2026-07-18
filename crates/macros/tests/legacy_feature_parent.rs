#[test]
fn feature_parent_is_rejected_with_navigation_group_migration_hint() {
    trybuild::TestCases::new().compile_fail("tests/ui/feature_parent.rs");
}
