#![cfg(feature = "desktop")]

use actions::updater::CheckForUpdates;
use gpui::{Empty, TestAppContext};
use gpui_component::kbd::Kbd;
use nexora::desktop::{
    ApplicationInfo, UpdateChannel, UpdateConfig, UpdateError, application_info,
    check_for_updates_button, install_updater, report_health_from_env_args,
    run_sidecar_from_env_args, updater_available,
};

const UPDATER_INTEGRATION_SOURCE: &str = include_str!("../src/desktop/updater.rs");

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

    let window = cx.add_window(|_, _| Empty);
    window
        .update(cx, |_, window, _| {
            assert!(Kbd::binding_for_action(&CheckForUpdates, None, window).is_some());
        })
        .unwrap();
}

#[test]
fn check_for_updates_action_defers_active_window_update() {
    let handler = UPDATER_INTEGRATION_SOURCE
        .split_once("cx.on_action(|_: &CheckForUpdates, cx|")
        .and_then(|(_, source)| source.split_once("window_actions::enable_update_menu(cx);"))
        .map(|(source, _)| source)
        .expect("应当可以定位检查更新 Action 处理器");

    let defer = handler
        .find("cx.defer(move |cx|")
        .expect("检查更新 Action 必须延迟到当前窗口事件结束后执行");
    let window_update = handler
        .find("window.update(cx")
        .expect("延迟回调必须更新触发 Action 的活动窗口");

    assert!(
        defer < window_update,
        "活动窗口更新必须位于 defer 回调内，避免重入当前窗口"
    );
    assert!(handler.contains("check_for_updates(window, cx)"));
}

#[test]
fn sidecar_runtime_entries_are_available_from_desktop_facade() {
    let run_sidecar: fn() -> Result<bool, UpdateError> = run_sidecar_from_env_args;
    let report_health: fn() -> Result<bool, UpdateError> = report_health_from_env_args;

    assert!(!run_sidecar().unwrap());
    assert!(!report_health().unwrap());
}

#[test]
fn general_application_info_reader_is_available_from_desktop_facade() {
    let _: for<'a> fn(&'a gpui::App) -> &'a ApplicationInfo = application_info;
}

#[test]
fn only_successful_health_session_opens_update_completed_dialog() {
    let startup = UPDATER_INTEGRATION_SOURCE
        .split_once("pub(crate) fn start_installed_updater")
        .map(|(_, source)| source)
        .unwrap();
    assert!(startup.contains("match report_health_from_env_args()"));
    assert!(startup.contains("Ok(true) =>"));
    assert!(startup.contains("show_update_completed_dialog("));
    assert!(startup.contains("Ok(false) => {}"));
}
