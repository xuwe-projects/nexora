//! 基于 gpui-component 组合实现的单选级联选择器。
//!
//! 组件复用 [`gpui_component::popover::Popover`]、[`gpui_component::input::Input`] 与
//! [`gpui_component::button::Button`]，只在本模块维护层级路径、分列展示和搜索匹配逻辑。

use std::fmt;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, IntoElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Subscription, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    popover::Popover,
    scroll::ScrollableElement as _,
    v_flex,
};

/// 级联选择器中的一个稳定选项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CascaderOption {
    value: SharedString,
    label: SharedString,
    disabled: bool,
    children: Vec<Self>,
}

impl CascaderOption {
    /// 创建一个没有子项的可选节点。
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
            children: Vec::new(),
        }
    }

    /// 设置节点是否不可选择。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 追加一个子节点。
    pub fn child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// 追加多个子节点。
    pub fn children(mut self, children: impl IntoIterator<Item = Self>) -> Self {
        self.children.extend(children);
        self
    }

    /// 返回提交给业务层的稳定值。
    pub fn value(&self) -> &str {
        self.value.as_ref()
    }

    /// 返回显示文本。
    pub fn label(&self) -> &str {
        self.label.as_ref()
    }

    /// 返回节点是否不可选择。
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// 返回只读子节点。
    pub fn children_ref(&self) -> &[Self] {
        &self.children
    }

    /// 返回节点是否为叶子节点。
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// 一次完整的级联选择结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CascaderSelection {
    values: Vec<SharedString>,
    labels: Vec<SharedString>,
}

impl CascaderSelection {
    fn new(values: Vec<SharedString>, labels: Vec<SharedString>) -> Self {
        Self { values, labels }
    }

    /// 返回从根节点到当前节点的稳定值路径。
    pub fn values(&self) -> &[SharedString] {
        &self.values
    }

    /// 返回从根节点到当前节点的显示文本路径。
    pub fn labels(&self) -> &[SharedString] {
        &self.labels
    }

    /// 返回是否尚未选择任何节点。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// 级联选择器产生的类型化事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CascaderEvent {
    /// 用户选择了一个节点，清空时携带空路径。
    Change(
        /// 从根节点到当前节点的稳定值与展示文本路径。
        CascaderSelection,
    ),
}

/// 设置的值路径无法在选项树中完整解析。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CascaderValueError {
    value: SharedString,
    depth: usize,
}

impl CascaderValueError {
    /// 返回首个无法解析的值。
    pub fn value(&self) -> &str {
        self.value.as_ref()
    }

    /// 返回无法解析的路径深度（从零开始）。
    pub fn depth(&self) -> usize {
        self.depth
    }
}

impl fmt::Display for CascaderValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "级联路径第 {} 层不存在值 `{}`",
            self.depth, self.value
        )
    }
}

impl std::error::Error for CascaderValueError {}

/// 级联选择器的长期状态。
///
/// `id` 必须在同一窗口内稳定唯一。输入框 Entity 与订阅在构造时创建，渲染阶段不会创建
/// 长期状态。业务层应在 Feature 或表单组件初始化时持有 `Entity<CascaderState>`。
pub struct CascaderState {
    id: SharedString,
    options: Vec<CascaderOption>,
    selected: CascaderSelection,
    active_values: Vec<SharedString>,
    open: bool,
    disabled: bool,
    allow_clear: bool,
    searchable: bool,
    change_on_select: bool,
    placeholder: SharedString,
    separator: SharedString,
    search_query: SharedString,
    search_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CascaderEvent> for CascaderState {}

impl CascaderState {
    /// 创建级联状态并初始化搜索输入框。
    ///
    /// `id` 用于生成 Popover、列和选项的稳定 Element ID；`options` 的 `value` 应在同级
    /// 节点中保持唯一。组件不会执行 I/O，也不会修改传入的选项。
    pub fn new(
        id: impl Into<SharedString>,
        options: impl IntoIterator<Item = CascaderOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索选项"));
        let subscriptions =
            vec![
                cx.subscribe(&search_input, |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.search_query = input.read(cx).value();
                        cx.notify();
                    }
                }),
            ];

