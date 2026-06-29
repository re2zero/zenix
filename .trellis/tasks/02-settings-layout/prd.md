# Settings UI Layout Optimization

**Created:** 2026-06-29
**Author:** re2zero
**Priority:** P1

---

## Problem Statement

The settings dialog has several layout issues that make it look unprofessional and hard to use:

1. **Terminal tab**: `setting_row` uses `justify_between` which pushes controls (cursor style buttons, opacity +/-, scrollback values) to the far right edge of the dialog. The label sits on the left but the control is far away, breaking visual connection.

2. **MCP form**: Label widths use `w(px(60.))` which is too narrow. Combined with `h_flex().gap(px(8.))`, the form fields spread out, making the form look stretched horizontally.

3. **API form**: Label widths use `w(px(80.))` which is inconsistent with the MCP form's `w(px(60.))`. Presets, suffix options, and thinking level buttons wrap awkwardly with no max-width constraint.

4. **Cards (MCP, API, Skills)**: No `max_w` constraints. Cards can expand to fill the entire dialog width, making them look thin and scattered when there's only one item.

5. **Action buttons**: Edit/Test/Remove buttons cluster on the far right with no alignment anchor. When there are many buttons, they overflow or create uneven spacing.

6. **Form submit/cancel buttons**: Located in a `justify_end` h_flex at the bottom of forms, but with no width constraints on the form itself, the buttons can sit too far from the form fields above.

---

## Goals

1. **Standardize label widths**: Use consistent label widths across all forms (100px for wide labels, 80px for short ones)
2. **Add max-width constraints**: Cards should have `max_w(px(600.))` to prevent over-spreading
3. **Improve control alignment**: Replace `justify_between` with `justify_end` or `flex_grow` patterns where controls need to stay close to labels
4. **Constrain form width**: Forms should have reasonable max widths so buttons align vertically with form fields
5. **Button sizing**: Compact buttons should have consistent sizing, not spread out

---

## Anti-Patterns (DO NOT USE)

- ❌ `justify_between` on setting rows (pushes controls far right)
- ❌ Inconsistent label widths (60px in MCP, 80px in API)
- ❌ No `max_w` on cards or forms
- ❌ Unconstrained `flex_wrap` with no max width (buttons spread infinitely)

## Preferred Patterns

- ✅ `setting_row` with `justify_end` or inline controls next to labels
- ✅ Consistent label widths: 100px for most labels
- ✅ `max_w(px(600.))` on card containers
- ✅ `flex_wrap` with `max_w` constraint to prevent overflow
- ✅ Buttons anchored with `justify_end` at form bottom, aligned with form content width

---

## Implementation Plan

### 1. Fix `setting_row` in `helpers.rs`

Change from `justify_between` to a layout that keeps controls close to labels:

```rust
// Before:
h_flex().items_center().justify_between().gap(px(12.))

// After:
h_flex().items_center().gap(px(12.)).child(ctrl).flex_grow(1.0).child(setting_label(label))
// Or for toggle-style rows:
h_flex().items_center().gap(px(16.)).child(ctrl)
```

### 2. Fix Terminal Tab (`tabs.rs`)

- Terminal cursor style buttons: Use `justify_center` or `justify_start` instead of spreading across full width
- Opacity/Scrollback controls: Keep inline with label, not pushed to right edge
- Consider using a 2-column grid layout for settings with label on left, control on right

### 3. Fix MCP Form (`components.rs`)

- Change `w(px(60.))` to `w(px(100.))` for all labels
- Add `max_w` to the form container
- Ensure form fields don't over-expand

### 4. Fix API Form (`components.rs`)

- Change `w(px(80.))` to `w(px(100.))` for consistency
- Add `max_w` constraints
- Constrain preset/suffix/thinking button groups with `max_w`

### 5. Fix Cards

- Add `max_w(px(600.))` to `mcp_card`, `api_provider_card`, `agent_detail_panel`
- Constrain action button groups with `justify_end` and consistent gaps

### 6. Fix Inline Edit Forms

- `mcp_inline_edit_card` and `api_inline_edit_card`: Standardize label widths and add constraints

---

## Acceptance Criteria

1. ✅ Terminal tab: controls sit next to their labels, not pushed to far right
2. ✅ MCP form: labels are 100px wide, form doesn't spread horizontally
3. ✅ API form: consistent 100px label width, button groups stay within bounds
4. ✅ All cards have `max_w(px(600.))` or similar constraint
5. ✅ Form submit/cancel buttons align vertically with form content
6. ✅ `cargo check` passes with no warnings
7. ✅ No visual regressions — all functionality preserved
