# Navigation & Layout Components

> **Module path:** `crate::tab`, `crate::sidebar`, `crate::dock`, `crate::resizable`, `crate::scroll`

**Contents:** [Tabs](#tabs) · [Sidebar](#sidebar) · [TitleBar](#titlebar) · [Breadcrumb & Pagination](#breadcrumb--pagination) · [Accordion & Collapsible](#accordion--collapsible) · [GroupBox](#groupbox) · [Resizable Panels](#resizable-panels) · [Scroll Components](#scroll-components) · [FocusTrap](#focustrap) · [Separator & StatusBar](#separator--statusbar) · [Dock System](#dock-system) · [Usage Patterns](#usage-patterns--best-practices)

---

## Tabs

`crate::tab` provides `Tab` and `TabBar` for tabbed navigation.

### `Tab`

```rust
use gpui_component::tab::Tab;

// Create a labeled tab
Tab::new("tab-id")
    .label("Home")
    .icon(IconName::Home)
    .on_click(|event, window, cx| { /* selected */ });

// Variant shortcuts
Tab::new("tab-id").pill().label("Pill");
Tab::new("tab-id").outline().label("Outline");
Tab::new("tab-id").segmented().label("Segmented");
Tab::new("tab-id").underline().label("Underline");
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> Tab` | Create a tab; generated `ElementId`. Also available via `From<&str>`, `From<String>`, `From<SharedString>`, `From<Icon>`, `From<IconName>`. |
| `label(label)` | `impl Into<SharedString> -> Tab` | Set tab text. |
| `icon(icon)` | `impl Into<Icon> -> Tab` | Set tab icon. |
| `with_variant(variant)` | `TabVariant -> Tab` | Set variant explicitly. |
| `pill()` / `outline()` / `segmented()` / `underline()` | `-> Tab` | Convenience variant setters. |
| `prefix(element)` | `impl IntoElement -> Tab` | Left-side element. |
| `suffix(element)` | `impl IntoElement -> Tab` | Right-side element. |
| `disabled(disabled)` | `bool -> Tab` | Disable the tab. |
| `on_click(f)` | `Fn(&ClickEvent, &mut Window, &mut App) + 'static -> Tab` | Click handler. |
| `child(element)` | (via `ParentElement`) | Nest content inside the tab. |

**Traits:** `Selectable`, `Styled`, `Sizable`, `InteractiveElement`, `RenderOnce`.

### `TabBar`

The container for multiple `Tab` items that manages selection and animated indicator sliding.

```rust
use gpui_component::tab::TabBar;

TabBar::new("tabbar-id")
    .child(Tab::new("tab1").label("Editor"))
    .child(Tab::new("tab2").label("Terminal"))
    .selected_index(0)
    .on_click(|index: &usize, window, cx| {
        // index = the clicked tab's position
    });
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> TabBar` | Create tab bar. |
| `child(child)` | `impl Into<Tab> -> TabBar` | Append a tab. |
| `children(tabs)` | `impl IntoIterator<Item = impl Into<Tab>> -> TabBar` | Append multiple tabs. |
| `selected_index(index)` | `usize -> TabBar` | Set the active tab. |
| `with_variant(variant)` | `TabVariant -> TabBar` | Set variant for all children (they inherit it). |
| `pill()` / `outline()` / `segmented()` / `underline()` | `-> TabBar` | Variant shortcut for all tabs. |
| `menu(menu)` | `bool -> TabBar` | Show overflow menu when tabs exceed width (default `false`). |
| `track_scroll(handle)` | `&ScrollHandle -> TabBar` | Track scroll position. |
| `prefix(element)` | `impl IntoElement -> TabBar` | Element before the tabs. |
| `suffix(element)` | `impl IntoElement -> TabBar` | Element after the tabs. |
| `last_empty_space(element)` | `impl IntoElement -> TabBar` | Trailing spacer. |
| `on_click(f)` | `Fn(&usize, &mut Window, &mut App) + 'static -> TabBar` | Click callback with the clicked tab index. When set, children's `on_click` is ignored. |

**Traits:** `Styled`, `Sizable`, `RenderOnce`.

### `TabVariant`

```rust
pub enum TabVariant {
    Tab,        // default — simple tab
    Outline,    // outlined button style
    Pill,       // rounded pill background
    Segmented,  // segmented control with background indicator
    Underline,  // underline indicator
}
```

---

## Sidebar

`crate::sidebar` provides a collapsible app sidebar with groups, menus, headers, and footers.

### Complete app sidebar structure

```rust
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarGroup};
use gpui_component::sidebar::{SidebarHeader, SidebarFooter};
use gpui_component::sidebar::SidebarMenuItem;
use gpui_component::sidebar::SidebarToggleButton;
use gpui_component::Sizable;

Sidebar::new("app-sidebar")
    .side(Side::Left)           // or Side::Right
    .collapsible(true)          // SidebarCollapsible::Icon, Offcanvas, None
    .collapsed(false)           // initial collapse state
    .header(
        SidebarHeader::new()
            .child(Icon::new(IconName::Home))
            .child("Project Name")
    )
    .child(
        SidebarGroup::new("Navigation").child(
            SidebarMenu::new()
                .child(
                    SidebarMenuItem::new("Dashboard")
                        .icon(IconName::LayoutDashboard)
                        .active(true)
                        .on_click(|ev, w, cx| { /* navigate */ })
                )
                .child(
                    SidebarMenuItem::new("Settings")
                        .icon(IconName::Settings)
                        .children([
                            SidebarMenuItem::new("General"),
                            SidebarMenuItem::new("Appearance"),
                        ])
                        .default_open(true)
                        .click_to_open(true)
                )
        )
    )
    .child(
        SidebarGroup::new("Tools").child(
            SidebarMenu::new()
                .child(SidebarMenuItem::new("Terminal").icon(IconName::Terminal))
                .child(SidebarMenuItem::new("Search").icon(IconName::Search))
        )
    )
    .footer(
        SidebarFooter::new()
            .child("v1.0.0")
    );
```

### `Sidebar<E: SidebarItem>`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> Sidebar<E>` | Create sidebar. Generic `E` must implement `SidebarItem`. |
| `side(side)` | `Side -> Sidebar<E>` | Left or Right placement (default `Side::Left`). |
| `collapsible(collapsible)` | `impl Into<SidebarCollapsible> -> Sidebar<E>` | Collapse behavior. |
| `collapsed(collapsed)` | `bool -> Sidebar<E>` | Initial collapsed state (default `false`). |
| `header(element)` | `impl IntoElement -> Sidebar<E>` | Header element. |
| `footer(element)` | `impl IntoElement -> Sidebar<E>` | Footer element. |
| `child(child)` | `E -> Sidebar<E>` | Add a collapsible child. |
| `children(children)` | `impl IntoIterator<Item = E> -> Sidebar<E>` | Add multiple children. |

**Traits:** `Styled`, `RenderOnce`.

### `SidebarCollapsible`

```rust
pub enum SidebarCollapsible {
    Icon,      // collapse to icon width (default)
    Offcanvas, // slide out, releases layout space
    None,      // always expanded, ignores collapsed
}
```

Convert from `bool`: `true -> Icon`, `false -> None`.

### `SidebarMenu`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> SidebarMenu` | Create menu. |
| `child(child)` | `impl Into<SidebarMenuItem> -> SidebarMenu` | Append item. |
| `children(children)` | `impl IntoIterator -> SidebarMenu` | Set items. |

**Traits:** `Collapsible`, `SidebarItem`, `Styled`.

### `SidebarMenuItem`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(label)` | `impl Into<SharedString> -> SidebarMenuItem` | Create item. |
| `icon(icon)` | `impl Into<Icon> -> SidebarMenuItem` | Set icon. |
| `active(active)` | `bool -> SidebarMenuItem` | Highlight active state. |
| `on_click(f)` | `Fn(&ClickEvent, &mut Window, &mut App) + 'static -> Self` | Click handler. |
| `collapsed(collapsed)` | `bool -> SidebarMenuItem` | Collapse state. |
| `default_open(open)` | `bool -> SidebarMenuItem` | Submenu initially open (initial render only). |
| `click_to_open(click)` | `bool -> SidebarMenuItem` | Click opens submenu (default `false`). |
| `click_to_toggle(toggle)` | `bool -> SidebarMenuItem` | Click toggles submenu. |
| `children(children)` | `impl IntoIterator -> SidebarMenuItem` | Submenu items. |
| `suffix(builder)` | `Fn(&mut Window, &mut App) -> impl IntoElement + 'static -> Self` | Suffix element builder. |
| `disable(disable)` | `bool -> SidebarMenuItem` | Disable item. |
| `context_menu(f)` | `Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static -> Self` | Right-click context menu. |

**Traits:** `Collapsible`, `SidebarItem`, `FluentBuilder`.

### `SidebarGroup<E: SidebarItem>`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(label)` | `impl Into<SharedString> -> SidebarGroup<E>` | Create group with heading label. |
| `child(child)` | `E -> SidebarGroup<E>` | Add child. |
| `children(children)` | `impl IntoIterator<Item = E> -> SidebarGroup<E>` | Add multiple children. |

**Traits:** `Collapsible`, `SidebarItem`.

### `SidebarHeader`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> SidebarHeader` | Create header. |
| `selected(selected)` | `bool -> SidebarHeader` | Toggle selected style. |

**Traits:** `Collapsible`, `Selectable`, `ParentElement`, `Styled`, `InteractiveElement`, `DropdownMenu`, `RenderOnce`.

### `SidebarFooter`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> SidebarFooter` | Create footer. |

**Traits:** `Collapsible`, `Selectable`, `ParentElement`, `Styled`, `InteractiveElement`, `DropdownMenu`, `RenderOnce`.

### `SidebarToggleButton`

Button to toggle sidebar collapse/expand state.

```rust
SidebarToggleButton::new("toggle-btn")
    .collapsed( &sidebar_collapsed_entity)
    .on_click(|collapsed: &bool, window, cx| { / toggle / });
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> SidebarToggleButton` | Create toggle. |
| `collapsed(state)` | `&Entity<bool> -> SidebarToggleButton` | Bind collapsed state entity. |
| `on_click(f)` | `Fn(&bool, &mut Window, &mut App) + 'static -> Self` | Toggle callback. |

---

## TitleBar

`crate::title_bar` — Custom window title bar component.

```rust
use gpui_component::title_bar::TitleBar;

TitleBar::new()
    .child("My App")
    .on_close_window(|event, window, cx| {
        // Custom close behavior (Linux only)
    });
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> TitleBar` | Create title bar. |
| `title_bar_options()` | `-> TitlebarOptions` | Default platform options: `appears_transparent: true`, traffic light position `(9, 9)`. |
| `on_close_window(f)` | `Fn(&ClickEvent, &mut Window, &mut App) + 'static -> Self` | Custom close handler (Linux only). |
| `child(element)` | (via `ParentElement`) | Add content. |

**Constants:** `TITLE_BAR_HEIGHT = px(34.)`, platform-dependent left padding.

**Traits:** `Styled`, `ParentElement`, `RenderOnce`.

---

## Breadcrumb & Pagination

### `Breadcrumb`

```rust
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};

Breadcrumb::new()
    .child(BreadcrumbItem::new("Home").on_click(|ev, w, cx| {}))
    .child(BreadcrumbItem::new("Settings"))
    .child(BreadcrumbItem::new("Profile").disabled(true));
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> Breadcrumb` | Create breadcrumb. |
| `child(item)` | `impl Into<BreadcrumbItem> -> Breadcrumb` | Append item. |
| `children(items)` | `impl IntoIterator -> Breadcrumb` | Append multiple items. |

#### `BreadcrumbItem`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(label)` | `impl Into<SharedString> -> BreadcrumbItem` | Create item. |
| `disabled(disabled)` | `bool -> BreadcrumbItem` | Disable item. |
| `on_click(f)` | `Fn(&ClickEvent, &mut Window, &mut App) + 'static -> Self` | Click handler. |

**Traits (BreadcrumbItem):** `Styled`, `RenderOnce`.

### `Pagination`

```rust
use gpui_component::pagination::Pagination;

Pagination::new("pages")
    .current_page(3)
    .total_pages(10)
    .visible_pages(5)
    .on_click(|page: &usize, window, cx| {
        // Navigate to `page` (1-based)
    });

// Compact mode (prev/next icons only)
Pagination::new("compact-pages")
    .current_page(1)
    .total_pages(5)
    .compact();
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> Pagination` | Create pagination. |
| `current_page(page)` | `usize -> Pagination` | Set current page (1-based). |
| `total_pages(pages)` | `usize -> Pagination` | Set total pages. |
| `on_click(f)` | `Fn(&usize, &mut Window, &mut App) + 'static -> Self` | Page change callback. |
| `compact()` | `-> Pagination` | Show prev/next icons only. |
| `visible_pages(max)` | `usize -> Pagination` | Max visible page buttons (default 5). |

**Traits:** `Disableable`, `Sizable`, `Styled`, `RenderOnce`.

---

## Accordion & Collapsible

### `Accordion`

```rust
use gpui_component::accordion::{Accordion, AccordionItem};

Accordion::new("faq")
    .multiple(false)      // single-expand (default)
    .bordered(true)       // borders between items
    .on_toggle_click(|open_indices: &[usize], window, cx| {
        // `open_indices` = currently open item indices
    })
    .item(|a| {
        a.title("What is GPUI?")
            .child("GPUI is a fast, productive Rust UI framework.")
    })
    .item(|a| {
        a.title("How to install?")
            .open(true)   // initially open
            .child("Add to Cargo.toml...")
    });
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> Accordion` | Create accordion. |
| `multiple(multi)` | `bool -> Accordion` | Allow multiple open items (default `false`). |
| `bordered(bordered)` | `bool -> Accordion` | Show borders (default `true`). |
| `disabled(disabled)` | `bool -> Accordion` | Disable all items. |
| `item(f)` | `FnOnce(AccordionItem) -> AccordionItem -> Accordion` | Build an item. Uses `AccordionItem::new()` as seed. |
| `on_toggle_click(f)` | `Fn(&[usize], &mut Window, &mut App) + Send + Sync + 'static -> Self` | Callback with open indices. |

**Traits:** `Sizable`, `RenderOnce`.

#### `AccordionItem`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> AccordionItem` | Create item. Do not call directly — prefer `Accordion::item()`. |
| `icon(icon)` | `impl Into<Icon> -> AccordionItem` | Header icon. |
| `title(title)` | `impl IntoElement -> AccordionItem` | Header title. |
| `bordered(bordered)` | `bool -> AccordionItem` | Override border. |
| `open(open)` | `bool -> AccordionItem` | Initial open state. |
| `disabled(disabled)` | `bool -> AccordionItem` | Disable this item. |
| `child(content)` | (via `ParentElement`) | Collapsible content. |

**Traits:** `ParentElement`, `Sizable`, `RenderOnce`.

### `Collapsible`

A simple wrapper that shows/hides content.

```rust
use gpui_component::collapsible::Collapsible;

div().child(
    Collapsible::new()
        .open(true)
        .child(div().child("Always visible"))
        .content(div().child("Only visible when open"))
);
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> Collapsible` | Create collapsible. |
| `open(open)` | `bool -> Collapsible` | Show content? (default `false`). |
| `content(content)` | `impl IntoElement -> Collapsible` | Set collapsible content (hidden when closed). |
| `child(element)` | (via `ParentElement`) | Set always-visible children. |

**Traits:** `Styled`, `ParentElement`, `RenderOnce`.

---

## GroupBox

```rust
use gpui_component::group_box::{GroupBox, GroupBoxVariants};

GroupBox::new()
    .id("settings-group")
    .title("Appearance")
    .fill()  // or .outline(), .normal()
    .child(/* content */);

// Variants via trait
GroupBox::new().fill();
GroupBox::new().outline();
GroupBox::new().normal();  // default
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> GroupBox` | Create group box. |
| `id(id)` | `impl Into<ElementId> -> GroupBox` | Set element ID. |
| `title(title)` | `impl IntoElement -> GroupBox` | Title label. |
| `title_style(style)` | `StyleRefinement -> GroupBox` | Override title style. |
| `content_style(style)` | `StyleRefinement -> GroupBox` | Override content area style. |
| `child(element)` | (via `ParentElement`) | Add content. |

**Traits (GroupBox):** `ParentElement`, `Styled`, `GroupBoxVariants`, `RenderOnce`.

**`GroupBoxVariant`:** `Normal` (default), `Fill` (filled background), `Outline` (bordered).

---

## Resizable Panels

`crate::resizable` provides split-pane resizable panels.

### `ResizablePanelGroup`

```rust
use gpui_component::resizable::{h_resizable, v_resizable, resizable_panel};

// Horizontal (side-by-side) split
h_resizable("layout-h")
    .child(resizable_panel().size(px(220.)).child(sidebar_panel))
    .child(resizable_panel().child(main_content))
    .child(resizable_panel().size(px(280.)).child(details_panel))
    .on_resize(|state, window, cx| {
        // Persist sizes
        let sizes = state.read(cx).sizes().clone();
    });

// Vertical (top-bottom) split
v_resizable("layout-v")
    .child(resizable_panel().child(top_panel))
    .child(resizable_panel().child(bottom_panel));
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id)` | `impl Into<ElementId> -> ResizablePanelGroup` | Create group. |
| `axis(axis)` | `Axis -> ResizablePanelGroup` | Horizontal or Vertical. |
| `with_state(state)` | `&Entity<ResizableState> -> ResizablePanelGroup` | Bind external state entity. |
| `child(panel)` | `impl Into<ResizablePanel> -> ResizablePanelGroup` | Add a panel. |
| `children(panels)` | `impl IntoIterator<Item = impl Into<ResizablePanel>> -> Self` | Add multiple panels. |
| `size(size)` | `Pixels -> ResizablePanelGroup` | Set perpendicular size: when axis=Horizontal this is height; when Vertical this is width. |
| `on_resize(f)` | `Fn(&Entity<ResizableState>, &mut Window, &mut App) + 'static -> Self` | Resize callback. |

**Traits:** `EventEmitter<ResizablePanelEvent>`, `RenderOnce`.

**Factory functions:** `h_resizable(id)`, `v_resizable(id)` — shortcuts that set axis.

### `ResizablePanel`

| Method | Signature | Description |
|--------|-----------|-------------|
| (constructor) | `resizable_panel() -> ResizablePanel` | Create panel via factory. |
| `visible(visible)` | `bool -> ResizablePanel` | Show/hide panel. |
| `size(size)` | `impl Into<Pixels> -> ResizablePanel` | Initial size. |
| `size_range(range)` | `impl Into<Range<Pixels>> -> ResizablePanel` | Min/max size constraint. Default: `PANEL_MIN_SIZE..Pixels::MAX` (100px..no limit). |
| `child(element)` | (via `ParentElement`) | Panel content. |

**Traits:** `Styled`, `ParentElement`, `RenderOnce`. Also: `From<T: Into<AnyElement>>`, `From<ResizablePanelGroup>`.

**Style overrides:** Panels set `flex_grow: 1` internally. Call `.flex_none()` on a sized panel to hold its width. Reserved styles (do NOT call externally): `.flex_basis()`, `.absolute()`, `.overflow_hidden()`.

### `ResizableState`

If `with_state` is not called, `ResizablePanelGroup` manages its own state internally.

| Method | Signature | Description |
|--------|-----------|-------------|
| `sizes()` | `&Vec<Pixels>` | Current panel sizes in order. |
| `resize_panel(ix, size, window, cx)` | `usize, Pixels, ... -> ()` | Programmatically resize panel at index. Clamped to `size_range`. Emits `ResizablePanelEvent::Resized`. |

---

## Scroll Components

`crate::scroll` — scrollbar, scrollable element, auto-scroll, and scrollable mask.

### `ScrollableElement` trait

Extension trait automatically implemented for `Div` and `Stateful<E>`.

```rust
use gpui_component::scroll::ScrollableElement;
// The trait is in scope automatically.

// With an external scroll handle
let handle = ScrollHandle::new();
div()
    .h_full()
    .overflow_scroll()          // enable scrolling
    .track_scroll(&handle)
    .vertical_scrollbar(&handle)  // add vertical scrollbar
    .child(content);

// Convenience: overflow + scrollbar in one
div()
    .overflow_y_scrollbar()     // vertical overflow + scrollbar
    .child(long_content);

div()
    .overflow_x_scrollbar()     // horizontal overflow + scrollbar
    .child(wide_content);

div()
    .overflow_scrollbar()       // both axes
    .child(large_content);
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `.scrollbar(scroll_handle, axis)` | `&H: ScrollbarHandle + Clone, impl Into<ScrollbarAxis> -> Self` | Add scrollbar for the given axis. |
| `.vertical_scrollbar(h)` | `&H: ScrollbarHandle + Clone -> Self` | Add vertical scrollbar. |
| `.horizontal_scrollbar(h)` | `&H: ScrollbarHandle + Clone -> Self` | Add horizontal scrollbar. |
| `.overflow_scrollbar()` | `-> Scrollable<Self>` | Enable overflow scroll on both axes with scrollbars. |
| `.overflow_x_scrollbar()` | `-> Scrollable<Self>` | Horizontal overflow with scrollbar. |
| `.overflow_y_scrollbar()` | `-> Scrollable<Self>` | Vertical overflow with scrollbar. |

### `Scrollbar`

Standalone scrollbar component bound to a scroll handle.

```rust
use gpui_component::scroll::{Scrollbar, ScrollbarShow};

Scrollbar::new(&scroll_handle)
    .axis(ScrollbarAxis::Vertical)
    .scrollbar_show(ScrollbarShow::Always);

Scrollbar::vertical(&scroll_handle);    // shortcut
Scrollbar::horizontal(&scroll_handle);  // shortcut
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(handle)` | `&H: ScrollbarHandle + Clone -> Scrollbar` | Create with both axes. |
| `vertical(handle)` | `&H -> Scrollbar` | Vertical-only shortcut. |
| `horizontal(handle)` | `&H -> Scrollbar` | Horizontal-only shortcut. |
| `id(id)` | `impl Into<ElementId> -> Scrollbar` | Override element ID. |
| `axis(axis)` | `impl Into<ScrollbarAxis> -> Scrollbar` | Set axis. |
| `scrollbar_show(show)` | `ScrollbarShow -> Scrollbar` | Visibility mode. Falls back to `cx.theme().scrollbar_show`. |
| `scroll_size(size)` | `Size<Pixels> -> Scrollbar` | Explicit content size; default reads from handle. |

### `ScrollbarAxis`

```rust
pub enum ScrollbarAxis { Vertical, Horizontal, Both }
```

### `ScrollbarShow`

```rust
pub enum ScrollbarShow {
    Always,
    Scrolling,    // hide when not scrolling
}
```

### `ScrollbarHandle` trait

```rust
pub trait ScrollbarHandle: 'static {
    fn get_offset(&self, window: &Window, cx: &App) -> Point<Pixels>;
    fn set_offset(&self, offset: Point<Pixels>, window: &mut Window, cx: &mut App);
}
```

Implemented for: `ScrollHandle`, `UniformListScrollHandle`, `ListState`.

### `AutoScroll`

Manages timer-based auto-scrolling during drag-selection.

```rust
pub struct AutoScroll {
    pub last_drag_position: Option<Point<Pixels>>,
}
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `delta()` | `-> Option<Pixels>` | Current scroll delta (positive = towards bottom). |
| `compute_delta(y, bounds)` | `Pixels, Bounds<Pixels> -> Option<Pixels>` | Compute delta for a mouse Y position within viewport. |
| `set(delta, cx, tick)` | `Option<Pixels>, &mut Context<T>, Fn(Pixels, &mut T, &mut Context<T>) -> ()` | Start/update auto-scroll task. |
| `is_active()` | `-> bool` | Currently scrolling? |
| `stop()` | `-> ()` | Stop auto-scrolling. |

