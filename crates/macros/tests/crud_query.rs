#[test]
fn crud_query_rejects_invalid_contracts() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/crud_query_*.rs");
}
