---
name: gpui-component
description: How to use the gpui-component UI library (Button, Input, Select, Dialog, Tabs, Sidebar, List, Table, Chart, Dock, Slider, Switch, Toggle, Combobox, etc.) in GPUI applications. Use this skill whenever building any UI element, dialog, form, settings panel, sidebar, or data display in a GPUI project that depends on gpui-component — even if the user doesn't explicitly name a component.
---

# gpui-component Skill

Progressive-disclosure navigation for gpui-component v0.5.x (longbridge/gpui-component).
**Fast path: Section 4 has copy-paste combo patterns for Dialog+Input, Tabs+Toggle+Slider, Sidebar+List.**
**Section 1-2 have everything for 80% of tasks. Load reference files (Section 3) only for deep API detail.**
---

## 1. Quick Start (minimum to use any component)

### Cargo.toml
```toml
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit"] }
gpui-component = { git = "https://github.com/longbridge/gpui-component" }
gpui-component-assets = { git = "https://github.com/longbridge/gpui-component" }
```

### Entry Point (REQUIRED)
```rust
use gpui_component::{Root, init};

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            init(cx);  // MUST be first before any component use

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|_| MyApp);
                    cx.new(|cx| Root::new(view, window, cx))  // Root wraps first view
                }).expect("Failed to open window");
            }).detach();
        });
}
```

**Critical: `init(cx)` must be called exactly once in `app.run()`. `Root::new(view, window, cx)` must wrap the first-level view of every window.**

### Two Component Patterns

| Pattern | When | Example |
|---------|------|---------|
| **Stateless** (`RenderOnce`) | Most components: Button, Checkbox, Badge, Icon, etc. | `Button::new("id").primary().label("OK").on_click(\|_, _, _\| {})` |
| **Stateful** (`Entity<State>`) | Input, Select, Combobox, Slider, List, Table, Tree | Hold `Entity<InputState>` in struct, pass `&self.input` to `Input::new(&self.input)` |

**Stateless pattern:**
```rust
use gpui_component::button::Button;

fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    Button::new("btn").primary().label("Submit").on_click(|_, _, _| println!("clicked"))
}
```

**Stateful pattern:**
```rust
use gpui_component::input::{Input, InputState};

struct MyView { name: Entity<InputState> }

impl MyView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self { name: cx.new(|cx| InputState::new(window, cx).placeholder("Name")) }
    }
}

impl Render for MyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Input::new(&self.name)
    }
}
```

### Sizes
`.xsmall()` `.small()` `.medium()` (default) `.large()` — via `Sizable` trait.

### Theme Colors
```rust
use gpui_component::ActiveTheme as _;
cx.theme().primary       // Primary brand color
cx.theme().background    // Page background
cx.theme().foreground    // Default text
cx.theme().border        // Default border
cx.theme().surface       // Card/surface bg
cx.theme().muted         // Muted bg (skeleton, switch)
cx.theme().muted_foreground  // Disabled/subtle text
cx.theme().destructive   // Danger/destructive
cx.theme().ring          // Focus ring
```

### Layout Helpers
```rust
use gpui_component::{h_flex, v_flex};
// h_flex() == div().flex().flex_row().items_center()
// v_flex() == div().flex().flex_col()
```

### Overlay Layers (add to your root render)
```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div().size_full()
        .child(self.main_content(window, cx))
        .children(Root::render_dialog_layer(cx))
        .children(Root::render_sheet_layer(cx))
        .children(Root::render_notification_layer(cx))
}
```
**Without these `Root::render_*_layer` calls, dialogs/sheets/notifications will NOT appear.**

---

## 2. Component Catalog

Each entry shows: Component name | Import path | State requirement | Key builder methods.

### 2.1 Input & Form

