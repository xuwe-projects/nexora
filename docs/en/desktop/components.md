---
title: Components
order: 3
---

# Components

Nexora desktop components do not replace `gpui-component`. They add framework-level building blocks
for application shells, CRUD pages, form dialogs, labeled fields, tables, and hierarchical pickers.
Applications still depend on and import `gpui` and `gpui_component` directly; Nexora exposes stable
cross-application additions through `nexora::desktop`.

## Quick Start

A generated desktop app usually enables Nexora's `desktop, derive` features and imports the native
GPUI component crates directly:

```toml
[dependencies]
nexora = { version = "0.33.0", features = ["desktop", "derive"] }
gpui = { workspace = true }
gpui-component = { workspace = true }
theme = { workspace = true }
```

Common imports:

```rust
use gpui::{Context, Entity, Render, Window};
use gpui_component::{
    Sizable as _,
    button::Button,
    input::{Input, InputEvent, InputState},
    table::{Column, DataTable, TableState},
};
use nexora::desktop::{
    Cascader, CascaderEvent, CascaderOption, CascaderState,
    CrudPanel, CrudPanelToolbar, CrudTableDelegate, CrudTableSelection,
    FormDialog, FormDialogState, FormItem,
    LabeledControl, TableCell,
};
```

Create long-lived state in a Feature or page-private component during initialization. `render`
should only read state and build elements; do not create `InputState`, subscriptions, async tasks,
or long-lived entities from `render`.

## Overview

| Component | Purpose | State owner |
| --- | --- | --- |
| `FormDialog` | Standard create/edit dialog with draft tracking and discard confirmation | `Entity<FormDialogState>` |
| `FormItem` | Standard field row inside `FormDialog` | Stateless |
| `LabeledControl` | Label, description, and error container; also a typed field entity | Stateless in visual mode; `Entity<LabeledControl<V>>` in field mode |
| `CrudPanel` | Resource-management layout with summary, toolbar, and body | Stateless |
| `CrudPanelToolbar` | CRUD filters and actions | Stateless |
| `CrudTableDelegate` | Connects business rows to `gpui_component::DataTable` | Stored in `TableState` |
| `CrudTableSelection` | Controlled selection column for CRUD tables | Caller owns selected IDs |
| `TableCell` / `TableHeaderCell` | Table cell alignment helpers | Stateless |
| `Cascader` | Single-select hierarchical picker | `Entity<CascaderState>` |
| `SidebarRegion` | Stable region inside Sidebar header/footer | Stateless |

## FormDialog

`FormDialog` is the default container for resource creation and editing. It provides a title,
optional description, scrollable content, cancel, and submit actions. Its overlay is scoped to the
current Feature panel and does not cover the Sidebar or window-level menus. Clicking the overlay does
not close the form; cancel, close, and submit are the explicit intents.

```rust
use gpui::{Context, Entity, Render, Subscription, Window};
use gpui_component::{
    Sizable as _,
    input::{InputEvent, InputState},
};
use nexora::desktop::{FormDialog, FormDialogState, FormItem};

struct UserEditor {
    form: Entity<FormDialogState>,
    name: Entity<InputState>,
    email: Entity<InputState>,
    _name_subscription: Subscription,
}

impl UserEditor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = cx.new(FormDialogState::new);
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("Email"));

        let tracked_form = form.clone();
        let _name_subscription = cx.subscribe(&name, move |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                tracked_form.update(cx, |form, cx| {
                    form.set_field_draft("name", "Name", "", input.read(cx).value().to_string(), cx);
                });
            }
        });

        Self { form, name, email, _name_subscription }
    }
}

impl Render for UserEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        FormDialog::new("create-user-dialog", self.form.clone())
            .title("Create user")
            .description("Fill in the profile fields.")
            .child(FormItem::new("Name").required().input(&self.name))
            .child(FormItem::new("Email").input(&self.email))
            .submit_label("Create")
            .submit_disabled(self.name.read(cx).value().trim().is_empty())
            .with_size(theme::component_size(cx))
            .on_submit(cx.listener(Self::submit))
    }
}
```

### API

| Type | API | Notes |
| --- | --- | --- |
| `FormDialog` | `new(id, state)` | Create a dialog bound to long-lived state |
| `FormDialog` | `title` / `description` / `columns` | Configure the header and field grid |
| `FormDialog` | `child(FormItem)` / `section(element)` | Add fields or full-width custom sections |
| `FormDialog` | `cancel_label` / `submit_label` / `submit_disabled` | Configure actions |
| `FormDialog` | `max_panel_height_ratio` | Clamp the panel-relative maximum height |
| `FormDialog` | `on_submit` / `on_cancel` / `with_size` | Bind behavior and density |
| `FormDialogState` | `open` / `close` / `set_submitting` | Manage visibility and submit state |
| `FormDialogState` | `set_field_draft` / `reset_fields` / `mark_saved` | Track draft state |
| `FormDialogState` | `is_dirty` / `unsaved_fields` / `draft_values` | Read unsaved data |

