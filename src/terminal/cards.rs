//! Pane cards: every pane is drawn as a card with a gap around it, instead of
//! the daemon's box-drawing borders (PLAN.md §M11a).
//!
//! The daemon lays panes out contiguously and spends cells on their borders:
//! a pane's `rect` is its content plus one border cell on each side it borders,
//! and the scrollbar gutter the daemon reserves is a further column inside
//! that. None of that is moved — the client paints the border cells as the gap
//! and strokes the content rectangle itself, so layout, input and the cells the
//! daemon sends stay exactly what they were.

use herdr_client::protocol::{PaneSurfacePane, SurfaceRect};

/// The background the grid keeps from the edges of the pane area, in pixels.
pub(crate) const INSET: f32 = 8.;

/// One card's corner radius, in pixels.
pub(crate) const RADIUS: f32 = 6.;

/// The cells a pane shows: its inner rectangle, plus the scrollbar column when
/// the daemon reserved one. That column sits next to `inner_rect` and carries
/// the thumb the client paints over, so it is content, not chrome.
pub(crate) fn content(pane: &PaneSurfacePane) -> SurfaceRect {
    let mut rect = pane.inner_rect;
    if pane.scrollbar_rect.is_some() {
        rect.width = rect.width.saturating_add(1);
    }
    rect
}

/// The chrome cells around a pane's content, as top, bottom, left and right.
/// A band on an edge the daemon drew no border for is empty.
pub(crate) fn bands(pane: &PaneSurfacePane) -> [SurfaceRect; 4] {
    let rect = pane.rect;
    let content = content(pane);
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    // A frame can carry a pane whose inner rectangle no longer fits it (a
    // border drawn while the pane shrank); clamping keeps every band inside
    // the pane instead of masking a neighbour's cells.
    let content_x = content.x.clamp(rect.x, right);
    let content_y = content.y.clamp(rect.y, bottom);
    let content_right = content
        .x
        .saturating_add(content.width)
        .clamp(content_x, right);
    let content_bottom = content
        .y
        .saturating_add(content.height)
        .clamp(content_y, bottom);
    [
        SurfaceRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: content_y.saturating_sub(rect.y),
        },
        SurfaceRect {
            x: rect.x,
            y: content_bottom,
            width: rect.width,
            height: bottom.saturating_sub(content_bottom),
        },
        SurfaceRect {
            x: rect.x,
            y: content_y,
            width: content_x.saturating_sub(rect.x),
            height: content_bottom.saturating_sub(content_y),
        },
        SurfaceRect {
            x: content_right,
            y: content_y,
            width: right.saturating_sub(content_right),
            height: content_bottom.saturating_sub(content_y),
        },
    ]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn pane(rect: SurfaceRect, inner: SurfaceRect, scrollbar: bool) -> PaneSurfacePane {
        PaneSurfacePane {
            pane_id: "p".into(),
            content_revision: 0,
            rect,
            inner_rect: inner,
            scrollbar_rect: scrollbar.then_some(SurfaceRect {
                x: inner.x + inner.width,
                y: inner.y,
                width: 1,
                height: inner.height,
            }),
            scroll: None,
            focused: false,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn cells(rect: SurfaceRect) -> impl Iterator<Item = (u16, u16)> {
        (rect.y..rect.y + rect.height)
            .flat_map(move |y| (rect.x..rect.x + rect.width).map(move |x| (x, y)))
    }

    /// The daemon's frame for a two-by-two split, as measured from a live
    /// daemon: `rect` carries the border cell, `inner_rect` the scrollbar
    /// gutter.
    fn split() -> Vec<PaneSurfacePane> {
        vec![
            pane(
                SurfaceRect {
                    x: 0,
                    y: 0,
                    width: 42,
                    height: 10,
                },
                SurfaceRect {
                    x: 1,
                    y: 1,
                    width: 39,
                    height: 8,
                },
                true,
            ),
            pane(
                SurfaceRect {
                    x: 42,
                    y: 0,
                    width: 38,
                    height: 10,
                },
                SurfaceRect {
                    x: 43,
                    y: 1,
                    width: 35,
                    height: 8,
                },
                true,
            ),
        ]
    }

    #[test]
    fn the_content_keeps_the_scrollbar_column() {
        let panes = split();
        let shown = content(&panes[0]);
        assert_eq!((shown.x, shown.y, shown.width, shown.height), (1, 1, 40, 8));
        // Without a thumb the gutter is empty, so the column is chrome.
        let mut bare = panes[0].clone();
        bare.scrollbar_rect = None;
        assert_eq!(content(&bare).width, 39);
    }

    #[test]
    fn the_bands_are_the_cells_the_daemon_spent_on_borders_and_gutters() {
        let panes = split();
        let bands = bands(&panes[0]);
        let covered: Vec<(u16, u16)> = bands.iter().flat_map(|band| cells(*band)).collect();
        for (x, y) in cells(panes[0].rect) {
            let is_content = cells(content(&panes[0])).any(|cell| cell == (x, y));
            assert_eq!(covered.contains(&(x, y)), !is_content, "cell {x},{y}");
        }
    }

    #[test]
    fn a_pane_without_borders_has_no_bands() {
        let mut single = pane(
            SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            false,
        );
        single.scrollbar_rect = None;
        for band in bands(&single) {
            assert_eq!(
                u32::from(band.width) * u32::from(band.height),
                0,
                "{band:?}"
            );
        }
    }

    #[test]
    fn bands_never_leave_the_pane_even_when_the_inner_rect_does_not_fit() {
        let stale = pane(
            SurfaceRect {
                x: 10,
                y: 5,
                width: 4,
                height: 3,
            },
            SurfaceRect {
                x: 12,
                y: 6,
                width: 40,
                height: 20,
            },
            true,
        );
        for band in bands(&stale) {
            assert!(band.x >= 10 && band.x + band.width <= 14, "{band:?}");
            assert!(band.y >= 5 && band.y + band.height <= 8, "{band:?}");
        }
    }
}
