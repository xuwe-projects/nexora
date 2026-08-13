use gpui::TestAppContext;
use gpui_component::{Size, Theme, ThemeMode, ThemeSet};
use theme::{
    ColorScheme, NEXORA_THEME_PRESET_ID, ThemeCatalog, ThemeCatalogError, ThemePresetSource,
    ThemeSelection, ThemeSelectionError,
};

const NEXORA_THEME_SET: &str = include_str!("../themes/nexora.json");
const CUSTOM_THEME_SET: &str = r##"
{
  "name": "Customer Theme",
  "themes": [
    { "name": "Shared Name", "mode": "light", "colors": { "background": "#ffffff" } },
    { "name": "Shared Name", "mode": "dark", "colors": { "background": "#000000" } }
  ]
}
"##;

fn source(id: &'static str, label: &'static str) -> ThemePresetSource {
    ThemePresetSource::new(id, label, CUSTOM_THEME_SET)
}

#[test]
fn theme_selection_defaults_to_nexora_following_system() {
    let selection = ThemeSelection::default();

    assert_eq!(selection.preset_id(), NEXORA_THEME_PRESET_ID);
    assert_eq!(selection.color_scheme(), ColorScheme::System);
}

#[test]
fn color_scheme_ids_round_trip() {
    for scheme in ColorScheme::ALL {
        assert_eq!(ColorScheme::from_id(scheme.id()), Some(scheme));
    }

    assert_eq!(ColorScheme::from_id("unknown"), None);
}

#[test]
fn embedded_theme_set_contains_light_and_dark_variants() {
    let theme_set: ThemeSet = serde_json::from_str(NEXORA_THEME_SET).unwrap();

    assert_eq!(theme_set.name.as_ref(), "Nexora");
    assert_eq!(theme_set.themes.len(), 2);
    assert_eq!(theme_set.themes[0].mode, ThemeMode::Light);
    assert_eq!(theme_set.themes[1].mode, ThemeMode::Dark);
}

#[test]
fn embedded_themes_distinguish_workspace_and_content_surfaces() {
    let theme_set: serde_json::Value = serde_json::from_str(NEXORA_THEME_SET).unwrap();
    let themes = theme_set["themes"].as_array().unwrap();

    for theme in themes {
        let colors = &theme["colors"];

        assert_ne!(colors["background"], colors["group_box.background"]);
        assert_eq!(colors["group_box.background"], colors["table.background"]);
    }
}

#[test]
fn catalog_preserves_registration_order_and_application_default() {
    let catalog = ThemeCatalog::new(
        &[source("acme", "Acme"), source("ocean_blue", "Ocean")],
        Some("ocean_blue"),
    )
    .unwrap();
    let presets = catalog
        .presets()
        .map(|preset| (preset.id(), preset.label()))
        .collect::<Vec<_>>();

    assert_eq!(
        presets,
        vec![
            ("nexora", "Nexora"),
            ("neutral", "中性"),
            ("acme", "Acme"),
            ("ocean_blue", "Ocean")
        ]
    );
    assert_eq!(catalog.default_preset_id(), "ocean_blue");
}

#[test]
fn catalog_resolves_first_launch_saved_and_legacy_preferences() {
    let catalog = ThemeCatalog::new(&[source("acme", "Acme")], Some("acme")).unwrap();

    assert_eq!(catalog.resolve_preset_id(None), "acme");
    assert_eq!(catalog.resolve_preset_id(Some("nexora")), "nexora");
    assert_eq!(catalog.resolve_preset_id(Some("acme")), "acme");
    assert_eq!(catalog.resolve_preset_id(Some("missing")), "acme");
    assert_eq!(catalog.resolve_preset_id(Some("xuwe")), "nexora");
}

