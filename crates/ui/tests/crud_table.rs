use std::{
    fmt::{self, Display},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use gpui::{
    App, Context, Entity, IntoElement, Modifiers, Render, TestAppContext, Window, div, prelude::*,
    px,
};
use gpui_component::{
    Sizable as _,
    table::{Column, DataTable, TableDelegate as _, TableEvent, TableState},
};
use ui::{
    CrudTableDelegate, CrudTableRow, CrudTableSelection, LoadedRowsSelectionEvent,
    RowSelectionEvent, TableCell,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestRow {
    id: u64,
    name: String,
    enabled: bool,
}

impl TestRow {
    fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            enabled: true,
        }
    }

    fn disabled(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            enabled: false,
        }
    }
}

impl CrudTableRow for TestRow {
    type Id = u64;
    type Sort = contracts::crud_query::NoCrudSort;

    fn row_id(&self) -> &Self::Id {
        &self.id
    }

    fn columns() -> Vec<Column> {
        vec![Column::new("name", "名称").width(px(160.))]
    }

    fn render_cell(&self, key: &str, _window: &mut Window, _cx: &mut App) -> gpui::AnyElement {
        match key {
            "name" => TableCell::new(self.name.clone()).into_any_element(),
            _ => gpui::Empty.into_any_element(),
        }
    }

    fn cell_text(&self, key: &str, _cx: &App) -> String {
        match key {
            "name" => self.name.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq)]
struct DisplayOnlyId {
    value: u64,
    label: &'static str,
}

impl PartialEq for DisplayOnlyId {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Hash for DisplayOnlyId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl Display for DisplayOnlyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label)
    }
}

#[derive(Clone)]
struct DisplayIdRow {
    id: DisplayOnlyId,
    name: String,
}

impl CrudTableRow for DisplayIdRow {
    type Id = DisplayOnlyId;
    type Sort = contracts::crud_query::NoCrudSort;

    fn row_id(&self) -> &Self::Id {
        &self.id
    }

    fn columns() -> Vec<Column> {
        vec![Column::new("name", "名称")]
    }

    fn render_cell(&self, _key: &str, _window: &mut Window, _cx: &mut App) -> gpui::AnyElement {
        TableCell::new(self.name.clone()).into_any_element()
    }

    fn cell_text(&self, _key: &str, _cx: &App) -> String {
        self.name.clone()
    }
}

#[test]
fn selection_column_is_only_added_when_selection_is_enabled() {
    let delegate = CrudTableDelegate::new(vec![TestRow::new(1, "北京")]);
    assert!(!delegate.selection_enabled());
    assert_eq!(delegate.columns().len(), 1);
    assert_eq!(delegate.columns()[0].key.as_ref(), "name");

    let delegate = CrudTableDelegate::new(vec![TestRow::new(1, "北京")]).selection(
        CrudTableSelection::new(Vec::<u64>::new(), |_, _, _| {}, |_, _, _| {}),
    );

    assert!(delegate.selection_enabled());
    assert_eq!(delegate.columns().len(), 2);
    let selection = &delegate.columns()[0];
    assert_eq!(
        selection.key.as_ref(),
        CrudTableDelegate::<TestRow>::selection_column_key()
    );
    assert_eq!(delegate.columns()[1].key.as_ref(), "name");
    assert!(selection.fixed.is_some());
    assert!(!selection.resizable);
    assert!(!selection.movable);
    assert!(!selection.selectable);
}

#[gpui::test]
fn action_column_order_and_selection_cell_text_are_preserved(cx: &mut TestAppContext) {
    let delegate = CrudTableDelegate::new(vec![TestRow::new(1, "北京")])
        .action_column(Column::new("actions", "操作"), |_row, _window, _cx| {
            div().child("编辑")
        })
        .action_text(|_, _| "编辑".to_owned())
        .selection(CrudTableSelection::new(
            Vec::<u64>::new(),
            |_, _, _| {},
            |_, _, _| {},
        ));
    assert_eq!(
        delegate.columns()[0].key.as_ref(),
        "__nexora_crud_table_selection"
    );
    assert_eq!(delegate.columns()[1].key.as_ref(), "name");
    assert_eq!(delegate.columns()[2].key.as_ref(), "actions");
    cx.update(|app| {
        assert_eq!(delegate.cell_text(0, 0, app), "");
        assert_eq!(delegate.cell_text(0, 1, app), "北京");
        assert_eq!(delegate.cell_text(0, 2, app), "编辑");
    });
}

#[test]
#[should_panic(expected = "重复业务 ID")]
fn duplicate_business_ids_panic() {
    let _ = CrudTableDelegate::new(vec![TestRow::new(1, "北京"), TestRow::new(1, "上海")]);
}

