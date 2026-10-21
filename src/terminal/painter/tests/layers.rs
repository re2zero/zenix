use super::*;

/// Each layer paints only its own share of the grid: a region that paints one
/// layer must not also paint another layer's cells, and the three layers
/// together must be the whole.
///
/// Upstream compares quad and glyph counts from its `integration-test`
/// instrumentation. zenix does not build that feature (M3 dropped it), so the
/// share this test can observe is the decorations: the same cells split the
/// same way, and a regression that made one layer paint another's cells would
/// show up here as a count that is too high.
#[gpui_kit::test]
fn each_layer_paints_only_its_share_of_the_decorations(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Empty);
    cx.draw(Point::default(), size(px(800.), px(600.)), |_, _| {
        canvas(
            |_, _, _| (),
            |bounds, _, window, cx| {
                let frame = FrameData {
                    width: 3,
                    height: 1,
                    cells: vec![
                        cell("a"),
                        CellData {
                            modifier: UNDERLINE,
                            bg: 0x02123456,
                            ..cell("b")
                        },
                        cell("┼"),
                    ],
                    cursor: Some(herdr_client::protocol::CursorState {
                        x: 0,
                        y: 0,
                        visible: true,
                        shape: 0,
                    }),
                    hyperlinks: vec![],
                    graphics: vec![],
                };
                let area = whole(&frame);
                let mut painter = TerminalPainter::default();
                painter.set_appearance(21.35, 30.5, Theme::default());
                let mut paint = |part: Option<Part<'_>>| {
                    painter.painted_decorations.clear();
                    painter.paint_frame(
                        &frame,
                        bounds.origin,
                        None,
                        12.81,
                        &font("Menlo"),
                        &[],
                        part,
                        None,
                        window,
                        cx,
                    );
                    painter.painted_decorations.len()
                };
                let all = paint(None);
                let [backgrounds, text, decorations] =
                    Layer::ALL.map(|layer| paint(Some(Part { area: &area, layer })));
                // One cell is underlined, and only the decorations layer draws it.
                assert_eq!(all, 1);
                assert_eq!((backgrounds, text), (0, 0));
                assert_eq!(decorations, 1);
            },
        )
        .size_full()
    });
}
