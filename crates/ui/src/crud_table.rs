//! 标准 CRUD 数据表增强能力。
//!
//! 本模块在 `gpui-component` 的 `DataTable`、`Column` 与 `TableDelegate` 之上提供薄封装：
//! 行数据可以通过 [`CrudTableRow`] 描述默认列、正文渲染与导出文本，调用方也可以继续直接
//! 实现原生 `TableDelegate`，不需要经过本模块。

use std::{collections::HashSet, fmt::Display, hash::Hash, rc::Rc};

use gpui::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, Styled as _, TextAlign, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    checkbox::Checkbox,
    table::{Column, ColumnSort, TableDelegate, TableState},
    v_flex,
};

use crate::{TableCellVerticalAlign, TableHeaderCell};

/// 描述一行 CRUD 表格数据如何转换成 gpui-component 表格列与单元格。
///
/// `#[derive(nexora::CrudTableRow)]` 可以为普通命名字段结构体生成该 trait 的实现；复杂交
/// 互、远程排序或自定义分组表头仍然可以绕过派生宏，直接实现本 trait 或原生
/// `gpui_component::table::TableDelegate`。
pub trait CrudTableRow: Clone + 'static {
    /// 当前行的稳定业务 ID 类型。
    ///
    /// 该类型保持业务原始形态，delegate 的选择事件和受控 selected IDs 均直接使用该类型。
    /// [`Display`] 只用于生成带命名空间的稳定 GPUI Element ID；同一批已加载行中，业务 ID
    /// 和其显示文本都必须唯一。
    type Id: Clone + Eq + Hash + Display + 'static;

    /// 返回当前行的稳定业务 ID。
    ///
    /// 该 ID 用于行 Element ID、选择列状态和选择事件。实现必须保证同一批已加载行中 ID
    /// 唯一，且不要在 [`CrudTableDelegate::update_rows`] 中修改身份字段。
    fn row_id(&self) -> &Self::Id;

    /// 返回当前行类型默认声明的业务数据列。
    ///
    /// 返回值沿用 gpui-component 的 [`Column`]，因此列宽、排序、固定列和选择行为都仍然
    /// 由官方组件解释。
    fn columns() -> Vec<Column>;

    /// 返回列 key 对应的稳定后台排序字段名。
    ///
    /// 不支持后台排序的列返回 `None`。派生宏会为声明 `sortable`、`ascending` 或
    /// `descending` 的列生成映射；业务可以用 `sort_field = "updated_at"` 显式覆盖字段名。
    fn backend_sort_field(_key: &str) -> Option<&'static str> {
        None
    }

    /// 返回指定列的表头水平对齐方式。
    ///
    /// 默认表头水平居中；派生宏会根据字段属性覆盖该值。垂直方向由 [`TableHeaderCell`]
    /// 固定为居中。
    fn header_alignment(_key: &str) -> TextAlign {
        TextAlign::Center
    }

    /// 返回指定列的正文水平对齐方式。
    ///
    /// 默认正文水平靠左；派生宏会根据字段属性覆盖该值。
    fn cell_alignment(_key: &str) -> TextAlign {
        TextAlign::Left
    }

    /// 返回指定列的正文垂直对齐方式。
    ///
    /// 默认正文垂直居中；派生宏会根据字段属性覆盖该值。
    fn cell_vertical_alignment(_key: &str) -> TableCellVerticalAlign {
        TableCellVerticalAlign::Center
    }

    /// 渲染指定列的正文单元格。
    ///
    /// `key` 来自 [`Column::key`]。实现应返回完整单元格内容；派生宏默认使用 [`crate::TableCell`]
    /// 包裹字段文本，复杂列可以通过字段属性指定自定义渲染函数。
    fn render_cell(&self, key: &str, window: &mut Window, cx: &mut App) -> AnyElement;

    /// 返回指定列的文本表示。
    ///
    /// 该值用于表格导出、复制或测试断言。复杂展示列应让文本与用户可见含义保持一致。
    fn cell_text(&self, key: &str, cx: &App) -> String;
}

