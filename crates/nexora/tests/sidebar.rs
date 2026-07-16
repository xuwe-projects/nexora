use gpui::{AppContext as _, Context, Empty, IntoElement, Render, TestAppContext, Window};
use nexora::{AppRegistry, RegistryError};

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