| Component | Import | State | Key API |
|-----------|--------|-------|---------|
| `Input` | `input::{Input, InputState}` | Entity | `.placeholder()`, `.cleanable()`, `.disabled()`, `.prefix()`, `.suffix()`, `.mask_toggle()`, `.appearance(false)`, `.multi_line()`, `.code_editor("rust")`, `.auto_grow(min,max)`, `.searchable()`, `.folding()`, `.line_number()` |
| `NumberInput` | `input::number_input::{NumberInput, NumberInputState}` | Entity | Numeric with step; similar to Input |
| `OtpInput` | `input::otp_input::{OtpInput, OtpInputState}` | Entity | One-time password segmented input |
| `Select` | `select::{Select, SelectState}` | Entity\<SelectState\<D\>\> | `SelectState::new(delegate, selected_index, w, cx)`, `.searchable()`, `.icon()`, `.title_prefix()` |
| `Combobox` | `combobox::{Combobox, ComboboxState}` | Entity | Multi-select, searchable, `.render_trigger()` custom trigger |
| `Checkbox` | `checkbox::Checkbox` | Stateless | `.checked(bool)`, `.on_click(\|&bool, ...\|)` |
| `Switch` | `switch::Switch` | Stateless | `.checked(bool)`, `.on_click(\|&bool, ...\|)` |
| `Radio` | `radio::{Radio, RadioGroup}` | Stateless | `RadioGroup::new()`, `.child(Radio::new("id"))` |
| `Slider` | `slider::{Slider, SliderState}` | Entity | Range slider, `.min()`, `.max()`, `.step()` |
| `Toggle` | `button::toggle::Toggle` | Stateless | Toggle button variant |
| `Rating` | `rating::Rating` | Stateless | Star rating, `.count()`, `.value()` |
| `Stepper` | `stepper::Stepper` | Stateless | Increment/decrement, `.min()`, `.max()` |
| `ColorPicker` | `color_picker::{ColorPicker, ColorPickerState}` | Entity | Color picker with popover |
| `DatePicker` | `time::date_picker::{DatePicker, DatePickerState}` | Entity | Calendar date picker |
| `Form` | `form::{v_form, h_form, field}` | N/A | Layout: `v_form().child(field().label("Name").child(...))` |

> **DETAIL**: Load `references/input-form.md` for full `InputState` API (InputEvent, value access, LSP/editor mode), `Select` delegate pattern, `Combobox` custom triggers.

### 2.2 Display & Feedback

| Component | Import | State | Key API |
|-----------|--------|-------|---------|
| `Button` | `button::{Button, ButtonGroup}` | Stateless | `.label()`, `.primary()`, `.danger()`, `.warning()`, `.success()`, `.ghost()`, `.link()`, `.disabled()`, `.loading()`, `.selected()`, `.icon(IconName::Plus)`, `.on_click()` |
| `ButtonGroup` | `button::ButtonGroup` | Stateless | `.child(Button::new(...))`, `.on_click(\|indices, ...\|)` |
| `DropdownButton` | `button::DropdownButton` | Stateless | Button with dropdown menu |
| `Icon` | `{Icon, IconName}` | Stateless | `Icon::new(IconName::Check).small().text_color(cx.theme().primary)` |
| `Badge` | `badge::Badge` | Stateless | `.variant()` for color |
| `Tag` | `tag::Tag` | Stateless | Closable with `.on_close()` |
| `Avatar` | `avatar::Avatar` | Stateless | `.src()`, `.fallback()`, `AvatarGroup` |
| `Label` | `label::Label` | Stateless | Form label, `.required()` shows asterisk |
| `Kbd` | `kbd::Kbd` | Stateless | Keyboard shortcut display |
| `Alert` | `alert::Alert` | Stateless | `.info()`, `.success()`, `.warning()`, `.error()`, `.title()`, `.description()` |
| `Spinner` | `spinner::Spinner` | Stateless | Loading indicator |
| `Skeleton` | `skeleton::Skeleton` | Stateless | Loading placeholder |
| `ProgressBar` | `progress::ProgressBar` | Stateless | `.value(f32)` (0-100) |
| `ProgressCircle` | `progress::ProgressCircle` | Stateless | Circular progress |
| `Tooltip` | `tooltip::Tooltip` | Via element | `.tooltip("text")` on any element with `.id()`, or `Button::new("b").tooltip("tip")` |
| `HoverCard` | `hover_card::{HoverCard, HoverCardState}` | Entity | Rich hover popup |
| `Link` | `link::Link` | Stateless | Hyperlink styled text |
| `Clipboard` | `clipboard::Clipboard` | Stateless | Copy-to-clipboard button |
| `Image` | `image::Image` | Stateless | Image display |
| `Separator` | `separator::Separator` | Stateless | `.horizontal()` or `.vertical()` |

