use gpui::{Context, Entity, IntoElement, Render, TestAppContext, Window, div, prelude::*};
use ui::{Cascader, CascaderOption, CascaderState};

struct CascaderTestRoot {
    state: Entity<CascaderState>,
}

impl CascaderTestRoot {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let options =
            [
                CascaderOption::new("cn", "中国").child(
                    CascaderOption::new("gd", "广东").children([
                        CascaderOption::new("sz", "深圳"),
                        CascaderOption::new("gz", "广州"),
                    ]),
                ),
            ];
        let state = cx.new(|cx| CascaderState::new("location", options, window, cx));
        Self { state }
    }
}

impl Render for CascaderTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(Cascader::new(&self.state).w_full())
    }
}

#[gpui::test]
fn cascader_resolves_stable_value_and_label_paths(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (root, cx) = cx.add_window_view(CascaderTestRoot::new);
    let state = cx.read_entity(&root, |root, _| root.state.clone());

    cx.update_entity(&state, |state, cx| {
        state
            .set_value(["cn", "gd", "sz"], cx)
            .expect("完整路径应能设置");
    });
    cx.read_entity(&state, |state, _| {
        assert_eq!(
            state
                .selection()
                .values()
                .iter()
                .map(AsRef::<str>::as_ref)
                .collect::<Vec<_>>(),
            ["cn", "gd", "sz"]
        );
        assert_eq!(
            state
                .selection()
                .labels()
                .iter()
                .map(AsRef::<str>::as_ref)
                .collect::<Vec<_>>(),
            ["中国", "广东", "深圳"]
        );
    });
}

#[gpui::test]
fn cascader_rejects_unknown_path_without_overwriting_selection(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (root, cx) = cx.add_window_view(CascaderTestRoot::new);
    let state = cx.read_entity(&root, |root, _| root.state.clone());

    cx.update_entity(&state, |state, cx| {
        state
            .set_value(["cn", "gd", "gz"], cx)
            .expect("初始路径应能设置");
        let error = state
            .set_value(["cn", "unknown"], cx)
            .expect_err("未知路径必须被拒绝");
        assert_eq!(error.depth(), 1);
        assert_eq!(error.value(), "unknown");
        assert_eq!(
            state
                .selection()
                .values()
                .iter()
                .map(AsRef::<str>::as_ref)
                .collect::<Vec<_>>(),
            ["cn", "gd", "gz"]
        );
    });
}

#[gpui::test]
fn cascader_trigger_renders_with_gpui_component_primitives(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(CascaderTestRoot::new);

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}
