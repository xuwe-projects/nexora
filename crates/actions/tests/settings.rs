use actions::settings;

#[test]
fn settings_shortcut_matches_the_current_platform() {
    let expected = if cfg!(target_os = "macos") {
        "Cmd+,"
    } else {
        "Ctrl+,"
    };

    assert_eq!(settings::shortcut_label(), expected);
}
