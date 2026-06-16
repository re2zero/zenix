# Display & Feedback Components

**Contents:** [Button](#button) · [Icon](#icon) · [Badge](#badge) · [Tag](#tag) · [Alert](#alert) · [Loading Indicators](#loading-indicators) · [Tooltip](#tooltip) · [HoverCard](#hovercard) · [Label](#label) · [Kbd](#kbd) · [Link](#link) · [Clipboard](#clipboard) · [Image](#image) · [Avatar](#avatar) · [Separator](#separator)

---

## Button

**Import:** `use gpui_component_ui::button::{Button, ButtonVariant, ButtonVariants, ButtonGroup, DropdownButton, Toggle, ToggleGroup};`

`Button` is a stateless `RenderOnce` element implementing `IntoElement`, `Styled`, `Sizable`, `Disableable`, `Selectable`, `ButtonVariants`, `ParentElement`, and `InteractiveElement`.

### Creating Buttons

```rust
use gpui_component_ui::button::{Button, ButtonVariant};

// Bare minimum
Button::new("my-btn").label("Click me");

// Icon-only button
Button::new("icon-btn").icon(IconName::Plus);

// Icon + label
Button::new("save").icon(IconName::Check).label("Save");
```

### Variants

Set via the `ButtonVariants` trait (returning `Self`):

| Method               | `ButtonVariant` value  | Description                                     |
|----------------------|------------------------|-------------------------------------------------|
| (default)            | `ButtonVariant::Default`| Bordered outline style, default shadow          |
| `.primary()`         | `Primary`              | Filled primary accent, high-emphasis action     |
| `.secondary()`       | `Secondary`            | Subtle filled background                        |
| `.danger()`          | `Danger`               | Red destructive-action indicator                |
| `.warning()`         | `Warning`              | Amber warning indicator                         |
| `.success()`         | `Success`              | Green affirmative indicator                     |
| `.info()`            | `Info`                 | Blue informational styling                      |
| `.ghost()`           | `Ghost`                | Transparent background, no border               |
| `.link()`            | `Link`                 | Text-only with underline on hover               |
| `.text()`            | `Text`                 | Text with no padding, normal-looking text       |
| `.custom(style)`     | `Custom(...)`          | Arbitrary colors via `ButtonCustomVariant`      |

```rust
// Typical usage
Button::new("delete").danger().label("Delete");
Button::new("approve").primary().label("Approve");
Button::new("cancel").ghost().label("Cancel");
```

#### Custom variant

```rust
let custom = ButtonCustomVariant::new(cx)
    .color(cx.theme().purple)
    .foreground(white())
    .hover(cx.theme().purple.opacity(0.8))
    .active(cx.theme().purple.opacity(0.6))
    .shadow(true);

Button::new("custom").custom(custom).label("Custom");
```

### States

| Trait / Method          | Description                                  |
|-------------------------|----------------------------------------------|
| `Disableable::disabled(self, bool)` | Greyed-out, non-interactive          |
| `.loading(self, bool)`  | Shows spinner icon, text dimmed, clicks ignored |
| `Selectable::selected(self, bool)`  | Toggle-selected appearance              |

```rust
Button::new("submit")
    .primary()
    .label("Submit")
    .disabled(true);

Button::new("save")
    .label("Saving...")
    .loading(true);  // replaces icon with spinner, dims button

Button::new("option")
    .selected(true)
    .label("Option A");
```

### Sizes

`Button` implements `Sizable` — use `.xsmall()`, `.small()`, `.medium()` (default), `.large()`, or `.with_size(Size::Small)`, etc.

```rust
Button::new("big").large().label("Large");
Button::new("small").small().label("Small");
Button::new("xs").xsmall().label("XS");
```

For icon-only buttons (no label, no children), sizes map to fixed pixel dimensions; for labeled buttons, sizes control height and padding.

### Icon

Set an icon via `.icon()`. Accepts anything that implements `Into<ButtonIcon>`, which includes `Icon`, `IconName`, `Spinner`, and `ProgressCircle`:

```rust
Button::new("add").icon(IconName::Plus).label("Add");
Button::new("icon-only").icon(Icon::new(IconName::Search).small());

// Custom loading icon
Button::new("busy")
    .label("Processing")
    .loading(true)
    .loading_icon(Icon::new(IconName::Loader));
```

Icons inside buttons are automatically sized: icon size = button size × 0.75 for text buttons, and fixed px for icon-only buttons.

### `on_click` Signature

```rust
Button::new("action").on_click(|event: &ClickEvent, window: &mut Window, cx: &mut App| {
    // event contains mouse button, click count, modifiers
    println!("clicked at {:?}", event.position);
});
```

The `ClickEvent` provides:
- `position: Point<Pixels>` — click position
- `button: MouseButton` — Left/Right/Middle
- `click_count: usize` — for double-click detection
- `modifiers: Modifiers` — ctrl, shift, alt, etc.

### Other Methods

| Method | Description |
|--------|-------------|
| `.outline()` | Force outline style (border visible even for filled variants) |
| `.rounded(ButtonRounded::Small)` | Set corner radius; variants: `None`, `Small`, `Medium` (default), `Large`, `Size(Pixels)` |
| `.compact()` | Reduced padding for dense layouts |
| `.tooltip("tooltip text")` | Attach a managed tooltip shown on hover |
| `.tooltip_with_action("text", &action, Some("context"))` | Tooltip with keyboard shortcut auto-displayed |
| `.on_hover(|hovered: &bool, window, cx| {})` | Hover state callback |
| `.dropdown_caret(true)` | Show a chevron-down icon at the end of the button |
| `.tab_index(index)` | Set tab order for focus (default 0) |
| `.tab_stop(bool)` | Whether the button participates in tab focus (default true) |
| `.loading_icon(icon)` | Custom icon shown during loading state (default: spinner) |

### ButtonGroup

`ButtonGroup` wraps multiple `Button` children with shared styling and coordinated selection.

```rust
ButtonGroup::new("size-group")
    .child(Button::new("large").label("Large").selected(true))
    .child(Button::new("medium").label("Medium"))
    .child(Button::new("small").label("Small"))
    .multiple(false)  // single selection (default)
    .on_click(|indices: &Vec<usize>, _window: &mut Window, _cx: &mut App| {
        // indices contains the currently selected button positions
        if indices.contains(&0) { /* large selected */ }
    });
```

Key methods:
- `.child(button)` — add a single `Button` child
- `.children(iter)` — add multiple children
- `.multiple(bool)` — allow multi-select (default: `false` = single)
- `.on_click(|indices: &Vec<usize>, window, cx|)` — selection change callback; `indices` carries the positions of all selected buttons
- `.layout(Axis::Vertical)` — horizontal (default) or vertical layout
- `.compact()`, `.outline()` — forwarded to child buttons
- All `ButtonVariants` and `Sizable` methods apply to children

### DropdownButton

A split button: left side is an action button, right side opens a dropdown menu via a chevron.

```rust
DropdownButton::new("export")
    .button(Button::new("export-action").label("Export"))
    .dropdown_menu(|menu: PopupMenu, _window, _cx| {
        menu.menu("export-csv", "CSV")
            .menu("export-pdf", "PDF")
    })
    .primary()
    .outline()
    .tooltip("Export file");
```

Key methods:
- `.button(Button)` — set the left action button
- `.dropdown_menu(|menu: PopupMenu, win, cx| -> PopupMenu)` — set the dropdown menu via popup menu builder
- `.dropdown_menu_with_anchor(anchor, |menu, win, cx| -> PopupMenu)` — with custom anchor corner (default `Anchor::TopRight`)
- `.rounded(ButtonRounded)`, `.compact()`, `.outline()`, `.loading(bool)` — forwarded to children
- All `ButtonVariants`, `Sizable`, `Disableable`, `Selectable` traits apply

### Toggle

A pressed-state toggle button. Call `on_click` receives `&bool` — the new state.

```rust
Toggle::new("bold")
    .label("Bold")
    .checked(true)          // initial state
    .icon(IconName::Check)  // optional icon before label
    .on_click(|checked: &bool, _window, _cx| {
        println!("toggled: {}", checked);
    })
    .tooltip("Toggle bold (Ctrl+B)");
```

`ToggleVariant` enum:
- `ToggleVariant::Ghost` (default) — transparent, background only when pressed
- `ToggleVariant::Outline` — bordered style

#### ToggleGroup

Groups multiple `Toggle` components with coordinated selection:

```rust
ToggleGroup::new("toolbar")
    .child(Toggle::new("bold").label("B"))
    .child(Toggle::new("italic").label("I"))
    .child(Toggle::new("underline").label("U"))
    .on_click(|states: &Vec<bool>, _window, _cx| {
        // states[i] is the checked state of the i-th toggle
    })
    .segmented();  // render as connected segmented control
```

---

## Icon

**Import:** `use gpui_component_ui::{Icon, IconName, IconNamed};`

`Icon` is a stateless `RenderOnce` element wrapping an SVG, implementing `Styled` and `Sizable`.

### Basic Usage

```rust
// From IconName enum
Icon::new(IconName::Check);

// With custom path
Icon::default().path("icons/custom-icon.svg");

// IconName can be used directly in child position
div().child(IconName::Plus);
```

### Sizes

| Method          | Size variant | Pixel output       |
|-----------------|-------------|---------------------|
| `.xsmall()`     | `XSmall`    | `size_3()`  (12px)  |
| `.small()`      | `Small`     | `size_3p5()` (14px) |
| `.medium()`     | `Medium`*   | `size_4()`  (16px)  |
| `.large()`      | `Large`     | `size_6()`  (24px)  |
| `.with_size(Size::Size(px(20)))` | Custom pixel | exact px    |

\*Default: no explicit size → uses `window.text_style().font_size` (follows parent text size).

### Color

```rust
Icon::new(IconName::Check).text_color(cx.theme().danger);
```

### Rotation

```rust
Icon::new(IconName::ChevronDown).rotate(90.0_f32.to_radians());
```

### Transformation

```rust
Icon::new(IconName::ArrowRight).transform(Transformation::rotate(180.0_f32.to_radians()));
```

### `IconName` Enum

Generated by `icon_named!` from SVG files in the assets crate. Standard icons include:

| Variant            | Description              |
|--------------------|--------------------------|
| `Check`            | Checkmark                |
| `Plus`             | Plus sign                |
| `X`, `Close`       | Close/cross              |
| `ChevronDown`      | Downward chevron         |
| `ChevronLeft`      | Left chevron             |
| `ChevronRight`     | Right chevron            |
| `ChevronUp`        | Upward chevron           |
| `ChevronsUpDown`   | Vertical sort chevrons   |
| `Search`           | Magnifying glass         |
| `Trash`            | — (use `Delete`)         |
| `Delete`           | Trash can icon           |
| `Copy`             | Clipboard copy           |
| `Settings`         | Gear icon                |
| `Loader`           | Spinning loader          |
| `LoaderCircle`     | Circular loader          |
| `CircleCheck`      | Check in circle          |
| `CircleX`          | X in circle              |
| `TriangleAlert`    | Warning triangle         |
| `Info`             | Info circle              |
| `Star`, `StarFill`, `StarOff` | Rating stars   |
| `Eye`, `EyeOff`    | Visibility toggle        |
| `Bell`             | Notification bell        |
| `Calendar`         | Calendar icon            |
| `File`             | Document                 |
| `Folder`, `FolderOpen`, `FolderClosed` | Folder states |
| `Sun`, `Moon`      | Theme toggle             |
| `Palette`          | Color palette            |
| `Play`, `Pause`    | Media controls           |
| `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight` | Directional arrows |
| `ExternalLink`     | External link indicator  |
| `Ellipsis`, `EllipsisVertical` | Overflow menu   |
| `User`, `CircleUser` | User/profile           |
| `Bot`              | Bot/AI icon              |
| `GitHub`           | GitHub mark              |
| `Menu`             | Hamburger menu           |
| `Minus`, `Dash`    | Minus/dash               |
| `Undo`, `Redo`     | History navigation       |
| `BookOpen`         | Documentation            |
| `Globe`            | Language/internet        |
| `Heart`, `HeartOff`| Like toggle              |
| `ThumbsUp`, `ThumbsDown` | Feedback          |
| `Cpu`              | Processor                |
| `HardDrive`        | Disk                     |
| `Network`          | Network/connection       |
| `Battery`, `BatteryFull`, `BatteryMedium`, `BatteryLow`, `BatteryWarning`, `BatteryCharging` | Power |
| `Inbox`            | Inbox/mail               |
| `Frame`            | Aspect ratio / frame     |
| `ChartPie`         | Pie chart                |
| `Map`              | Map icon                 |
| `Replace`          | Find and replace         |
| `CaseSensitive`    | Case-sensitive toggle    |
| `SortAscending`, `SortDescending` | Sort order     |
| `SquareTerminal`   | Terminal                 |
| `ALargeSmall`      | Text size                |
| `Asterisk`         | Asterisk / required      |
| `Building2`        | Organization/building    |
| `MemoryStick`      | Memory/storage media     |
| `ResizeCorner`     | Resize gripper           |
| `Inspector`        | DevTools inspector       |
| `LayoutDashboard`  | Dashboard layout         |
| `PanelLeft`, `PanelRight`, `PanelBottom` | Sidebar toggles |
| `PanelLeftOpen`, `PanelRightOpen`, `PanelBottomOpen` | Open states |
| `PanelLeftClose`, `PanelRightClose` | Close states           |
| `WindowMinimize`, `WindowMaximize`, `WindowClose`, `WindowRestore` | Window controls |
| `GalleryVerticalEnd`| Gallery layout          |

All filenames in the assets directory are converted from kebab-case to PascalCase for `IconName` variants (e.g., `circle-check.svg` → `IconName::CircleCheck`).

### `IconNamed` Trait

```rust
pub trait IconNamed {
    fn path(self) -> SharedString;
}
```

Implement this to create custom icon sets that work like `IconName` everywhere:

```rust
enum MyIcons { Home, About }
impl IconNamed for MyIcons {
    fn path(self) -> SharedString {
        match self {
            MyIcons::Home => "custom/home.svg".into(),
            MyIcons::About => "custom/about.svg".into(),
        }
    }
}
// Then: Icon::new(MyIcons::Home)
```

Use the `icon_named!` macro from `gpui_component_macros` to auto-generate an enum from a directory of SVGs:

```rust
icon_named!(MyIconName, "$MY_ICONS_DIR");
```

### `Icon::view()` and `IconName::view()`

For stateful rendering (e.g., animated icons in a component that holds state):

```rust
let icon_entity: Entity<Icon> = Icon::new(IconName::Loader).view(cx);
// or
let icon_entity: Entity<Icon> = IconName::Loader.view(cx);
```

`IconName` also implements `RenderOnce`, `From<IconName> for AnyElement`, and `From<T: IconNamed> for Icon` — so `IconName::Check` can be used directly anywhere an `Icon` or `AnyElement` is accepted.

---

## Badge

**Import:** `use gpui_component_ui::badge::Badge;`

A small indicator overlay — a dot, count, or icon — typically attached to a parent element via `child()`.

### Variants

```rust
// Numeric badge (hidden when count == 0)
Badge::new().count(5);

// With max threshold (shows "99+" when count > max)
Badge::new().count(150).max(99);

// Dot badge (simple presence indicator)
Badge::new().dot();

// Icon badge
Badge::new().icon(IconName::Check);
```

### Styling

```rust
Badge::new()
    .count(42)
    .color(cx.theme().danger)  // background color
    .small();                   // Sizable trait
```

`Badge` implements `Sizable` and `ParentElement`. When used as a child, it renders as an absolutely-positioned overlay.

---

## Tag

**Import:** `use gpui_component_ui::tag::{Tag, TagVariant};`

A small status indicator / chip used inline or in lists. Supports `Size::Medium` and `Size::Small` only.

### Variants

| Constructor    | `TagVariant`   | Description                       |
|----------------|-----------------|-----------------------------------|
| `Tag::new()`   | `Secondary`*    | Default, neutral background       |
| `.primary()`   | `Primary`       | Accent color                      |
| `.secondary()` | `Secondary`     | Neutral / default                 |
| `.danger()`    | `Danger`        | Error / destructive               |
| `.success()`   | `Success`       | Affirmative / green               |
| `.warning()`   | `Warning`       | Caution / amber                   |
| `.info()`      | `Info`          | Informational / blue              |
| `.color(name)` | `Color(ColorName)` | Named theme color              |
| `.custom(hsla, hsla, hsla)` | `Custom { color, foreground, border }` | Full control |

### Usage

```rust
// Default
Tag::new().child("Active");

// With outline style
Tag::new().primary().outline().child("Primary");

// Size
Tag::new().success().small().child("Done");

// Rounded
Tag::new().danger().rounded_full().child("Error");

// Custom colors
Tag::custom(cx.theme().purple, white(), cx.theme().purple)
    .child("Custom");
```

`Tag` implements `Sizable`, `ParentElement`, and `Styled`. Use `.child()` to set content (text, icon, etc.).

---

## Alert

**Import:** `use gpui_component_ui::alert::{Alert, AlertVariant};`

A message banner with icon and optional title. Implements `RenderOnce`.

### Variants

`AlertVariant` enum:
- `AlertVariant::Default` — neutral styling
- `AlertVariant::Info` — (blue)
- `AlertVariant::Success` — (green)
- `AlertVariant::Warning` — (amber)
- `AlertVariant::Error` — (red)

### Constructors

Each variant has a convenience constructor that pre-sets the variant and icon:

```rust
Alert::info("alert-1", "Your changes have been saved.")
    .title("Success");

Alert::warning("alert-2", "Your session will expire soon.");

Alert::error("alert-3", "Failed to connect to server.");

Alert::success("alert-4", "Payment processed.");
```

### Generic Constructor

```rust
Alert::new("my-alert", "Message text")
    .with_variant(AlertVariant::Info)
    .icon(IconName::Bell)
    .title("Optional Title");
```

### Methods

| Method | Description |
|--------|-------------|
| `.title("text")` | Bold title above message |
| `.icon(icon)` | Custom icon (default matches variant) |
| `.with_variant(variant)` | Change variant after construction |
| `.banner()` | Full-width, borderless banner style; hides title |
| `.visible(bool)` | Show/hide (default true) |
| `.on_close(\|event, window, cx\| {})` | Adds close (X) button; callback receives `ClickEvent` |

```rust
Alert::error("alert", "Something went wrong.")
    .title("Error")
    .on_close(|_event, _window, _cx| {
        // dismiss
    });
```

`Alert` also implements `Sizable`.

---

## Loading Indicators

### Spinner

**Import:** `use gpui_component_ui::spinner::Spinner;`

A rotating icon animation. Uses `IconName::Loader` by default.

```rust
Spinner::new();                          // default: medium, Loader icon
Spinner::new().small();                  // size variants
Spinner::new().large();
Spinner::new()
    .icon(IconName::LoaderCircle)       // custom icon
    .color(cx.theme().primary);          // custom color

// In a button:
Button::new("busy").label("Saving").loading(true);
```

Properties:
- Default rotation speed: 0.8s per full rotation
- Custom icon via `.icon()` (should be suitable for rotation)
- Custom color via `.color(Hsla)`
- Implements `Sizable`

### Skeleton

**Import:** `use gpui_component_ui::skeleton::Skeleton;`

A pulsing placeholder block used during content loading.

```rust
Skeleton::new();                        // default color
Skeleton::new().secondary();           // lighter variant
Skeleton::new().h_8().w_full();        // style via Styled trait
```

Implements `Styled` — apply any layout styling directly. The pulse animation is built-in (2s cycle).

### Progress

**Import:** `use gpui_component_ui::progress::Progress;`

A linear horizontal progress bar.

```rust
Progress::new("upload-progress")
    .value(45.0)                        // 0.0 to 100.0
    .color(cx.theme().primary);         // custom color

// Indeterminate loading animation
Progress::new("loading-progress")
    .loading(true);
```

Methods:
- `.value(f32)` — set progress percentage (0.0–100.0)
- `.loading(bool)` — enable indeterminate sliding animation (ignores `value`)
- `.color(Hsla)` — custom fill color
- Implements `Sizable`, `Styled`

### ProgressCircle

**Import:** `use gpui_component_ui::progress::ProgressCircle;`

A circular progress indicator (arc).

```rust
ProgressCircle::new("download-progress")
    .value(60.0)                        // 0.0 to 100.0
    .color(cx.theme().success);

// Indeterminate
ProgressCircle::new("loading-circle")
    .loading(true);

// Center content (e.g., percentage text)
ProgressCircle::new("progress")
    .value(75.0)
    .child("75%");      // ParentElement trait
```

Methods:
- `.value(f32)` — arc fill percentage (0.0–100.0)
- `.loading(bool)` — indeterminate rotating arc animation
- `.color(Hsla)` — custom arc color
- Implements `Sizable`, `Styled`, `ParentElement`

---

## Tooltip

**Import:** `use gpui_component_ui::tooltip::Tooltip;`

> **Note:** The managed tooltip system hooks into each window's `Root` element. Components registered with `.managed_tooltip()` automatically communicate with the overlay.

### Inline on Any Element

The `Button` and `Toggle` components provide a direct `.tooltip()` method:

```rust
Button::new("save")
    .label("Save")
    .tooltip("Save current file");

Toggle::new("bold")
    .label("B")
    .tooltip("Bold (Ctrl+B)");

// Tooltip with action keybinding auto-display
Button::new("undo")
    .label("Undo")
    .tooltip_with_action("Undo last action", &EditorAction::Undo, Some("editor"));
```

### Via Tooltip Builder

For elements that don't have `.tooltip()`, use `Tooltip::new().build(window, cx)`:

```rust
Tooltip::new("This is a tooltip")
    .action(&my_action, Some("context"))           // show keybinding if action has one
    .key_binding(Some(Kbd::new(Keystroke::parse("ctrl-s").unwrap()))) // explicit keybinding
    .text_sm()
    .build(window, cx);
```

### Tooltip Methods

| Method | Description |
|--------|-------------|
| `Tooltip::new(text)` | Text content (accepts `Into<Text>`) |
| `Tooltip::element(\|win, cx\| -> Element)` | Custom element content |
| `.action(&dyn Action, Option<&str>)` | Auto-resolve keybinding for the action + context |
| `.key_binding(Option<Kbd>)` | Explicit keyboard shortcut display |
| `.build(window, cx) -> AnyView` | Render to an `AnyView` for the managed tooltip system |

### Behavior

The managed system provides:
- 500ms show delay for new tooltips
- 300ms grace period for switching between adjacent elements (instant show)
- Slide-down + fade-in enter animation (150ms)
- Horizontal slide animation on tooltip switch (200ms)
- Auto-positioning above/below trigger with viewport clamping

---

## HoverCard

**Import:** `use gpui_component_ui::hover_card::{HoverCard, HoverCardState};`

A popover triggered by mouse hover with configurable delays. Similar to `Popover` but activated by hover, not click.

```rust
HoverCard::new("user-info")
    .trigger(Button::new("user-btn").label("User"))
    .content(|state: &mut HoverCardState, _window, cx: &mut Context<HoverCardState>| {
        div()
            .p_3()
            .child("User details here")
            .child(format!("Open: {}", state.is_open()))
    })
    .anchor(Anchor::TopCenter)     // where content appears relative to trigger
    .open_delay(Duration::from_millis(600))   // default 600ms
    .close_delay(Duration::from_millis(300))  // default 300ms
    .appearance(true)              // apply default popover styling
    .on_open_change(|is_open: &bool, window, cx| {
        // notified when open/close state changes
    });
```

### HoverCardState

Passed to the `content` builder, provides:

| Method | Description |
|--------|-------------|
| `state.is_open() -> bool` | Whether the hover card is currently visible |
| `state.open()` | Programmatically open |
| `state.close()` | Programmatically close |
| `state.toggle()` | Toggle open/close |

### HoverCard Methods

| Method | Description |
|--------|-------------|
| `.trigger(element)` | The element that activates the hover card on mouse enter |
| `.content(\|state, window, cx\| -> Element)` | Builder for the card's content |
| `.anchor(anchor)` | Anchoring corner (default `Anchor::TopCenter`) |
| `.open_delay(duration)` | Delay before showing (default 600ms) |
| `.close_delay(duration)` | Delay before hiding after mouse leaves (default 300ms) |
| `.appearance(bool)` | Apply default popover chrome (default true) |
| `.on_open_change(\|is_open, win, cx\|)` | State change callback |

---

## Label

**Import:** `use gpui_component_ui::label::Label;`

A text label element with optional secondary (muted) text, text masking, and highlight support.

```rust
// Basic
Label::new("Hello World");

// With secondary text (rendered in muted color after primary text)
Label::new("version").secondary("1.0.0");

// Masked content (shows bullet dots, e.g., for passwords/secrets)
Label::new("secret-key-12345").masked(true);

// Text highlighting (prefix or full-match)
Label::new("Hello World").highlights(HighlightsMatch::Prefix("Hel"));
Label::new("Hello World").highlights(HighlightsMatch::Full("Hello"));
```

`HighlightsMatch` enum:
- `HighlightsMatch::Prefix(SharedString)` — highlight prefix of the label text
- `HighlightsMatch::Full(SharedString)` — highlight full match

Implements `Styled`.

---

## Kbd

**Import:** `use gpui_component_ui::kbd::Kbd;`

Displays a keyboard key combination as a styled tag. Auto-formats to platform conventions (Cmd on macOS, Ctrl on Windows/Linux).

```rust
use gpui::{Keystroke, AsKeystroke};

// From Keystroke
Kbd::new(Keystroke::parse("ctrl-s").unwrap());

// Platform-aware formatting
Kbd::from(Keystroke::parse("cmd-shift-p").unwrap());

// Without visual appearance (text only, no border/chip)
Kbd::new(Keystroke::parse("ctrl-c").unwrap()).appearance(false);

// Outline style
Kbd::new(Keystroke::parse("enter").unwrap()).outline();
```

### Resolving Keybindings

These methods return `Option<Kbd>` for the first matching binding:

```rust
// For the current window's global context
Kbd::binding_for_action(&my_action, Some("editor"), window);

// For a specific focus handle (respects layered key contexts)
Kbd::binding_for_action_in(&my_action, &focus_handle, window);
```

### Platform Formatting

`Kbd::format(&keystroke) -> String` converts a `Keystroke` to a human-readable string:
- macOS: `⇧⌘P`, `⌃C`
- Windows/Linux: `Ctrl+Shift+P`, `Ctrl+C`

---

## Link

**Import:** `use gpui_component_ui::link::Link;`

A hyperlink-like element. When `href` is set, clicking opens the link (if no custom `on_click` handler overrides it).

```rust
Link::new("docs-link")
    .href("https://example.com")
    .child("Visit documentation");

// Custom click handler (overrides href navigation)
Link::new("internal-link")
    .on_click(|_event: &ClickEvent, _window, _cx| {
        // custom navigation logic
    })
    .child("Internal page");

// Disabled link
Link::new("disabled").disabled(true).child("Disabled");
```

Methods:
- `.href("url")` — set the hyperlink target
- `.on_click(|event, window, cx|)` — custom click handler (overrides href behavior)
- `.disabled(bool)` — greyed-out, non-interactive
- Implements `Styled`, `ParentElement`

---

## Clipboard

**Import:** `use gpui_component_ui::clipboard::Clipboard;`

A copy-to-clipboard button. Renders as a `Button` with copy icon + tooltip.

```rust
Clipboard::new("copy-token")
    .value("abc123-token")            // static value to copy
    .tooltip("Copy token")
    .on_copied(|value: SharedString, _window, _cx| {
        println!("Copied: {}", value);
    });

// Dynamic value (evaluated at copy time)
Clipboard::new("copy-dynamic")
    .value_fn(|_window, _cx| {
        // compute the value to copy at click time
        "dynamic-value".into()
    });
```

Methods:
- `.value(str)` — static copy value (default empty string)
- `.value_fn(|window, cx| -> SharedString)` — dynamic value, evaluated on click
- `.tooltip(str)` — tooltip text for the copy button
- `.on_copied(|value, window, cx|)` — callback when content is copied

---

## Image

gpui-component does **not** wrap gpui's image support in a dedicated component. Use gpui's built-in `img()` element directly:

```rust
div().child(
    img(ImageSource::File("path/to/image.png".into()))
        .size_full()
);
```

For images within rich text, use `ImageNode` in `crate::text::node`.

---

## Avatar

**Import:** `use gpui_component_ui::avatar::Avatar;`

A user avatar with image, name fallback initials, and placeholder icon support.

```rust
// Image avatar with fallback initials
Avatar::new()
    .src(ImageSource::File("avatar.png".into()))
    .name("Jane Doe");       // used as fallback if image fails / for initials

// Name-only (shows extracted initials, e.g., "JD" from "Jane Doe")
Avatar::new().name("Jane Doe");

// Custom placeholder icon (default: IconName::User)
Avatar::new()
    .name("Bot")
    .placeholder(IconName::Bot);

// Sizing
Avatar::new().large();
Avatar::new().xsmall();
```

Name extraction:
- Single word: first character (uppercased)
- Multiple words: first character of first word + first character of last word

Implements `Sizable`, `Styled`, `InteractiveElement`.

---

## Separator

**Import:** `use gpui_component_ui::separator::{Separator, SeparatorStyle};`

A horizontal or vertical divider line, with optional label.

```rust
// Vertical solid separator
Separator::vertical();

// Horizontal solid separator
Separator::horizontal();

// Dashed variants
Separator::vertical_dashed();
Separator::horizontal_dashed();

// With label (centered text in the gap)
Separator::horizontal().label("OR");

// Custom color
Separator::horizontal().color(cx.theme().border);

// Explicit dashed style
Separator::horizontal().dashed();
```

`SeparatorStyle` enum:
- `SeparatorStyle::Solid` — continuous line (default)
- `SeparatorStyle::Dashed` — dashed line

Implements `Styled`.
