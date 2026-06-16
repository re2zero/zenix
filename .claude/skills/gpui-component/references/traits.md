# Traits & Component Patterns

Core traits, conventions, and patterns used across all gpui-component widgets.

**Contents:** [Component Structure](#component-structure) · [Required Traits](#required-traits) · [Optional Traits](#optional-traits) · [Sizable Trait](#sizable-trait) · [Disableable Trait](#disableable-trait) · [Selectable Trait](#selectable-trait) · [StyledExt Trait](#styledext-trait) · [ElementExt Trait](#elementext-trait) · [InteractiveElement & StatefulInteractiveElement](#interactiveelement--statefulinteractiveelement) · [Focus Handling](#focus-handling) · [RenderOnce Pattern](#renderonce-pattern) · [Action System](#action-system) · [Callback Conventions](#callback-conventions) · [Size Helpers](#size-helpers)

---

## Component Structure

### Stateless Component (Div-based)

A component that renders once, has no mouse interaction tracking:

```rust
use std::rc::Rc;

use crate::{ActiveTheme, Sizable, Size, StyledExt as _};
use gpui::{
    AnyElement, App, Div, ElementId, IntoElement,
    ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
};

/// A MyComponent element.
#[derive(IntoElement)]
pub struct MyComponent {
    // 1. Identity & rendering
    id: ElementId,
    base: Div,                    // Plain Div — no state tracking needed
    style: StyleRefinement,

    // 2. Configuration
    size: Size,
    disabled: bool,
    selected: bool,

    // 3. Content
    label: Option<SharedString>,
    children: Vec<AnyElement>,

    // 4. Callbacks (last)
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}
```

### Stateful Component (Stateful\<Div\>)

A component that needs mouse interaction tracking (click, hover, focus):

```rust
use gpui::{
    Interactivity, InteractiveElement, Stateful,
    StatefulInteractiveElement as _,  // trait import for on_click() etc.
};

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,          // Stateful<Div> — enables interaction tracking
    style: StyleRefinement,
    // ... other fields
}
```

**When to use `Stateful<Div>` vs `Div`:**

| Scenario                                         | Base Type       |
|--------------------------------------------------|-----------------|
| Rendering only (label, badge, separator)          | `Div`           |
| Click handlers (`on_click`)                       | `Stateful<Div>` |
| Hover tracking (`on_hover`)                       | `Stateful<Div>` |
| Focus tracking (`track_focus`)                    | `Stateful<Div>` |
| Mouse down/up handlers                            | `Stateful<Div>` |
| Keyboard action dispatch (`on_action`)            | `Stateful<Div>` |

**Creating a `Stateful<Div>`:** call `.id(...)` on a `Div`:

```rust
let id = id.into();
Self {
    base: div().flex_shrink_0().id(id),  // .id() returns Stateful<Div>
    // ...
}
```

---

## Required Traits

Every component MUST implement these traits. The exact set depends on the component type.

### IntoElement

Provided by `#[derive(IntoElement)]`. Generates the `into_any_element()` method. All components need this.

### ParentElement

Required when the component accepts children:

```rust
impl ParentElement for MyComponent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}
```

### Styled

Required when users should be able to apply style overrides. Always delegate to the `style` field:

```rust
impl Styled for MyComponent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
```

This is what powers `.refine_style(&self.style)` in `RenderOnce::render()`.

### InteractiveElement

Required for components with mouse interaction. Delegate to the `Stateful<Div>` base:

```rust
impl InteractiveElement for MyComponent {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
```

### StatefulInteractiveElement

Required if `InteractiveElement` is implemented. It is always an empty blanket impl:

```rust
impl StatefulInteractiveElement for MyComponent {}
```

The trait import `use gpui::StatefulInteractiveElement as _;` gives access to `.on_click()`, `.on_hover()`, `.track_focus()`, etc.

### RenderOnce

Every component MUST implement `RenderOnce`. This renders the component's elements:

```rust
impl RenderOnce for MyComponent {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.base
            .id(self.id)
            // Apply component defaults, size styles, interactions...
            // Apply user style overrides LAST (before children)
            .refine_style(&self.style)
            .children(self.children)
    }
}
```

---

## Optional Traits

Traits that may be implemented depending on component capabilities.

### Disableable

For components that support a disabled state:

```rust
impl Disableable for MyComponent {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
```

Usage: `my_component.disabled(true)`

In `render()`, apply visual feedback:
```rust
.when(self.disabled, |this| this.opacity(0.5).cursor_not_allowed())
```

### Selectable

For components with a selected/checked state:

```rust
impl Selectable for MyComponent {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}
```

Usage: `my_component.selected(true)`

### Sizable

For components with configurable size. See [Sizable Trait](#sizable-trait) below.

### Collapsible

For components that can be collapsed/expanded:

```rust
// From styled.rs
pub trait Collapsible {
    fn collapsed(self, collapsed: bool) -> Self;
    fn is_collapsed(&self) -> bool;
}
```

---

## Variants Pattern

Use a `Variants` trait with default method impls for each variant. This avoids a cluttered public API:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

pub trait AlertVariants: Sized {
    fn with_variant(self, variant: AlertVariant) -> Self;

    fn info(self) -> Self    { self.with_variant(AlertVariant::Info) }
    fn success(self) -> Self { self.with_variant(AlertVariant::Success) }
    fn warning(self) -> Self { self.with_variant(AlertVariant::Warning) }
    fn error(self) -> Self   { self.with_variant(AlertVariant::Error) }
}

impl AlertVariants for MyAlert {
    fn with_variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }
}
```

Consumers import the trait method: `use crate::alert::AlertVariants as _;`

---

## Sizable Trait

Defined in `crate::styled`. Provides a unified size API for all components.

### Size Enum

```rust
#[derive(Clone, Default, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Size(Pixels),    // Custom pixel size
    XSmall,          // Extra small
    Small,           // Small
    #[default]
    Medium,          // Medium (default)
    Large,           // Large
}
```

Key methods on `Size`:
- `as_str()` → `"xs"`, `"sm"`, `"md"`, `"lg"`, `"custom"`
- `from_str(s)` → parses `"xs"`, `"xsmall"`, `"sm"`, `"small"`, `"md"`, `"medium"`, `"lg"`, `"large"` (case-insensitive)
- `smaller()` / `larger()` → next smaller/larger size
- `min(other)` / `max(other)` → extremum between two sizes
- `input_px()` / `input_py()` → horizontal/vertical input padding in pixels
- `table_row_height()` → pixel height for table rows
- `table_cell_padding()` → paddings for table cells

Custom pixel sizes: `Size::from(px(30.))` or `px(30.).into()`.

### Sizable Trait

```rust
pub trait Sizable: Sized {
    /// Set the size. Accepts Size, Pixels, or any Into<Size>.
    fn with_size(mut self, size: impl Into<Size>) -> Self;

