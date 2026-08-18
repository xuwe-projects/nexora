use actions::search::OpenGlobalSearch;
use gpui::{
    Context, Empty, FocusHandle, KeyBinding, Modifiers, Render, TestAppContext, Window, div,
    prelude::*, px,
};
use nexora::ShellToolbarOptions;
use ui::ShortcutHint;

const APPLICATION_SOURCE: &str = include_str!("../src/application.rs");

struct FocusedShortcutHost {
    focus_handle: FocusHandle,
    invocations: std::rc::Rc<std::cell::Cell<usize>>,
}

impl Render for FocusedShortcutHost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div().track_focus(&self.focus_handle).on_action(cx.listener(
            |this, _: &OpenGlobalSearch, _, _| {
                this.invocations.set(this.invocations.get() + 1);
            },
        ))
    }
}

#[test]
fn tab_bar_keeps_navigation_prefix_feature_icons_and_open_page_suffix() {
    let tabs = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_active_feature"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Shell 标签栏实现");

    assert!(tabs.contains(".h(px(42.0))"));
    assert!(tabs.contains(".prefix(self.render_tab_bar_prefix(cx))"));
    assert!(tabs.contains(".suffix(self.render_tab_bar_suffix(cx))"));

    let tab = APPLICATION_SOURCE
        .split_once("fn render_tab(")
        .and_then(|(_, source)| source.split_once("fn build_tab_context_menu"))
        .map(|(source, _)| source)
        .expect("应当可以定位单个 Feature 标签实现");
    assert!(tab.contains(".prefix(feature_icon(route.icon()))"));

    let prefix = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_prefix")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_suffix"))
        .map(|(source, _)| source)
        .expect("应当可以定位标签栏导航前缀实现");
    for action in ["tabs-back", "tabs-forward", "tabs-reload"] {
        assert!(prefix.contains(action), "标签栏前缀缺少 {action}");
    }
    assert!(prefix.contains("workspace_toolbar_icon_button("));
    assert!(!prefix.contains(".xsmall()"));
    assert!(
        !prefix.contains(".when("),
        "刷新入口必须始终留在 prefix，不支持刷新时仅禁用"
    );
    assert!(prefix.contains("!= crate::FeatureReloadAvailability::Available"));

    let suffix = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_suffix")
        .and_then(|(_, source)| source.split_once("fn account_partition_id"))
        .map(|(source, _)| source)
        .expect("应当可以定位标签栏后缀实现");
    assert!(suffix.contains("open-feature-search"));
    assert!(suffix.contains("SearchMode::OpenPage"));
    assert!(suffix.contains("workspace_toolbar_icon_button("));
}

#[test]
fn global_search_trigger_matches_prototype_structure() {
    let global_bar = APPLICATION_SOURCE
        .split_once("fn render_global_title_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位全局顶栏实现");

    for required in [
        "nexora-global-search-trigger",
        ".w(px(420.0))",
        ".h(px(30.0))",
        ".rounded(px(8.0))",
        "Icon::new(IconName::Search)",
        "搜索或跳转到…",
        "ShortcutHint::binding_for_action(&OpenGlobalSearch, None, window)",
    ] {
        assert!(global_bar.contains(required), "全局搜索入口缺少 {required}");
    }
}

