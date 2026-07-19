//! 受桌面工作区右侧 Panel 边界约束的模态对话框。
//!
//! 与窗口级 `gpui_component::dialog::Dialog` 不同，本组件由 Panel 宿主直接渲染，
//! 遮罩只覆盖最近的相对定位父容器。业务 Feature 负责持有打开状态、输入状态和副作用，
//! `PanelDialog` 只提供统一的视觉、焦点约束和关闭交互。

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, RenderOnce, Role, StyleRefinement, Styled, Window, div, prelude::*,
    px, relative,
};
use gpui_component::{
    ActiveTheme as _, FocusTrapElement as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    dialog::CancelDialog,
    h_flex, v_flex,
};

type CloseHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 只覆盖当前工作区 Panel 的模态对话框。
///
/// 调用方应把本组件渲染为 Panel 相对定位容器的最后一个子元素，并在打开时把传入的
/// `FocusHandle` 聚焦。组件会阻止鼠标事件穿透遮罩、把 Tab 焦点限制在对话框内，并复用
/// `gpui-component` 的 Dialog 键盘上下文处理 Escape 关闭。
#[derive(IntoElement)]
pub struct PanelDialog {
    id: ElementId,
    focus_handle: FocusHandle,
    style: StyleRefinement,
    title: Option<AnyElement>,
    footer: Option<AnyElement>,
    children: Vec<AnyElement>,
    overlay_closable: bool,
    on_close: CloseHandler,
}

impl PanelDialog {
    /// 创建一个带稳定元素 ID 和焦点边界的 Panel 对话框。
    ///
    /// `id` 用于区分同一窗口中的不同对话框，`focus_handle` 应由持有打开状态的 Feature
    /// 在构造阶段创建，从而在标签切换期间保持稳定。
    pub fn new(id: impl Into<ElementId>, focus_handle: FocusHandle) -> Self {
        Self {
            id: id.into(),
            focus_handle,
            style: StyleRefinement::default(),
            title: None,
            footer: None,
            children: Vec::new(),
            overlay_closable: false,
            on_close: Rc::new(|_, _, _| {}),
        }
    }

    /// 设置对话框标题区内容。
    ///
    /// 标题可以是普通文本，也可以是包含图标、状态标签等元素的复合布局。
    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    /// 设置固定在内容滚动区下方的操作区。
    ///
    /// 调用方通常在这里放置取消与确认按钮；业务提交行为仍由调用方管理。
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// 设置点击遮罩时是否触发关闭，默认禁用。
    ///
    /// 即使关闭该选项，遮罩仍会拦截鼠标事件，避免操作穿透到当前 Panel 的业务内容。
    pub fn overlay_closable(mut self, overlay_closable: bool) -> Self {
        self.overlay_closable = overlay_closable;
        self
    }

    /// 注册关闭意图处理器。
    ///
    /// 点击关闭按钮、点击可关闭遮罩或按下 Escape 都会调用该处理器；组件本身不保存
    /// 打开状态，调用方应在处理器中更新所属 Feature 或子 Entity。
    pub fn on_close(
        mut self,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Rc::new(on_close);
        self
    }
}

impl ParentElement for PanelDialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for PanelDialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PanelDialog {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let close_from_button = self.on_close.clone();
        let close_from_keyboard = self.on_close.clone();
        let close_from_overlay = self.on_close.clone();
        let overlay_closable = self.overlay_closable;

        let surface = v_flex()
            .id("panel-dialog-surface")
            .debug_selector(|| "panel-dialog-surface".into())
            .relative()
            .flex_none()
            .role(Role::Dialog)
            .key_context("Dialog")
            .w(px(480.0))
            .h_auto()
            .max_w(relative(0.9))
            .min_h(px(180.0))
            .overflow_hidden()
            .bg(cx.theme().tokens.background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .shadow_xl()
            .refine_style(&self.style)
            .on_action(move |_: &CancelDialog, window, cx| {
                close_from_keyboard(&ClickEvent::default(), window, cx);
            })
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_semibold()
                            .children(self.title),
                    )
                    .child(
                        Button::new("panel-dialog-close")
                            .debug_selector(|| "panel-dialog-close".into())
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("关闭")
                            .on_click(move |event, window, cx| {
                                close_from_button(event, window, cx);
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .min_h_0()
                    .debug_selector(|| "panel-dialog-content".into())
                    .gap_4()
                    .p_4()
                    .children(self.children),
            )
            .when_some(self.footer, |this, footer| {
                this.child(
                    h_flex()
                        .flex_none()
                        .justify_end()
                        .gap_2()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(footer),
                )
            })
            .focus_trap("panel-dialog-focus-trap", &self.focus_handle);

        div()
            .id(self.id)
            .debug_selector(|| "panel-dialog-overlay".into())
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .occlude()
            .bg(cx.theme().overlay)
            .when(overlay_closable, |this| {
                this.on_any_mouse_down(move |event, window, cx| {
                    cx.stop_propagation();
                    if event.button == MouseButton::Left {
                        close_from_overlay(&ClickEvent::default(), window, cx);
                    }
                })
            })
            .child(surface)
    }
}
