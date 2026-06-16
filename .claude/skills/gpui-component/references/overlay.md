# Overlay Components

Overlay components render **above** the main window content. They require a `Root` view as the window root and explicit layer-render calls from your app's `Render`.

## Root is Mandatory

`Root` manages the lifecycle and stacking of sheets, dialogs, and notifications. Your top-level `Render` MUST include:

```rust
use gpui_component::Root;

// In your main view's Render:
Root::render_notification_layer(window, cx),
Root::render_sheet_layer(window, cx),
Root::render_dialog_layer(window, cx),
```

`Root::render_notification_layer` is called on every frame. The sheet and dialog layers only return `Some` when a sheet/dialog is active, so they can be placed last (they stack above notifications and each other).

---

## WindowExt Trait

Import via `use gpui_component::WindowExt;`. All methods operate on `&mut Window` and `&mut App`.

### Sheet Methods

```rust
/// Opens a Sheet sliding in from the right edge.
window.open_sheet(cx, |sheet, window, cx| { .. sheet });

/// Opens a Sheet at the given Placement.
window.open_sheet_at(Placement::Top, cx, |sheet, window, cx| { .. sheet });

/// Returns true if a Sheet is currently active.
window.has_active_sheet(cx);

/// Closes the active Sheet.
window.close_sheet(cx);
```

### Dialog Methods

```rust
/// Opens a Dialog (modal stack — new dialog pushes on top).
window.open_dialog(cx, |dialog, window, cx| { .. dialog });

/// Opens an AlertDialog (opinionated dialog wrapper).
window.open_alert_dialog(cx, |alert, window, cx| { .. alert });

/// Returns true if at least one dialog is active.
window.has_active_dialog(cx);

/// Closes the topmost dialog.
window.close_dialog(cx);

/// Closes ALL active dialogs at once.
window.close_all_dialogs(cx);
```

### Notification Methods

```rust
/// Pushes a notification into the notification stack.
/// Quick string form:
window.push_notification("File saved", cx);
/// Or with builder:
window.push_notification(Notification::success("Saved!"), cx);

/// Removes all notifications whose id type matches T (via `Notification::id::<T>()` or `id1<T>(key)`).
window.remove_notification::<MyKind>(cx);

/// Removes a single notification matching type T + element key (paired with `Notification::id1::<T>(key)`).
window.remove_notification1::<MyKind>("save-123", cx);

/// Clears all notifications.
window.clear_notifications(cx);

/// Returns the count of active notifications.
window.notifications(cx).len();
```

### Input & Selection Helpers

```rust
/// Returns the currently focused Input entity, if any.
window.focused_input(cx);
/// Returns true if any Input is focused.
window.has_focused_input(cx);
/// Returns the merged selected text across all selectable TextViews, joined with '\n'.
window.selected_text(cx);
/// Returns true if there is any active text selection.
window.has_text_selection(cx);
/// Clears all text selections.
window.clear_text_selection(cx);
```

---

## Dialog

`Dialog` is a modal that renders centered in the window with an overlay backdrop. Multiple dialogs stack — each new one opens above the previous.

### Constructor

```rust
let dialog = Dialog::new(cx);
// cx: &mut App — the focus handle is created here
```

### Builder Methods

#### Content

```rust
dialog.content(|content, window, cx| {
    content.child(Label::new("Hello"))
})
```

The closure receives a `DialogContent` (a `v_flex` container) that you add children to. Returns `DialogContent`.

#### Title

```rust
dialog.title(Label::new("My Title"))
// or any IntoElement
```

Sets a title rendered at the top of the dialog using a `DialogTitle` wrapper.

#### Footer (Custom)

```rust
dialog.footer(
    h_flex().gap_2().justify_end()
        .child(Button::new("cancel").label("Cancel"))
        .child(Button::new("ok").label("OK"))
)
```

**When a custom footer is set, `button_props` (auto OK/Cancel buttons) are completely ignored.** You must wire your own buttons to `window.close_dialog(cx)`.

#### Button Props (Auto Footer)

When no custom `footer()` is set, the dialog renders an auto footer with OK and optional Cancel buttons:

