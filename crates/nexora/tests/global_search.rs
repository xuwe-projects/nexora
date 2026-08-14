#![cfg(feature = "desktop")]

use std::time::Duration;

use gpui::{Task, TestAppContext};
use nexora::{
    SearchAction, SearchItem, SearchMode, SearchProvider, SearchSection, install_search_providers,
};

const GLOBAL_SEARCH_SOURCE: &str = include_str!("../src/global_search.rs");

#[test]
fn search_provider_declares_stable_identity_modes_and_history_resolution() {
    let provider = SearchProvider::new("imes.resources", 20)
        .modes([SearchMode::Global, SearchMode::Custom("stock".to_owned())])
        .debounce(Duration::from_millis(300))
        .on_resolve_history(|_, _, _, _| Task::ready(Ok(None)));

    assert_eq!(provider.provider_id(), "imes.resources");
    assert_eq!(provider.order(), 20);
    assert!(provider.supports(&SearchMode::Global));
    assert!(provider.supports(&SearchMode::Custom("stock".to_owned())));
    assert!(!provider.supports(&SearchMode::OpenPage));
    assert!(provider.resolves_history());
}

#[test]
fn search_sections_keep_provider_and_item_identity() {
    let item = SearchItem::new("nexora.features", "users", "用户", |_, _, _| {
        Task::ready(Ok(SearchAction::Close))
    })
    .description("/users");
    let section = SearchSection::new("pages", "页面").item(item);

    assert_eq!(section.section_id(), "pages");
    assert_eq!(section.title().as_ref(), "页面");
    assert_eq!(section.search_items().len(), 1);
    assert_eq!(section.search_items()[0].provider_id(), "nexora.features");
    assert_eq!(section.search_items()[0].item_id(), "users");
    assert_eq!(section.search_items()[0].title().as_ref(), "用户");
}

#[test]
fn enter_confirmation_is_handled_by_the_search_dialog_action_boundary() {
    let render = GLOBAL_SEARCH_SOURCE
        .split_once("impl Render for SearchDialog")
        .map(|(_, source)| source)
        .expect("应当可以定位 SearchDialog 渲染实现");
    assert!(render.contains(".on_action(cx.listener(Self::confirm_selection))"));

    let subscription = GLOBAL_SEARCH_SOURCE
        .split_once("let _input_subscription = cx.subscribe_in(")
        .and_then(|(_, source)| source.split_once("let provider_states"))
        .map(|(source, _)| source)
        .expect("应当可以定位 Input 事件订阅");
    assert!(subscription.contains("InputEvent::PressEnter { .. } => {}"));
    assert!(!subscription.contains("this.activate_item"));
}

#[gpui::test]
#[should_panic(expected = "SearchProvider ID 不能重复")]
fn duplicate_provider_ids_are_rejected(cx: &mut TestAppContext) {
    cx.update(|cx| {
        install_search_providers(
            vec![
                SearchProvider::new("duplicate", 0),
                SearchProvider::new("duplicate", 1),
            ],
            cx,
        );
    });
}
