const DIALOG_SOURCE: &str = include_str!("../src/dialog.rs");

#[test]
fn update_prompt_uses_standard_components_and_preserves_mandatory_gate() {
    for expected in [
        "Button::new(\"update-view-release-notes\")",
        "Progress::new(\"update-notes-loading\")",
        "TextView::markdown(\"update-release-notes-markdown\"",
        ".scrollable(true)",
        "Button::new(\"update-later\")",
        "Button::new(\"update-background\")",
        "Button::new(\"update-immediate\")",
    ] {
        assert!(
            DIALOG_SOURCE.contains(expected),
            "缺少标准更新 UI：{expected}"
        );
    }
    let prompt = DIALOG_SOURCE
        .split_once("fn open_update_prompt(")
        .and_then(|(_, source)| source.split_once("fn open_progress_dialog("))
        .map(|(source, _)| source)
        .unwrap();
    assert!(prompt.contains(".overlay_closable(false)"));
    assert!(prompt.contains(".close_button(false)"));
    assert!(prompt.contains(".keyboard(false)"));
    assert!(DIALOG_SOURCE.contains(".when(!mandatory"));
}

#[test]
fn update_completed_dialog_is_process_once_closable_and_uses_local_verified_notes() {
    assert!(DIALOG_SOURCE.contains("cx.has_global::<UpdateCompletedDialogShown>()"));
    assert!(DIALOG_SOURCE.contains("cx.set_global(UpdateCompletedDialogShown)"));
    assert!(DIALOG_SOURCE.contains(".overlay_closable(true)"));
    assert!(DIALOG_SOURCE.contains(".close_button(true)"));
    assert!(DIALOG_SOURCE.contains("read_verified_local_release_notes"));
}
