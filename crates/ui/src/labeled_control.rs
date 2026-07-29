//! 通用字段标签容器和表单字段状态。
//!
//! `LabeledControl<()>` 保留原有纯视觉模式，只负责“标签、可选说明、控件、可选错误”的纵向
//! 排列；`LabeledControl<V>` 是有状态字段 Entity，负责把 gpui-component 输入控件的原始
//! 状态转换为业务值 `V`，并保存声明式规则错误、异步事件错误和字段生命周期。

use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};

use gpui::{
    AnyElement, App, Context, ElementId, Entity, IntoElement, ParentElement as _, Pixels, Render,
    RenderOnce, SharedString, Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, Size,
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState, NumberInput},
    label::Label,
    v_flex,
};
use regex::Regex;

type EventFuture = Pin<Box<dyn Future<Output = ()>>>;
type FieldEventHandler<V> = Rc<dyn Fn(Event<V>) -> EventFuture>;
type ResetFormField = Rc<dyn Fn(&mut App)>;
type TakePendingFieldTask = Rc<dyn Fn(&mut App) -> Option<Task<()>>>;
type ValidateFormField = Rc<dyn Fn(&mut App) -> bool>;
type FocusFormField = Rc<dyn Fn(&mut Window, &mut App)>;

/// 字段事件发生时传给业务回调的类型化快照。
///
/// 泛型 `V` 是当前字段声明的业务值类型，例如 `SharedString`、`i64`、`f64`、`bool` 或
/// `Option<i64>`。事件持有发生当时的值快照，后续输入继续变化不会改变已经发出的事件。
pub struct Event<V> {
    value: V,
    current_target: LabeledControlTarget<V>,
}

impl<V> Event<V> {
    /// 返回事件发生时的业务值快照引用。
    pub fn value(&self) -> &V {
        &self.value
    }

    /// 消费事件并取出拥有型业务值快照。
    pub fn into_value(self) -> V {
        self.value
    }

    /// 返回当前字段目标，业务异步回调可用它设置或清除当前字段的事件错误。
    pub fn current_target(&self) -> &LabeledControlTarget<V> {
        &self.current_target
    }
}

/// 字段事件回调中用于更新当前字段事件错误的目标对象。
///
/// 目标对象只弱关联字段 Entity 和事件 revision。字段已销毁、对话框关闭或字段产生了更新
/// revision 后，回调结果会在应用阶段被丢弃，慢请求不会覆盖新值的错误状态。
pub struct LabeledControlTarget<V> {
    revision: u64,
    command: Rc<RefCell<EventCommand>>,
    field: WeakEntity<LabeledControl<V>>,
}

impl<V> Clone for LabeledControlTarget<V> {
    fn clone(&self) -> Self {
        Self {
            revision: self.revision,
            command: self.command.clone(),
            field: self.field.clone(),
        }
    }
}

impl<V: 'static> LabeledControlTarget<V> {
    /// 设置当前异步事件来源的错误消息。
    ///
    /// 该方法不需要 `cx`；消息会在事件 future 结束后回到字段 Entity 上应用。它只影响异步
    /// 事件错误来源，不会覆盖或清除声明式规则错误。
    pub fn set_error(&self, message: impl Into<SharedString>) {
        let _ = self.field.upgrade();
        *self.command.borrow_mut() = EventCommand::SetError(message.into());
    }

    /// 清除当前异步事件来源的错误消息。
    ///
    /// 该方法只清除事件错误来源；如果字段仍未通过 `required`、`pattern` 或数值转换规则，
    /// 展示错误会继续保留声明式规则的第一条失败消息。
    pub fn clear_error(&self) {
        let _ = self.field.upgrade();
        *self.command.borrow_mut() = EventCommand::ClearError;
    }
}

enum EventCommand {
    None,
    SetError(SharedString),
    ClearError,
}

/// 可注册到 `FormDialogState` 的类型擦除字段句柄。
///
/// 该类型只擦除表单聚合需要的生命周期、等待、校验和聚焦能力；字段事件本身仍由
/// `LabeledControl<V>` 的泛型 API 保持编译期类型安全。
#[derive(Clone)]
pub struct AnyFormField {
    key: SharedString,
    reset: ResetFormField,
    take_pending_task: TakePendingFieldTask,
    validate: ValidateFormField,
    focus: FocusFormField,
}

