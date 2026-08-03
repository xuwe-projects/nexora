use actions::account::{AccountActionKind, menu_actions_with_updates};

#[test]
fn account_menu_only_exposes_updates_when_installed() {
    assert!(
        menu_actions_with_updates(false)
            .iter()
            .all(|item| item.kind() != AccountActionKind::Updates)
    );
    assert!(
        menu_actions_with_updates(true)
            .iter()
            .any(|item| item.kind() == AccountActionKind::Updates && item.label() == "检查更新")
    );
}