        Self {
            id: id.into(),
            options: options.into_iter().collect(),
            selected: CascaderSelection::default(),
            active_values: Vec::new(),
            open: false,
            disabled: false,
            allow_clear: true,
            searchable: true,
            change_on_select: false,
            placeholder: "请选择".into(),
            separator: " / ".into(),
            search_query: SharedString::default(),
            search_input,
            _subscriptions: subscriptions,
        }
    }

    /// 设置没有选中值时的提示文本。
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// 更新搜索框提示文本。
    pub fn set_search_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
        cx.notify();
    }

    /// 设置路径显示分隔符。
    pub fn separator(mut self, separator: impl Into<SharedString>) -> Self {
        self.separator = separator.into();
        self
    }

    /// 设置是否允许清空，默认为 `true`。
    pub fn allow_clear(mut self, allow_clear: bool) -> Self {
        self.allow_clear = allow_clear;
        self
    }

    /// 设置是否显示搜索框，默认为 `true`。
    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    /// 设置是否在选择非叶子节点时立即产生变更事件。
    pub fn change_on_select(mut self, change_on_select: bool) -> Self {
        self.change_on_select = change_on_select;
        self
    }

    /// 设置整个组件是否禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 返回当前选择。
    pub fn selection(&self) -> &CascaderSelection {
        &self.selected
    }

    /// 返回当前是否展开。
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 使用稳定值路径设置当前选择，不产生用户变更事件。
    ///
    /// 路径中的任一值不存在时保持原状态并返回 [`CascaderValueError`]。
    ///
    /// # Errors
    ///
    /// 任一层级不存在指定稳定值时返回包含失败值与零基深度的
    /// [`CascaderValueError`]，原选择保持不变。
    pub fn set_value(
        &mut self,
        values: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) -> Result<(), CascaderValueError> {
        let values = values.into_iter().map(Into::into).collect::<Vec<_>>();
        let labels = Self::resolve_labels(&self.options, &values)?;
        self.active_values = values.clone();
        self.selected = CascaderSelection::new(values, labels);
        cx.notify();
        Ok(())
    }

    /// 清空当前选择并产生空路径变更事件。
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.selected = CascaderSelection::default();
        self.active_values.clear();
        cx.emit(CascaderEvent::Change(self.selected.clone()));
        cx.notify();
    }

    fn resolve_labels(
        options: &[CascaderOption],
        values: &[SharedString],
    ) -> Result<Vec<SharedString>, CascaderValueError> {
        let mut current = options;
        let mut labels = Vec::with_capacity(values.len());
        for (depth, value) in values.iter().enumerate() {
            let Some(option) = current.iter().find(|option| option.value == *value) else {
                return Err(CascaderValueError {
                    value: value.clone(),
                    depth,
                });
            };
            labels.push(option.label.clone());
            current = &option.children;
        }
        Ok(labels)
    }

    fn select_option(&mut self, depth: usize, option: &CascaderOption, cx: &mut Context<Self>) {
        if self.disabled || option.disabled {
            return;
        }
        self.active_values.truncate(depth);
        self.active_values.push(option.value.clone());

        if option.is_leaf() || self.change_on_select {
            let labels = Self::resolve_labels(&self.options, &self.active_values)
                .expect("活动路径来自已存在选项");
            self.selected = CascaderSelection::new(self.active_values.clone(), labels);
            cx.emit(CascaderEvent::Change(self.selected.clone()));
        }
        if option.is_leaf() {
            self.open = false;
        }
        cx.notify();
    }

    fn select_search_result(&mut self, selection: CascaderSelection, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.active_values = selection.values.clone();
        self.selected = selection;
        self.open = false;
        cx.emit(CascaderEvent::Change(self.selected.clone()));
        cx.notify();
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            self.open = false;
            return;
        }
        self.open = open;
        if !open && !self.search_query.is_empty() {
            self.search_query = SharedString::default();
            self.search_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    fn columns(&self) -> Vec<Vec<CascaderOption>> {
        let mut columns = vec![self.options.clone()];
        let mut current = &self.options;
        for value in &self.active_values {
            let Some(option) = current.iter().find(|option| option.value == *value) else {
                break;
            };
            if option.children.is_empty() {
                break;
            }
            columns.push(option.children.clone());
            current = &option.children;
        }
        columns
    }

    fn search_results(&self) -> Vec<CascaderSelection> {
        let query = self.search_query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        let mut values = Vec::new();
        let mut labels = Vec::new();
        Self::collect_search_results(
            &self.options,
            &query,
            &mut values,
            &mut labels,
            &mut results,
        );
        results
    }

    fn collect_search_results(
        options: &[CascaderOption],
        query: &str,
        values: &mut Vec<SharedString>,
        labels: &mut Vec<SharedString>,
        results: &mut Vec<CascaderSelection>,
    ) {
        for option in options {
            values.push(option.value.clone());
            labels.push(option.label.clone());
            if option.is_leaf() {
                let searchable = labels
                    .iter()
                    .map(SharedString::as_ref)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                if searchable.contains(query) && !option.disabled {
                    results.push(CascaderSelection::new(values.clone(), labels.clone()));
                }
            } else {
                Self::collect_search_results(&option.children, query, values, labels, results);
            }
            values.pop();
            labels.pop();
        }
    }
}

/// 绑定 [`CascaderState`] 的级联选择器元素。
#[derive(IntoElement)]
pub struct Cascader {
    state: Entity<CascaderState>,
    style: StyleRefinement,
}