`FormItem` supports `new`, `description`, `required`, `error`, `input`, `password_input`,
`number_input`, `checkbox`, `field`, `element`, `disabled`, and `full_row`.

## LabeledControl

`LabeledControl` has two modes. `LabeledControl::new(label, child)` is a visual wrapper for a label,
description, required marker, and error text. The typed field builders create
`Entity<LabeledControl<V>>` values that can be registered with `FormDialogState` for submit-time
validation and focus.

```rust
use gpui::SharedString;
use gpui_component::input::InputState;
use nexora::desktop::{FormDialogState, FormItem, LabeledControl};

let name_input = cx.new(|cx| InputState::new(window, cx));
let name_field = LabeledControl::input("name", "Name", &name_input)
    .required("Enter a name")
    .pattern(r"^.{2,32}$", "Name must be 2 to 32 characters")
    .on_change(|event| async move {
        tracing::debug!(name = %event.value(), "name changed");
    })
    .build(window, cx);

let form = cx.new(|cx| FormDialogState::new(cx).field(&name_field));
let item = FormItem::field(&name_field);
```

### API

| Type | API | Notes |
| --- | --- | --- |
| `LabeledControl<()>` | `new(label, child)` | Visual field container |
| `LabeledControl<()>` | `input` / `password_input` / `number_input::<V>` / `checkbox` | Typed field builders |
| `LabeledControlBuilder<V>` | `description` / `required` / `pattern` / `parse_error` | Declarative validation |
| `LabeledControlBuilder<V>` | `on_input` / `on_change` / `on_blur` | Typed async events |
| `LabeledControlBuilder<V>` | `build(window, cx)` | Create the field entity during initialization |
| `LabeledControl<V>` | `key` / `value` / `visible_error` / `has_error` | Read field state |

Async handlers can use `event.current_target()` to set or clear an event error. Stale async results
are discarded after the field has moved to a newer revision.

## CrudPanel

`CrudPanel` is the standard resource-management layout: summary card, optional toolbar, and a body
that fills the remaining height.

```rust
use gpui_component::{Sizable as _, button::Button, input::Input};
use nexora::desktop::{CrudPanel, CrudPanelToolbar};

let toolbar = CrudPanelToolbar::new()
    .filter(Input::new(&self.keyword).placeholder("Search cities"))
    .action(Button::new("search").label("Search"))
    .action(Button::new("create").primary().label("Create"));

CrudPanel::new("Cities", self.render_table(window, cx))
    .description("Manage cities and their countries or regions")
    .refresh("refresh-cities", self.loading, false, cx.listener(Self::reload))
    .toolbar(toolbar)
    .with_size(theme::component_size(cx))
```

### API

| Type | API |
| --- | --- |
| `CrudPanel` | `new`, `description`, `refresh`, `toolbar`, `filter`, `filters`, `action`, `actions`, `has_toolbar`, `with_size` |
| `CrudPanelToolbar` | `new`, `filter`, `filters`, `action`, `actions`, `is_empty` |

Use `refresh` for reloading current data. Put search, create, import, export, and batch operations
in the toolbar action area.

## CrudTableRow and CrudTableDelegate

Prefer `#[derive(nexora::CrudTableRow)]` for CRUD row data, then connect it to
`gpui_component::DataTable` with `CrudTableDelegate<T>`.

```rust
use gpui_component::table::{Column, DataTable, TableState};
use nexora::desktop::{CrudTableDelegate, TableCell};

#[derive(Clone, nexora::CrudTableRow)]
struct CityRow {
    #[nexora(row_id, column(name = "ID", width = 64., fixed_left))]
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
    .action_column(
        Column::new("actions", "Actions").width(gpui::px(160.)).selectable(false),
        |row, _window, _cx| render_row_actions(row),
    )
    .empty_title("No cities");

let table = DataTable::new(cx.new(|cx| TableState::new(delegate, window, cx))).bordered(true);
```

### Derive Attributes

