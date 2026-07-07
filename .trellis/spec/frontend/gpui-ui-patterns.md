# GPUI UI Patterns — zenix chat panel

Code-spec for GPUI rendering, overlay, and interaction patterns learned from the zenixai-redesign-v4 work. These are executable contracts — violating them produces silent UI breakage (events not firing, layout collapse, content clipping).

---

## Pattern: `.occlude()` on absolute interactive elements

**Problem**: An absolutely-positioned element (`div().absolute()...`) used as a click/drag target — resize handle, popover backdrop, overlay button — appears in the layout and the cursor changes on hover, but `on_drag`, `on_click`, `on_mouse_down` never fire.

**Cause**: By default, gpui absolute elements do not claim hit-test priority. Mouse events pass through to the flex-flow siblings underneath. The cursor change is pure CSS hover styling; the interaction never reaches your listener.

**Solution**: Chain `.occlude()` on any Stateful element that is `absolute()` AND must receive pointer events.

```rust
// Correct — resize handle intercepts drag
div()
    .id("ai-panel-resize-handle")
    .occlude()           // <-- required for on_drag to fire
    .absolute()
    .left(px(0.))
    .h_full()
    .w(px(6.))
    .cursor_col_resize()
    .on_drag(value, |_, _, _, cx| cx.new(|_| DragPreview))
```

**Wrong**:
```rust
div()
    .id("handle")
    .absolute()
    .on_drag(...)   // never fires; events pass through to messages list below
```

**Where**: `src/ai/chat/resize.rs` (both handles), `src/ai/chat/header.rs` (menu backdrop). Any future overlay that must catch clicks.

---

## Pattern: `.deferred()` for overlays that must not consume flex space

**Problem**: A dropdown / popover / menu rendered as a panel child with `.absolute()` visually appears, but its presence shifts the flex layout — input bar gets "pushed up", content area shrinks — as if the absolute element were participating in the flex column.

**Cause**: gpui's absolute elements are styled but still attached as children of the flex container. The flex engine reserves space for them along the main axis.

**Solution**: Wrap the overlay element in `gpui::deferred(...)`. Deferred elements render into a top-of-window overlay layer that is fully outside any flex flow.

```rust
// Correct — dropdown floats above without disturbing layout
panel = panel.child(gpui::deferred(
    input::render_model_dropdown(data, theme, on_action.clone()),
));

// drawer, menu, any other absolute overlay must be wrapped the same way
if data.header_menu_open {
    panel = panel.child(gpui::deferred(header::render_header_menu(...)));
}
```

The inner element still uses `.absolute()` with `bottom`/`top`/`left`/`right` to position itself relative to the panel (which is `.relative()`).

**Wrong**:
```rust
panel = panel.child(render_dropdown(...));   // pushes input bar upward
```

**Where**: `src/ai/chat/mod.rs` (dropdown/menu/drawer rendering).

**See also**: `gpui-component::Popover` internally uses `deferred` + `anchored`. Prefer the library `Popover`/`Select` widgets over hand-rolled overlays when the content fits them — they handle occlude, dismiss-on-outside-click, and focus tracking for free.

---

## Pattern: `.min_w_0()` on flex children that hold long text

**Problem**: A bubble / card / list-row inside a flex container overflows its parent when it holds long unbroken content (URLs, code blocks, long tokens). The right edge of the text is clipped or pushes the bubble wider than the panel.

**Cause**: Flex items default to `min-width: auto`, meaning they refuse to shrink below their content's intrinsic width. Long content forces the item wider than the available track.

**Solution**: Add `.min_w_0()` (and usually `.overflow_hidden()`) on the flex item that holds text content.

```rust
// Correct — bubble shrinks and its content wraps
div()
    .w_full()
    .min_w_0()
    .overflow_hidden()
    .rounded_lg()
    .bg(muted)
    .child(body),
```

**Wrong**:
```rust
div()
    .w_full()
    .bg(muted)
    .child(long_body),   // body's intrinsic width forces bubble to overflow
```

**Where**: `src/ai/chat/messages.rs` (user + assistant bubbles).

---

## Pattern: `on_drag_move` listener belongs on the drag handle's direct parent

