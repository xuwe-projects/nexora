use actions::window;
use gpui::{Menu, MenuItem};

#[test]
fn application_menus_expose_quit_and_window_commands() {
    let menus = window::application_menus("Nexora Console");

    assert_eq!(
        menus
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>(),
        vec!["Nexora Console", "Window"]
    );
    assert_eq!(action_names(&menus[0]), vec!["Quit Nexora Console"]);
    assert_eq!(
        action_names(&menus[1]),
        vec!["Minimize", "Zoom", "Toggle Full Screen"]
    );
}

#[test]
fn application_menu_only_exposes_updates_when_enabled() {
    let without_updates = window::application_menus_with_updates("Nexora Console", false);
    let with_updates = window::application_menus_with_updates("Nexora Console", true);

    assert_eq!(
        action_names(&without_updates[0]),
        vec!["Quit Nexora Console"]
    );
    assert_eq!(
        action_names(&with_updates[0]),
        vec!["检查更新…", "Quit Nexora Console"]
    );
}

fn action_names(menu: &Menu) -> Vec<&str> {
    menu.items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect()
}
