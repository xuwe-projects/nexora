#![cfg(all(feature = "desktop", feature = "derive"))]

use gpui::{AppContext as _, Context, Empty, IntoElement, Render, TestAppContext, Window};
use nexora::{AppRegistry, RegistryError};

const APPLICATION_SOURCE: &str = include_str!("../src/application.rs");
const SIDEBAR_REGION_SOURCE: &str = include_str!("../../ui/src/sidebar_region.rs");

#[derive(nexora::SidebarHeader)]
struct TestSidebarHeader {
    value: u32,
}

impl Default for TestSidebarHeader {
    fn default() -> Self {
        Self { value: 7 }
    }
}

impl Render for TestSidebarHeader {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(nexora::SidebarFooter)]
#[nexora(factory = TestSidebarFooter::new)]
struct TestSidebarFooter {
    created_by_factory: bool,
}

impl TestSidebarFooter {
    fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            created_by_factory: true,
        }
    }
}

impl Render for TestSidebarFooter {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[gpui::test]
fn sidebar_slots_are_discovered_and_create_render_entities(cx: &mut TestAppContext) {
    let registry = AppRegistry::discover().expect("每种 Sidebar 插槽只有一个实现时应自动发现");
    let window = cx.add_window(|_, _| Empty);
    let (header, footer) = window
        .update(cx, |_, window, cx| {
            (
                registry
                    .create_sidebar_header(window, cx)
                    .expect("应创建自动发现的 Header"),
                registry
                    .create_sidebar_footer(window, cx)
                    .expect("应创建自动发现的 Footer"),
            )
        })
        .unwrap();
    let header = header.downcast::<TestSidebarHeader>().unwrap();
    let footer = footer.downcast::<TestSidebarFooter>().unwrap();

    assert_eq!(cx.read_entity(&header, |header, _| header.value), 7);
    assert!(cx.read_entity(&footer, |footer, _| footer.created_by_factory));
}

#[test]
fn duplicate_sidebar_slots_return_structured_registry_errors() {
    let header_error = AppRegistry::builder()
        .sidebar_header::<TestSidebarHeader>()
        .sidebar_header::<TestSidebarHeader>()
        .build()
        .err()
        .expect("重复 Header 必须失败");
    assert!(matches!(
        header_error,
        RegistryError::DuplicateSidebarHeader { first, duplicate }
            if first.ends_with("TestSidebarHeader") && duplicate.ends_with("TestSidebarHeader")
    ));

    let footer_error = AppRegistry::builder()
        .sidebar_footer::<TestSidebarFooter>()
        .sidebar_footer::<TestSidebarFooter>()
        .build()
        .err()
        .expect("重复 Footer 必须失败");
    assert!(matches!(
        footer_error,
        RegistryError::DuplicateSidebarFooter { first, duplicate }
            if first.ends_with("TestSidebarFooter") && duplicate.ends_with("TestSidebarFooter")
    ));
}

#[test]
fn shell_uses_gpui_component_sidebar_for_navigation() {
    let sidebar = APPLICATION_SOURCE
        .split_once("fn render_sidebar")
        .and_then(|(_, source)| source.split_once("fn render_tab"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Shell 实现");

    for required in [
        "Sidebar::new(\"nexora-sidebar\")",
        "SidebarGroup::new(section)",
        "SidebarMenu::new().children(",
    ] {
        assert!(
            sidebar.contains(required),
            "Sidebar Shell 必须使用官方组件：{required}"
        );
    }
    assert!(
        APPLICATION_SOURCE.contains("SidebarMenuItem::new(metadata.title())"),
        "Sidebar 导航项必须使用官方 SidebarMenuItem"
    );

    for forbidden in [
        "v_flex()\n            .id(\"nexora-sidebar\")",
        "Button::new(format!(\"nexora-navigation-feature-{}\"",
        "Button::new(format!(\"nexora-navigation-group-",
    ] {
        assert!(
            !sidebar.contains(forbidden),
            "Sidebar Shell 不得手写官方组件已有的导航能力：{forbidden}"
        );
    }
}

#[test]
fn shell_keeps_sidebar_structure_without_injecting_custom_slot_interactions() {
    let default_footer = APPLICATION_SOURCE
        .split_once("fn render_default_account_footer")
        .and_then(|(_, source)| source.split_once("fn render_sidebar"))
        .map(|(source, _)| source)
        .expect("应当可以定位默认账户 Footer 实现");
    assert!(
        default_footer.contains(".hover("),
        "默认 Footer 必须在自身实现中显式声明 hover"
    );

    let sidebar = APPLICATION_SOURCE
        .split_once("fn render_sidebar")
        .and_then(|(_, source)| source.split_once("fn render_tab"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Shell 实现");
    let brand_position = sidebar
        .find("nexora-sidebar-brand")
        .expect("Shell 必须保留默认品牌区域");
    let custom_position = sidebar
        .find(".children(self.sidebar_header.clone())")
        .expect("Shell 必须在品牌之后追加应用 Header Context");
    assert!(brand_position < custom_position);
    assert!(sidebar.contains(".border_b_1()"));
    assert!(sidebar.contains(".border_t_1()"));
    assert!(sidebar.contains(".gap_2()"));

    let region_render = SIDEBAR_REGION_SOURCE
        .split_once("impl RenderOnce for SidebarRegion")
        .map(|(_, source)| source)
        .expect("应当可以定位 SidebarRegion 渲染实现");
    for forbidden in [".hover(", ".on_click(", ".cursor_pointer(", ".rounded("] {
        assert!(
            !region_render.contains(forbidden),
            "自定义 SidebarRegion 不得隐式注入交互视觉：{forbidden}"
        );
    }
}
