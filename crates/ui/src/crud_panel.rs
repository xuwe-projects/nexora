//! 强类型标准 CRUD 资源管理 Panel。

use gpui::{
    AnyElement, App, ElementId, Entity, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Size, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    form::{Field, v_form},
    h_flex,
    pagination::Pagination,
    table::DataTable,
    v_flex,
};

use crate::{CrudListState, CrudTableRow};

fn single_page_navigation_button(
    id: ElementId,
    label: &'static str,
    icon: IconName,
    reverse: bool,
    size: Size,
    debug_selector: &'static str,
) -> Button {
    Button::new(id)
        .debug_selector(move || debug_selector.to_owned())
        .ghost()
        .compact()
        .with_size(size)
        .disabled(true)
        .tooltip(label)
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .flex_nowrap()
                .when(reverse, |this| this.flex_row_reverse())
                .child(SharedString::from(label))
                .child(Icon::new(icon)),
        )
}

fn single_page_pagination(id: ElementId, size: Size, loading: bool) -> impl IntoElement {
    h_flex()
        .id(id.clone())
        .role(Role::Navigation)
        .aria_label("Pagination")
        .px_2()
        .py_2()
        .gap_1()
        .items_center()
        .child(single_page_navigation_button(
            (id.clone(), "previous").into(),
            "上一页",
            IconName::ChevronLeft,
            true,
            size,
            "crud-panel-pagination-previous",
        ))
        .child(
            Button::new((id.clone(), "page-1"))
                .debug_selector(|| "crud-panel-pagination-page-1".to_owned())
                .outline()
                .compact()
                .with_size(size)
                .disabled(loading)
                .label("1"),
        )
        .child(single_page_navigation_button(
            (id, "next").into(),
            "下一页",
            IconName::ChevronRight,
            false,
            size,
            "crud-panel-pagination-next",
        ))
}

/// 标准单主数据集分页列表的默认 Panel。
///
/// `R` 必须实现 [`CrudTableRow`]，`Q` 必须实现 [`contracts::crud_query::CrudQuery`]；因此任意
/// 内容不能再伪装成 CRUD Panel。筛选字段直接接收官方 [`Field`]，表格和分页也使用官方组件。
#[derive(IntoElement)]
pub struct CrudPanel<R, Q>
where
    R: CrudTableRow,
    Q: contracts::crud_query::CrudQuery<Sort = R::Sort>,
{
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    state: Entity<CrudListState<R, Q>>,
    quick_filters: Vec<AnyElement>,
    fields: Vec<Field>,
    header_actions: Vec<AnyElement>,
    toolbar_actions: Vec<AnyElement>,
    filter_columns: usize,
    size: Size,
}

impl<R, Q> CrudPanel<R, Q>
where
    R: CrudTableRow,
    Q: contracts::crud_query::CrudQuery<Sort = R::Sort>,
{
    /// 创建绑定强类型列表状态的 CRUD Panel。
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        state: Entity<CrudListState<R, Q>>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            state,
            quick_filters: Vec::new(),
            fields: Vec::new(),
            header_actions: Vec::new(),
            toolbar_actions: Vec::new(),
            filter_columns: 1,
            size: Size::default(),
        }
    }

    /// 设置标题下方的资源说明。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 添加由 `CrudQuery` 快速筛选字段驱动的官方控件。
    #[must_use]
    pub fn quick_filter(mut self, filter: impl IntoElement) -> Self {
        self.quick_filters.push(filter.into_any_element());
        self
    }

    /// 添加官方 `Form` 使用的筛选字段。
    #[must_use]
    pub fn filter(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// 设置标准筛选表单列数。
    #[must_use]
    pub fn filter_columns(mut self, columns: usize) -> Self {
        self.filter_columns = columns.max(1);
        self
    }

    /// 添加创建、导出等页面主操作。
    #[must_use]
    pub fn header_action(mut self, action: impl IntoElement) -> Self {
        self.header_actions.push(action.into_any_element());
        self
    }

    /// 添加导入、重置、批量处理等辅助操作。
    #[must_use]
    pub fn toolbar_action(mut self, action: impl IntoElement) -> Self {
        self.toolbar_actions.push(action.into_any_element());
        self
    }

    /// 返回当前 Panel 是否会渲染筛选表单。
    pub fn has_filters(&self) -> bool {
        !self.fields.is_empty()
    }
}

