# Traits & Component Patterns

Core traits, conventions, and patterns for gpui-component widgets.

**Contents:** [Component Structure](#component-structure) · [Required Traits](#required-traits) · [Optional Traits](#optional-traits) · [Variants](#variants) · [Sizable](#sizable) · [StyledExt](#styledext) · [Focus Handling](#focus-handling) · [RenderOnce & FluentBuilder](#renderonce--fluentbuilder) · [Action System](#action-system) · [Callbacks](#callbacks) · [Import Organization](#import-organization) · [Doc Comments](#doc-comments) · [Checklist](#checklist)

---

## Component Structure

**Stateless (Div-based):** for static rendering (label, badge, separator).
**Stateful (Stateful\<Div\>):** for click, hover, focus, keyboard dispatch.

```rust
/// A MyComponent element.
#[derive(IntoElement)]
pub struct MyComponent {
    id: ElementId,
    base: Div,                           // or Stateful<Div> for interactivity
    style: StyleRefinement,
    size: Size,
    disabled: bool,
    selected: bool,
    label: Option<SharedString>,
    children: Vec<AnyElement>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}
```

Stateful constructor: `base: div().flex_shrink_0().id(id)` returns `Stateful<Div>`.
Use `Stateful<Div>` for `on_click`, `on_hover`, `track_focus`, `on_action`, mouse handlers. Otherwise `Div`.

---

## Required Traits

| Trait | When | Implementation |
|-------|------|---------------|
| `IntoElement` | Always | `#[derive(IntoElement)]` |
| `ParentElement` | Accepts children | `fn extend(&mut self, e: …) { self.children.extend(e) }` |
| `Styled` | Style overrides | `fn style(&mut self) -> &mut StyleRefinement { &mut self.style }` |
| `InteractiveElement` | Interactive | `fn interactivity(&mut self) -> &mut Interactivity { self.base.interactivity() }` |
| `StatefulInteractiveElement` | Interactive | `impl StatefulInteractiveElement for MyComponent {}` (empty) |
| `RenderOnce` | Always | See [RenderOnce & FluentBuilder](#renderonce--fluentbuilder) |

Import `use gpui::StatefulInteractiveElement as _;` to unlock `.on_click()`, `.on_hover()`, `.track_focus()`.

---

## Optional Traits

### Disableable

```rust
pub trait Disableable { fn disabled(mut self, disabled: bool) -> Self; }
impl Disableable for MyComponent {
    fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
}
```

Render: `.when(self.disabled, |this| this.opacity(0.5).cursor_not_allowed())`

### Selectable

```rust
pub trait Selectable: Sized {
    fn selected(mut self, selected: bool) -> Self;
    fn is_selected(&self) -> bool;
    fn secondary_selected(self, _: bool) -> Self { self }
}
impl Selectable for MyComponent {
    fn selected(mut self, s: bool) -> Self { self.selected = s; self }
    fn is_selected(&self) -> bool { self.selected }
}
```

Render: `.when(self.selected, |this| this.bg(cx.theme().primary))`

### Collapsible

```rust
pub trait Collapsible {
    fn collapsed(self, collapsed: bool) -> Self;
    fn is_collapsed(&self) -> bool;
}
```

---

## Variants

Enum + trait with default method impls per variant:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AlertVariant { #[default] Info, Success, Warning, Error }

pub trait AlertVariants: Sized {
    fn with_variant(self, variant: AlertVariant) -> Self;
    fn info(self) -> Self    { self.with_variant(AlertVariant::Info) }
    fn success(self) -> Self { self.with_variant(AlertVariant::Success) }
    fn warning(self) -> Self { self.with_variant(AlertVariant::Warning) }
    fn error(self) -> Self   { self.with_variant(AlertVariant::Error) }
}
impl AlertVariants for MyAlert {
    fn with_variant(mut self, v: AlertVariant) -> Self { self.variant = v; self }
}
```

Consumer: `use crate::alert::AlertVariants as _;`

---

## Sizable

Defined in `crate::styled`. Unified size API.

```rust
#[derive(Clone, Default, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Size(Pixels),                            // Custom: px(30.).into(), Size::from(px(30.))
    XSmall, Small, #[default] Medium, Large,
}
```

Methods: `as_str()` → `"xs"/"sm"/"md"/"lg"/"custom"`, `from_str(s)`, `smaller()`/`larger()`, `min()`/`max()`, `input_px()`/`input_py()`, `table_row_height()`.

```rust
pub trait Sizable: Sized {
    fn with_size(mut self, size: impl Into<Size>) -> Self;
    fn xsmall(self) -> Self { self.with_size(Size::XSmall) }
    fn small(self) -> Self  { self.with_size(Size::Small) }
    fn large(self) -> Self  { self.with_size(Size::Large) }
    // Medium is Default — no convenience method needed
}
```

### StyleSized (blanket for T: Styled)

```rust
pub trait StyleSized<T: Styled> {
    fn input_text_size(self, size: Size) -> Self;   // font size
    fn input_size(self, size: Size) -> Self;        // padding + height
    fn list_size(self, size: Size) -> Self;         // list item sizing
    fn table_cell_size(self, size: Size) -> Self;   // font + padding
    fn button_text_size(self, size: Size) -> Self;  // button font
    fn size_with(self, size: Size) -> Self;         // w + h
}
```

Usage: `div().input_size(self.size)`, `div().button_text_size(self.size)`.

---

## StyledExt

Blanket-impl for `E: Styled`. Import `use crate::StyledExt as _;`.

```rust
pub trait StyledExt: Styled + Sized {
    fn refine_style(self, style: &StyleRefinement) -> Self;      // merge user overrides
    fn h_flex(self) -> Self;                                     // flex row, items center
    fn v_flex(self) -> Self;                                     // flex column
    fn paddings<L>(self, p: impl Into<Edges<L>>) -> Self;
    fn margins<L>(self, m: impl Into<Edges<L>>) -> Self;
    fn corner_radii(self, r: Corners<Pixels>) -> Self;
    fn popover_style(self, cx: &App) -> Self;                    // bg + border + shadow + rounded
    fn focused_border(self, cx: &App) -> Self;                   // 1px theme.ring border
    fn font_bold(self) -> Self;     // also: thin, light, normal, medium, semibold, extrabold, black
    fn debug_red(self) -> Self;     // also: blue, green, yellow, pink (no-op in release)
    fn debug_focused(self, f: &FocusHandle, w: &Window, cx: &App) -> Self;
}
```

Common: `div().h_flex().paddings(px(8.)).font_bold()`.

---

## Focus Handling

```rust
let focus_handle = window
    .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
    .read(cx).clone();
```

Store `tab_index: isize`, `tab_stop: bool` on struct, thread through in render:

```rust
.when(!self.disabled, |this| {
    this.track_focus(&focus_handle.tab_index(self.tab_index).tab_stop(self.tab_stop))
})
```

Focus ring (internal `FocusableExt`): `self.base.focus_ring(focus_handle.is_focused(window), px(0.), window, cx)`

---

## RenderOnce & FluentBuilder

**Core rules:** component defaults first → `.refine_style(&self.style)` → `.children(self.children)` last.

Import `use gpui::prelude::FluentBuilder as _;`.

```rust
impl RenderOnce for MyComponent {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.base
            .h_flex()
            .when(self.disabled, |this| this.opacity(0.5).cursor_not_allowed())
            .when_some(self.label.as_ref(), |this, label| {
                this.child(div().child(label.clone()))
            })
            .when_some(self.on_click, |this, on_click| {
                this.on_click(move |event, window, cx| { on_click(event, window, cx); })
            })
            .refine_style(&self.style)   // user overrides
            .children(self.children)
    }
}
```

Patterns: `.when(cond, |this| { … })` — bool gate; `.when_some(opt, |this, val| { … })` — Option gate; `.map(|this| { … })` — arbitrary transform.

### Theme Access

Import `use crate::ActiveTheme;`. Use `.bg(cx.theme().surface)`, `.text_color(cx.theme().foreground)`, `.border_color(cx.theme().border)`, `.rounded(cx.theme().radius)`. Full palette: `background`, `foreground`, `surface`, `muted`, `card`, `popover`, `primary`, `secondary`, `accent`, `border`, `input`, `ring`, `destructive`, `warning`, `success`, `info`, `chart1`–`chart5`.

---

## Action System

Declare with `gpui::actions!` macro or `#[derive(Action)]`:

```rust
actions!(ui, [Cancel, SelectUp, SelectDown, SelectFirst, SelectLast]);

#[derive(Clone, Action, PartialEq, Eq)]
#[action(namespace = ui, no_json)]
pub struct Confirm { pub secondary: bool }
```

Dispatch — Component (`cx.listener`), Model (`window.listener_for`), or ad-hoc closure:

```rust
self.base.on_action(cx.listener(Self::on_action_confirm))
self.base.on_action(window.listener_for(&self.state, MyState::on_enter))
self.base.on_action(move |_: &Cancel, window, cx| { … })
```

Handler: `fn handler(&mut self, _: &ActionType, window: &mut Window, cx: &mut Context<Self>)`.
Scoped with `key_context`; `cx.propagate()` bubbles unhandled actions up:

```rust
const CONTEXT: &str = "MyComponent";
self.base.key_context(CONTEXT).track_focus(&focus_handle)
    .on_action(cx.listener(Self::on_action_confirm));
```

---

## Callbacks

Always `Option<Rc<dyn Fn(…)>>` — components are cloned, `Rc` is cheap.

```rust
on_click:  Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>
on_click:  Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>       // Checkbox-style
on_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>
on_change: Option<Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>>
on_hover:  Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>
```

Builder setter, wired via `.when_some()` in render:

```rust
pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(handler)); self
}
```

Double-click: `use crate::event::InteractiveElementExt; div().on_double_click(|event, window, cx| { … })`

---

## Import Organization

Three blocks, blank line between. Trait imports use `as _`.

```rust
// 1. std
use std::rc::Rc;

// 2. crate
use crate::{
    ActiveTheme, Disableable, Selectable, Sizable, Size, StyledExt as _,
    h_flex, v_flex,
};

// 3. gpui
use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement,
    Stateful, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
    px, rems, relative,
};
```
Required `as _` per component: `StyledExt`, `StatefulInteractiveElement`, `FluentBuilder`, plus variant traits.

---

## Doc Comments

```rust
/// A Checkbox element.              ← struct: one-line, capital, period

impl Checkbox {
    /// Create a new Checkbox with the given id.   ← constructor
    pub fn new(id: impl Into<ElementId>) -> Self { ... }

    /// Set the label.                             ← setter: "Set the {field}."
    pub fn label(mut self, label: impl Into<Text>) -> Self { ... }

    /// Set the click handler.                     ← doc only non-obvious behavior
    ///
    /// The `&bool` parameter is the new checked state after the click.
    pub fn on_click(mut self, ...) -> Self { ... }
}
```
No redundant comments — document struct, constructor, setters, and non-obvious callback parameters.

---

## Checklist

For every new widget:
- [ ] `#[derive(IntoElement)]`
- [ ] Fields: `id: ElementId`, `base: Div` (or `Stateful<Div>`), `style: StyleRefinement`
- [ ] `impl RenderOnce` — `.refine_style(&self.style)` before `.children()`
- [ ] `impl Styled` returning `&mut self.style`
- [ ] `impl ParentElement` if accepts children
- [ ] `impl InteractiveElement` + `impl StatefulInteractiveElement` if interactive
- [ ] `impl Sizable` if size variants
- [ ] `impl Disableable` if can be disabled
- [ ] `impl Selectable` if can be selected
- [ ] Callbacks as `Option<Rc<dyn Fn(...)>>`, wired via `.when_some()`
- [ ] Doc comment on struct and all public methods
- [ ] Imports: `std` → `crate` → `gpui`, trait imports with `as _`
