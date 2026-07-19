#![allow(dead_code)]

extern crate self as nexora;

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use desktop::CrudTableRow as _;

mod __private {
    pub mod gpui {
        #[derive(Clone, Copy)]
        pub struct Pixels(pub f32);

        #[derive(Clone, Copy)]
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

            #[derive(Clone)]
            pub struct Column {
                pub key: String,
                pub name: String,
                pub align: TextAlign,
                pub width: Pixels,
                pub min_width: Pixels,
                pub max_width: Pixels,
                pub sortable: bool,
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
                        width: px(100.),
                        min_width: px(20.),
                        max_width: px(f32::MAX),
                        sortable: false,
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
                    self.sortable = true;
                    self
                }

                pub fn ascending(self) -> Self {
                    self.sortable()
                }

                pub fn descending(self) -> Self {
                    self.sortable()
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

#[derive(Clone, nexora_macros::CrudTableRow)]
struct DerivedCityRow {
    #[nexora(column(name = "ID", width = 64., min_width = 48., fixed_left))]
    id: u64,
    #[nexora(column(title = "城市", width = 160., sortable))]
    name: String,
    #[nexora(column(
        key = "status",
        title = "状态",
        width = 76.,
        align = "center",
        render = Self::render_status,
        text = Self::status_text,
        resizable = false
    ))]
    enabled: bool,
}

impl DerivedCityRow {
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

#[derive(Clone)]
struct HandwrittenCityRow {
    id: u64,
    name: String,
    enabled: bool,
}

impl desktop::CrudTableRow for HandwrittenCityRow {
    fn columns() -> Vec<__private::gpui_component::table::Column> {
        vec![
            __private::gpui_component::table::Column::new("id", "ID")
                .width(__private::gpui::px(64.))
                .min_width(__private::gpui::px(48.))
                .fixed_left(),
            __private::gpui_component::table::Column::new("name", "城市")
                .width(__private::gpui::px(160.))
                .sortable(),
            __private::gpui_component::table::Column::new("status", "状态")
                .width(__private::gpui::px(76.))
                .text_center()
                .resizable(false),
        ]
    }

    fn header_alignment(_key: &str) -> __private::gpui::TextAlign {
        __private::gpui::TextAlign::Center
    }

    fn cell_alignment(key: &str) -> __private::gpui::TextAlign {
        match key {
            "status" => __private::gpui::TextAlign::Center,
            _ => __private::gpui::TextAlign::Left,
        }
    }

    fn cell_vertical_alignment(_key: &str) -> desktop::TableCellVerticalAlign {
        desktop::TableCellVerticalAlign::Center
    }

    fn render_cell(
        &self,
        key: &str,
        window: &mut __private::gpui::Window,
        cx: &mut __private::gpui::App,
    ) -> __private::gpui::AnyElement {
        match key {
            "id" => __private::gpui::IntoElement::into_any_element(
                desktop::TableCell::new(self.id.to_string())
                    .align(<Self as desktop::CrudTableRow>::cell_alignment("id"))
                    .vertical_align(<Self as desktop::CrudTableRow>::cell_vertical_alignment(
                        "id",
                    )),
            ),
            "name" => __private::gpui::IntoElement::into_any_element(
                desktop::TableCell::new(self.name.to_string())
                    .align(<Self as desktop::CrudTableRow>::cell_alignment("name"))
                    .vertical_align(<Self as desktop::CrudTableRow>::cell_vertical_alignment(
                        "name",
                    )),
            ),
            "status" => {
                let label = if self.enabled { "启用" } else { "停用" };
                let _ = (window, cx);
                __private::gpui::IntoElement::into_any_element(desktop::TableCell::new(label))
            }
            _ => __private::gpui::IntoElement::into_any_element(__private::gpui::Empty),
        }
    }

    fn cell_text(&self, key: &str, _cx: &__private::gpui::App) -> String {
        match key {
            "id" => self.id.to_string(),
            "name" => self.name.to_string(),
            "status" => {
                if self.enabled {
                    "启用".to_owned()
                } else {
                    "停用".to_owned()
                }
            }
            _ => String::new(),
        }
    }
}

fn measure(label: &str, mut run: impl FnMut()) -> Duration {
    let started = Instant::now();
    run();
    let elapsed = started.elapsed();
    println!("{label}: {} ns", elapsed.as_nanos());
    elapsed
}

fn main() {
    const ITERATIONS: usize = 25_000;
    let derived = DerivedCityRow {
        id: 1,
        name: "北京".to_owned(),
        enabled: true,
    };
    let handwritten = HandwrittenCityRow {
        id: 1,
        name: "北京".to_owned(),
        enabled: true,
    };
    let app = __private::gpui::App;
    let mut window = __private::gpui::Window;

    let derived_elapsed = measure("derived", || {
        for _ in 0..ITERATIONS {
            black_box(DerivedCityRow::columns());
            black_box(derived.cell_text("id", &app));
            black_box(derived.cell_text("name", &app));
            black_box(derived.cell_text("status", &app));
            black_box(derived.render_cell("status", &mut window, &mut __private::gpui::App));
        }
    });
    let handwritten_elapsed = measure("handwritten", || {
        for _ in 0..ITERATIONS {
            black_box(HandwrittenCityRow::columns());
            black_box(handwritten.cell_text("id", &app));
            black_box(handwritten.cell_text("name", &app));
            black_box(handwritten.cell_text("status", &app));
            black_box(handwritten.render_cell("status", &mut window, &mut __private::gpui::App));
        }
    });

    let allowed = handwritten_elapsed.as_nanos().saturating_mul(3).max(1);
    assert!(
        derived_elapsed.as_nanos() <= allowed,
        "CrudTableRow derive benchmark regressed: derived={}ns handwritten={}ns",
        derived_elapsed.as_nanos(),
        handwritten_elapsed.as_nanos()
    );
}