    /// Set to Size::XSmall
    fn xsmall(self) -> Self { self.with_size(Size::XSmall) }

    /// Set to Size::Small
    fn small(self) -> Self { self.with_size(Size::Small) }

    /// Set to Size::Large
    fn large(self) -> Self { self.with_size(Size::Large) }
}

impl Sizable for MyComponent {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}
```

`Size::Medium` is the default — no `.medium()` convenience method because it's the `Default`.

### StyleSized Blanket Impl

`StyleSized` is blanket-implemented for all `T: Styled`. It applies size-appropriate styling to any styled element:

```rust
pub trait StyleSized<T: Styled> {
    fn input_text_size(self, size: Size) -> Self;   // Font size for inputs
    fn input_size(self, size: Size) -> Self;        // Padding + height for inputs
    fn input_pl(self, size: Size) -> Self;          // Left padding only
    fn input_pr(self, size: Size) -> Self;          // Right padding only
    fn input_px(self, size: Size) -> Self;          // Horizontal padding
    fn input_py(self, size: Size) -> Self;          // Vertical padding
    fn input_h(self, size: Size) -> Self;           // Height only
    fn list_size(self, size: Size) -> Self;         // Full list item sizing
    fn list_px(self, size: Size) -> Self;           // List horizontal padding
    fn list_py(self, size: Size) -> Self;           // List vertical padding
    fn size_with(self, size: Size) -> Self;         // Width + height
    fn table_cell_size(self, size: Size) -> Self;   // Table cell font + padding
    fn button_text_size(self, size: Size) -> Self;  // Button font size
}
```

Usage in components:
```rust
div().input_size(self.size)          // full input sizing
div().button_text_size(self.size)    // button font sizing
div().table_cell_size(self.size)     // table cell sizing
```

---

## Disableable Trait

```rust
pub trait Disableable {
    /// Set the disabled state of the element.
    fn disabled(mut self, disabled: bool) -> Self;
}
```

Standard render-time pattern:
```rust
.when(self.disabled, |this| {
    this.opacity(0.5).cursor_not_allowed()
})
.when(!self.disabled, |this| {
    // enable interactions
    this.track_focus(&focus_handle)
})
```

---

## Selectable Trait

```rust
pub trait Selectable: Sized {
    /// Set the selected state.
    fn selected(mut self, selected: bool) -> Self;

