use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AppContext as _, Context, IntoElement, Modifiers, Render, TestAppContext, Window, div,
    prelude::*, px,
};
use ui::{FormDialog, FormDialogState, FormItem};

const FORM_DIALOG_SOURCE: &str = include_str!("../src/form_dialog.rs");

struct FormDialogTestRoot {
    state: gpui::Entity<FormDialogState>,
    cancelled: Arc<AtomicUsize>,
    submitted: Arc<AtomicUsize>,
}

impl Render for FormDialogTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let cancelled = self.cancelled.clone();
        let submitted = self.submitted.clone();

        div()
            .id("form-dialog-host")
            .debug_selector(|| "form-dialog-host".into())
            .relative()
            .size_full()
            .child(
                FormDialog::new("disabled-submit-form-dialog", self.state.clone())
                    .title("编辑用户")
                    .child(
                        FormItem::new("名称").element(
                            div()
                                .debug_selector(|| "form-dialog-custom-content".into())
                                .child("表单内容"),
                        ),
                    )
                    .submit_disabled(true)
                    .on_submit(move |_, _, _| {
                        submitted.fetch_add(1, Ordering::SeqCst);
                    })
                    .on_cancel(move |_, _, _| {
                        cancelled.fetch_add(1, Ordering::SeqCst);
                    }),
            )
    }
}

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

#[gpui::test]
fn submit_disabled_only_blocks_submit_and_keeps_cancel_available(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cancelled = Arc::new(AtomicUsize::new(0));
    let submitted = Arc::new(AtomicUsize::new(0));
    let cancelled_for_view = cancelled.clone();
    let submitted_for_view = submitted.clone();
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let state = cx.new(FormDialogState::new);
        state.update(cx, |state, cx| state.open(window, cx));
        FormDialogTestRoot {
            state,
            cancelled: cancelled_for_view,
            submitted: submitted_for_view,
        }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let cancel = cx
        .debug_bounds("form-dialog-cancel")
        .expect("FormDialog 应当渲染取消按钮");
    let submit = cx
        .debug_bounds("form-dialog-submit")
        .expect("FormDialog 应当渲染提交按钮");
    let content = cx
        .debug_bounds("form-dialog-custom-content")
        .expect("FormDialog 应当渲染标准表单项内容");
    let panel_content = cx
        .debug_bounds("panel-dialog-content")
        .expect("PanelDialog 应当渲染内容区域");
    let surface = cx
        .debug_bounds("panel-dialog-surface")
        .expect("PanelDialog 应当渲染 surface");
    assert!(content.size.height > px(0.0));
    assert!(panel_content.size.height > content.size.height);
    assert!(cancel.origin.y + cancel.size.height <= surface.origin.y + surface.size.height);

    cx.simulate_click(submit.center(), Modifiers::none());
    assert_eq!(submitted.load(Ordering::SeqCst), 0);
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let cancel = cx
        .debug_bounds("form-dialog-cancel")
        .expect("FormDialog 应当继续渲染取消按钮");

    cx.simulate_click(cancel.center(), Modifiers::none());
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
}

#[gpui::test]
fn form_dialog_defaults_to_eighty_percent_of_panel_height(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cancelled = Arc::new(AtomicUsize::new(0));
    let submitted = Arc::new(AtomicUsize::new(0));
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let state = cx.new(FormDialogState::new);
        state.update(cx, |state, cx| state.open(window, cx));
        FormDialogTestRoot {
            state,
            cancelled,
            submitted,
        }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let host = cx
        .debug_bounds("form-dialog-host")
        .expect("测试宿主应当完成布局");
    let surface = cx
        .debug_bounds("panel-dialog-surface")
        .expect("FormDialog 应当渲染 PanelDialog surface");
    let expected = host.size.height * 0.8;
    let delta = (surface.size.height.as_f32() - expected.as_f32()).abs();

    assert!(
        delta <= 1.0,
        "FormDialog surface height should be 80% of the panel: actual {}, expected {}",
        surface.size.height,
        expected
    );
}

#[test]
fn source_contract_applies_business_disabled_state_to_submit_only() {
    let (_, after_cancel) = FORM_DIALOG_SOURCE
        .split_once("Button::new(\"form-dialog-cancel\")")
        .expect("FormDialog 源码应当包含取消按钮");
    let (cancel_block, after_submit) = after_cancel
        .split_once("Button::new(\"form-dialog-submit\")")
        .expect("FormDialog 源码应当包含提交按钮");
    let (submit_block, _) = after_submit
        .split_once(".on_click")
        .expect("提交按钮应当绑定点击处理器");

    assert!(cancel_block.contains(".disabled(submitting)"));
    assert!(!cancel_block.contains("self.submit_disabled"));
    assert!(submit_block.contains(".disabled(submit_disabled)"));
    assert!(FORM_DIALOG_SOURCE.contains("let submit_disabled ="));
    assert!(FORM_DIALOG_SOURCE.contains("DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO: f32 = 0.8"));
    assert!(FORM_DIALOG_SOURCE.contains("fn form_dialog_height("));
    assert!(FORM_DIALOG_SOURCE.contains(".h(relative(ratio))"));
    assert!(FORM_DIALOG_SOURCE.contains(".max_h(relative(ratio))"));
    assert!(FORM_DIALOG_SOURCE.contains(".max_w(relative(0.92))"));
}
