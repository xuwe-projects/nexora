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
to custom cancel handlers. `submit_disabled(true)` only disables submit and keeps cancel available;
`set_submitting(true)` blocks cancel, close, and duplicate submit while work is in flight. Business
code must implement `on_submit`, call `mark_saved` after success, and then `close`.

Create the form component in `FeatureElement::initialize` and always return the same overlay Entity
from `panel_overlay`; do not create inputs, subscriptions, or tasks from `render`.

## CrudPanel and CrudTableRow

`CrudPanel` is the standard three-part resource-management layout: a summary card, an optional
filter/action toolbar, and a main body that fills the remaining height. The header refresh action
uses the shared `rotate-ccw.svg` icon and means “reload current data”; search, create, import,
export, and batch actions belong in `CrudPanelToolbar`.

CRUD tables should prefer `#[derive(nexora::CrudTableRow)]` for row data and
`CrudTableDelegate<T>` to connect those rows to gpui-component `DataTable`. Field attributes only
describe `Column` options, header/body alignment, and custom rendering. Operation columns are added
with `action_column`; complex tables can still implement the native `TableDelegate` directly.

```rust
use gpui_component::table::{Column, DataTable, TableState};
use nexora::desktop::{CrudTableDelegate, CrudPanel, CrudPanelToolbar, TableCell};

#[derive(Clone, nexora::CrudTableRow)]
struct CityRow {
    #[nexora(column(name = "ID", width = 64., fixed_left))]
    id: u64,
    #[nexora(column(title = "City", width = 160., sortable))]
    name: String,
    #[nexora(column(title = "Status", width = 76., align = "center", render = Self::status_cell))]
    enabled: bool,
}

impl CityRow {
    fn status_cell(row: &Self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> TableCell {
        TableCell::new(if row.enabled { "Enabled" } else { "Disabled" }).center()
    }
}

let delegate = CrudTableDelegate::new(rows)
    .row_id(|row| format!("city-{}", row.id))
    .action_column(Column::new("actions", "Actions").width(gpui::px(160.)), render_actions);
let table = DataTable::new(cx.new(|cx| TableState::new(delegate, window, cx))).bordered(true);
let panel = CrudPanel::new("Cities", table).toolbar(CrudPanelToolbar::new());
```

Headers use `TableHeaderCell` and are horizontally and vertically centered by default. Body cells use
`TableCell`, vertically centered and left-aligned by default, with `.left()`, `.center()`, `.right()`,
`.top()`, `.middle()`, and `.bottom()` for per-column overrides. Grid lines should use native
`DataTable::bordered(true)` or related table styles.

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
