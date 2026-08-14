use actions::search::OpenGlobalSearch;
use gpui::Action;

#[test]
fn global_search_action_has_a_stable_type_identity() {
    let action: Box<dyn Action> = Box::new(OpenGlobalSearch);
    assert_eq!(action.name(), "global_search::OpenGlobalSearch");
}
