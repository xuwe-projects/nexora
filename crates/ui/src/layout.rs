//! 共享桌面应用布局组件。
//!
//! 该模块提供跨桌面应用复用的工作区结构，业务应用只需要传入导航、全局顶栏、标签栏和主面板。

use gpui::{AnyElement, Context, IntoElement, Pixels, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, TitleBar, scroll::ScrollableElement as _};

/// 工作区右侧全局栏的固定高度，与视觉原型保持一致。
pub const WORKSPACE_GLOBAL_BAR_HEIGHT: Pixels = px(44.0);

/// 工作区右侧 Feature 标签栏的固定高度，与视觉原型保持一致。
pub const WORKSPACE_TAB_BAR_HEIGHT: Pixels = px(42.0);

/// 带窗口顶部栏和侧边导航的桌面工作区布局。
///
/// 该组件负责创建官方 `TitleBar`，并组织“整窗高左侧导航 + 右侧窗口顶部栏 + 主内容区”的桌面应用结构。
/// 它统一处理 macOS 全屏时的标题栏占位和内容滚动，但不保存业务导航状态，
/// 也不理解具体 feature、标签页或菜单语义。
pub struct WorkspaceLayout {
    sidebar: AnyElement,
    title_bar_content: AnyElement,
    tab_bar_content: AnyElement,
    panel_overlay: Option<AnyElement>,
    content: AnyElement,
    content_padding: Pixels,
    content_scrollable: bool,
}

impl WorkspaceLayout {
    /// 创建一个使用默认尺寸和间距的桌面工作区布局。
    ///
    /// `sidebar` 通常应由 `gpui-component` 的 `Sidebar` 及其子组件构成；
    /// `title_bar_content` 是插入官方 `TitleBar` 的全局搜索和窗口工具区；
    /// `tab_bar_content` 是位于右侧工作区顶部的 Feature 标签栏；
    /// `content` 是当前 feature 的主面板内容。
    pub fn new(
        sidebar: impl IntoElement,
        title_bar_content: impl IntoElement,
        tab_bar_content: impl IntoElement,
        content: impl IntoElement,
    ) -> Self {
        Self {
            sidebar: sidebar.into_any_element(),
            title_bar_content: title_bar_content.into_any_element(),
            tab_bar_content: tab_bar_content.into_any_element(),
            panel_overlay: None,
            content: content.into_any_element(),
            content_padding: px(24.0),
            content_scrollable: true,
        }
    }

    /// 设置只覆盖右侧主面板的浮层。
    ///
    /// 浮层会在标签栏与业务内容之后渲染，并受右侧主面板边界裁剪，
    /// 因此不会覆盖窗口级标签栏或左侧导航。通常传入共享 [`crate::PanelDialog`]。
    pub fn with_panel_overlay(mut self, panel_overlay: impl IntoElement) -> Self {
        self.panel_overlay = Some(panel_overlay.into_any_element());
        self
    }

    /// 设置主内容区的内边距。
    ///
    /// 该值作用于内容滚动容器四周，适合不同应用在保持统一壳结构的同时调整内容密度。
    pub fn with_content_padding(mut self, padding: Pixels) -> Self {
        self.content_padding = padding;
        self
    }

    /// 设置主内容区是否由应用壳提供纵向滚动。
    ///
    /// 普通 feature 页面通常应该保持该值为 `true`，让页面内容超过窗口高度时可以继续向下滚动；
    /// 像 `DataTable`、编辑器或虚拟列表这类内部组件自己管理滚动时，可以设置为 `false`，避免出现双层滚动。
    pub fn with_content_scrollable(mut self, scrollable: bool) -> Self {
        self.content_scrollable = scrollable;
        self
    }

    /// 返回主内容区当前使用的内边距。
    ///
    /// 该方法用于让调用方和集成测试确认共享布局的密度配置。
    pub fn content_padding(&self) -> Pixels {
        self.content_padding
    }

    /// 返回主内容区是否由应用壳提供纵向滚动。
    ///
    /// 返回 `true` 表示普通页面内容会随应用壳滚动；返回 `false` 表示滚动行为应由内容内部组件自行处理。
    pub fn content_scrollable(&self) -> bool {
        self.content_scrollable
    }

    /// 返回右侧主面板是否配置了局部浮层。
    ///
    /// 该值用于调用方和集成测试确认临时界面是否挂载在 Panel 层级，而不是窗口根遮罩层。
    pub fn has_panel_overlay(&self) -> bool {
        self.panel_overlay.is_some()
    }

    /// 将桌面工作区渲染为 GPUI 元素树。
    ///
    /// 返回元素包含固定的桌面工作区结构：Sidebar 从窗口顶边延伸到底边，右侧工作区依次放置
    /// 44px 官方窗口顶部栏、42px Feature 标签栏和剩余主内容区域。主内容可以按 feature 需要
    /// 开启或关闭外层滚动；Sidebar 自己负责展开、折叠与动画。颜色和背景读取当前
    /// `gpui-component` 主题，避免业务应用重复处理平台差异或写死视觉样式。
    pub fn render<T>(self, _window: &mut Window, cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let background = cx.theme().tokens.background;
        let foreground = cx.theme().foreground;
        let Self {
            sidebar,
            title_bar_content,
            tab_bar_content,
            panel_overlay,
            content,
            content_padding,
            content_scrollable,
        } = self;
        let title_bar = TitleBar::new()
            // 交通灯位于整窗高 Sidebar 内；右侧标题栏不再为它重复保留左侧空白。
            .pl(px(0.0))
            .h(WORKSPACE_GLOBAL_BAR_HEIGHT)
            .border_b(px(0.0))
            .child(title_bar_content);

        let content_panel = div()
            .debug_selector(|| "nexora-workspace-content".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p(content_padding)
            .child(content);
        let content_panel = if content_scrollable {
            content_panel.overflow_y_scrollbar().into_any_element()
        } else {
            content_panel.overflow_hidden().into_any_element()
        };
        div()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(background)
            .text_color(foreground)
            .child(sidebar)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(title_bar)
                    .child(
                        div()
                            .debug_selector(|| "nexora-workspace-tab-bar".into())
                            .w_full()
                            .h(WORKSPACE_TAB_BAR_HEIGHT)
                            .flex_shrink_0()
                            .child(tab_bar_content),
                    )
                    .child(content_panel)
                    .when_some(panel_overlay, |this, panel_overlay| {
                        this.child(panel_overlay)
                    }),
            )
            .into_any_element()
    }
}
