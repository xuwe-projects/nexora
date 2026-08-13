---
title: Application and Branding
order: 1
---

# Application and Branding

`Application` starts GPUI, discovers registrations, and creates the main window. Import GPUI types
directly from `gpui`:

```rust
use gpui::App;
use nexora::{Application as _, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("My App")
            .application_version(env!("CARGO_PKG_VERSION"))
    }

    fn initialize(&mut self, _cx: &mut App) {}
}
```

## Application singleton and window lifecycle

Nexora allows one main application instance by default and hosts the main window, extra Shells,
the unique Settings window, and registered Windows in the same GPUI event loop. The default
configuration therefore uses one OS process, allowing application Globals, business models,
Account state, preferences, and caches to be shared by every window. Each window still owns its
Shell, Feature UI, focus, input, and form drafts. Windows cannot be switched to subprocess hosting.

Extra Shells, the unique Settings window, and registered Windows remain available while the
application is running. Nexora persists only the main window's display, position, size, and
maximized/fullscreen state. It does not persist tabs, extra Shells, Settings, or registered Window
sessions. Every cold start creates exactly one main window and opens `initial_path`. Duplicate
launches still activate the running process through the application-identity singleton IPC.

With in-process Shells, `Context<T>::navigate` targets the Shell
containing the source entity, `App::navigate` targets the active Shell, and unresolved origins fall
back to the main Shell.

### Migrating from window-session releases

Remove `.single_instance(...)`, `.subprocess_windows(...)`, and `.restore_window_sessions(...)`
calls; the application-identity singleton gate is mandatory and there is no replacement switch.
`WindowSession`, `WindowSessionRole`, and `WindowTabSession` have also been removed from the public
Rust API. When an old `workspace.toml` is first read, schema 0 keeps `main_window` geometry and
drops `pinned_tabs`; schema 1 extracts only the main-window placement from the `windows` entry whose
ID is `main`. A successful save removes the retired session fields while preserving theme,
DataTable layout, and non-secret Account preferences.

## Logo

The default login page and sidebar header share the branding configuration:

```rust
use nexora::ApplicationLogo;

ApplicationOptions::new().application_logo(ApplicationLogo::png(include_bytes!(
    "../assets/logos/logo-icon-128.png"
)))
```

The generator copies the PNG, ICNS, and ICO icon set into the desktop package's `assets/logos`
directory. Changing the name, version, or logo does not require a custom login feature. Use the
singleton `LoginFeature` override only when replacing the complete layout.

A custom `SidebarHeader` replaces the default brand area. The Shell owns the header boundary and
divider but does not add interaction styles. When the header should show both brand and application
context, compose stable-ID `SidebarRegion` values inside the custom header so a non-interactive logo
and an interactive selector remain separate hit regions.

## Sidebar Navigation Search

Sidebar navigation search is disabled by default. When enabled, the Shell renders a search input
between the default or custom `SidebarHeader` and the navigation list, filtering only the Section,
NavigationGroup, and Feature titles visible to the current user:

```rust
use nexora::ApplicationOptions;

ApplicationOptions::new().sidebar_search(true)
```

Matching supports original title substrings, tone-less full pinyin, and pinyin initials. During
search, groups use temporary expansion state; clearing the query restores the user's previous
expanded/collapsed state.

## Tab Style

The main-window Feature tabs use the official default `Tabs` style from gpui-component's story by
default. Applications can switch to the official `Tab`, `Underline`, `Pill`, `Outline`, or
`Segmented` variant through `ApplicationOptions::tab_style` without replacing tab switching,
pinning, scrolling, or context-menu behavior. The tab bar applies `theme::component_size(cx)` so it
follows the component-size setting:

```rust
use nexora::{ApplicationOptions, ApplicationTabStyle};

ApplicationOptions::new().tab_style(ApplicationTabStyle::Underline)
```

## Registering application themes

Downstream applications can embed multiple gpui-component `ThemeSet` JSON files. Every preset must
contain exactly one `light` and one `dark` theme. Stable IDs use ASCII `snake_case`; `nexora` and
the legacy alias `xuwe` are reserved:

```rust
use nexora::{ApplicationOptions, ApplicationThemePreset};

ApplicationOptions::new()
    .theme_preset(ApplicationThemePreset::new(
        "acme",
        "Acme",
        include_str!("../themes/acme.json"),
    ))
    .theme_preset(ApplicationThemePreset::new(
        "ocean_blue",
        "Ocean Blue",
        include_str!("../themes/ocean-blue.json"),
    ))
    .default_theme_preset("acme")
```

`Application::validate()` and `run()` validate the JSON, IDs, light/dark pairing, and application
default before entering the GPUI event loop. The default Settings window lists Nexora and every
application preset. A selection updates the Shell, Features, Settings, and all existing or newly
created windows, then persists through the shared Shell preferences.

Startup priority is an existing valid user preference, then the application default, then Nexora.
An existing valid Nexora preference remains selected after an upgrade adds a branded default. A
removed preset falls back to the application default and the repaired ID is persisted. Resetting
appearance also selects the application default and follows the system color scheme.

A custom `SettingsWindow` should use the stable facade instead of mutating gpui-component's global
Theme directly:

```rust
use gpui::App;
use nexora::desktop::{
    ColorScheme, default_theme_preset_id, set_color_scheme, set_theme_preset,
    theme_presets, theme_selection,
};

fn select_brand_theme(cx: &mut App) -> Result<(), nexora::desktop::ThemeSelectionError> {
    for preset in theme_presets(cx) {
        println!("{}: {}", preset.id(), preset.label());
    }
    set_theme_preset("acme", cx)?;
    set_color_scheme(ColorScheme::System, cx);
    let _current = theme_selection(cx);
    let _application_default = default_theme_preset_id(cx);
    Ok(())
}
```

Successful changes refresh all windows and persist automatically. An unknown ID returns a
structured error without changing the current selection.

## Panel Header Actions

Applications can install actions for the right side of the main panel header from
`Application::initialize`. The Shell renders these actions on every business Feature `PanelHeader`,
before the built-in current-tab pin toggle:

```rust
use gpui::App;
use gpui_component::{Sizable as _, button::Button};
use nexora::{PanelHeaderAction, install_panel_header_actions};

fn initialize(&mut self, cx: &mut App) {
    install_panel_header_actions(
        vec![PanelHeaderAction::new(|_cx| {
            Button::new("open-tasks")
                .small()
                .label("Tasks")
                .on_click(|_, _, _| {
                    // Trigger navigation, open a dialog, or dispatch an application action here.
                })
        })],
        cx,
    );
}
```

The `PanelHeaderAction` render closure receives the current `App` context while the header is being
rendered. It should only read state and build elements; navigation, dialogs, network requests, or
business side effects should live in the element event callbacks. Calling
`install_panel_header_actions` again replaces the previous list, and passing an empty list clears the
installed actions.

## Automatic Account detection

The `desktop` feature compiles Account client capabilities, but regular applications keep the gate
disabled. Installing the authenticator in `Application::initialize` automatically enables the login
gate and default user, role, and permission pages; `ApplicationOptions` has no duplicate switch.
