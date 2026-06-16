# Input & Form Components

Comprehensive reference for gpui-component input, selection, and form components.

---

## InputState

**File:** `crates/ui/src/input/state.rs`

`InputState` is the state entity for text input fields. It manages text content, cursor/selection, editing modes, LSP integration, undo history, and rendering.

### Constructor

```rust
impl InputState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;
}
```

Creates a new `InputState` with default `PlainText` (single-line) mode. The constructor:
- Creates a focus handle with `tab_stop(true)`.
- Sets up a `BlinkCursor`, undo `History`, and internal subscriptions.
- Initializes the `DisplayMap` for text layout.

### Modes

Three modes control the editing behavior:

#### `single_line` (default)
The default mode — a single-line text field.

```rust
// Explicit (no-op for new instances, but available):
state = state.multi_line(false);
```

#### `multi_line`
Multi-line text area. Default rows = 2.

```rust
pub fn multi_line(mut self, multi_line: bool) -> Self;
```

#### `auto_grow(min_rows, max_rows)`
Multi-line that grows/shrinks within bounds.

```rust
pub fn auto_grow(mut self, min_rows: usize, max_rows: usize) -> Self;
```

#### `code_editor(language)`
Full code editor mode with syntax highlighting, line numbers, folding, indent guides.
Default options: `line_number: true`, `tab_size: 2`, `multi_line: true`, `indent_guides: true`.

```rust
pub fn code_editor(mut self, language: impl Into<SharedString>) -> Self;
```

Enables `searchable` by default. Supports up to 50K lines.

### Builder Methods (consume self, return Self)

| Method | Signature | Description |
|--------|-----------|-------------|
| `placeholder` | `fn placeholder(self, placeholder: impl Into<SharedString>) -> Self` | Placeholder text shown when empty |
| `disabled` | `fn disabled(self, disabled: bool) -> Self` | Disable editing (pub(crate)) |
| `masked` | `fn masked(self, masked: bool) -> Self` | Password mask (single-line only) |
| `searchable` | `fn searchable(self, searchable: bool) -> Self` | Enable Ctrl+F search (multi-line only) |
| `replaceable` | `fn replaceable(self, allow: bool) -> Self` | Enable replacement in search UI (default true) |
| `rows` | `fn rows(self, rows: usize) -> Self` | Set number of visible rows for multi-line |
| `folding` | `fn folding(self, folding: bool) -> Self` | Enable code folding (code editor only, default true) |
| `line_number` | `fn line_number(self, line_number: bool) -> Self` | Show line numbers (code editor only) |
| `context_menu` | `fn context_menu(self, enable: bool) -> Self` | Enable right-click context menu (default true) |
| `clean_on_escape` | `fn clean_on_escape(self) -> Self` | Pressing Escape clears the input |
| `submit_on_enter` | `fn submit_on_enter(self, submit: bool) -> Self` | Enter emits PressEnter instead of newline in multi-line |
| `soft_wrap` | `fn soft_wrap(self, wrap: bool) -> Self` | Enable soft wrapping (multi-line, default true) |
| `show_whitespaces` | `fn show_whitespaces(self, show: bool) -> Self` | Show whitespace indicators |
| `scroll_beyond_last_line` | `fn scroll_beyond_last_line(self, rows: Option<usize>) -> Self` | Extra space below last line (code editor) |
| `cursor_surrounding_lines` | `fn cursor_surrounding_lines(self, lines: Option<usize>) -> Self` | Min lines cursor is clear of viewport edges |
| `pattern` | `fn pattern(self, pattern: regex::Regex) -> Self` | Regex validation pattern (single-line) |
| `validate` | `fn validate(self, f: impl Fn(&str, &mut Context<Self>) -> bool + 'static) -> Self` | Custom validation function (single-line) |
| `default_value` | `fn default_value(self, value: impl Into<SharedString>) -> Self` | Set initial text content |
| `mask_pattern` | `fn mask_pattern(self, pattern: impl Into<MaskPattern>) -> Self` | Input mask pattern |

### Runtime Setters (take `&mut self`)

| Method | Signature | Description |
|--------|-----------|-------------|
| `set_value` | `fn set_value(&mut self, value: impl Into<SharedString>, window, cx)` | Replace entire text; resets selection and history |
| `set_placeholder` | `fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, window, cx)` | Update placeholder text |
| `set_highlighter` | `fn set_highlighter(&mut self, new_language: impl Into<SharedString>, cx)` | Change code editor language |
| `set_masked` | `fn set_masked(&mut self, masked: bool, window, cx)` | Toggle password mask |
| `set_disabled` | (via direct `state.disabled = true; cx.notify()`) | No dedicated setter; modify the public `disabled` field directly |
| `set_soft_wrap` | `fn set_soft_wrap(&mut self, wrap: bool, window, cx)` | Toggle soft wrap |
| `set_show_whitespaces` | `fn set_show_whitespaces(&mut self, show: bool, window, cx)` | Toggle whitespace rendering |
| `set_scroll_beyond_last_line` | `fn set_scroll_beyond_last_line(&mut self, rows: Option<usize>, window, cx)` | Update scroll-beyond-last-line |
| `set_cursor_surrounding_lines` | `fn set_cursor_surrounding_lines(&mut self, lines: Option<usize>, window, cx)` | Update cursor margin |
| `set_pattern` | `fn set_pattern(&mut self, pattern: regex::Regex, window, cx)` | Update regex validation |
| `set_loading` | `fn set_loading(&mut self, loading: bool, window, cx)` | Show/hide spinner (single-line) |
| `set_mask_pattern` | `fn set_mask_pattern(&mut self, pattern: impl Into<MaskPattern>, window, cx)` | Update mask pattern |
| `set_folding` | `fn set_folding(&mut self, folding: bool, window, cx)` | Toggle code folding |
| `set_line_number` | `fn set_line_number(&mut self, line_number: bool, window, cx)` | Toggle line numbers |
| `set_cursor_position` | `fn set_cursor_position(&mut self, position: impl Into<Position>, window, cx)` | Move cursor and focus |
| `set_scroll_offset` | `fn set_scroll_offset(&mut self, offset: Point<Pixels>, cx)` | Programmatic scroll |
| `unselect` | `fn unselect(&mut self, window, cx)` | Clear text selection |

