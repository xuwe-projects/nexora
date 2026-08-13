#![cfg(all(feature = "desktop", feature = "derive"))]

use gpui::TestAppContext;
use nexora::desktop::{self, ThemeSelectionError};
use theme::{ThemeCatalog, ThemePresetSource};

const CUSTOM_THEME: &str = r##"{
    "themes": [
        { "name": "Light", "mode": "light", "colors": { "background": "#ffffff" } },
        { "name": "Dark", "mode": "dark", "colors": { "background": "#000000" } }
    ]
}"##;

#[gpui::test]
fn desktop_facade_lists_registered_themes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init_with_catalog(
            ThemeCatalog::new(
                &[ThemePresetSource::new("acme", "Acme", CUSTOM_THEME)],
                Some("acme"),
            )
            .unwrap(),
            cx,
        );

        assert_eq!(desktop::default_theme_preset_id(cx), "acme");
        assert_eq!(
            desktop::theme_presets(cx)
                .map(|preset| (preset.id(), preset.label()))
                .collect::<Vec<_>>(),
            vec![("nexora", "Nexora"), ("neutral", "中性"), ("acme", "Acme")]
        );

        assert_eq!(desktop::theme_selection(cx).preset_id(), "acme");
    });
}

#[gpui::test]
fn desktop_facade_rejects_unknown_theme_without_mutation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(cx);
        let before = desktop::theme_selection(cx);

        let error = desktop::set_theme_preset("missing", cx).unwrap_err();

        assert_eq!(
            error,
            ThemeSelectionError::UnknownPreset {
                id: "missing".to_owned()
            }
        );
        assert_eq!(desktop::theme_selection(cx), before);
    });
}
