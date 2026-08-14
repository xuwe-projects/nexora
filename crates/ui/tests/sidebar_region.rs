use gpui::{Anchor, Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px};
use gpui_component::{h_flex, menu::DropdownMenu as _};
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

struct SidebarFooterRegions;

impl Render for SidebarFooterRegions {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                h_flex().w(px(236.0)).child(
                    div().flex_1().min_w_0().child(
                        SidebarRegion::new("expanded-footer-region")
                            .debug_selector(|| "expanded-footer-region".into())
                            .h(px(32.0))
                            .child("账户")
                            .dropdown_menu_with_anchor(Anchor::BottomLeft, |menu, _, _| menu),
                    ),
                ),
            )
            .child(
                h_flex().w(px(56.0)).justify_center().child(
                    SidebarRegion::new("collapsed-footer-region")
                        .debug_selector(|| "collapsed-footer-region".into())
                        .size_8()
                        .child("A")
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, |menu, _, _| menu),
                ),
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

#[gpui::test]
fn footer_dropdown_fills_expanded_width_and_centers_collapsed_square(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_view, cx) = cx.add_window_view(|_, _| SidebarFooterRegions);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let expanded = cx
        .debug_bounds("expanded-footer-region")
        .expect("展开 Footer 应完成布局");
    let collapsed = cx
        .debug_bounds("collapsed-footer-region")
        .expect("收起 Footer 应完成布局");

    assert_eq!(expanded.size.width, px(236.0));
    assert_eq!(collapsed.size, gpui::size(px(32.0), px(32.0)));
    assert_eq!(collapsed.origin.x, px(12.0));
}