**Problem**: A drag handle starts a drag (`on_drag` fires, drag preview appears), but the resize never takes effect — the `on_drag_move` listener on a far ancestor (e.g. the top-level `main_ui`) never receives the event.

**Cause**: gpui's `DragMoveEvent<T>` dispatch is sensitive to listener proximity. Listeners registered on elements far up the tree may not receive the event depending on hit-testing during the drag.

**Solution**: Put `on_drag_move` and `on_drop` on the handle's direct parent — the panel/container that owns the handle, not the root window layout.

```rust
// The panel owns both the handle and the drag listeners
let mut panel = div()
    .id("ai-chat-panel")
    .relative()
    ...
    .on_drag_move(move |ev: &DragMoveEvent<PanelResizeDrag>, window, cx| {
        let drag = ev.drag(cx);
        let new_width = compute_resized_width(drag, f32::from(ev.event.position.x));
        on_action(AiChatAction::ResizePanel(new_width), window, cx);
    })
    .on_drop(move |_drag: &PanelResizeDrag, window, cx| {
        on_action(AiChatAction::PersistPanelWidth, window, cx);
    });

// later, as a child of the same panel:
panel = panel.child(resize::render_resize_handle(width, theme));
```

**Wrong**:
```rust
// main_ui (far ancestor) — drag listener may never fire
main_ui.on_drag_move(...).child(panel_with_handle);
```

**Where**: `src/ai/chat/mod.rs` (panel construction), `src/app.rs` (legacy main_ui listeners retained but the panel-level ones are authoritative).

---

## Pattern: prefer gpui-component library widgets over hand-rolled equivalents

**zenix pins `gpui-component`** which ships battle-tested widgets. Before building a custom overlay, check:

| Need | Library widget | Location |
|------|----------------|----------|
| Popover / floating panel | `Popover` | `gpui_component::popover` |
| Dropdown / picker | `Select` (with `SearchableVec<String>` delegate) | `gpui_component::select` |
| Drag handle for resizable panel | `resize_handle` (pub(crate) — pattern in `resizable/resize_handle.rs`) | internal only, copy the pattern |
| Button (with Selectable for Popover triggers) | `Button` | `gpui_component::button` |
| Multi-line text input | `Input` with `.multi_line(true).auto_grow(min, max)` | `gpui_component::input` |
| List with follow/streaming | `gpui::list` + `FollowMode::Tail` + `ListAlignment::Bottom` | `gpui::list` |

**Why**: library widgets already handle occlude, deferred rendering, focus, keyboard dismiss, and theme tokens. Hand-rolled versions miss these and regress one interaction at a time.

**Convention**: only hand-roll when the library widget is genuinely missing or forces a worse UX. Document the reason in a comment on the custom widget.

---

## Convention: bubble ↔ panel border gap is controlled by list/container padding

**What**: the gap between message bubbles and the panel edge is set by `msg_area.px(N)`, not by per-bubble margins. Both user and assistant bubbles share the same outer padding so they align.

**Why**: per-bubble margins drift apart as new bubble variants are added; a single container padding stays consistent and is trivial to tune.

**Where**: `src/ai/chat/mod.rs` (`msg_area` construction), `src/ai/chat/messages.rs` (bubbles must not add their own horizontal margins).

```rust
let mut msg_area = v_flex()
    .flex_grow(1.0)
    .min_h(px(200.))
    .px(px(4.));   // gap between bubbles and panel border

// inside the list, items must not re-add horizontal padding
gpui::list(...).px(px(0.))
```

---

## Common Mistake: toggling a dropdown via a `Toggle*Dropdown` action and forgetting to clear `config_dropdown` in the select handler

**Symptom**: user picks an item from a dropdown; the dropdown stays open, or the chosen value doesn't "commit".

**Cause**: the `SelectX(...)` handler updates the underlying field but never resets `data.config_dropdown = None`.

**Fix**: every select-style action must (1) write the field, (2) clear `config_dropdown`, (3) `cx.notify()`.

```rust
AiChatAction::SelectModel(model) => {
    if let Some(ref mut data) = this.ai_chat_panel {
        data.selected_model = model;
        data.config_dropdown = None;   // <-- required to dismiss
    }
    cx.notify();
}
```

**Where**: `src/app.rs` `handle_ai_chat_action` — `SelectModel`, `SelectProvider`, `SelectThinkingLevel`.
