//! 带草稿追踪与未保存确认的内容区表单对话框。
//!
//! `FormDialogState` 保存打开状态、提交状态和字段草稿；`FormDialog` 负责统一渲染标题、
//! 描述、可滚动内容区以及取消/提交操作。组件组合 [`crate::PanelDialog`]，因此遮罩只覆盖
//! Feature 所属的 Panel，不会阻塞整个应用窗口和 Sidebar。

use std::{collections::BTreeMap, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FocusHandle, IntoElement,
    ParentElement as _, RenderOnce, SharedString, WeakFocusHandle, Window, div, prelude::*, px,
    relative,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputContentType, InputState, NumberInput},
    v_flex,
};

use crate::PanelDialog;

type DialogHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type CheckboxHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;
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
    open: bool,
    submitting: bool,
    confirming_discard: bool,
}

/// 标准表单项控件。
///
/// 该类型覆盖默认 CRUD 表单里最常见的输入控件，并在渲染时接收 [`Size`]，让上层
/// [`FormDialog`] 可以统一控制表单密度。复杂业务控件仍可通过 [`Self::element`] 直接传入
/// 自定义 GPUI 元素。
pub struct FormItemControl {
    kind: FormItemControlKind,
}

enum FormItemControlKind {
    Element(AnyElement),
    Input {
        state: Entity<InputState>,
        password: bool,
        disabled: bool,
    },
    NumberInput {
        state: Entity<InputState>,
        disabled: bool,
    },
    Checkbox {
        id: ElementId,
        checked: bool,
        disabled: bool,
        on_click: CheckboxHandler,
    },
}

impl FormItemControl {
    /// 使用自定义元素作为表单项控件。
    pub fn element(element: impl IntoElement) -> Self {
        Self {
            kind: FormItemControlKind::Element(element.into_any_element()),
        }
    }

    /// 使用官方文本输入框作为控件。
    pub fn input(state: &Entity<InputState>) -> Self {
        Self {
            kind: FormItemControlKind::Input {
                state: state.clone(),
                password: false,
                disabled: false,
            },
        }
    }

    /// 使用官方密码输入框作为控件。
    ///
    /// 该控件会启用密码切换按钮和语义化密码类型；如果需要默认隐藏内容，请在创建
    /// [`InputState`] 时调用 `masked(true)`。
    pub fn password_input(state: &Entity<InputState>) -> Self {
        Self {
            kind: FormItemControlKind::Input {
                state: state.clone(),
                password: true,
                disabled: false,
            },
        }
    }

    /// 使用官方数值输入框作为控件。
    pub fn number_input(state: &Entity<InputState>) -> Self {
        Self {
            kind: FormItemControlKind::NumberInput {
                state: state.clone(),
                disabled: false,
            },
        }
    }

    /// 使用官方复选框作为控件。
    pub fn checkbox(
        id: impl Into<ElementId>,
        checked: bool,
        on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            kind: FormItemControlKind::Checkbox {
                id: id.into(),
                checked,
                disabled: false,
                on_click: Rc::new(on_click),
            },
        }
    }

    /// 设置标准控件禁用状态；自定义元素需要在传入前自行处理禁用态。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self.kind {
            FormItemControlKind::Element(_) => {}
            FormItemControlKind::Input {
                disabled: current, ..
            }
            | FormItemControlKind::NumberInput {
                disabled: current, ..
            }
            | FormItemControlKind::Checkbox {
                disabled: current, ..
            } => *current = disabled,
        }
        self
    }

    fn render(self, size: Size, _window: &mut Window, _cx: &mut App) -> AnyElement {
        match self.kind {
            FormItemControlKind::Element(element) => element,
            FormItemControlKind::Input {
                state,
                password,
                disabled,
            } => {
                let input = Input::new(&state).with_size(size).disabled(disabled);
                if password {
                    input
                        .mask_toggle()
                        .content_type(InputContentType::Password)
                        .into_any_element()
                } else {
                    input.into_any_element()
                }
            }
            FormItemControlKind::NumberInput { state, disabled } => NumberInput::new(&state)
                .with_size(size)
                .disabled(disabled)
                .into_any_element(),
            FormItemControlKind::Checkbox {
                id,
                checked,
                disabled,
                on_click,
            } => Checkbox::new(id)
                .with_size(size)
                .checked(checked)
                .disabled(disabled)
                .on_click(move |checked, window, cx| {
                    on_click(checked, window, cx);
                })
                .into_any_element(),
        }
    }
}

/// 标准表单项。
///
/// `FormItem` 负责标签、说明、必填标记与控件的排列；控件可以用 [`FormItemControl`] 的常用
/// 构造器声明，也可以直接传入完全自定义元素。
#[derive(IntoElement)]
pub struct FormItem {
    label: SharedString,
    description: Option<SharedString>,
    required: bool,
    control: Option<FormItemControl>,
    size: Size,
}

impl FormItem {
    /// 创建一个带标签的表单项。
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            required: false,
            control: None,
            size: Size::default(),
        }
    }

    /// 设置表单项说明。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 标记该字段为必填。
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// 设置表单项控件。
    #[must_use]
    pub fn control(mut self, control: FormItemControl) -> Self {
        self.control = Some(control);
        self
    }

    /// 使用自定义元素作为控件。
    #[must_use]
    pub fn element(self, element: impl IntoElement) -> Self {
        self.control(FormItemControl::element(element))
    }

    /// 使用官方文本输入框作为控件。
    #[must_use]
    pub fn input(self, state: &Entity<InputState>) -> Self {
        self.control(FormItemControl::input(state))
    }

    /// 使用官方密码输入框作为控件。
    #[must_use]
    pub fn password_input(self, state: &Entity<InputState>) -> Self {
        self.control(FormItemControl::password_input(state))
    }

    /// 使用官方数值输入框作为控件。
    #[must_use]
    pub fn number_input(self, state: &Entity<InputState>) -> Self {
        self.control(FormItemControl::number_input(state))
    }

    /// 使用官方复选框作为控件。
    #[must_use]
    pub fn checkbox(
        self,
        id: impl Into<ElementId>,
        checked: bool,
        on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.control(FormItemControl::checkbox(id, checked, on_click))
    }

    /// 设置标准控件禁用状态；自定义元素需要在传入前自行处理禁用态。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.control = self.control.map(|control| control.disabled(disabled));
        self
    }
}

