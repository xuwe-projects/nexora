use std::fs;

#[test]
fn login_gate_uses_a_controlled_checkbox_and_component_size() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/login_gate.rs"))
        .expect("登录门禁源码应可读取");

    assert!(source.contains("checkbox::Checkbox"));
    assert!(source.contains(".checked(self.remember_login)"));
    assert!(source.contains(".disabled("));
    assert!(source.contains("!self.remember_login_enabled || self.busy"));
    assert!(source.contains(".with_size(theme::component_size(cx))"));
    assert!(source.contains("on_remember_login"));
}

#[test]
fn login_gate_exposes_recovery_actions_without_token_state() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/login_gate.rs"))
        .expect("登录门禁源码应可读取");

    assert!(source.contains("recovery_actions"));
    assert!(source.contains("on_retry_recovery"));
    assert!(source.contains("on_login_other_account"));
    assert!(!source.contains("OidcTokenCache"));
    assert!(!source.contains("keyring"));
}
