#![cfg(feature = "desktop")]

use gpui::TestAppContext;
use nexora::desktop::{
    UpdateChannel, UpdateConfig, check_for_updates_button, install_updater, updater_available,
};

fn update_config() -> UpdateConfig {
    UpdateConfig::new(
        "https://updates.example.invalid/latest.json",
        "com.example.integration",
        "1.0.0",
        1,
        UpdateChannel::Stable,
    )
    .unwrap()
}

#[gpui::test]
fn updater_entries_follow_explicit_installation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        assert!(!updater_available(cx));
        assert!(check_for_updates_button("not-installed", cx).is_none());

        install_updater(update_config(), cx).unwrap();

        assert!(updater_available(cx));
        assert!(check_for_updates_button("installed", cx).is_some());
        assert!(install_updater(update_config(), cx).is_err());
    });
}
