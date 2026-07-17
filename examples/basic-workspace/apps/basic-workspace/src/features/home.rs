use nexora::{
    Feature, FeatureElement,
    gpui::{Context, IntoElement, Window, div, prelude::*},
};

#[derive(Default, Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "工作台",
    icon = "layout-dashboard",
    order = 0
)]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child("欢迎使用 Nexora")
    }
}