### Value Access

```rust
pub fn value(&self) -> SharedString;             // Full text value
pub fn selected_value(&self) -> SharedString;    // Selected portion only
pub fn unmask_value(&self) -> SharedString;      // Value with mask separators stripped
pub fn text(&self) -> &Rope;                     // Raw rope reference
pub fn cursor(&self) -> usize;                   // Cursor byte offset
pub fn cursor_position(&self) -> Position;       // Cursor as line/column
pub fn selected_range(&self) -> Range<usize>;    // Byte range of selection
pub fn visible_row_range(&self) -> Option<Range<usize>>;  // Visible line range
pub fn scroll_offset(&self) -> Point<Pixels>;    // Current scroll offset
pub fn line_height(&self) -> Option<Pixels>;     // Laid-out line height
pub fn diagnostics(&self) -> Option<&DiagnosticSet>;
```

### Text Mutation

```rust
pub fn set_value(&mut self, value: impl Into<SharedString>, window, cx);
pub fn insert(&mut self, text: impl Into<SharedString>, window, cx);   // Insert at cursor
pub fn replace(&mut self, text: impl Into<SharedString>, window, cx);  // Replace at cursor
pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>);
pub fn range_to_bounds(&self, range: &Range<usize>) -> Option<Bounds<Pixels>>;
```

### Events

```rust
pub enum InputEvent {
    Change,                              // Text content changed
    PressEnter { secondary: bool, shift: bool },  // Enter pressed
    Focus,                               // Gained focus
    Blur,                                // Lost focus
}
```

`InputState` implements `EventEmitter<InputEvent>`.

### Subscribe Pattern

The standard GPUI subscription pattern for observing input changes:

```rust
let subscription = cx.subscribe_in(
    &input_state,
    window,
    |this, _editor, event: &InputEvent, window, cx| {
        match event {
            InputEvent::Change => {
                let value = this_input.read(cx).value();
                // Handle change...
            }
            InputEvent::PressEnter { shift, .. } => {
                // Handle enter...
            }
            InputEvent::Focus => { /* ... */ }
            InputEvent::Blur => { /* ... */ }
        }
    },
);
```

Store subscriptions in your state struct as `_subscriptions: Vec<Subscription>`.

### Focus

```rust
// InputState implements Focusable
state.focus(window, cx);           // Programmatic focus
state.focus_handle.focus(window, cx);
```

### LSP Integration

The `Lsp` struct on `InputState` holds providers:

```rust
pub struct Lsp {
    pub completion_provider: Option<Rc<dyn CompletionProvider>>,
    pub code_action_providers: Vec<Rc<dyn CodeActionProvider>>,
    pub hover_provider: Option<Rc<dyn HoverProvider>>,
    pub definition_provider: Option<Rc<dyn DefinitionProvider>>,
    pub document_color_provider: Option<Rc<dyn DocumentColorProvider>>,
    pub semantic_tokens_provider: Option<Rc<dyn DocumentRangeSemanticTokensProvider>>,
}
```

#### Provider Traits

- **`CompletionProvider`** — provides `completions(text, offset, trigger, window, cx) -> Task<Result<CompletionResponse>>` and optional `inline_completion(...)`.
- **`CodeActionProvider`** — provides code actions for a range.
- **`HoverProvider`** — provides hover information at an offset.
- **`DefinitionProvider`** — provides "go to definition" locations.
- **`DocumentColorProvider`** — provides document color ranges.
- **`DocumentRangeSemanticTokensProvider`** — provides semantic token ranges.

Set providers after construction via `InputState.lsp` field access:

```rust
state.update(cx, |state, cx| {
    state.lsp.completion_provider = Some(Rc::new(my_provider));
    cx.notify();
});
```

LSP features are code editor mode only. Key capabilities:
- **Completions:** Shown in a popover below the cursor; navigated with arrow keys.
- **Code Actions:** Shown in a popover; triggered via Ctrl+. or context menu.
- **Hover:** Diagnostic and hover info on hover/right-click.
- **Go to Definition:** Ctrl+Click or context menu.
- **Inline Completions:** Ghost text suggestions that are debounced.

#### Applying LSP Edits

```rust
state.apply_lsp_edits(&text_edits, window, cx);
```

### Search/Replace Panel

Enabled when `searchable(true)` is set (multi-line only). Opened via Ctrl+F.

```rust
// Builder:
state.searchable(true)          // Enable search (default for code editor)
      .replaceable(true);       // Enable replace UI (default true)

// Runtime control:
state.search_panel              // Option<Entity<SearchPanel>>
// The panel is created lazily when Ctrl+F is pressed.
```

