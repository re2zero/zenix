# Theming

The gpui-component library provides a comprehensive theming system based on a global `Theme` struct, a shadcn-inspired color palette, and a registry for loading custom themes from JSON files.

**Contents:** [ActiveTheme Trait](#activetheme-trait) · [Theme Struct](#theme-struct) · [ThemeColor Struct](#themecolor-struct--full-field-reference) · [Colorize Trait](#colorize-trait) · [ThemeMode](#thememode) · [ThemeRegistry](#themeregistry) · [Custom Themes JSON](#creating-custom-themes--json-schema) · [Font Configuration](#font-configuration) · [i18n](#i18n)

---

## ActiveTheme Trait

```rust
// crates/ui/src/theme/mod.rs

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}
```

The `ActiveTheme` trait provides access to the global `Theme`. It is implemented for `App`, and gpui's `Context<T>` also exposes `.theme()` as a built-in method (it delegates to the global `Theme` set via `cx.set_global()`).

| Context          | Access                                |
| ---------------- | ------------------------------------- |
| `App`            | `app.theme()` (via `ActiveTheme`)     |
| `Context<T>`     | `cx.theme()` (gpui built-in)          |
| Anywhere         | `Theme::global(cx)` (static method)   |

### Usage

```rust
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
}
```

`Theme` also implements `Deref<Target = ThemeColor>`, so all color fields are available directly:

```rust
cx.theme().primary           // same as cx.theme().colors.primary
cx.theme().foreground        // same as cx.theme().colors.foreground
```

---

## Theme Struct

```rust
// crates/ui/src/theme/mod.rs

pub struct Theme {
    pub colors: ThemeColor,
    pub highlight_theme: Arc<HighlightTheme>,
    pub light_theme: Rc<ThemeConfig>,
    pub dark_theme: Rc<ThemeConfig>,

    pub mode: ThemeMode,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub mono_font_family: SharedString,
    pub mono_font_size: Pixels,
    pub radius: Pixels,
    pub radius_lg: Pixels,
    pub shadow: bool,
    pub transparent: Hsla,
    pub scrollbar_show: ScrollbarShow,
    pub notification: NotificationSettings,
    pub tile_grid_size: Pixels,
    pub tile_shadow: bool,
    pub tile_radius: Pixels,
    pub list: ListSettings,
    pub sheet: SheetSettings,
}
```

| Field                | Type                  | Default                        | Description                                              |
| -------------------- | --------------------- | ------------------------------ | -------------------------------------------------------- |
| `colors`             | `ThemeColor`          | —                              | All 60+ color slots (see ThemeColor section).            |
| `highlight_theme`    | `Arc<HighlightTheme>` | light/dark default             | Syntax highlighting theme for code editors.              |
| `light_theme`        | `Rc<ThemeConfig>`     | default light config           | The light-mode `ThemeConfig` backing this theme.         |
| `dark_theme`         | `Rc<ThemeConfig>`     | default dark config            | The dark-mode `ThemeConfig` backing this theme.          |
| `mode`               | `ThemeMode`           | `Light`                        | Current theme mode (Light / Dark).                       |
| `font_family`        | `SharedString`        | `.SystemUIFont`                | System UI font family.                                   |
| `font_size`          | `Pixels`              | `px(16.)`                      | Base font size.                                          |
| `mono_font_family`   | `SharedString`        | platform-specific              | Monospace font family (Menlo / Consolas / DejaVu Sans Mono). |
| `mono_font_size`     | `Pixels`              | `px(13.)`                      | Monospace font size.                                     |
| `radius`             | `Pixels`              | `px(6.)`                       | Border radius for general elements.                      |
| `radius_lg`          | `Pixels`              | `px(8.)`                       | Border radius for large elements (Dialogs, Notifications). |
| `shadow`             | `bool`                | `true`                         | Enable shadows on inputs, buttons, etc.                  |
| `transparent`        | `Hsla`                | `Hsla::transparent_black()`    | Utility transparent color for mixing.                    |
| `scrollbar_show`     | `ScrollbarShow`       | `Scrolling`                    | Scrollbar visibility behavior.                           |
| `notification`       | `NotificationSettings`| see below                      | Notification placement and limits.                       |
| `tile_grid_size`     | `Pixels`              | `px(8.)`                       | Grid size for dock tiles.                                |
| `tile_shadow`        | `bool`                | `true`                         | Shadow on tile panels.                                   |
| `tile_radius`        | `Pixels`              | `px(0.)`                       | Border radius of tile panels.                            |
| `list`               | `ListSettings`        | `active_highlight: true`       | List component settings.                                 |
| `sheet`              | `SheetSettings`       | `margin_top: TITLE_BAR_HEIGHT` | Sheet component settings.                                |

### Key Methods on `Theme`

```rust
impl Theme {
    /// Read-only access to the global Theme.
    pub fn global(cx: &App) -> &Theme;

    /// Mutable access to the global Theme.
    pub fn global_mut(cx: &mut App) -> &mut Theme;

    /// Returns true if the current mode is Dark.
    pub fn is_dark(&self) -> bool;

    /// Returns the current theme name (e.g., "Default Light" or "Default Dark").
    pub fn theme_name(&self) -> &SharedString;

    /// Sync the theme mode with the system appearance (light/dark).
    pub fn sync_system_appearance(window: Option<&mut Window>, cx: &mut App);

    /// Sync scrollbar show behavior with the OS setting.
    pub fn sync_scrollbar_appearance(cx: &mut App);

    /// Change the theme mode (Light or Dark).
    /// Applies the corresponding ThemeConfig and refreshes the given window.
    pub fn change(mode: impl Into<ThemeMode>, window: Option<&mut Window>, cx: &mut App);

    /// Returns the input background color (for dark: transparent mix, for light: background).
    pub fn input_background(&self) -> Hsla;

    /// Returns the editor background color from the highlight theme, falling back to input_background.
    pub(crate) fn editor_background(&self) -> Hsla;

    /// Apply a ThemeConfig to this Theme.
    pub fn apply_config(&mut self, config: &Rc<ThemeConfig>);
}
```

### ListSettings

```rust
// crates/ui/src/list/mod.rs

pub struct ListSettings {
    /// Whether to use active highlight style on ListItem, default: true.
    pub active_highlight: bool,
}
```

### SheetSettings

```rust
// crates/ui/src/sheet.rs

pub struct SheetSettings {
    /// The margin top for the sheet, default: TITLE_BAR_HEIGHT.
    pub margin_top: Pixels,
}
```

### NotificationSettings

```rust
// crates/ui/src/notification.rs

pub struct NotificationSettings {
    /// Placement of the notification, default: Anchor::TopRight.
    pub placement: Anchor,
    /// Margins from window edges.
    pub margins: Edges<Pixels>,
    /// Maximum simultaneous notifications, default: 10.
    pub max_items: usize,
}
```

### ScrollbarShow

```rust
// crates/ui/src/scroll/scrollbar.rs

pub enum ScrollbarShow {
    /// Show scrollbar when scrolling, fade out after idle. (default)
    Scrolling,
    /// Show scrollbar on hover.
    Hover,
    /// Always show scrollbar.
    Always,
}
```

---

## ThemeColor Struct — Full Field Reference

```rust
// crates/ui/src/theme/theme_color.rs

pub struct ThemeColor {
    // ── Core ──
    pub background: Hsla,           // Default background color.
    pub foreground: Hsla,           // Default text color.
    pub border: Hsla,               // Default border color.
    pub ring: Hsla,                 // Focus ring color.
    pub input: Hsla,                // Border color for Input, Select, etc.

    // ── Primary ──
    pub primary: Hsla,              // Primary background color.
    pub primary_foreground: Hsla,   // Primary text color.
    pub primary_hover: Hsla,        // Primary hover background.
    pub primary_active: Hsla,       // Primary active background.

    // ── Secondary ──
    pub secondary: Hsla,            // Secondary background color.
    pub secondary_foreground: Hsla, // Secondary text color.
    pub secondary_hover: Hsla,      // Secondary hover background.
    pub secondary_active: Hsla,     // Secondary active background.

    // ── Accent ──
    pub accent: Hsla,               // Accent background (hover on MenuItem, ListItem, etc.).
    pub accent_foreground: Hsla,    // Accent text color.

    // ── Destructive / Danger ──
    pub danger: Hsla,               // Danger background color.
    pub danger_foreground: Hsla,    // Danger text color.
    pub danger_hover: Hsla,         // Danger hover background.
    pub danger_active: Hsla,        // Danger active background.

    // ── Success ──
    pub success: Hsla,              // Success background color.
    pub success_foreground: Hsla,   // Success text color.
    pub success_hover: Hsla,        // Success hover background.
    pub success_active: Hsla,       // Success active background.

    // ── Warning ──
    pub warning: Hsla,              // Warning background color.
    pub warning_foreground: Hsla,   // Warning foreground color.
    pub warning_hover: Hsla,        // Warning hover background.
    pub warning_active: Hsla,       // Warning active background.

    // ── Info ──
    pub info: Hsla,                 // Info background color.
    pub info_foreground: Hsla,      // Info text color.
    pub info_hover: Hsla,           // Info hover background.
    pub info_active: Hsla,          // Info active background.

    // ── Muted ──
    pub muted: Hsla,                // Muted backgrounds (Skeleton, Switch).
    pub muted_foreground: Hsla,     // Muted text / disabled text.

    // ── Button Primary ──
    pub button_primary: Hsla,               // Button primary background (fallback: primary).
    pub button_primary_foreground: Hsla,    // Button primary text (fallback: primary_foreground).
    pub button_primary_hover: Hsla,         // Button primary hover (fallback: primary_hover).
    pub button_primary_active: Hsla,        // Button primary active (fallback: primary_active).

    // ── Accordion ──
    pub accordion: Hsla,            // Accordion background.
    pub accordion_hover: Hsla,      // Accordion hover background.

    // ── List ──
    pub list: Hsla,                 // List/ListItem background.
    pub list_active: Hsla,          // Active ListItem background.
    pub list_active_border: Hsla,   // Active ListItem border.
    pub list_even: Hsla,            // Even-row stripe background.
    pub list_head: Hsla,            // List header background.
    pub list_hover: Hsla,           // ListItem hover background.

    // ── Table ──
    pub table: Hsla,                // Table background.
    pub table_active: Hsla,         // Table active row background.
    pub table_active_border: Hsla,  // Table active row border.
    pub table_even: Hsla,           // Even-row stripe background.
    pub table_head: Hsla,           // Table header background.
    pub table_head_foreground: Hsla,// Table header text.
    pub table_foot: Hsla,           // Table footer background.
    pub table_foot_foreground: Hsla,// Table footer text.
    pub table_hover: Hsla,          // Table row hover background.
    pub table_row_border: Hsla,     // Table row border.

    // ── Tab ──
    pub tab: Hsla,                  // Tab background.
    pub tab_active: Hsla,           // Tab active background.
    pub tab_active_foreground: Hsla,// Tab active text.
    pub tab_bar: Hsla,              // TabBar background.
    pub tab_bar_segmented: Hsla,    // Segmented TabBar background.
    pub tab_foreground: Hsla,       // Tab text.

    // ── Sidebar ──
    pub sidebar: Hsla,              // Sidebar background.
    pub sidebar_foreground: Hsla,   // Sidebar text.
    pub sidebar_border: Hsla,       // Sidebar border.
    pub sidebar_accent: Hsla,       // Sidebar accent background.
    pub sidebar_accent_foreground: Hsla, // Sidebar accent text.
    pub sidebar_primary: Hsla,      // Sidebar primary background.
    pub sidebar_primary_foreground: Hsla, // Sidebar primary text.

    // ── Popover ──
    pub popover: Hsla,              // Popover background.
    pub popover_foreground: Hsla,   // Popover text.

    // ── Slider ──
    pub slider_bar: Hsla,           // Slider bar background.
    pub slider_thumb: Hsla,         // Slider thumb background.

    // ── Switch ──
    pub switch: Hsla,               // Switch background.
    pub switch_thumb: Hsla,         // Switch thumb background.

    // ── Scrollbar ──
    pub scrollbar: Hsla,            // Scrollbar track background.
    pub scrollbar_thumb: Hsla,      // Scrollbar thumb background.
    pub scrollbar_thumb_hover: Hsla,// Scrollbar thumb hover background.

    // ── Misc Component Colors ──
    pub caret: Hsla,                // Input caret (blinking cursor).
    pub selection: Hsla,            // Text selection background.
    pub link: Hsla,                 // Link text.
    pub link_hover: Hsla,           // Link hover text.
    pub link_active: Hsla,          // Link active text.
    pub progress_bar: Hsla,         // Progress bar background.
    pub skeleton: Hsla,             // Skeleton loading background.
    pub title_bar: Hsla,            // TitleBar background (window title bar).
    pub title_bar_border: Hsla,     // TitleBar border.
    pub status_bar: Hsla,           // StatusBar background.
    pub status_bar_border: Hsla,    // StatusBar border.
    pub group_box: Hsla,            // GroupBox background.
    pub group_box_foreground: Hsla, // GroupBox text.
    pub description_list_label: Hsla,        // DescriptionList label background.
    pub description_list_label_foreground: Hsla, // DescriptionList label text.
    pub drag_border: Hsla,          // Drag border color.
    pub drop_target: Hsla,          // Drop target background.
    pub tiles: Hsla,                // Tiles background.
    pub overlay: Hsla,              // Overlay background (e.g., dialog backdrop).
    pub window_border: Hsla,        // Window border color (Linux only).

    // ── Chart Colors ──
    pub chart_1: Hsla,              // Chart color 1.
    pub chart_2: Hsla,              // Chart color 2.
    pub chart_3: Hsla,              // Chart color 3.
    pub chart_4: Hsla,              // Chart color 4.
    pub chart_5: Hsla,              // Chart color 5.
    pub chart_bullish: Hsla,        // Candlestick bullish (upward) color.
    pub chart_bearish: Hsla,        // Candlestick bearish (downward) color.

    // ── Base Colors ──
    pub red: Hsla,                  // Base red.
    pub red_light: Hsla,            // Base light red.
    pub green: Hsla,                // Base green.
    pub green_light: Hsla,          // Base light green.
    pub blue: Hsla,                 // Base blue.
    pub blue_light: Hsla,           // Base light blue.
    pub yellow: Hsla,               // Base yellow.
    pub yellow_light: Hsla,         // Base light yellow.
    pub magenta: Hsla,              // Base magenta.
    pub magenta_light: Hsla,        // Base light magenta.
    pub cyan: Hsla,                 // Base cyan.
    pub cyan_light: Hsla,           // Base light cyan.
}
```

### Static Constructors

```rust
impl ThemeColor {
    /// Get the default light theme colors.
    pub fn light() -> Arc<Self>;

    /// Get the default dark theme colors.
    pub fn dark() -> Arc<Self>;
}
```

---

## Colorize Trait

The `Colorize` trait (from `crate::color`) is implemented for `Hsla` and provides color manipulation methods used throughout the theming system:

```rust
pub trait Colorize: Sized {
    fn opacity(&self, opacity: f32) -> Self;          // 0.0 .. 1.0
    fn divide(&self, divisor: f32) -> Self;
    fn invert(&self) -> Self;
    fn invert_l(&self) -> Self;
    fn lighten(&self, amount: f32) -> Self;            // 0.0 .. 1.0
    fn darken(&self, amount: f32) -> Self;             // 0.0 .. 1.0
    fn apply(&self, base_color: Self) -> Self;
    fn mix(&self, other: Self, factor: f32) -> Self;   // 0.0 .. 1.0
    fn mix_oklab(&self, other: Self, factor: f32) -> Self;
    fn hue(&self, hue: f32) -> Self;                   // 0.0 .. 1.0
    fn saturation(&self, saturation: f32) -> Self;     // 0.0 .. 1.0
    fn lightness(&self, lightness: f32) -> Self;       // 0.0 .. 1.0
    fn to_hex(&self) -> String;
    fn parse_hex(hex: &str) -> Result<Self>;
}
```

### `try_parse_color`

Parses color strings into `Hsla`. Supports HEX and [Tailwind Color](https://tailwindcss.com/docs/colors) expressions:

```
try_parse_color("#3b82f6")          // HEX
try_parse_color("#3b82f640")        // HEX with alpha
try_parse_color("blue-600")         // Tailwind name-scale
try_parse_color("red-500/50")       // Tailwind name-scale/opacity (0-100)
try_parse_color("white")            // Named color
try_parse_color("black")            // Named color
```

### Pre-built Color Palette Functions

Color scale functions generate `Hsla` for shadcn-style colors (slate, gray, zinc, neutral, stone, red, orange, amber, yellow, lime, green, emerald, teal, cyan, sky, blue, indigo, violet, purple, fuchsia, pink, rose):

```rust
use gpui_component::color::{slate, red, blue, green, /* ... */};

let bg = slate(100);     // e.g., equivalent to "slate-100"
let text = red(600);     // "red-600"
let accent = blue(500);  // "blue-500"
```

Also available: `black()` and `white()`.

---

## ThemeMode

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}
```

### Methods

```rust
impl ThemeMode {
    /// Returns true if the mode is Dark.
    pub fn is_dark(&self) -> bool;

    /// Returns lowercase name: "light" or "dark".
    pub fn name(&self) -> &'static str;
}
```

### From\<WindowAppearance\>

```rust
impl From<WindowAppearance> for ThemeMode {
    // WindowAppearance::Dark | VibrantDark => ThemeMode::Dark
    // WindowAppearance::Light | VibrantLight => ThemeMode::Light
}
```

### Switching Modes

Use `Theme::change()` to switch between light and dark:

```rust
// Toggle to dark mode
Theme::change(ThemeMode::Dark, window, cx);

// Sync with system appearance
Theme::sync_system_appearance(window, cx);
```

Checking current mode:

```rust
if cx.theme().is_dark() {
    // dark mode logic
}
```

---

## ThemeRegistry

The `ThemeRegistry` manages loading, watching, and enumerating theme configurations.

```rust
// crates/ui/src/theme/registry.rs

pub struct ThemeRegistry {
    themes_dir: PathBuf,
    default_themes: HashMap<ThemeMode, Rc<ThemeConfig>>,
    themes: HashMap<SharedString, Rc<ThemeConfig>>,
    has_custom_themes: bool,
}
```

### Methods

| Method | Signature | Description |
| ------ | --------- | ----------- |
| `global` | `fn global(cx: &App) -> &Self` | Read-only access to the global registry. |
| `global_mut` | `fn global_mut(cx: &mut App) -> &mut Self` | Mutable access. |
| `watch_dir` | `fn watch_dir(themes_dir: PathBuf, cx: &mut App, on_load: F)` | Watch a directory for `.json` theme files; auto-reloads on changes. (native only) |
| `themes` | `fn themes(&self) -> &HashMap<SharedString, Rc<ThemeConfig>>` | All loaded themes (default + custom). |
| `sorted_themes` | `fn sorted_themes(&self) -> Vec<&Rc<ThemeConfig>>` | Sorted: defaults first, then light-before-dark, then case-insensitive by name. |
| `default_themes` | `fn default_themes(&self) -> &HashMap<ThemeMode, Rc<ThemeConfig>>` | Default themes keyed by mode. |
| `default_light_theme` | `fn default_light_theme(&self) -> &Rc<ThemeConfig>` | The default light `ThemeConfig`. |
| `default_dark_theme` | `fn default_dark_theme(&self) -> &Rc<ThemeConfig>` | The default dark `ThemeConfig`. |
| `load_themes_from_str` | `fn load_themes_from_str(&mut self, content: &str) -> Result<()>` | Parse and register themes from a JSON string. |

### Loading Custom Themes

#### From JSON String (Embedded Themes)

```rust
use gpui_component::theme::ThemeRegistry;

let custom_json = r#"{
    "name": "My Theme Set",
    "author": "Me",
    "themes": [
        {
            "is_default": false,
            "name": "My Custom Light",
            "mode": "light",
            "colors": {
                "background": "white",
                "foreground": "neutral-900",
                "primary.background": "blue-600",
                "primary.foreground": "white"
            }
        }
    ]
}"#;