    /// Returns true if the element is selected.
    fn is_selected(&self) -> bool;

    /// Set if the element is secondary (right-click) selected. Default no-op.
    fn secondary_selected(self, _: bool) -> Self { self }
}
```

Standard render-time pattern:
```rust
.when(self.selected, |this| {
    this.bg(cx.theme().primary)
})
```

---

## StyledExt Trait

Blanket-implemented for all `E: Styled`. Extends gpui's `Styled` with gpui-component-specific styling methods.

```rust
pub trait StyledExt: Styled + Sized {
    /// Merge user style refinements onto this element.
    fn refine_style(mut self, style: &StyleRefinement) -> Self;

    /// Horizontal flex layout with items_center.
    fn h_flex(self) -> Self;

    /// Vertical flex layout.
    fn v_flex(self) -> Self;

    /// Apply uniform or per-edge paddings.
    /// Accepts Edges<Pixels>, Edges<DefiniteLength>, or a single value for all sides.
    fn paddings<L>(self, paddings: impl Into<Edges<L>>) -> Self
    where L: Into<DefiniteLength> + Clone + Default + Debug + PartialEq;

    /// Apply uniform or per-edge margins.
    fn margins<L>(self, margins: impl Into<Edges<L>>) -> Self
    where L: Into<DefiniteLength> + Clone + Default + Debug + PartialEq;

    /// Apply corner radii from a Corners<Pixels>.
    fn corner_radii(self, radius: Corners<Pixels>) -> Self;

    /// Popover style: bg, text_color, border_1, border_color, shadow_lg, rounded.
    fn popover_style(self, cx: &App) -> Self;

    /// Border width 1px, color = theme.ring (for focused state).
    fn focused_border(self, cx: &App) -> Self;

    // Debug helpers (no-op in release builds):
    fn debug_red(self) -> Self;     // 1px red border
    fn debug_blue(self) -> Self;    // 1px blue border
    fn debug_yellow(self) -> Self;  // 1px yellow border
    fn debug_green(self) -> Self;   // 1px green border
    fn debug_pink(self) -> Self;    // 1px pink border

    /// Blue border when element is focused (debug only).
    fn debug_focused(self, focus_handle: &FocusHandle, window: &Window, cx: &App) -> Self;

    // Font weight helpers:
    fn font_thin(self) -> Self;
    fn font_extralight(self) -> Self;
    fn font_light(self) -> Self;
    fn font_normal(self) -> Self;
    fn font_medium(self) -> Self;
    fn font_semibold(self) -> Self;
    fn font_bold(self) -> Self;
    fn font_extrabold(self) -> Self;
    fn font_black(self) -> Self;
}
```

Usage examples:
```rust
use crate::StyledExt;

