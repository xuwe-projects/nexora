use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px};
use ui::layout::{
    WORKSPACE_GLOBAL_BAR_HEIGHT, WORKSPACE_SHELL_ICON_SIZE, WORKSPACE_SIDEBAR_COLLAPSED_WIDTH,
    WORKSPACE_SIDEBAR_EXPANDED_WIDTH, WORKSPACE_TAB_BAR_HEIGHT, WorkspaceLayout,
};

#[test]
fn workspace_layout_uses_prototype_shell_bar_heights() {
    assert_eq!(WORKSPACE_GLOBAL_BAR_HEIGHT, px(44.0));
    assert_eq!(WORKSPACE_TAB_BAR_HEIGHT, px(42.0));
}

#[test]
fn workspace_layout_uses_shared_sidebar_widths() {
    assert_eq!(WORKSPACE_SIDEBAR_EXPANDED_WIDTH, px(236.0));
    assert_eq!(WORKSPACE_SIDEBAR_COLLAPSED_WIDTH, px(80.0));
    assert_eq!(WORKSPACE_SHELL_ICON_SIZE, px(20.0));
}

#[test]
fn workspace_layout_uses_default_content_padding() {
    let layout = WorkspaceLayout::new(div(), div(), div(), div());

    assert_eq!(layout.content_padding(), px(24.0));
    assert!(layout.content_scrollable());
}

#[test]
fn workspace_layout_allows_overriding_content_padding() {
    let layout = WorkspaceLayout::new(div(), div(), div(), div()).with_content_padding(px(12.0));

    assert_eq!(layout.content_padding(), px(12.0));
}

#[test]
fn workspace_layout_allows_disabling_outer_content_scroll() {
    let layout = WorkspaceLayout::new(div(), div(), div(), div()).with_content_scrollable(false);

    assert!(!layout.content_scrollable());
}

#[test]
fn workspace_layout_accepts_a_panel_scoped_overlay() {
    let layout = WorkspaceLayout::new(div(), div(), div(), div()).with_panel_overlay(div());

    assert!(layout.has_panel_overlay());
}

struct WorkspaceLayoutHarness;

impl Render for WorkspaceLayoutHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        WorkspaceLayout::new(
            div()
                .debug_selector(|| "nexora-workspace-sidebar".into())
                .w(px(236.0))
                .h_full(),
            div(),
            div(),
            div(),
        )
        .with_sidebar_collapsed(false)
        .render(window, cx)
    }
}

struct CollapsedWorkspaceLayoutHarness;

impl Render for CollapsedWorkspaceLayoutHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        WorkspaceLayout::new(
            div()
                .debug_selector(|| "nexora-workspace-sidebar".into())
                .w(WORKSPACE_SIDEBAR_COLLAPSED_WIDTH)
                .h_full(),
            div(),
            div(),
            div(),
        )
        .with_sidebar_collapsed(true)
        .render(window, cx)
    }
}

struct AnimatedWorkspaceLayoutHarness {
    collapsed: bool,
}

impl Render for AnimatedWorkspaceLayoutHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar_width = if self.collapsed {
            WORKSPACE_SIDEBAR_COLLAPSED_WIDTH
        } else {
            WORKSPACE_SIDEBAR_EXPANDED_WIDTH
        };
        WorkspaceLayout::new(
            div()
                .debug_selector(|| "nexora-workspace-sidebar".into())
                .w(sidebar_width)
                .h_full(),
            div(),
            div(),
            div(),
        )
        .with_sidebar_collapsed(self.collapsed)
        .render(window, cx)
    }
}

#[gpui::test]
fn workspace_sidebar_reaches_top_and_bars_keep_fixed_vertical_order(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(|_, _| WorkspaceLayoutHarness);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let sidebar = cx
        .debug_bounds("nexora-workspace-sidebar")
        .expect("Sidebar 应当进入布局");
    let tab_bar = cx
        .debug_bounds("nexora-workspace-tab-bar")
        .expect("标签栏应当进入布局");
    let content = cx
        .debug_bounds("nexora-workspace-content")
        .expect("内容区应当进入布局");

    assert_eq!(sidebar.origin.y, px(0.0));
    assert_eq!(tab_bar.origin.y, WORKSPACE_GLOBAL_BAR_HEIGHT);
    assert_eq!(tab_bar.size.height, WORKSPACE_TAB_BAR_HEIGHT);
    assert_eq!(
        content.origin.y,
        WORKSPACE_GLOBAL_BAR_HEIGHT + WORKSPACE_TAB_BAR_HEIGHT
    );
}

#[gpui::test]
fn collapsed_sidebar_uses_the_full_shared_navigation_frame(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(|_, _| CollapsedWorkspaceLayoutHarness);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let frame = cx
        .debug_bounds("nexora-workspace-sidebar-frame")
        .expect("收起导航框架应当进入布局");
    let sidebar = cx
        .debug_bounds("nexora-workspace-sidebar")
        .expect("官方 Sidebar 应当进入收起导航框架");
    let tab_bar = cx
        .debug_bounds("nexora-workspace-tab-bar")
        .expect("标签栏应当进入布局");

    assert_eq!(frame.size.width, WORKSPACE_SIDEBAR_COLLAPSED_WIDTH);
    assert_eq!(sidebar.size.width, WORKSPACE_SIDEBAR_COLLAPSED_WIDTH);
    assert_eq!(sidebar.origin.x, frame.origin.x);
    assert_eq!(tab_bar.origin.x, WORKSPACE_SIDEBAR_COLLAPSED_WIDTH);
}

#[gpui::test]
fn sidebar_navigation_frame_tracks_collapsed_state_with_reduced_motion(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (root, cx) = cx.add_window_view(|_, _| AnimatedWorkspaceLayoutHarness { collapsed: false });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        cx.debug_bounds("nexora-workspace-sidebar-frame")
            .expect("展开导航框架应当进入布局")
            .size
            .width,
        WORKSPACE_SIDEBAR_EXPANDED_WIDTH
    );

    cx.update_entity(&root, |root, cx| {
        root.collapsed = true;
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        cx.debug_bounds("nexora-workspace-sidebar-frame")
            .expect("收起导航框架应当进入布局")
            .size
            .width,
        WORKSPACE_SIDEBAR_COLLAPSED_WIDTH
    );
}