### `ScrollableMask`

Low-level element: listens for mouse wheel events and moves a `ScrollHandle`. Single-axis only.

```rust
use gpui_component::scroll::ScrollableMask;

ScrollableMask::new(Axis::Vertical, &scroll_handle);
ScrollableMask::new(Axis::Vertical, &scroll_handle).debug();  // show bounds
```

---

## FocusTrap

`crate::focus_trap` — Keyboard focus containment for modals, dialogs, sheets.

```rust
use gpui_component::focus_trap::FocusTrapElement;

v_flex()
    .child(Button::new("btn1").label("Button 1"))
    .child(Input::new("field1"))
    .child(Button::new("btn3").label("Button 3"))
    .focus_trap("dialog-trap", &self.container_focus_handle);
// Tab/Shift-Tab cycles within: btn1 -> field1 -> btn3 -> btn1
// Focus cannot escape this container.
```

### `FocusTrapElement` trait

| Method | Signature | Description |
|--------|-----------|-------------|
| `.focus_trap(id, focus_handle)` | `impl Into<ElementId>, &FocusHandle -> FocusTrapContainer<Self>` | Wrap element in a focus trap. Requires `Self: ParentElement + Styled + Element + 'static`. |

Implemented for all `InteractiveElement + Sized` types.

### Architecture