#[test]
fn catalog_rejects_invalid_reserved_and_duplicate_ids() {
    assert!(matches!(
        ThemeCatalog::new(&[source("Acme", "Acme")], None),
        Err(ThemeCatalogError::InvalidId { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[source("nexora", "Nexora")], None),
        Err(ThemeCatalogError::ReservedId { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[source("xuwe", "Legacy")], None),
        Err(ThemeCatalogError::ReservedId { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[source("neutral", "Neutral")], None),
        Err(ThemeCatalogError::ReservedId { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[source("acme", "One"), source("acme", "Two")], None),
        Err(ThemeCatalogError::DuplicateId { .. })
    ));
}

#[test]
fn catalog_rejects_empty_label_invalid_json_and_unknown_default() {
    assert!(matches!(
        ThemeCatalog::new(&[source("acme", "  ")], None),
        Err(ThemeCatalogError::EmptyLabel { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[ThemePresetSource::new("acme", "Acme", "{")], None),
        Err(ThemeCatalogError::InvalidJson { .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(&[source("acme", "Acme")], Some("missing")),
        Err(ThemeCatalogError::UnknownDefaultPreset { .. })
    ));
}

#[test]
fn catalog_rejects_unpaired_theme_sets() {
    let one_theme = r##"{
        "themes": [
            { "name": "Light", "mode": "light", "colors": {} }
        ]
    }"##;
    let duplicate_mode = r##"{
        "themes": [
            { "name": "Light One", "mode": "light", "colors": {} },
            { "name": "Light Two", "mode": "light", "colors": {} }
        ]
    }"##;
    let extra_theme = r##"{
        "themes": [
            { "name": "Light", "mode": "light", "colors": {} },
            { "name": "Dark", "mode": "dark", "colors": {} },
            { "name": "Extra", "mode": "light", "colors": {} }
        ]
    }"##;

    assert!(matches!(
        ThemeCatalog::new(&[ThemePresetSource::new("one", "One", one_theme)], None),
        Err(ThemeCatalogError::InvalidThemeCount { actual: 1, .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(
            &[ThemePresetSource::new(
                "duplicate",
                "Duplicate",
                duplicate_mode
            )],
            None
        ),
        Err(ThemeCatalogError::DuplicateMode { mode: "light", .. })
    ));
    assert!(matches!(
        ThemeCatalog::new(
            &[ThemePresetSource::new("extra", "Extra", extra_theme)],
            None
        ),
        Err(ThemeCatalogError::InvalidThemeCount { actual: 3, .. })
    ));
}

#[gpui::test]
fn custom_theme_switches_and_internal_names_are_isolated(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init_with_catalog(
            ThemeCatalog::new(
                &[source("acme", "Acme"), source("ocean", "Ocean")],
                Some("acme"),
            )
            .unwrap(),
            cx,
        );

        assert_eq!(theme::selection(cx).preset_id(), "acme");
        assert_eq!(Theme::global(cx).light_theme.name, "__nexora_acme_light");
        assert_eq!(Theme::global(cx).dark_theme.name, "__nexora_acme_dark");

        theme::set_preset("ocean", cx).unwrap();
        theme::set_color_scheme(ColorScheme::Dark, cx);

        assert_eq!(theme::selection(cx).preset_id(), "ocean");
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark);
        assert_eq!(Theme::global(cx).light_theme.name, "__nexora_ocean_light");
        assert_eq!(Theme::global(cx).dark_theme.name, "__nexora_ocean_dark");
    });
}

#[gpui::test]
fn unknown_runtime_preset_keeps_current_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(cx);
        let before = theme::selection(cx);

        let result = theme::set_preset("missing", cx);

        assert_eq!(
            result,
            Err(ThemeSelectionError::UnknownPreset {
                id: "missing".to_owned()
            })
        );
        assert_eq!(theme::selection(cx), before);
    });
}

#[gpui::test]
fn font_and_component_size_survive_theme_changes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init_with_catalog(
            ThemeCatalog::new(&[source("acme", "Acme")], Some("acme")).unwrap(),
            cx,
        );

        theme::set_font_size(18, cx);
        theme::set_component_size(Size::Large, cx);
        theme::set_color_scheme(ColorScheme::Dark, cx);

        assert_eq!(theme::font_size(cx), 18);
        assert_eq!(Theme::global(cx).font_size, gpui::px(18.0));
        assert_eq!(theme::component_size(cx), Size::Large);
    });
}