cx.update(|cx| {
    ThemeRegistry::global_mut(cx).load_themes_from_str(custom_json).unwrap();
});
```

#### From a Directory (File System)

```rust
ThemeRegistry::watch_dir(
    PathBuf::from("./themes"),
    cx,
    |cx| {
        // Called after themes are loaded/reloaded
        Theme::change(ThemeMode::Light, None, cx);
    },
).unwrap();
```

The directory is watched with `notify` — adding, modifying, or removing JSON files triggers automatic reload.

---

## Creating Custom Themes — JSON Schema

### ThemeSet (Top-Level)

```json
{
    "$schema": "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json",
    "name": "My Theme Set",
    "author": "Author Name",
    "url": "https://example.com",
    "themes": [ /* ThemeConfig array */ ]
}
```

### ThemeConfig

Each entry in the `themes` array:

| JSON Key             | Rust Field          | Type       | Description |
| -------------------- | ------------------- | ---------- | ----------- |
| `is_default`         | `is_default`        | `bool`     | Mark as default for its mode. |
| `name`               | `name`              | `string`   | Unique theme name. |
| `mode`               | `mode`              | `"light" \| "dark"` | Theme mode. |
| `font.size`          | `font_size`         | `f32`?     | Base font size (default: 16). |
| `font.family`        | `font_family`       | `string`?  | UI font family (default: `.SystemUIFont`). |
| `mono_font.family`   | `mono_font_family`  | `string`?  | Monospace font family. |
| `mono_font.size`     | `mono_font_size`    | `f32`?     | Monospace font size (default: 13). |
| `radius`             | `radius`            | `usize`?   | General border radius (default: 6). |
| `radius.lg`          | `radius_lg`         | `usize`?   | Large border radius for dialogs (default: 8). |
| `shadow`             | `shadow`            | `bool`?    | Enable shadows (default: true). |
| `colors`             | `colors`            | `object`   | Color definitions (see below). |
| `highlight`          | `highlight`         | `object`?  | Syntax highlighting styles (Zed-compatible). |

### Color Value Format

Each color value is a string parsable by `try_parse_color`:

```
"background": "#ffffff"           // HEX
"foreground": "#0a0a0a"           // HEX
"border": "neutral-200"           // Tailwind name-scale
"primary.background": "blue-600"  // Tailwind name-scale
"overlay": "#00000033"            // HEX with alpha
"drop_target.background": "#3b82f640" // HEX with alpha
```

### Full Color Keys Reference

| JSON Key | Rust Field |
| -------- | ---------- |
| `accent.background` | `accent` |
| `accent.foreground` | `accent_foreground` |
| `accordion.background` | `accordion` |
| `accordion.hover.background` | `accordion_hover` |
| `background` | `background` |
| `border` | `border` |
| `button.primary.background` | `button_primary` |
| `button.primary.active.background` | `button_primary_active` |
| `button.primary.foreground` | `button_primary_foreground` |
| `button.primary.hover.background` | `button_primary_hover` |
| `group_box.background` | `group_box` |
| `group_box.foreground` | `group_box_foreground` |
| `group_box.title.foreground` | `group_box_title_foreground` |
| `caret` | `caret` |
| `chart.1` | `chart_1` |
| `chart.2` | `chart_2` |
| `chart.3` | `chart_3` |
| `chart.4` | `chart_4` |
| `chart.5` | `chart_5` |
| `chart_bullish` | `chart_bullish` |
| `chart_bearish` | `chart_bearish` |
| `danger.background` | `danger` |
| `danger.active.background` | `danger_active` |
| `danger.foreground` | `danger_foreground` |
| `danger.hover.background` | `danger_hover` |
| `description_list.label.background` | `description_list_label` |
| `description_list.label.foreground` | `description_list_label_foreground` |
| `drag.border` | `drag_border` |
| `drop_target.background` | `drop_target` |
| `foreground` | `foreground` |
| `info.background` | `info` |
| `info.active.background` | `info_active` |
| `info.foreground` | `info_foreground` |
| `info.hover.background` | `info_hover` |
| `input.border` | `input` |
| `link` | `link` |
| `link.active` | `link_active` |
| `link.hover` | `link_hover` |
| `list.background` | `list` |
| `list.active.background` | `list_active` |
| `list.active.border` | `list_active_border` |
| `list.even.background` | `list_even` |
| `list.head.background` | `list_head` |
| `list.hover.background` | `list_hover` |
| `muted.background` | `muted` |
| `muted.foreground` | `muted_foreground` |
| `popover.background` | `popover` |
| `popover.foreground` | `popover_foreground` |
| `primary.background` | `primary` |
| `primary.active.background` | `primary_active` |
| `primary.foreground` | `primary_foreground` |
| `primary.hover.background` | `primary_hover` |
| `progress.bar.background` | `progress_bar` |
| `ring` | `ring` |
| `scrollbar.background` | `scrollbar` |
| `scrollbar.thumb.background` | `scrollbar_thumb` |
| `scrollbar.thumb.hover.background` | `scrollbar_thumb_hover` |
| `secondary.background` | `secondary` |
| `secondary.active.background` | `secondary_active` |
| `secondary.foreground` | `secondary_foreground` |
| `secondary.hover.background` | `secondary_hover` |
| `selection.background` | `selection` |
| `sidebar.background` | `sidebar` |
| `sidebar.accent.background` | `sidebar_accent` |
| `sidebar.accent.foreground` | `sidebar_accent_foreground` |
| `sidebar.border` | `sidebar_border` |
| `sidebar.foreground` | `sidebar_foreground` |
| `sidebar.primary.background` | `sidebar_primary` |
| `sidebar.primary.foreground` | `sidebar_primary_foreground` |
| `skeleton.background` | `skeleton` |
| `slider.background` | `slider_bar` |
| `slider.thumb.background` | `slider_thumb` |
| `success.background` | `success` |
| `success.foreground` | `success_foreground` |
| `success.hover.background` | `success_hover` |
| `success.active.background` | `success_active` |
| `switch.background` | `switch` |
| `switch.thumb.background` | `switch_thumb` |
| `tab.background` | `tab` |
| `tab.active.background` | `tab_active` |
| `tab.active.foreground` | `tab_active_foreground` |
| `tab_bar.background` | `tab_bar` |
| `tab_bar.segmented.background` | `tab_bar_segmented` |
| `tab.foreground` | `tab_foreground` |
| `table.background` | `table` |
| `table.active.background` | `table_active` |
| `table.active.border` | `table_active_border` |
| `table.even.background` | `table_even` |
| `table.head.background` | `table_head` |
| `table.head.foreground` | `table_head_foreground` |
| `table.foot.background` | `table_foot` |
| `table.foot.foreground` | `table_foot_foreground` |
| `table.hover.background` | `table_hover` |
| `table.row.border` | `table_row_border` |
| `title_bar.background` | `title_bar` |
| `title_bar.border` | `title_bar_border` |
| `status_bar.background` | `status_bar` |
| `status_bar.border` | `status_bar_border` |
| `tiles.background` | `tiles` |
| `warning.background` | `warning` |
| `warning.active.background` | `warning_active` |
| `warning.hover.background` | `warning_hover` |
| `warning.foreground` | `warning_foreground` |
| `overlay` | `overlay` |
| `window.border` | `window_border` |
| `base.red` | `red` |
| `base.red.light` | `red_light` |
| `base.green` | `green` |
| `base.green.light` | `green_light` |
| `base.blue` | `blue` |
| `base.blue.light` | `blue_light` |
| `base.yellow` | `yellow` |
| `base.yellow.light` | `yellow_light` |
| `base.magenta` | `magenta` |
| `base.magenta.light` | `magenta_light` |
| `base.cyan` | `cyan` |
| `base.cyan.light` | `cyan_light` |

---

## Font Configuration

### Overriding Fonts via ThemeConfig

Font settings are part of `ThemeConfig` and applied when `apply_config()` is called:

```json
{
    "name": "Custom Font Theme",
    "mode": "light",
    "font.size": 18,
    "font.family": "Inter",
    "mono_font.size": 14,
    "mono_font.family": "JetBrains Mono",
    "colors": { /* ... */ }
}
```

### Embedding Custom Fonts

To embed custom fonts in your application, call `cx.text_system().add_fonts()` during app initialization:

```rust
cx.text_system().add_fonts(vec![
    include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice(),
    include_bytes!("../assets/fonts/Inter-Bold.ttf").as_slice(),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
]).unwrap();
```

Then reference the family name in your theme config's `font.family` or `mono_font.family`.

### Accessing Font Settings at Runtime

```rust
let ui_font_size = cx.theme().font_size;
let ui_font = &cx.theme().font_family;
let mono_font = &cx.theme().mono_font_family;
let mono_size = cx.theme().mono_font_size;
```

---

## i18n

gpui-component uses `rust-i18n` for internationalization. Locale files are YAML files under a `locales/` directory.

### Initialization

In `lib.rs`, the `rust_i18n::i18n!` macro loads locale files at compile time:

```rust
// crates/ui/src/lib.rs
rust_i18n::i18n!("locales", fallback = "en");
```

### API

```rust
/// Get the current locale.
pub fn locale() -> impl Deref<Target = str>;