#[test]
#[should_panic(expected = "重复业务 ID Display 文本")]
fn duplicate_business_id_display_texts_panic() {
    let _ = CrudTableDelegate::new(vec![
        DisplayIdRow {
            id: DisplayOnlyId {
                value: 1,
                label: "same",
            },
            name: "北京".to_owned(),
        },
        DisplayIdRow {
            id: DisplayOnlyId {
                value: 2,
                label: "same",
            },
            name: "上海".to_owned(),
        },
    ]);
}

#[test]
#[should_panic(expected = "不能增加、删除或修改业务 ID")]
fn update_rows_rejects_identity_changes() {
    let mut delegate = CrudTableDelegate::new(vec![TestRow::new(1, "北京")]);

    delegate.update_rows(|rows| rows[0].id = 2);
}

#[test]
fn update_rows_allows_reordering_without_identity_changes() {
    let mut delegate =
        CrudTableDelegate::new(vec![TestRow::new(1, "北京"), TestRow::new(2, "上海")]);

    delegate.update_rows(|rows| rows.swap(0, 1));

    assert_eq!(delegate.rows()[0].id, 2);
    assert_eq!(delegate.rows()[1].id, 1);
}

#[test]
#[should_panic(expected = "只能在启用 selection 后调用")]
fn set_selected_ids_without_selection_panics() {
    CrudTableDelegate::new(vec![TestRow::new(1, "北京")]).set_selected_ids([1]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RowEventLog {
    row_id: u64,
    row_name: String,
    selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BatchEventLog {
    row_ids: Vec<u64>,
    row_names: Vec<String>,
    selected: bool,
}

type RowEvents = Arc<Mutex<Vec<RowEventLog>>>;
type BatchEvents = Arc<Mutex<Vec<BatchEventLog>>>;
type NativeEvents = Arc<Mutex<Vec<TableEvent>>>;

struct CrudTableTestRoot {
    state: Entity<TableState<CrudTableDelegate<TestRow>>>,
}

impl CrudTableTestRoot {
    fn new(
        rows: Vec<TestRow>,
        selected_ids: Vec<u64>,
        row_events: RowEvents,
        batch_events: BatchEvents,
        native_events: NativeEvents,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let row_events_for_callback = row_events.clone();
        let batch_events_for_callback = batch_events.clone();
        let selection = CrudTableSelection::new(
            selected_ids,
            move |event: RowSelectionEvent<TestRow>, _, _| {
                row_events_for_callback
                    .lock()
                    .expect("行事件日志锁不应中毒")
                    .push(RowEventLog {
                        row_id: event.row_id,
                        row_name: event.row.name,
                        selected: event.selected,
                    });
            },
            move |event: LoadedRowsSelectionEvent<TestRow>, _, _| {
                batch_events_for_callback
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .push(BatchEventLog {
                        row_ids: event.row_ids,
                        row_names: event.rows.into_iter().map(|row| row.name).collect(),
                        selected: event.selected,
                    });
            },
        )
        .row_selectable(|row, _| row.enabled);

        let delegate = CrudTableDelegate::new(rows).selection(selection);
        let state = cx.new(|cx| TableState::new(delegate, window, cx));
        cx.subscribe(&state, move |_, _, event, _| {
            native_events
                .lock()
                .expect("原生事件日志锁不应中毒")
                .push(event.clone());
        })
        .detach();

        Self { state }
    }
}

impl Render for CrudTableTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("crud-table-test-host")
            .debug_selector(|| "crud-table-test-host".into())
            .size(px(420.))
            .child(
                DataTable::new(&self.state)
                    .bordered(true)
                    .with_size(px(36.)),
            )
    }
}

fn with_table_fixture(
    cx: &mut TestAppContext,
    rows: Vec<TestRow>,
    selected_ids: Vec<u64>,
    run: impl FnOnce(RowEvents, BatchEvents, NativeEvents, &mut gpui::VisualTestContext),
) {
    cx.update(gpui_component::init);
    let row_events = Arc::new(Mutex::new(Vec::new()));
    let batch_events = Arc::new(Mutex::new(Vec::new()));
    let native_events = Arc::new(Mutex::new(Vec::new()));
    let row_events_for_view = row_events.clone();
    let batch_events_for_view = batch_events.clone();
    let native_events_for_view = native_events.clone();
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        CrudTableTestRoot::new(
            rows,
            selected_ids,
            row_events_for_view,
            batch_events_for_view,
            native_events_for_view,
            window,
            cx,
        )
    });
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    run(row_events, batch_events, native_events, cx);
}

