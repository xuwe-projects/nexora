use std::sync::{Arc, Mutex};

use gpui::{Context, Modifiers, Render, TestAppContext, Window, div, point, prelude::*, px};
use ui::TableSwitchCell;

struct SwitchCellHarness {
    allowed: bool,
    loading: bool,
    changes: Arc<Mutex<Vec<bool>>>,
}

impl Render for SwitchCellHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        let changes = self.changes.clone();
        div().size_full().child(
            TableSwitchCell::new("table-switch-test", false)
                .allowed(self.allowed)
                .loading(self.loading)
                .on_change(move |checked, _, _| {
                    changes.lock().expect("变化日志锁不应中毒").push(checked);
                }),
        )
    }
}

fn click_switch(cx: &mut TestAppContext, allowed: bool, loading: bool) -> Arc<Mutex<Vec<bool>>> {
    cx.update(gpui_component::init);
    let changes = Arc::new(Mutex::new(Vec::new()));
    let changes_for_view = changes.clone();
    let (_, cx) = cx.add_window_view(move |_, _| SwitchCellHarness {
        allowed,
        loading,
        changes: changes_for_view,
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(10.), px(10.)), Modifiers::none());
    changes
}

#[gpui::test]
fn allowed_idle_switch_reports_target_value(cx: &mut TestAppContext) {
    let changes = click_switch(cx, true, false);
    assert_eq!(
        changes.lock().expect("变化日志锁不应中毒").as_slice(),
        [true]
    );
}

#[gpui::test]
fn permission_and_loading_states_block_updates(cx: &mut TestAppContext) {
    let denied = click_switch(cx, false, false);
    assert!(denied.lock().expect("变化日志锁不应中毒").is_empty());

    let loading = click_switch(cx, true, true);
    assert!(loading.lock().expect("变化日志锁不应中毒").is_empty());
}