impl AnyFormField {
    /// 从一个有状态字段 Entity 创建可注册到表单对话框的擦除句柄。
    pub fn new<V: FieldValue>(field: &Entity<LabeledControl<V>>) -> Self {
        let key = SharedString::from(format!("field-{}", field.entity_id()));
        let field_for_reset = field.clone();
        let field_for_task = field.clone();
        let field_for_validate = field.clone();
        let field_for_focus = field.clone();
        Self {
            key,
            reset: Rc::new(move |cx| {
                field_for_reset.update(cx, |field, cx| field.reset_validation(cx));
            }),
            take_pending_task: Rc::new(move |cx| {
                field_for_task.update(cx, |field, _| field.take_pending_task())
            }),
            validate: Rc::new(move |cx| {
                field_for_validate.update(cx, |field, cx| field.validate_for_submit(cx))
            }),
            focus: Rc::new(move |window, cx| {
                field_for_focus.update(cx, |field, cx| field.focus(window, cx));
            }),
        }
    }

    /// 返回字段注册时使用的稳定标识。
    pub fn key(&self) -> &SharedString {
        &self.key
    }

    pub(crate) fn reset(&self, cx: &mut App) {
        (self.reset)(cx);
    }

    pub(crate) fn take_pending_task(&self, cx: &mut App) -> Option<Task<()>> {
        (self.take_pending_task)(cx)
    }

    pub(crate) fn validate(&self, cx: &mut App) -> bool {
        (self.validate)(cx)
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut App) {
        (self.focus)(window, cx);
    }
}

/// 通用的“标签 + 控件”视觉容器，或类型化表单字段 Entity。
///
/// `V` 表示有状态字段的业务值类型。纯视觉模式使用默认类型参数 `()`，通过
/// [`Self::new`] 构造；类型化字段通过 [`Self::input`]、[`Self::password_input`]、
/// [`Self::number_input`] 或 [`Self::checkbox`] 构造 builder，并在初始化阶段调用
/// `build(window, cx)` 得到长期持有的 Entity。
pub struct LabeledControl<V = ()> {
    inner: LabeledControlInner<V>,
}

enum LabeledControlInner<V> {
    Visual(VisualControl),
    Field(Box<FieldState<V>>),
}

struct VisualControl {
    label: SharedString,
    child: AnyElement,
    description: Option<SharedString>,
    required: bool,
    error: Option<SharedString>,
    width: Option<Pixels>,
    size: Size,
}

struct VisualRenderData {
    label: SharedString,
    child: AnyElement,
    description: Option<SharedString>,
    required: bool,
    error: Option<SharedString>,
    width: Option<Pixels>,
    size: Size,
}

struct FieldState<V> {
    key: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    control: FieldControl,
    rules: Vec<Rule>,
    parse_error: Option<SharedString>,
    value: Option<V>,
    committed_value: Option<V>,
    touched: bool,
    showing_error: bool,
    rule_error: Option<SharedString>,
    event_error: Option<SharedString>,
    revision: u64,
    on_input: Vec<FieldEventHandler<V>>,
    on_change: Vec<FieldEventHandler<V>>,
    on_blur: Vec<FieldEventHandler<V>>,
    pending_task: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
    size: Size,
}

enum FieldControl {
    Input {
        state: Entity<InputState>,
        password: bool,
    },
    NumberInput {
        state: Entity<InputState>,
    },
    Checkbox {
        id: ElementId,
        checked: bool,
    },
}

enum Rule {
    Required(SharedString),
    Pattern(Regex, SharedString),
}

/// 类型化字段构造器。
///
/// 构造器只在组件初始化阶段使用。调用 `build(window, cx)` 后会创建字段 Entity、订阅输入事件
/// 并保存异步任务生命周期；不要在 `render` 中重新 build 字段。
pub struct LabeledControlBuilder<V> {
    key: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    control: FieldControl,
    rules: Vec<Rule>,
    parse_error: Option<SharedString>,
    on_input: Vec<FieldEventHandler<V>>,
    on_change: Vec<FieldEventHandler<V>>,
    on_blur: Vec<FieldEventHandler<V>>,
}