> **DETAIL**: Load `references/display-feedback.md` for Button variants full API, IconName enumeration, notification levels.

### 2.3 Overlays & Popups

| Component | How to open | Notes |
|-----------|-------------|-------|
| `Dialog` | `window.open_dialog(cx, \|dialog, w, cx\| { dialog.title("Title").child(...).footer(...) })` | Modal dialog, requires `WindowExt` import. `.w()`, `.overlay()`, `.keyboard()`, `.close_button()` |
| `AlertDialog` | `window.open_alert_dialog(cx, \|alert, _, _\| { alert.warning().title("...").description("...") })` | Opinionated alert, `WindowExt` required |
| `Sheet` | `window.open_sheet(cx, \|sheet, ...\| { ... })` or `window.open_sheet_at(Placement::Right, ...)` | Side panel overlay |
| `Notification` | `window.push_notification("text", cx)` or `window.push_notification(Notification::new("id").success().message("body"), cx)` | Toast notifications |
| `Popover` | `popover::Popover` | Anchored floating overlay |
| `PopupMenu` | `menu::PopupMenu` | Context menu |
| `DropdownMenu` | `menu::DropdownMenu` | Dropdown triggered menu |

**CRITICAL**: Overlays require `Root` as the window's top-level view AND `Root::render_dialog_layer(cx)` (etc.) in your render method. Without both, overlays silently fail.

> **DETAIL**: Load `references/overlay.md` for full Dialog/Sheet/Notification API, `WindowExt` trait methods, notification lifecycle.

### 2.4 Navigation & Layout

| Component | Import | State | Key API |
|-----------|--------|-------|---------|
| `Tab` / `TabBar` | `tab::{Tab, TabBar}` | Stateless | `TabBar::new("id").child(Tab::new("tab1").child("Label"))` |
| `Sidebar` | `sidebar::{Sidebar, SidebarMenu, SidebarHeader, SidebarFooter}` | — | Full app sidebar with menu groups |
| `TitleBar` | `title_bar::TitleBar` | Stateless | Window title bar replacement |
| `Breadcrumb` | `breadcrumb::Breadcrumb` | Stateless | Navigation trail |
| `Pagination` | `pagination::Pagination` | Stateless | Page navigation `.total()`, `.current()` |
| `Accordion` | `accordion::Accordion` | Stateless | Collapsible sections with headers |
| `Collapsible` | `collapsible::Collapsible` | Stateless | Single collapsible section |
| `GroupBox` | `group_box::GroupBox` | Stateless | Labeled container with border |
| `Resizable` | `resizable::Resizable` | Stateless | Draggable split: `.child(top).child(bottom)` |
| `Scrollbar` | `scroll::Scrollbar` | — | Custom scrollbar; `scroll::ScrollableElement` trait adds `.scrollable()` |
| `FocusTrap` | `focus_trap::FocusTrap` | Stateless | Keyboard focus trap for modals |
| `Separator` | `separator::Separator` | Stateless | `.horizontal()` `.vertical()` |
| `StatusBar` | `status_bar::StatusBar` | Stateless | Bottom status bar |
| `WindowBorder` | `window_border::WindowBorder` | Stateless | Custom window border (Linux only for color) |

> **DETAIL**: Load `references/navigation-layout.md` for Dock system (DockArea, DockItem, Panel, Tiles), Sidebar full API, Resizable patterns.

### 2.5 Data Display

