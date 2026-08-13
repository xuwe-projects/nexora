use contracts::{crud_query::CrudQuery, pagination::PageQuery};
use gpui::{App, IntoElement as _, ParentElement as _, SharedString, TextAlign, Window, div};
use gpui_component::{form::field, table::Column};
use serde::Serialize;
use serde_json::Value;
use ui::{CrudPanel, CrudTableRow, TableCell, TableCellVerticalAlign, TableHeaderCell};

#[derive(Clone)]
struct TestRow {
    id: u64,
}

impl CrudTableRow for TestRow {
    type Id = u64;
    type Sort = contracts::crud_query::NoCrudSort;

    fn row_id(&self) -> &Self::Id {
        &self.id
    }

    fn columns() -> Vec<Column> {
        vec![Column::new("id", "ID")]
    }

    fn render_cell(&self, _: &str, _: &mut Window, _: &mut App) -> gpui::AnyElement {
        div().child(self.id.to_string()).into_any_element()
    }

    fn cell_text(&self, _: &str, _: &App) -> String {
        self.id.to_string()
    }
}

#[derive(Clone, Serialize)]
struct TestQuery {
    #[serde(flatten)]
    page: PageQuery,
}

impl CrudQuery for TestQuery {
    type Sort = contracts::crud_query::NoCrudSort;

    fn pagination(&self) -> &PageQuery {
        &self.page
    }

    fn pagination_mut(&mut self) -> &mut PageQuery {
        &mut self.page
    }

    fn sort(&self) -> Option<&Self::Sort> {
        None
    }

    fn set_sort(&mut self, _: Option<Self::Sort>) {}

    fn metadata() -> &'static contracts::crud_query::CrudQueryMetadata {
        static METADATA: contracts::crud_query::CrudQueryMetadata =
            contracts::crud_query::CrudQueryMetadata {
                page_size: contracts::crud_query::CrudPageSizeMetadata {
                    default: 25,
                    min: 15,
                    max: 100,
                    options: &[15, 25, 50, 100],
                },
                filters: &[],
                sort_field: None,
            };
        &METADATA
    }

    fn filter_value(&self, _: &str) -> Option<Value> {
        None
    }

    fn set_filter_value(&mut self, _: &str, _: Value) -> Result<(), String> {
        Err("未知筛选字段".to_owned())
    }
}

#[test]
fn crud_panel_type_contract_accepts_row_and_query() {
    fn assert_panel_type<R, Q>()
    where
        R: CrudTableRow,
        Q: CrudQuery<Sort = R::Sort>,
    {
        let _ = std::any::type_name::<CrudPanel<R, Q>>();
    }

    assert_panel_type::<TestRow, TestQuery>();
}

#[test]
fn official_field_type_is_the_only_filter_input() {
    let _: gpui_component::form::Field = field().label(SharedString::from("关键词"));
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