1. `FocusTrapElement::focus_trap()` wraps an element in a `FocusTrapContainer`.
2. A global `FocusTrapManager` tracks all active trap containers.
3. When Tab/Shift-Tab is pressed, the system checks if focus would leave the container.
4. If so, focus cycles to the first/last focusable element within the container.

---

## Separator & StatusBar

### `Separator`

```rust
use gpui_component::separator::{Separator, SeparatorStyle};

Separator::vertical();
Separator::horizontal();
Separator::vertical_dashed();
Separator::horizontal_dashed();
Separator::horizontal().label("OR").color(gpui::red());
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `vertical()` | `-> Separator` | Vertical solid line. |
| `horizontal()` | `-> Separator` | Horizontal solid line. |
| `vertical_dashed()` | `-> Separator` | Vertical dashed line. |
| `horizontal_dashed()` | `-> Separator` | Horizontal dashed line. |
| `label(label)` | `impl Into<SharedString> -> Separator` | Text label on the line. |
| `color(color)` | `impl Into<Hsla> -> Separator` | Line color. |
| `dashed()` | `-> Separator` | Switch to dashed style. |

**Traits:** `Styled`, `RenderOnce`.

### `StatusBar`

```rust
use gpui_component::status_bar::StatusBar;

StatusBar::new()
    .left("Ln 1, Col 1")
    .left(Separator::vertical())
    .right("UTF-8")
    .right("Spaces: 2")
    .child("center content");  // center region
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `-> StatusBar` | Create empty status bar. |
| `left(element)` | `impl IntoElement -> StatusBar` | Append to left region. |
| `right(element)` | `impl IntoElement -> StatusBar` | Append to right region. |
| `child(element)` | (via `ParentElement`) | Append to center region. |

