# Quick Start

Bootstrap a GPUI application with `gpui-component` in under 5 minutes. Covers
the mandatory scaffold, the two component patterns, and shared primitives.
**Minimum:** `gpui-component` ≥ 0.5.x, `gpui` ≥ 0.2.x (Entity-based API).
---

## 1. Cargo.toml Dependencies

```toml
[dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = [
    "font-kit", "x11", "wayland", "runtime_shaders"
] }
gpui-component = "0.5"
gpui-component-assets = "0.5"   # optional — 100+ SVG icons
```

---

## 2. Full main.rs — Copy-Paste Scaffold

```rust
use gpui::{div, prelude::*, px, App, Context, Render, Window, WindowOptions};
use gpui_component::{h_flex, ActiveTheme as _, Button, Root, Sizable as _};
use gpui_component_assets::Assets;

struct MainView;
impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background).p_4()
            .child(h_flex().gap_2()
                .child(Button::new("btn").label("Click").primary()))
            .child(Root::render_notification_layer(_window, cx))
            .child(Root::render_sheet_layer(_window, cx))
            .child(Root::render_dialog_layer(_window, cx))
    }
}

fn main() {
    gpui_platform::application().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);   // ⚠️ first, before any window
        cx.activate(true);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Fixed(
                Bounds::centered(None, size(px(800.), px(600.)), cx))), ..Default::default() },
            |_window, cx| cx.new(|cx| Root::new(MainView, _window, cx)),
        ).unwrap();
    });
}
```

**Three non-negotiable rules:**
1. `gpui_component::init(cx)` — once, before any window.
2. `Root::new(your_view, window, cx)` — wraps every window's root view.
3. `Root::render_*_layer(…)` — all three overlay calls in every `render()`, last.

---

## 3. Component Patterns

### Stateless — use directly in `render()`, no stored Entity

```rust
use gpui_component::{Button, h_flex, Label, StyledExt as _};

fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    h_flex().gap_2()
        .child(Label::new("Status: OK"))
        .child(Button::new("save").label("Save").primary())
}
```
Button, Label, Icon, Badge, Divider, Separator — all stateless.

### Stateful — hold `Entity<State>` in your struct

```rust
use gpui::{Entity, Subscription};
use gpui_component::input::{InputEvent, InputState};

struct MyView {
    name_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
// Construction (inside cx.new for your view):
let name_input = cx.new(|cx| {
    InputState::new(window, cx).placeholder("Your name…").default_value("Alice")
});
let view = cx.new(|cx| {
    let inp = name_input.downgrade();
    let subs = vec![cx.subscribe_in(&name_input, window, move |_, _, e: &InputEvent, _, cx| {
        if matches!(e, InputEvent::Change) {
            let val = inp.upgrade().unwrap().read(cx).value(); // handle change…
        }
    })];
    MyView { name_input, _subscriptions: subs }
});
// Render:
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    gpui_component::Input::new(&self.name_input)
}
```
**Stateful components:** `Input`, `Select`, `Slider`, `Switch`, `Checkbox`,
`Combobox`, `DatePicker`, `List`, `Table`, `Tree` — all require `Entity<State>`.

---

## 4. Shared Concepts

### Sizes
```rust
use gpui_component::Sizable as _;

el.xsmall()   // extra-small
el.small()    // small
el.medium()   // default
el.large()    // large
```

### Theme Colors

```rust
use gpui_component::ActiveTheme as _;  // ← REQUIRED

cx.theme().primary            // brand color
cx.theme().background         // page background
cx.theme().foreground         // default text
cx.theme().border             // border color
cx.theme().muted              // skeleton / muted bg
cx.theme().muted_foreground   // disabled / secondary text
cx.theme().destructive        // danger color
cx.theme().radius             // default border-radius: px(6.)
```
Usage: `div().bg(cx.theme().background).text_color(cx.theme().foreground)`

### Layout Helpers

```rust
use gpui_component::{h_flex, v_flex};

h_flex().gap_2()   // flex row, items-center, 8px gap
v_flex().gap_4()   // flex column, 16px gap
## Next Steps

- **Need a dialog?** → `references/overlay.md`
- **Building a form?** → `references/input-form.md`
- **Want a sidebar + list?** → `references/navigation-layout.md`
- **Custom theming?** → `references/theming.md`
