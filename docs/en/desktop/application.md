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

## Automatic Account detection

The `desktop` feature compiles Account client capabilities, but regular applications keep the gate
disabled. Installing the authenticator in `Application::initialize` automatically enables the login
gate and default user, role, and permission pages; `ApplicationOptions` has no duplicate switch.
