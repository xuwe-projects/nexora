//! 数据表布尔字段的显式受控开关单元格。

use std::rc::Rc;

use gpui::{App, ElementId, IntoElement, RenderOnce, Window, prelude::*};
use gpui_component::{
    Disableable as _, Sizable as _, Size, h_flex, spinner::Spinner, switch::Switch,
};

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// 使用官方 [`Switch`] 展示并提交异步布尔更新的表格单元格。
///
/// 本组件不持有乐观值、权限或请求任务；业务 Entity 必须传入当前服务端确认值、权限结果与
/// loading 状态，并在回调失败后继续渲染原确认值。这样失败恢复、并发门禁和生命周期仍由最
/// 接近真实业务请求的状态所有者管理。
#[derive(IntoElement)]
pub struct TableSwitchCell {
    id: ElementId,
    checked: bool,
    allowed: bool,
    loading: bool,
    on_change: Option<ChangeHandler>,
    size: Size,
}

impl TableSwitchCell {
    /// 创建一个默认不可编辑的受控开关单元格。
    ///
    /// 调用方必须同时设置 [`Self::allowed`] 与 [`Self::on_change`] 才能产生更新意图；未显
    /// 式绑定权限和回调时只展示当前值。
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            allowed: false,
            loading: false,
            on_change: None,
            size: Size::Small,
        }
    }

    /// 设置当前操作者是否有权修改该字段。
    #[must_use]
    pub fn allowed(mut self, allowed: bool) -> Self {
        self.allowed = allowed;
        self
    }

    /// 设置业务异步更新是否正在执行。
    ///
    /// loading 时开关保持受控值并禁用，旁边使用官方 [`Spinner`] 显示进度。
    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// 注册目标值变化回调。
    ///
    /// 回调只报告用户意图，不会修改本组件值。业务应启动受其 Entity 管理的请求；成功后更
    /// 新确认值，失败后清除 loading 并保留原值，同时展示业务错误。
    #[must_use]
    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// 设置官方 Switch 和 Spinner 的组件尺寸。
    #[must_use]
    pub fn size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for TableSwitchCell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let disabled = !self.allowed || self.loading || self.on_change.is_none();
        let size = self.size;
        let switch = Switch::new(self.id)
            .checked(self.checked)
            .disabled(disabled)
            .with_size(size)
            .when_some(self.on_change, |this, on_change| {
                this.on_click(move |checked, window, cx| {
                    on_change(*checked, window, cx);
                })
            });

        h_flex()
            .items_center()
            .gap_1()
            .child(switch)
            .when(self.loading, |this| {
                this.child(Spinner::new().with_size(size))
            })
    }
}