| Attribute | Notes |
| --- | --- |
| `#[nexora(row_id)]` | Required unique business ID |
| `#[nexora(skip)]` | Do not generate a column |
| `#[nexora(column)]` | Generate a column from the field name |
| `column(key = "...")` | Override the column key |
| `column(name = "...")` / `column(title = "...")` | Override the header |
| `column(width = 120.)` / `min_width` / `max_width` | Configure width |
| `column(sortable)` / `ascending` / `descending` | Configure sorting |
| `column(fixed_left)` | Fix the column on the left |
| `column(resizable = false)` / `movable = false` / `selectable = false` | Forward native column behavior |
| `column(header_align = "left")` | Header alignment |
| `column(align = "right")` / `cell_align = "right"` | Body alignment |
| `column(vertical_align = "top")` | Body vertical alignment |
| `column(render = Self::render_status)` | Custom body renderer |
| `column(text = Self::status_text)` | Custom text export |

### Delegate API

`CrudTableDelegate` provides `new`, `rows`, `columns`, `replace_rows`, `append_rows`, `update_rows`,
`set_total`, `set_loading`, `set_loading_more`, `on_load_more`, `selection`, `set_selected_ids`,
`selection_enabled`, `loaded_rows_checked`, `has_selectable_loaded_rows`, `action_column`,
`action_text`, `empty_title`, and `empty_description`.

Selection is controlled. Row and header clicks emit `RowSelectionEvent` and
`LoadedRowsSelectionEvent`; the caller updates its selected IDs, writes them back with
`set_selected_ids`, and notifies the table state.

## Cascader

`Cascader` is a single-select hierarchical picker composed from `gpui-component` Popover, Input,
Button, Icon, and scroll primitives. It supports arbitrary depth, stable value paths, disabled
nodes, clear, search, custom separators, and `change_on_select`.

```rust
use nexora::desktop::{Cascader, CascaderEvent, CascaderOption, CascaderState};

let options = [
    CascaderOption::new("resources", "Resources").children([
        CascaderOption::new("production", "Production").children([
            CascaderOption::new("workshop", "Workshop"),
            CascaderOption::new("line", "Line"),
        ]),
    ]),
];

let cascader = cx.new(|cx| {
    CascaderState::new("resource-cascader", options, window, cx)
        .placeholder("Select resource")
        .separator(" / ")
        .allow_clear(true)
        .searchable(true)
});

cx.subscribe(&cascader, |_, _, event: &CascaderEvent, _| {
    let CascaderEvent::Change(selection) = event;
    tracing::info!(values = ?selection.values(), labels = ?selection.labels());
});

Cascader::new(&cascader).w(gpui::px(280.0))
```

### API

| Type | API |
| --- | --- |
| `CascaderOption` | `new`, `disabled`, `child`, `children`, `value`, `label`, `is_disabled`, `children_ref`, `is_leaf` |
| `CascaderState` | `new`, `placeholder`, `set_search_placeholder`, `separator`, `allow_clear`, `searchable`, `change_on_select`, `disabled`, `selection`, `is_open`, `set_value`, `clear` |
| `CascaderSelection` | `values`, `labels`, `is_empty` |
| `CascaderValueError` | `value`, `depth` |
| `Cascader` | `new(&state)` |

Use `values()` for submission; labels are display text. Each Cascader ID and sibling option value
should be stable and unique.

## Table Helpers

`TableHeaderCell` is centered by default. `TableCell` is vertically centered and left-aligned by
default.

```rust
use nexora::desktop::{TableCell, TableCellVerticalAlign, TableHeaderCell};

TableHeaderCell::new("Amount").right();
TableCell::new("128.00").right().middle();
TableCell::new("Notes").vertical_align(TableCellVerticalAlign::Top);
```

## SidebarRegion

`SidebarRegion::new(id)` creates a stable area inside a Sidebar header or footer. It provides a
stable element ID, horizontal layout, full width, and style refinement, but it does not add implicit
hover, selected backgrounds, cursor, or click behavior.

```rust
use gpui_component::StyledExt as _;
use nexora::desktop::SidebarRegion;

SidebarRegion::new("factory-switcher")
    .px_2()
    .py_1()
    .child(current_factory_name)
```

Use distinct stable IDs for brand, factory switcher, account menu, and similar regions. Add
interactive styling only to regions that are actually interactive.

## Guidelines

- Prefer native `gpui_component` Button, Input, Select, Popover, DataTable, Dialog, and related controls.
- Use `CrudPanel` plus `CrudTableDelegate` for ordinary resource-management pages.
- Use `FormDialog` for create/edit workflows, with input state, subscriptions, and submit tasks owned by a page-private component.
- Use typed `LabeledControl` when fields need declarative validation, async validation, or submit-time focus.
- Use `Cascader` only when the business value is a hierarchical path; use native selection components for flat enums.
- Pass `theme::component_size(cx)` to sizable components so the application density setting applies immediately.
