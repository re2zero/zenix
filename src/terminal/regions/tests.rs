use super::{Look, Owner, Region, partition};
use crate::terminal::painter::Span;
use crate::terminal::theme::Theme;
use gpui_kit::font;
use herdr_client::protocol::{
    CellData, CursorState, FrameData, PaneSurfaceFrame, PaneSurfacePane, PaneSurfaceScrollMetrics,
    SurfaceRect,
};
use std::sync::Arc;

fn rect(x: u16, y: u16, width: u16, height: u16) -> SurfaceRect {
    SurfaceRect {
        x,
        y,
        width,
        height,
    }
}

fn pane(id: &str, rect: SurfaceRect) -> PaneSurfacePane {
    PaneSurfacePane {
        pane_id: id.into(),
        content_revision: 1,
        rect,
        inner_rect: rect,
        scrollbar_rect: None,
        scroll: None,
        focused: false,
        mouse_reporting: false,
        sgr_pixel_mouse: false,
        alternate_screen_active: false,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn frame(rows: &[&str]) -> FrameData {
    let width = rows[0].chars().count() as u16;
    FrameData {
        width,
        height: rows.len() as u16,
        cells: rows
            .iter()
            .flat_map(|row| row.chars())
            .map(|symbol| CellData {
                symbol: symbol.to_string(),
                fg: 0,
                bg: 0,
                modifier: 0,
                skip: false,
                hyperlink: None,
            })
            .collect(),
        cursor: None,
        hyperlinks: vec![],
        graphics: vec![],
    }
}

fn surface(frame: FrameData, panes: Vec<PaneSurfacePane>) -> Arc<PaneSurfaceFrame> {
    Arc::new(PaneSurfaceFrame {
        boot_id: String::new(),
        projection_revision: 0,
        surface_revision: 1,
        frame,
        panes,
        splits: vec![],
        popup: None,
        graphics: Default::default(),
    })
}

fn look() -> Look {
    Look {
        font: font("Lilex"),
        font_size: 14.,
        cell_width: 8.,
        cell_height: 20.,
        theme: Theme::default(),
    }
}

/// Each cell's owner, row by row, as the partition assigns it.
fn owners(frame: &FrameData, panes: &[PaneSurfacePane]) -> Vec<Vec<Option<Owner>>> {
    let mut grid = vec![vec![None; usize::from(frame.width)]; usize::from(frame.height)];
    for (owner, area) in partition(frame, panes) {
        for span in area {
            for x in span.columns {
                let cell = &mut grid[usize::from(span.row)][usize::from(x)];
                assert!(cell.is_none(), "cell {x},{} painted twice", span.row);
                *cell = Some(owner.clone());
            }
        }
    }
    grid
}

#[test]
fn every_cell_belongs_to_one_region_and_earlier_panes_win_overlaps() {
    let frame = frame(&["aaaa|bbbb", "aaaa|bbbb", "---------"]);
    let a = Owner::Pane("a".into());
    let b = Owner::Pane("b".into());
    // Side by side, with a border column and a status row between them.
    let grid = owners(
        &frame,
        &[pane("a", rect(0, 0, 4, 2)), pane("b", rect(5, 0, 4, 2))],
    );
    for row in &grid[..2] {
        assert!(row[..4].iter().all(|o| o.as_ref() == Some(&a)));
        assert_eq!(row[4], Some(Owner::Chrome));
        assert!(row[5..].iter().all(|o| o.as_ref() == Some(&b)));
    }
    assert!(grid[2].iter().all(|o| o.as_ref() == Some(&Owner::Chrome)));
    // A pane over another, and one reaching past the grid, still paint
    // each cell once: the first listed keeps the shared cells.
    let grid = owners(
        &frame,
        &[pane("a", rect(2, 1, 4, 5)), pane("b", rect(0, 0, 20, 2))],
    );
    assert_eq!(grid[0], vec![Some(b.clone()); 9]);
    assert_eq!(
        grid[1],
        [&b, &b, &a, &a, &a, &a, &b, &b, &b].map(|o| Some(o.clone()))
    );
    assert!(grid[2][2..6].iter().all(|o| o.as_ref() == Some(&a)));
    assert!(
        grid[2][..2]
            .iter()
            .all(|o| o.as_ref() == Some(&Owner::Chrome))
    );
}

/// What upstream checks through a window and entities
/// (`only_regions_whose_paint_changed_are_replaced`): a region's view is
/// kept exactly while the next surface paints what the last one painted.
#[test]
fn a_region_is_kept_only_while_it_paints_the_same_cells() {
    let panes = vec![pane("a", rect(0, 0, 4, 2)), pane("b", rect(5, 0, 4, 2))];
    let grid = frame(&["aaaa|bbbb", "aaaa|bbbb"]);
    let region = Region {
        owner: Owner::Pane("a".into()),
        surface: surface(grid.clone(), panes.clone()),
        area: vec![
            Span {
                row: 0,
                columns: 0..4,
            },
            Span {
                row: 1,
                columns: 0..4,
            },
        ],
        look: look(),
    };
    // The same frame in a new surface repaints nothing.
    let next = Region {
        surface: surface(grid.clone(), panes.clone()),
        ..region.clone()
    };
    assert!(region.paints_like(&next));
    // Output in one cell of the area replaces the region.
    let mut changed_frame = grid.clone();
    changed_frame.cells[2].symbol = "x".into();
    let changed = Region {
        surface: surface(changed_frame, panes.clone()),
        ..region.clone()
    };
    assert!(!region.paints_like(&changed));
    // A cursor outside the area is another region's change.
    let mut elsewhere_frame = grid.clone();
    elsewhere_frame.cursor = Some(CursorState {
        x: 6,
        y: 0,
        visible: true,
        shape: 0,
    });
    let elsewhere = Region {
        surface: surface(elsewhere_frame, panes.clone()),
        ..region.clone()
    };
    assert!(region.paints_like(&elsewhere));
    // A cursor crossing into the area changes it.
    let mut inside_frame = grid.clone();
    inside_frame.cursor = Some(CursorState {
        x: 1,
        y: 1,
        visible: true,
        shape: 0,
    });
    let inside = Region {
        surface: surface(inside_frame, panes.clone()),
        ..region.clone()
    };
    assert!(!region.paints_like(&inside));
    // A scrollbar crossing the area, at a new position, changes it.
    let mut scrolled_panes = panes.clone();
    scrolled_panes[0].scrollbar_rect = Some(rect(3, 0, 1, 2));
    scrolled_panes[0].scroll = Some(PaneSurfaceScrollMetrics {
        offset_from_bottom: 1,
        max_offset_from_bottom: 10,
        viewport_rows: 2,
    });
    let scrolled = Region {
        surface: surface(grid.clone(), scrolled_panes),
        ..region.clone()
    };
    assert!(!region.paints_like(&scrolled));
    // Anything else that changes how the cells paint does too.
    let mut resized = region.clone();
    resized.look.cell_width = 9.;
    assert!(!region.paints_like(&resized));
}
