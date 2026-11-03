//! The IME candidate window's anchor, and the one rule for pushing it.
//!
//! On X11 the candidate window follows the input context's spot location.
//! fcitx5's XIM frontend recomputes the candidate rectangle from the stored
//! spot on every `XIM_SET_IC_VALUES` (`xim.cpp` `updateCursorLocation`), and
//! re-translates the stored spot with the client window's current geometry
//! each time its panel is about to show — so one `set_ic_values` with the
//! cursor position is enough, and window moves correct themselves at the next
//! panel show. With no spot stored at all its fallback is the client window's
//! bottom-left corner, which is exactly the misplaced panel this module
//! exists to fix.
//!
//! Two facts make client-side pushes necessary:
//!
//! - The spot only ever leaves zenix through `Window::update_ime_position`,
//!   and nothing calls it by itself: `invalidate_character_coordinates` is
//!   the one API, and neither the component library nor zenix used it. The
//!   framework's one automatic re-push lives in its preedit handler, which
//!   only fires when the IME draws the preedit in the client — sogou's
//!   floating panel shows the preedit itself and sends no preedit draws at
//!   all, so while composing in that mode nothing but these pushes keeps the
//!   spot current.
//!
//! - The spot is one value per process: the XIM connection keeps a single
//!   input context, focused on the one active window. A text surface that
//!   re-gains focus with an unchanged cursor must still re-push after another
//!   surface moved the slot, so every caller compares against this one shared
//!   [`Spot`] rather than its own memory — and only the active window pushes,
//!   because a spot is meaningful only in the window the input context points
//!   at.
//!
//! The platform pull (`selected_bounds` → `bounds_for_range`) reads whoever
//! holds the focus, so a caller gates the push on its own focus handle and
//! passes the anchor it would itself report. The terminal grid, the
//! conversation composer and the find field hook this from their paint;
//! a new text surface that takes keyboard input should do the same.
use gpui::{App, Bounds, Global, Pixels, Window};

/// The anchor last offered to the platform IME, in the pushing window's
/// coordinates.
#[derive(Default, PartialEq)]
pub struct Spot(Option<Bounds<Pixels>>);

impl Global for Spot {}

/// Offers `anchor` to the platform IME when it differs from the last one any
/// surface offered. The platform then pulls the real bounds from the focused
/// element's input handler on the next frame, so this is only the change
/// signal — nothing is sent when the anchor did not move.
pub fn push_spot(anchor: Option<Bounds<Pixels>>, window: &Window, cx: &mut App) {
    if !window.is_window_active() {
        return;
    }
    let slot = cx.default_global::<Spot>();
    if slot.0 == anchor {
        return;
    }
    slot.0 = anchor;
    window.invalidate_character_coordinates();
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, EmptyView, TestAppContext, point, px, size};

    use super::{Spot, push_spot};

    #[gpui::test]
    fn a_repeated_anchor_stops_pushing_and_a_new_one_pushes_again(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| EmptyView);
        let anchor = Some(Bounds::new(point(px(10.), px(20.)), size(px(8.), px(16.))));
        let other = Some(Bounds::new(point(px(10.), px(40.)), size(px(8.), px(16.))));
        cx.update(|window, _| window.activate_window());
        // The platform's active callback lands on a later pass.
        cx.run_until_parked();
        cx.update(|window, cx| {
            push_spot(anchor, window, cx);
            assert_eq!(cx.default_global::<Spot>().0, anchor);
            // The same anchor again: the slot keeps it, no second push.
            push_spot(anchor, window, cx);
            assert_eq!(cx.default_global::<Spot>().0, anchor);
            // A moved cursor replaces it.
            push_spot(other, window, cx);
            assert_eq!(cx.default_global::<Spot>().0, other);
            // No anchor (nothing focused under this surface) clears the slot.
            push_spot(None, window, cx);
            assert_eq!(cx.default_global::<Spot>().0, None);
        });
    }
}
