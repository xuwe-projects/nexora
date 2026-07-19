use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    Context, FocusHandle, IntoElement, Modifiers, Render, TestAppContext, Window, div, prelude::*,
};
use ui::PanelDialog;

const PANEL_DIALOG_SOURCE: &str = include_str!("../src/panel_dialog.rs");

struct PanelDialogTestRoot {
    dismissed: Arc<AtomicBool>,
    focus_handle: FocusHandle,
}

impl Render for PanelDialogTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dismissed = self.dismissed.clone();

        div()
            .id("panel-dialog-host")
            .debug_selector(|| "panel-dialog-host".into())
            .relative()
            .size_full()
            .child(
                PanelDialog::new("test-panel-dialog", self.focus_handle.clone())
                    .title("创建角色")
                    .footer(div().child("操作区"))
                    .on_close(move |_, _, _| {
                        dismissed.store(true, Ordering::SeqCst);
                    })
                    .child("表单内容"),
            )
    }
}

#[gpui::test]
fn panel_dialog_is_scoped_to_its_parent_and_exposes_close_action(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dismissed = Arc::new(AtomicBool::new(false));
    let dismissed_for_view = dismissed.clone();
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        PanelDialogTestRoot {
            dismissed: dismissed_for_view,
            focus_handle,
        }
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let host = cx
        .debug_bounds("panel-dialog-host")
        .expect("测试宿主应当完成布局");
    let overlay = cx
        .debug_bounds("panel-dialog-overlay")
        .expect("PanelDialog 应当渲染局部遮罩");
    let close = cx
        .debug_bounds("panel-dialog-close")
        .expect("PanelDialog 应当渲染关闭按钮");

    assert_eq!(overlay, host);

    cx.simulate_click(close.center(), Modifiers::none());

    assert!(dismissed.load(Ordering::SeqCst));
}

#[test]
fn source_contract_keeps_body_scroll_inside_dialog_surface() {
    assert!(PANEL_DIALOG_SOURCE.contains(".debug_selector(|| \"panel-dialog-content\".into())"));
    assert!(PANEL_DIALOG_SOURCE.contains(".flex_1()"));
    assert!(PANEL_DIALOG_SOURCE.contains(".min_h_0()"));
    assert!(PANEL_DIALOG_SOURCE.contains(".overflow_y_scrollbar()"));
}