div().h_flex()                           // flex row, items center
div().v_flex()                           // flex column
div().paddings(px(8.))                   // uniform 8px padding
div().margins(Edges { top: px(4.), .. }) // per-edge margins
div().corner_radii(Corners::all(px(8.))) // all corners 8px
div().popover_style(cx)                  // full popover appearance
div().font_bold()                        // bold font
```

---

## ElementExt Trait

Blanket-implemented for all `T: ParentElement`. Adds a pre-paint callback:

```rust
pub trait ElementExt: ParentElement + Sized {
    /// Add a callback that fires after layout, before paint.
    /// Receives the element's computed Bounds<Pixels>.
    fn on_prepaint<F>(self, f: F) -> Self
    where
        F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static;
}
```

Implementation uses an absolute-positioned `canvas` child:
```rust
fn on_prepaint<F>(self, f: F) -> Self
where
    F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
{
    self.child(
        canvas(
            move |bounds, window, cx| f(bounds, window, cx),
            |_, _, _, _| {}
        )
        .absolute()
        .size_full(),
    )
}
```

Usage: get element bounds after layout for positioned overlays, tooltips, etc.

---

## InteractiveElement & StatefulInteractiveElement

These are gpui traits. `InteractiveElement` gives access to the element's `Interactivity`:

```rust
impl InteractiveElement for MyComponent {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
```

`StatefulInteractiveElement` is the companion marker trait. Import it as:
```rust
use gpui::StatefulInteractiveElement as _;
```

This import unlocks methods on `Stateful<Div>`:
- `.on_click(handler)` — click handler
- `.on_hover(handler)` — hover enter/leave with `&bool`
- `.track_focus(&focus_handle)` — keyboard focus tracking
- `.on_mouse_down(button, handler)` — mouse down
- `.on_mouse_down_out(handler)` — click outside

### When to Use Stateful\<Div\> vs Div

| Need                                     | Base           |
|------------------------------------------|----------------|
| Static rendering only                    | `Div`          |
| `on_click`, `on_hover`                   | `Stateful<Div>`|
| `track_focus`                            | `Stateful<Div>`|
| `on_action` (keyboard dispatch)          | `Stateful<Div>`|
| Mouse button handlers                    | `Stateful<Div>`|

---

## Focus Handling

### FocusHandle

Created via `window.use_keyed_state()` in `RenderOnce::render()`:

```rust
let focus_handle = window
    .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
    .read(cx)
    .clone();
let is_focused = focus_handle.is_focused(window);
```

### tab_index and tab_stop

Store on the component struct and pass to the focus handle:

```rust
// struct fields
tab_index: isize,   // Tab order (0 = default)
tab_stop: bool,     // Whether reachable via Tab key
```

```rust
// In render():
this.track_focus(
    &focus_handle
        .tab_index(self.tab_index)
        .tab_stop(self.tab_stop),
)
```

### track_focus Pattern

```rust
self.base
    .when(!self.disabled, |this| {
        this.track_focus(
            &focus_handle
                .tab_index(self.tab_index)
                .tab_stop(self.tab_stop),
        )
    })
```

### Focus Ring Visual

The crate provides a `FocusableExt` trait (internal, via `focus_ring()`):

```rust
pub(crate) trait FocusableExt<T: ParentElement + Styled + Sized> {
    fn focus_ring(
        self,
        is_focused: bool,
        margins: Pixels,
        window: &Window,
        cx: &App,
    ) -> Self;
}
```

Used at the end of render:
```rust
self.base
    // ... all styling and children ...
    .focus_ring(is_focused, px(0.), window, cx)
```

The focus ring renders as an absolute-positioned overlaying div with the theme's ring color at 20% opacity.

---

## RenderOnce Pattern

The `render()` method is the heart of every component. Key rules:

### 1. always call `refine_style` before children

```rust
fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
    self.base
        .flex()
        .items_center()
        // Apply component defaults above refine_style
        .refine_style(&self.style)   // User overrides go here
        .children(self.children)      // Children go last
}
```

### 2. Use FluentBuilder conditionals

```rust
use gpui::prelude::FluentBuilder as _;  // Required import

div()
    .when(self.disabled, |this| this.opacity(0.5))
    .when(self.selected, |this| this.bg(cx.theme().primary))
    .when_some(self.label.as_ref(), |this, label| {
        this.child(div().child(label.clone()))
    })
    .when_some(self.on_click, |this, on_click| {
        this.on_click(move |event, window, cx| { on_click(event, window, cx); })
    })
    .map(|this| {
        // arbitrary transform
        this
    })
```

### 3. Children rendering

```rust
impl ParentElement for MyComponent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

// In render():
self.base.children(self.children)
```

### 4. Theme access

```rust
use crate::ActiveTheme;

// In render():
div()
    .bg(cx.theme().surface)
    .text_color(cx.theme().foreground)
    .border_color(cx.theme().border)
    .rounded(cx.theme().radius)
```

Full theme color palette on `cx.theme()`:
- `background`, `foreground` — page colors
- `surface`, `muted`, `card`, `popover` — surface colors
- `primary`, `secondary`, `accent` — semantic colors
- `border`, `input`, `ring` — border/input/focus
- `destructive`, `warning`, `success`, `info` — status colors
- `chart1` through `chart5` — chart palette
- `radius` — border radius
- `shadow` — whether shadows are enabled
- `font_family`, `font_size` — typography

---

## Action System

Actions enable keyboard-driven interaction dispatch. Built on gpui's `Action` trait.

### Declaring Actions: `actions!` Macro

The `actions!` macro (from gpui) generates unit-struct action types:

```rust
use gpui::actions;

