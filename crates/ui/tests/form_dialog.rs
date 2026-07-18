use gpui::{AppContext as _, TestAppContext};
use ui::FormDialogState;

#[gpui::test]
fn form_dialog_state_reports_dirty_fields_and_draft_values(cx: &mut TestAppContext) {
    let state = cx.new(FormDialogState::new);

    cx.update_entity(&state, |state, cx| {
        state.set_field_draft("name", "名称", "旧名称", "新名称", cx);
        state.set_field_draft("email", "邮箱", "a@example.com", "a@example.com", cx);
    });

    cx.read_entity(&state, |state, _| {
        assert!(state.is_dirty());
        let unsaved = state.unsaved_fields();
        assert_eq!(unsaved.len(), 1);
        assert_eq!(unsaved[0].key(), "name");
        assert_eq!(unsaved[0].label().as_ref(), "名称");
        assert_eq!(unsaved[0].original(), "旧名称");
        assert_eq!(unsaved[0].draft(), "新名称");
        assert_eq!(
            state.draft_values().get("email").map(String::as_str),
            Some("a@example.com")
        );
    });
}

#[gpui::test]
fn form_dialog_state_can_promote_drafts_to_saved_baseline(cx: &mut TestAppContext) {
    let state = cx.new(FormDialogState::new);

    cx.update_entity(&state, |state, cx| {
        state.set_field_draft("roles", "角色", "1", "1,2", cx);
        assert!(state.is_dirty());
        state.mark_saved(cx);
    });

    cx.read_entity(&state, |state, _| {
        assert!(!state.is_dirty());
        assert!(state.unsaved_fields().is_empty());
    });
}
