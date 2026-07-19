extern crate self as nexora;

mod __private {
    pub mod gpui {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Pixels(pub f32);

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        impl IntoElement for &'static str {}
    }

    pub mod gpui_component {
        pub mod table {
            use crate::__private::gpui::{Pixels, TextAlign, px};

            #[derive(Debug, Clone, PartialEq)]
            pub struct Column {
                pub key: String,
                pub name: String,
                pub align: TextAlign,
                pub sort: Option<&'static str>,
                pub width: Pixels,
                pub min_width: Pixels,
                pub max_width: Pixels,
                pub fixed_left: bool,
                pub resizable: bool,
                pub movable: bool,
                pub selectable: bool,
            }

            impl Column {
                pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
                    Self {
                        key: key.into(),
                        name: name.into(),
                        align: TextAlign::Left,
                        sort: None,
                        width: px(100.),
                        min_width: px(20.),
                        max_width: px(f32::MAX),
                        fixed_left: false,
                        resizable: true,
                        movable: true,
                        selectable: true,
                    }
                }

                pub fn width(mut self, width: Pixels) -> Self {
                    self.width = width;
                    self
                }

                pub fn min_width(mut self, min_width: Pixels) -> Self {
                    self.min_width = min_width;
                    self
                }

                pub fn max_width(mut self, max_width: Pixels) -> Self {
                    self.max_width = max_width;
                    self
                }

                pub fn sortable(mut self) -> Self {
                    self.sort = Some("default");
                    self
                }

                pub fn ascending(mut self) -> Self {
                    self.sort = Some("ascending");
                    self
                }

                pub fn descending(mut self) -> Self {
                    self.sort = Some("descending");
                    self
                }

                pub fn fixed_left(mut self) -> Self {
                    self.fixed_left = true;
                    self
                }

                pub fn resizable(mut self, resizable: bool) -> Self {
                    self.resizable = resizable;
                    self
                }

                pub fn movable(mut self, movable: bool) -> Self {
                    self.movable = movable;
                    self
                }

                pub fn selectable(mut self, selectable: bool) -> Self {
                    self.selectable = selectable;
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

use desktop::CrudTableRow as _;

#[derive(Clone, nexora_macros::CrudTableRow)]
struct CityRow {
    #[nexora(column(name = "ID", width = 64., min_width = 48., fixed_left))]
    id: u64,
    #[nexora(column(title = "城市", width = 160., sortable))]
    name: String,
    #[nexora(column(title = "代码", width = 96., ascending, vertical_align = "top"))]
    code: String,
    #[nexora(column(
        title = "排序",
        width = 80.,
        descending,
        align = "right",
        vertical_align = "bottom"
    ))]
    sort_order: u32,
    #[nexora(column(
        key = "status",
        title = "状态",
        width = 76.,
        max_width = 96.,
        align = "center",
        header_align = "center",
        vertical_align = "middle",
        render = Self::render_status,
        text = Self::status_text,
        resizable = false,
        movable = false,
        selectable = false
    ))]
    enabled: bool,
}

impl CityRow {
    fn render_status(
        row: &Self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::App,
    ) -> desktop::TableCell {
        let label = if row.enabled { "启用" } else { "停用" };
        desktop::TableCell::new(label)
    }

    fn status_text(row: &Self, _cx: &__private::gpui::App) -> String {
        if row.enabled {
            "启用".to_owned()
        } else {
            "停用".to_owned()
        }
    }
}

#[test]
fn crud_table_row_derive_generates_columns_rendering_and_text_accessors() {
    let columns = CityRow::columns();
    assert_eq!(columns.len(), 5);
    assert_eq!(columns[0].key, "id");
    assert_eq!(columns[0].name, "ID");
    assert_eq!(columns[0].width, __private::gpui::Pixels(64.));
    assert_eq!(columns[0].min_width, __private::gpui::Pixels(48.));
    assert!(columns[0].fixed_left);
    assert_eq!(columns[1].sort, Some("default"));
    assert_eq!(columns[2].sort, Some("ascending"));
    assert_eq!(columns[3].sort, Some("descending"));
    assert_eq!(columns[3].align, __private::gpui::TextAlign::Right);
    assert_eq!(columns[4].key, "status");
    assert_eq!(columns[4].align, __private::gpui::TextAlign::Center);
    assert!(!columns[4].resizable);
    assert!(!columns[4].movable);
    assert!(!columns[4].selectable);

    assert_eq!(
        CityRow::header_alignment("status"),
        __private::gpui::TextAlign::Center
    );
    assert_eq!(
        CityRow::cell_alignment("status"),
        __private::gpui::TextAlign::Center
    );
    assert_eq!(
        CityRow::cell_vertical_alignment("status"),
        desktop::TableCellVerticalAlign::Center
    );
    assert_eq!(
        CityRow::cell_alignment("sort_order"),
        __private::gpui::TextAlign::Right
    );
    assert_eq!(
        CityRow::cell_vertical_alignment("code"),
        desktop::TableCellVerticalAlign::Top
    );
    assert_eq!(
        CityRow::cell_vertical_alignment("sort_order"),
        desktop::TableCellVerticalAlign::Bottom
    );

    let row = CityRow {
        id: 7,
        name: "北京".to_owned(),
        code: "110000".to_owned(),
        sort_order: 1,
        enabled: true,
    };
    let app = __private::gpui::App;
    assert_eq!(row.cell_text("id", &app), "7");
    assert_eq!(row.cell_text("name", &app), "北京");
    assert_eq!(row.cell_text("code", &app), "110000");
    assert_eq!(row.cell_text("sort_order", &app), "1");
    assert_eq!(row.cell_text("status", &app), "启用");

    let mut window = __private::gpui::Window;
    let mut app = __private::gpui::App;
    let _element = row.render_cell("status", &mut window, &mut app);
}

#[test]
fn crud_table_row_compile_failures_are_checked() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/crud_table_row_*.rs");
}
