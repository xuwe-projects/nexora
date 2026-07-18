use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px};
use ui::SidebarRegion;

struct SidebarRegions;

impl Render for SidebarRegions {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(240.))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                SidebarRegion::new("test-brand-region")
                    .debug_selector(|| "test-brand-region".into())
                    .h(px(32.))
                    .child("品牌"),
            )
            .child(
                SidebarRegion::new("test-context-region")
                    .debug_selector(|| "test-context-region".into())
                    .h(px(40.))
                    .child("当前工厂"),
            )
    }
}

#[gpui::test]
fn brand_and_context_are_independent_hit_regions(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_view, cx) = cx.add_window_view(|_, _| SidebarRegions);

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let brand = cx
        .debug_bounds("test-brand-region")
        .expect("品牌区域应完成布局");
    let context = cx
        .debug_bounds("test-context-region")
        .expect("应用 Context 区域应完成布局");

    assert_ne!(brand, context);
    assert!(brand.bottom() <= context.top());
}