| Component | Import | State | Key API |
|-----------|--------|-------|---------|
| `Table` | `table::{Table, TableState, TableDelegate}` | Entity | `TableDelegate` trait: `fn render_cell()`, `fn render_header()` |
| `DataTable` | `table::DataTable` | Entity | Higher-level table with built-in features |
| `List` | `list::{List, ListState, ListDelegate, ListItem}` | Entity | `ListDelegate` trait, searchable, keyboard nav |
| `VirtualList` | `{v_virtual_list, h_virtual_list}` | Stateless | High-perf: `v_virtual_list(count, ix, \|ix, size\| { render_item(ix) })` |
| `Tree` | `tree::{Tree, TreeState, TreeDelegate}` | Entity | `TreeDelegate` trait, expandable hierarchy |
| `DescriptionList` | `description_list::DescriptionList` | Stateless | Key-value term/description pairs |
| `Settings` | `setting::Settings` | — | Settings panel with pages, groups, fields |

> **DETAIL**: Load `references/data.md` for TableDelegate pattern, ListDelegate with search/filter, VirtualList scroll handles, Tree expand/collapse.

### 2.6 Charts

| Component | Import | State | Key API |
|-----------|--------|-------|---------|
| `Chart` | `chart::Chart` | Stateless | Wrapper for chart types |
| `BarChart` | `chart::BarChart` | Stateless | Vertical/horizontal bars |
| `LineChart` | `chart::LineChart` | Stateless | Line series |
| `AreaChart` | `chart::AreaChart` | Stateless | Filled area under line |
| `PieChart` | `chart::PieChart` | Stateless | Donut/pie |
| `CandlestickChart` | `chart::CandlestickChart` | Stateless | OHLC financial chart |
| `Plot` | `plot::Plot` | Derive macro | `#[derive(IntoPlot)]` on data struct for chart data |

> **DETAIL**: Load `references/data.md` (charts section) for `IntoPlot` derive macro, axis configuration, color series.

### 2.7 Advanced

| Component | Import | State | Notes |
|-----------|--------|-------|-------|
| `DockArea` | `dock::{DockArea, DockItem, Panel, PanelView}` | Entity | Full dock panel system with split/tabs, drag-drop, zoom, serialization |
| `Input::code_editor` | `input::InputState` method | N/A | Code editor mode with syntax highlighting (Tree-sitter), LSP, line numbers, folding |
| `highlighter` | `highlighter::Highlighter` | — | Syntax highlighting engine |
| `text::TextView` | `text::text_view::TextView` | Entity | Text selection and display view |

### 2.8 StyledExt Trait Methods

All `Styled` elements get these from `use gpui_component::StyledExt as _`:

| Method | Effect |
|--------|--------|
| `.h_flex()` | `flex().flex_row().items_center()` |
| `.v_flex()` | `flex().flex_col()` |
| `.paddings(edges)` | Set all 4 paddings at once |
| `.margins(edges)` | Set all 4 margins at once |
| `.popover_style(cx)` | Popover bg/border/shadow/radius |
| `.focused_border(cx)` | `border_1().border_color(cx.theme().ring)` |
| `.refine_style(&style)` | Merge external StyleRefinement (use in RenderOnce) |
| `.font_thin()` … `.font_black()` | 9 font-weight presets |
| `.corner_radii(corners)` | Individual corner radii |
| `debug_red/blue/yellow/green/pink()` | Debug borders (only in debug builds) |
| `debug_focused(handle, w, cx)` | Debug border when focused |

### 2.9 Shared Traits

| Trait | Methods | Impl by |
|-------|---------|---------|
| `Sizable` | `.xsmall()`, `.small()`, `.medium()`, `.large()`, `.with_size(Size)` | Most components |
| `Disableable` | `.disabled(bool)` | Interactive components |
| `Selectable` | `.selected(bool)`, `.is_selected()` | Selectable components |
| `StyleSized` | Auto-impl: applies size-dependent styling | Elements implementing `Sizable` |
| `Collapsible` | `.collapsed(bool)` | Collapsible elements |
| `StyledExt` | Layout & style helpers (see 2.8) | All `Styled` elements |
| `ElementExt` | `.on_prepaint(\|bounds, w, cx\| {})` | All `ParentElement` |
| `WindowExt` | `.open_dialog()`, `.open_sheet()`, `.push_notification()`, `.focused_input()`, `.selected_text()` | `Window` |

