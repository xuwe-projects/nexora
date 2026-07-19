use gpui::{TextAlign, div};
use ui::{CrudPanel, CrudPanelToolbar, TableCell, TableCellVerticalAlign, TableHeaderCell};

#[test]
fn toolbar_is_empty_by_default() {
    assert!(CrudPanelToolbar::new().is_empty());
}

#[test]
fn toolbar_is_not_empty_after_adding_filters_or_actions() {
    assert!(!CrudPanelToolbar::new().filter(div()).is_empty());
    assert!(!CrudPanelToolbar::new().action(div()).is_empty());
}

#[test]
fn crud_panel_omits_empty_toolbar() {
    assert!(!CrudPanel::new("城市", div()).has_toolbar());
    assert!(CrudPanel::new("城市", div()).action(div()).has_toolbar());
}

#[test]
fn table_header_cell_is_centered_by_default_and_customizable() {
    assert_eq!(TableHeaderCell::new("状态").alignment(), TextAlign::Center);
    assert_eq!(
        TableHeaderCell::new("名称").left().alignment(),
        TextAlign::Left
    );
    assert_eq!(
        TableHeaderCell::new("金额").right().alignment(),
        TextAlign::Right
    );
}

#[test]
fn table_cell_is_left_and_vertically_centered_by_default_and_customizable() {
    let default_cell = TableCell::new(div());
    assert_eq!(default_cell.horizontal_alignment(), TextAlign::Left);
    assert_eq!(
        default_cell.vertical_alignment(),
        TableCellVerticalAlign::Center
    );
    assert_eq!(
        TableCell::new(div()).center().horizontal_alignment(),
        TextAlign::Center
    );
    assert_eq!(
        TableCell::new(div()).bottom().vertical_alignment(),
        TableCellVerticalAlign::Bottom
    );
}
