# Dialog, AlertDialog, Sheet & Notification

Overlay components render **above** main content. They require `Root` + explicit layer-render calls.

## Table of Contents
- [Root Requirement](#root-requirement)
- [WindowExt Methods](#windowext-methods)
- [Dialog](#dialog)
- [AlertDialog](#alertdialog)
- [Sheet](#sheet)
- [Notification](#notification)

---

## Root Requirement

Three mandatory steps:

1. **`gpui_component::init(cx)`** — call exactly once in `app.run()`, before any window opens.
2. **`Root::new(view, window, cx)`** — every window's first view must be wrapped.
3. **Render overlay layers** in your `Render` impl:

```rust
impl Render for MyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full()
            .child(self.render_content(window, cx))
            .children(Root::render_notification_layer(window, cx))  // always renders
            .children(Root::render_sheet_layer(window, cx))         // Option
            .children(Root::render_dialog_layer(window, cx))        // Option
    }
}
```

Without these layers, dialogs/sheets/notifications will **not appear**.

---

## WindowExt Methods

Import: `use gpui_component::WindowExt;`. All operate on `&mut Window` + `&mut App`.

```rust
// Dialogs
window.open_dialog(cx, |dialog, window, cx| { /* build and return dialog */ });
window.open_alert_dialog(cx, |alert, window, cx| { /* build and return alert */ });
window.close_dialog(cx);
window.close_all_dialogs(cx);
window.has_active_dialog(cx)   // -> bool

// Sheets
window.open_sheet(cx, |sheet, window, cx| { /* default: Placement::Right */ });
window.open_sheet_at(Placement::Top, cx, |sheet, window, cx| { /* ... */ });
window.close_sheet(cx);
window.has_active_sheet(cx)    // -> bool

// Notifications
window.push_notification("Saved", cx);            // quick string
window.push_notification(Notification::success("Done!"), cx);  // typed
window.remove_notification::<MyKind>(cx);
window.clear_notifications(cx);
```

---

## Dialog

Centered modal with overlay backdrop. Multiple dialogs stack — each `open_dialog` pushes on top.

### Basic Pattern

```rust
window.open_dialog(cx, |dialog, window, cx| {
    dialog
        .title(Label::new("Confirm"))
        .content(|content, _, cx| content.child(Label::new("Proceed?")))
})
```

### Builder Methods

`Dialog::new(cx)` creates the builder. Key methods:

| Builder | Default | Description |
|---|---|---|
| `title(e)` | none | Any `IntoElement` |
| `content(f)` | none | `Fn(DialogContent, &mut Window, &mut App) -> DialogContent` |
| `footer(e)` | none | Custom footer — **ignores button_props** |
| `button_props(p)` | `default()` | Auto OK/Cancel buttons |
| `trigger(e)` | none | Clickable trigger; no `open_dialog()` needed |
| `w(px)` / `width(px)` | 448px | Dialog width |
| `max_w(px)` | None | Maximum width |
| `margin_top(px)` | 1/10 viewport | Top offset |
| `close_button(b)` | true | Show X button |
| `overlay(b)` | true | Show backdrop |
| `overlay_closable(b)` | true | Click backdrop to close |
| `keyboard(b)` | true | Esc to close |

### Button Props (Auto Footer)

When no `.footer()` is set, dialog auto-renders OK + optional Cancel:

```rust
dialog.button_props(
    DialogButtonProps::default()
        .ok_text("Delete")
        .ok_variant(ButtonVariant::Danger)
        .cancel_text("Keep")
        .show_cancel(true)
        .on_ok(|event, window, cx| { /* return true to close */ true })
)
```

| Builder | Default | Type |
|---|---|---|
| `ok_text(t)` | `"OK"` | `impl Into<SharedString>` |
| `ok_variant(v)` | `Primary` | `ButtonVariant` |
| `cancel_text(t)` | `"Cancel"` | `impl Into<SharedString>` |
| `cancel_variant(v)` | `Default` | `ButtonVariant` |
| `show_cancel(b)` | `false` | `bool` |
| `on_ok(fn)` | `\|_,_,_\| true` | `Fn(&ClickEvent, &mut Window, &mut App) -> bool` |
| `on_cancel(fn)` | `\|_,_,_\| true` | Same as on_ok |
| `on_close(fn)` | no-op | `Fn(&ClickEvent, &mut Window, &mut App)` |

**When `.footer()` is set, button_props are silently ignored.**

### Sub-Components (for .content() declarative layout)

`DialogHeader`, `DialogTitle` (`text_base + font_semibold`), `DialogDescription` (`text_sm + muted_foreground`), `DialogFooter` (`h_flex + justify_end + gap_2`), `DialogClose` (click → `CancelDialog`), `DialogAction` (click → `ConfirmDialog`).

```rust
Dialog::new(cx).content(|content, _, cx| {
    content
        .child(DialogHeader::new()
            .child(DialogTitle::new().child("Delete File"))
            .child(DialogDescription::new().child("Cannot be undone.")))
        .child(DialogFooter::new()
            .child(DialogClose::new().child(Button::new("cancel").label("Cancel")))
            .child(DialogAction::new().child(Button::new("ok").label("Delete"))))
})
```

### Declarative Trigger

```rust
Dialog::new(cx)
    .trigger(Button::new("open").label("Open"))
    .content(|content, _, cx| content.child(/* ... */))
```

No imperative `open_dialog()` needed. Keyboard: `ConfirmDialog` (Enter), `CancelDialog` (Escape) — auto-bound when `.keyboard(true)`.

---

## AlertDialog

Wraps `Dialog` with opinionated defaults:

| Property | Dialog | AlertDialog |
|---|---|---|
| `close_button` | true | **false** |
| `overlay_closable` | true | **false** |
| Footer alignment | right | center |
| Width | 448px | 420px |

### Builder

```rust
window.open_alert_dialog(cx, |alert, _, cx| {
    alert
        .warning()                           // also: .info(), .error(), .success()
        .title("Unsaved Changes")
        .description("Leave without saving?")
        .show_cancel(true)
        .on_ok(|event, window, cx| { true })
})
```

OK button is always present. Supports `.on_ok()`, `.on_cancel()`, `.on_close()`, `.overlay_closable()`, `.close_button()`, `.keyboard()`, `.icon()`, `.button_props()`.

---

## Sheet

Slide-in panel. Default: 350px, `Placement::Right`, resizable, with overlay. Builder: `.title()`, `.footer()`, `.size()`, `.resizable()`, `.overlay()`, `.overlay_closable()`, `.on_close()`.

```rust
window.open_sheet(cx, |sheet, window, cx| {
    sheet
        .title(Label::new("Inspect"))
        .size(px(400.))
        .resizable(true)
        .overlay_closable(true)
        .on_close(|_, window, cx| { /* cleanup */ })
        .child(v_flex().child(/* ... */))
});

window.open_sheet_at(Placement::Bottom, cx, |sheet, window, cx| {
    sheet.size(px(300.)).child(/* ... */)
});
```

---

## Notification

Toast notifications that auto-dismiss (default ~5s). Closeable via X button.

### Quick

```rust
window.push_notification("File saved", cx);
window.push_notification((NotificationType::Warning, "Disk low"), cx);
```

### Static Constructors

```rust
Notification::info("Update available")
Notification::success("Saved!")
Notification::warning("Low disk space")
Notification::error("Connection failed")
```

### Full Builder

```rust
let note = Notification::new()
    .id::<ExportTask>()                           // typed ID for remove_notification
    .message("Export complete")
    .with_type(NotificationType::Success)
    .autohide(true)                               // default: true
    .on_click(|event, window, cx| { /* ... */ })
    .on_close(|window, cx| { /* cleanup */ });

window.push_notification(note, cx);
window.remove_notification::<ExportTask>(cx);     // remove by type
window.clear_notifications(cx);                   // remove all
```

---

## Anti-Pattern: open_modal

**`open_modal()` does NOT exist.** The correct API is `window.open_dialog(cx, ...)`.

Other common mistakes:
- Rendering `Dialog::new(cx)` directly in `Render` — use `WindowExt` instead.
- Forgetting `Root::render_dialog_layer(window, cx)` — dialog won't appear.
- Using `.footer()` AND `.button_props()` — footer wins; button_props silently ignored.
