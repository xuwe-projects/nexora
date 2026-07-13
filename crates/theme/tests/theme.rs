use gpui::TestAppContext;
use gpui_component::{Theme, ThemeMode, ThemeSet};
use theme::{ColorScheme, ThemePreset, ThemeSelection};

const XUWE_THEME_SET: &str = include_str!("../themes/xuwe.json");

#[test]
fn theme_selection_defaults_to_xuwe_following_system() {
    let selection = ThemeSelection::default();

    assert_eq!(selection.preset(), ThemePreset::Xuwe);
    assert_eq!(selection.color_scheme(), ColorScheme::System);
}

#[test]
fn theme_option_ids_round_trip() {
    for preset in ThemePreset::ALL {
        assert_eq!(ThemePreset::from_id(preset.id()), Some(preset));
    }

    for scheme in ColorScheme::ALL {
        assert_eq!(ColorScheme::from_id(scheme.id()), Some(scheme));
    }

    assert_eq!(ThemePreset::from_id("unknown"), None);
    assert_eq!(ColorScheme::from_id("unknown"), None);
}

#[test]
fn embedded_theme_set_contains_light_and_dark_variants() {
    let theme_set: ThemeSet = serde_json::from_str(XUWE_THEME_SET).unwrap();

    assert_eq!(theme_set.name.as_ref(), "Xuwe");
    assert_eq!(theme_set.themes.len(), 2);
    assert_eq!(theme_set.themes[0].name.as_ref(), "Xuwe Light");
    assert_eq!(theme_set.themes[0].mode, ThemeMode::Light);
    assert_eq!(theme_set.themes[1].name.as_ref(), "Xuwe Dark");
    assert_eq!(theme_set.themes[1].mode, ThemeMode::Dark);
}

#[test]
fn embedded_themes_distinguish_workspace_and_content_surfaces() {
    let theme_set: serde_json::Value = serde_json::from_str(XUWE_THEME_SET).unwrap();
    let themes = theme_set["themes"].as_array().unwrap();

    for theme in themes {
        let colors = &theme["colors"];

        assert_ne!(colors["background"], colors["group_box.background"]);
        assert_eq!(colors["group_box.background"], colors["table.background"]);
    }
}

#[gpui::test]
fn theme_global_initializes_and_switches_inside_gpui(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(cx);

        assert_eq!(theme::selection(cx), ThemeSelection::default());

        theme::set_color_scheme(ColorScheme::Dark, cx);

        assert_eq!(theme::selection(cx).color_scheme(), ColorScheme::Dark);
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark);
    });
}