type ActionRenderer<R> = Rc<dyn Fn(&R, &mut Window, &mut App) -> AnyElement>;
type ActionText<R> = Rc<dyn Fn(&R, &App) -> String>;
type LoadMoreHandler<R> = Rc<dyn Fn(&mut Window, &mut Context<TableState<CrudTableDelegate<R>>>)>;
type RowSelectionHandler<R> = Rc<dyn Fn(RowSelectionEvent<R>, &mut Window, &mut App)>;
type LoadedRowsSelectionHandler<R> = Rc<dyn Fn(LoadedRowsSelectionEvent<R>, &mut Window, &mut App)>;
type RowSelectable<R> = Rc<dyn Fn(&R, &App) -> bool>;
type SortChangedHandler = Rc<dyn Fn(Option<CrudTableSort>, &mut Window, &mut App)>;

const SELECTION_COLUMN_KEY: &str = "__nexora_crud_table_selection";
const SELECTION_COLUMN_WIDTH: f32 = 42.0;

/// CRUD 表格交给业务请求层的单列排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrudTableSortDirection {
    /// 后台字段按升序排列。
    Ascending,
    /// 后台字段按降序排列。
    Descending,
}

/// CRUD 表格当前选中的后台排序描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrudTableSort {
    /// DataTable 列定义使用的稳定列 key。
    pub column_key: String,
    /// 业务 API 使用的稳定后台字段名。
    pub backend_field: String,
    /// 当前单列排序方向。
    pub direction: CrudTableSortDirection,
}

/// 一列可持久化的顺序和宽度快照。
#[derive(Debug, Clone, PartialEq)]
pub struct CrudTableColumnState {
    /// DataTable 列定义使用的稳定列 key。
    pub key: String,
    /// 用户交互完成后的逻辑像素宽度。
    pub width: f32,
}

/// CRUD DataTable 可跨会话恢复的布局与后台排序快照。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrudTableState {
    /// 按当前显示顺序保存的已知列。
    pub columns: Vec<CrudTableColumnState>,
    /// 当前单列后台排序；`None` 表示使用业务默认顺序。
    pub sort: Option<CrudTableSort>,
}

struct CrudActionColumn<R: CrudTableRow> {
    column: Column,
    render: ActionRenderer<R>,
    text: Option<ActionText<R>>,
}

/// 单行选择复选框触发的受控选择事件。
///
/// 事件由 [`CrudTableDelegate`] 在用户点击标准选择列的行复选框时构造并派发。调用方只读取
/// 事件中的业务 ID、行快照和目标选中状态，随后更新自己持有的 selected IDs，并通过
/// [`CrudTableDelegate::set_selected_ids`] 回写新的渲染快照。
#[non_exhaustive]
#[derive(Clone)]
pub struct RowSelectionEvent<R: CrudTableRow> {
    /// 被点击行的稳定业务 ID，保持 [`CrudTableRow::Id`] 原始类型。
    pub row_id: R::Id,
    /// 点击发生时 delegate 中该行的完整克隆快照。
    pub row: R,
    /// 点击后的目标选中状态；`true` 表示请求选中，`false` 表示请求取消选中。
    pub selected: bool,
}

/// 表头选择复选框触发的当前已加载行批量选择事件。
///
/// “当前已加载行”只表示 [`CrudTableDelegate::rows`] 返回的行，不包含服务端尚未加载的数据，
/// 也不会根据 `total` 推断远端全集。禁用行不会进入该事件。调用方处理事件后仍负责回写
/// selected IDs 并通知对应 `TableState` 刷新。
#[non_exhaustive]
#[derive(Clone)]
pub struct LoadedRowsSelectionEvent<R: CrudTableRow> {
    /// 当前已加载且允许选择的行 ID，顺序与 [`Self::rows`] 一一对应。
    pub row_ids: Vec<R::Id>,
    /// 当前已加载且允许选择的完整行快照，顺序与 [`Self::row_ids`] 一一对应。
    pub rows: Vec<R>,
    /// 表头点击后的目标批量选中状态。
    pub selected: bool,
}

