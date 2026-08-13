use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AppContext as _, Context, IntoElement, Modifiers, Render, TestAppContext, Window, div,
    prelude::*, px,
};
use gpui_component::form::field;
use ui::{FormDialog, FormDialogState};

const FORM_DIALOG_SOURCE: &str = include_str!("../src/form_dialog.rs");
const PANEL_DIALOG_SOURCE: &str = include_str!("../src/panel_dialog.rs");

struct FormDialogTestRoot {
    state: gpui::Entity<FormDialogState>,
    cancelled: Arc<AtomicUsize>,
    submitted: Arc<AtomicUsize>,
}

struct FormDialogGridRoot {
    state: gpui::Entity<FormDialogState>,
}

struct FormDialogLongContentRoot {
    state: gpui::Entity<FormDialogState>,
}

struct FormDialogCustomMaxHeightRoot {
    state: gpui::Entity<FormDialogState>,
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
                        field().label("名称").child(
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

impl Render for FormDialogGridRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("form-dialog-grid-host")
            .relative()
            .size_full()
            .child(
                FormDialog::new("grid-form-dialog", self.state.clone())
                    .title("编辑资料")
                    .columns(2)
                    .child(
                        field()
                            .label("名字")
                            .child(grid_control("form-dialog-first-field")),
                    )
                    .child(
                        field()
                            .label("姓氏")
                            .child(grid_control("form-dialog-second-field")),
                    )
                    .child(
                        field()
                            .label("详细说明")
                            .col_span(2)
                            .child(grid_control("form-dialog-full-row-field")),
                    )
                    .on_submit(|_, _, _| {}),
            )
    }
}

impl Render for FormDialogLongContentRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dialog = (0..16).fold(
            FormDialog::new("long-content-form-dialog", self.state.clone())
                .title("编辑长表单")
                .description("字段较多时只有内容区滚动。"),
            |dialog, index| {
                dialog.section(
                    div()
                        .h(px(72.0))
                        .w_full()
                        .child(format!("长表单内容 {index}")),
                )
            },
        );

        div()
            .id("form-dialog-long-host")
            .debug_selector(|| "form-dialog-long-host".into())
            .relative()
            .size_full()
            .child(dialog.on_submit(|_, _, _| {}))
    }
}

impl Render for FormDialogCustomMaxHeightRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("form-dialog-custom-max-host")
            .debug_selector(|| "form-dialog-custom-max-host".into())
            .relative()
            .size_full()
            .child(
                FormDialog::new("custom-max-form-dialog", self.state.clone())
                    .title("短表单")
                    .child(
                        field().label("名称").child(
                            div()
                                .debug_selector(|| "form-dialog-custom-max-content".into())
                                .child("短内容"),
                        ),
                    )
                    .max_panel_height_ratio(0.6)
                    .on_submit(|_, _, _| {}),
            )
    }
}

