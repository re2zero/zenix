# Setup — Cargo, Init, Root, Assets & Configuration

This reference covers everything needed to bootstrap a GPUI application that uses
`gpui-component`: Cargo dependency setup, `init()` call, the `Root` view, asset
integration, platform specifics, font/locale configuration, and common pitfalls.

**Contents:** [Cargo.toml Setup](#1-cargotoml-setup) · [gpui_component::init](#2-gpui_componentinitcx--what-it-initializes) · [The Root Component](#3-the-root-component) · [Assets Integration](#4-gpui-component-assets-integration) · [Platform-Specific Notes](#5-platform-specific-notes) · [WindowOptions](#6-windowoptions-configuration) · [Font Setup](#7-font-setup) · [Locale Setup (i18n)](#8-locale-setup-i18n) · [Common Mistakes](#9-common-initialization-mistakes) · [Minimal Working Example](#10-minimal-working-example)

---

## 1. Cargo.toml Setup

### Minimal dependencies

```toml
[dependencies]
anyhow.workspace = true
gpui.workspace = true
gpui_platform.workspace = true
gpui-component = { workspace = true }
```

Three crates are **always** required:

| Crate                    | Purpose                                      |
| ------------------------ | -------------------------------------------- |
| `gpui`                   | Core GPUI primitives                         |
| `gpui_platform`          | Platform entrypoint (`application()`)        |
| `gpui-component`         | The component library (re-exports `Root`, `WindowExt`, etc.) |

### Adding bundled icons

```toml
[dependencies]
gpui-component-assets = { workspace = true }
```

This ships the default icon set (100+ SVG icons) as `gpui_component_assets::Assets`.
It is **optional** — you can also use your own `AssetSource` or `RustEmbed` struct
(see Section 4).

### Optional features

All available features on the `gpui-component` crate:

| Feature                | Effect                                                       |
| ---------------------- | ------------------------------------------------------------ |
| `decimal`              | Pulls in `rust_decimal` for use with `NumberInput`          |
| `inspector`            | Enables the debug inspector overlay (dev builds also enable it when `debug_assertions` is active) |
| `tree-sitter-languages`| Meta-feature: enables ALL tree-sitter grammars below         |
| `tree-sitter-rust`     | Enables Rust syntax highlighting                             |
| `tree-sitter-python`   | … Python                                                     |
| `tree-sitter-typescript` / `tree-sitter-tsx` | TypeScript / TSX                      |
| `tree-sitter-go`       | Go                                                           |
| `tree-sitter-javascript` | JavaScript                                                |
| `tree-sitter-html` / `tree-sitter-css` | Web languages                                |
| (and ~30 more)         | Each grammar has its own feature gate                        |

Example with a few grammars:

```toml
[dependencies]
gpui-component = { workspace = true, features = ["tree-sitter-rust", "tree-sitter-typescript"] }
```

> **Note:** Tree-sitter features are only available on native builds (`cfg(not(target_family = "wasm"))`).

### Workspace `Cargo.toml` pointers

`gpui-component` depends on GPUI from the Zed repository. A typical workspace
definition (from the main `Cargo.toml`):

```toml
[workspace.dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
gpui_macros = { git = "https://github.com/zed-industries/zed" }
gpui-component = { path = "crates/ui", version = "0.5.2" }
gpui-component-assets = { path = "crates/assets", version = "0.5.1" }
```

---

## 2. `gpui_component::init(cx)` — What It Initializes

**Call this exactly once**, at application entry, before any window is opened:

```rust
fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        // ...
    });
}
```

### Full initialization chain

`pub fn init(cx: &mut App)` performs these steps in order:

1. **`theme::init(cx)`**
   - Creates the global `ThemeRegistry` (loaded from the built-in `default-theme.json`).
   - Sets `Theme` as a GPUI global (default: Light mode, 16px base font, `.SystemUIFont`).
   - Registers an observer so that future theme-registry mutations auto-refresh the active theme.
   - Syncs the system scrollbar appearance.

2. **`global_state::init(cx)`**
   - Creates `GlobalState` as a GPUI global.
   - Tracks: the text-view state stack, open deferred popovers, application menus, and a
     `suppress_text_selection` flag.

3. **`inspector::init(cx)`** *(only when `feature = "inspector"` or `debug_assertions`)*
   - Enables the debug inspector overlay.

4. **`root::init(cx)`**
   - Binds global keybindings on the `"Root"` context:
     - `tab` → `Tab`
     - `shift-tab` → `TabPrev`
     - `cmd-c` (macOS) / `ctrl-c` (other) → `Copy`

5. **Per-component inits** (each registers its own actions/observers/keybindings):
   `focus_trap`, `color_picker`, `date_picker`, `dock`, `sheet`, `combobox`,
   `select`, `input`, `list`, `dialog`, `popover`, `menu`, `table`,
   `text`, `tree`, `tooltip`.

### Actions registered during `init`

The `actions!` macro in `src/actions.rs` defines UI-wide actions:

```rust
actions!(ui, [
    Cancel,
    SelectUp, SelectDown, SelectLeft, SelectRight,
    SelectFirst, SelectLast,
    SelectPrevColumn, SelectNextColumn,
    SelectPageUp, SelectPageDown
]);
```

A separate `Confirm` action carries a `secondary: bool` flag (for primary vs. secondary confirmation).

---

## 3. The Root Component

### Purpose

`Root` is the **required top-level view** in every GPUI window that uses
`gpui-component`. It is the first child placed in a window and manages three
overlay stacks:

- **Dialog stack** — multiple dialogs are stacked; the topmost captures focus.
- **Sheet overlay** — exactly one sheet at a time (left/right/top/bottom).
- **Notification list** — positioned according to the theme's `notification.placement`.

Additionally, it manages:
- Keyboard navigation (`tab` / `shift-tab` through focusable elements).
- Copy action (`cmd-c` / `ctrl-c`).
- Window-level text selection controller.
- Tooltip and native-menu overlays.

### Constructor

```rust
pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self
```

- `view` is your application's content (anything that implements `Render`).
- The constructor spawns internal entities for `NotificationList`, `TooltipOverlay`,
  and `FallbackMenuOverlay`.
- It initialises the window shadow size to `window_border::SHADOW_SIZE` (12px on
  Linux, 0px on macOS/Windows).

### Builder methods

| Method                   | Signature                               | Description                                      |
| ------------------------ | --------------------------------------- | ------------------------------------------------ |
| `window_shadow_size`     | `fn(self, size: impl Into<Pixels>)`     | Override the drop-shadow width (Linux CSD only). |

### Static helpers

```rust
// Mutate the Root from anywhere (no context needed)
Root::update(window, cx, |root, window, cx| { ... });

// Read-only access
let root = Root::read(window, cx);

// Render overlay layers (call in your window's render loop)
Root::render_notification_layer(window, cx);  // -> Option<impl IntoElement>
Root::render_sheet_layer(window, cx);         // -> Option<impl IntoElement>
Root::render_dialog_layer(window, cx);        // -> Option<impl IntoElement>
```

### WindowExt trait

`WindowExt` methods are convenience wrappers that delegate to `Root::update`.

```rust
use gpui_component::WindowExt;

window.open_dialog(cx, |dialog, window, cx| {
    dialog.title("About").child("v1.0")
});

window.open_sheet(cx, |sheet, window, cx| {
    sheet.child("Side panel")
});

window.push_notification(
    Notification::new("Saved").info(), cx
);
```

All `WindowExt` methods:

| Method                     | Purpose                             |
| -------------------------- | ----------------------------------- |
| `open_dialog(build)`       | Push a new dialog onto the stack    |
| `open_alert_dialog(build)` | Shorthand for `AlertDialog`         |
| `has_active_dialog()`      | Returns `bool`                      |
| `close_dialog()`           | Pop the topmost dialog              |
| `close_all_dialogs()`      | Clear the dialog stack              |
| `open_sheet(build)`        | Open a sheet (default: right edge)  |
| `open_sheet_at(placement)` | Open sheet at any `Placement`       |
| `has_active_sheet()`       | Returns `bool`                      |
| `close_sheet()`            | Close the active sheet              |
| `push_notification(note)`  | Add a notification to the stack     |
| `remove_notification::<T>()` | Remove notifications keyed by type |
| `remove_notification1::<T>(key)` | Remove one by type + key       |
| `clear_notifications()`    | Remove all notifications            |
| `notifications()`          | Returns `Rc<Vec<Entity<Notification>>>` |
| `focused_input()`          | Returns currently focused `InputState` entity |
| `selected_text()`          | Merged text selection across views  |
| `clear_text_selection()`   | Clear all text selections           |

### Struct internals (read-only context)

```
Root {
    view: AnyView,                          // Your app content
    active_sheet: Option<ActiveSheet>,      // One sheet at a time
    active_dialogs: Vec<ActiveDialog>,      // Dialog stack (LIFO)
    focused_input: Option<Entity<InputState>>, // Currently focused input
    notification: Entity<NotificationList>,
    tooltip_overlay: Entity<TooltipOverlay>,
    native_menu_overlay: Entity<FallbackMenuOverlay>,
    text_selection: WindowTextSelection,    // Window-level selection state
    selectable_text_views: HashMap<EntityId, ...>,
    pending_focus_restore: Option<WeakFocusHandle>, // Animation focus chain
    // ...
}
```

---

## 4. gpui-component-assets Integration

The `gpui-component-assets` crate provides the default SVG icon set (100+ icons)
as an `AssetSource`. Use `gpui_component_assets::Assets` with GPUI's asset system:

```rust
use gpui_component_assets::Assets;

fn main() {
    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        // ...
    });
}
```

### Custom assets

You can provide your own `AssetSource` instead. Derive `RustEmbed` and implement
`AssetSource`:

```rust
#[derive(rust_embed::RustEmbed)]
#[folder = "./assets"]
#[include = "icons/**/*.svg"]
pub struct MyAssets;

impl gpui::AssetSource for MyAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() { return Ok(None); }
        Self::get(path)
            .map(|f| Some(f.data))
            .ok_or_else(|| anyhow!("could not find asset at path \"{path}\""))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|p| p.starts_with(path).then(|| p.into()))
            .collect())
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(MyAssets);
    // ...
}
```

### Platform differences

| Platform   | How icons are served                     |
| ---------- | ---------------------------------------- |
| Desktop    | Embedded via `rust_embed::RustEmbed`     |
| WASM       | Downloaded on-demand from a CDN (cached) |

### How icons are discovered

`gpui-component-macros`'s `icon_named!` macro reads the directory path published
by `gpui-component-assets` via Cargo's `links` mechanism. Your `build.rs` must
re-export that path so the macro can enumerate available icons at compile time:

```rust
// In your crate's build.rs
fn main() {
    // Let the icon_named! macro discover icon SVGs from gpui-component-assets.
    println!("cargo:rustc-env=GPUI_COMPONENT_DEFAULT_ICONS_DIR={}",
        std::env::var("DEP_GPUI_COMPONENT_ASSETS_ICONS_DIR").unwrap());
}
```

This is **only needed if you use `icon_named!` to generate a custom `IconName`
enum**. When using the default `gpui_component::IconName`, the assets crate does
this automatically through Cargo's `links` propagation.

---

## 5. Platform-Specific Notes

### macOS

- Native menus (`NSMenu`) are driven through `objc2` / `objc2-app-kit`.
- Font defaults: `.SystemUIFont` for UI, `Menlo` for monospace.
- Traffic-light buttons are positioned at `(9px, 9px)` — use `TitleBar::title_bar_options()`.
- Window shadow is handled natively (`SHADOW_SIZE = 0px`).
- Copy action bound to `cmd-c`.

### Linux

- Client-side decorations (CSD) with a 12px drop shadow by default. You can
  adjust it via `Root::window_shadow_size()` or `WindowBorder::shadow_size()`.
- The close button on the title bar calls `window.remove_window()` by default;
  override with `TitleBar::on_close_window()` (Linux only).
- Font defaults: `.SystemUIFont` for UI, `DejaVu Sans Mono` for monospace.
- Copy action bound to `ctrl-c`.
- Use `gpui_platform` with features `x11` and/or `wayland`:
  ```toml
  gpui_platform = { workspace = true, features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
  ```

### Windows

- Native menus driven through Win32 (`windows` crate).
- Font defaults: `.SystemUIFont` for UI, `Consolas` for monospace.
- Window shadow is handled natively (`SHADOW_SIZE = 0px`).

### WASM

- Tree-sitter is unavailable (all tree-sitter dependencies are `[target.'cfg(not(target_family = "wasm"))']`).
- `smol` (async runtime) is native-only; WASM uses `wasm-bindgen` / browser APIs.
- Icons are fetched on-demand from a CDN, not embedded — reducing bundle size.
- Theme loads synchronously on startup for WASM compatibility (see `theme::init`).

---

## 6. WindowOptions Configuration

After `init()`, open windows with `cx.open_window()`:

```rust
cx.spawn(async move |cx| {
    cx.open_window(WindowOptions::default(), |window, cx| {
        // Build views, place Root as first child
    })
    .expect("Failed to open window");
})
.detach();
```

### Common options

```rust
let options = WindowOptions {
    window_bounds: Some(WindowBounds::centered(
        size(px(800.), px(600.)),
        cx,
    )),
    titlebar: Some(TitlebarOptions {
        title: Some("My App".into()),
        appears_transparent: true,
        traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
    }),
    ..Default::default()
};
```

For compatibility with `gpui_component::TitleBar`, use the pre-built options:

```rust
let options = WindowOptions {
    titlebar: Some(TitleBar::title_bar_options()),
    ..Default::default()
};
```

### Inside the window callback

```rust
cx.open_window(options, |window, cx| {
    let content = cx.new(|_| MyApp);  // Your app view
    cx.new(|cx| Root::new(content, window, cx))  // Root wraps it
})
```

`Root::new` places itself as the window's root view. You can chain style
refinements:

```rust
cx.new(|cx| {
    Root::new(content, window, cx)
        .bg(cx.theme().background)
})
```

---

## 7. Font Setup

### Default fonts

The `Theme` struct defines these defaults (platform-aware):

| Field               | macOS          | Windows    | Linux              |
| ------------------- | -------------- | ---------- | ------------------ |
| `font_family`       | `.SystemUIFont`| `.SystemUIFont` | `.SystemUIFont` |
| `mono_font_family`  | `Menlo`        | `Consolas` | `DejaVu Sans Mono` |
| `font_size`         | `16px`         | `16px`     | `16px`             |
| `mono_font_size`    | `13px`         | `13px`     | `13px`             |

### Overriding via Theme

```rust
use gpui_component::theme::Theme;

// After init(), mutate the global theme:
Theme::global_mut(cx).font_family = "Fira Sans".into();
Theme::global_mut(cx).font_size = px(14.);
Theme::global_mut(cx).mono_font_family = "Fira Code".into();
```

### The Root applies `font_family` and `font_size`

In its `render()` method, `Root` calls:

```rust
window.set_rem_size(cx.theme().font_size);
// …
.font_family(cx.theme().font_family.clone())
```

So all `rem`-based sizes throughout the component tree scale from the theme's
`font_size`. No manual `window.set_rem_size()` is needed — `Root` handles it.

### Embedded fonts

GPUI does not automatically load font files from disk. To ship custom fonts:

1. Use `gpui_platform` with `font-kit` for system font loading.
2. Register custom font paths via the platform's font database (platform-
   specific and outside the scope of this reference — see the `gpui_platform`
   docs).

