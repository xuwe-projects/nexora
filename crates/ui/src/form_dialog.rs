//! 带草稿追踪与未保存确认的内容区表单对话框。
//!
//! `FormDialogState` 保存打开状态、提交状态和字段草稿；`FormDialog` 负责统一渲染标题、
//! 描述、可滚动内容区以及取消/提交操作。组件组合 [`crate::PanelDialog`]，因此遮罩只覆盖
//! Feature 所属的 Panel，不会阻塞整个应用窗口和 Sidebar。

use std::{collections::BTreeMap, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FocusHandle, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Task, WeakFocusHandle, Window, div, prelude::*,
    px, relative,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
    form::{Field, v_form},
    h_flex, v_flex,
};

use crate::{AnyFormFieldState, FieldValue, FormFieldState, PanelDialog};

type DialogHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
const DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO: f32 = 0.8;

/// 表单字段在打开对话框时的原值与当前草稿。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFieldDraft {
    key: String,
    label: SharedString,
    original: String,
    draft: String,
}

impl FormFieldDraft {
    /// 创建一条字段草稿记录。
    ///
    /// `key` 是调用方用于更新和查询字段的稳定标识，`label` 用于未保存确认界面；
    /// `original` 与 `draft` 不相等时字段被视为尚未保存。
    pub fn new(
        key: impl Into<String>,
        label: impl Into<SharedString>,
        original: impl Into<String>,
        draft: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            original: original.into(),
            draft: draft.into(),
        }
    }

    /// 返回字段的稳定标识。
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    /// 返回确认界面中使用的字段名称。
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// 返回本次编辑开始时的已保存值。
    pub fn original(&self) -> &str {
        self.original.as_str()
    }

    /// 返回字段当前尚未提交的草稿值。
    pub fn draft(&self) -> &str {
        self.draft.as_str()
    }

    /// 返回当前草稿是否不同于已保存值。
    pub fn is_dirty(&self) -> bool {
        self.original != self.draft
    }
}

/// `FormDialog` 的打开状态、提交状态和字段草稿模型。
///
/// 调用方应在持有表单输入的 Entity 初始化时创建一个长期存在的
/// `Entity<FormDialogState>`，并在输入变化时调用 [`Self::set_field_draft`]。这样默认取消
/// 行为和自定义取消处理器都可以查询相同的未保存事实来源。
pub struct FormDialogState {
    focus_handle: FocusHandle,
    previous_focus: Option<WeakFocusHandle>,
    fields: BTreeMap<String, FormFieldDraft>,
    validation_fields: Vec<AnyFormFieldState>,
    open: bool,
    submitting: bool,
    confirming_discard: bool,
}

