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
    assert!(
        APPLICATION_SOURCE.contains("Input::new(input)")
            && APPLICATION_SOURCE.contains(
                ".prefix(Icon::new(IconName::Search).with_size(WORKSPACE_SHELL_ICON_SIZE))"
            )
            && APPLICATION_SOURCE.contains(".with_size(theme::component_size(cx))"),
        "Sidebar 搜索输入必须复用 gpui-component Input、搜索图标前缀并跟随组件尺寸"
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
fn collapsed_navigation_groups_open_recursive_official_popup_menus() {
    let collapsed = APPLICATION_SOURCE
        .split_once("fn render_collapsed_navigation_entry")
        .and_then(|(_, source)| source.split_once("#[derive(Clone)]\nstruct NavigationSearchIndex"))
        .map(|(source, _)| source)
        .expect("应当可以定位收起导航目录实现");

    for required in [
        "dropdown_menu_with_anchor(Anchor::RightCenter",
        "populate_navigation_popup_menu(",
        "menu.submenu_with_icon(",
        "PopupMenuItem::new(metadata.title())",
        "cx.navigate(path.clone())",
    ] {
        assert!(collapsed.contains(required), "收起导航目录缺少 {required}");
    }
}

#[test]
fn sidebar_shell_controls_share_the_navigation_icon_size() {
    let sidebar = APPLICATION_SOURCE
        .split_once("fn render_sidebar")
        .and_then(|(_, source)| source.split_once("fn render_tab"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Shell 实现");

    for control in [
        "\"expand-sidebar\"",
        "\"collapse-sidebar\"",
        "\"expand-sidebar-search\"",
    ] {
        assert!(sidebar.contains(control), "Sidebar 缺少 {control}");
    }
    assert!(sidebar.contains("workspace_icon_button("));
    assert!(!sidebar.contains("SidebarToggleButton::new()"));
}

#[test]
fn sidebar_footer_fills_expanded_width_and_centers_only_when_collapsed() {
    let footer_host = APPLICATION_SOURCE
        .split_once("fn workspace_sidebar_footer_host")
        .and_then(|(_, source)| source.split_once("/// Shell 顶部工具区"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Footer 宿主");

    assert!(footer_host.contains("host.justify_center().child(content)"));
    assert!(footer_host.contains("div().flex_1().min_w_0().child(content)"));
}

#[test]
fn sidebar_feature_icons_keep_an_explicit_twenty_pixel_size() {
    let sidebar_feature_icon = APPLICATION_SOURCE
        .split_once("fn sidebar_feature_icon")
        .and_then(|(_, source)| source.split_once("impl Render for ApplicationShell"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Feature 图标构造函数");

    assert!(
        sidebar_feature_icon.contains("WORKSPACE_SHELL_ICON_SIZE"),
        "Sidebar 图标必须显式使用共享 20px 尺寸"
    );
    assert!(
        APPLICATION_SOURCE.contains(".icon(sidebar_feature_icon(metadata.icon()))"),
        "Sidebar Feature 与目录必须使用专用图标尺寸"
    );

    let feature_icon = APPLICATION_SOURCE
        .split_once("fn feature_icon")
        .and_then(|(_, source)| source.split_once("impl Render for ApplicationShell"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Feature 图标构造函数");

    assert!(
        feature_icon.contains(".size_4()"),
        "非 Sidebar Feature 图标继续保持 16px"
    );
}

#[test]
fn shell_keeps_sidebar_structure_without_injecting_custom_slot_interactions() {
    let default_header = APPLICATION_SOURCE
        .split_once("fn render_default_sidebar_header")
        .and_then(|(_, source)| source.split_once("fn render_sidebar_header_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位默认 Sidebar Header 实现");
    for required in [
        ".w_full()",
        ".flex_1()",
        ".overflow_hidden()",
        ".whitespace_nowrap()",
        ".truncate()",
    ] {
        assert!(
            default_header.contains(required),
            "默认 Sidebar Header 必须防止品牌文案在窄侧边栏中按字符换行：{required}"
        );
    }

    let default_footer = APPLICATION_SOURCE
        .split_once("fn render_default_account_footer")
        .and_then(|(_, source)| source.split_once("fn render_sidebar"))
        .map(|(source, _)| source)
        .expect("应当可以定位默认账户 Footer 实现");
    assert!(
        default_footer.contains(".hover("),
        "默认 Footer 必须在自身实现中显式声明 hover"
    );
    assert!(default_footer.contains("Avatar::new().name(display_name.clone()).small()"));
    assert!(
        default_footer.contains(".when(collapsed") && default_footer.contains(".size_8()"),
        "默认 Footer 收起态必须用 32px 正方形 hover 区居中 Avatar"
    );
    assert!(
        !default_footer.contains(".src("),
        "默认登录用户区域只能显示首字母/默认 Avatar，不再读取图片 URL"
    );

    let header_content = APPLICATION_SOURCE
        .split_once("fn render_sidebar_header_content")
        .and_then(|(_, source)| source.split_once("#[cfg(feature = \"desktop\")]"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Header 内容选择实现");
    assert!(
        header_content.contains("if let Some(sidebar_header) = self.sidebar_header.as_ref()"),
        "Shell 必须优先使用应用自定义 SidebarHeader"
    );
    assert!(
        header_content.contains("return sidebar_header.clone().into_any_element();"),
        "应用自定义 SidebarHeader 必须替换默认品牌内容"
    );
    assert!(
        header_content.contains("SidebarRegion::new(\"nexora-sidebar-brand\")"),
        "没有自定义 SidebarHeader 时 Shell 才渲染默认品牌区域"
    );

    let sidebar = APPLICATION_SOURCE
        .split_once("fn render_sidebar")
        .and_then(|(_, source)| source.split_once("fn render_tab"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Sidebar Shell 实现");
    assert!(
        sidebar.contains(".child(self.render_sidebar_header_content(cx))"),
        "Sidebar Shell 必须通过统一分支渲染 Header 内容"
    );
    assert!(
        !sidebar.contains(".children(self.sidebar_header.clone())"),
        "Sidebar Shell 不得把自定义 Header 追加在默认品牌之后"
    );
    assert!(sidebar.contains(".border_b_1()"));
    assert!(APPLICATION_SOURCE.contains("fn workspace_sidebar_footer_host"));
    assert!(APPLICATION_SOURCE.contains(".border_t_1()"));
    assert!(sidebar.contains(".gap_2()"));
    assert!(sidebar.contains("SidebarCollapsible::None"));
    assert!(sidebar.contains("WORKSPACE_SIDEBAR_COLLAPSED_WIDTH"));
    assert!(
        sidebar.contains(".child(self.render_sidebar_header_content(cx))")
            && sidebar.contains(".children(self.render_sidebar_search(cx))"),
        "Sidebar 搜索必须位于 Header 内容之后、导航列表之前"
    );

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
