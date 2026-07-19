extern crate self as nexora;

mod __private {
    pub mod gpui {
        pub struct Pixels(pub f32);
        pub enum TextAlign {
            Left,
            Center,
            Right,
        }
        pub struct Window;
        pub struct App;
        pub struct AnyElement;
        pub struct Empty;

        pub fn px(value: f32) -> Pixels {
            Pixels(value)
        }

        pub trait IntoElement: Sized {
            fn into_any_element(self) -> AnyElement {
                AnyElement
            }
        }

        impl IntoElement for Empty {}
        impl IntoElement for String {}
    }

    pub mod gpui_component {
        pub mod table {
            use crate::__private::gpui::{Pixels, TextAlign, px};

            pub struct Column {
                pub key: String,
                pub name: String,
                pub align: TextAlign,
                pub width: Pixels,
            }

            impl Column {
                pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
                    Self {
                        key: key.into(),
                        name: name.into(),
                        align: TextAlign::Left,
                        width: px(100.),
                    }
                }

                pub fn width(mut self, width: Pixels) -> Self {
                    self.width = width;
                    self
                }

                pub fn min_width(self, _width: Pixels) -> Self {
                    self
                }

                pub fn max_width(self, _width: Pixels) -> Self {
                    self
                }

                pub fn sortable(self) -> Self {
                    self
                }

                pub fn ascending(self) -> Self {
                    self
                }

                pub fn descending(self) -> Self {
                    self
                }

                pub fn fixed_left(self) -> Self {
                    self
                }

                pub fn resizable(self, _resizable: bool) -> Self {
                    self
                }

                pub fn movable(self, _movable: bool) -> Self {
                    self
                }

                pub fn selectable(self, _selectable: bool) -> Self {
                    self
                }

                pub fn text_center(mut self) -> Self {
                    self.align = TextAlign::Center;
                    self
                }

                pub fn text_right(mut self) -> Self {
                    self.align = TextAlign::Right;
                    self
                }
            }
        }
    }
}

mod desktop {
    use crate::__private::gpui::{AnyElement, App, IntoElement, TextAlign, Window};
    use crate::__private::gpui_component::table::Column;

    pub enum TableCellVerticalAlign {
        Top,
        Center,
        Bottom,
    }

    pub struct TableCell;

    impl TableCell {
        pub fn new(_content: impl IntoElement) -> Self {
            Self
        }

        pub fn align(self, _align: TextAlign) -> Self {
            self
        }

        pub fn vertical_align(self, _align: TableCellVerticalAlign) -> Self {
            self
        }
    }

    impl IntoElement for TableCell {}

    pub trait CrudTableRow: Clone + 'static {
        fn columns() -> Vec<Column>;

        fn header_alignment(_key: &str) -> TextAlign {
            TextAlign::Center
        }

        fn cell_alignment(_key: &str) -> TextAlign {
            TextAlign::Left
        }

        fn cell_vertical_alignment(_key: &str) -> TableCellVerticalAlign {
            TableCellVerticalAlign::Center
        }

        fn render_cell(&self, key: &str, window: &mut Window, cx: &mut App) -> AnyElement;

        fn cell_text(&self, key: &str, cx: &App) -> String;
    }
}
