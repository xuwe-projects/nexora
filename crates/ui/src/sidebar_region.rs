//! Sidebar Header/Footer 中由应用自行控制交互视觉的稳定区域。

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
};
use gpui_component::{Selectable, StyledExt as _, menu::DropdownMenu};

/// Sidebar Header/Footer 内具有调用方稳定 ID 的内容区域。
///
/// 组件只提供横向排列、完整宽度和样式扩展点，不会隐式添加 hover、selected 背景、圆角、
/// cursor 或点击行为。品牌、工厂选择器和账号菜单等区域可以分别创建实例，并只为真正可
/// 交互的区域添加视觉与事件。
#[derive(IntoElement)]
pub struct SidebarRegion {
    base: Stateful<gpui::Div>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    selected: bool,
}

impl SidebarRegion {
    /// 使用调用方提供的稳定元素 ID 创建一个无隐式交互视觉的 Sidebar 区域。
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            children: Vec::new(),
            selected: false,
        }
    }
}

impl ParentElement for SidebarRegion {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for SidebarRegion {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for SidebarRegion {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for SidebarRegion {}

impl Selectable for SidebarRegion {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl DropdownMenu for SidebarRegion {}

impl RenderOnce for SidebarRegion {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.base
            .flex()
            .items_center()
            .min_w_0()
            .w_full()
            .refine_style(&self.style)
            .children(self.children)
    }
}