```rust
dialog.button_props(
    DialogButtonProps::default()
        .ok_text("Delete")
        .ok_variant(ButtonVariant::Danger)
        .cancel_text("Keep")
        .cancel_variant(ButtonVariant::Outline)
        .show_cancel(true)
)
```

`DialogButtonProps` fields and builder methods:

| Method | Default | Description |
|---|---|---|
| `ok_text(text)` | `"OK"` (i18n) | Text for the OK/confirm button |
| `ok_variant(variant)` | `Primary` | Button variant for OK |
| `cancel_text(text)` | `"Cancel"` (i18n) | Text for the Cancel button |
| `cancel_variant(variant)` | `Default` | Button variant for Cancel |
| `show_cancel(bool)` | `false` | Whether to show Cancel button |
| `on_ok(fn)` | closes dialog | Called when OK clicked. Return `true` to close, `false` to keep open |
| `on_cancel(fn)` | closes dialog | Called when Cancel clicked. Return `true` to close, `false` to keep open |
| `on_close(fn)` | no-op | Called AFTER the dialog closes (both OK and Cancel paths) |

#### Callbacks

```rust
dialog
    .on_ok(|event, window, cx| {
        // Do work, return true to close, false to stay open
        true
    })
    .on_cancel(|event, window, cx| {
        true // returning false prevents closing
    })
    .on_close(|event, window, cx| {
        // Cleanup after dialog is dismissed
    })
```

#### Visual Options

```rust
dialog
    .w(px(500.))             // Width, default: 448px
    .width(px(500.))          // Alias for w()
    .max_w(px(800.))          // Max width, default: None
    .margin_top(px(80.))     // Top offset from viewport; default: 1/10 viewport height
    .close_button(true)       // Show X close button, default: true
    .overlay(true)            // Show backdrop overlay, default: true
    .overlay_closable(true)   // Clicking overlay closes dialog, default: true
    .keyboard(true)           // Esc key dismisses dialog, default: true
```

#### Trigger Pattern (Declarative)

```rust
Dialog::new(cx)
    .trigger(Button::new("open").label("Open Dialog"))
    .content(|content, window, cx| {
        content.child(Label::new("Content here"))
    })
```

When a `trigger` is set, the dialog element renders as a clickable trigger that opens the dialog when clicked. This avoids imperative `window.open_dialog()` calls.

### Dialog Sub-Components

For declarative layout within `.content()`:

| Component | Purpose |
|---|---|
| `DialogContent::new()` | Container passed into `content()` closure |
| `DialogHeader::new()` | Header section (groups title + description) |
| `DialogTitle::new()` | Title text (`text_base`, `font_semibold`) |
| `DialogDescription::new()` | Description text (`text_sm`, `muted_foreground`) |
| `DialogFooter::new()` | Footer container (`h_flex`, `justify_end`, `gap_2`) |
| `DialogClose::new()` | Wrapper that dispatches `CancelDialog` action on click |
| `DialogAction::new()` | Wrapper that dispatches `ConfirmDialog` action on click |

Example:

```rust
Dialog::new(cx)
    .trigger(Button::new("delete").label("Delete"))
    .content(|content, _, cx| {
        content
            .child(
                DialogHeader::new()
                    .items_center()
                    .child(DialogTitle::new().child("Delete File"))
                    .child(DialogDescription::new().child("This cannot be undone."))
            )
            .child(
                DialogFooter::new()
                    .justify_center()
                    .child(DialogClose::new().child(Button::new("cancel").label("Cancel")))
                    .child(DialogAction::new().child(Button::new("confirm").label("Delete")))
            )
    })
```

### Actions (Keybindings)

| Action | Trigger |
|---|---|
| `ConfirmDialog` | Enter (or programmatic dispatch) |
| `CancelDialog` | Escape |

These are auto-bound when `.keyboard(true)` (the default).

---

## AlertDialog

`AlertDialog` wraps `Dialog` with opinionated defaults for interrupt/confirmation patterns.

### Defaults Compared to Dialog

| Property | Dialog | AlertDialog |
|---|---|---|
| `close_button` | `true` | `false` |
| `overlay_closable` | `true` | `false` |
| Footer alignment | right (`justify_end`) | center |
| Width | 448px | 420px |

### Constructor and Quick Variants

```rust
let alert = AlertDialog::new(cx);
```

On its own, `AlertDialog::new()` is a modal with just an OK button. Use the fluent builder:

