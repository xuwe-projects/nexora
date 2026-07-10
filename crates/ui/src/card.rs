//! 桌面工作区内容卡片。
//!
//! 该模块提供用于承载表格、表单和业务摘要的统一内容面，避免各 feature
//! 重复定义背景、边框、圆角和阴影。

use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window, div,
    prelude::*,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// 带主题背景、圆角、边框和轻量阴影的内容卡片。
///
/// Card 只负责通用视觉边界，不内置固定内边距或业务布局。调用方可以继续使用
/// `p_*`、`gap_*`、`flex_*` 等 GPUI 样式控制内容密度和尺寸。
#[derive(IntoElement)]
pub struct Card {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl Card {
    /// 创建一个空内容卡片。
    ///
    /// 卡片渲染时会读取当前 `gpui-component` 主题，因此切换明暗主题或自定义主题后
    /// 无需修改业务 feature。
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().tokens.group_box)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .when(cx.theme().shadow, |this| this.shadow_md())
            .refine_style(&self.style)
            .children(self.children)
    }
}
