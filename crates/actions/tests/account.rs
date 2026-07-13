use actions::{
    account::{self, AccountActionKind},
    settings::{self, OpenSettings},
};

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
    assert_eq!(actions[1].kind(), AccountActionKind::Settings);
    assert_eq!(actions[1].shortcut(), Some(settings::shortcut_label()));
    assert!(actions[1].to_action().as_any().is::<OpenSettings>());
    assert_eq!(actions[2].kind(), AccountActionKind::SignOut);
    assert_eq!(actions[2].shortcut(), Some("Cmd+Shift+Q"));
}

#[test]
fn signed_out_account_menu_actions_start_with_login() {
    let actions = account::signed_out_menu_actions();

    assert_eq!(
        actions
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>(),
        vec!["登录", "设置"]
    );
    assert_eq!(actions[0].kind(), AccountActionKind::SignIn);
    assert_eq!(actions[0].shortcut(), Some("Cmd+Shift+L"));
    assert_eq!(actions[1].kind(), AccountActionKind::Settings);
}

#[test]
fn account_menu_context_is_stable() {
    assert_eq!(account::CONTEXT, "console_account_menu");
    actions::init();
}
