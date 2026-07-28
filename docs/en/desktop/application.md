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
