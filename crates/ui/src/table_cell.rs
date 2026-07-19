//! 桌面数据表正文单元格辅助组件。
//!
//! 本模块提供轻量正文单元格，适合在 `gpui-component` 的 `TableDelegate::render_td`
//! 中复用。默认垂直居中、水平靠左；调用方可以按列语义覆盖水平和垂直对齐方式。

use gpui::{AnyElement, App, IntoElement, RenderOnce, TextAlign, Window, div, prelude::*};

/// 表格正文单元格的垂直对齐方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableCellVerticalAlign {
    /// 内容贴近单元格顶部。
    Top,
    /// 内容在单元格中垂直居中。
    Center,
    /// 内容贴近单元格底部。
    Bottom,
}

/// 默认垂直居中、水平靠左的表格正文单元格。
///
/// 调用方可以通过 [`Self::left`]、[`Self::center`]、[`Self::right`] 覆盖水平对齐，
/// 通过 [`Self::top`]、[`Self::middle`]、[`Self::bottom`] 或
/// [`Self::vertical_align`] 覆盖垂直对齐；需要完全自定义正文内容时，仍可直接在
/// `TableDelegate::render_td` 返回自己的 GPUI 元素。
#[derive(IntoElement)]
pub struct TableCell {
    content: AnyElement,
    horizontal_align: TextAlign,
    vertical_align: TableCellVerticalAlign,
}

impl TableCell {
    /// 创建一个默认左对齐、垂直居中的正文单元格。
    pub fn new(content: impl IntoElement) -> Self {
        Self {
            content: content.into_any_element(),
            horizontal_align: TextAlign::Left,
            vertical_align: TableCellVerticalAlign::Center,
        }
    }

    /// 设置正文内容的水平对齐方式。
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.horizontal_align = align;
        self
    }

    /// 将正文内容左对齐。
    #[must_use]
    pub fn left(self) -> Self {
        self.align(TextAlign::Left)
    }

    /// 将正文内容水平居中。
    #[must_use]
    pub fn center(self) -> Self {
        self.align(TextAlign::Center)
    }

    /// 将正文内容右对齐。
    #[must_use]
    pub fn right(self) -> Self {
        self.align(TextAlign::Right)
    }

    /// 设置正文内容的垂直对齐方式。
    #[must_use]
    pub fn vertical_align(mut self, align: TableCellVerticalAlign) -> Self {
        self.vertical_align = align;
        self
    }

    /// 将正文内容贴近单元格顶部。
    #[must_use]
    pub fn top(self) -> Self {
        self.vertical_align(TableCellVerticalAlign::Top)
    }

    /// 将正文内容垂直居中。
    #[must_use]
    pub fn middle(self) -> Self {
        self.vertical_align(TableCellVerticalAlign::Center)
    }

    /// 将正文内容贴近单元格底部。
    #[must_use]
    pub fn bottom(self) -> Self {
        self.vertical_align(TableCellVerticalAlign::Bottom)
    }

    /// 返回当前正文内容的水平对齐方式。
    pub fn horizontal_alignment(&self) -> TextAlign {
        self.horizontal_align
    }

    /// 返回当前正文内容的垂直对齐方式。
    pub fn vertical_alignment(&self) -> TableCellVerticalAlign {
        self.vertical_align
    }
}

impl RenderOnce for TableCell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .size_full()
            .min_w_0()
            .flex()
            .when(self.vertical_align == TableCellVerticalAlign::Top, |this| {
                this.items_start()
            })
            .when(
                self.vertical_align == TableCellVerticalAlign::Center,
                |this| this.items_center(),
            )
            .when(
                self.vertical_align == TableCellVerticalAlign::Bottom,
                |this| this.items_end(),
            )
            .when(self.horizontal_align == TextAlign::Left, |this| {
                this.justify_start()
            })
            .when(self.horizontal_align == TextAlign::Center, |this| {
                this.justify_center()
            })
            .when(self.horizontal_align == TextAlign::Right, |this| {
                this.justify_end()
            })
            .child(
                div()
                    .min_w_0()
                    .when(self.horizontal_align == TextAlign::Center, |this| {
                        this.text_center()
                    })
                    .when(self.horizontal_align == TextAlign::Right, |this| {
                        this.text_right()
                    })
                    .child(self.content),
            )
    }
}
