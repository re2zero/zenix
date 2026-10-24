use super::*;
use crate::terminal::selection::Selection;
use herdr_client::protocol::PaneSurfaceScrollMetrics;

const CELL_WIDTH: f32 = 10.;
const CELL_HEIGHT: f32 = 20.;

fn blank() -> CellData {
    CellData {
        symbol: " ".into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    }
}

fn cell(symbol: &str) -> CellData {
    CellData {
        symbol: symbol.into(),
        ..blank()
    }
}

/// A frame whose rows are plain one-cell characters; every row is padded to
/// the frame's width with blanks, as a terminal paints it.
fn frame(width: u16, rows: &[&str]) -> FrameData {
    let mut cells = Vec::new();
    for row in rows {
        let symbols: Vec<char> = row.chars().collect();
        for column in 0..usize::from(width) {
            cells.push(
                symbols
                    .get(column)
                    .copied()
                    .map_or_else(blank, |symbol| cell(&symbol.to_string())),
            );
        }
    }
    FrameData {
        cells,
        width,
        height: rows.len() as u16,
        cursor: None,
        hyperlinks: vec![],
        graphics: vec![],
    }
}

/// One pane owning the whole frame, optionally scrolled back.
fn surface(frame: FrameData, offset_from_bottom: Option<(u64, u64)>) -> PaneSurfaceFrame {
    let (width, height) = (frame.width, frame.height);
    PaneSurfaceFrame {
        boot_id: "boot".into(),
        projection_revision: 1,
        surface_revision: 1,
        frame,
        panes: vec![PaneSurfacePane {
            pane_id: "pane".into(),
            content_revision: 2,
            rect: SurfaceRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            inner_rect: SurfaceRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            scrollbar_rect: None,
            scroll: offset_from_bottom.map(|(offset, max)| PaneSurfaceScrollMetrics {
                offset_from_bottom: offset,
                max_offset_from_bottom: max,
                viewport_rows: u64::from(height),
            }),
            focused: true,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 0,
            pixel_height: 0,
        }],
        splits: vec![],
        popup: None,
        graphics: Default::default(),
    }
}

/// The pointer at the left half of a cell, which is what a press anchors on.
fn at(column: u16, row: u16) -> (f32, f32) {
    (
        (f32::from(column) + 0.25) * CELL_WIDTH,
        (f32::from(row) + 0.5) * CELL_HEIGHT,
    )
}

fn begin(surface: &PaneSurfaceFrame, column: u16, row: u16, clicks: usize) -> Selection {
    let (x, y) = at(column, row);
    Selection::begin(surface, x, y, CELL_WIDTH, CELL_HEIGHT, clicks)
        .expect("the pane covers the press")
}

#[test]
fn a_drag_selects_the_cells_it_covers_and_copies_their_text() {
    let surface = surface(frame(10, &["hello     ", "world     "]), None);
    let mut selection = begin(&surface, 0, 0, 1);
    assert!(selection.extend(
        &surface,
        5. * CELL_WIDTH,
        1. * CELL_HEIGHT + 5.,
        CELL_WIDTH,
        CELL_HEIGHT
    ));
    let text = selection.text(&surface, CELL_WIDTH).expect("on screen");
    // The second row is cut at the pointer's half-cell, and no padding is
    // invented after it.
    assert_eq!(text, "hello\nworld");
    let highlights = selection.highlights(&surface, CELL_WIDTH);
    assert_eq!(highlights.len(), 2);
    assert_eq!(highlights[0].columns, 0..10);
    assert_eq!(highlights[1].columns, 0..5);
}

#[test]
fn a_click_that_never_leaves_its_half_cell_selects_nothing() {
    let surface = surface(frame(10, &["hello     "]), None);
    let selection = begin(&surface, 2, 0, 1);
    assert!(selection.is_empty(&surface, CELL_WIDTH));
    assert_eq!(selection.text(&surface, CELL_WIDTH).as_deref(), Some(""));
}

#[test]
fn a_double_click_takes_the_word_under_the_pointer() {
    let surface = surface(frame(12, &["foo bar baz "]), None);
    let selection = begin(&surface, 5, 0, 2);
    let text = selection.text(&surface, CELL_WIDTH).expect("on screen");
    assert_eq!(text, "bar");
    let highlights = selection.highlights(&surface, CELL_WIDTH);
    assert_eq!(highlights[0].columns, 4..7);
}

#[test]
fn a_triple_click_takes_the_whole_row() {
    let surface = surface(frame(12, &["  padded    "]), None);
    let selection = begin(&surface, 3, 0, 3);
    let text = selection.text(&surface, CELL_WIDTH).expect("on screen");
    assert_eq!(text, "  padded");
}

#[test]
fn dragging_backwards_keeps_the_end_the_press_took() {
    let surface = surface(frame(10, &["abcdefghij"]), None);
    let mut selection = begin(&surface, 4, 0, 1);
    // Left of the press: the selection runs from the pointer to the press's
    // far half-cell, so the cell under the press is kept.
    selection.extend(
        &surface,
        1. * CELL_WIDTH + 5.,
        0.5 * CELL_HEIGHT,
        CELL_WIDTH,
        CELL_HEIGHT,
    );
    let text = selection.text(&surface, CELL_WIDTH).expect("on screen");
    assert_eq!(text, "cd");
}

#[test]
fn a_selection_that_scrolled_away_copies_nothing() {
    let mut surface = surface(frame(10, &["hello     "]), Some((0, 40)));
    let selection = begin(&surface, 0, 0, 1);
    // The pane scrolls far enough that the row it selected is history now.
    let pane = &mut surface.panes[0];
    pane.scroll = Some(PaneSurfaceScrollMetrics {
        offset_from_bottom: 20,
        max_offset_from_bottom: 40,
        viewport_rows: 1,
    });
    assert_eq!(selection.text(&surface, CELL_WIDTH), None);
    assert!(selection.highlights(&surface, CELL_WIDTH).is_empty());
}

#[test]
fn a_tint_is_painted_for_every_selected_row_after_a_pane_scroll() {
    // Two visible rows, one history row above: content row 0 is history and
    // content row 1 paints on grid row 0. A selection made at the top of the
    // scrolled grid keeps its content rows.
    let surface = surface(frame(6, &["aaaaaa", "bbbbbb"]), Some((99, 100)));
    let selection = begin(&surface, 0, 0, 3);
    let highlights = selection.highlights(&surface, CELL_WIDTH);
    // The first painted row is history: the selection paints only the row the
    // viewport shows.
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].row, 0);
    assert_eq!(highlights[0].columns, 0..6);
}

#[test]
fn a_wide_cell_covers_its_continuation_without_doubling_its_text() {
    let mut surface = surface(frame(6, &["      "]), None);
    // Two wide cells: the symbol owns the first column and the second is the
    // wire's skip continuation.
    surface.frame.cells[0] = cell("你");
    surface.frame.cells[1] = CellData {
        skip: true,
        ..blank()
    };
    surface.frame.cells[2] = cell("好");
    surface.frame.cells[3] = CellData {
        skip: true,
        ..blank()
    };
    let selection = begin(&surface, 0, 0, 3);
    let text = selection.text(&surface, CELL_WIDTH).expect("on screen");
    assert_eq!(text, "你好");
}