```rust
alert
    .warning()    // Sets icon + variant (also: .info(), .error(), .success())
    .title("Unsaved Changes")
    .description("You have unsaved changes. Leave anyway?")
    .show_cancel(true)
```

Note: `.warning()`, `.info()`, `.error()`, `.success()` are methods on `AlertDialog` that set the icon to the appropriate variant icon. They are **not** static constructors — constructors are on `Notification`.

### Builder API

```rust
AlertDialog::new(cx)
    .icon(Icon::new(IconName::Trash))          // Custom icon
    .title("Delete Item")                      // Title element
    .description("This action is permanent.")   // Description element
    .confirm()                                  // Same as .show_cancel(true)
    .show_cancel(true)                          // Show Cancel button
    .button_props(
        DialogButtonProps::default()
            .ok_text("Delete")
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Keep")
    )
    .on_ok(|event, window, cx| { /* ... */ true })
    .on_cancel(|event, window, cx| { true })
    .on_close(|event, window, cx| { /* cleanup */ })
    .width(px(480.))                   // Override default width
    .keyboard(true)                    // Esc dismisses, default: true
    .overlay_closable(true)            // Override default (false → true)
    .close_button(true)                // Override default (false → true)
```

### Declarative (Trigger/Content) Pattern

```rust
AlertDialog::new(cx)
    .trigger(Button::new("delete").label("Delete"))
    .content(|content, _, cx| {
        content
            .child(
                DialogHeader::new()
                    .child(DialogTitle::new().child("Delete File"))
                    .child(DialogDescription::new().child("This cannot be undone."))
            )
            .child(
                DialogFooter::new()
                    .justify_center()
                    .child(Button::new("cancel").label("Cancel"))
                    .child(Button::new("confirm").label("Delete"))
            )
    })
```

**When using `.trigger()` + `.content()`**, the imperative `.title()`, `.description()`, `.icon()`, and `.button_props()` are ignored. You must build the entire dialog layout declaratively.

### Opening via WindowExt

```rust
window.open_alert_dialog(cx, |alert, _, _| {
    alert.warning()
        .title("Unsaved Changes")
        .description("Leave anyway?")
        .show_cancel(true)
})
```

---

## Sheet

`Sheet` is a slide-in panel from any window edge.

### Placement Enum

```rust
pub enum Placement {
    Top,
    Bottom,
    Left,
    Right,  // Default
}
```

### Opening

```rust
// Default: slides in from the right
window.open_sheet(cx, |sheet, window, cx| {
    sheet.title(Label::new("Settings"))
});

// Custom placement
window.open_sheet_at(Placement::Bottom, cx, |sheet, window, cx| {
    sheet.title(Label::new("Terminal"))
});

// Close from anywhere
window.close_sheet(cx);
```

### Builder API

```rust
Sheet::new(window, cx)     // Not typically called directly — use window.open_sheet()
    .title(element)         // Title bar content (IntoElement)
    .footer(element)        // Footer bar content (IntoElement)
    .size(px(400.))         // Width (Left/Right) or Height (Top/Bottom), default: 350px
    .resizable(true)        // Whether user can drag-resize, default: true
    .overlay(true)          // Show backdrop overlay, default: true
    .overlay_closable(true) // Clicking overlay closes sheet, default: true
    .on_close(|event, window, cx| {}) // Called when sheet is dismissed
```

The Sheet receives children via `ParentElement`:

```rust
window.open_sheet(cx, |sheet, _, _| {
    sheet
        .title("Settings")
        .child(Label::new("Content goes here"))
        .child(Checkbox::new("enabled"))
})
```

### Behavior

- Clicking the overlay (when `overlay_closable` is true) closes the sheet and fires `on_close`
- Pressing Escape closes the sheet and fires `on_close`
- The close button in the title bar always closes the sheet
- Opening a sheet pushes existing notifications inward by the sheet's size

---

## Notification

`Notification` renders as a toast-like card in a corner of the window.

### Quick Push (via WindowExt)

```rust
// Simple string — defaults to neutral/info appearance
window.push_notification("File saved", cx);

// Typed shorthand
window.push_notification(Notification::success("Operation complete"), cx);
window.push_notification(Notification::error("Failed to connect"), cx);
window.push_notification(Notification::warning("Low disk space"), cx);
window.push_notification(Notification::info("Syncing..."), cx);
```

