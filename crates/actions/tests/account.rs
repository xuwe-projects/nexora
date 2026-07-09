use actions::account::{self, AccountActionKind};

#[test]
fn account_menu_actions_keep_stable_order_and_shortcuts() {
    let actions = account::menu_actions("刘吉祥");

    assert_eq!(
        actions
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>(),
        vec!["刘吉祥", "设置", "退出登录"]
    );
    assert_eq!(actions[0].kind(), AccountActionKind::Profile);
    assert_eq!(actions[0].shortcut(), Some("Cmd+Shift+P"));
    assert!(actions[0].uses_account_avatar());
    assert_eq!(actions[1].kind(), AccountActionKind::Settings);
    assert_eq!(actions[1].shortcut(), Some("Cmd+,"));
    assert_eq!(actions[2].kind(), AccountActionKind::SignOut);
    assert_eq!(actions[2].shortcut(), Some("Cmd+Shift+Q"));
}

#[test]
fn account_menu_context_is_stable() {
    assert_eq!(account::CONTEXT, "console_account_menu");
    actions::init();
}