**Layout behavior:** Center region aligns based on pinned ends: centered when both left and right exist, end-aligned when only left exists, start-aligned when only right or neither.

**Traits:** `Styled`, `ParentElement`, `RenderOnce`.

---

## Dock System

`crate::dock` — Full-featured dockable panel system with drag-and-drop, split panes, tabs, tiles, zoom, and layout persistence.

### Architecture Overview

```
DockArea                     Top-level container
├── left_dock: Dock          Fixed left panel area
├── bottom_dock: Dock        Fixed bottom panel area
├── right_dock: Dock         Fixed right panel area
└── center: DockItem         Main area — tree structure
    ├── Split { axis, items }        ↔ StackPanel (resizable splits)
    │   ├── Tabs { items, active_ix } → TabPanel
    │   ├── Split { ... }            → StackPanel (nested)
    │   └── Tiles { items }          → Tiles (freeform)
    ├── Tabs { items, active_ix }   → TabPanel
    ├── Panel { view }              → single fixed panel
    └── Tiles { items }            → freeform canvas
```

### `DockArea`

The main container owner. Created as a `Entity<DockArea>` via `cx.new()`.

```rust
use gpui_component::dock::{DockArea, DockItem, DockPlacement};

let dock_area = cx.new(|cx| {
    let mut area = DockArea::new("main", Some(1), window, cx);
    area.set_center(
        DockItem::h_split(vec![
            DockItem::tabs(vec![panel_a], &dock_area_weak, window, cx),
            DockItem::tabs(vec![panel_b], &dock_area_weak, window, cx),
        ], &dock_area_weak, window, cx),
        window, cx,
    );
    area
});
```

