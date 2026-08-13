use std::{cell::RefCell, rc::Rc};

use contracts::{crud_query::CrudQuery, pagination::PageQuery};
use gpui::{
    App, AppContext as _, Context, Empty, IntoElement as _, ParentElement as _, Render,
    TestAppContext, Window, div,
};
use gpui_component::table::Column;
use serde::Serialize;
use serde_json::{Value, json};
use ui::{CrudListState, CrudLoadError, CrudPage, CrudTableRow};

#[derive(Clone)]
struct TestRow {
    id: u64,
}

impl CrudTableRow for TestRow {
    type Id = u64;

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
    keyword: Option<String>,
}

impl TestQuery {
    fn new(page: u32) -> Self {
        Self {
            page: PageQuery {
                page,
                page_size: 15,
            },
            keyword: None,
        }
    }
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
        static FILTERS: &[contracts::crud_query::CrudFilterMetadata] =
            &[contracts::crud_query::CrudFilterMetadata {
                name: "keyword",
                label: "关键词",
                description: None,
                placeholder: None,
                control: contracts::crud_query::CrudFilterControl::Input,
                presentation: contracts::crud_query::CrudFilterPresentation::Form,
                trigger: contracts::crud_query::CrudFilterTrigger::Manual,
                required: false,
                required_message: None,
                pattern: None,
                pattern_message: None,
                parse_error: None,
                width: None,
            }];
        static METADATA: contracts::crud_query::CrudQueryMetadata =
            contracts::crud_query::CrudQueryMetadata {
                page_size: contracts::crud_query::CrudPageSizeMetadata {
                    default: 15,
                    min: 15,
                    max: 100,
                    options: &[15, 25, 50, 100],
                },
                filters: FILTERS,
                sort_field: None,
            };
        &METADATA
    }

    fn filter_value(&self, name: &str) -> Option<Value> {
        (name == "keyword").then(|| serde_json::to_value(&self.keyword).unwrap())
    }

    fn set_filter_value(&mut self, name: &str, value: Value) -> Result<(), String> {
        if name != "keyword" {
            return Err("未知筛选字段".to_owned());
        }
        self.keyword = serde_json::from_value(value).map_err(|_| "筛选值无效".to_owned())?;
        Ok(())
    }
}

struct TestRoot {
    list: gpui::Entity<CrudListState<TestRow, TestQuery>>,
}

impl Render for TestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }
}

fn add_list(
    cx: &mut TestAppContext,
    query: TestQuery,
    requests: Rc<RefCell<Vec<TestQuery>>>,
) -> gpui::Entity<CrudListState<TestRow, TestQuery>> {
    let root = cx.add_window(|window, cx| {
        let list = CrudListState::create(
            query,
            move |query| {
                requests.borrow_mut().push(query.clone());
                async move {
                    if query.keyword.as_deref() == Some("error") || query.page.page == 3 {
                        return Err(CrudLoadError::retryable("加载失败"));
                    }
                    let first_id = u64::from(query.page.page) * 100;
                    Ok(CrudPage::new(
                        vec![TestRow { id: first_id }],
                        query.page.page,
                        query.page.page_size,
                        100,
                    ))
                }
            },
            window,
            cx,
        )
        .unwrap();
        TestRoot { list }
    });
    root.read_with(cx, |root, _| root.list.clone()).unwrap()
}

#[gpui::test]
fn initial_page_and_cache_hit_do_not_reload(cx: &mut TestAppContext) {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let list = add_list(cx, TestQuery::new(5), requests.clone());

    cx.update_entity(&list, CrudListState::load_current);
    cx.run_until_parked();
    cx.update_entity(&list, |list, cx| list.go_to_page(4, cx));
    cx.run_until_parked();
    cx.update_entity(&list, |list, cx| list.go_to_page(5, cx));
    cx.run_until_parked();

    assert_eq!(
        requests
            .borrow()
            .iter()
            .map(|query| query.page.page)
            .collect::<Vec<_>>(),
        [5, 4]
    );
    assert_eq!(list.read_with(cx, |list, _| list.current_page()), 5);
}

#[gpui::test]
fn failed_jump_keeps_current_page_and_retry_uses_failed_query(cx: &mut TestAppContext) {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let list = add_list(cx, TestQuery::new(2), requests.clone());
    cx.update_entity(&list, CrudListState::load_current);
    cx.run_until_parked();

    cx.update_entity(&list, |list, cx| list.go_to_page(3, cx));
    cx.run_until_parked();
    assert_eq!(list.read_with(cx, |list, _| list.current_page()), 2);
    assert!(list.read_with(cx, |list, _| list.visible_error().is_some()));

    cx.update_entity(&list, CrudListState::retry_visible);
    cx.run_until_parked();
    assert_eq!(
        requests.borrow().last().map(|query| query.page.page),
        Some(3)
    );
}

#[gpui::test]
fn changing_filter_invalidates_cache_and_returns_to_first_page(cx: &mut TestAppContext) {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let list = add_list(cx, TestQuery::new(4), requests);
    cx.update_entity(&list, CrudListState::load_current);
    cx.run_until_parked();

    cx.update_entity(&list, |list, cx| {
        list.set_filter_value("keyword", json!("new"), cx).unwrap();
    });

    list.read_with(cx, |list, _| {
        assert_eq!(list.current_page(), 1);
        assert!(!list.has_current_page());
        assert!(!list.loaded_once());
    });
}
