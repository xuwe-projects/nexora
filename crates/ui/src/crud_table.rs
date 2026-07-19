//! 标准 CRUD 数据表增强能力。
//!
//! 本模块在 `gpui-component` 的 `DataTable`、`Column` 与 `TableDelegate` 之上提供薄封装：
//! 行数据可以通过 [`CrudTableRow`] 描述默认列、正文渲染与导出文本，调用方也可以继续直接
//! 实现原生 `TableDelegate`，不需要经过本模块。

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, Styled as _, TextAlign, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _,
    table::{Column, TableDelegate, TableState},
    v_flex,
};

use crate::{TableCellVerticalAlign, TableHeaderCell};

/// 描述一行 CRUD 表格数据如何转换成 gpui-component 表格列与单元格。
///
/// `#[derive(nexora::CrudTableRow)]` 可以为普通命名字段结构体生成该 trait 的实现；复杂交
/// 互、远程排序或自定义分组表头仍然可以绕过派生宏，直接实现本 trait 或原生
/// `gpui_component::table::TableDelegate`。
pub trait CrudTableRow: Clone + 'static {
    /// 返回当前行类型默认声明的业务数据列。
    ///
    /// 返回值沿用 gpui-component 的 [`Column`]，因此列宽、排序、固定列和选择行为都仍然
    /// 由官方组件解释。
    fn columns() -> Vec<Column>;

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
type RowIdFactory<R> = Rc<dyn Fn(&R) -> String>;

struct CrudActionColumn<R: CrudTableRow> {
    column: Column,
    render: ActionRenderer<R>,
    text: Option<ActionText<R>>,
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
    row_id: Option<RowIdFactory<R>>,
    action_columns: Vec<CrudActionColumn<R>>,
    empty_title: SharedString,
    empty_description: Option<SharedString>,
}

impl<R: CrudTableRow> CrudTableDelegate<R> {
    /// 使用一组初始行创建 delegate。
    ///
    /// 默认列来自 [`CrudTableRow::columns`]；`total` 默认为当前行数，不触发无限加载。
    pub fn new(rows: Vec<R>) -> Self {
        let total = rows.len();
        Self {
            columns: R::columns(),
            rows,
            total,
            loading: false,
            loading_more: false,
            load_more: None,
            row_id: None,
            action_columns: Vec::new(),
            empty_title: SharedString::new("暂无数据"),
            empty_description: None,
        }
    }

    /// 返回当前可见行数据。
    pub fn rows(&self) -> &[R] {
        &self.rows
    }

    /// 返回当前可见行数据的可变引用。
    ///
    /// 该方法适合在异步保存完成后就地更新某一行。调用方仍需要在外层 `Context` 中触发
    /// `notify`，让 `DataTable` 重新读取 delegate。
    pub fn rows_mut(&mut self) -> &mut [R] {
        &mut self.rows
    }

    /// 返回当前所有列定义，包含追加的操作列。
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// 用新数据替换当前全部行。
    ///
    /// 该方法只更新 delegate 内部数据；调用方在 GPUI Entity 中修改后仍应通过对应
    /// `Context` 调用 `notify`。
    pub fn replace_rows(&mut self, rows: Vec<R>) {
        self.rows = rows;
        self.total = self.rows.len();
    }

    /// 追加一批行数据。
    pub fn append_rows(&mut self, rows: impl IntoIterator<Item = R>) {
        self.rows.extend(rows);
        self.total = self.total.max(self.rows.len());
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

    /// 设置每行使用的稳定 Element ID。
    ///
    /// 默认使用行下标生成 ID；涉及排序、分页、追加加载或单元格交互时，建议传入业务 ID，
    /// 例如 `|row| row.id.clone()`。
    #[must_use]
    pub fn row_id(mut self, row_id: impl Fn(&R) -> String + 'static) -> Self {
        self.row_id = Some(Rc::new(row_id));
        self
    }

    /// 追加一个操作列。
    ///
    /// 操作列使用原生 [`Column`] 定义；建议调用方设置 `.selectable(false)`，避免按钮、
    /// 开关等交互内容参与单元格选择。
    #[must_use]
    pub fn action_column<E>(
        mut self,
        column: Column,
        render: impl Fn(&R, &mut Window, &mut App) -> E + 'static,
    ) -> Self
    where
        E: IntoElement,
    {
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

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let column = &self.columns[col_ix];
        TableHeaderCell::new(column.name.clone()).align(R::header_alignment(column.key.as_ref()))
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        if col_ix >= self.columns.len() || to_ix >= self.columns.len() || col_ix == to_ix {
            return;
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
        let id = self
            .rows
            .get(row_ix)
            .and_then(|row| self.row_id.as_ref().map(|row_id| row_id(row)))
            .unwrap_or_else(|| format!("crud-row-{row_ix}"));
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
        if let Some(action) = self.find_action_column(column_key) {
            return action
                .text
                .as_ref()
                .map_or_else(String::new, |text| text(row, cx));
        }

        row.cell_text(column_key, cx)
    }
}
