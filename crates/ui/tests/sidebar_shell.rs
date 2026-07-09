use gpui::{div, px};
use ui::layout::SidebarShell;

#[test]
fn sidebar_shell_uses_console_content_padding_by_default() {
    let shell = SidebarShell::new(div(), div(), div());

    assert_eq!(shell.content_padding(), px(24.0));
    assert!(shell.content_scrollable());
}

#[test]
fn sidebar_shell_allows_overriding_content_padding() {
    let shell = SidebarShell::new(div(), div(), div()).with_content_padding(px(12.0));

    assert_eq!(shell.content_padding(), px(12.0));
}

#[test]
fn sidebar_shell_allows_disabling_outer_content_scroll() {
    let shell = SidebarShell::new(div(), div(), div()).with_content_scrollable(false);

    assert!(!shell.content_scrollable());
}

#[test]
fn sidebar_shell_uses_resizable_sidebar_width_limits_by_default() {
    let shell = SidebarShell::new(div(), div(), div());

    assert_eq!(shell.sidebar_width(), px(248.0));
    assert_eq!(shell.sidebar_min_width(), px(208.0));
    assert_eq!(shell.sidebar_max_width(), px(360.0));
}

#[test]
fn sidebar_shell_allows_overriding_resizable_sidebar_width_limits() {
    let shell = SidebarShell::new(div(), div(), div())
        .with_sidebar_width(px(280.0))
        .with_sidebar_width_range(px(220.0)..px(420.0));

    assert_eq!(shell.sidebar_width(), px(280.0));
    assert_eq!(shell.sidebar_min_width(), px(220.0));
    assert_eq!(shell.sidebar_max_width(), px(420.0));
}