#[gpui::test]
fn sparse_rows_use_total_as_logical_count_and_keep_current_page_selection_rows(
    cx: &mut TestAppContext,
) {
    let current = vec![TestRow::new(21, "第二页")];
    let delegate = CrudTableDelegate::new(Vec::new()).selection(CrudTableSelection::new(
        Vec::new(),
        |_, _, _| {},
        |_, _, _| {},
    ));
    let mut delegate = delegate;
    delegate.replace_sparse_rows(
        40,
        current,
        [
            (0, TestRow::new(1, "第一页")),
            (20, TestRow::new(21, "第二页")),
        ],
    );

    assert_eq!(cx.read(|cx| delegate.rows_count(cx)), 40);
    assert_eq!(delegate.rows().len(), 1);
    assert_eq!(delegate.rows()[0].id, 21);
}

#[gpui::test]
fn row_checkbox_dispatches_single_row_selection_event(cx: &mut TestAppContext) {
    with_table_fixture(
        cx,
        vec![TestRow::new(1, "北京")],
        Vec::new(),
        |row_events, batch_events, native_events, cx| {
            let checkbox = cx
                .debug_bounds("crud-table-row-select-1")
                .expect("行选择复选框应当渲染");
            cx.simulate_click(checkbox.center(), Modifiers::none());

            assert_eq!(
                row_events.lock().expect("行事件日志锁不应中毒").as_slice(),
                [RowEventLog {
                    row_id: 1,
                    row_name: "北京".to_owned(),
                    selected: true,
                }]
            );
            assert!(
                batch_events
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .is_empty()
            );
            assert!(
                native_events
                    .lock()
                    .expect("原生事件日志锁不应中毒")
                    .is_empty()
            );
        },
    );
}

#[gpui::test]
fn row_checkbox_dispatches_single_row_deselection_event(cx: &mut TestAppContext) {
    with_table_fixture(
        cx,
        vec![TestRow::new(1, "北京")],
        vec![1],
        |row_events, batch_events, _, cx| {
            let checkbox = cx
                .debug_bounds("crud-table-row-select-1")
                .expect("行选择复选框应当渲染");
            cx.simulate_click(checkbox.center(), Modifiers::none());

            assert_eq!(
                row_events.lock().expect("行事件日志锁不应中毒").as_slice(),
                [RowEventLog {
                    row_id: 1,
                    row_name: "北京".to_owned(),
                    selected: false,
                }]
            );
            assert!(
                batch_events
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .is_empty()
            );
        },
    );
}

#[gpui::test]
fn header_checkbox_dispatches_loaded_rows_selection_event(cx: &mut TestAppContext) {
    with_table_fixture(
        cx,
        vec![
            TestRow::new(1, "北京"),
            TestRow::disabled(2, "上海"),
            TestRow::new(3, "深圳"),
        ],
        vec![99],
        |row_events, batch_events, _, cx| {
            let checkbox = cx
                .debug_bounds("crud-table-loaded-rows-select")
                .expect("表头选择复选框应当渲染");
            cx.simulate_click(checkbox.center(), Modifiers::none());

            assert!(row_events.lock().expect("行事件日志锁不应中毒").is_empty());
            assert_eq!(
                batch_events
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .as_slice(),
                [BatchEventLog {
                    row_ids: vec![1, 3],
                    row_names: vec!["北京".to_owned(), "深圳".to_owned()],
                    selected: true,
                }]
            );
        },
    );
}

#[gpui::test]
fn header_checkbox_dispatches_loaded_rows_deselection_event(cx: &mut TestAppContext) {
    with_table_fixture(
        cx,
        vec![TestRow::new(1, "北京"), TestRow::new(2, "上海")],
        vec![1, 2, 99],
        |_, batch_events, _, cx| {
            let checkbox = cx
                .debug_bounds("crud-table-loaded-rows-select")
                .expect("表头选择复选框应当渲染");
            cx.simulate_click(checkbox.center(), Modifiers::none());

            assert_eq!(
                batch_events
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .as_slice(),
                [BatchEventLog {
                    row_ids: vec![1, 2],
                    row_names: vec!["北京".to_owned(), "上海".to_owned()],
                    selected: false,
                }]
            );
        },
    );
}

#[gpui::test]
fn disabled_rows_do_not_dispatch_selection_events(cx: &mut TestAppContext) {
    with_table_fixture(
        cx,
        vec![TestRow::disabled(2, "上海")],
        vec![2],
        |row_events, batch_events, _, cx| {
            let row_checkbox = cx
                .debug_bounds("crud-table-row-select-2")
                .expect("禁用行选择复选框仍应渲染");
            cx.simulate_click(row_checkbox.center(), Modifiers::none());
            let header_checkbox = cx
                .debug_bounds("crud-table-loaded-rows-select")
                .expect("无可选行时表头复选框仍应渲染");
            cx.simulate_click(header_checkbox.center(), Modifiers::none());

            assert!(row_events.lock().expect("行事件日志锁不应中毒").is_empty());
            assert!(
                batch_events
                    .lock()
                    .expect("批量事件日志锁不应中毒")
                    .is_empty()
            );
        },
    );
}