### Mask Pattern

Regex-based input formatting for structured input.

```rust
// Builder:
state.mask_pattern(MaskPattern::new("(999) 999-9999"));

// Runtime:
state.set_mask_pattern(MaskPattern::new("999-999-9999"), window, cx);

// Get value without mask separators:
let raw = state.unmask_value();  // e.g. "1234567890" from "(123) 456-7890"
```

Token reference:
- `9` — Digit (`[0-9]`)
- `A` — Letter (`[a-zA-Z]`)
- `#` — Letter or digit (`[a-zA-Z0-9]`)
- `*` — Any character
- Other — Literal separator characters

### Code Editor Features (Summary)

| Feature | Builder | Notes |
|---------|---------|-------|
| Syntax highlighting | `code_editor("rust")` | Uses tree-sitter via `SyntaxHighlighter` |
| Line numbers | `line_number(true)` | Default on; `set_line_number()` at runtime |
| Code folding | `folding(true)` | Default on; click fold indicators to collapse ranges |
| Indent guides | (default on) | Visual indent level guides |
| Large text | (built-in) | Tested up to 50K lines |
| Soft wrap | `soft_wrap(true)` | Default on; `set_soft_wrap()` at runtime |
| Scroll beyond last line | `scroll_beyond_last_line(Some(5))` | Extra space below content |
| Undo/Redo | (built-in) | Ctrl+Z / Ctrl+Shift+Z |
| Context menu | `context_menu(true)` | Right-click menu with Cut/Copy/Paste/Select All |
| Tab size | (default: 2) | Configurable per mode |
| Indent | (built-in) | Auto-indent on Enter in code editor mode |

---

## Input (Element)

**File:** `crates/ui/src/input/input.rs`

The visual element that renders an `InputState`. Derives `IntoElement`.

```rust
#[derive(IntoElement)]
pub struct Input {
    // fields: state, style, size, prefix, suffix, height, appearance,
    //          cleanable, mask_toggle, disabled, bordered, focus_bordered,
    //          tab_index, selected, context_menu_builder
}
```

### Constructor

```rust
impl Input {
    pub fn new(state: &Entity<InputState>) -> Self;
}
```

### Builder Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `prefix` | `fn prefix(self, element: impl IntoElement) -> Self` | Element shown before the text field |
| `suffix` | `fn suffix(self, element: impl IntoElement) -> Self` | Element shown after the text field |
| `cleanable` | `fn cleanable(self, cleanable: bool) -> Self` | Show clear button when not empty |
| `disabled` | `fn disabled(self, disabled: bool) -> Self` | Disable the input |
| `mask_toggle` | `fn mask_toggle(self) -> Self` | Show eye icon to toggle password mask |
| `appearance` | `fn appearance(self, appearance: bool) -> Self` | Border/background (false = minimal) |
| `bordered` | `fn bordered(self, bordered: bool) -> Self` | Show border (default true) |
| `focus_bordered` | `fn focus_bordered(self, bordered: bool) -> Self` | Show border on focus (default true) |
| `tab_index` | `fn tab_index(self, index: isize) -> Self` | Tab order (default 0) |
| `h_full` | `fn h_full(self) -> Self` | Full height (multi-line only) |
| `h` | `fn h(self, height: impl Into<DefiniteLength>) -> Self` | Set height (multi-line only) |
| `context_menu` | `fn context_menu(self, f: impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static) -> Self` | Custom right-click context menu |

### Traits

- `Sizable` — `with_size(Size)` for XSmall/Small/Medium/Large
- `Selectable` — `selected(bool)` for visual selection state
- `Styled` — `style()` for StyleRefinement access
- `RenderOnce` — renders the complete input element

---

## NumberInput

**File:** `crates/ui/src/input/number_input.rs`

A number input with increment/decrement buttons wrapping an `InputState`.

```rust
#[derive(IntoElement)]
pub struct NumberInput {
    state: Entity<InputState>,
    placeholder: SharedString,
    size: Size,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    appearance: bool,
    disabled: bool,
    style: StyleRefinement,
}
```

### Constructor

```rust
impl NumberInput {
    pub fn new(state: &Entity<InputState>) -> Self;
}
```

### Builder Methods

| Method | Signature |
|--------|-----------|
| `placeholder` | `fn placeholder(self, placeholder: impl Into<SharedString>) -> Self` |
| `prefix` | `fn prefix(self, element: impl IntoElement) -> Self` |
| `suffix` | `fn suffix(self, element: impl IntoElement) -> Self` |
| `appearance` | `fn appearance(self, appearance: bool) -> Self` |

### Events

```rust
pub enum NumberInputEvent {
    Step(StepAction),
}
pub enum StepAction {
    Decrement,
    Increment,
}
```

`InputState` implements `EventEmitter<NumberInputEvent>` for this context.
Arrow keys (Up/Down) trigger `Increment`/`Decrement` under the `"NumberInput"` key context.

### Traits

- `Disableable` — `disabled(bool)`
- `Sizable` — `with_size(Size)`
- `Styled` — `style()`
- `Focusable` — delegates to inner InputState
- `RenderOnce`

---

## OtpInput

**File:** `crates/ui/src/input/otp_input.rs`

A one-time password input with fixed-length digit input and optional masking.

### OtpState