impl gpui_component::Sizable for FormItem {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for FormItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let control = self
            .control
            .map(|control| control.render(self.size, window, cx));

        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_sm().font_medium().child(self.label))
                    .when(self.required, |this| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child("*"))
                    }),
            )
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            })
            .children(control)
    }
}

impl FormDialogState {
    /// 创建关闭状态的表单对话框模型，并分配稳定焦点边界。
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            fields: BTreeMap::new(),
            open: false,
            submitting: false,
            confirming_discard: false,
        }
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
        if let Some(handle) = self
            .previous_focus
            .take()
            .and_then(|handle| handle.upgrade())
        {
            handle.focus(window, cx);
        }
        cx.notify();
    }
}

/// 只覆盖当前 Feature Panel 的通用创建/编辑表单对话框。
///
/// 组件固定提供标题、可选描述、纵向可滚动内容区以及“取消/提交”操作。`on_submit` 是必需
/// 回调且没有默认业务实现；未设置自定义 `on_cancel` 时，组件使用
/// [`FormDialogState`] 的脏字段确认与关闭行为。
#[derive(IntoElement)]
pub struct FormDialog {
    id: ElementId,
    state: Entity<FormDialogState>,
    title: Option<AnyElement>,
    description: Option<SharedString>,
    items: Vec<FormItem>,
    sections: Vec<AnyElement>,
    columns: usize,
    size: Size,
    cancel_label: SharedString,
    submit_label: SharedString,
    submit_disabled: bool,
    panel_height_ratio: Option<f32>,
    on_cancel: Option<DialogHandler>,
    on_submit: Option<DialogHandler>,
}

impl FormDialog {
    /// 创建一个带默认取消/提交操作的表单对话框。
    ///
    /// `state` 必须是调用方长期持有的状态 Entity。调用方可以通过 [`Self::child`] 添加
    /// [`FormItem`]，通过 [`Self::section`] 插入角色列表等自定义区域，并用 [`Self::on_submit`]
    /// 绑定提交逻辑。
    pub fn new(id: impl Into<ElementId>, state: Entity<FormDialogState>) -> Self {
        Self {
            id: id.into(),
            state,
            title: None,
            description: None,
            items: Vec::new(),
            sections: Vec::new(),
            columns: 1,
            size: Size::default(),
            cancel_label: "取消".into(),
            submit_label: "提交".into(),
            submit_disabled: false,
            panel_height_ratio: Some(DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO),
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

    /// 设置表单项列数。
    ///
    /// 列数只作用于通过 [`Self::child`] 添加的标准表单项；通过 [`Self::section`] 添加的自定义
    /// 区域始终占据整行。
    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// 添加一个标准表单项。
    pub fn child(mut self, item: FormItem) -> Self {
        self.items.push(item);
        self
    }

    /// 添加一段自定义表单内容。
    ///
    /// 适合权限列表、角色列表、警告提示或其他不能自然表达为单个字段的内容。
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

    /// 设置表单对话框相对当前 Feature Panel 的高度比例。
    ///
    /// 默认值为 `0.8`，表示常规创建/编辑表单 surface 高度固定为当前 Panel 可用高度的
    /// 80%，标题和底部操作区固定，表单内容区独立纵向滚动。传入值会被限制在 `0.1..=1.0`
    /// 之间，避免对话框不可用或溢出 Panel。
    pub fn panel_height_ratio(mut self, ratio: f32) -> Self {
        self.panel_height_ratio = Some(ratio.clamp(0.1, 1.0));
        self
    }

    /// 使用内容自适应高度。
    ///
    /// 该模式只适合字段极少的小表单；对话框仍会保留默认的 Panel 高度 80% 上限，字段过多时
    /// 继续由内容区纵向滚动。
    pub fn auto_height(mut self) -> Self {
        self.panel_height_ratio = None;
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
                .max_h(relative(DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO))
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
        let cancel_from_close = cancel.clone();
        let cancel_from_button = cancel;
        let size = self.size;
        let item_elements = self
            .items
            .into_iter()
            .map(|item| item.with_size(size).into_any_element())
            .collect::<Vec<_>>();
        let items = if item_elements.is_empty() {
            None
        } else if self.columns > 1 {
            Some(
                div()
                    .grid()
                    .grid_cols(self.columns as u16)
                    .gap_4()
                    .children(item_elements)
                    .into_any_element(),
            )
        } else {
            Some(v_flex().gap_4().children(item_elements).into_any_element())
        };
        let body = items.into_iter().chain(self.sections).collect::<Vec<_>>();

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
                                on_submit(event, window, cx);
                            }),
                    ),
            )
            .w(px(520.0))
            .max_w(relative(0.92));

        form_dialog_height(dialog, self.panel_height_ratio).into_any_element()
    }
}

fn form_dialog_height(dialog: PanelDialog, panel_height_ratio: Option<f32>) -> PanelDialog {
    match panel_height_ratio {
        Some(ratio) => dialog.h(relative(ratio)).max_h(relative(ratio)),
        None => dialog.max_h(relative(DEFAULT_FORM_DIALOG_PANEL_HEIGHT_RATIO)),
    }
}
