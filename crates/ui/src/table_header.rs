//! 桌面数据表表头辅助组件。
//!
//! 本模块提供轻量表头单元格，适合在 `gpui-component` 的 `TableDelegate::render_th`
//! 中复用，让业务表格默认拥有稳定的水平与垂直对齐方式。

use gpui::{App, IntoElement, RenderOnce, SharedString, TextAlign, Window, div, prelude::*};

/// 默认垂直居中、水平居中的表头单元格。
///
/// 调用方可以通过 [`Self::left`]、[`Self::center`]、[`Self::right`] 或 [`Self::align`]
/// 覆盖水平对齐方式；需要完全自定义表头内容时，仍可直接在 `TableDelegate::render_th`
/// 返回自己的 GPUI 元素。
#[derive(Clone, Debug, IntoElement)]
pub struct TableHeaderCell {
    label: SharedString,
    align: TextAlign,
}

impl TableHeaderCell {
    /// 创建一个默认居中的表头单元格。
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            align: TextAlign::Center,
        }
    }

    /// 设置表头文本的水平对齐方式。
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// 将表头文本左对齐。
    #[must_use]
    pub fn left(self) -> Self {
        self.align(TextAlign::Left)
    }

    /// 将表头文本居中对齐。
    #[must_use]
    pub fn center(self) -> Self {
        self.align(TextAlign::Center)
    }

    /// 将表头文本右对齐。
    #[must_use]
    pub fn right(self) -> Self {
        self.align(TextAlign::Right)
    }

    /// 返回当前表头文本的水平对齐方式。
    pub fn alignment(&self) -> TextAlign {
        self.align
    }
}

impl RenderOnce for TableHeaderCell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .h_full()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .truncate()
                    .when(self.align == TextAlign::Center, |this| this.text_center())
                    .when(self.align == TextAlign::Right, |this| this.text_right())
                    .child(self.label),
            )
    }
}
