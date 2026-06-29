# Quality Guidelines

> GPUI patterns, code standards, and anti-patterns for the zenix Rust codebase.

---

## Overview

Zenix is a GPUI desktop app. Quality standards center on correct GPUI pattern usage, no-unwrap policy, and clean module boundaries.

---

## GPUI Patterns (MANDATORY)

### Entity / Context / Action Model

All application state lives in `Entity` types. State changes happen via `Action` enums handled in `Context` methods.

```rust
// Actions are PascalCase enums
pub enum TerminalAction {
    SetCursorStyle(String),
    ToggleCursorBlink,
    SetOpacity(f32),
    SetScrollback(u32),
}

pub enum McpAction {
    ShowAddForm,
    SubmitAddForm,
    CancelAddForm,
    RemoveZenix { id: usize },
    EditZenix { id: usize },
    TestServer { id: usize },
    ToggleZenixAgent { id: usize },
    SetFormType { agent_name: String },
}
```

### Render is Pure — No State Mutation

**`render()` must be a pure function of state.** Never mutate entity state inside `render()`.

```rust
// ❌ Bad — mutating state in render
fn render(&mut self, ...) {
    self.settings.open = true; // NO!
}

// ✅ Good — state changes via actions
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // read self.settings.open, build UI elements
}
```

Use action handlers (via `ActionCb<T>` callbacks or GPUI `Action` trait) for state changes.

### Async via `cx.spawn()`

Never block the GPUI event loop. All async work goes through `cx.spawn()`:

```rust
cx.spawn(move |this, mut cx| async move {
    // async work here
    cx.update(|cx| { /* modify state */ })
}).detach();
```

### FocusHandle for Keyboard/Input Focus

Use `FocusHandle` to manage which element receives keyboard input. Define focus handles in the entity:

```rust
focus: FocusHandle,
// ...
let focus = cx.focus_handle();
```

---

## Code Standards

### No `unwrap()` in Production

Hard rule. Use `anyhow::Context` / `?` operator everywhere.

```rust
// ❌
let config = ConfigStore::load().unwrap();

// ✅
let config = ConfigStore::load().context("load config")?;
```

### Platform-Gated Code

**Ideal**: All `#[cfg(target_os = "...")]` blocks go in `src/platform.rs`. Do not scatter platform checks across files.

**Reality**: Some modules have legitimate platform checks (`main.rs` for Linux icon, `single_instance.rs` for Unix/Windows IPC, `terminal/mod.rs` for PTY, `config/mod.rs` for font detection). The rule is: when adding **new** platform-specific code, prefer `platform.rs`. Existing scattered `#[cfg]` in legacy modules is documented but not retroactively refactored.

```rust
// ❌ Bad — platform check in app.rs
#[cfg(target_os = "linux")]
use foo;

// ✅ Good — centralized in platform.rs
// src/platform.rs
#[cfg(target_os = "linux")]
pub fn get_icon_path() -> PathBuf { ... }
```

### Theme Provider Tokens

Never hardcode theme colors. Always go through the theme provider:

```rust
// ❌ Bad
Color::rgb(0.2, 0.2, 0.2)

// ✅ Good — use theme token
theme.colors().background
```

### Dependency Awareness

Before adding a new dependency, check these first:
1. `gpui` — does GPUI already provide this?
2. `gpui-component` — does the component library have it?
3. `alacritty_terminal` — does the terminal crate have it?

---

## Testing & Quality Commands

```bash
cargo check            # Type check (fast, do frequently)
cargo fmt --check      # Format check
cargo clippy           # Lint (use before committing)
cargo build --release  # Full release build
```

### Formatting

Rustfmt handles formatting. Run `cargo fmt` before committing. The project uses standard Rust formatting — no custom fmt config.

### Clippy

Run `cargo clippy` before committing. Fix all warnings. The project treats clippy warnings as errors in practice.

---

## Anti-Patterns (THIS PROJECT)

| Anti-Pattern | Why | Correct Approach |
|-------------|-----|------------------|
| `unwrap()` in production | Runtime crash risk | `.context("msg")?` + `anyhow::Result` |
| Mutating state in `render()` | Breaks GPUI purity | Use action handlers / callbacks |
| Platform `#[cfg]` outside `platform.rs` | Scatters platform logic | Centralize in `platform.rs` |
| Hardcoded colors | Breaks theme switching | Use theme provider tokens |
| Adding deps without checking gpui/gpui-component/alacritty_terminal first | Bloats binary, may conflict | Check existing deps first |
| Direct `cargo run` without herdr | Missing herdr binary causes runtime failure | Use `just` or `HERDR_BUILD=1` |
| Editing vendored `vendor/gpui_wgpu_patched/` without patch index | Breaks wgpu rendering | Always update patch index |

---

## Configuration Conventions

### Config Store (`src/config/mod.rs`)

- Default values via `fn default_*() -> T` functions
- `ConfigStore::load()` reads `~/.config/zenix/config.json`
- `ConfigStore::save()` writes back atomically
- Config format: JSONC (JSON with comments), parsed via `serde_json` with a custom comment-stripping preprocessor in `config/jsonc.rs`
- The `toml` crate is declared as a dependency but **not used for config parsing**

### Module Structure for Config

```
src/config/
├── mod.rs       # ConfigStore struct, load/save, defaults
├── jsonc.rs     # JSON-with-comments parser
└── scanner.rs   # ConfigMap (field-preserving read/write)
```

---

## I18n

Chinese/English via `rust-i18n`:
- Add locale keys in `src/i18n.rs`
- Add translations in `.po` files under `locales/`
- Use `rust_i18n::i18n!("locales")` in `main.rs` and `lib.rs`
