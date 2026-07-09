//! 共享桌面应用布局组件。
//!
//! 该模块提供跨桌面应用复用的应用壳结构，业务应用只需要传入自己的导航、窗口顶部栏和内容面板。

use std::ops::Range;

use gpui::{AnyElement, Context, IntoElement, Pixels, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
};

/// 带侧边栏的桌面应用壳布局。
///
/// 该组件负责组织“窗口顶部栏 + 左侧导航 + 主内容区”的后台管理控制台结构。
/// 它不保存业务导航状态，也不理解具体 feature，只承载已经由应用层创建好的三个区域。
pub struct SidebarShell {
    sidebar: AnyElement,
    top_bar: AnyElement,
    content: AnyElement,
    content_padding: Pixels,
    content_scrollable: bool,
    sidebar_width: Pixels,
    sidebar_width_range: Range<Pixels>,
}

impl SidebarShell {
    /// 创建一个使用默认控制台间距的侧边栏应用壳。
    ///
    /// `sidebar` 通常应由 `gpui-component` 的 `Sidebar` 及其子组件构成；
    /// `top_bar` 通常应使用 `gpui-component` 的 `TitleBar`，用于承载窗口级 tabs、工具区或应用菜单；
    /// `content` 是当前 feature 的主面板内容。
    pub fn new(
        sidebar: impl IntoElement,
        top_bar: impl IntoElement,
        content: impl IntoElement,
    ) -> Self {
        Self {
            sidebar: sidebar.into_any_element(),
            top_bar: top_bar.into_any_element(),
            content: content.into_any_element(),
            content_padding: px(24.0),
            content_scrollable: true,
            sidebar_width: px(248.0),
            sidebar_width_range: px(208.0)..px(360.0),
        }
    }

    /// 设置侧边导航区域的初始宽度。
    ///
    /// 该值会传给 `gpui-component` 的 `ResizablePanel` 作为首次渲染时的宽度；
    /// 用户后续拖拽调整后，实际宽度会由组件内部状态继续维护。
    pub fn with_sidebar_width(mut self, width: Pixels) -> Self {
        self.sidebar_width = width;
        self
    }

    /// 设置侧边导航区域允许拖拽调整的宽度范围。
    ///
    /// `range.start` 表示最小宽度，`range.end` 表示最大宽度；
    /// 当用户拖拽分隔条时，侧边栏不会超过该范围。
    pub fn with_sidebar_width_range(mut self, range: Range<Pixels>) -> Self {
        self.sidebar_width_range = range;
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

    /// 返回侧边导航区域首次渲染时使用的宽度。
    ///
    /// 该值用于确认共享布局的默认导航宽度，也方便具体应用在测试中校验自己的壳配置。
    pub fn sidebar_width(&self) -> Pixels {
        self.sidebar_width
    }

    /// 返回侧边导航区域允许拖拽到的最小宽度。
    ///
    /// 它来自 `with_sidebar_width_range` 设置的范围起点，用于避免导航项被压缩到不可读。
    pub fn sidebar_min_width(&self) -> Pixels {
        self.sidebar_width_range.start
    }

    /// 返回侧边导航区域允许拖拽到的最大宽度。
    ///
    /// 它来自 `with_sidebar_width_range` 设置的范围终点，用于避免导航占用过多主内容空间。
    pub fn sidebar_max_width(&self) -> Pixels {
        self.sidebar_width_range.end
    }

    /// 将应用壳渲染为 GPUI 元素树。
    ///
    /// 返回元素包含固定的桌面控制台布局结构：外层全尺寸容器、窗口顶部栏、左侧导航区域，
    /// 以及可以按 feature 需要开启或关闭外层滚动的主内容区域；侧边导航支持拖拽调整宽度并受上下限约束。
    /// 颜色和背景会读取当前 `gpui-component` 主题，避免业务应用在布局层写死视觉样式。
    pub fn render<T>(self, cx: &Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let theme = cx.theme();
        let Self {
            sidebar,
            top_bar,
            content,
            content_padding,
            content_scrollable,
            sidebar_width,
            sidebar_width_range,
        } = self;

        let content_panel = div()
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
            .flex_col()
            .size_full()
            .bg(theme.tokens.background)
            .text_color(theme.foreground)
            .child(top_bar)
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("sidebar-shell-layout")
                        .child(
                            resizable_panel()
                                .flex_none()
                                .size(sidebar_width)
                                .size_range(sidebar_width_range)
                                .child(sidebar),
                        )
                        .child(
                            resizable_panel().child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .child(content_panel),
                            ),
                        ),
                ),
            )
            .into_any_element()
    }
}