**Key methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(id, version, window, cx)` | `impl Into<SharedString>, Option<usize>, ... -> Self` | Create dock area with version for migration. |
| `center()` | `&DockItem` | Get center layout tree. |
| `set_center(item, window, cx)` | `DockItem, ... -> ()` | Set center layout. |
| `left_dock()` / `right_dock()` / `bottom_dock()` | `Option<&Entity<Dock>>` | Access fixed docks. |
| `set_left_dock(panel, size, open, w, cx)` / `set_bottom_dock(...)` / `set_right_dock(...)` | `DockItem, Option<Pixels>, bool, ... -> ()` | Set a fixed dock. |
| `remove_left_dock(...)` / `remove_bottom_dock(...)` / `remove_right_dock(...)` | `... -> ()` | Remove fixed dock. |
| `has_dock(placement)` | `DockPlacement -> bool` | Check if dock exists at placement. |
| `is_dock_open(placement, cx)` | `DockPlacement, &App -> bool` | Check if dock is open. |
| `toggle_dock(placement, w, cx)` | `DockPlacement, ... -> ()` | Toggle dock open/close. |
| `set_dock_collapsible(edges, w, cx)` | `Edges<bool>, ... -> ()` | Set collapsible state per edge. |
| `is_dock_collapsible(placement, cx)` | `DockPlacement, &App -> bool` | Is dock collapsible? |
| `add_panel(panel, placement, bounds, w, cx)` | `Arc<dyn PanelView>, DockPlacement, Option<Bounds>, ... -> ()` | Add panel to a dock. |
| `remove_panel(panel, placement, w, cx)` | `Arc<dyn PanelView>, DockPlacement, ... -> ()` | Remove from specific dock. |
| `remove_panel_from_all_docks(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove from all docks. |
| `set_locked(locked, w, cx)` | `bool, ... -> ()` | Lock layout (prevents split/move, resize still allowed). |
| `is_locked()` | `bool` | Check lock state. |
| `panel_style(style)` | `PanelStyle -> DockArea` | Set panel style. |
| `set_version(version, w, cx)` | `usize, ... -> ()` | Update version. |
| `load(state, w, cx)` | `DockAreaState, ... -> Result<()>` | Restore from serialized state. |
| `dump(cx)` | `&App -> DockAreaState` | Serialize current state. |
| `set_toggle_button_visible(visible, cx)` | `bool, &mut Context -> ()` | Show/hide toggle buttons. |

**Traits:** `EventEmitter<DockEvent>`, `Render`.

### `DockItem` enum

```rust
pub enum DockItem {
    Split {
        axis: Axis,
        size: Option<Pixels>,
        items: Vec<DockItem>,
        sizes: Vec<Option<Pixels>>,
        view: Entity<StackPanel>,
    },
    Tabs {
        size: Option<Pixels>,
        items: Vec<Arc<dyn PanelView>>,
        active_ix: usize,
        view: Entity<TabPanel>,
    },
    Panel {
        size: Option<Pixels>,
        view: Arc<dyn PanelView>,
    },
    Tiles {
        size: Option<Pixels>,
        items: Vec<TileItem>,
        view: Entity<Tiles>,
    },
}
```

**`DockItem` methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `size(new_size)` | `impl Into<Pixels> -> DockItem` | Set self size (used when nested in Split). |
| `active_index(ix, cx)` | `usize, &mut App -> DockItem` | Set active tab index (Tabs only). |
| `split(axis, items, dock_area, w, cx)` | `Axis, Vec<DockItem>, &WeakEntity<DockArea>, ... -> DockItem` | Create Split with auto sizes. |
| `h_split(items, dock_area, w, cx)` | `Vec<DockItem>, ... -> DockItem` | Horizontal split shortcut. |
| `v_split(items, dock_area, w, cx)` | `Vec<DockItem>, ... -> DockItem` | Vertical split shortcut. |
| `split_with_sizes(axis, items, sizes, dock_area, w, cx)` | `..., Vec<Option<Pixels>>, ... -> DockItem` | Split with explicit sizes. `None` = auto. |
| `panel(view)` | `Arc<dyn PanelView> -> DockItem` | Single panel wrapper. |
| `tabs(items, dock_area, w, cx)` | `Vec<Arc<dyn PanelView>>, ... -> DockItem` | Create tab group. |
| `tab(item, dock_area, w, cx)` | `Entity<P: Panel>, ... -> DockItem` | Single-panel tab. |
| `tiles(items, metas, dock_area, w, cx)` | `Vec<DockItem>, Vec<impl Into<TileMeta>>, ... -> DockItem` | Freeform tile canvas. |
| `view()` | `Arc<dyn PanelView>` | Get the panel view for rendering. |
| `find_panel(panel)` | `Arc<dyn PanelView> -> Option<Arc<dyn PanelView>>` | Recursively find a panel. |
| `add_panel(panel, dock_area, bounds, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Add panel to first suitable container. |
| `remove_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove panel recursively. |
| `set_collapsed(collapsed, w, cx)` | `bool, ... -> ()` | Collapse/expand recursively. |

### `DockPlacement`

```rust
pub enum DockPlacement {
    Left,
    Bottom,
    Right,
    Center,
}
```

### `Dock`

Fixed container at left/bottom/right edge of `DockArea`. Unlike split panels, docks cannot be freely moved.

```rust
// Not created directly — use DockArea methods:
dock_area.set_left_dock(item, Some(px(250.)), true, window, cx);
dock_area.toggle_dock(DockPlacement::Left, window, cx);
```

**`Dock` methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `left(dock_area, w, cx)` | `WeakEntity<DockArea>, ... -> Dock` | (Internal) Create left dock. |
| `bottom(dock_area, w, cx)` | `... -> Dock` | (Internal) Create bottom dock. |
| `right(dock_area, w, cx)` | `... -> Dock` | (Internal) Create right dock. |
| `set_collapsible(collapsible, w, cx)` | `bool, ... -> ()` | Allow collapsing. |
| `set_panel(panel, w, cx)` | `DockItem, ... -> ()` | Set panel content. |
| `panel()` | `&DockItem` | Get panel. |
| `is_open()` | `bool` | Open state. |
| `toggle_open(w, cx)` | `... -> ()` | Toggle open/close. |
| `set_open(open, w, cx)` | `bool, ... -> ()` | Set open state. |
| `size()` | `Pixels` | Current size (width for left/right, height for bottom). |
| `set_size(size, w, cx)` | `Pixels, ... -> ()` | Set size (clamped to `PANEL_MIN_SIZE`). |
| `add_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Add to dock. |
| `remove_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove from dock. |

### `Panel` trait

Every panel in the dock system implements `Panel`.

```rust
pub trait Panel: EventEmitter<PanelEvent> + Render + Focusable {
    fn panel_name(&self) -> &'static str;
    fn tab_name(&self, cx: &App) -> Option<SharedString> { None }
    fn title(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
    fn title_style(&self, cx: &App) -> Option<TitleStyle> { None }
    fn title_suffix(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> { None::<Div> }
    fn closable(&self, cx: &App) -> bool { true }
    fn zoomable(&self, cx: &App) -> Option<PanelControl> { Some(PanelControl::Menu) }
    fn visible(&self, cx: &App) -> bool { true }
    fn set_active(&mut self, active: bool, w: &mut Window, cx: &mut Context<Self>) {}
    fn set_zoomed(&mut self, zoomed: bool, w: &mut Window, cx: &mut Context<Self>) {}
    fn on_added_to(&mut self, tab_panel: WeakEntity<TabPanel>, w: &mut Window, cx: &mut Context<Self>) {}
    fn on_removed(&mut self, w: &mut Window, cx: &mut Context<Self>) {}
    fn dropdown_menu(&mut self, menu: PopupMenu, w: &mut Window, cx: &mut Context<Self>) -> PopupMenu { menu }
    fn toolbar_buttons(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Option<Vec<Button>> { None }
    fn dump(&self, cx: &App) -> PanelState { PanelState::new(self) }
    fn inner_padding(&self, cx: &App) -> bool { true }
}
```

**Key required methods:** `panel_name()`, `title()`.

**Optional overrides:** `closable()` controls close button. `zoomable()` returns `None` to hide zoom controls, `Some(PanelControl::Menu)` for dropdown, or `Some(PanelControl::Button)` for direct button. `inner_padding()` controls padding when inside tabs. `dump()` for serialization.

### `PanelView` trait

Object-safe version of `Panel` used for type-erased storage. Automatically implemented for `Entity<T: Panel>`.

```rust
pub trait PanelView: 'static + Send + Sync {
    fn panel_name(&self, cx: &App) -> &'static str;
    fn panel_id(&self, cx: &App) -> EntityId;
    // ... (mirrors all Panel methods with object-safe signatures)
    fn view(&self) -> AnyView;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn dump(&self, cx: &App) -> PanelState;
}
```

### `PanelRegistry`

Global registry mapping panel names to deserialization functions. Enables layout save/restore.

```rust
use gpui_component::dock::{register_panel, DockArea, PanelState, PanelInfo};

register_panel(cx, "MyPanel", |dock_area, state, info, window, cx| {
    Box::new(cx.new(|cx| {
        MyPanel::new(window, cx)
    }))
});
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `register_panel(cx, name, deserialize)` | `&mut App, &str, Fn(WeakEntity<DockArea>, &PanelState, &PanelInfo, &mut Window, &mut App) -> Box<dyn PanelView> + 'static -> ()` | Register a panel type for deserialization. |

**`PanelRegistry::build_panel(name, dock_area, state, info, w, cx)`** — Construct a panel from serialized state. Returns `InvalidPanel` if name is unregistered.

### `StackPanel`

Implements `Panel`. Manages resizable split panels via `ResizablePanelGroup`.

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(axis, w, cx)` | `Axis, ... -> StackPanel` | Create stack. |
| `add_panel(panel, size, dock_area, w, cx)` | `Arc<dyn PanelView>, Option<Pixels>, WeakEntity<DockArea>, ... -> ()` | Append panel to end. |
| `add_panel_at(panel, placement, size, dock_area, w, cx)` | `..., Placement, ... -> ()` | Add at a placement relative to last panel. |
| `insert_panel_at(panel, ix, placement, size, dock_area, w, cx)` | `..., usize, Placement, ... -> ()` | Insert at index with placement direction. |
| `insert_panel_before(panel, ix, size, dock_area, w, cx)` | `... -> ()` | Insert before index. |
| `insert_panel_after(panel, ix, size, dock_area, w, cx)` | `... -> ()` | Insert after index. |
| `remove_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove panel by identity. |
| `panel_name()` | `"StackPanel"` | — |

The panel argument must be either a `TabPanel` or `StackPanel` (asserted).

### `TabPanel`

Implements `Panel`. Container for tabbed panels with drag-and-drop splitting.

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(stack_panel, dock_area, w, cx)` | `Option<WeakEntity<StackPanel>>, WeakEntity<DockArea>, ... -> TabPanel` | Create tab panel. Pass `None` for `stack_panel` to disable split/move (locked mode). |
| `active_panel(cx)` | `&App -> Option<Arc<dyn PanelView>>` | Get active panel (falls back to first visible). |
| `active_ix()` | `usize` | Active tab index. |
| `add_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Add to end, make active. |
| `add_panel_at(panel, placement, size, w, cx)` | `Arc<dyn PanelView>, Placement, Option<Pixels>, ... -> ()` | Add and trigger split. |
| `remove_panel(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove panel. Self-destructs if empty (unless root). |
| `set_collapsed(collapsed, w, cx)` | `bool, ... -> ()` | Collapse/expand the tab panel. |
| `panel_name()` | `"TabPanel"` | — |

**Built-in features:** Tab dragging to split/reorder, panel zoom, close button, dropdown menu, toolbar buttons.

### `Tiles`

Implements `Panel`. Freeform canvas where panels can be positioned and resized arbitrarily. Supports drag-float, resize, undo/redo.

```rust
use gpui_component::dock::{TileItem, TileMeta};

// Created via DockItem::tiles(...)
let tiles_item = DockItem::tiles(
    vec![panel_tab, another_panel],
    vec![
        TileMeta { bounds: Bounds { origin: point(px(10.), px(10.)), size: size(px(300.), px(250.)) }, z_index: 0 },
        TileMeta::default(),  // default position & size
    ],
    &dock_area_weak,
    window, cx,
);
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new(w, cx)` | `&mut Window, &mut Context -> Tiles` | Create tiles canvas. |
| `add_item(item, dock_area, w, cx)` | `TileItem, WeakEntity<DockArea>, ... -> ()` | Add tile to canvas. |
| `remove(panel, w, cx)` | `Arc<dyn PanelView>, ... -> ()` | Remove tile by panel. |

**`TileItem`:** Combines a `PanelView` with `bounds` and `z_index`.

**`TileMeta`:** Serialization struct — `{ bounds: Bounds<Pixels>, z_index: usize }`. Default creates a 200×200 tile at `(10, 10)`.

**Features:** Drag to move, resize handles on all sides, undo/redo (actions: `tiles::Undo`, `tiles::Redo`), grid-snapping via `cx.theme().tile_grid_size`.

### `PanelEvent`

```rust
pub enum PanelEvent {
    ZoomIn,
    ZoomOut,
    LayoutChanged,
}
```

Handled by `DockArea` in its `subscribe_panel` method to coordinate zoom and toggle button updates.

### `DockEvent`

```rust
pub enum DockEvent {
    LayoutChanged,
    DragDrop(AnyDrag),
}
```

### `PanelStyle`

```rust
pub enum PanelStyle {
    Default,
    // ... platform-specific variants
}
```

### Actions

```rust
// Built-in dock actions
use gpui_component::dock::dock;
dock::ToggleZoom  // toggle zoom on active panel
dock::ClosePanel  // close active panel
```

### Layout Persistence (Save/Restore)

```rust
// Save
let state: DockAreaState = dock_area.read(cx).dump(cx);
let json = serde_json::to_string(&state)?;

// Restore
let state: DockAreaState = serde_json::from_str(&json)?;
dock_area.update(cx, |area, cx| {
    area.load(state, window, cx)
        .expect("Failed to load dock state");
});
```

**`DockAreaState`:**
- `version: Option<usize>` — compatibility marker
- `center: PanelState` — main area tree
- `left_dock: Option<DockState>` / `right_dock` / `bottom_dock`

**`DockState`:** `{ panel: PanelState, placement: DockPlacement, size: Pixels, open: bool }`

**`PanelState`:** `{ panel_name: String, children: Vec<PanelState>, info: PanelInfo }`

**`PanelInfo` enum:**
- `Stack { sizes: Vec<Pixels>, axis: usize }` — 0=horizontal, 1=vertical
- `Tabs { active_index: usize }`
- `Panel(serde_json::Value)` — custom per-panel data
- `Tiles { metas: Vec<TileMeta> }`

---

## Usage Patterns & Best Practices

### Typical Full Dock Layout

```rust
// 1. Register panels
register_panel(cx, "FileExplorer", file_explorer_deserialize);
register_panel(cx, "Editor", editor_deserialize);
register_panel(cx, "Terminal", terminal_deserialize);

// 2. Create DockArea
let dock_area = cx.new(|cx| {
    DockArea::new("workspace", Some(1), window, cx)
});

// 3. Set up layout
dock_area.update(cx, |area, cx| {
    let weak = cx.entity().downgrade();

    // Center: horizontal split with editor + terminal
    area.set_center(
        DockItem::h_split(vec![
            DockItem::tabs(vec![editor_panel], &weak, window, cx),
            DockItem::tabs(vec![terminal_panel], &weak, window, cx),
        ], &weak, window, cx),
        window, cx,
    );

    // Left dock: file explorer
    area.set_left_dock(
        DockItem::tabs(vec![file_panel], &weak, window, cx),
        Some(px(260.)),
        true,
        window, cx,
    );

    // Bottom dock: output/results
    area.set_bottom_dock(
        DockItem::tabs(vec![output_panel], &weak, window, cx),
        Some(px(200.)),
        false,  // initially closed
        window, cx,
    );
});
```