impl<R, Q> gpui_component::Sizable for CrudPanel<R, Q>
where
    R: CrudTableRow,
    Q: contracts::crud_query::CrudQuery<Sort = R::Sort>,
{
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl<R, Q> RenderOnce for CrudPanel<R, Q>
where
    R: CrudTableRow,
    Q: contracts::crud_query::CrudQuery<Sort = R::Sort>,
{
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let total = state.total();
        let page = state.visible_page() as usize;
        let total_pages = state.total_pages() as usize;
        let page_size = state.page_size();
        let loading = state.is_loading();
        let refreshing = state.is_refreshing();
        let error = state.visible_error().cloned();
        let table_state = state.table_state().clone();
        let has_rows = !state.current_rows().is_empty();
        let weak_for_page = self.state.downgrade();
        let weak_for_retry = self.state.downgrade();
        let id = self.id.clone();
        let pagination_id: ElementId = (id.clone(), "pagination").into();
        let size = self.size;
        let has_quick_filters = !self.quick_filters.is_empty();
        let has_fields = !self.fields.is_empty();
        let has_toolbar_actions = !self.toolbar_actions.is_empty();

        let header = h_flex()
            .w_full()
            .min_w_0()
            .items_start()
            .justify_between()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_xl().font_bold().child(self.title))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("共 {total} 条")),
                            ),
                    )
                    .when_some(self.description, |this, description| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        )
                    }),
            )
            .when(!self.header_actions.is_empty(), |this| {
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .flex_wrap()
                        .justify_end()
                        .gap_2()
                        .children(self.header_actions),
                )
            });

        let controls = v_flex()
            .w_full()
            .gap_3()
            .when(has_quick_filters, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_2()
                        .children(self.quick_filters),
                )
            })
            .when(has_fields, |this| {
                this.child(
                    v_form()
                        .columns(self.filter_columns)
                        .children(self.fields)
                        .with_size(size),
                )
            })
            .when(has_toolbar_actions, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .justify_end()
                        .gap_2()
                        .children(self.toolbar_actions),
                )
            });

        let table = DataTable::new(&table_state)
            .stripe(true)
            .bordered(true)
            .with_size(size);
        let body = v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_2()
            .when_some(error.clone(), |this, error| {
                let message = error.message().clone();
                this.child(
                    v_flex()
                        .gap_2()
                        .child(
                            Alert::error((id.clone(), "load-error"), message).title("数据加载失败"),
                        )
                        .when(error.is_retryable(), |this| {
                            this.child(
                                Button::new((id.clone(), "retry"))
                                    .outline()
                                    .with_size(size)
                                    .label("重试")
                                    .on_click(move |_, _, cx| {
                                        _ = weak_for_retry.update(cx, |state, cx| {
                                            state.retry_visible(cx);
                                        });
                                    }),
                            )
                        }),
                )
            })
            .when(refreshing, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("正在刷新当前页…"),
                )
            })
            .when(!error.is_some() || has_rows || loading, |this| {
                this.child(div().w_full().flex_1().min_h_0().child(table))
            });

        v_flex()
            .id(self.id)
            .size_full()
            .min_h_0()
            .gap_4()
            .p_5()
            .child(header)
            .when(
                has_quick_filters || has_fields || has_toolbar_actions,
                |this| this.child(controls),
            )
            .child(body)
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "第 {page} / {total_pages} 页 · 每页 {page_size} 条"
                            )),
                    )
                    .child(if total_pages == 1 {
                        single_page_pagination(pagination_id, size, loading).into_any_element()
                    } else {
                        Pagination::new(pagination_id)
                            .current_page(page)
                            .total_pages(total_pages)
                            .visible_pages(5)
                            .with_size(size)
                            .disabled(loading)
                            .on_click(move |page, _, cx| {
                                _ = weak_for_page.update(cx, |state, cx| {
                                    state.go_to_page(*page as u32, cx);
                                });
                            })
                            .into_any_element()
                    }),
            )
    }
}