```rust
pub struct OtpState {
    focus_handle: FocusHandle,
    value: SharedString,
    blink_cursor: Entity<BlinkCursor>,
    masked: bool,
    length: usize,
    input_state: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
```

#### Constructor

```rust
impl OtpState {
    pub fn new(length: usize, window: &mut Window, cx: &mut Context<Self>) -> Self;
}
```

#### Builder / Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `default_value` | `fn default_value(self, value: impl Into<SharedString>) -> Self` | Initial value |
| `masked` | `fn masked(self, masked: bool) -> Self` | Mask characters |
| `set_value` | `fn set_value(&mut self, value: impl Into<SharedString>, window, cx)` | Update value |
| `set_masked` | `fn set_masked(&mut self, masked: bool, window, cx)` | Toggle masking |
| `value` | `fn value(&self) -> &SharedString` | Current value |
| `focus` | `fn focus(&self, window, cx)` | Focus the OTP input |

#### Events

Emits `InputEvent::{Change, Focus, Blur}`. `Change` fires when all positions are filled.

Uses a shadow `InputState` written into `Root::focused_input` while OTP has focus, so the system clipboard (Ctrl+V) works — the shadow state is never rendered, keeping focus tree clean.

### OtpInput (Element)

```rust
#[derive(IntoElement)]
pub struct OtpInput {
    state: Entity<OtpState>,
    number_of_groups: usize,  // default: 2
    size: Size,
    disabled: bool,
}
```

#### Constructor / Builder

```rust
impl OtpInput {
    pub fn new(state: &Entity<OtpState>) -> Self;
    pub fn groups(self, n: usize) -> Self;  // Number of slot groups for visual spacing
}
```

#### Traits

- `Disableable` — `disabled(bool)`
- `Sizable` — `with_size(Size)`
- `RenderOnce`

---

## Select

**File:** `crates/ui/src/select.rs`

A dropdown select component with search, keyboard navigation, and item selection.

### Back-Compat Aliases

```rust
pub use crate::searchable_list::SearchableListDelegate as SelectDelegate;
pub use crate::searchable_list::SearchableListItem as SelectItem;
pub use crate::searchable_list::SearchableListItemElement as SelectListItem;
pub use crate::searchable_list::SearchableVec;
```

### SelectState

```rust
pub struct SelectState<D: SearchableListDelegate + 'static>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    state: SearchableListState<D>,
    searchable: bool,
    icon: Option<Icon>,
    title_prefix: Option<SharedString>,
}
```

#### Constructor

```rust
impl<D> SelectState<D> {
    pub fn new(
        delegate: D,
        selected_index: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self;
}
```

#### Builder / Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `searchable` | `fn searchable(self, searchable: bool) -> Self` | Show search input in dropdown |
| `set_selected_index` | `fn set_selected_index(&mut self, ix: Option<IndexPath>, window, cx)` | Set selection by index |
| `set_selected_value` | `fn set_selected_value(&mut self, value: &Value, window, cx)` | Set selection by value |
| `set_items` | `fn set_items(&mut self, items: D, window, cx)` | Replace delegate items |
| `selected_index` | `fn selected_index(&self, cx: &App) -> Option<IndexPath>` | Current selected index |
| `selected_value` | `fn selected_value(&self) -> Option<&Value>` | Current selected value |
| `focus` | `fn focus(&self, window, cx)` | Focus the trigger |

#### Events

```rust
pub enum SelectEvent<D: SearchableListDelegate + 'static> {
    Confirm(Option<<D::Item as SearchableListItem>::Value>),
}
```

Emits `SelectEvent::Confirm` when the user selects an item and the popover closes. Also emits `DismissEvent` on close without selection.

Keyboard: Arrow keys + Enter to open and navigate; Escape to close and cancel.

### Select (Element)

```rust
#[derive(IntoElement)]
pub struct Select<D: SearchableListDelegate + 'static> { /* ... */ }
```

#### Constructor / Builder

```rust
impl<D> Select<D> {
    pub fn new(state: &Entity<SelectState<D>>) -> Self;
    pub fn menu_width(self, width: impl Into<Length>) -> Self;
    pub fn menu_max_h(self, max_h: impl Into<Length>) -> Self;       // default: 20rem
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn icon(self, icon: impl Into<Icon>) -> Self;                // override chevron
    pub fn title_prefix(self, prefix: impl Into<SharedString>) -> Self;  // e.g. "Country: "
    pub fn cleanable(self, cleanable: bool) -> Self;
    pub fn search_placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn appearance(self, appearance: bool) -> Self;               // border/background
    pub fn empty<E: IntoElement + 'static>(
        self,
        builder: impl Fn(&mut Window, &App) -> E + 'static,
    ) -> Self;                                                       // custom empty state
}
```

#### Traits

- `Sizable` — `with_size(Size)`
- `Styled` — `style()`
- `RenderOnce`

#### Delegate Pattern

The `SelectDelegate` (alias for `SearchableListDelegate`) trait provides items for the dropdown:

```rust
// Implement SearchableListDelegate for your type:
// Required: fn item_count(&self) -> usize;
// Required: fn item(&self, ix: IndexPath) -> Option<D::Item>;
// Required: fn label(&self, ix: IndexPath) -> &str;
// Optional: fn on_confirm, on_will_change, position, filter, etc.
```

---

## Combobox

**File:** `crates/ui/src/combobox.rs`

A multi-select or single-select combobox with customizable trigger, search, and footer.

### ComboboxState

