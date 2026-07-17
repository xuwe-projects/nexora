use gpui::{Context, IntoElement, Window, div, prelude::*};
use nexora::{Feature, FeatureElement};

#[derive(Default, Feature)]
#[nexora(
    title = "关于",
    path = "/about",
    section = "工作台",
    icon = "info",
    order = 10
)]
struct AboutFeature;

impl FeatureElement for AboutFeature {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child("这是一个不启用 Account 的 Nexora workspace 示例")
    }
}
