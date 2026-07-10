//! 桌面工作区主面板顶部栏。
//!
//! 该模块提供位于窗口级标签栏下方、业务内容上方的统一页面工具栏，用于承载面包屑和页面级操作。

use gpui::{AnyElement, App, IntoElement, RenderOnce, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, h_flex};

/// 带左侧导航信息和右侧操作区的主面板顶部栏。
///
/// 左侧通常传入 `gpui_component::breadcrumb::Breadcrumb`，右侧可以通过 `action` 或
/// `actions` 添加 `Button`、`Toggle`、下拉菜单等官方组件。顶部栏只负责布局和主题视觉，
/// 不保存业务状态，也不解释具体操作语义。
#[derive(IntoElement)]
pub struct PanelHeader {
    leading: AnyElement,
    actions: Vec<AnyElement>,
}

impl PanelHeader {
    /// 创建一个只包含左侧导航内容的主面板顶部栏。
    ///
    /// `leading` 推荐使用官方 `Breadcrumb`；在没有面包屑语义的页面中，也可以传入标题或其他
    /// 能表达当前位置的轻量元素。
    pub fn new(leading: impl IntoElement) -> Self {
        Self {
            leading: leading.into_any_element(),
            actions: Vec::new(),
        }
    }

    /// 向右侧操作区添加一个页面级操作。
    ///
    /// 该方法可以重复调用，操作会按照添加顺序从左到右排列。图标命令应优先使用
    /// `gpui_component::button::Button` 或 `Toggle`，并为纯图标操作提供 tooltip。
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// 向右侧操作区批量添加页面级操作。
    ///
    /// 该方法适合 feature 已经根据权限、选择状态或窗口尺寸生成一组操作的场景。
    pub fn actions(mut self, actions: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.actions
            .extend(actions.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for PanelHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let has_actions = !self.actions.is_empty();

        h_flex()
            .w_full()
            .h(px(48.0))
            .min_w_0()
            .flex_none()
            .justify_between()
            .gap_4()
            .px_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tokens.background)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.leading),
            )
            .when(has_actions, |this| {
                this.child(h_flex().flex_none().gap_1().children(self.actions))
            })
    }
}
