# Data Display & Charts

Reference for data display components (Table, List, Tree, VirtualList, DescriptionList, Settings) and chart/plot components in gpui-component.

**Contents:** [IndexPath](#indexpath) · [Table](#table) · [List](#list) · [Tree](#tree) · [VirtualList](#virtuallist) · [DescriptionList](#descriptionlist) · [Settings](#settings) · [Charts](#charts) · [Plot Toolkit](#plot-toolkit)

---

## IndexPath

Used by `ListDelegate`, `TreeDelegate`, and `SearchableListDelegate` to reference item positions.

```rust
pub struct IndexPath {
    pub section: usize,  // section index (default: 0)
    pub row: usize,      // item index within the section
    pub column: usize,   // column index (default: 0, rarely used)
}
```

```rust
// Construction
IndexPath::new(3)               // section=0, row=3, column=0
IndexPath::new(5).section(1)    // section=1, row=5
IndexPath::new(0).row(2).column(1)  // section=0, row=2, column=1
```

**In ListDelegate.render_item**: access `ix.row` (and optionally `ix.section`) to index into your data source.

```rust
fn render_item(&mut self, ix: IndexPath, w: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<Self::Item> {
    let shortcut = self.shortcuts.get(ix.row)?;
    Some(ListItem::new(("item", ix.row)).child(shortcut.name))
}
```

**In ListDelegate.set_selected_index**: `ix` is `Option<IndexPath>`. Use `ix.map(|i| i.row)` to extract the selected row.


---

## Table

Two table APIs: a simple **composable table** (`Table`) and a feature-rich **data table** (`DataTable`) with virtual scrolling, sorting, resizing, cell/row/column selection, and keyboard navigation.

### Basic Table (`Table`)

A lightweight, stateless, composable table without virtual scrolling or column management. Use for small, static data sets.

**Component hierarchy:**

```
Table
  TableHeader
    TableRow
      TableHead        -- header cell
  TableBody
    TableRow
      TableCell        -- data cell
  TableFooter
    TableRow
      TableCell
  TableCaption
```

**Example:**

```rust
Table::new()
    .small()
    .child(TableHeader::new().child(
        TableRow::new()
            .child(TableHead::new().child("Name"))
            .child(TableHead::new().child("Email"))
    ))
    .child(TableBody::new()
        .child(TableRow::new()
            .child(TableCell::new().child("John"))
            .child(TableCell::new().child("john@example.com")))
    )
    .child(TableCaption::new().child("A list of recent invoices."))
```

**Components:**
- `Table::new()` — root; implements `Sizable`, `ChildElement`, `RenderOnce`.
- `TableHeader::new()` — wraps header rows.
- `TableBody::new()` — wraps data rows.
- `TableFooter::new()` — wraps footer rows.
- `TableRow::new()` — a single row; implements `Sizable`, `ChildElement`.
- `TableHead::new()` — header cell; implements `ParentElement` (accepts children).
- `TableCell::new()` — data cell; implements `ParentElement`.
- `TableCaption::new()` — caption displayed below the table.

Size set via `Sizable` on `Table` is automatically propagated to all children.

### Data Table (`DataTable`)

A stateful table with virtual scrolling, column management, and multi-mode selection.

**Core types:**

```rust
use gpui_component::table::{DataTable, TableState, TableDelegate, Column, ColumnGroup, ColumnSort, TableEvent};
```

#### `TableState<D: TableDelegate>`

Create via `cx.new(|cx| TableState::new(delegate, cx))`. Key public fields and builder methods:

```rust
// Selection modes
let state = TableState::new(delegate, cx)
    .cell_selectable(true)    // enable individual cell selection
    .row_selectable(true)     // enable row selection
    .col_selectable(true);    // enable column selection (default: false)

// Public fields (set directly or via builder)
state.cell_selectable = true;
state.row_header = false;     // hide row header column when cell_selectable
state.loop_selection = true;  // wrap selection at boundaries (default: true)
state.sortable = true;
state.col_resizable = true;
state.col_movable = true;
state.col_fixed = true;

// Scroll handles
state.vertical_scroll_handle;   // UniformListScrollHandle
state.horizontal_scroll_handle; // VirtualListScrollHandle
```

**Selection modes:**
- **Row Selection** (default): click rows; emits `SelectRow`, `DoubleClickedRow`, `RightClickedRow`.
- **Column Selection**: click column headers; emits `SelectColumn`.
- **Cell Selection**: click individual cells; emits `SelectCell`, `DoubleClickedCell`, `RightClickedCell`. A row header column appears on the left for row selections. Keyboard navigation (arrow keys, Tab, Home, End, PageUp, PageDown) works at cell level.

**Subscribe to events:**

```rust
cx.subscribe(&table_state, |this, table, event, cx| {
    match event {
        TableEvent::SelectCell(row_ix, col_ix) => { /* ... */ }
        TableEvent::DoubleClickedCell(row_ix, col_ix) => { /* ... */ }
        TableEvent::RightClickedCell(row_ix, col_ix) => { /* ... */ }
        TableEvent::RightClickedRow(Some(row_ix)) => { /* show context menu */ }
        TableEvent::SelectRow(row_ix) => { /* ... */ }
        TableEvent::ColumnWidthsChanged(widths) => { /* ... */ }
        TableEvent::MoveColumn(from, to) => { /* ... */ }
        TableEvent::ClearSelection => { /* ... */ }
        _ => {}
    }
});
```

**Rendering:**

```rust
DataTable::new(&table_state)
    .stripe(true)     // alternate row background colors
    .bordered(true)   // cell borders
```

#### `TableDelegate` Trait

Implement this trait on your data model to supply the table with data and rendering.

```rust
pub trait TableDelegate: Sized + 'static {
    // --- Required ---
    fn columns_count(&self, cx: &App) -> usize;
    fn rows_count(&self, cx: &App) -> usize;
    fn column(&self, col_ix: usize, cx: &App) -> Column;
    fn render_td(
        &mut self, row_ix: usize, col_ix: usize,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement;

    // --- Optional (with defaults) ---
    fn render_header(
        &mut self, window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> { div().id("header") }

    fn render_th(
        &mut self, col_ix: usize,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // default: renders column name in a centered div
    }

    fn render_tr(
        &mut self, row_ix: usize,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> { div().id(("row", row_ix)) }

    // Sorting
    fn perform_sort(
        &mut self, col_ix: usize, sort: ColumnSort,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) {}

    // Column groups (multi-level headers)
    fn group_headers(&self, cx: &App) -> Option<Vec<Vec<ColumnGroup>>> { None }
    fn render_group_th(
        &mut self, label: &SharedString, col_span: usize, width: Pixels,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement;

    // Column reorder / context menu
    fn move_column(
        &mut self, col_ix: usize, to_ix: usize,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) {}

    fn context_menu(
        &mut self, row_ix: usize, menu: PopupMenu,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu { menu }

    // Empty / loading states
    fn render_empty(
        &mut self, window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement;

    fn loading(&self, cx: &App) -> bool { false }
    fn render_loading(
        &mut self, size: Size,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement { Loading::new().size(size) }

    // Infinite scroll (pagination)
    fn has_more(&self, cx: &App) -> bool { false }
    fn load_more_threshold(&self) -> usize { 20 }
    fn load_more(
        &mut self, window: &mut Window, cx: &mut Context<TableState<Self>>
    ) {}

    // Visible range callbacks (called frequently — keep fast)
    fn visible_rows_changed(
        &mut self, visible_range: Range<usize>,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) {}

    fn visible_columns_changed(
        &mut self, visible_range: Range<usize>,
        window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) {}

    // Extra trailing column
    fn render_last_empty_col(
        &mut self, window: &mut Window, cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement { h_flex().w_3().h_full().flex_shrink_0() }

    // Export support
    fn cell_text(&self, row_ix: usize, col_ix: usize, cx: &App) -> String { String::new() }
}
```

**Typical cell rendering pattern:**

```rust
fn render_td(&mut self, row_ix: usize, col_ix: usize,
    window: &mut Window, cx: &mut Context<TableState<Self>>,
) -> impl IntoElement {
    let row = &self.data[row_ix];
    let col = self.column(col_ix, cx);
    match col.key.as_ref() {
        "name" => div().child(row.name.clone()),
        "status" => div().child(format!("{:?}", row.status)),
        _ => div().child("—"),
    }
}
```

#### `Column`

Defines column metadata. Builder pattern via `FluentBuilder`.

```rust
pub struct Column {
    pub key: SharedString,         // unique identifier (match to data source field)
    pub name: SharedString,        // display name
    pub align: TextAlign,          // text alignment (default: Left)
    pub sort: Option<ColumnSort>,  // None = not sortable
    pub paddings: Option<Edges<Pixels>>,
    pub width: Pixels,             // default: 100px
    pub fixed: Option<ColumnFixed>,// None = not fixed; Some(Left) = pin to left
    pub resizable: bool,           // default: true
    pub movable: bool,             // default: true
    pub selectable: bool,          // default: true (false for action columns)
    pub min_width: Pixels,         // default: 20px
    pub max_width: Pixels,         // default: f32::MAX
}
```

```rust
Column::new("name", "Name")
    .width(px(200.))
    .sort(ColumnSort::Ascending)
    .fixed(ColumnFixed::Left)
    .resizable(true)

Column::new("actions", "")
    .width(px(60.))
    .selectable(false)
    .resizable(false)
```

#### `ColumnSort`

```rust
pub enum ColumnSort {
    Default,     // no sorting
    Ascending,
    Descending,
}
```

#### `ColumnGroup`

Multi-level header grouping:

```rust
pub struct ColumnGroup {
    pub label: SharedString,  // display label
    pub span: usize,          // how many columns this group spans
}

ColumnGroup::new("Personal Info", 2)
```

#### `TableEvent`

```rust
pub enum TableEvent {
    SelectRow(usize),
    DoubleClickedRow(usize),
    SelectColumn(usize),
    SelectCell(usize, usize),          // (row_ix, col_ix)
    DoubleClickedCell(usize, usize),   // (row_ix, col_ix)
    ColumnWidthsChanged(Vec<Pixels>),
    MoveColumn(usize, usize),          // (from_ix, to_ix)
    RightClickedRow(Option<usize>),     // None = empty area
    RightClickedCell(usize, usize),
    ClearSelection,
}
```

---

## List

A vertically scrolling list with optional search, sections, headers/footers, keyboard navigation, and virtualized rendering.

### Core Types

```rust
use gpui_component::list::{List, ListState, ListDelegate, ListEvent, ListItem, IndexPath};
```

#### `ListState<D: ListDelegate>`

Create via `cx.new(|cx| ListState::new(delegate, window, cx))`.

```rust
// Builder
ListState::new(delegate, window, cx)
    .searchable(true)    // show search input at top (default: false)
    .selectable(true);   // default: true

// Methods
state.set_searchable(true, cx);
state.set_selectable(false, cx);
state.set_selected_index(Some(ix), window, cx);     // no scroll
state.set_right_clicked_index(Some(ix), window, cx);
state.selected_index() -> Option<IndexPath>
state.delegate() -> &D
state.delegate_mut() -> &mut D
state.focus(window, cx);
state.scroll_to_selected_item(window, cx);
```

`IndexPath` supports multi-section indices (section + item within section).

#### `ListDelegate` Trait

```rust
pub trait ListDelegate: Sized + 'static {
    type Item: Selectable + IntoElement;

    // --- Required ---
    fn items_count(&self, section: usize, cx: &App) -> usize;
    fn render_item(
        &mut self, ix: IndexPath,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item>;
    fn set_selected_index(
        &mut self, ix: Option<IndexPath>,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    );

    // --- Optional ---
    // Search
    fn perform_search(
        &mut self, query: &str,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> Task<()> { Task::ready(()) }

    // Sections
    fn sections_count(&self, cx: &App) -> usize { 1 }
    fn render_section_header(
        &mut self, section: usize,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> { None::<AnyElement> }
    fn render_section_footer(
        &mut self, section: usize,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> { None::<AnyElement> }

    // Initial / empty / loading states
    fn render_initial(
        &mut self, window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> Option<AnyElement> { None }
    fn render_empty(
        &mut self, window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement;
    fn loading(&self, cx: &App) -> bool { false }
    fn render_loading(
        &mut self, window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement { Loading }

    // Selection events
    fn set_right_clicked_index(
        &mut self, ix: Option<IndexPath>,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) {}
    fn confirm(
        &mut self, secondary: bool,
        window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) {}
    fn cancel(
        &mut self, window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) {}

    // Infinite scroll
    fn has_more(&self, cx: &App) -> bool { false }
    fn load_more_threshold(&self) -> usize { 20 }
    fn load_more(
        &mut self, window: &mut Window, cx: &mut Context<ListState<Self>>,
    ) {}
}
```

**IMPORTANT:** All items MUST have the same height. All section headers MUST have the same height. All section footers MUST have the same height.

#### `ListEvent`

```rust
pub enum ListEvent {
    Select(IndexPath),     // move selection (arrow keys)
    Confirm(IndexPath),    // click or Enter
    Cancel,                // ESC to deselect
}
```

#### Rendering a List

```rust
// Create state
let list_state = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));

// Subscribe to events
cx.subscribe(&list_state, |this, list, event, cx| {
    match event {
        ListEvent::Confirm(ix) => { /* item selected */ }
        ListEvent::Cancel => { /* dismissed */ }
        _ => {}
    }
});

// Render
List::new(&list_state).small()
```

#### `ListItem`

The standard list item element (implements `Selectable`):

```rust
ListItem::new(id)
    .selected(true)           // highlight as selected
    .confirmed(true)          // show check icon
    .disabled(true)           // grey out
    .separator()              // non-selectable separator style
    .check_icon(Icon::new(IconName::Check))
    .suffix(|window, cx| div().child("→"))
    .on_click(|event, window, cx| { /* ... */ })
    .on_mouse_down(MouseButton::Left, |event, window, cx| { /* ... */ })
    .on_mouse_enter(|event, window, cx| { /* ... */ })
    .child(label)
```

#### `ListSeparatorItem`

A non-selectable separator line:

```rust
ListSeparatorItem::new()
    .child(div())
```

#### Searchable Lists

Set `searchable(true)` on `ListState` and implement `perform_search` on your delegate. The list renders a search input at the top. When the query changes, `perform_search` is called (returns a `Task<()>` for async search).

---

## Tree

A hierarchical tree view backed by `UniformList` for virtualized rendering with keyboard navigation.

```rust
use gpui_component::tree::{Tree, TreeState, TreeItem, TreeEntry, tree};
```

#### `TreeItem`

```rust
pub struct TreeItem {
    pub id: SharedString,     // unique identifier (e.g. file path)
    pub label: SharedString,  // display label (e.g. file name)
    pub children: Vec<TreeItem>,
}

TreeItem::new("src/lib.rs", "lib.rs")
    .child(TreeItem::new("src/ui/button.rs", "button.rs"))
    .child(TreeItem::new("src/ui/input.rs", "input.rs"))
    .children(vec![/* more items */])
    .expanded(true)            // start expanded
    .disabled(false)

// Query
item.is_folder()    // true if has children
item.is_expanded()
item.is_disabled()
```

#### `TreeEntry`

A flat representation of a tree item at render time, with depth.

```rust
pub struct TreeEntry {
    // fields: item: TreeItem, depth: usize
}

entry.item()       // &TreeItem
entry.depth()      // usize — nesting level
entry.is_folder()  // has children
entry.is_expanded()
entry.is_disabled()
```

#### `TreeState`

```rust
let state = cx.new(|_| {
    TreeState::new()
        .items(vec![
            TreeItem::new("src", "src")
                .expanded(true)
                .child(TreeItem::new("src/lib.rs", "lib.rs")),
            TreeItem::new("Cargo.toml", "Cargo.toml"),
        ])
});

// Methods
state.set_items(items, cx);                           // replace items at runtime
state.set_selected_index(Some(ix), cx);               // select by index
state.set_selected_item(Some(&item), cx);             // select by item reference (auto-expands ancestors)
state.selected_index() -> Option<usize>
state.selected_item() -> Option<&TreeItem>
state.selected_entry() -> Option<&TreeEntry>
state.scroll_to_item(ix, ScrollStrategy::Top);        // programmatic scroll
state.focus(window, cx);
```

Keyboard: `Up`/`Down` to navigate, `Enter`/`Right` to expand, `Left` to collapse, wrapping at boundaries.

#### Rendering

```rust
// Using the convenience function
tree(&state, |ix, entry, selected, window, cx| {
    let item = entry.item();
    let indent = px(16.) * entry.depth() as f32;
    let chevron = if entry.is_folder() {
        if entry.is_expanded() { IconName::ChevronDown } else { IconName::ChevronRight }
    } else {
        IconName::File
    };
    ListItem::new(ix)
        .pl(indent)
        .selected(selected)
        .child(
            h_flex()
                .gap_1()
                .child(Icon::new(chevron).size_4())
                .child(item.label.clone())
        )
})

// Or using the element directly with optional context menu:
Tree::new(&state, |ix, entry, selected, window, cx| {
    // same render closure
})
.context_menu(|ix, entry, menu, window, cx| {
    menu.item("Delete", |window, cx| { /* ... */ })
})
```

---

## VirtualList

A virtualized scrolling list for large numbers of **differently sized** items. Unlike GPUI's built-in `uniform_list`, items can have different heights (vertical) or widths (horizontal).

```rust
use gpui_component::virtual_list::{
    v_virtual_list, h_virtual_list,
    VirtualListScrollHandle, VirtualListFrameState, ItemSizeLayout,
};
```

### `v_virtual_list` / `h_virtual_list`

```rust
// Vertical
v_virtual_list(
    view,           // Entity<V> where V: Render
    "list-id",      // ElementId
    item_sizes,     // Rc<Vec<Size<Pixels>>> — each item's size; only height used
    |view, range, window, cx| -> Vec<impl IntoElement> {
        // range: Range<usize> — visible item indices
        range.map(|ix| {
            div().h(item_sizes[ix].height).child(format!("Item {}", ix))
        }).collect()
    },
)

// Horizontal (only width used from item_sizes)
h_virtual_list(
    view, "list-id", item_sizes,
    |view, range, window, cx| -> Vec<impl IntoElement> { /* ... */ },
)
```

**Performance characteristics:**
- Only visible items are rendered (plus a small overscan buffer).
- Each item can have a different size; layout uses the provided `item_sizes` to compute scroll offsets.
- The first item's cross-axis dimension is measured to determine the non-scrolling dimension.
- Suitable for heterogeneous lists (e.g., chat messages, variable-height table rows).

### `VirtualListScrollHandle`

Programmatic scroll control:

```rust
pub struct VirtualListScrollHandle { /* ... */ }

let handle = VirtualListScrollHandle::new();

handle.scroll_to_item(ix, ScrollStrategy::Top);
handle.scroll_to_item(ix, ScrollStrategy::Bottom);
handle.scroll_to_bottom();
handle.base_handle() -> &ScrollHandle  // underlying GPUI ScrollHandle
```

---

## DescriptionList

Key-value term/description pairs displayed in a horizontal or vertical layout.

```rust
use gpui_component::description_list::{DescriptionList, DescriptionItem, DescriptionText};
```

### Layout Modes

- **Horizontal** (default): label on the left, value on the right, organized in columns.
- **Vertical**: label above value, stacked.

```rust
DescriptionList::new()        // horizontal (default)
DescriptionList::vertical()   // vertical
DescriptionList::horizontal() // explicit horizontal
```

### Methods

```rust
DescriptionList::new()
    .label_width(px(150.))   // label column width (horizontal only; default 120px)
    .layout(Axis::Vertical)
    .bordered(true)           // show borders between cells (horizontal only; default true)
    .columns(4)               // 1-10 columns (default 3)
    .small()                  // Sizable support
    .item("Name", "John Doe", 1)              // label, value, span
    .child(
        DescriptionItem::new("Email")
            .value("john@example.com")
            .span(2)
    )
    .separator()              // insert a visual separator row
    .children(vec![
        DescriptionItem::new("Role").value("Admin"),
    ])
```

### `DescriptionItem`

```rust
pub enum DescriptionItem {
    Item { label: DescriptionText, value: DescriptionText, span: usize },
    Separator,
}

DescriptionItem::new("Label")   // empty value, span=1
    .value("Value")             // set value
    .span(2)                    // set column span
```

### `DescriptionText`

Accepts strings, `Text` elements, or arbitrary `AnyElement`:

```rust
pub enum DescriptionText {
    String(SharedString),
    Text(Text),              // crate::text::Text
    AnyElement(AnyElement),
}

// Auto-convertible via From:
DescriptionText::from("plain text")
DescriptionText::from(SharedString::from("shared"))
DescriptionText::from(String::from("owned"))
DescriptionText::from(Text::new("styled text").text_sm())
DescriptionText::from(div().child("element").into_any_element())
```

---

## Settings

A structured settings UI with a sidebar for page navigation, grouped sections, and typed field types.

```rust
use gpui_component::setting::{
    Settings, SettingPage, SettingGroup, SettingItem,
    SettingField, SettingFieldType,
};
```

### Hierarchy

```
Settings
  SettingPage           <- single active page in the sidebar
    SettingGroup        <- titled/untitled group within a page
      SettingItem       <- individual setting row
        Label
        SettingField     <- Switch, Checkbox, NumberInput, Input, Dropdown, Element
```

### `Settings`

```rust
Settings::new("app-settings")
    .page(SettingPage::new("General")     // pages appear in sidebar
        .icon(IconName::Settings)
        .group(
            SettingGroup::new()
                .title("Appearance")
                .description("Customize how the app looks")
                .item(SettingItem::new("Dark Mode", dark_mode_field))
                .item(SettingItem::new("Font Size", font_size_field)
                    .description("Base font size in pixels"))
                .items(vec![/* more items */])
        )
    )
    .pages(vec![/* more pages */])
    .sidebar_width(px(280.))              // default: 250px
    .sidebar_style(&style_refinement)
    .header_style(&style_refinement)      // style for sidebar header area
    .default_selected_index(SelectIndex { page_ix: 0, group_ix: Some(0) })
    .with_group_variant(GroupBoxVariant::Card)
    .small()
```

### `SettingPage`

```rust
SettingPage::new("General")
    .title("General Settings")
    .icon(IconName::Settings)
    .description("General application settings")
    .default_open(true)       // expanded in sidebar
    .resettable(true)         // show reset button when items are modified
    .group(group)
    .groups(vec![group1, group2])
    .header_style(&style_refinement)
```

### `SettingGroup`

```rust
SettingGroup::new()
    .title("Appearance")             // optional; None = no title shown
    .description("Visual settings")
    .item(setting_item)
    .items(vec![item1, item2])
```

### `SettingItem`

Two kinds: typed field items and custom element items.

#### Typed field item:

```rust
SettingItem::new(
    "Dark Mode",         // title
    dark_mode_field,     // SettingField
)
    .description("Toggle dark color scheme")
    .layout(Axis::Vertical)   // label above field (default: Horizontal)
    .disabled(false)
    .keywords(vec!["theme", "color"])  // extra search terms
```

#### Custom element item:

```rust
SettingItem::render(|options, window, cx| {
    // options: RenderOptions { page_ix, group_ix, item_ix, size, group_variant, layout, disabled }
    div().child("Custom UI here")
})
    .on_reset(
        |cx| true,                    // is_dirty closure
        |window, cx| { /* reset */ }  // reset closure
    )
    .keywords(vec!["custom"])
```

### `SettingFieldType`

Determines how the field value is rendered:

```rust
pub enum SettingFieldType {
    Switch,                                    // toggle
    Checkbox,                                  // checkbox
    NumberInput { options: NumberFieldOptions }, // numeric input with min/max/step
    Input,                                     // text input
    Dropdown {                                 // select dropdown
        options: Vec<(SharedString, SharedString)>,  // (label, value) pairs
        scrollable: bool,
    },
    Element {                                   // custom element
        element: Rc<dyn SettingFieldElement<Element = AnyElement>>,
    },
}
```

### `SettingField<T>`

Typed value holder with reactive change notifications:

```rust
// Create fields through App context or builder methods
let field: SettingField<bool> = SettingField::new("dark_mode")
    .default_value(true)                   // initial value, enables reset
    .field_type(SettingFieldType::Switch)
    .on_change(|new_val, window, cx| {     // called on every change
        // persist or react
    });

let string_field: SettingField<SharedString> = SettingField::new("username")
    .default_value(SharedString::from(""))
    .field_type(SettingFieldType::Input)
    .placeholder("Enter username")
    .on_change(|val, window, cx| { /* ... */ });

let number_field: SettingField<f64> = SettingField::new("volume")
    .default_value(75.0)
    .field_type(SettingFieldType::NumberInput {
        options: NumberFieldOptions::new().min(0.0).max(100.0).step(5.0),
    })
    .on_change(|val, window, cx| { /* ... */ });

// Dropdown
let dropdown_field: SettingField<SharedString> = SettingField::new("language")
    .default_value(SharedString::from("en"))
    .field_type(SettingFieldType::Dropdown {
        options: vec![
            ("English".into(), "en".into()),
            ("中文".into(), "zh".into()),
        ],
        scrollable: true,
    });
```

---

## Charts

Chart components implement the `Plot` trait and use `#[derive(IntoPlot)]` to become GPUI elements. This macro generates `IntoElement` and `Element` impls that request a full-size layout and delegate painting to `Plot::paint`.

```rust
use gpui_component::chart::{BarChart, LineChart, AreaChart, PieChart, CandlestickChart};
use gpui_component::plot::{
    PlotAxis, AxisText, AxisLabelSide, Grid, StrokeStyle,
    Tooltip, TooltipState, TooltipPosition, CrossLine, Dot,
};
```

### `#[derive(IntoPlot)]`

Derivable on any struct that implements `Plot`. Generates:
- `IntoElement` — returns `self`
- `Element` — requests full-size layout, delegates `paint` to `Plot::paint(bounds, window, cx)`

This means chart structs can be used directly as elements:

```rust
BarChart::new(data).band(|d| &d.category).value(|d| d.count)
    // used like any GPUI element:
    // div().child(chart)
```

### Chart Color Theme

Colors are accessed via `cx.theme()`:

```
cx.theme().chart_1         // lightest blue (#93c5fd)
cx.theme().chart_2         // default medium blue (#3b82f6)
cx.theme().chart_3         // darker blue (#2563eb)
cx.theme().chart_4         // (#1d4ed8)
cx.theme().chart_5         // darkest blue (#1e40af)
cx.theme().chart_bullish   // green (upward movement)
cx.theme().chart_bearish   // red (downward movement)
```

Fallbacks: `chart_1` through `chart_5` fall back to blue shades; `chart_bullish` → green; `chart_bearish` → red.

`chart_2` is the default stroke/fill for most charts.

### `BarChart<T, B, V>`

Categorical bar chart with configurable alignment and fill.

```rust
use gpui_component::plot::shape::{Bar, BarAlignment};

pub struct BarChart<T, B, V>
where B: PartialEq + Into<SharedString>, V: Copy + PartialOrd + Num + ToPrimitive + Sealed
```

**Builder:**

```rust
BarChart::new(data)
    .band(|d| &d.category)        // map datum to band-axis (categorical)
    .value(|d| d.count)            // map datum to value-axis (numeric)
    .fill(|d, bar_bounds, chart_bounds, alignment| {
        // per-datum fill closure; returns Background
        cx.theme().chart_2.into()
    })
    .fill_gradient(|d, chart_range, chart_to_bar| {
        // per-datum gradient fill; returns [LinearColorStop; 2]
        [
            linear_color_stop(cx.theme().chart_2.opacity(0.3), 0.0),
            linear_color_stop(cx.theme().chart_2, 1.0),
        ]
    })
    .tick_margin(2)                // label interval (default: 1, every item)
    .label(|d| d.label.clone())    // text labels on bars
    .label_axis(true)              // show band axis labels (default: true)
    .grid(true)                    // show grid lines (default: true)
    .alignment(BarAlignment::Bottom) // Bottom (default), Top, Left, Right
    .corner_radii(Corners::all(px(4.0)))  // rounded bar corners
```

### `LineChart<T, X, Y>`

Point-based line chart with optional dots.

```rust
LineChart::new(data)
    .x(|d| d.timestamp)            // map to point axis (categorical-like)
    .y(|d| d.value)                // map to numeric axis
    .stroke(cx.theme().chart_2)    // line color (default: chart_2)
    .natural()                     // smooth curved line (default stroke style)
    .linear()                      // straight line segments
    .step_after()                  // step line (horizontal then vertical)
    .dot(true)                     // show dots at data points
    .tick_margin(4)                // label interval (default: 1)
    .x_axis(true)                  // show x-axis labels (default: true)
    .grid(true)                    // show grid (default: false)
```

### `AreaChart<T, X, Y>`

Multi-series area chart. `y()` can be called multiple times to add series.

```rust
AreaChart::new(data)
    .x(|d| d.timestamp)
    .y(|d| d.series_a)              // first series
    .y(|d| d.series_b)              // second series (call again)
    .stroke(cx.theme().chart_2)     // stroke for most recent series
    .fill(cx.theme().chart_2.opacity(0.4))  // fill for most recent series
    .natural()                       // stroke style
    .linear()
    .step_after()
    .tick_margin(4)
    .x_axis(true)
    .grid(true)
```

Each call to `.y()`, `.stroke()`, `.fill()` appends to internal vectors; the i-th series uses `strokes[i]`, `fills[i]`, and `stroke_styles[i]`.

### `PieChart<T>`

```rust
PieChart::new(data)
    .value(|d| d.count)                    // map to numeric value
    .color(|d| d.color)                    // per-slice color (default: chart_2)
    .inner_radius(30.0)                    // donut hole radius; 0 = full pie
    .outer_radius(100.0)                   // outer radius (default: height * 0.4)
    .inner_radius_fn(|arc| arc.value)      // dynamic inner radius per slice
    .outer_radius_fn(|arc| arc.value)      // dynamic outer radius per slice
    .pad_angle(0.02)                       // gap between slices
```

`ArcData<T>` (passed to radius closures):
- `data: &T` — the original datum
- `value: f32` — the computed value
- `start_angle: f32`, `end_angle: f32` — in radians

### `CandlestickChart<T, X, Y>`

Financial OHLC candlestick chart.

```rust
CandlestickChart::new(data)
    .x(|d| d.date)
    .open(|d| d.open)
    .high(|d| d.high)
    .low(|d| d.low)
    .close(|d| d.close)
    .tick_margin(4)
    .body_width_ratio(0.6)         // body width as fraction of band (default: 0.6)
    .x_axis(true)
    .grid(true)
```

Bullish (close >= open) uses `cx.theme().chart_bullish`; bearish uses `cx.theme().chart_bearish`.

---

## Plot Toolkit

Building blocks for creating custom charts. Used internally by the chart implementations; also available for custom plot types.

### `PlotAxis`

Draws axis lines and labels with configurable sides.

```rust
let mut axis = PlotAxis::new()
    .stroke(cx.theme().border)
    .x(height_in_px)              // x-axis line position
    .x_axis(true)                 // show x-axis line (default: true)
    .x_label(labels)              // Vec<AxisText> for x-axis
    .x_label_side(AxisLabelSide::End)   // labels below (End) or above (Start)
    .y(left_margin_px)            // y-axis line position
    .y_axis(true)
    .y_label(labels)
    .y_label_side(AxisLabelSide::Start); // labels left (Start) or right (End)

axis.paint(&bounds, window, cx);
```

### `AxisText`

```rust
AxisText::new("Jan", px(50.), color)
    .font_size(px(12.))
    .align(TextAlign::Center)
```

### `Grid`

```rust
Grid::new()
    .x(vec![px(100.), px(200.)])         // vertical grid lines at these x positions
    .y(vec![px(50.), px(100.)])          // horizontal grid lines at these y positions
    .stroke(cx.theme().border)
    .dash_array(&[px(4.), px(2.)])       // dashed lines
    .paint(&bounds, window);
```

### `Scale` Traits (plot::scale)

```rust
pub trait Scale<T> {
    fn tick(&self, value: &T) -> Option<f32>;       // map value to pixel position
    fn least_index(&self, tick: f32) -> usize;
    fn least_index_with_domain(&self, tick: f32, domain: &[T]) -> (usize, f32);
}
```

Implementations:
- `ScalePoint<X>` — evenly spaced point positions (for line/area charts).
- `ScaleBand<X>` — banded positions with padding (for bar/candlestick charts).
- `ScaleLinear<V>` — continuous numeric scale.
- `ScaleOrdinal<X>` — sequential ordinal positions.

```rust
let x_scale = ScalePoint::new(domain_values, vec![0.0, chart_width]);
let y_scale = ScaleLinear::new(all_values, vec![chart_height, 10.0]);
let band_scale = ScaleBand::new(categories, vec![0.0, chart_width])
    .padding_inner(0.4)
    .padding_outer(0.2);
```

### `Tooltip`

Hover tooltip with cross-line, dots, and content.

```rust
Tooltip::new()
    .position(TooltipPosition::Left)           // or Right (default)
    .gap(px(8.))                               // offset from edge
    .cross_line(
        CrossLine::new(point(px(x), px(y)))
            .horizontal()                      // also show horizontal line
            .both()                            // show both vertical + horizontal
            .height(chart_height)
    )
    .dots(vec![
        Dot::new(point(px(x1), px(y1)))
            .size(px(8.))
            .stroke(cx.theme().chart_2)
            .fill(cx.theme().chart_2),
    ])
    .appearance(true)                          // show border/background (default)
    .child(div().child("Value: 42"))
```

### `TooltipState`

Snapshot for coordinating tooltip position:

```rust
pub struct TooltipState {
    pub index: usize,
    pub cross_line: Point<Pixels>,
    pub dots: Vec<Point<Pixels>>,
    pub position: TooltipPosition,
}

TooltipState::new(index, cross_line_point, dot_points, TooltipPosition::Left)
```

### `StrokeStyle`

```rust
pub enum StrokeStyle {
    Natural,     // smooth curved line (default)
    Linear,      // straight segments
    StepAfter,   // step (horizontal then vertical)
}
```

### Custom Plot Example

```rust
use gpui_component::plot::{Plot, PlotAxis, Grid, AxisText};
use gpui_component_macros::IntoPlot;

#[derive(IntoPlot)]
struct MyChart {
    data: Vec<f32>,
}

impl Plot for MyChart {
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        // draw axes, grid, shapes using plot toolkit
        let axis = PlotAxis::new().stroke(cx.theme().border);
        axis.paint(&bounds, window, cx);

        Grid::new()
            .stroke(cx.theme().border)
            .dash_array(&[px(4.), px(2.)])
            .paint(&bounds, window);

        // draw custom shapes via shape module...
    }
}

// Use it:
div().size_full().child(MyChart { data: vec![1.0, 2.0, 3.0] })
```