### 2.10 Re-Exports from Root
```rust
use gpui_component::{
    // Components
    Button, Checkbox, Icon, IconName, Switch, Radio,
    // Layout
    h_flex, v_flex,
    // Theme
    ActiveTheme, Theme, ThemeColor, ThemeMode, ThemeRegistry,
    // Utility types
    Size, IndexPath, Root, WindowBorder,
    // Traits
    Sizable, Disableable, Selectable, StyledExt,
    ElementExt, WindowExt, FocusTrapElement,
    // Text
    Rope, RopeExt, RopeLines,
    // VirtualList
    v_virtual_list, h_virtual_list, VirtualList, VirtualListScrollHandle,
    // Input
    InputState,
    // Time
    calendar, date_picker,
    // Global
    GlobalState,
};
```

---

## 3. Reference Files Index

Load these files ONLY when you need deeper detail on a topic. Each file covers its topic exhaustively.

| Topic | Reference File | Load when |
|-------|---------------|-----------|
| Full Setup Guide | `references/setup.md` | Need `init()` details, `Root` customization, asset loading, platform quirks |
| Input & Form Components | `references/input-form.md` | Using Input/Select/Combobox/Form, need InputEvent API, validation, mask patterns, LSP/delegate patterns |
| Display & Feedback | `references/display-feedback.md` | Using Button variants, IconName list, Alert/Tag/Badge full API |
| Overlays & Popups | `references/overlay.md` | Using Dialog/Sheet/Notification, need WindowExt full API, notification lifecycle |
| Navigation & Layout | `references/navigation-layout.md` | Using Tabs/Sidebar/Dock/Resizable, need Dock system full API |
| Data Display | `references/data.md` | Using Table/List/Tree/VirtualList, need delegate trait implementations, chart IntoPlot |
| Theme System | `references/theming.md` | Theme creation, color customization, theme registry, ThemeColor full fields, font setup |
| Shared Traits & Patterns | `references/traits.md` | Building custom components, understanding trait contracts, RenderOnce patterns |


> **Before loading reference files:** check Section 4 Combo Patterns first — the most common UI tasks (dialog+input, tabs+toggle+slider, sidebar+list) are covered there with complete copy-paste code.
---


## 4. Common Combo Patterns (self-contained — copy-paste starting points)

Each pattern is complete. No reference file needed.

### Dialog + Input

```
use gpui_component::{WindowExt, Root};
use gpui_component::input::{Input, InputState};
use gpui_component::dialog::{Dialog, DialogButtonProps};
use gpui_component::button::Button;

// State
struct MyView { name: Entity<InputState> }
impl MyView {
    fn new(w: &mut Window, cx: &mut Context<Self>) -> Self {
        Self { name: cx.new(|cx| InputState::new(w, cx).placeholder("Enter…")) }
    }
}

// Render
fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let input = self.name.clone();
    div().child(Button::new("open").label("Open").on_click(move |_, win, cx| {
        let input = input.clone();
        win.open_dialog(cx, move |d, _, _| {
            d.title("My Dialog").content(move |c, _, _| c.child(Input::new(&input)))
             .button_props(DialogButtonProps::default().show_cancel(true).on_ok(move |_, _, cx| {
                 println!("{}", input.read(cx).value());
                 true // close dialog
             }))
        });
    }))
    .children(Root::render_dialog_layer(cx))
}
```

### Tabs + Toggle + Slider

```
use gpui_component::{ActiveTheme as _, v_flex};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::button::toggle::Toggle;
use gpui_component::slider::{Slider, SliderState, SliderValue};
use gpui_component::theme::{Theme, ThemeMode};

struct Settings { tab: Entity<usize>, slider: Entity<SliderState> }
impl Settings {
    fn new(w: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            tab: cx.new(|_| 0usize),
            slider: cx.new(|_| SliderState::new().min(10.).max(24.).step(1.)
                .default_value(SliderValue::Single(14.))),
        }
    }
}

fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let t = *self.tab.read(cx);
    v_flex()
        .child(TabBar::new("tb")
            .child(Tab::new("g").label("General").selected(t==0)
                .on_click(cx.listener(|this,_,_,cx| *this.tab.update(cx,|t,_|*t=0))))
            .child(Tab::new("a").label("Appearance").selected(t==1)
                .on_click(cx.listener(|this,_,_,cx| *this.tab.update(cx,|t,_|*t=1)))))
        .child(if t==0 {
            Toggle::new("dark").checked(cx.theme().is_dark()).label("Dark Mode")
                .on_click(|checked, w, cx| Theme::change(
                    if *checked {ThemeMode::Dark} else {ThemeMode::Light}, Some(w), cx))
                .into_any_element()
        } else { div().child(Slider::new(&self.slider)).into_any_element() })
}
```