actions!(ui, [
    Cancel,
    SelectUp,
    SelectDown,
    SelectLeft,
    SelectRight,
    SelectFirst,
    SelectLast,
]);
```

Each generates a struct like `pub struct SelectUp;` implementing `gpui::Action`. The first argument is the namespace (`ui`).

Custom actions with data:

```rust
#[derive(Clone, Action, PartialEq, Eq)]
#[action(namespace = ui, no_json)]
pub struct Confirm {
    pub secondary: bool,
}
```

### Dispatching on_action

On the component element, use `.on_action()` to wire handlers:

```rust
// For Component (cx.listener):
self.base
    .on_action(cx.listener(Self::on_action_confirm))

// For Model/Entity (window.listener_for):
self.base
    .on_action(window.listener_for(&self.state, MyState::on_enter))

// Ad-hoc closure:
self.base
    .on_action(move |_: &Cancel, window, cx| {
        // handle cancel
    })
```

Handler signatures on `Model<State>`:
```rust
impl MyState {
    fn on_action_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        // ...
    }
}
```

Handler signatures on `Component`:
```rust
impl MyComponent {
    fn on_action_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        // ...
    }
}
```

### Key Context and focus_trap

```rust
const CONTEXT: &str = "MyComponent";

// In render:
self.base
    .key_context(CONTEXT)
    .track_focus(&focus_handle)
    .on_action(cx.listener(Self::on_action_confirm))
```

The `key_context` ensures action bindings are scoped. Use `cx.propagate()` to let unhandled actions bubble up.

---

## Callback Conventions

### Standard Signatures

```rust
// Click events
on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>

// State change callbacks (value first)
on_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>
on_change: Option<Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>>
on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>

// Hover state
on_hover: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>
```

### Storing Callbacks

Always `Option<Rc<dyn Fn(...)>>` — components are cloned, and `Rc` is cheap:

```rust
pub struct MyComponent {
    // ...
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}
```

### Setting Callbacks (Builder Style)

```rust
impl MyComponent {
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}
```

### Wiring in render()

```rust
.when_some(self.on_click, |this, on_click| {
    this.on_click(move |event, window, cx| {
        on_click(event, window, cx);
    })
})
```

### InteractiveElementExt (Double Click)

```rust
use crate::event::InteractiveElementExt;

// On any Stateful<E> element:
div().on_double_click(|event, window, cx| {
    // fires when event.click_count() == 2
})
```

---

## Size Helpers

These are gpui-provided functions. Used everywhere for defining dimensions:

```rust
use gpui::{px, rems, relative, ems};

// Absolute pixels
px(16.)          // 16 pixels
px(0.5)          // 0.5 pixels

// Relative to rem size
rems(1.0)        // 1 rem
rems(0.875)      // 0.875 rem

// Relative to parent font size
ems(1.0)         // 1 em

// Relative fraction (0.0-1.0)
relative(0.5)    // 50% of available space
```

Common patterns in components:

```rust
// Dynamic font size based on Size enum
let font_size = match self.size {
    Size::XSmall => rems(0.75),
    Size::Small => rems(0.875),
    Size::Medium | Size::Size(_) => rems(1.0),
    Size::Large => rems(1.125),
};

// Logo/svg sizing
let height = match self.size {
    Size::Large => px(24.),
    Size::Medium => px(20.),
    Size::Small => px(16.),
    Size::XSmall => px(14.),
    Size::Size(v) => v,
};
```

---

## Trait Implementation Checklist

For a new component, verify:

- [ ] `#[derive(IntoElement)]`
- [ ] Fields: `id: ElementId`, `base: Div` (or `Stateful<Div>`), `style: StyleRefinement`
- [ ] `impl RenderOnce` — calls `.refine_style(&self.style)` on root element
- [ ] `impl Styled` — returns `&mut self.style`
- [ ] `impl ParentElement` — if component accepts children
- [ ] `impl InteractiveElement` + `impl StatefulInteractiveElement` — if interactive
- [ ] `impl Sizable` — if size variants
- [ ] `impl Disableable` — if can be disabled
- [ ] `impl Selectable` — if can be selected
- [ ] Callbacks stored as `Option<Rc<dyn Fn(...)>>`
- [ ] Doc comment on struct and all public methods
- [ ] Import `use gpui::prelude::FluentBuilder as _;`
