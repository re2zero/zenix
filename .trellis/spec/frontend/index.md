# Frontend Specs — GPUI & Chat Panel

Code-specs governing GPUI rendering, overlay, and interaction patterns in zenix.

## Files

- [`gpui-ui-patterns.md`](./gpui-ui-patterns.md) — Executable contracts for absolute/overlay/flex/drag patterns in the GPUI chat panel.

## Scope

These specs apply to all code under `src/ai/chat/` and any future GPUI UI work in `src/ui/` or other view modules. They capture the non-obvious behaviors of gpui and gpui-component that the v4 redesign had to learn the hard way.

## Quick reference

- Absolute element that must catch clicks → `.occlude()`.
- Overlay that must not disturb layout → wrap in `gpui::deferred(...)`.
- Flex item holding long text → `.min_w_0().overflow_hidden()`.
- Drag handle listener → on the handle's direct parent, not the root.
- Library widget first (`Popover`, `Select`, `Button`) — hand-roll only with a documented reason.
- Bubble-to-border gap → one `msg_area.px(N)`, no per-bubble margins.
- Select-style action must clear `config_dropdown` + `cx.notify()`.