### Sidebar + List with Delegate

```
use gpui_component::{h_flex, IndexPath, ActiveTheme as _};
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};

struct Delegate { items: Vec<(&'static str, &'static str)> }
impl ListDelegate for Delegate {
    type Item = ListItem;
    fn items_count(&self, _: usize, _: &App) -> usize { self.items.len() }
    fn render_item(&mut self, ix: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>)
        -> Option<Self::Item> {
        let (k, v) = self.items.get(ix.row)?;  // ix.row indexes into items
        Some(ListItem::new(("item", ix.row)).child(
            h_flex().gap_2().child(*k).child(div().text_color(cx.theme().muted_foreground).child(*v))))
    }
    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}
}

struct View { list: Entity<ListState<Delegate>> }
impl View {
    fn new(w: &mut Window, cx: &mut Context<Self>) -> Self {
        let d = Delegate { items: vec![("Ctrl+S","Save"), ("Ctrl+O","Open"), ("Ctrl+Q","Quit")] };
        Self { list: cx.new(|cx| ListState::new(d, w, cx)) }
    }
}
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().child(Sidebar::new("sb").side(Side::Left).w(px(200.))
            .child(SidebarMenu::new().child(SidebarMenuItem::new("Shortcuts"))))
        .child(div().flex_1().child(List::new(&self.list).small()))
    }
}
```

## 5. General Patterns

### Reading Input Value
```rust
// Get value immediately
let value = input.read(cx).value();  // returns SharedString

// Subscribe to changes
cx.subscribe_in(&input, window, |view, state, event, window, cx| {
    match event {
        InputEvent::Change => { let v = state.read(cx).value(); }
        InputEvent::PressEnter { .. } => { /* submit */ }
        InputEvent::Focus | InputEvent::Blur => {}
    }
});
```


### Theme Switch
```rust
use gpui_component::Theme;

// Toggle light/dark
cx.update_global::<Theme, _>(|theme, cx| theme.toggle_mode(cx));

// Load named theme
Theme::global_mut(cx).apply_config(&theme_config);
```

### Custom Font
```rust
// After init(cx):
let theme = Theme::global_mut(cx);
theme.font_family = "YourFont".into();
theme.mono_font_family = "YourMonoFont".into();
```

---

## 6. Critical Pitfalls

1. **MUST call `gpui_component::init(cx)`** before any component use. Forgetting this causes panics.
2. **MUST wrap window's first view in `Root::new(view, window, cx)`**. Without Root, dialogs/sheets/notifications silently fail.
3. **MUST add overlay layers in render**: `Root::render_dialog_layer(cx)`, `Root::render_sheet_layer(cx)`, `Root::render_notification_layer(cx)`.
4. **Stateful components need `Entity<State>` in struct**: Input, Select, Combobox, Slider, List, Table, Tree. Create with `cx.new(|cx| State::new(window, cx))`.
5. **Don't nest `Entity<InputState>` inside other state structs unnecessarily** — each `InputState` is its own `Entity` managed by `cx.new()`.
6. **`id` parameter in `Component::new("id")` must be unique** within the render tree.
7. **Tooltip requires `.id()` on the parent element** (or use `Button::new("b").tooltip("text")` which auto-sets id).
8. **Button cursor is `default`, not `pointer`** (desktop app convention). Use `.link()` variant for pointer cursor.
9. **WindowExt import required**: `use gpui_component::WindowExt;` to call `window.open_dialog()`, `window.close_dialog()`, etc.
10. **Icon SVGs not bundled** — add `gpui-component-assets` dependency for Lucide icons, or provide your own SVG files named per `IconName`.