> The `.SystemUIFont` keyword instructs GPUI to use the OS system UI font (San
> Francisco on macOS, Segoe UI on Windows, etc.).

---

## 8. Locale Setup (i18n)

`gpui-component` uses `rust-i18n` for internationalisation. The locale files
live in the `locales/` directory of the `gpui-component` crate:

```
locales/
  en.yml   (fallback)
  zh-CN.yml
  ...
```

### Reading the current locale

```rust
let current: &str = &*gpui_component::locale(); // e.g. "en" or "zh-CN"
```

### Setting the locale

```rust
gpui_component::set_locale("zh-CN");
```

Call this **after** `init()`. The locale change takes effect immediately for
all subsequent translation lookups via `rust_i18n::t!()`.

### The `i18n!` macro

In `lib.rs`, the crate declares:

```rust
rust_i18n::i18n!("locales", fallback = "en");
```

This tells `rust-i18n` to look for YAML files in the `locales/` directory
relative to the crate root (or relative to your binary's workspace root when
using `rust_i18n::i18n!` in your own code).

### Adding i18n to your own app

If your app needs its own translations, add:

```rust
// In your main.rs or lib.rs
rust_i18n::i18n!("locales", fallback = "en");
```

Then place YAML files under your `locales/` directory. Each file uses the
language code as the filename (e.g. `en.yml`, `de.yml`). The fallback language
serves any missing keys.