impl LabeledControl<()> {
    /// 创建一个带必需标签和必需控件的纯视觉字段容器。
    ///
    /// `label` 渲染在控件上方，`child` 可以是任意 GPUI 元素。该模式不保存业务状态、不执行
    /// 声明式校验，也不会触发类型化事件。
    pub fn new(label: impl Into<SharedString>, child: impl IntoElement) -> Self {
        Self {
            inner: LabeledControlInner::Visual(VisualControl {
                label: label.into(),
                child: child.into_any_element(),
                description: None,
                required: false,
                error: None,
                width: None,
                size: Size::default(),
            }),
        }
    }

    /// 创建绑定 gpui-component `Input` 的文本字段构造器。
    pub fn input(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        state: &Entity<InputState>,
    ) -> LabeledControlBuilder<SharedString> {
        LabeledControlBuilder::new(
            key,
            label,
            FieldControl::Input {
                state: state.clone(),
                password: false,
            },
        )
    }

    /// 创建绑定 gpui-component `Input` 密码语义的文本字段构造器。
    pub fn password_input(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        state: &Entity<InputState>,
    ) -> LabeledControlBuilder<SharedString> {
        LabeledControlBuilder::new(
            key,
            label,
            FieldControl::Input {
                state: state.clone(),
                password: true,
            },
        )
    }

    /// 创建绑定 gpui-component `NumberInput` 的数值字段构造器。
    ///
    /// 数值控件内部仍使用 `InputState` 保存编辑中的原始文本；只有当前文本能转换为 `V` 时，
    /// 才会更新业务值并触发类型化事件。非空非法文本会在失焦或提交时显示 `parse_error`。
    pub fn number_input<V: NumberFieldValue>(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        state: &Entity<InputState>,
    ) -> LabeledControlBuilder<V> {
        LabeledControlBuilder::new(
            key,
            label,
            FieldControl::NumberInput {
                state: state.clone(),
            },
        )
    }

    /// 创建使用 gpui-component `Checkbox` 的布尔字段构造器。
    pub fn checkbox(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        id: impl Into<ElementId>,
        checked: bool,
    ) -> LabeledControlBuilder<bool> {
        LabeledControlBuilder::new(
            key,
            label,
            FieldControl::Checkbox {
                id: id.into(),
                checked,
            },
        )
    }
}

impl LabeledControl<()> {
    /// 设置标签和控件之间的辅助说明文本。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        if let LabeledControlInner::Visual(visual) = &mut self.inner {
            visual.description = Some(description.into());
        }
        self
    }

    /// 在标签旁显示必填星号。
    #[must_use]
    pub fn required(mut self) -> Self {
        if let LabeledControlInner::Visual(visual) = &mut self.inner {
            visual.required = true;
        }
        self
    }

    /// 设置控件下方的错误文本。
    #[must_use]
    pub fn error(mut self, error: impl Into<SharedString>) -> Self {
        if let LabeledControlInner::Visual(visual) = &mut self.inner {
            visual.error = Some(error.into());
        }
        self
    }

    /// 设置容器固定像素宽度。
    #[must_use]
    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        if let LabeledControlInner::Visual(visual) = &mut self.inner {
            visual.width = Some(width.into());
        }
        self
    }
}

impl gpui_component::Sizable for LabeledControl<()> {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        if let LabeledControlInner::Visual(visual) = &mut self.inner {
            visual.size = size.into();
        }
        self
    }
}

impl IntoElement for LabeledControl<()> {
    type Element = gpui::ViewElement<Self>;

    fn into_element(self) -> Self::Element {
        gpui::ViewElement::new(self)
    }
}