#[test]
fn global_search_shortcut_and_navigation_follow_registered_framework_actions() {
    let global_bar = APPLICATION_SOURCE
        .split_once("fn render_global_title_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位全局顶栏实现");
    assert!(!global_bar.contains("Keystroke::parse"));
    assert!(global_bar.contains("when_some(search_shortcut"));

    let shell = APPLICATION_SOURCE
        .split_once("struct ApplicationShell {")
        .and_then(|(_, source)| source.split_once("enum NavigationEntry"))
        .map(|(source, _)| source)
        .expect("应当可以定位 ApplicationShell 状态");
    assert!(shell.contains("focus_handle: gpui::FocusHandle"));

    let render = APPLICATION_SOURCE
        .split_once("impl Render for ApplicationShell")
        .map(|(_, source)| source)
        .expect("应当可以定位 ApplicationShell 渲染实现");
    assert!(render.contains(".track_focus(&self.focus_handle)"));
    assert!(render.contains(".on_action(cx.listener(|this, _: &OpenGlobalSearch"));
    assert!(!APPLICATION_SOURCE.contains("dispatch_global_search"));

    let search_item = APPLICATION_SOURCE
        .split_once("fn feature_search_item")
        .and_then(|(_, source)| source.split_once("pub(crate) fn record_search_history"))
        .map(|(source, _)| source)
        .expect("应当可以定位框架页面搜索项实现");
    assert!(search_item.contains("cx.navigate(path)"));
    assert!(!search_item.contains("shell.update_in"));

    let open_search = APPLICATION_SOURCE
        .split_once("fn open_search(")
        .and_then(|(_, source)| source.split_once("fn render_global_title_bar_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位全局搜索打开逻辑");
    assert!(open_search.contains("window.has_active_dialog(cx)"));
    assert!(open_search.contains("return;"));
}

#[gpui::test]
fn shell_fallback_focus_dispatches_double_shift_without_a_focused_feature(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new("shift shift", OpenGlobalSearch, None)]);
    });
    let invocations = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed_invocations = invocations.clone();
    let (_view, cx) = cx.add_window_view(|window, cx| {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        FocusedShortcutHost {
            focus_handle,
            invocations: observed_invocations,
        }
    });

    cx.simulate_modifiers_change(Modifiers::shift());
    cx.simulate_modifiers_change(Modifiers::none());
    assert_eq!(invocations.get(), 0);

    cx.simulate_modifiers_change(Modifiers::shift());
    cx.simulate_modifiers_change(Modifiers::none());
    assert_eq!(invocations.get(), 1);
}

#[gpui::test]
fn global_search_kbd_exists_only_after_downstream_registers_a_binding(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let window = cx.add_window(|_, _| Empty);

    let without_binding = window
        .update(cx, |_, window, _| {
            ShortcutHint::binding_for_action(&OpenGlobalSearch, None, window)
        })
        .unwrap();
    assert!(without_binding.is_none());

    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new("shift shift", OpenGlobalSearch, None)]);
    });
    let with_binding = window
        .update(cx, |_, window, _| {
            ShortcutHint::binding_for_action(&OpenGlobalSearch, None, window)
        })
        .unwrap();
    let with_binding = with_binding.expect("下游注册后应显示快捷键提示");
    assert_eq!(with_binding.keystrokes().len(), 2);
    assert!(
        with_binding
            .keystrokes()
            .iter()
            .all(|keystroke| keystroke.key == "shift")
    );
}

#[test]
fn shell_toolbar_exposes_container_padding_and_uses_compact_icons() {
    assert_eq!(ShellToolbarOptions::new().right_padding_value(), px(12.0));
    assert_eq!(
        ShellToolbarOptions::new()
            .right_padding(px(20.0))
            .right_padding_value(),
        px(20.0)
    );

    let toolbar = APPLICATION_SOURCE
        .split_once("pub struct ShellToolbarAction")
        .and_then(|(_, source)| source.split_once("pub(crate) struct ApplicationBranding"))
        .map(|(source, _)| source)
        .expect("应当可以定位 ShellToolbarAction 实现");
    assert!(toolbar.contains("pub fn icon_button"));
    assert!(toolbar.contains("workspace_toolbar_icon_button("));
    assert!(toolbar.contains("right_padding"));

    let icon_button = APPLICATION_SOURCE
        .split_once("fn workspace_toolbar_icon_button(")
        .and_then(|(_, source)| source.split_once("fn workspace_sidebar_footer_host"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Shell 工具栏图标按钮实现");
    assert!(icon_button.contains("COMPONENT_SIZE_FOR_SIXTEEN_PIXEL_ICON"));
    assert!(icon_button.contains("workspace_icon_button_with_component_size("));
    assert!(icon_button.contains(".with_size(component_size)"));
    assert!(icon_button.contains(".size_8()"));

    let global_bar = APPLICATION_SOURCE
        .split_once("fn render_global_title_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位全局顶栏实现");
    assert!(global_bar.contains(".pr(toolbar.right_padding)"));
}
