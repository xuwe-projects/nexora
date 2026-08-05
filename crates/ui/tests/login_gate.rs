use std::fs;

use gpui::{Context, Render, TestAppContext, Window, div, prelude::*, px};
use ui::LoginGate;

struct LoginGateTestRoot {
    title_bar: bool,
}

impl Render for LoginGateTestRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
            .debug_selector(|| "login-gate-test-root".into())
            .size(px(640.0))
            .child(
                LoginGate::new("iMES", "1.2.3", |_, _, _| {}, |_, _, _| {})
                    .title_bar(self.title_bar),
            )
    }
}

#[test]
fn login_gate_uses_a_controlled_checkbox_and_component_size() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/login_gate.rs"))
        .expect("登录门禁源码应可读取");

    assert!(source.contains("checkbox::Checkbox"));
    assert!(source.contains(".checked(self.remember_login)"));
    assert!(source.contains(".disabled("));
    assert!(source.contains("!self.remember_login_enabled || self.busy"));
    assert!(source.contains(".with_size(theme::component_size(cx))"));
    assert!(source.contains("on_remember_login"));
}

#[test]
fn login_gate_exposes_recovery_actions_without_token_state() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/login_gate.rs"))
        .expect("登录门禁源码应可读取");

    assert!(source.contains("recovery_actions"));
    assert!(source.contains("on_retry_recovery"));
    assert!(source.contains("on_login_other_account"));
    assert!(!source.contains("OidcTokenCache"));
    assert!(!source.contains("keyring"));
}

#[gpui::test]
fn standalone_login_gate_title_bar_has_full_width_and_nonzero_height(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(theme::init);
    let (_root, cx) = cx.add_window_view(|_, _| LoginGateTestRoot { title_bar: true });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let root = cx
        .debug_bounds("login-gate-test-root")
        .expect("登录页测试根容器应渲染");
    let title_bar = cx
        .debug_bounds("login-gate-title-bar")
        .expect("独立 LoginGate 应渲染一组官方 TitleBar");
    assert_eq!(title_bar.size.width, root.size.width);
    assert!(title_bar.size.height > px(0.0));
}

#[gpui::test]
fn shell_managed_login_gate_can_disable_its_own_title_bar(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(theme::init);
    let (_root, cx) = cx.add_window_view(|_, _| LoginGateTestRoot { title_bar: false });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    assert!(cx.debug_bounds("login-gate-title-bar").is_none());
}