impl FormDialogState {
    /// 创建关闭状态的表单对话框模型，并分配稳定焦点边界。
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            fields: BTreeMap::new(),
            validation_fields: Vec::new(),
            open: false,
            submitting: false,
            confirming_discard: false,
        }
    }

    /// 注册一个由 [`FormFieldState<V>`] 管理的类型化字段。
    ///
    /// `FormDialog` 提交时会等待该字段已存在的最新异步事件任务，然后执行声明式规则并读取
    /// 已有异步事件错误。注册只擦除聚合所需能力，不会破坏字段事件回调的泛型类型安全。
    #[must_use]
    pub fn field<V: FieldValue>(mut self, field: &Entity<FormFieldState<V>>) -> Self {
        self.validation_fields.push(AnyFormFieldState::new(field));
        self
    }

    /// 打开对话框、保存此前焦点并聚焦表单边界。
    ///
    /// 本方法不会清空字段草稿；调用方应先通过 [`Self::reset_fields`] 或
    /// [`Self::set_field_draft`] 初始化本次编辑的原值和草稿。
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.previous_focus = window.focused(cx).map(|handle| handle.downgrade());
        self.open = true;
        self.submitting = false;
        self.confirming_discard = false;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// 返回对话框当前是否打开。
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// 设置异步提交状态。
    ///
    /// 提交期间默认取消、遮罩关闭和提交按钮都会被禁用，避免重复请求或丢失响应。
    pub fn set_submitting(&mut self, submitting: bool, cx: &mut Context<Self>) {
        if self.submitting == submitting {
            return;
        }
        self.submitting = submitting;
        cx.notify();
    }

    /// 返回表单是否正在执行提交操作。
    pub const fn is_submitting(&self) -> bool {
        self.submitting
    }

    /// 新增或替换一个字段的原值与当前草稿。
    ///
    /// 相同 `key` 的后续调用只更新这一字段，不会影响其他字段。调用方可以用 JSON、逗号
    /// 分隔 ID 或其他稳定文本表示复合控件的草稿，只要原值与草稿使用相同表示即可。
    pub fn set_field_draft(
        &mut self,
        key: impl Into<String>,
        label: impl Into<SharedString>,
        original: impl Into<String>,
        draft: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let field = FormFieldDraft::new(key, label, original, draft);
        self.fields.insert(field.key.clone(), field);
        self.confirming_discard = false;
        cx.notify();
    }

    /// 清空全部字段草稿和放弃确认状态。
    ///
    /// 新建表单每次重新打开前通常调用该方法，编辑表单则随后写入当前资源值作为原值。
    pub fn reset_fields(&mut self, cx: &mut Context<Self>) {
        self.fields.clear();
        self.confirming_discard = false;
        cx.notify();
    }

    fn reset_validation_fields(&self, cx: &mut App) {
        for field in &self.validation_fields {
            field.reset(cx);
        }
    }

    /// 把当前全部草稿标记为已保存的新基线。
    ///
    /// 提交成功但对话框仍保持打开时调用本方法，后续取消不会把刚保存的字段误报为未保存。
    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        for field in self.fields.values_mut() {
            field.original.clone_from(&field.draft);
        }
        self.confirming_discard = false;
        cx.notify();
    }

    /// 返回任意字段是否存在尚未保存的修改。
    pub fn is_dirty(&self) -> bool {
        self.fields.values().any(FormFieldDraft::is_dirty)
    }

    /// 返回按稳定字段标识排序的全部未保存字段。
    pub fn unsaved_fields(&self) -> Vec<FormFieldDraft> {
        self.fields
            .values()
            .filter(|field| field.is_dirty())
            .cloned()
            .collect()
    }

    /// 返回全部字段的当前草稿值。
    ///
    /// 该快照适合自定义 `on_cancel` 记录草稿、生成恢复提示或交给业务层持久化；返回值按
    /// 稳定字段标识排序。
    pub fn draft_values(&self) -> BTreeMap<String, String> {
        self.fields
            .iter()
            .map(|(key, field)| (key.clone(), field.draft.clone()))
            .collect()
    }

    fn request_default_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        if self.is_dirty() {
            self.confirming_discard = true;
            cx.notify();
        } else {
            self.close(window, cx);
        }
    }

    fn continue_editing(&mut self, cx: &mut Context<Self>) {
        self.confirming_discard = false;
        cx.notify();
    }

    fn discard_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        self.fields.clear();
        self.reset_validation_fields(cx);
        self.close(window, cx);
    }

    /// 无条件关闭当前对话框并恢复打开前的焦点。
    ///
    /// 自定义取消处理器或提交成功处理器在已经自行处理草稿后可以调用本方法。默认取消路径
    /// 会先进行脏字段检查，因此普通表单不应直接用本方法绕过未保存确认。
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.submitting = false;
        self.confirming_discard = false;
        self.reset_validation_fields(cx);
        if let Some(handle) = self
            .previous_focus
            .take()
            .and_then(|handle| handle.upgrade())
        {
            handle.focus(window, cx);
        }
        cx.notify();
    }

    fn take_validation_tasks(&self, cx: &mut App) -> Vec<Task<()>> {
        self.validation_fields
            .iter()
            .filter_map(|field| field.take_pending_task(cx))
            .collect()
    }

    fn validate_registered_fields(&self, window: &mut Window, cx: &mut App) -> bool {
        let mut first_invalid = None;
        for field in &self.validation_fields {
            if !field.validate(cx) && first_invalid.is_none() {
                first_invalid = Some(field.clone());
            }
        }
        if let Some(field) = first_invalid {
            field.focus(window, cx);
            return false;
        }
        true
    }
}

/// 只覆盖当前 Feature Panel 的通用创建/编辑表单对话框。
///
/// 组件固定提供标题、可选描述、纵向可滚动内容区以及“取消/提交”操作。`on_submit` 是必需
/// 回调且没有默认业务实现；未设置自定义 `on_cancel` 时，组件使用
/// [`FormDialogState`] 的脏字段确认与关闭行为。
/// 字段布局直接使用 `gpui_component::form::Field`，由官方 `Form` 处理标签、说明、必填标记、
/// 多列与列跨度。Nexora 只保留 Panel 范围、草稿、校验任务、提交和取消流程。
#[derive(IntoElement)]
pub struct FormDialog {
    id: ElementId,
    state: Entity<FormDialogState>,
    title: Option<AnyElement>,
    description: Option<SharedString>,
    fields: Vec<Field>,
    sections: Vec<AnyElement>,
    columns: usize,
    size: Size,
    cancel_label: SharedString,
    submit_label: SharedString,
    submit_disabled: bool,
    max_panel_height_ratio: f32,
    on_cancel: Option<DialogHandler>,
    on_submit: Option<DialogHandler>,
}

