---
title: Features and Navigation
order: 2
---

# Features and Navigation

```rust
use gpui::{Context, IntoElement, Window, div};
use nexora::FeatureElement;

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "Home",
    path = "/",
    section = "Workspace",
    icon = "layout-dashboard",
    order = 0
)]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().child("Hello Nexora")
    }
}
```

`section` is the top-level sidebar category. Use `NavigationGroup` for a directory that only expands
or collapses, then reference its stable ID from leaf features with `group`:

```rust
#[derive(nexora::NavigationGroup)]
#[nexora(id = "production-model", title = "Production Model", section = "Resources")]
struct ProductionModelGroup;

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "Workshops",
    path = "/workshops",
    section = "Resources",
    group = "production-model"
)]
struct WorkshopsFeature;
```

A NavigationGroup has no path, page factory, window, or tab. Its `parent` may only reference another
NavigationGroup in the same section. A Feature always represents a navigable page and can no longer
act as a directory. Registration rejects duplicate IDs, unknown references, self references, cycles,
and cross-section references. Active leaf ancestors expand after navigation and route restoration;
group breadcrumb labels are not links. Set `navigation = false` for parameterized routes.

Custom `SidebarHeader` and `SidebarFooter` implementations provide content only. The Shell keeps
structural spacing and dividers but injects no hover, selected background, radius, cursor, or click
behavior. Compose independent brand and application-context hit regions with stable-ID
`nexora::desktop::SidebarRegion` values.

Create long-lived entities, subscriptions, and tasks in lifecycle methods rather than `render`.
Import gpui-component directly:

```rust
use gpui_component::button::Button;
```