/// Set the current locale.
pub fn set_locale(locale: &str);
```

### Usage

```rust
use gpui_component::{locale, set_locale};

// Set locale (e.g., "zh-CN", "en", "it")
set_locale("zh-CN");

// Read current locale
let current = locale().to_string();
```

### Locale File Format (YAML)

Locale files live under the `locales/` directory. Example `locales/ui.yml`:

```yaml
_version: 2

Calendar:
  week.0:
    en: Su
    zh-CN: 日
    zh-HK: 日
    zh-TW: 日
    it: Do
  week.1:
    en: Mo
    zh-CN: 一
    zh-HK: 一
    zh-TW: 一
    it: Lu
  month.January:
    en: January
    zh-CN: 一月
    it: Gennaio

Settings:
  search_placeholder:
    en: Search...
    zh-CN: 搜索...
    it: Ricerca...
  Reset All:
    en: Reset All
    zh-CN: 重置全部
    it: Resetta Tutto

Pagination:
  previous:
    en: Previous
    zh-CN: 上一页
  next:
    en: Next
    zh-CN: 下一页
```

The top-level key is the translation namespace (e.g., `Calendar`, `Settings`, `Pagination`). Each entry is keyed by locale code with its translation. The fallback locale (`"en"` in the default config) is used when a translation is missing for the current locale.

To use translations in Rust code:

```rust
use rust_i18n::t;

let prev_label = t!("Pagination.previous");  // "Previous" (or "上一页" if locale is zh-CN)
```