impl<V: Clone + PartialEq + 'static> LabeledControlBuilder<V> {
    fn new(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        control: FieldControl,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: None,
            control,
            rules: Vec::new(),
            parse_error: None,
            on_input: Vec::new(),
            on_change: Vec::new(),
            on_blur: Vec::new(),
        }
    }

    /// 设置字段说明文本。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 添加必填规则。
    ///
    /// 文本值会在 `trim()` 后判断空值，`bool` 需要为 `true`，`Option<V>` 需要为 `Some`。
    /// 规则按声明顺序执行，并只展示第一条失败消息。
    #[must_use]
    pub fn required(mut self, message: impl Into<SharedString>) -> Self {
        self.rules.push(Rule::Required(message.into()));
        self
    }

    /// 添加正则规则，校验字段当前文本表示。
    ///
    /// `regex` 使用 Rust `regex` 语法。无法编译的表达式会在构造阶段 panic，以便应用在开发
    /// 阶段尽早发现错误配置。
    ///
    /// # Panics
    ///
    /// 当 `regex` 不是合法的 Rust 正则表达式时 panic。表单规则通常在初始化阶段声明，panic
    /// 用于暴露开发期配置错误，而不是处理用户输入。
    #[must_use]
    pub fn pattern(mut self, regex: &str, message: impl Into<SharedString>) -> Self {
        self.rules.push(Rule::Pattern(
            Regex::new(regex).expect("LabeledControl::pattern 应收到合法 regex"),
            message.into(),
        ));
        self
    }

    /// 设置数值字段转换失败时展示的错误消息。
    ///
    /// 空值且存在 `required` 失败时会优先显示必填错误，不会显示转换错误。
    #[must_use]
    pub fn parse_error(mut self, message: impl Into<SharedString>) -> Self {
        self.parse_error = Some(message.into());
        self
    }

    /// 注册用户每次可转换编辑时触发的类型化事件。
    #[must_use]
    pub fn on_input<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Event<V>) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        self.on_input
            .push(Rc::new(move |event| Box::pin(handler(event))));
        self
    }

    /// 注册字段产生提交性变化时触发的类型化事件。
    #[must_use]
    pub fn on_change<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Event<V>) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        self.on_change
            .push(Rc::new(move |event| Box::pin(handler(event))));
        self
    }

    /// 注册字段每次失焦时触发的类型化事件。
    #[must_use]
    pub fn on_blur<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Event<V>) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        self.on_blur
            .push(Rc::new(move |event| Box::pin(handler(event))));
        self
    }

    /// 在当前组件初始化阶段创建字段 Entity 和输入订阅。
    pub fn build<T: 'static>(
        self,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<LabeledControl<V>>
    where
        V: FieldValue,
    {
        cx.new(|field_cx| {
            let mut field = LabeledControl {
                inner: LabeledControlInner::Field(Box::new(FieldState {
                    key: self.key,
                    label: self.label,
                    description: self.description,
                    control: self.control,
                    rules: self.rules,
                    parse_error: self.parse_error,
                    value: None,
                    committed_value: None,
                    touched: false,
                    showing_error: false,
                    rule_error: None,
                    event_error: None,
                    revision: 0,
                    on_input: self.on_input,
                    on_change: self.on_change,
                    on_blur: self.on_blur,
                    pending_task: None,
                    subscriptions: Vec::new(),
                    size: Size::default(),
                })),
            };
            field.initialize_value(field_cx);
            field.subscribe_input(window, field_cx);
            field
        })
    }
}

impl<V: FieldValue> LabeledControl<V> {
    /// 返回字段稳定标识。
    pub fn key(&self) -> Option<&SharedString> {
        self.field().map(|field| &field.key)
    }

    /// 返回最后一次成功转换得到的业务值。
    ///
    /// 数值字段当前原始文本非法时，该值仍是最后一次成功值；提交校验会额外检查当前原始
    /// 文本，不能用旧业务值绕过转换错误。
    pub fn value(&self) -> Option<&V> {
        self.field().and_then(|field| field.value.as_ref())
    }

    /// 返回当前应展示的错误消息，声明式规则错误优先于异步事件错误。
    pub fn visible_error(&self) -> Option<&SharedString> {
        self.field()
            .and_then(|field| field.rule_error.as_ref().or(field.event_error.as_ref()))
    }

    /// 返回字段是否存在任一来源的错误。
    pub fn has_error(&self) -> bool {
        self.visible_error().is_some()
    }

    fn field(&self) -> Option<&FieldState<V>> {
        match &self.inner {
            LabeledControlInner::Field(field) => Some(field),
            LabeledControlInner::Visual(_) => None,
        }
    }

    fn field_mut(&mut self) -> Option<&mut FieldState<V>> {
        match &mut self.inner {
            LabeledControlInner::Field(field) => Some(field),
            LabeledControlInner::Visual(_) => None,
        }
    }