### NotificationType Enum

```rust
pub enum NotificationType {
    Info,      // Default
    Success,
    Warning,
    Error,
}
```

Each variant auto-renders a themed icon:
- `Info` → `IconName::Info` with `theme.info` color
- `Success` → `IconName::CircleCheck` with `theme.success` color
- `Warning` → `IconName::TriangleAlert` with `theme.warning` color
- `Error` → `IconName::CircleX` with `theme.danger` color

### Full Builder API

```rust
Notification::new()
    .message("File uploaded successfully")   // Primary text body
    .title("Upload Complete")                // Optional bold title
    .with_type(NotificationType::Success)    // Icon + theming
    .icon(Icon::new(IconName::Cloud))        // Override the default icon
    .autohide(true)                          // Auto-dismiss after 5s, default: true
    .id::<MyNotificationKind>()              // Type-based unique ID (for dedup)
    .id1::<MyNotificationKind>("resume-42")  // Type + key unique ID
    .on_click(|event, window, cx| {          // Click to dismiss + callback
        // Notification auto-dismisses before this runs
    })
    .on_close(|window, cx| {                // Fires on ANY dismiss path
        // Cleanup
    })
    .action(|this, window, cx| {            // Add an action button
        // Setting an action disables autohide
        Button::new("undo").label("Undo")
    })
    .content(|this, window, cx| {           // Custom body content
        Label::new("Custom").into_any_element()
    })
```

### ID System and Deduplication

Notifications with the same `NotificationId` **replace** previous instances rather than stacking.

```rust
// Removes any existing notification registered with this TypeId
let note = Notification::new().message("Syncing...").id::<SyncProgress>();

// Removes existing notification registered with TypeId + specific key
let note = Notification::new().message("File 42 saved").id1::<FileSave>("42");
```

Corresponding removal methods on `WindowExt`:

```rust
// Remove ALL notifications of type T (any key)
window.remove_notification::<SyncProgress>(cx);

// Remove ONLY the notification with type T and specific key
window.remove_notification1::<FileSave>("42", cx);
```

### Lifecycle & Dismissal

1. **Auto-dismiss**: After 5 seconds, if `autohide` is `true` AND no `action()` is set
2. **Close button**: Hover to reveal the X button
3. **Middle-click**: Click with middle mouse button anywhere on the notification
4. **Click handler**: If `on_click` is set, clicking dismisses the notification first, then calls your callback
5. **Programmatic**: `dismiss()` on the `Notification` entity triggers the dismiss animation, then emits `DismissEvent` which removes it from the list

Setting an `action()` **disables auto-dismiss** — the notification stays until manually dismissed.

### NotificationSettings

```rust
pub struct NotificationSettings {
    pub placement: Anchor,           // Default: Anchor::TopRight
    pub margins: Edges<Pixels>,      // Default: 16px, top = TITLE_BAR_HEIGHT + 16px
    pub max_items: usize,            // Default: 10
}
```

Access via `cx.theme().notification.placement` (etc). Configure by modifying the theme.

### NotificationList

`NotificationList` is the internal view that manages the notification queue. You do not create it directly — it is owned by `Root` and rendered by `Root::render_notification_layer()`. Key behaviors:

- Notifications animate in (slide + fade) and out (slide away + fade)
- Maximum 10 visible at once (oldest trimmed)
- Hovering the notification area expands to show all (even if previously collapsed)
- Placement respects the sheet position — notifications slide to accommodate an open sheet

---

## Popover

`Popover` is a generic anchored floating panel. It is the foundation for dropdown menus, context menus, and custom tooltips.

### Constructor & Builder

```rust
Popover::new("my-popover")
    .trigger(Button::new("open").label("Open"))   // Clickable trigger element
    .anchor(Anchor::TopLeft)                       // Where popover appears relative to trigger
    .mouse_button(MouseButton::Left)               // Which button activates, default: Left
    .content(|state, window, cx| {                 // Content builder
        // state is &mut PopoverState
        Label::new("Hello from popover")
    })
    .default_open(true)                            // Start open, default: false
    .open(is_open)                                 // Controlled open state (requires on_open_change)
    .on_open_change(|new_state, window, cx| {})    // Called when open state changes
    .appearance(true)                              // Style with bg/border/shadow/padding, default: true
    .overlay_closable(true)                        // Click outside dismisses, default: true
    .track_focus(&some_focus_handle)              // Focus this handle when popover opens
    .trigger_style(style)                          // Extra style for the trigger element
```

