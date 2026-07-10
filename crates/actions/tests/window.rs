use actions::window;
use gpui::{Menu, MenuItem};

#[test]
fn application_menus_expose_quit_and_window_commands() {
    let menus = window::application_menus("Xuwe Console");

    assert_eq!(
        menus
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>(),
        vec!["Xuwe Console", "Window"]
    );
    assert_eq!(action_names(&menus[0]), vec!["Quit Xuwe Console"]);
    assert_eq!(
        action_names(&menus[1]),
        vec!["Minimize", "Zoom", "Toggle Full Screen"]
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
