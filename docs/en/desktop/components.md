---
title: Shared Desktop Components
order: 3
---

# Shared Desktop Components

Nexora uses gpui-component directly wherever possible and only adds missing cross-application
interactions under `nexora::desktop`.

## FormDialog

`FormDialog` is the default create/edit form container. It has a title and optional description, a
vertically scrollable content region, and cancel/submit actions. Its `PanelDialog` overlay only covers
the active Feature panel, leaving the Sidebar available.

Keep a long-lived `Entity<FormDialogState>` next to input entities. Record each input change with
`set_field_draft`. The default cancel path closes a clean form and presents field names and draft
values before discarding a dirty form. `is_dirty`, `unsaved_fields`, and `draft_values` are available
to custom cancel handlers. Business code must implement `on_submit`, use `set_submitting` around
asynchronous work, call `mark_saved` after success, and then `close`.

Create the form component in `FeatureElement::initialize` and always return the same overlay Entity
from `panel_overlay`; do not create inputs, subscriptions, or tasks from `render`.

## Cascader

`Cascader` is a single-select hierarchical picker composed from gpui-component Popover, Input,
Button, Icon, and scrolling primitives. `CascaderOption` builds an arbitrary-depth option tree;
`CascaderSelection::values()` returns the stable value path while `labels()` returns display text.

Create `Entity<CascaderState>` during initialization with a stable unique ID, subscribe to
`CascaderEvent::Change`, and render `Cascader::new(&state)`. The state supports disabled nodes,
clear, search, custom separators, `change_on_select`, and controlled `set_value`. An unknown value
returns `CascaderValueError` without overwriting the previous selection.

## Card and SidebarRegion

`Card` uses the themed `group_box` surface, border, radius, and shadow so data tables and forms remain
visually distinct from the desktop workspace. `SidebarRegion::new(id)` defines an independent stable
hit region without implicit hover, selected, cursor, or click behavior.
