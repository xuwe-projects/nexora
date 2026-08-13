const APPLICATION_SOURCE: &str = include_str!("../src/application.rs");

#[test]
fn tab_bar_keeps_navigation_prefix_feature_icons_and_open_page_suffix() {
    let tabs = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_active_feature"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Shell 标签栏实现");

    assert!(tabs.contains(".h(px(42.0))"));
    assert!(tabs.contains(".prefix(self.render_tab_bar_prefix(cx))"));
    assert!(tabs.contains(".suffix(self.render_tab_bar_suffix(cx))"));

    let tab = APPLICATION_SOURCE
        .split_once("fn render_tab(")
        .and_then(|(_, source)| source.split_once("fn build_tab_context_menu"))
        .map(|(source, _)| source)
        .expect("应当可以定位单个 Feature 标签实现");
    assert!(tab.contains(".prefix(feature_icon(route.icon()))"));

    let prefix = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_prefix")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_suffix"))
        .map(|(source, _)| source)
        .expect("应当可以定位标签栏导航前缀实现");
    for action in ["tabs-back", "tabs-forward", "tabs-reload"] {
        assert!(prefix.contains(action), "标签栏前缀缺少 {action}");
    }
    assert!(
        !prefix.contains(".when("),
        "刷新入口必须始终留在 prefix，不支持刷新时仅禁用"
    );
    assert!(prefix.contains("!= crate::FeatureReloadAvailability::Available"));

    let suffix = APPLICATION_SOURCE
        .split_once("fn render_tab_bar_suffix")
        .and_then(|(_, source)| source.split_once("fn account_partition_id"))
        .map(|(source, _)| source)
        .expect("应当可以定位标签栏后缀实现");
    assert!(suffix.contains("open-feature-search"));
    assert!(suffix.contains("SearchMode::OpenPage"));
}

#[test]
fn global_search_trigger_matches_prototype_structure() {
    let global_bar = APPLICATION_SOURCE
        .split_once("fn render_global_title_bar_content")
        .and_then(|(_, source)| source.split_once("fn render_tab_bar_content"))
        .map(|(source, _)| source)
        .expect("应当可以定位全局顶栏实现");

    for required in [
        "nexora-global-search-trigger",
        ".w(px(420.0))",
        ".h(px(30.0))",
        ".rounded(px(8.0))",
        "Icon::new(IconName::Search)",
        "搜索或跳转到…",
        "Kbd::new(search_shortcut)",
    ] {
        assert!(global_bar.contains(required), "全局搜索入口缺少 {required}");
    }
}