```rust
pub struct ComboboxState<D: SearchableListDelegate + 'static>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    state: SearchableListState<D>,
    multiple: bool,          // Multi-select mode
    searchable: bool,
    trigger_icon: Option<Icon>,
    check_icon: Option<Icon>,
    render_trigger: Option<Box<dyn Fn(...) -> AnyElement + 'static>>,
    footer: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
}
```

#### Constructor

```rust
impl<D> ComboboxState<D> {
    pub fn new(
        delegate: D,
        selected_indices: Vec<IndexPath>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self;
}
```

#### Builder / Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `multiple` | `fn multiple(self, multiple: bool) -> Self` | Enable multi-select (default: false) |
| `searchable` | `fn searchable(self, searchable: bool) -> Self` | Enable search input in dropdown |
| `selected_values` | `fn selected_values(&self) -> Vec<Value>` | All currently selected values |
| `selected_value` | `fn selected_value(&self) -> Option<Value>` | First selected value (single-select convenience) |
| `selection` | `fn selection(&self) -> &[(IndexPath, D::Item)]` | All selected pairs |
| `set_selected_indices` | `fn set_selected_indices(&mut self, indices: impl IntoIterator<Item=IndexPath>, window, cx)` | Replace selection |
| `add_selected_index` | `fn add_selected_index(&mut self, index: IndexPath, cx) -> bool` | Add one item |
| `remove_selected_index` | `fn remove_selected_index(&mut self, index: IndexPath, cx) -> bool` | Remove one item |
| `clear_selection` | `fn clear_selection(&mut self, cx)` | Clear all |
| `set_items` | `fn set_items(&mut self, items: D, window, cx)` | Replace delegate |
| `focus` | `fn focus(&self, window, cx)` | Focus the trigger |

#### Events

```rust
pub enum ComboboxEvent<D: SearchableListDelegate + 'static> {
    Change(Vec<Value>),   // Emitted on every toggle
    Confirm(Vec<Value>),  // Emitted when the popover closes
}
```

### Combobox (Element)

```rust
#[derive(IntoElement)]
pub struct Combobox<D: SearchableListDelegate + 'static> { /* ... */ }
```

#### Constructor / Builder

```rust
impl<D> Combobox<D> {
    pub fn new(state: &Entity<ComboboxState<D>>) -> Self;
    pub fn menu_width(self, width: impl Into<Length>) -> Self;
    pub fn menu_max_h(self, max_h: impl Into<Length>) -> Self;
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn icon(self, icon: impl Into<Icon>) -> Self;              // trigger chevron
    pub fn check_icon(self, icon: impl Into<Icon>) -> Self;        // selected item check
    pub fn search_placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn cleanable(self, cleanable: bool) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn appearance(self, appearance: bool) -> Self;
    pub fn empty<E: IntoElement + 'static>(
        self,
        builder: impl Fn(&mut Window, &App) -> E + 'static,
    ) -> Self;
    pub fn render_trigger<E: IntoElement + 'static>(
        self,
        f: impl Fn(&ComboboxTriggerCtx<D>, &mut Window, &mut App) -> E + 'static,
    ) -> Self;                                                     // fully custom trigger
    pub fn footer<E: IntoElement + 'static>(
        self,
        f: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self;                                                     // footer below dropdown list
}
```

#### Single vs Multi-Select

- **Single-select** (`multiple(false)`, default): Clicking an item deselects previous, closes popover, emits `Confirm`.
- **Multi-select** (`multiple(true)`): Clicking toggles items, popover stays open, emits `Change` on each toggle and `Confirm` on close.

#### Traits

- `Sizable` — `with_size(Size)`
- `Styled` — `style()`
- `RenderOnce`

---

## Checkbox

**File:** `crates/ui/src/checkbox.rs`

A standalone checkbox with label and click handler. No separate state entity — state is managed via `use_keyed_state` internally.

```rust
#[derive(IntoElement)]
pub struct Checkbox {
    // id, base(Div), style, label, children, checked, disabled,
    // size, tab_stop, tab_index, on_click, tooltip
}
```

### Constructor

```rust
impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self;
}
```

### Builder Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `tooltip` | `fn tooltip(self, text: impl Into<SharedString>) -> Self` | Tooltip text |
| `label` | `fn label(self, label: impl Into<Text>) -> Self` | Label text |
| `checked` | `fn checked(self, checked: bool) -> Self` | Initial checked state |
| `on_click` | `fn on_click(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self` | Click handler; `&bool` = new state |
| `tab_stop` | `fn tab_stop(self, tab_stop: bool) -> Self` | Tab stop (default true) |
| `tab_index` | `fn tab_index(self, tab_index: isize) -> Self` | Tab order (default 0) |

### Traits

- `InteractiveElement`, `StatefulInteractiveElement`
- `Styled`, `Disableable`, `Selectable`
- `ParentElement`, `Sizable`
- `RenderOnce`

### Example

```rust
Checkbox::new("agree")
    .label("I agree to the terms")
    .checked(false)
    .on_click(|checked, _window, cx| {
        cx.notify(); // handle state change
    })
```

---

## Switch

**File:** `crates/ui/src/switch.rs`

An animated toggle switch with label. No separate state entity.

```rust
#[derive(IntoElement)]
pub struct Switch {
    // id, style, checked, disabled, label: Option<Text>,
    // label_side: Side, on_click: Option<Rc<...>>,
    // size, color: Option<Hsla>, tooltip
}
```

### Constructor

