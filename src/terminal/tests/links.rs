use super::*;
use crate::terminal::links::{LinkTarget, activation_fallback, link_at, plain_url, web_url};
use serde_json::json;

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

fn frame(width: u16, rows: &[&str], hyperlinks: Vec<String>) -> FrameData {
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
        hyperlinks,
        graphics: vec![],
    }
}

fn surface(frame: FrameData) -> PaneSurfaceFrame {
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
            scroll: None,
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

fn at(column: u16, row: u16) -> (f32, f32) {
    (
        (f32::from(column) + 0.5) * CELL_WIDTH,
        (f32::from(row) + 0.5) * CELL_HEIGHT,
    )
}

fn hit_at(surface: &PaneSurfaceFrame, column: u16, row: u16) -> Option<usize> {
    let (x, y) = at(column, row);
    link_at(surface, x, y, CELL_WIDTH, CELL_HEIGHT).map(|hit| hit.columns.start as usize)
}

#[test]
fn a_plain_url_is_found_under_the_pointer() {
    let surface = surface(frame(30, &["see https://example.com/x now "], vec![]));
    let (x, y) = at(12, 0);
    let hit = link_at(&surface, x, y, CELL_WIDTH, CELL_HEIGHT).expect("the pointer is on the URL");
    assert_eq!(hit.target, LinkTarget::Web("https://example.com/x".into()));
    assert_eq!(hit.columns, 4..25);
    // A cell outside the address resolves nothing.
    assert!(hit_at(&surface, 0, 0).is_none());
    assert!(hit_at(&surface, 25, 0).is_none());
}

#[test]
fn an_explicit_hyperlink_wins_and_covers_its_whole_run() {
    let mut frame = frame(
        20,
        &["click here          "],
        vec!["https://explicit.example/a".into()],
    );
    for column in 6..10 {
        frame.cells[column].hyperlink = Some(0);
    }
    let surface = surface(frame);
    let (x, y) = at(7, 0);
    let hit = link_at(&surface, x, y, CELL_WIDTH, CELL_HEIGHT).expect("an explicit link");
    assert_eq!(
        hit.target,
        LinkTarget::Web("https://explicit.example/a".into())
    );
    assert_eq!(hit.columns, 6..10);
}

#[test]
fn prose_punctuation_at_the_edge_is_not_part_of_the_address() {
    let surface = surface(frame(30, &["go to https://example.com/a.    "], vec![]));
    let (x, y) = at(10, 0);
    let hit = link_at(&surface, x, y, CELL_WIDTH, CELL_HEIGHT).expect("a URL before a full stop");
    assert_eq!(hit.target, LinkTarget::Web("https://example.com/a".into()));
}

#[test]
fn only_http_addresses_are_opened() {
    assert_eq!(
        web_url("https://example.com/a"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        web_url("http://example.com"),
        Some("http://example.com".into())
    );
    for refused in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://example.com/a b",
        "https://example.com/a\nnext",
        "",
    ] {
        assert_eq!(web_url(refused), None, "{refused:?}");
    }
}

#[test]
fn plain_url_trims_trailing_punctuation_but_marks_a_row_that_may_continue() {
    // A full stop after the address closes the sentence, not the URL.
    assert_eq!(
        plain_url("see https://example.com/a. ", 8),
        Some((4..25, false))
    );
    // Balanced brackets stay; the closing one beyond the opening is prose.
    assert_eq!(plain_url("(https://example.com/a)", 5), Some((1..22, true)));
    // A URL that fills the row may continue on the next one.
    assert_eq!(plain_url("https://example.com/a", 5), Some((0..21, true)));
    assert_eq!(plain_url("nothing here", 3), None);
}

#[test]
fn an_activation_answer_opens_the_daemon_url_or_the_rows_own() {
    let fallback = Some("https://fallback.example/x");
    // A plugin handled it: nothing opens.
    assert_eq!(
        activation_fallback(
            &json!({"result": {"type": "pane_link_activated", "handled": true}}),
            fallback
        ),
        None
    );
    // The daemon read a wrapped address the row could not.
    assert_eq!(
        activation_fallback(
            &json!({"result": {"type": "pane_link_activated", "handled": false,
                               "url": "https://wrapped.example/a/b"}}),
            fallback
        ),
        Some("https://wrapped.example/a/b".into())
    );
    // Nothing read there: the row's own address is opened.
    assert_eq!(
        activation_fallback(
            &json!({"result": {"type": "pane_link_activated", "handled": false}}),
            fallback
        ),
        Some("https://fallback.example/x".into())
    );
    // An error still leaves the row-local address as the fallback.
    assert_eq!(
        activation_fallback(
            &json!({"error": {"code": "stale", "message": "moved"}}),
            fallback
        ),
        Some("https://fallback.example/x".into())
    );
    // A non-web fallback is never opened.
    assert_eq!(
        activation_fallback(
            &json!({"error": {"code": "stale"}}),
            Some("file:///etc/passwd")
        ),
        None
    );
}
