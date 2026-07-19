//! 标准 CRUD Panel 骨架。
//!
//! 本模块提供三段式资源管理 Panel：顶部摘要卡片、可选筛选/操作工具栏，以及默认填满剩余
//! 高度的主内容区。分页、虚拟滚动和数据加载策略仍由调用方传入的表格或列表组件负责。

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, IntoElement, ParentElement, RenderOnce, SharedString, Window, div,
    prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, Size, StyledExt as _, button::Button,
    h_flex, v_flex,
};

use crate::Card;

const REFRESH_ICON_PATH: &str = "icons/rotate-ccw.svg";

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

struct CrudRefreshAction {
    id: String,
    loading: bool,
    disabled: bool,
    on_click: ClickHandler,
}

/// CRUD Panel 中位于标题摘要下方的可选工具栏。
///
/// 工具栏分为两个区域：左侧/上方的筛选条件区，以及右侧/下方的操作区。调用方可以只提供
/// 其中一个区域；如果两个区域都为空，工具栏会渲染为空元素，配合 [`CrudPanel`] 使用时则会
/// 直接省略整张工具栏卡片。
#[derive(Default, IntoElement)]
pub struct CrudPanelToolbar {
    filters: Vec<AnyElement>,
    actions: Vec<AnyElement>,
}

impl CrudPanelToolbar {
    /// 创建一个没有筛选条件和操作按钮的工具栏。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个筛选条件控件。
    ///
    /// 控件通常是 `Input`、`Select`、`Combobox`、日期选择器或自定义筛选组合。工具栏只负
    /// 责排列和换行，不解释控件的业务语义。
    #[must_use]
    pub fn filter(mut self, filter: impl IntoElement) -> Self {
        self.filters.push(filter.into_any_element());
        self
    }

