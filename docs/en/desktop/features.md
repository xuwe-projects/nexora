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

`section` becomes a visible sidebar group label. Different sections are visually separated.
Use `parent` for nested navigation and set `navigation = false` for parameterized routes.

Custom `SidebarHeader` and `SidebarFooter` implementations provide content only. The Shell keeps
the shared hover surface, padding, and the dividers below the header and above the footer.

Create long-lived entities, subscriptions, and tasks in lifecycle methods rather than `render`.
Import gpui-component directly:

```rust
use gpui_component::button::Button;
```
