use gpui::{Empty, KeyBinding, TestAppContext};
use ui::ShortcutHint;

gpui::actions!(shortcut_hint_test, [OpenSearch]);

#[gpui::test]
fn shortcut_hint_preserves_every_keystroke_in_a_registered_sequence(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let window = cx.add_window(|_, _| Empty);

    let without_binding = window
        .update(cx, |_, window, _| {
            ShortcutHint::binding_for_action(&OpenSearch, None, window)
        })
        .unwrap();
    assert!(without_binding.is_none());

    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new("shift shift", OpenSearch, None)]);
    });
    let hint = window
        .update(cx, |_, window, _| {
            ShortcutHint::binding_for_action(&OpenSearch, None, window)
        })
        .unwrap()
        .expect("注册双击 Shift 后应生成提示");

    assert_eq!(hint.keystrokes().len(), 2);
    assert!(
        hint.keystrokes()
            .iter()
            .all(|keystroke| keystroke.key == "shift")
    );
}