```rust
impl Switch {
    // Created via IntoElement / fluent builder
    // No explicit new() — use Default or construct directly
    pub fn new(id: impl Into<ElementId>) -> Self;  // implied pattern
}
```

### Builder Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `checked` | `fn checked(self, checked: bool) -> Self` | Initial state |
| `label` | `fn label(self, label: impl Into<Text>) -> Self` | Label text |
| `on_click` | `fn on_click(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self` | Click handler; `&bool` = new state |
| `color` | `fn color(self, color: impl Into<Hsla>) -> Self` | Checked color (default: theme primary) |
| `tooltip` | `fn tooltip(self, tooltip: impl Into<SharedString>) -> Self` | Tooltip text |

### Traits

- `Styled`, `Sizable`, `Disableable`
- `RenderOnce`

---

## Radio

**File:** `crates/ui/src/radio.rs`

Radio buttons and radio groups. No separate state entity — checked state is managed inline.

### Radio

```rust
#[derive(IntoElement)]
pub struct Radio {
    // base(Div), style, id, label, children, checked, disabled,
    // tab_stop, tab_index, size, on_click, tooltip
}
```

#### Constructor / Builder

```rust
impl Radio {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn tooltip(self, tooltip: impl Into<SharedString>) -> Self;
    pub fn label(self, label: impl Into<Text>) -> Self;
    pub fn checked(self, checked: bool) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn tab_index(self, tab_index: isize) -> Self;
    pub fn tab_stop(self, tab_stop: bool) -> Self;
    pub fn on_click(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self;
}
```

#### Traits

- `Sizable`, `Styled`, `InteractiveElement`, `StatefulInteractiveElement`
- `ParentElement`, `From<&'static str>`, `From<SharedString>`, `From<String>`
- `RenderOnce`

### RadioGroup

A container that manages exclusive selection among child radios.

```rust
#[derive(IntoElement)]
pub struct RadioGroup {
    // id, style, radios: Vec<Radio>, layout: Axis,
    // selected_index: Option<usize>, disabled: bool,
    // on_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App)>>
}
```

#### Constructor / Builder

```rust
impl RadioGroup {
    pub fn vertical(id: impl Into<ElementId>) -> Self;
    pub fn horizontal(id: impl Into<ElementId>) -> Self;
    pub fn layout(self, layout: Axis) -> Self;
    pub fn on_click(self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self;
    pub fn selected_index(self, index: Option<usize>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn child(self, radio: impl Into<Radio>) -> Self;
    pub fn children(self, radios: impl IntoIterator<Item = impl Into<Radio>>) -> Self;
}
```

#### Traits

- `Sizable`, `Styled`, `RenderOnce`

#### Example

```rust
RadioGroup::vertical("theme")
    .selected_index(Some(0))
    .on_click(|ix, _window, cx| {
        println!("Selected index: {}", ix);
    })
    .child(Radio::new("light").label("Light"))
    .child(Radio::new("dark").label("Dark"))
```

---

## Slider

**File:** `crates/ui/src/slider.rs`

A slider with single-value or range mode, linear or logarithmic scale.

### SliderState

```rust
pub struct SliderState {
    min: f32,              // default: 0.0
    max: f32,              // default: 100.0
    step: f32,             // default: 1.0
    value: SliderValue,    // default: Single(0.0)
    scale: SliderScale,    // default: Linear
    // ... internal fields
}
```

#### Constructor

```rust
impl SliderState {
    pub fn new() -> Self;
}
```

#### Builder / Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `min` | `fn min(self, min: f32) -> Self` | Minimum value (default 0.0) |
| `max` | `fn max(self, max: f32) -> Self` | Maximum value (default 100.0) |
| `step` | `fn step(self, step: f32) -> Self` | Step size (default 1.0) |
| `scale` | `fn scale(self, scale: SliderScale) -> Self` | `Linear` or `Logarithmic` |
| `default_value` | `fn default_value(self, value: impl Into<SliderValue>) -> Self` | Initial value |
| `set_value` | `fn set_value(&mut self, value: impl Into<SliderValue>, window, cx)` | Update value |
| `value` | `fn value(&self) -> SliderValue` | Current value |

#### Value Types

```rust
pub enum SliderValue {
    Single(f32),         // Single thumb
    Range(f32, f32),    // Two thumbs for range
}
// Conversions from f32, (f32, f32), Range<f32>
```

#### Scale Types

```rust
pub enum SliderScale { Linear, Logarithmic }
// Logarithmic requires min > 0.0
```

#### Events

```rust
pub enum SliderEvent {
    Change(SliderValue),   // Continuous while dragging
    Release(SliderValue),  // Fired once on release
}
```

### Slider (Element)

```rust
#[derive(IntoElement)]
pub struct Slider {
    state: Entity<SliderState>,
    axis: Axis,
    style: StyleRefinement,
    disabled: bool,
}
```

#### Constructor / Builder

```rust
impl Slider {
    pub fn new(state: &Entity<SliderState>) -> Self;
    pub fn horizontal(self) -> Self;   // default
    pub fn vertical(self) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}
```

#### Traits

- `Styled` — `style()`
- `RenderOnce`

---

## Toggle

**File:** `crates/ui/src/button/toggle.rs`

A toggle button (not the same as Switch). Supports icon, label, and checked state.

### ToggleVariant

```rust
pub enum ToggleVariant {
    #[default] Default,
    Outline,
    Ghost,
}
```

### Toggle

