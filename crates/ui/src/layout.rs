//! 共享桌面应用布局组件。
//!
//! 该模块提供跨桌面应用复用的工作区结构，业务应用只需要传入自己的导航、窗口顶部栏内容和主面板。

use std::ops::Range;

use gpui::{AnyElement, Context, IntoElement, Pixels, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, TitleBar,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
};

/// 带窗口顶部栏和侧边导航的桌面工作区布局。
///
/// 该组件负责创建官方 `TitleBar`，并组织“窗口顶部栏 + 左侧导航 + 主内容区”的桌面应用结构。
/// 它统一处理 macOS 全屏时的标题栏占位、侧边栏拖拽范围和内容滚动，但不保存业务导航状态，
/// 也不理解具体 feature、标签页或菜单语义。
pub struct WorkspaceLayout {
    sidebar: AnyElement,
    title_bar_content: AnyElement,
    panel_header: Option<AnyElement>,
    content: AnyElement,
    content_padding: Pixels,
    content_scrollable: bool,
    sidebar_width: Pixels,
    sidebar_width_range: Range<Pixels>,
}

impl WorkspaceLayout {
    /// 创建一个使用默认尺寸和间距的桌面工作区布局。
    ///
    /// `sidebar` 通常应由 `gpui-component` 的 `Sidebar` 及其子组件构成；
    /// `title_bar_content` 是插入官方 `TitleBar` 的业务内容，适合承载窗口级 tabs、工具区或应用菜单；
    /// `content` 是当前 feature 的主面板内容。
    pub fn new(
        sidebar: impl IntoElement,
        title_bar_content: impl IntoElement,
        content: impl IntoElement,
    ) -> Self {
        Self {
            sidebar: sidebar.into_any_element(),
            title_bar_content: title_bar_content.into_any_element(),
            panel_header: None,
            content: content.into_any_element(),
            content_padding: px(24.0),
            content_scrollable: true,
            sidebar_width: px(248.0),
            sidebar_width_range: px(208.0)..px(360.0),
        }
    }

    /// 设置右侧主面板中位于业务内容上方的公共顶部栏。
    ///
    /// 顶部栏会在内容滚动区域之外固定展示，适合传入 `PanelHeader`，统一承载面包屑和页面级操作。
    /// 未调用该方法时不会预留顶部栏高度，保证不需要页面导航栏的应用仍可复用当前布局。
    pub fn with_panel_header(mut self, panel_header: impl IntoElement) -> Self {
        self.panel_header = Some(panel_header.into_any_element());
        self
    }

    /// 设置侧边导航区域的初始宽度。
    ///
    /// 该值会作为首次渲染时的像素宽度；用户后续拖拽调整后，实际宽度会由应用壳内部状态继续维护。
    /// 窗口整体尺寸变化不会按比例修改侧边栏宽度。
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

    /// 返回右侧主面板是否已经配置公共顶部栏。
    ///
    /// 该值方便应用壳和集成测试确认面包屑与页面操作区是否由共享布局统一承载。
    pub fn has_panel_header(&self) -> bool {
        self.panel_header.is_some()
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

    /// 将桌面工作区渲染为 GPUI 元素树。
    ///
    /// 返回元素包含固定的桌面工作区结构：外层全尺寸容器、官方窗口顶部栏、左侧导航区域，
    /// 以及可以按 feature 需要开启或关闭外层滚动的主内容区域；侧边导航支持拖拽调整宽度并受上下限约束。
    /// 侧边导航宽度会保存为像素状态，因此窗口整体拉宽或缩小时不会按比例改变侧栏宽度，只有用户拖动分隔条时才会更新。
    /// macOS 全屏时会释放原本为交通灯保留的左侧空间；颜色和背景读取当前 `gpui-component` 主题，
    /// 避免业务应用重复处理平台差异或写死视觉样式。
    pub fn render<T>(self, window: &mut Window, cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let background = cx.theme().tokens.background;
        let foreground = cx.theme().foreground;
        let Self {
            sidebar,
            title_bar_content,
            panel_header,
            content,
            content_padding,
            content_scrollable,
            sidebar_width,
            sidebar_width_range,
        } = self;
        let title_bar = TitleBar::new()
            .when(
                cfg!(target_os = "macos") && window.is_fullscreen(),
                |this| this.pl(px(0.0)),
            )
            .border_b(px(0.0))
            .child(title_bar_content);

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
        let workspace_panels = h_resizable("workspace-layout-panels")
            .child(
                resizable_panel()
                    .size(sidebar_width)
                    .size_range(sidebar_width_range)
                    .flex_none()
                    .child(div().size_full().min_w_0().min_h_0().child(sidebar)),
            )
            .child(
                resizable_panel().child(
                    div()
                        .flex()
                        .flex_col()
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .when_some(panel_header, |this, panel_header| this.child(panel_header))
                        .child(content_panel),
                ),
            );

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .child(title_bar)
            .child(div().flex_1().min_w_0().min_h_0().child(workspace_panels))
            .into_any_element()
    }
}
