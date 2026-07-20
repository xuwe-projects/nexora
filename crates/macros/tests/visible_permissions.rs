#[test]
fn visible_permissions_requires_string_array() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/feature_visible_permissions.rs");
}
