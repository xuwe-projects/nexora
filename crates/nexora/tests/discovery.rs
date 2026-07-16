use nexora::{
    AppRegistry, Feature, FeatureElement, RouteTargetKind, Window, WindowElement,
    gpui::{Context, Empty, IntoElement, Window as GpuiWindow},
};

#[derive(Default, Feature)]
#[nexora(title = "自动发现页面", path = "/discovered", section = "测试")]
struct DiscoveredFeature;

impl FeatureElement for DiscoveredFeature {
    fn render(&mut self, _: &mut GpuiWindow, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default, Window)]
#[nexora(title = "自动发现窗口", path = "/discovered-window")]
struct DiscoveredWindow;

impl WindowElement for DiscoveredWindow {
    fn render(&mut self, _: &mut GpuiWindow, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[test]
fn derives_are_discovered_without_a_manual_registration_list() {
    let registry = AppRegistry::discover().unwrap();

    assert_eq!(registry.features(), [DiscoveredFeature::METADATA]);
    assert_eq!(registry.windows(), [DiscoveredWindow::METADATA]);
    assert_eq!(
        registry.resolve("/discovered").unwrap().target().kind(),
        RouteTargetKind::Feature
    );
    assert_eq!(
        registry
            .resolve("/discovered-window")
            .unwrap()
            .target()
            .kind(),
        RouteTargetKind::Window
    );
}