```rust
#[derive(IntoElement)]
pub struct Toggle {
    // id, style, checked, size, variant, disabled,
    // border_corners, border_edges, children, on_click, tooltip
}
```

#### Constructor / Builder

```rust
impl Toggle {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn tooltip(self, tooltip: impl Into<SharedString>) -> Self;
    pub fn label(self, label: impl Into<SharedString>) -> Self;
    pub fn icon(self, icon: impl Into<Icon>) -> Self;
    pub fn checked(self, checked: bool) -> Self;
    pub fn on_click(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self;
    // pub(crate): border_corners, border_edges
}
```

#### Traits

- `ToggleVariants` — `with_variant(ToggleVariant)` for Default/Outline/Ghost
- `ParentElement`, `Disableable`, `Sizable`, `Styled`
- `RenderOnce`

### ToggleGroup

A connected group of toggles (like a segmented control).

```rust
#[derive(IntoElement)]
pub struct ToggleGroup {
    // id, style, size, variant, disabled, segmented, items: Vec<Toggle>,
    // on_click: Option<Rc<dyn Fn(&Vec<bool>, ...)>>
}
```

#### Constructor / Builder

```rust
impl ToggleGroup {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn child(self, toggle: impl Into<Toggle>) -> Self;
    pub fn children(self, children: impl IntoIterator<Item = impl Into<Toggle>>) -> Self;
    pub fn on_click(self, handler: impl Fn(&Vec<bool>, &mut Window, &mut App) + 'static) -> Self;
    pub fn segmented(self) -> Self;  // Connected segmented control style
}
```

#### Traits

- `Sizable`, `ToggleVariants`, `Disableable`, `Styled`
- `RenderOnce`

---

## Rating

**File:** `crates/ui/src/rating.rs`

A star rating component. No separate state entity — state tracked via `use_keyed_state`.

```rust
#[derive(IntoElement)]
pub struct Rating {
    // id, style, size, disabled, value: usize,
    // max: usize, color: Option<Hsla>, on_click: Option<Rc<...>>
}
```

### Constructor / Builder

```rust
impl Rating {
    pub fn new(id: impl Into<ElementId>) -> Self;   // defaults: value=0, max=5
    pub fn value(self, value: usize) -> Self;        // Initial value, clamped to max
    pub fn max(self, max: usize) -> Self;            // Max stars (default 5)
    pub fn color(self, color: impl Into<Hsla>) -> Self;  // Active star color (default: theme yellow)
    pub fn on_click(self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}
```

Clicking the same star deselects (value drops by 1). Hover previews the rating.

#### Traits

- `Styled`, `Sizable`, `Disableable`
- `RenderOnce`

---

## Stepper

**File:** `crates/ui/src/stepper/`

A step progress indicator. No separate state entity.

### Stepper

```rust
#[derive(IntoElement)]
pub struct Stepper {
    // id, style, items: Vec<StepperItem>, step: usize,
    // layout: Axis, disabled, size, text_center, on_click
}
```

#### Constructor / Builder

```rust
impl Stepper {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn text_center(self, center: bool) -> Self;
    pub fn layout(self, layout: Axis) -> Self;
    pub fn vertical(self) -> Self;
    pub fn selected_index(self, step: usize) -> Self;   // Current active step
    pub fn item(self, item: impl Into<StepperItem>) -> Self;
    pub fn items(self, items: impl IntoIterator<Item = StepperItem>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
    pub fn on_click(self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self;
}
```

### StepperItem

```rust
#[derive(IntoElement)]
pub struct StepperItem { /* ... */ }
```

#### Constructor / Builder

```rust
impl StepperItem {
    pub fn new() -> Self;
    pub fn icon(self, icon: impl Into<Icon>) -> Self;     // Custom icon for completed steps
    pub fn disabled(self, disabled: bool) -> Self;
}
```

Add children (label text, etc.) via `ParentElement`:

```rust
Stepper::new("setup")
    .selected_index(1)
    .item(StepperItem::new().child("Account"))
    .item(StepperItem::new().child("Profile"))
    .item(StepperItem::new().child("Done"))
```

#### Traits

- `Sizable`, `Styled`, `RenderOnce` (on both Stepper and StepperItem)
- `ParentElement` on StepperItem

---

## DatePicker

**File:** `crates/ui/src/time/date_picker.rs`

A date picker with single or range date selection, calendar popup, and preset support.

### DatePickerState

```rust
pub struct DatePickerState {
    focus_handle: FocusHandle,
    date: Date,
    open: bool,
    calendar: Entity<CalendarState>,
    date_format: SharedString,       // default: "%Y/%m/%d"
    number_of_months: usize,         // default: 1
    disabled_matcher: Option<Rc<Matcher>>,
    _subscriptions: Vec<Subscription>,
}
```

#### Constructor

```rust
impl DatePickerState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;       // Single date mode
    pub fn range(window: &mut Window, cx: &mut Context<Self>) -> Self;     // Range mode
}
```

#### Builder / Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `date_format` | `fn date_format(self, format: impl Into<SharedString>) -> Self` | strftime format (default `%Y/%m/%d`) |
| `number_of_months` | `fn number_of_months(self, n: usize) -> Self` | Months in calendar view |
| `date` | `fn date(&self) -> Date` | Current date |
| `set_date` | `fn set_date(&mut self, date: impl Into<Date>, window, cx)` | Set date programmatically |
| `disabled_matcher` | `fn disabled_matcher(self, disabled: impl Into<Matcher>) -> Self` | Dates to disable |
| `set_year_range` | `fn set_year_range(&mut self, range: (i32, i32), cx)` | Year range (default ±50 years) |