    fn initialize_value(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.field_mut() else {
            return;
        };
        let raw = field.raw_text(cx);
        if let Ok(value) = V::parse_field_value(&raw, field.control.is_checkbox()) {
            field.value = Some(value.clone());
            field.committed_value = Some(value);
        }
    }

    fn subscribe_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.field_mut() else {
            return;
        };
        let (FieldControl::Input { state, .. } | FieldControl::NumberInput { state }) =
            &field.control
        else {
            return;
        };
        let state = state.clone();
        field.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.handle_input_event(window, cx),
                InputEvent::Blur => this.handle_blur_event(window, cx),
                InputEvent::Focus | InputEvent::PressEnter { .. } => {}
            },
        ));
    }

    fn handle_input_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.field_mut() else {
            return;
        };
        field.revision += 1;
        let parsed = field.parse_current(cx);
        if let Ok(value) = parsed {
            field.value = Some(value.clone());
            field.run_handlers(EventKind::Input, value, window, cx);
        }
        if field.showing_error {
            field.validate_rules(cx);
        }
        cx.notify();
    }

    fn handle_blur_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.field_mut() else {
            return;
        };
        field.touched = true;
        field.showing_error = true;
        field.validate_rules(cx);
        if let Some(value) = field.value.clone() {
            if field.committed_value.as_ref() != Some(&value) {
                field.committed_value = Some(value.clone());
                field.run_handlers(EventKind::Change, value.clone(), window, cx);
            }
            field.run_handlers(EventKind::Blur, value, window, cx);
        }
        cx.notify();
    }

    fn validate_for_submit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.field_mut() else {
            return true;
        };
        field.touched = true;
        field.showing_error = true;
        field.validate_rules(cx);
        cx.notify();
        field.rule_error.is_none() && field.event_error.is_none()
    }

    fn reset_validation(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.field_mut() else {
            return;
        };
        field.revision += 1;
        field.pending_task = None;
        field.touched = false;
        field.showing_error = false;
        field.rule_error = None;
        field.event_error = None;
        cx.notify();
    }

    fn take_pending_task(&mut self) -> Option<Task<()>> {
        self.field_mut().and_then(|field| field.pending_task.take())
    }

    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.field() else {
            return;
        };
        match &field.control {
            FieldControl::Input { state, .. } | FieldControl::NumberInput { state } => {
                state.update(cx, |state, cx| state.focus(window, cx));
            }
            FieldControl::Checkbox { .. } => {}
        }
    }
}

impl<V: FieldValue> FieldState<V> {
    fn raw_text(&self, cx: &App) -> SharedString {
        match &self.control {
            FieldControl::Input { state, .. } | FieldControl::NumberInput { state } => {
                state.read(cx).value()
            }
            FieldControl::Checkbox { checked, .. } => checked.to_string().into(),
        }
    }

    fn parse_current(&self, cx: &App) -> Result<V, FieldValueParseError> {
        V::parse_field_value(&self.raw_text(cx), self.control.is_checkbox())
    }

    fn validate_rules(&mut self, cx: &App) {
        let raw = self.raw_text(cx);
        self.rule_error = None;
        let parsed = V::parse_field_value(&raw, self.control.is_checkbox());

        for rule in &self.rules {
            match rule {
                Rule::Required(message) if !V::is_present(&raw, parsed.as_ref().ok()) => {
                    self.rule_error = Some(message.clone());
                    return;
                }
                Rule::Required(_) => {}
                Rule::Pattern(regex, message)
                    if !raw.is_empty() && !regex.is_match(raw.as_ref()) =>
                {
                    self.rule_error = Some(message.clone());
                    return;
                }
                Rule::Pattern(_, _) => {}
            }
        }

        if parsed.is_err() && !raw.trim().is_empty() {
            self.rule_error = self.parse_error.clone();
            return;
        }

        if let Ok(value) = parsed {
            self.value = Some(value);
        }
    }