### PopoverState

Available inside the `content()` closure:

```rust
pub struct PopoverState {
    // Public methods:
    is_open(&self) -> bool
    dismiss(&mut self, window, cx)
    show(&mut self, window, cx)
    // Also implements Focusable
}
```

`PopoverState` manages open/close with focus tracking — it saves the previously focused element on open and restores focus on close.

---

## Menus

### DropdownMenu

A trait implemented on `Button` that creates a dropdown popover:

```rust
use gpui_component::menu::DropdownMenu;

Button::new("options").label("Options")
    .dropdown_menu(|menu, window, cx| {
        menu.menu("Copy", Box::new(Copy))
            .menu("Paste", Box::new(Paste))
            .separator()
            .menu("Delete", Box::new(Delete))
    })
```

Also available with custom anchor:

```rust
button.dropdown_menu_with_anchor(Anchor::TopRight, |menu, window, cx| { ... })
```

### ContextMenu

Attached to any `InteractiveElement + ParentElement + Styled` via the `ContextMenuExt` trait:

```rust
use gpui_component::menu::ContextMenuExt;

div()
    .context_menu(|menu, window, cx| {
        menu.menu("Copy", Box::new(Copy))
            .separator()
            .menu("Paste", Box::new(Paste))
    })
```

The context menu opens on right-click at the cursor position. It wraps the element in a `relative` container and attaches an `absolute`-positioned `ContextMenu` child.

### PopupMenu

The menu content builder (used by both `DropdownMenu` and `ContextMenu`):

```rust
let entity = PopupMenu::build(window, cx, |menu, window, cx| {
    menu
        .menu("New File", Box::new(NewFile))
        .menu_with_icon("Save", IconName::Save, Box::new(Save))
        .menu_with_check("Auto Save", true, Box::new(ToggleAutoSave))
        .menu_with_disabled("Delete", Box::new(Delete), true)  // grayed out
        .separator()                                             // horizontal divider
        .label("Recent Files")                                   // non-interactive label
        .link("Documentation", "https://docs.example.com")      // external link
        .submenu("More", window, cx, |submenu, window, cx| {
            submenu.menu("Option A", Box::new(OptA))
        })
        .menu_element(Box::new(CustomAction), |window, cx| {
            // Custom element render
            h_flex().child(Label::new("Custom")).child(Icon::new(IconName::Star))
        })
        .action_context(focus_handle)         // Focus to restore on dismiss
        .min_w(px(200.))
        .max_w(px(400.))
        .max_h(px(300.))
        .scrollable(true)                     // Enable scrollbar (disables submenus)
        .check_side(Side::Right)              // Check icon placement, default: Left
        .external_link_icon(false)            // Hide external link icon, default: true
})
```

**Key menu item methods** (all return `Self` for chaining):
| Method | Description |
|---|---|
| `menu(label, action)` | Standard menu item |
| `menu_with_icon(label, icon, action)` | Item with icon |
| `menu_with_check(label, checked, action)` | Item with checkmark |
| `menu_with_disabled(label, action, disabled)` | Item that can be grayed out |
| `menu_with_enable(label, action, enable)` | Inverse of disabled |
| `link(label, href)` | External link item |
| `label(text)` | Non-interactive text |
| `separator()` | Horizontal divider (no-op if first or already preceded by separator) |
| `submenu(label, window, cx, builder)` | Nested submenu |
| `menu_element(action, builder)` | Custom-rendered item |
| `menu_element_with_check(checked, action, builder)` | Custom-rendered with check |
| `menu_element_with_icon(icon, action, builder)` | Custom-rendered with icon |
| `menu_element_with_disabled(action, disabled, builder)` | Custom-rendered with disabled |

### AppMenuBar

For native menu bar integration:

```rust
use gpui_component::menu::AppMenuBar;

// Registered at init; provides macOS-style menu bar
// See app_menu_bar.rs for full API
```