impl FormDialog {
    /// 创建一个带默认取消/提交操作的表单对话框。
    ///
    /// `state` 必须是调用方长期持有的状态 Entity。调用方通过 [`Self::child`] 添加官方
    /// `Field`，通过 [`Self::section`] 插入角色列表等整行自定义区域。
    pub fn new(id: impl Into<ElementId>, state: Entity<FormDialogState>) -> Self {
        Self {
            id: id.into(),
            state,
            title: None,
            description: None,
            fields: Vec::new(),
            sections: Vec::new(),
            columns: 1,
            size: Size::default(),
            cancel_label: "取消".into(),
            submit_label: "提交".into(),
            submit_disabled: false,
            max_panel_height_ratio: DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO,
            on_cancel: None,
            on_submit: None,
        }
    }

    /// 设置对话框标题。
    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    /// 设置标题下方的辅助说明。
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置官方表单的列数。
    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// 添加一个官方表单字段。
    pub fn child(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// 添加一段自定义表单内容。
    ///
    /// 适合权限列表、角色列表、警告提示或其他不能自然表达为单个字段的内容。单个字段需要
    /// 跨越多列时，直接使用官方 `Field::col_span`。
    pub fn section(mut self, section: impl IntoElement) -> Self {
        self.sections.push(section.into_any_element());
        self
    }

    /// 设置默认取消按钮文案。
    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// 设置提交按钮文案。
    pub fn submit_label(mut self, label: impl Into<SharedString>) -> Self {
        self.submit_label = label.into();
        self
    }

    /// 设置提交按钮是否因业务条件禁用。
    ///
    /// 本设置不影响取消按钮；只有 [`FormDialogState::set_submitting`] 表示请求正在执行时，
    /// 取消、关闭和提交才会一起禁用。
    pub fn submit_disabled(mut self, disabled: bool) -> Self {
        self.submit_disabled = disabled;
        self
    }

    /// 设置表单对话框相对当前 Feature Panel 的最大高度比例。
    ///
    /// 默认值为 `0.8`，表示 surface 会由实际标题、内容和底部操作区撑开，但最高不超过
    /// 当前 Panel 可用高度的 80%。达到上限后，标题和底部操作区保持固定，仅表单内容区
    /// 纵向滚动。传入值会被限制在 `0.1..=1.0` 之间，避免对话框不可用或溢出 Panel。
    pub fn max_panel_height_ratio(mut self, ratio: f32) -> Self {
        self.max_panel_height_ratio = ratio.clamp(0.1, 1.0);
        self
    }

    /// 设置提交处理器。
    pub fn on_submit(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }

    /// 覆盖默认取消行为。
    ///
    /// 自定义处理器可以通过捕获同一个 `Entity<FormDialogState>` 查询 `is_dirty()`、
    /// `unsaved_fields()` 与 `draft_values()`，并在处理完成后显式调用
    /// [`FormDialogState::close`]。设置本回调后组件不会自动显示放弃确认。
    pub fn on_cancel(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }
}

impl gpui_component::Sizable for FormDialog {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for FormDialog {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let snapshot = self.state.read(cx);
        if !snapshot.is_open() {
            return div().into_any_element();
        }
        let submitting = snapshot.is_submitting();
        let confirming_discard = snapshot.confirming_discard;
        let unsaved_fields = snapshot.unsaved_fields();
        let focus_handle = snapshot.focus_handle.clone();
        let state_for_cancel = self.state.clone();
        let state_for_close_confirmation = self.state.clone();
        let state_for_stay = self.state.clone();
        let state_for_discard = self.state.clone();
        let custom_cancel = self.on_cancel.clone();
        let max_panel_height_ratio = self.max_panel_height_ratio;
        let cancel: DialogHandler = Rc::new(move |event, window, cx| {
            if let Some(handler) = custom_cancel.as_ref() {
                handler(event, window, cx);
            } else {
                state_for_cancel.update(cx, |state, cx| {
                    state.request_default_cancel(window, cx);
                });
            }
        });