    fn run_handlers(
        &mut self,
        kind: EventKind,
        value: V,
        _window: &mut Window,
        cx: &mut Context<LabeledControl<V>>,
    ) {
        let handlers = match kind {
            EventKind::Input => self.on_input.clone(),
            EventKind::Change => self.on_change.clone(),
            EventKind::Blur => self.on_blur.clone(),
        };
        if handlers.is_empty() {
            return;
        }

        let revision = self.revision;
        let task = cx.spawn(async move |field: WeakEntity<LabeledControl<V>>, cx| {
            let command = Rc::new(RefCell::new(EventCommand::None));
            for handler in handlers {
                let target = LabeledControlTarget {
                    revision,
                    command: command.clone(),
                    field: field.clone(),
                };
                handler(Event {
                    value: value.clone(),
                    current_target: target,
                })
                .await;
            }
            let command = command.replace(EventCommand::None);
            let _ = field.update(cx, |field, cx| {
                let Some(field) = field.field_mut() else {
                    return;
                };
                if field.revision != revision {
                    return;
                }
                match command {
                    EventCommand::None => {}
                    EventCommand::SetError(message) => field.event_error = Some(message),
                    EventCommand::ClearError => field.event_error = None,
                }
                cx.notify();
            });
        });
        self.pending_task = Some(task);
    }
}

impl FieldControl {
    fn is_checkbox(&self) -> bool {
        matches!(self, Self::Checkbox { .. })
    }
}

enum EventKind {
    Input,
    Change,
    Blur,
}

/// 字段原始文本无法转换为业务类型时返回的轻量错误。
///
/// 该错误不包含用户可见消息；表单展示文案由应用通过 `parse_error` 等规则显式传入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldValueParseError;

/// 字段值解析和必填判定的公共边界。
///
/// 应用通常不需要直接实现该 trait；首期内置支持文本、布尔、整数、浮点数和可选数值。
pub trait FieldValue: Clone + PartialEq + 'static {
    /// 从控件当前原始文本转换为业务类型。
    ///
    /// # Errors
    ///
    /// 当当前原始文本不能表示目标业务类型时返回 [`FieldValueParseError`]。调用方应使用
    /// 字段构造器上的 `parse_error` 提供用户可见消息，不应直接向用户展示该错误类型。
    fn parse_field_value(raw: &SharedString, checkbox: bool) -> Result<Self, FieldValueParseError>;

    /// 判断当前值是否满足必填语义。
    fn is_present(raw: &SharedString, value: Option<&Self>) -> bool;
}

impl FieldValue for SharedString {
    fn parse_field_value(raw: &SharedString, _: bool) -> Result<Self, FieldValueParseError> {
        Ok(raw.clone())
    }

    fn is_present(raw: &SharedString, _: Option<&Self>) -> bool {
        !raw.trim().is_empty()
    }
}

impl FieldValue for bool {
    fn parse_field_value(raw: &SharedString, checkbox: bool) -> Result<Self, FieldValueParseError> {
        if checkbox {
            Ok(raw.as_ref() == "true")
        } else {
            raw.as_ref().parse().map_err(|_| FieldValueParseError)
        }
    }

    fn is_present(_: &SharedString, value: Option<&Self>) -> bool {
        value.copied().unwrap_or(false)
    }
}

impl FieldValue for i64 {
    fn parse_field_value(raw: &SharedString, _: bool) -> Result<Self, FieldValueParseError> {
        raw.trim().parse().map_err(|_| FieldValueParseError)
    }

    fn is_present(raw: &SharedString, _: Option<&Self>) -> bool {
        !raw.trim().is_empty()
    }
}

impl FieldValue for f64 {
    fn parse_field_value(raw: &SharedString, _: bool) -> Result<Self, FieldValueParseError> {
        raw.trim().parse().map_err(|_| FieldValueParseError)
    }

    fn is_present(raw: &SharedString, _: Option<&Self>) -> bool {
        !raw.trim().is_empty()
    }
}

impl FieldValue for Option<i64> {
    fn parse_field_value(raw: &SharedString, _: bool) -> Result<Self, FieldValueParseError> {
        let raw = raw.trim();
        if raw.is_empty() {
            Ok(None)
        } else {
            raw.parse().map(Some).map_err(|_| FieldValueParseError)
        }
    }

    fn is_present(_: &SharedString, value: Option<&Self>) -> bool {
        value.copied().flatten().is_some()
    }
}

