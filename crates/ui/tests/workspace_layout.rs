use gpui::{div, px};
use ui::layout::WorkspaceLayout;

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