        if confirming_discard {
            let rows = unsaved_fields.into_iter().map(|field| {
                v_flex()
                    .gap_1()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().tokens.group_box)
                    .child(div().text_sm().font_semibold().child(field.label().clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if field.draft().is_empty() {
                                "（空值）".to_owned()
                            } else {
                                field.draft().to_owned()
                            }),
                    )
            });
            return PanelDialog::new(self.id, focus_handle)
                .title("放弃未保存的更改？")
                .overlay_closable(false)
                .on_close(move |_, _, cx| {
                    state_for_close_confirmation.update(cx, FormDialogState::continue_editing);
                })
                .child(
                    v_flex()
                        .gap_2()
                        .child("以下字段仍有未保存的草稿。放弃后无法恢复：")
                        .children(rows),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("form-dialog-continue-editing")
                                .outline()
                                .label("继续编辑")
                                .on_click(move |_, _, cx| {
                                    state_for_stay.update(cx, FormDialogState::continue_editing);
                                }),
                        )
                        .child(
                            Button::new("form-dialog-discard")
                                .danger()
                                .label("放弃更改")
                                .on_click(move |_, window, cx| {
                                    state_for_discard.update(cx, |state, cx| {
                                        state.discard_and_close(window, cx);
                                    });
                                }),
                        ),
                )
                .w(px(520.0))
                .max_w(relative(0.92))
                .max_h(relative(max_panel_height_ratio))
                .into_any_element();
        }

        let title = v_flex().gap_1().children(self.title).when_some(
            self.description,
            |this, description| {
                this.child(
                    div()
                        .text_sm()
                        .font_normal()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            },
        );
        let has_submit_handler = self.on_submit.is_some();
        let submit_disabled = submitting || self.submit_disabled || !has_submit_handler;
        let on_submit = self.on_submit.unwrap_or_else(|| Rc::new(|_, _, _| {}));
        let state_for_submit = self.state.clone();
        let cancel_from_close = cancel.clone();
        let cancel_from_button = cancel;
        let size = self.size;
        let form = if self.fields.is_empty() {
            None
        } else {
            Some(
                v_form()
                    .columns(self.columns)
                    .children(self.fields)
                    .with_size(size)
                    .into_any_element(),
            )
        };
        let body = form.into_iter().chain(self.sections).collect::<Vec<_>>();

        let dialog = PanelDialog::new(self.id, focus_handle)
            .title(title)
            .overlay_closable(false)
            .on_close(move |event, window, cx| {
                cancel_from_close(event, window, cx);
            })
            .children(body)
            .footer(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("form-dialog-cancel")
                            .debug_selector(|| "form-dialog-cancel".into())
                            .outline()
                            .with_size(size)
                            .label(self.cancel_label)
                            .disabled(submitting)
                            .on_click(move |event, window, cx| {
                                cancel_from_button(event, window, cx);
                            }),
                    )
                    .child(
                        Button::new("form-dialog-submit")
                            .debug_selector(|| "form-dialog-submit".into())
                            .primary()
                            .with_size(size)
                            .label(self.submit_label)
                            .loading(submitting)
                            .disabled(submit_disabled)
                            .on_click(move |event, window, cx| {
                                let click_event = event.clone();
                                let on_submit = on_submit.clone();
                                let state_for_submit = state_for_submit.clone();
                                let tasks = state_for_submit.update(cx, |state, cx| {
                                    state.set_submitting(true, cx);
                                    state.take_validation_tasks(cx)
                                });
                                window
                                    .spawn(cx, async move |cx| {
                                        for task in tasks {
                                            task.await;
                                        }
                                        let _ = cx.update(move |window, cx| {
                                            let valid = state_for_submit.update(cx, |state, cx| {
                                                let valid =
                                                    state.validate_registered_fields(window, cx);
                                                state.set_submitting(false, cx);
                                                valid
                                            });
                                            if valid {
                                                on_submit(&click_event, window, cx);
                                            }
                                        });
                                    })
                                    // nexora-lint: allow(nexora::detached_lifecycle) reason=提交点击创建一次性窗口任务；任务只等待字段当前句柄并回到同一窗口验证，字段关闭会使旧结果失效。
                                    .detach();
                            }),
                    ),
            )
            .w(px(520.0))
            .max_w(relative(0.92))
            .max_h(relative(max_panel_height_ratio));

        dialog.into_any_element()
    }
}