/// `CrudTableDelegate` 的标准受控行选择配置。
///
/// 该配置是启用选择列的唯一入口。调用方持有业务 selected IDs，本配置中的 selected IDs
/// 只是 delegate 用于渲染 checked 状态的快照；单行和表头点击只派发事件，不会直接修改该
/// 快照。调用方应在事件回调中更新自己的集合，再在对应 `TableState` 的更新上下文中调用
/// [`CrudTableDelegate::set_selected_ids`] 并通知刷新。
pub struct CrudTableSelection<R: CrudTableRow> {
    selected_ids: HashSet<R::Id>,
    on_select_row: RowSelectionHandler<R>,
    on_select_loaded_rows: LoadedRowsSelectionHandler<R>,
    row_selectable: RowSelectable<R>,
}

impl<R: CrudTableRow> CrudTableSelection<R> {
    /// 创建受控选择配置。
    ///
    /// `selected_ids` 可以是任意产生 `R::Id` 的集合或迭代器，重复 ID 会自动合并。两个回调
    /// 分别接收单行事件和当前已加载行批量事件，二者均按值接收事件快照。
    pub fn new(
        selected_ids: impl IntoIterator<Item = R::Id>,
        on_select_row: impl Fn(RowSelectionEvent<R>, &mut Window, &mut App) + 'static,
        on_select_loaded_rows: impl Fn(LoadedRowsSelectionEvent<R>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            selected_ids: selected_ids.into_iter().collect(),
            on_select_row: Rc::new(on_select_row),
            on_select_loaded_rows: Rc::new(on_select_loaded_rows),
            row_selectable: Rc::new(|_, _| true),
        }
    }

    /// 设置单行是否允许选择的谓词。
    ///
    /// 禁用行会显示 disabled 状态，不参与表头状态计算，也不会出现在批量选择事件中。已经
    /// 选中的禁用行仍保持 checked，delegate 不会自动删除调用方持有的 selected ID。
    #[must_use]
    pub fn row_selectable(mut self, row_selectable: impl Fn(&R, &App) -> bool + 'static) -> Self {
        self.row_selectable = Rc::new(row_selectable);
        self
    }
}

/// 可直接传给 gpui-component `TableState::new` 的标准 CRUD 表格 delegate。
///
/// 该 delegate 负责把 [`CrudTableRow`] 行数据接到原生 `TableDelegate`：默认表头使用
/// [`TableHeaderCell`] 居中，正文由行类型渲染，额外操作列通过 [`Self::action_column`]
/// 追加。需要分组表头、复杂选择状态或跨列编辑时，调用方仍可手写原生 `TableDelegate`。
pub struct CrudTableDelegate<R: CrudTableRow> {
    columns: Vec<Column>,
    rows: Vec<R>,
    total: usize,
    loading: bool,
    loading_more: bool,
    load_more: Option<LoadMoreHandler<R>>,
    selection: Option<CrudTableSelection<R>>,
    action_columns: Vec<CrudActionColumn<R>>,
    empty_title: SharedString,
    empty_description: Option<SharedString>,
    on_sort_changed: Option<SortChangedHandler>,
}

impl<R: CrudTableRow> CrudTableDelegate<R> {
    /// 使用一组初始行创建 delegate。
    ///
    /// 默认列来自 [`CrudTableRow::columns`]；`total` 默认为当前行数，不触发无限加载。
    ///
    /// # Panics
    ///
    /// 当初始行的业务 ID 重复、业务 ID 的 [`Display`] 文本重复，或业务列使用了 Nexora
    /// 内部保留选择列 key 时会 panic。
    pub fn new(rows: Vec<R>) -> Self {
        validate_loaded_row_ids(&rows);
        let columns = R::columns();
        validate_columns_do_not_use_reserved_key(&columns);
        let total = rows.len();
        Self {
            columns,
            rows,
            total,
            loading: false,
            loading_more: false,
            load_more: None,
            selection: None,
            action_columns: Vec::new(),
            empty_title: SharedString::new("暂无数据"),
            empty_description: None,
            on_sort_changed: None,
        }
    }