---

## 9. Common Initialization Mistakes

### ❌ Forgetting `gpui_component::init(cx)`

```rust
// BROKEN — components will panic due to missing globals/actions
fn main() {
    gpui_platform::application().run(|cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            cx.new(|cx| Root::new(content, window, cx))
        });
    });
}
```

**Fix:** Always call `init(cx)` before opening any windows.

### ❌ Not wrapping content in `Root`

```rust
// BROKEN — sheets, dialogs, notifications won't work
cx.open_window(WindowOptions::default(), |window, cx| {
    cx.new(|_| MyApp)
})
```

**Fix:** The first view in every window must be a `Root`:

```rust
cx.open_window(WindowOptions::default(), |window, cx| {
    let content = cx.new(|_| MyApp);
    cx.new(|cx| Root::new(content, window, cx))
})
```

### ❌ Calling `init()` more than once

Each call to `init()` re-registers keybindings and sets globals. Calling it
twice is a logic error (though GPUI will generally deduplicate actions). Keep
it at exactly one call point.

### ❌ Opening windows synchronously in `run()` callback

```rust
gpui_platform::application().run(|cx| {
    gpui_component::init(cx);
    // BROKEN — cannot open window in a non-async context
    cx.open_window(...);
});
```

**Fix:** Spawn an async task:

```rust
gpui_platform::application().run(move |cx| {
    gpui_component::init(cx);
    cx.spawn(async move |cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            // ...
        })
        .expect("Failed to open window");
    })
    .detach();
});
```

### ❌ Missing assets

If you use `IconName` elements (e.g. `<IconName::Inbox />`) without asset
registration, GPUI will fail to load the SVG:

```rust
// BROKEN — no AssetSource registered
let app = gpui_platform::application();
app.run(|cx| { /* ... */ });
```

**Fix:** Register `gpui_component_assets::Assets` or your own source:

```rust
let app = gpui_platform::application().with_assets(Assets);
```

### ❌ Using `gpui_component::Root` without the `gpui_component` import

```rust
// Root is re-exported from the crate root:
use gpui_component::Root;         // ✓
use gpui_component::root::Root;   // also works but verbose
```

### ❌ Adding `gpui-component-assets` as build dependency

Despite its name, the crate **must be a regular runtime dependency** — not a
build dependency. Cargo's `links` mechanism propagates the icon directory path
from `gpui-component-assets`'s `build.rs` to `gpui-component`'s `build.rs`
through `DEP_<links>_<key>` environment variables. These only work through
regular `[dependencies]`, not `[build-dependencies]`.

### ❌ Not setting `edition = "2024"`

`gpui-component` uses Rust edition 2024. Your crate must also use edition 2024
(or at minimum be compatible with async closures and RPITIT changes):

```toml
[package]
edition = "2024"
```

---

## 10. Minimal Working Example

```rust
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;

struct AppContent;
impl Render for AppContent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Hello gpui-component!")
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let content = cx.new(|_| AppContent);
                cx.new(|cx| Root::new(content, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
```

### Required Cargo.toml for the minimal example

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1"
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
gpui-component = "0.5"
gpui-component-assets = "0.5"
```