fn grid_control(selector: &'static str) -> impl IntoElement {
    div()
        .debug_selector(move || selector.into())
        .h(px(24.0))
        .w_full()
        .child("控件")
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
fn form_dialog_short_content_uses_intrinsic_height_below_default_max(cx: &mut TestAppContext) {
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
    let max_height = host.size.height * 0.8;

    assert!(
        surface.size.height < max_height,
        "短表单 surface 应按内容自适应且低于 80% 上限：actual {}, max {}",
        surface.size.height,
        max_height
    );
}

#[gpui::test]
fn form_dialog_long_content_is_capped_at_default_max_height(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let state = cx.new(FormDialogState::new);
        state.update(cx, |state, cx| state.open(window, cx));
        FormDialogLongContentRoot { state }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let host = cx
        .debug_bounds("form-dialog-long-host")
        .expect("长表单宿主应当完成布局");
    let surface = cx
        .debug_bounds("panel-dialog-surface")
        .expect("FormDialog 应当渲染 PanelDialog surface");
    let content = cx
        .debug_bounds("panel-dialog-content")
        .expect("PanelDialog 应当渲染内容区域");
    let cancel = cx
        .debug_bounds("form-dialog-cancel")
        .expect("FormDialog 应当渲染取消按钮");
    let submit = cx
        .debug_bounds("form-dialog-submit")
        .expect("FormDialog 应当渲染提交按钮");
    let max_height = host.size.height * 0.8;
    let delta = (surface.size.height.as_f32() - max_height.as_f32()).abs();

    assert!(
        delta <= 1.0,
        "长表单 surface 应达到但不超过 80% 上限：actual {}, max {}",
        surface.size.height,
        max_height
    );
    assert!(content.origin.y > surface.origin.y);
    assert!(content.origin.y + content.size.height <= cancel.origin.y);
    assert!(cancel.origin.y + cancel.size.height <= surface.origin.y + surface.size.height);
    assert!(submit.origin.y + submit.size.height <= surface.origin.y + surface.size.height);
}

#[gpui::test]
fn max_panel_height_ratio_does_not_force_short_content_taller(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let state = cx.new(FormDialogState::new);
        state.update(cx, |state, cx| state.open(window, cx));
        FormDialogCustomMaxHeightRoot { state }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let host = cx
        .debug_bounds("form-dialog-custom-max-host")
        .expect("自定义高度宿主应当完成布局");
    let surface = cx
        .debug_bounds("panel-dialog-surface")
        .expect("FormDialog 应当渲染 PanelDialog surface");
    let max_height = host.size.height * 0.6;

    assert!(
        surface.size.height < max_height,
        "自定义最大高度比例不应把短表单强制撑高：actual {}, max {}",
        surface.size.height,
        max_height
    );
}

#[gpui::test]
fn form_dialog_full_row_item_spans_all_configured_columns(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let state = cx.new(FormDialogState::new);
        state.update(cx, |state, cx| state.open(window, cx));
        FormDialogGridRoot { state }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let first = cx
        .debug_bounds("form-dialog-first-field")
        .expect("第一列字段应当渲染");
    let second = cx
        .debug_bounds("form-dialog-second-field")
        .expect("第二列字段应当渲染");
    let full = cx
        .debug_bounds("form-dialog-full-row-field")
        .expect("full_row 字段应当渲染");

    assert!(first.origin.y == second.origin.y);
    assert!(full.origin.y > first.origin.y);
    assert!(
        full.size.width > first.size.width + second.size.width,
        "full_row 字段应当跨越两列：full={}, first={}, second={}",
        full.size.width,
        first.size.width,
        second.size.width
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
    assert!(FORM_DIALOG_SOURCE.contains("max_panel_height_ratio: f32"));
    assert!(FORM_DIALOG_SOURCE.contains("pub fn max_panel_height_ratio(mut self, ratio: f32)"));
    assert!(FORM_DIALOG_SOURCE.contains("ratio.clamp(0.1, 1.0)"));
    assert!(FORM_DIALOG_SOURCE.contains(".max_h(relative(max_panel_height_ratio))"));
    assert!(!FORM_DIALOG_SOURCE.contains(&format!("pub fn auto_{}", "height")));
    assert!(!FORM_DIALOG_SOURCE.contains(&format!("pub fn panel_{}", "height_ratio")));
    assert!(!FORM_DIALOG_SOURCE.contains(&format!("fn form_dialog_{}(", "height")));
    assert!(!FORM_DIALOG_SOURCE.contains(&format!(".h(relative({}))", "ratio")));
    assert!(FORM_DIALOG_SOURCE.contains(".max_w(relative(0.92))"));
    assert!(FORM_DIALOG_SOURCE.contains("form::{Field, v_form}"));
    assert!(FORM_DIALOG_SOURCE.contains("v_form()"));
    assert!(!FORM_DIALOG_SOURCE.contains("FormItem"));
    assert!(!FORM_DIALOG_SOURCE.contains("FormItemControl"));
    assert!(PANEL_DIALOG_SOURCE.contains(".debug_selector(|| \"panel-dialog-content\".into())"));
    assert!(PANEL_DIALOG_SOURCE.contains(".flex_auto()"));
    assert!(PANEL_DIALOG_SOURCE.contains(".overflow_y_scroll()"));
    assert!(PANEL_DIALOG_SOURCE.contains(".overflow_hidden()"));
}