    /// 返回当前可见行数据。
    pub fn rows(&self) -> &[R] {
        &self.rows
    }

    /// 返回当前所有列定义，包含追加的操作列。
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// 返回当前列顺序、宽度与后台排序快照。
    pub fn persistent_state(&self) -> CrudTableState {
        CrudTableState {
            columns: self
                .columns
                .iter()
                .map(|column| CrudTableColumnState {
                    key: column.key.to_string(),
                    width: f32::from(column.width),
                })
                .collect(),
            sort: self.current_sort(),
        }
    }

    /// 按恢复规则把已保存状态合并进当前代码声明的列定义。
    ///
    /// 已删除列会忽略，新列保持声明顺序追加；宽度会限制到当前列的最小/最大值；已失效或
    /// 不可排序的字段不会恢复。调用方应在创建 `TableState` 前调用。
    pub fn restore_persistent_state(&mut self, state: &CrudTableState) {
        let mut remaining = std::mem::take(&mut self.columns);
        let mut restored = Vec::with_capacity(remaining.len());
        for saved in &state.columns {
            let Some(index) = remaining
                .iter()
                .position(|column| column.key.as_ref() == saved.key)
            else {
                continue;
            };
            let mut column = remaining.remove(index);
            if saved.width.is_finite() && saved.width > 0.0 {
                column.width = px(saved.width).clamp(column.min_width, column.max_width);
            }
            restored.push(column);
        }
        restored.extend(remaining);
        self.columns = restored;

        for column in &mut self.columns {
            if column.sort.is_some() {
                column.sort = Some(ColumnSort::Default);
            }
        }
        if let Some(sort) = &state.sort
            && let Some(column) = self.columns.iter_mut().find(|column| {
                column.key.as_ref() == sort.column_key
                    && column.sort.is_some()
                    && R::backend_sort_field(column.key.as_ref())
                        == Some(sort.backend_field.as_str())
            })
        {
            column.sort = Some(match sort.direction {
                CrudTableSortDirection::Ascending => ColumnSort::Ascending,
                CrudTableSortDirection::Descending => ColumnSort::Descending,
            });
        }
    }

    /// 把 gpui-component 在一次列宽拖动完成后报告的宽度同步回列定义。
    pub fn update_column_widths(&mut self, widths: &[gpui::Pixels]) {
        for (column, width) in self.columns.iter_mut().zip(widths.iter().copied()) {
            column.width = width.clamp(column.min_width, column.max_width);
        }
    }