    /// 批量添加筛选条件控件。
    ///
    /// 该方法适合调用方已经按权限、配置或资源类型生成了一组筛选控件的场景。
    #[must_use]
    pub fn filters<E>(mut self, filters: impl IntoIterator<Item = E>) -> Self
    where
        E: IntoElement,
    {
        self.filters
            .extend(filters.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// 添加一个操作控件。
    ///
    /// 操作通常是查询、创建、导入、导出或批量操作按钮。多个操作会按添加顺序排列在工具栏
    /// 的操作区。
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// 批量添加操作控件。
    ///
    /// 该方法适合调用方根据选择状态、权限或资源状态生成一组操作按钮的场景。
    #[must_use]
    pub fn actions<E>(mut self, actions: impl IntoIterator<Item = E>) -> Self
    where
        E: IntoElement,
    {
        self.actions
            .extend(actions.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// 返回工具栏是否没有任何可见内容。
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.actions.is_empty()
    }
}

impl RenderOnce for CrudPanelToolbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let has_filters = !self.filters.is_empty();
        let has_actions = !self.actions.is_empty();
        if !has_filters && !has_actions {
            return div().into_any_element();
        }

        Card::new()
            .w_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(
                v_flex()
                    .w_full()
                    .when(has_filters, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .gap_2()
                                .p_3()
                                .children(self.filters),
                        )
                    })
                    .when(has_actions, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .justify_end()
                                .gap_2()
                                .px_3()
                                .py_3()
                                .when(has_filters, |this| {
                                    this.border_t_1().border_color(cx.theme().border)
                                })
                                .children(self.actions),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// 标准 CRUD 资源管理 Panel 布局。
///
/// Panel 固定由顶部摘要卡片、可选 [`CrudPanelToolbar`] 和主内容区组成。主内容区会以
/// `flex_1` 和 `min_h_0` 填满剩余高度，因此传入的 `DataTable`、虚拟列表或自定义内容可以在自身内部
/// 管理 Y 轴滚动与无限加载。
#[derive(IntoElement)]
pub struct CrudPanel {
    title: SharedString,
    description: Option<SharedString>,
    refresh: Option<CrudRefreshAction>,
    toolbar: CrudPanelToolbar,
    content: AnyElement,
    size: Size,
}

impl CrudPanel {
    /// 创建一个带标题和主内容区的 CRUD Panel。
    ///
    /// `content` 是底部主体区域，通常传入表格、列表或一个包含错误提示与表格的垂直布局。
    pub fn new(title: impl Into<SharedString>, content: impl IntoElement) -> Self {
        Self {
            title: title.into(),
            description: None,
            refresh: None,
            toolbar: CrudPanelToolbar::new(),
            content: content.into_any_element(),
            size: Size::default(),
        }
    }

    /// 设置顶部摘要卡片中的描述文本。
    ///
    /// 描述适合展示资源说明、已加载数量、当前筛选结果数量等只读摘要。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置顶部摘要卡片右侧的刷新按钮。
    ///
    /// 刷新按钮使用项目统一的 `rotate-ccw.svg` 图标。建议把它用于重新拉取当前资源数据；
    /// 针对筛选条件的“查询/应用筛选”操作应作为工具栏 action 传入，避免页面级刷新与查询语
    /// 义混在一起。
    #[must_use]
    pub fn refresh(
        mut self,
        id: impl Into<String>,
        loading: bool,
        disabled: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.refresh = Some(CrudRefreshAction {
            id: id.into(),
            loading,
            disabled,
            on_click: Rc::new(on_click),
        });
        self
    }

    /// 替换整块可选工具栏。
    ///
    /// 如果传入的工具栏没有筛选条件和操作按钮，渲染时会省略工具栏卡片。
    #[must_use]
    pub fn toolbar(mut self, toolbar: CrudPanelToolbar) -> Self {
        self.toolbar = toolbar;
        self
    }

    /// 向默认工具栏添加一个筛选条件控件。
    ///
    /// 这是 [`CrudPanelToolbar::filter`] 的便捷转发，适合页面只需要少量筛选控件时直接链式调
    /// 用。
    #[must_use]
    pub fn filter(mut self, filter: impl IntoElement) -> Self {
        self.toolbar = self.toolbar.filter(filter);
        self
    }

    /// 向默认工具栏批量添加筛选条件控件。
    ///
    /// 这是 [`CrudPanelToolbar::filters`] 的便捷转发。
    #[must_use]
    pub fn filters<E>(mut self, filters: impl IntoIterator<Item = E>) -> Self
    where
        E: IntoElement,
    {
        self.toolbar = self.toolbar.filters(filters);
        self
    }

    /// 向默认工具栏添加一个操作控件。
    ///
    /// 这是 [`CrudPanelToolbar::action`] 的便捷转发，适合页面只需要少量操作按钮时直接链式调
    /// 用。
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.toolbar = self.toolbar.action(action);
        self
    }

    /// 向默认工具栏批量添加操作控件。
    ///
    /// 这是 [`CrudPanelToolbar::actions`] 的便捷转发。
    #[must_use]
    pub fn actions<E>(mut self, actions: impl IntoIterator<Item = E>) -> Self
    where
        E: IntoElement,
    {
        self.toolbar = self.toolbar.actions(actions);
        self
    }

    /// 返回当前 Panel 是否会渲染工具栏卡片。
    pub fn has_toolbar(&self) -> bool {
        !self.toolbar.is_empty()
    }
}

impl gpui_component::Sizable for CrudPanel {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for CrudPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let has_toolbar = !self.toolbar.is_empty();
        let size = self.size;

        v_flex()
            .size_full()
            .min_h_0()
            .gap_4()
            .p_5()
            .child(
                Card::new().w_full().flex_shrink_0().p_4().child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .justify_between()
                        .gap_4()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().text_xl().font_bold().child(self.title))
                                .when_some(self.description, |this, description| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(description),
                                    )
                                }),
                        )
                        .when_some(self.refresh, |this, action| {
                            let on_click = action.on_click;
                            this.child(
                                Button::new(action.id)
                                    .outline()
                                    .with_size(size)
                                    .icon(Icon::default().path(REFRESH_ICON_PATH))
                                    .label("刷新")
                                    .loading(action.loading)
                                    .disabled(action.loading || action.disabled)
                                    .on_click(move |event, window, cx| {
                                        on_click(event, window, cx);
                                    }),
                            )
                        }),
                ),
            )
            .when(has_toolbar, |this| this.child(self.toolbar))
            .child(v_flex().w_full().flex_1().min_h_0().child(self.content))
    }
}