#### Date Type

```rust
pub enum Date {
    Single(Option<NaiveDate>),
    Range(Option<NaiveDate>, Option<NaiveDate>),
}
```

#### Events

```rust
pub enum DatePickerEvent {
    Change(Date),
}
```

Emits `DatePickerEvent::Change` when the user selects a date in the calendar.

### DateRangePreset

```rust
pub struct DateRangePreset {
    pub label: SharedString,
    pub value: DateRangePresetValue,
}
pub enum DateRangePresetValue {
    Single(NaiveDate),
    Range(NaiveDate, NaiveDate),
}
```

### DatePicker (Element)

```rust
#[derive(IntoElement)]
pub struct DatePicker {
    // id, state, cleanable, placeholder, size, style,
    // number_of_months, presets, appearance, disabled
}
```

#### Constructor / Builder

```rust
impl DatePicker {
    pub fn new(state: &Entity<DatePickerState>) -> Self;
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn cleanable(self, cleanable: bool) -> Self;
    pub fn presets(self, presets: Vec<DateRangePreset>) -> Self;  // Sidebar presets
    pub fn number_of_months(self, n: usize) -> Self;               // Default: 2
    pub fn appearance(self, appearance: bool) -> Self;             // Minimal mode
}
```

#### Traits

- `Sizable`, `Focusable`, `Styled`, `Disableable`
- `RenderOnce`

---

## Form

**File:** `crates/ui/src/form/`

Layout components for building forms with labels, fields, and validation patterns.

### Free Functions

```rust
pub fn v_form() -> Form;    // Vertical form layout
pub fn h_form() -> Form;    // Horizontal form layout
pub fn field() -> Field;    // Create a new field
```

### Form

```rust
#[derive(IntoElement)]
pub struct Form {
    // style, fields: Vec<Field>, props: FieldProps
}
```

#### Constructor / Builder

```rust
impl Form {
    fn new() -> Self;                                           // internal
    pub fn horizontal() -> Self;                                // Axis::Horizontal
    pub fn vertical() -> Self;                                  // Axis::Vertical
    pub fn layout(self, layout: Axis) -> Self;                  // Set axis
    pub fn label_width(self, width: Pixels) -> Self;            // Label column width (default px(100.))
    pub fn label_text_size(self, size: Rems) -> Self;           // Label font size
    pub fn child(self, field: impl Into<Field>) -> Self;        // Add one field
    pub fn children(self, fields: impl IntoIterator<Item = Field>) -> Self;  // Add multiple
    pub fn columns(self, columns: usize) -> Self;               // Column count (default 1)
}
```

#### Traits

- `Styled`, `Sizable`
- `RenderOnce`

### Field

```rust
#[derive(IntoElement)]
pub struct Field {
    // id, props, style, label, label_indent, description,
    // children, visible, required, align_items, col_span, col_start, col_end
}
```

#### Constructor / Builder

```rust
impl Field {
    pub fn new() -> Self;
    pub fn label(self, label: impl Into<FieldBuilder>) -> Self;
    pub fn label_fn<F, E>(self, label: F) -> Self
        where E: IntoElement, F: Fn(&mut Window, &mut App) -> E + 'static;
    pub fn label_indent(self, indent: bool) -> Self;  // default true
    pub fn description(self, description: impl Into<FieldBuilder>) -> Self;
    pub fn description_fn<F, E>(self, desc: F) -> Self
        where E: IntoElement, F: Fn(&mut Window, &mut App) -> E + 'static;
    pub fn visible(self, visible: bool) -> Self;
    pub fn required(self, required: bool) -> Self;
    pub fn items_start(self) -> Self;
    pub fn items_end(self) -> Self;
    pub fn items_center(self) -> Self;
    pub fn col_span(self, col_span: u16) -> Self;
    pub fn col_start(self, col_start: i16) -> Self;
    pub fn col_end(self, col_end: i16) -> Self;
}
```

`FieldBuilder` accepts `&'static str`, `String`, `SharedString`, or a function returning an element.

#### Child Pattern

Fields use `ParentElement` to accept children — the actual form control:

```rust
Field::new()
    .label("Email")
    .child(Input::new(&email_state))
```

### Form Validation Patterns

gpui-component does not ship a declarative validation framework. Instead:

1. **Regex pattern** on `InputState`: `state.pattern(regex::Regex::new(r"^\d+$").unwrap())` — rejects invalid keystrokes.
2. **Custom validate function**: `state.validate(|text, cx| { text.len() <= 100 })` — called on each input change.
3. **Manual validation**: Subscribe to `InputEvent::Change`, check the value, and manage error state externally.
4. **Field required state**: `field.required(true)` adds a visual indicator (asterisk). No automatic enforcement — handle submission logic yourself.

#### Example: Full Form

```rust
let name_state = cx.new(|cx| InputState::new(window, cx));
let email_state = cx.new(|cx| {
    InputState::new(window, cx)
        .placeholder("user@example.com")
});

v_form()
    .label_width(px(120.))
    .child(
        field()
            .label("Name")
            .required(true)
            .child(Input::new(&name_state))
    )
    .child(
        field()
            .label("Email")
            .required(true)
            .child(Input::new(&email_state))
    )
```