    /// 安装后台排序变化回调。
    ///
    /// 表头点击会按降序、升序、清除排序循环调用。框架不修改当前页数据；业务应在回调中
    /// 回到第一页并携带 `backend_field` 重新请求服务端。
    #[must_use]
    pub fn on_sort_changed(
        mut self,
        handler: impl Fn(Option<CrudTableSort>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_sort_changed = Some(Rc::new(handler));
        self
    }

    /// 返回当前有效的后台排序选择。
    pub fn current_sort(&self) -> Option<CrudTableSort> {
        self.columns.iter().find_map(|column| {
            let direction = match column.sort? {
                ColumnSort::Ascending => CrudTableSortDirection::Ascending,
                ColumnSort::Descending => CrudTableSortDirection::Descending,
                ColumnSort::Default => return None,
            };
            Some(CrudTableSort {
                column_key: column.key.to_string(),
                backend_field: R::backend_sort_field(column.key.as_ref())?.to_owned(),
                direction,
            })
        })
    }

    /// 用新数据替换当前全部行。
    ///
    /// 该方法只更新 delegate 内部数据；调用方在 GPUI Entity 中修改后仍应通过对应
    /// `Context` 调用 `notify`。
    ///
    /// # Panics
    ///
    /// 当新行的业务 ID 重复，或业务 ID 的 [`Display`] 文本重复时会 panic。
    pub fn replace_rows(&mut self, rows: Vec<R>) {
        validate_loaded_row_ids(&rows);
        self.rows = rows;
        self.total = self.rows.len();
    }

    /// 追加一批行数据。
    ///
    /// # Panics
    ///
    /// 当追加后当前已加载行的业务 ID 重复，或业务 ID 的 [`Display`] 文本重复时会 panic。
    pub fn append_rows(&mut self, rows: impl IntoIterator<Item = R>) {
        self.rows.extend(rows);
        validate_loaded_row_ids(&self.rows);
        self.total = self.total.max(self.rows.len());
    }

    /// 在不改变行身份集合的前提下就地更新当前行数据。
    ///
    /// 该方法适合异步保存完成后修改展示字段、重新排序当前已加载行，或刷新权限相关的非身份
    /// 状态。需要增加、删除、替换身份字段或切换完整数据集合时，应使用
    /// [`Self::replace_rows`]。
    ///
    /// # Panics
    ///
    /// 当更新前已有非法重复 ID，更新后业务 ID 集合发生增加、删除或修改，或更新后 ID /
    /// [`Display`] 文本不唯一时会 panic。
    pub fn update_rows(&mut self, update: impl FnOnce(&mut [R])) {
        validate_loaded_row_ids(&self.rows);
        let before = row_id_set(&self.rows);
        update(&mut self.rows);
        validate_loaded_row_ids(&self.rows);
        let after = row_id_set(&self.rows);
        assert!(
            before == after,
            "CrudTableDelegate::update_rows 只能修改非身份字段或调整行顺序，不能增加、删除或修改业务 ID",
        );
    }

    /// 设置服务端或数据源报告的总行数。
    ///
    /// 当当前行数小于该值，且设置了 [`Self::on_load_more`]，滚动到底部时会触发加载更多。
    pub fn set_total(&mut self, total: usize) {
        self.total = total;
    }

    /// 设置表格是否处于加载状态。
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// 设置表格是否正在加载下一页数据。
    ///
    /// 该状态不会触发整张表格的 loading 视图，只用于暂停 `has_more`，避免滚动到底部时重
    /// 复触发加载更多。
    pub fn set_loading_more(&mut self, loading_more: bool) {
        self.loading_more = loading_more;
    }

    /// 启用标准受控行选择列。
    ///
    /// 选择列始终位于第一列，固定在左侧，并且不参与 DataTable 原生行、列或单元格选择。调用
    /// 方仍持有真实 selected IDs；delegate 只保存 [`CrudTableSelection`] 中传入的渲染快照。
    ///
    /// # Panics
    ///
    /// 当业务列或已追加操作列使用了 Nexora 内部保留选择列 key 时会 panic。
    #[must_use]
    pub fn selection(mut self, selection: CrudTableSelection<R>) -> Self {
        validate_columns_do_not_use_reserved_key(&self.columns);
        self.columns.insert(0, selection_column());
        self.selection = Some(selection);
        self
    }

    /// 替换 delegate 用于渲染 checked 状态的 selected IDs 快照。
    ///
    /// 该方法不会派发业务事件，也不会修改调用方集合。通常在处理
    /// [`RowSelectionEvent`] 或 [`LoadedRowsSelectionEvent`] 后，于对应 `TableState` 更新上下
    /// 文中调用，并随后通知刷新。
    ///
    /// # Panics
    ///
    /// 当当前 delegate 没有通过 [`Self::selection`] 启用选择能力时会 panic。
    pub fn set_selected_ids(&mut self, selected_ids: impl IntoIterator<Item = R::Id>) {
        let Some(selection) = &mut self.selection else {
            panic!("CrudTableDelegate::set_selected_ids 只能在启用 selection 后调用");
        };
        selection.selected_ids = selected_ids.into_iter().collect();
    }

    /// 返回是否已启用标准受控行选择列。
    pub fn selection_enabled(&self) -> bool {
        self.selection.is_some()
    }

    /// 返回当前可选择已加载行是否全部处于 checked 状态。
    ///
    /// 该状态只根据当前 [`Self::rows`] 和 `row_selectable` 谓词计算；禁用行和未加载 selected
    /// IDs 都不会参与。
    pub fn loaded_rows_checked(&self, cx: &App) -> bool {
        self.selectable_loaded_rows(cx)
            .is_some_and(|rows| rows.iter().all(|row| self.is_row_selected(row)))
    }

    /// 返回当前是否存在允许选择的已加载行。
    pub fn has_selectable_loaded_rows(&self, cx: &App) -> bool {
        self.selectable_loaded_rows(cx).is_some()
    }

    /// 返回指定行当前是否允许选择。
    pub fn row_selectable(&self, row: &R, cx: &App) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| (selection.row_selectable)(row, cx))
    }

    /// 返回指定行 ID 是否在当前受控 selected IDs 快照中。
    pub fn is_row_selected(&self, row: &R) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| selection.selected_ids.contains(row.row_id()))
    }

    /// 返回选择列使用的内部保留 key。
    ///
    /// 该方法主要用于测试和诊断；业务列和操作列不得使用该 key。
    pub fn selection_column_key() -> &'static str {
        SELECTION_COLUMN_KEY
    }

    #[must_use]
    fn selectable_loaded_rows(&self, cx: &App) -> Option<Vec<&R>> {
        let selection = self.selection.as_ref()?;
        let rows = self
            .rows
            .iter()
            .filter(|row| (selection.row_selectable)(row, cx))
            .collect::<Vec<_>>();
        (!rows.is_empty()).then_some(rows)
    }

    fn selection_target_state_for_header(&self, cx: &App) -> Option<bool> {
        self.selectable_loaded_rows(cx)
            .map(|rows| !rows.iter().all(|row| self.is_row_selected(row)))
    }

    fn is_selection_column_key(&self, key: &str) -> bool {
        self.selection.is_some() && key == SELECTION_COLUMN_KEY
    }

    fn display_row_element_id(row: &R) -> String {
        format!("nexora-crud-row-{}", row.row_id())
    }

    fn display_selection_element_id(row: &R) -> String {
        format!("nexora-crud-row-select-{}", row.row_id())
    }

    fn display_selection_debug_selector(row: &R) -> String {
        format!("crud-table-row-select-{}", row.row_id())
    }

    fn render_selection_cell(&mut self, row: &R, _window: &mut Window, cx: &mut App) -> AnyElement {
        let Some(selection) = &self.selection else {
            return div().into_any_element();
        };
        let checked = self.is_row_selected(row);
        let selectable = (selection.row_selectable)(row, cx);
        let on_select_row = selection.on_select_row.clone();
        let row_snapshot = row.clone();
        let row_id = row.row_id().clone();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Checkbox::new(Self::display_selection_element_id(row))
                    .small()
                    .checked(checked)
                    .disabled(!selectable)
                    .debug_selector(|| Self::display_selection_debug_selector(&row_snapshot))
                    .on_click({
                        let row_snapshot = row_snapshot.clone();
                        move |selected, window, cx| {
                            cx.stop_propagation();
                            if selectable {
                                on_select_row(
                                    RowSelectionEvent {
                                        row_id: row_id.clone(),
                                        row: row_snapshot.clone(),
                                        selected: *selected,
                                    },
                                    window,
                                    cx,
                                );
                            }
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_selection_header(&mut self, cx: &mut Context<TableState<Self>>) -> TableHeaderCell {
        let Some(selection) = &self.selection else {
            return TableHeaderCell::new("");
        };
        let disabled = self.selection_target_state_for_header(cx).is_none();
        let checked = self.loaded_rows_checked(cx);
        let on_select_loaded_rows = selection.on_select_loaded_rows.clone();
        let rows = self
            .rows
            .iter()
            .filter(|row| (selection.row_selectable)(row, cx))
            .cloned()
            .collect::<Vec<_>>();
        let row_ids = rows
            .iter()
            .map(|row| row.row_id().clone())
            .collect::<Vec<_>>();
        TableHeaderCell::element(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Checkbox::new("nexora-crud-loaded-rows-select")
                        .small()
                        .checked(checked)
                        .disabled(disabled)
                        .debug_selector(|| "crud-table-loaded-rows-select".to_owned())
                        .on_click(move |selected, window, cx| {
                            cx.stop_propagation();
                            if !disabled && !rows.is_empty() {
                                on_select_loaded_rows(
                                    LoadedRowsSelectionEvent {
                                        row_ids: row_ids.clone(),
                                        rows: rows.clone(),
                                        selected: *selected,
                                    },
                                    window,
                                    cx,
                                );
                            }
                        }),
                ),
        )
    }

    /// 追加一个操作列。
    ///
    /// 操作列使用原生 [`Column`] 定义；建议调用方设置 `.selectable(false)`，避免按钮、
    /// 开关等交互内容参与单元格选择。
    ///
    /// # Panics
    ///
    /// 当操作列使用了 Nexora 内部保留选择列 key，或与已有列 key 重复时会 panic。
    #[must_use]
    pub fn action_column<E>(
        mut self,
        column: Column,
        render: impl Fn(&R, &mut Window, &mut App) -> E + 'static,
    ) -> Self
    where
        E: IntoElement,
    {
        validate_action_column_key(&self.columns, &column);
        self.columns.push(column.clone());
        self.action_columns.push(CrudActionColumn {
            column,
            render: Rc::new(move |row, window, cx| render(row, window, cx).into_any_element()),
            text: None,
        });
        self
    }

    /// 为最近追加的操作列设置文本导出函数。
    ///
    /// 如果没有追加过操作列，该方法不会修改任何状态。
    #[must_use]
    pub fn action_text(mut self, text: impl Fn(&R, &App) -> String + 'static) -> Self {
        if let Some(column) = self.action_columns.last_mut() {
            column.text = Some(Rc::new(text));
        }
        self
    }

    /// 设置加载更多回调。
    ///
    /// 回调会直接接收原生 `TableState<CrudTableDelegate<R>>` 的 `Context`，因此调用方可以
    /// 沿用 gpui-component 的无限加载生命周期。
    #[must_use]
    pub fn on_load_more(
        mut self,
        load_more: impl Fn(&mut Window, &mut Context<TableState<Self>>) + 'static,
    ) -> Self {
        self.load_more = Some(Rc::new(load_more));
        self
    }

    /// 设置空表格标题。
    #[must_use]
    pub fn empty_title(mut self, title: impl Into<SharedString>) -> Self {
        self.empty_title = title.into();
        self
    }

    /// 设置空表格说明。
    #[must_use]
    pub fn empty_description(mut self, description: impl Into<SharedString>) -> Self {
        self.empty_description = Some(description.into());
        self
    }

    fn find_action_column(&self, key: &str) -> Option<&CrudActionColumn<R>> {
        self.action_columns
            .iter()
            .find(|action| action.column.key.as_ref() == key)
    }
}

impl<R: CrudTableRow> TableDelegate for CrudTableDelegate<R> {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        for (index, column) in self.columns.iter_mut().enumerate() {
            if column.sort.is_some() {
                column.sort = Some(if index == col_ix {
                    sort
                } else {
                    ColumnSort::Default
                });
            }
        }
        let selected = self.current_sort();
        if let Some(handler) = self.on_sort_changed.clone() {
            handler(selected, window, cx);
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let column = &self.columns[col_ix];
        if self.is_selection_column_key(column.key.as_ref()) {
            return self.render_selection_header(cx).into_any_element();
        }
        TableHeaderCell::new(column.name.clone())
            .align(R::header_alignment(column.key.as_ref()))
            .into_any_element()
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        mut to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        if col_ix >= self.columns.len() || to_ix >= self.columns.len() || col_ix == to_ix {
            return;
        }
        if self.selection.is_some() {
            if col_ix == 0 {
                return;
            }
            to_ix = to_ix.max(1);
            if col_ix == to_ix {
                return;
            }
        }
        let column = self.columns.remove(col_ix);
        self.columns.insert(to_ix, column);
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let id = self.rows.get(row_ix).map_or_else(
            || format!("nexora-crud-row-missing-{row_ix}"),
            Self::display_row_element_id,
        );
        div().id(id)
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix) else {
            return div().into_any_element();
        };
        let column_key = self.columns[col_ix].key.clone();
        if self.is_selection_column_key(column_key.as_ref()) {
            let row = row.clone();
            return self
                .render_selection_cell(&row, window, cx)
                .into_any_element();
        }
        if let Some(action) = self.find_action_column(column_key.as_ref()) {
            return (action.render)(row, window, cx).into_any_element();
        }

        row.render_cell(column_key.as_ref(), window, cx)
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .text_color(cx.theme().muted_foreground)
            .child(self.empty_title.clone())
            .when_some(self.empty_description.clone(), |this, description| {
                this.child(div().text_xs().child(description))
            })
    }

    fn loading(&self, _cx: &App) -> bool {
        self.loading
    }

    fn has_more(&self, _cx: &App) -> bool {
        self.rows.len() < self.total
            && self.load_more.is_some()
            && !self.loading
            && !self.loading_more
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if let Some(load_more) = self.load_more.clone() {
            load_more(window, cx);
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, cx: &App) -> String {
        let Some(row) = self.rows.get(row_ix) else {
            return String::new();
        };
        let column_key = self.columns[col_ix].key.as_ref();
        if self.is_selection_column_key(column_key) {
            return String::new();
        }
        if let Some(action) = self.find_action_column(column_key) {
            return action
                .text
                .as_ref()
                .map_or_else(String::new, |text| text(row, cx));
        }

        row.cell_text(column_key, cx)
    }
}

fn selection_column() -> Column {
    Column::new(SELECTION_COLUMN_KEY, "")
        .width(px(SELECTION_COLUMN_WIDTH))
        .min_width(px(SELECTION_COLUMN_WIDTH))
        .max_width(px(SELECTION_COLUMN_WIDTH))
        .fixed_left()
        .resizable(false)
        .movable(false)
        .selectable(false)
        .text_center()
}

fn validate_loaded_row_ids<R: CrudTableRow>(rows: &[R]) {
    let mut ids = HashSet::new();
    let mut displays = HashSet::new();
    for row in rows {
        let id = row.row_id().clone();
        let display = id.to_string();
        assert!(
            ids.insert(id),
            "CrudTableDelegate 当前已加载行存在重复业务 ID `{display}`",
        );
        assert!(
            displays.insert(display.clone()),
            "CrudTableDelegate 当前已加载行存在重复业务 ID Display 文本 `{display}`",
        );
    }
}

fn row_id_set<R: CrudTableRow>(rows: &[R]) -> HashSet<R::Id> {
    rows.iter().map(|row| row.row_id().clone()).collect()
}

fn validate_columns_do_not_use_reserved_key(columns: &[Column]) {
    for column in columns {
        assert!(
            column.key.as_ref() != SELECTION_COLUMN_KEY,
            "CrudTableDelegate 列 key `{SELECTION_COLUMN_KEY}` 是 Nexora 内部选择列保留 key",
        );
    }
}

fn validate_action_column_key(columns: &[Column], column: &Column) {
    assert!(
        column.key.as_ref() != SELECTION_COLUMN_KEY,
        "CrudTableDelegate 操作列 key `{SELECTION_COLUMN_KEY}` 是 Nexora 内部选择列保留 key",
    );
    assert!(
        !columns
            .iter()
            .any(|existing| existing.key.as_ref() == column.key.as_ref()),
        "CrudTableDelegate 操作列 key `{}` 与已有列重复",
        column.key
    );
}
