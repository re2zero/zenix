# Common Composition Patterns

Self-contained copy-paste starting points. Assumes `gpui_component::init(cx)` and `Root`.

**Contents:** [Dialog + Input](#dialog--input) · [Tabs + Toggle + Slider](#tabs--toggle--slider) ·
[Sidebar + List with Delegate](#sidebar--list-with-delegate) · [Critical Pitfalls](#critical-pitfalls)

---

## Dialog + Input

```rust
use gpui::{Entity, Render, Window, View, IntoElement, div, px, App, prelude::*};
use gpui_component::{Root, WindowExt, Input, Button, Label,
    Dialog, DialogButtonProps};
use gpui_component::input::InputState;

struct MyView { input: Entity<InputState> }

impl MyView {
    fn new(w: &mut Window, cx: &mut App) -> Self {
        Self { input: cx.new(|cx| InputState::new(w, cx).placeholder("Enter name…")) }
    }
}

impl Render for MyView {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inp = self.input.clone();
        div().size_full().child(
            Button::new("open").label("Open").on_click(move |_, w, cx| {
                let inp = inp.clone();
                w.open_dialog(cx, |d, w, cx| d
                    .title("Enter Name")
                    .content(|c, w, cx| c.child(Input::new(&inp.read(cx))))
                    .button_props(
                        DialogButtonProps::default().show_cancel(true)
                            .on_ok(move |_, w, cx| {
                                let val = inp.read(cx).value();
                                w.push_notification(
                                    format!("Name: {}", val.trim()), cx);
                                true
                            })
                    )
                );
            })
        )
        .child(Root::render_dialog_layer(w, cx))
        .child(Root::render_sheet_layer(w, cx))
        .child(Root::render_notification_layer(w, cx))
    }
}
```

`Entity<InputState>` lives in your view. `on_ok` closure captures a clone to read
the value. Dialogs open via `window.open_dialog()` — never render a Dialog inline.

---

## Tabs + Toggle + Slider

```rust
use gpui::prelude::*; use gpui::{Entity, Render, div, px, App};
use gpui_component::{Root, Slider, Toggle, Label};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::slider::SliderState;

struct SettingsView {
    tab: Entity<usize>,
    dark: bool,
    font: Entity<SliderState>,
}

impl SettingsView {
    fn new(w: &mut Window, cx: &mut App) -> Self {
        Self {
            tab: cx.new(|_| 0usize),
            dark: false,
            font: cx.new(|cx| SliderState::new(cx)
                .min(12.).max(24.).step(1.).default_value(14.)),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab.clone();
        let f = self.font.clone();
        let idx = *tab.read(cx);
        let dark = self.dark;
        div().size_full().p_4().gap_3().child(
            TabBar::new("tb").selected_index(idx)
                .child(Tab::new("gen").label("General"))
                .child(Tab::new("app").label("Appearance"))
                .on_click({ let t = tab.clone(); move |ix: &usize, _, cx| {
                    t.update(cx, |v, cx| { *v = *ix; cx.notify() }); }})
        ).when(idx == 0, |this| this.child(Label::new("General page")))
         .when(idx == 1, |this| this.child(div().gap_4()
            .child(div().gap_2().items_center().child(Label::new("Theme")).child(
                Toggle::new("theme").checked(dark)
                    .label(if dark { "Dark" } else { "Light" })
                    .on_click(move |checked: &bool, _, cx| { cx.notify(); })
            ))
            .child(div().gap_2().child(Label::new("Font Size"))
                .child(Slider::new(&f.read(cx)).w(px(200.))))
        ))
    }
}
```

Tab index is `Entity<usize>` so it survives rerenders; update it inside `on_click`.
`SliderState` is created via `cx.new(|cx| SliderState::new(cx))` with builder chain.

---

## Sidebar + List with Delegate

```rust
use gpui::{Entity, Render, Window, View, IntoElement, div, App, px, prelude::*};
use gpui_component::{Root, Scrollable, Label};
use gpui_component::sidebar::{Sidebar, SidebarHeader, SidebarMenu, SidebarMenuItem, Side};
use gpui_component::list::{List, ListState, ListDelegate, ListItem, IndexPath};

struct ShortcutDelegate { items: Vec<&'static str> }
impl ListDelegate for ShortcutDelegate {
    type Item = ListItem;
    fn items_count(&self, _: usize, _: &App) -> usize { self.items.len() }
    fn render_item(&mut self, ix: IndexPath, _: &mut Window, _: &mut Context<ListState<Self>>
    ) -> Option<Self::Item> {
        Some(ListItem::new(("item", ix.row)).child(*self.items.get(ix.row)?))
    }
    fn set_selected_index(&mut self, ix: Option<IndexPath>, _: &mut Window,
        _: &mut Context<ListState<Self>>) { /* handle selection */ }
    fn render_empty(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>
    ) -> impl IntoElement { Label::new("No items") }
}

struct AppView { list: Entity<ListState<ShortcutDelegate>> }
impl AppView {
    fn new(w: &mut Window, cx: &mut App) -> Self {
        let d = ShortcutDelegate { items: vec!["Cmd+S — Save","Cmd+F — Find","Cmd+P — Palette"] };
        Self { list: cx.new(|cx| ListState::new(d, w, cx)) }
    }
}
impl Render for AppView {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(
            Sidebar::new("sb").side(Side::Left).w(px(240.)).collapsible(true)
                .header(SidebarHeader::new().child("GPUI App"))
                .child(SidebarMenu::new()
                    .child(SidebarMenuItem::new("Dashboard"))
                    .child(SidebarMenuItem::new("Settings")))
                .footer(div().child("v1"))
        ).child(Scrollable::new().size_full().child(List::new(&self.list.read(cx)).small()))
    }
}
```

`ListDelegate` requires `items_count`, `render_item` (use `ix.row` to index data),
and `set_selected_index`. `ListItem` impls `Selectable`; give each a unique `("k", ix.row)` id.

---

## Critical Pitfalls

1. **`gpui_component::init(cx)` once**, before any window opens. Skip = `EntityNotFound` panic.
2. **Root wrapper**: `cx.new(|cx| Root::new(YourView::new(w, cx), w, cx))` for every window's first view.
3. **Overlay layers in Render** (after content): `Root::render_dialog_layer(w, cx)`,
   `Root::render_sheet_layer(w, cx)`, `Root::render_notification_layer(w, cx)`.
4. **Stateful components use `Entity<State>`** created via `cx.new(|cx| State::new(w, cx))`.
5. **Overlays via `WindowExt`, never inline**: `window.open_dialog(cx, ...)`,
   `window.open_sheet(cx, ...)`, `window.push_notification(...)`. No raw Dialog in view tree.
6. **Unique `ElementId`** per element: duplicate ids merge nodes → invisible/broken UI.
7. **`Tooltip::new(id)` requires `.id()` on the target**: element must call `.id(element_id.into())`.
8. **`Button` cursor is `cursor_default`, not `pointer`**: add `.cursor_pointer()` on custom clickable divs.
9. **`use gpui_component::WindowExt;`** is mandatory for `window.open_dialog()` to compile.
10. **Add `gpui-component-assets`** to Cargo.toml and pass `with_assets(Assets)` for bundled icons.
