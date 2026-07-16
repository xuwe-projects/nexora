use gpui::{div, px};
use ui::layout::WorkspaceLayout;

#[test]
fn workspace_layout_uses_default_content_padding() {
    let layout = WorkspaceLayout::new(div(), div(), div());

    assert_eq!(layout.content_padding(), px(24.0));
    assert!(layout.content_scrollable());
}

#[test]
fn workspace_layout_allows_overriding_content_padding() {
    let layout = WorkspaceLayout::new(div(), div(), div()).with_content_padding(px(12.0));

    assert_eq!(layout.content_padding(), px(12.0));
}

#[test]
fn workspace_layout_allows_disabling_outer_content_scroll() {
    let layout = WorkspaceLayout::new(div(), div(), div()).with_content_scrollable(false);

    assert!(!layout.content_scrollable());
}

#[test]
fn workspace_layout_accepts_a_shared_panel_header() {
    let layout = WorkspaceLayout::new(div(), div(), div()).with_panel_header(div());

    assert!(layout.has_panel_header());
}

#[test]
fn workspace_layout_accepts_a_panel_scoped_overlay() {
    let layout = WorkspaceLayout::new(div(), div(), div()).with_panel_overlay(div());

    assert!(layout.has_panel_overlay());
}

#[test]
fn workspace_layout_uses_resizable_sidebar_width_limits_by_default() {
    let layout = WorkspaceLayout::new(div(), div(), div());

    assert_eq!(layout.sidebar_width(), px(248.0));
    assert_eq!(layout.sidebar_min_width(), px(208.0));
    assert_eq!(layout.sidebar_max_width(), px(360.0));
}

#[test]
fn workspace_layout_allows_overriding_resizable_sidebar_width_limits() {
    let layout = WorkspaceLayout::new(div(), div(), div())
        .with_sidebar_width(px(280.0))
        .with_sidebar_width_range(px(220.0)..px(420.0));

    assert_eq!(layout.sidebar_width(), px(280.0));
    assert_eq!(layout.sidebar_min_width(), px(220.0));
    assert_eq!(layout.sidebar_max_width(), px(420.0));
}
