---
name: gpui-dev
description: GPUI framework and gpui-component UI library knowledge. Covers framework fundamentals (Actions, Async tasks, Context management, Entity state, Events, Focus, ElementId) and gpui-component components (Button, Input, Select, Dialog, Tabs, Sidebar, List, Table, Chart, Dock, Slider, Switch, Toggle, Combobox, etc.). Use when building any GPUI application — from framework-level concepts (actions, keybindings, async I/O, entity lifecycle, re-entrancy) to UI components and theming. Make sure to use this skill whenever working with GPUI, gpui-component, or any Rust UI framework based on GPUI — even if the user doesn't explicitly mention GPUI. Also use when the user asks about gpui_component::init, Root, WindowExt, or component patterns.
---

## Framework Fundamentals

Load these when working with GPUI framework concepts (not gpui-component library):

| Topic | File | When to load |
|-------|------|--------------|
| Actions & Keybindings | [framework-actions.md](references/framework-actions.md) | `actions!`, `bind_keys`, `on_action`, `key_context` |
| Async & Background Tasks | [framework-async.md](references/framework-async.md) | `cx.spawn`, `background_spawn`, `Task`, async I/O |
| Context Management | [framework-context.md](references/framework-context.md) | `App`, `Window`, `Context<T>`, `AsyncApp`, `cx.listener` |
| Entity State | [framework-entity.md](references/framework-entity.md) | `Entity<T>`, `WeakEntity`, re-entrancy, `defer_in` pitfalls |
| Events & Subscriptions | [framework-events.md](references/framework-events.md) | `cx.emit`, `cx.subscribe`, `cx.observe`, `observe_global` |
| Focus & Keyboard Nav | [framework-focus.md](references/framework-focus.md) | `FocusHandle`, `track_focus`, Tab navigation |
| ElementId | [framework-element-id.md](references/framework-element-id.md) | `.id()`, `Stateful<Div>`, uniqueness rules |


## Loading Strategy

**Start with the component library** for most tasks — it contains the practical patterns you'll use daily. Load framework references only when you hit framework-level concepts (entity lifecycle, async I/O, keybindings, focus management).

**Decision flow:**
1. User asks about a specific component (Button, Input, Dialog, Table)? → Load the matching **Component Library** file.
2. User asks about a pattern (settings page, sidebar with list)? → Load **Common Patterns**.
3. User asks about entity lifecycle, async tasks, keybindings, or focus? → Load the matching **Framework** file.
4. User asks about setup or initialization? → Load **quick-start.md** or **setup.md**.
## Component Library

Load the relevant reference file based on the task:

| Topic | File | When to load |
|-------|------|--------------|
| Quick Start / Setup | [quick-start.md](references/quick-start.md) | `init(cx)`, `Root`, stateful vs stateless, sizes, theme colors, overlay layers |
| Input & Form | [input-form.md](references/input-form.md) | Input, InputState, InputEvent, Select, Combobox, Checkbox, Switch, Slider, Form |
| Display & Feedback | [display-feedback.md](references/display-feedback.md) | Button, IconName, Alert, Badge, Tooltip, Spinner, Progress |
| Dialog & Overlay | [dialog.md](references/dialog.md) | Dialog, AlertDialog, Sheet, Notification, `window.open_dialog()`, WindowExt |
| Navigation & Layout | [navigation-layout.md](references/navigation-layout.md) | Tabs, TabBar, Sidebar, Dock, Resizable, Scrollable |
| Data Display | [data.md](references/data.md) | Table, List, ListDelegate, Tree, VirtualList, Charts, IndexPath |
| Theming | [theming.md](references/theming.md) | Theme, ThemeColor, ThemeRegistry, font setup, i18n |
| Traits & Custom Components | [traits.md](references/traits.md) | Sizable, Disableable, StyledExt, RenderOnce, building custom components |

## Common Patterns

Self-contained copy-paste starting points for the most frequent tasks:

| Pattern | File | Covers |
|---------|------|--------|
| Dialog + Input | [patterns.md](references/patterns.md#dialog--input) | `window.open_dialog()` with `InputState`, `on_ok` reading value, overlay layers |
| Tabs + Toggle + Slider | [patterns.md](references/patterns.md#tabs--toggle--slider) | TabBar with Tab, Toggle for theme, stateful SliderState |
| Sidebar + List | [patterns.md](references/patterns.md#sidebar--list-with-delegate) | Sidebar with ListDelegate, IndexPath usage, ListState |

## External Reference

For exhaustive per-component API documentation, fetch the full LLM reference:
- **Full docs**: `https://longbridge.github.io/gpui-component/llms-full.txt`
- **Per-component**: `https://longbridge.github.io/gpui-component/docs/components/{name}.md`

## Critical Rules

These are non-negotiable. Violating any of them causes silent failures or panics:

1. `gpui_component::init(cx)` must be called exactly once in `app.run()`, before any window opens.
2. Every window's first view must be wrapped: `cx.new(|cx| Root::new(view, window, cx))`.
3. Every window's render must include the overlay layers: `Root::render_dialog_layer(cx)`, `Root::render_sheet_layer(cx)`, `Root::render_notification_layer(cx)`.
4. Stateful components (Input, Select, Slider, List, Table) require `Entity<State>` in your struct, created via `cx.new(|cx| State::new(window, cx))`.
5. Overlays (Dialog, Sheet, Notification) open via `WindowExt` methods on `window`: `window.open_dialog(cx, ...)`, never by rendering them directly.