impl Cascader {
    /// 创建绑定到长期状态的级联元素。
    pub fn new(state: &Entity<CascaderState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Cascader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[derive(Clone, IntoElement)]
struct CascaderPopup {
    state: Entity<CascaderState>,
}

impl CascaderPopup {
    fn render_search_results(
        state: &Entity<CascaderState>,
        id: &SharedString,
        separator: &str,
        results: Vec<CascaderSelection>,
    ) -> AnyElement {
        if results.is_empty() {
            return div()
                .w_full()
                .p_4()
                .text_sm()
                .text_center()
                .child("没有匹配选项")
                .into_any_element();
        }

        v_flex()
            .w(px(320.0))
            .max_h(px(280.0))
            .overflow_y_scrollbar()
            .gap_1()
            .children(results.into_iter().enumerate().map(|(index, selection)| {
                let display = selection
                    .labels
                    .iter()
                    .map(SharedString::as_ref)
                    .collect::<Vec<_>>()
                    .join(separator);
                let event_selection = selection.clone();
                let event_state = state.clone();
                Button::new(format!("{}-search-result-{}", id, index))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .label(display)
                    .on_click(move |_, _, cx| {
                        event_state.update(cx, |state, cx| {
                            state.select_search_result(event_selection.clone(), cx);
                        });
                    })
            }))
            .into_any_element()
    }

    fn render_columns(
        state: &Entity<CascaderState>,
        id: &SharedString,
        columns: Vec<Vec<CascaderOption>>,
        active_values: &[SharedString],
    ) -> AnyElement {
        h_flex()
            .items_start()
            .max_w(px(720.0))
            .overflow_x_scrollbar()
            .children(columns.into_iter().enumerate().map(|(depth, options)| {
                v_flex()
                    .id(format!("{}-column-{}", id, depth))
                    .w(px(200.0))
                    .max_h(px(280.0))
                    .p_1()
                    .gap_0p5()
                    .overflow_y_scrollbar()
                    .when(depth > 0, |this| this.border_l_1())
                    .children(options.into_iter().map(|option| {
                        let selected = active_values.get(depth) == Some(&option.value);
                        let disabled = option.disabled;
                        let has_children = !option.children.is_empty();
                        let option_value = option.value.clone();
                        let event_option = option.clone();
                        let event_state = state.clone();
                        Button::new(format!("{}-option-{}-{}", id, depth, option_value))
                            .ghost()
                            .small()
                            .w_full()
                            .justify_start()
                            .selected(selected)
                            .disabled(disabled)
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .child(option.label.clone()),
                                    )
                                    .when(has_children, |this| {
                                        this.child(Icon::new(IconName::ChevronRight).xsmall())
                                    }),
                            )
                            .on_click(move |_, _, cx| {
                                event_state.update(cx, |state, cx| {
                                    state.select_option(depth, &event_option, cx);
                                });
                            })
                    }))
            }))
            .into_any_element()
    }
}

impl RenderOnce for CascaderPopup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (
            id,
            searchable,
            search_input,
            separator,
            search_query,
            search_results,
            columns,
            active_values,
        ) = {
            let state = self.state.read(cx);
            (
                state.id.clone(),
                state.searchable,
                state.search_input.clone(),
                state.separator.clone(),
                state.search_query.clone(),
                state.search_results(),
                state.columns(),
                state.active_values.clone(),
            )
        };

        v_flex()
            .min_w(px(200.0))
            .gap_2()
            .when(searchable, |this| {
                this.child(
                    Input::new(&search_input)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).xsmall()),
                )
            })
            .child(if searchable && !search_query.trim().is_empty() {
                Self::render_search_results(&self.state, &id, separator.as_ref(), search_results)
            } else {
                Self::render_columns(&self.state, &id, columns, &active_values)
            })
    }
}

impl RenderOnce for Cascader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (id, open, disabled, allow_clear, placeholder, separator, selection) = {
            let state = self.state.read(cx);
            (
                state.id.clone(),
                state.open,
                state.disabled,
                state.allow_clear,
                state.placeholder.clone(),
                state.separator.clone(),
                state.selected.clone(),
            )
        };

        let display: SharedString = if selection.is_empty() {
            placeholder
        } else {
            selection
                .labels
                .iter()
                .map(SharedString::as_ref)
                .collect::<Vec<_>>()
                .join(separator.as_ref())
                .into()
        };
        let clear_state = self.state.clone();
        let trigger = Button::new(format!("{}-trigger", id))
            .outline()
            .w_full()
            .justify_between()
            .disabled(disabled)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_left()
                    .truncate()
                    .when(selection.is_empty(), |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .child(display),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .when(allow_clear && !selection.is_empty() && !disabled, |this| {
                        this.child(
                            Button::new(format!("{}-clear", id))
                                .xsmall()
                                .ghost()
                                .icon(IconName::CircleX)
                                .tooltip("清空")
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    clear_state.update(cx, |state, cx| state.clear(cx));
                                }),
                        )
                    })
                    .child(Icon::new(IconName::ChevronDown).xsmall()),
            );

        let open_state = self.state.clone();
        let popup_state = self.state;
        Popover::new(format!("{}-popover", id))
            .open(open)
            .on_open_change(move |open, window, cx| {
                open_state.update(cx, |state, cx| state.set_open(*open, window, cx));
            })
            .trigger(trigger)
            .content(move |_, _, _| CascaderPopup {
                state: popup_state.clone(),
            })
            .refine_style(&self.style)
    }
}