impl FieldValue for Option<f64> {
    fn parse_field_value(raw: &SharedString, _: bool) -> Result<Self, FieldValueParseError> {
        let raw = raw.trim();
        if raw.is_empty() {
            Ok(None)
        } else {
            raw.parse().map(Some).map_err(|_| FieldValueParseError)
        }
    }

    fn is_present(_: &SharedString, value: Option<&Self>) -> bool {
        value.copied().flatten().is_some()
    }
}

/// 可由 `NumberInput` 转换出的首期业务数值类型。
pub trait NumberFieldValue: FieldValue {}

impl NumberFieldValue for i64 {}
impl NumberFieldValue for f64 {}
impl NumberFieldValue for Option<i64> {}
impl NumberFieldValue for Option<f64> {}

impl RenderOnce for LabeledControl<()> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        match self.inner {
            LabeledControlInner::Visual(visual) => render_visual_control(
                VisualRenderData {
                    label: visual.label,
                    child: visual.child,
                    description: visual.description,
                    required: visual.required,
                    error: visual.error,
                    width: visual.width,
                    size: visual.size,
                },
                cx,
            ),
            LabeledControlInner::Field(_) => div().into_any_element(),
        }
    }
}

impl<V: FieldValue> Render for LabeledControl<V> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(field) = self.field_mut() else {
            return div().into_any_element();
        };
        let size = field.size;
        let has_error = field.rule_error.is_some() || field.event_error.is_some();
        let control = match &mut field.control {
            FieldControl::Input { state, password } => {
                let input = Input::new(state).with_size(size).bordered(true);
                let input = if *password {
                    input.mask_toggle().content_type(InputContentType::Password)
                } else {
                    input
                };
                input
                    .when(has_error, |this| this.border_color(cx.theme().danger))
                    .into_any_element()
            }
            FieldControl::NumberInput { state } => NumberInput::new(state)
                .with_size(size)
                .when(has_error, |this| this.border_color(cx.theme().danger))
                .into_any_element(),
            FieldControl::Checkbox { id, checked } => {
                let id = id.clone();
                Checkbox::new(id)
                    .with_size(size)
                    .checked(*checked)
                    .on_click(cx.listener(|this, checked, window, cx| {
                        if let Some(field) = this.field_mut() {
                            if let FieldControl::Checkbox { checked: state, .. } =
                                &mut field.control
                            {
                                *state = *checked;
                            }
                            field.revision += 1;
                            let value = *checked;
                            if let Ok(value) = V::parse_field_value(&value.to_string().into(), true)
                            {
                                field.value = Some(value.clone());
                                field.committed_value = Some(value.clone());
                                field.run_handlers(EventKind::Input, value.clone(), window, cx);
                                field.run_handlers(EventKind::Change, value, window, cx);
                            }
                            if field.showing_error {
                                field.validate_rules(cx);
                            }
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }
        };
        let error = field
            .rule_error
            .clone()
            .or_else(|| field.event_error.clone());
        render_visual_control(
            VisualRenderData {
                label: field.label.clone(),
                child: control,
                description: field.description.clone(),
                required: field
                    .rules
                    .iter()
                    .any(|rule| matches!(rule, Rule::Required(_))),
                error,
                width: None,
                size,
            },
            cx,
        )
    }
}

fn render_visual_control(data: VisualRenderData, cx: &mut App) -> AnyElement {
    v_flex()
        .debug_selector(|| "labeled-control".into())
        .w_full()
        .min_w_0()
        .map(|this| match data.size {
            Size::XSmall => this.gap_0p5(),
            Size::Large => this.gap_2(),
            Size::Small | Size::Medium | Size::Size(_) => this.gap_1(),
        })
        .when_some(data.width, |this, width| this.w(width))
        .child(
            h_flex()
                .debug_selector(|| "labeled-control-label".into())
                .gap_1()
                .child(
                    Label::new(data.label)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground),
                )
                .when(data.required, |this| {
                    this.child(
                        div()
                            .debug_selector(|| "labeled-control-required".into())
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child("*"),
                    )
                }),
        )
        .when_some(data.description, |this, description| {
            this.child(
                div()
                    .debug_selector(|| "labeled-control-description".into())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
        })
        .child(data.child)
        .when_some(data.error, |this, error| {
            this.child(
                div()
                    .debug_selector(|| "labeled-control-error".into())
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error),
            )
        })
        .into_any_element()
}
