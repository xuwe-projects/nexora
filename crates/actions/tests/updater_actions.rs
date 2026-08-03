use actions::{
    account::{AccountActionKind, menu_actions_with_updates},
    updater::{self, CheckForUpdates},
};

#[test]
fn account_menu_only_exposes_updates_when_installed() {
    assert!(
        menu_actions_with_updates(false)
            .iter()
            .all(|item| item.kind() != AccountActionKind::Updates)
    );
    let actions = menu_actions_with_updates(true);
    let update = actions
        .iter()
        .find(|item| item.kind() == AccountActionKind::Updates)
        .expect("安装 updater 后应显示检查更新入口");

    assert_eq!(update.label(), "检查更新");
    assert_eq!(update.shortcut(), Some(updater::shortcut_label()));
    assert!(update.to_action().as_any().is::<CheckForUpdates>());
}

#[test]
fn updater_shortcut_matches_the_current_platform() {
    let expected = if cfg!(target_os = "macos") {
        "Cmd+Shift+U"
    } else {
        "Ctrl+Shift+U"
    };

    assert_eq!(updater::shortcut_label(), expected);
}
