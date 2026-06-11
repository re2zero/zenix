//! Custom GPUI element for terminal cell rendering.
//! Handles keyboard input, proper monospace rendering, and full background fill.

use alacritty_terminal::{
    term::cell::{Cell, Flags},
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor},
};
use gpui::{
    App, Bounds, Element, ElementId, FocusHandle, Font, FontStyle, FontWeight, GlobalElementId,
    Hsla, InspectorElementId, InputHandler, IntoElement, LayoutId, Pixels, Point, Rgba,
    SharedString, StrikethroughStyle, TextRun, UTF16Selection, UnderlineStyle, Window, fill,
    point, px, relative, rgb,
};
use gpui_component::ActiveTheme as _;

use crate::terminal::{BackendCommand, BackendTx, RenderSnapshot};

// ── Metrics & rects ─────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct TerminalMetrics {
    cell_width: Pixels,
    line_height: Pixels,
    ascent: Pixels,
    descent: Pixels,
}

#[derive(Clone)]
struct LayoutRect {
    row: i32,
    col: i32,
    cells: usize,
    color: Hsla,
}

impl LayoutRect {
    fn paint(&self, origin: Point<Pixels>, metrics: TerminalMetrics, window: &mut Window) {
        let pos = point(
            origin.x + metrics.cell_width * self.col as f32,
            origin.y + metrics.line_height * self.row as f32,
        );
        let size = gpui::size(metrics.cell_width * self.cells as f32, metrics.line_height);
        window.paint_quad(fill(Bounds::new(pos, size), self.color));
    }
}

// ── Batched text run ────────────────────────────────────────────────

#[derive(Clone)]
struct BatchedTextRun {
    row: i32,
    col: i32,
    cell_count: usize,
    text: String,
    style: TextRun,
    font_size: Pixels,
}

impl BatchedTextRun {
    fn new(row: i32, col: i32, ch: char, style: TextRun, font_size: Pixels) -> Self {
        Self { row, col, cell_count: 1, text: ch.to_string(), style, font_size }
    }

    fn can_append(&self, other: &TextRun, row: i32, col: i32) -> bool {
        self.row == row
            && self.col + self.cell_count as i32 == col
            && self.style.font == other.font
            && self.style.color == other.color
            && self.style.underline == other.underline
            && self.style.strikethrough == other.strikethrough
    }

    fn append(&mut self, ch: char) {
        self.text.push(ch);
        self.cell_count += 1;
        self.style.len += ch.len_utf8();
    }

    fn paint(&self, origin: Point<Pixels>, metrics: TerminalMetrics, window: &mut Window, cx: &mut App) {
        // Top-align: subtract GPUI's vertical centering padding
        let pad = px(f32::from(self.font_size) * 0.15);
        let pos = point(
            origin.x + metrics.cell_width * self.col as f32,
            origin.y + metrics.line_height * self.row as f32 - pad,
        );
        let _ = window
            .text_system()
            .shape_line(self.text.clone().into(), self.font_size, std::slice::from_ref(&self.style), Some(metrics.cell_width))
            .paint(pos, metrics.line_height, gpui::TextAlign::Left, None, window, cx);
    }
}

// ── Cursor ──────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct CursorLayout {
    row: usize,
    col: usize,
    shape: CursorShape,
    color: Hsla,
}

// ── PrepaintState ───────────────────────────────────────────────────

pub struct PrepaintState {
    bounds: Bounds<Pixels>,
    metrics: TerminalMetrics,
    rects: Vec<LayoutRect>,
    runs: Vec<BatchedTextRun>,
    cursor: Option<CursorLayout>,
}

// ── Terminal Input Handler ──────────────────────────────────────────

struct TerminalInputHandler {
    backend: BackendTx,
    element_bounds: Bounds<Pixels>,
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut App) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&mut self, _window: &mut Window, _cx: &mut App) -> Option<std::ops::Range<usize>> {
        None
    }

    fn text_for_range(&mut self, _range_utf16: std::ops::Range<usize>, _adjusted_range: &mut Option<std::ops::Range<usize>>, _window: &mut Window, _cx: &mut App) -> Option<String> {
        None
    }

    fn replace_text_in_range(&mut self, _replacement_range: Option<std::ops::Range<usize>>, text: &str, _window: &mut Window, _cx: &mut App) {
        self.backend.send(BackendCommand::Input(text.as_bytes().to_vec()));
    }

    fn replace_and_mark_text_in_range(&mut self, _range_utf16: Option<std::ops::Range<usize>>, _new_text: &str, _new_selected_range: Option<std::ops::Range<usize>>, _window: &mut Window, _cx: &mut App) {}

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut App) {}

    fn bounds_for_range(&mut self, _range_utf16: std::ops::Range<usize>, _window: &mut Window, _cx: &mut App) -> Option<Bounds<Pixels>> {
        Some(self.element_bounds)
    }

    fn character_index_for_point(&mut self, _point: Point<Pixels>, _window: &mut Window, _cx: &mut App) -> Option<usize> {
        None
    }

    fn accepts_text_input(&mut self, _window: &mut Window, _cx: &mut App) -> bool {
        true
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }

    fn prefers_ime_for_printable_keys(&mut self, _window: &mut Window, _cx: &mut App) -> bool {
        true
    }
}

// ── Terminal Element ────────────────────────────────────────────────

pub struct TerminalElement {
    snapshot: RenderSnapshot,
    backend: BackendTx,
    focus_handle: FocusHandle,
    font_family: SharedString,
    font_size: Pixels,
    line_height: Pixels,
    cell_width: Pixels,
    ascent: Pixels,
    descent: Pixels,
}

impl TerminalElement {
    pub fn new(
        snapshot: RenderSnapshot, backend: BackendTx, focus_handle: FocusHandle,
        font_family: impl Into<SharedString>, font_size: Pixels,
        line_height: Pixels, cell_width: Pixels, ascent: Pixels, descent: Pixels,
    ) -> Self {
        Self { snapshot, backend, focus_handle, font_family: font_family.into(),
               font_size, line_height, cell_width, ascent, descent }
    }

    fn cell_run_style(&self, cell: &Cell) -> TextRun {
        let weight = if cell.flags.intersects(Flags::BOLD | Flags::DIM_BOLD) {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let style = if cell.flags.intersects(Flags::ITALIC | Flags::BOLD_ITALIC) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        TextRun {
            len: cell.c.len_utf8(),
            color: Hsla::default(),
            background_color: None,
            font: Font { family: self.font_family.clone(), weight, style, ..Font::default() },
            underline: cell.flags.intersects(Flags::ALL_UNDERLINES).then(|| UnderlineStyle {
                color: None, thickness: px(1.0), wavy: cell.flags.contains(Flags::UNDERCURL),
            }),
            strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle {
                color: None, thickness: px(1.0),
            }),
        }
    }

    fn layout_grid(&self, cx: &App) -> (Vec<LayoutRect>, Vec<BatchedTextRun>) {
        let mut rects: Vec<LayoutRect> = Vec::new();
        let mut runs: Vec<BatchedTextRun> = Vec::new();
        let mut current_run: Option<BatchedTextRun> = None;

        for rc in &self.snapshot.cells {
            let cell = &rc.cell;
            if cell.flags.intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }

            let bg = color_to_hsla(cell.bg, false, cx);
            if !is_default_bg(cell.bg) {
                rects.push(LayoutRect { row: rc.row, col: rc.col, cells: 1, color: bg });
            }

            if is_blank(cell) {
                if let Some(run) = current_run.take() {
                    runs.push(run);
                }
                continue;
            }

            let mut style = self.cell_run_style(cell);
            style.color = color_to_hsla(cell.fg, true, cx);
            if cell.flags.contains(Flags::DIM) { style.color.a *= 0.7; }
            if cell.flags.contains(Flags::INVERSE) {
                style.color = color_to_hsla(cell.bg, false, cx);
            }

            if let Some(run) = current_run.as_mut() {
                if run.can_append(&style, rc.row, rc.col) {
                    run.append(cell.c);
                    continue;
                }
            }
            if let Some(run) = current_run.take() {
                runs.push(run);
            }
            current_run = Some(BatchedTextRun::new(rc.row, rc.col, cell.c, style, self.font_size));
        }
        if let Some(run) = current_run {
            runs.push(run);
        }
        (merge_rects(rects), runs)
    }

    fn cursor_layout(&self, cx: &App) -> Option<CursorLayout> {
        self.snapshot.cursor.map(|c| CursorLayout {
            row: c.row, col: c.col, shape: c.shape, color: cx.theme().primary,
        })
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _request_layout: &mut Self::RequestLayoutState, _window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        let metrics = TerminalMetrics { cell_width: self.cell_width, line_height: self.line_height, ascent: self.ascent, descent: self.descent };
        let (rects, runs) = self.layout_grid(cx);
        PrepaintState { bounds, metrics, rects, runs, cursor: self.cursor_layout(cx) }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, _bounds: Bounds<Pixels>, _request_layout: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        window.paint_quad(fill(prepaint.bounds, cx.theme().background));
        for rect in &prepaint.rects { rect.paint(prepaint.bounds.origin, prepaint.metrics, window); }
        for run in &prepaint.runs { run.paint(prepaint.bounds.origin, prepaint.metrics, window, cx); }
        if let Some(cursor) = &prepaint.cursor {
            if cursor.shape != CursorShape::Hidden {
                let pos = point(prepaint.bounds.origin.x + prepaint.metrics.cell_width * cursor.col as f32, prepaint.bounds.origin.y + prepaint.metrics.line_height * cursor.row as f32);
                let size = gpui::size(prepaint.metrics.cell_width, prepaint.metrics.line_height);
                match cursor.shape {
                    CursorShape::Block | CursorShape::HollowBlock => { window.paint_quad(fill(Bounds::new(pos, size), cursor.color)); }
                    CursorShape::Beam => { window.paint_quad(fill(Bounds::new(pos, gpui::size(px(2.0), size.height)), cursor.color)); }
                    CursorShape::Underline => { window.paint_quad(fill(Bounds::new(point(pos.x, pos.y + size.height - px(2.0)), gpui::size(size.width, px(2.0))), cursor.color)); }
                    CursorShape::Hidden => {}
                }
            }
        }
    }
}

// ── Helper functions ───────────────────────────────────────────────

fn merge_rects(mut rects: Vec<LayoutRect>) -> Vec<LayoutRect> {
    rects.sort_by_key(|r| (r.row, r.col));
    let mut merged: Vec<LayoutRect> = Vec::with_capacity(rects.len());
    for rect in rects {
        if let Some(last) = merged.last_mut() {
            if last.row == rect.row && last.color == rect.color && last.col + last.cells as i32 == rect.col {
                last.cells += rect.cells;
                continue;
            }
        }
        merged.push(rect);
    }
    merged
}

fn is_blank(cell: &Cell) -> bool {
    cell.c == ' ' && !cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
}

fn is_default_bg(color: AnsiColor) -> bool {
    matches!(color, AnsiColor::Named(NamedColor::Background))
}

fn color_to_hsla(color: AnsiColor, _foreground: bool, cx: &App) -> Hsla {
    match color {
        AnsiColor::Spec(rgb) => Hsla::from(Rgba {
            r: rgb.r as f32 / 255.0, g: rgb.g as f32 / 255.0, b: rgb.b as f32 / 255.0, a: 1.0,
        }),
        AnsiColor::Indexed(index) => ansi_index_color(index, cx),
        AnsiColor::Named(named) => named_color(named, cx),
    }
}

fn ansi_index_color(index: u8, _cx: &App) -> Hsla {
    const ANSI_16: [u32; 16] = [
        0x1f2430, 0xff5c57, 0x5af78e, 0xf3f99d, 0x57c7ff, 0xff6ac1, 0x9aedfe, 0xf1f1f0,
        0x686868, 0xff5c57, 0x5af78e, 0xf3f99d, 0x57c7ff, 0xff6ac1, 0x9aedfe, 0xffffff,
    ];
    if (index as usize) < ANSI_16.len() {
        return Hsla::from(rgb(ANSI_16[index as usize]));
    }
    if index >= 232 {
        let gray = 8 + (index - 232) * 10;
        return Hsla::from(Rgba { r: gray as f32 / 255.0, g: gray as f32 / 255.0, b: gray as f32 / 255.0, a: 1.0 });
    }
    let i = index - 16;
    let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
    Hsla::from(Rgba {
        r: conv(i / 36) as f32 / 255.0,
        g: conv((i % 36) / 6) as f32 / 255.0,
        b: conv(i % 6) as f32 / 255.0,
        a: 1.0,
    })
}

fn named_color(named: NamedColor, cx: &App) -> Hsla {
    match named {
        NamedColor::Foreground => cx.theme().foreground,
        NamedColor::Background => cx.theme().background,
        NamedColor::Black => Hsla::from(rgb(0x1f2430)),
        NamedColor::Red => Hsla::from(rgb(0xff5c57)),
        NamedColor::Green => Hsla::from(rgb(0x5af78e)),
        NamedColor::Yellow => Hsla::from(rgb(0xf3f99d)),
        NamedColor::Blue => Hsla::from(rgb(0x57c7ff)),
        NamedColor::Magenta => Hsla::from(rgb(0xff6ac1)),
        NamedColor::Cyan => Hsla::from(rgb(0x9aedfe)),
        NamedColor::White => Hsla::from(rgb(0xf1f1f0)),
        NamedColor::BrightBlack => Hsla::from(rgb(0x686868)),
        NamedColor::BrightRed => Hsla::from(rgb(0xff5c57)),
        NamedColor::BrightGreen => Hsla::from(rgb(0x5af78e)),
        NamedColor::BrightYellow => Hsla::from(rgb(0xf3f99d)),
        NamedColor::BrightBlue => Hsla::from(rgb(0x57c7ff)),
        NamedColor::BrightMagenta => Hsla::from(rgb(0xff6ac1)),
        NamedColor::BrightCyan => Hsla::from(rgb(0x9aedfe)),
        NamedColor::BrightWhite => Hsla::from(rgb(0xffffff)),
        NamedColor::Cursor => cx.theme().primary,
        NamedColor::DimBlack => Hsla { a: 0.5, ..Hsla::from(rgb(0x1f2430)) },
        NamedColor::DimRed => Hsla { a: 0.5, ..Hsla::from(rgb(0xff5c57)) },
        NamedColor::DimGreen => Hsla { a: 0.5, ..Hsla::from(rgb(0x5af78e)) },
        NamedColor::DimYellow => Hsla { a: 0.5, ..Hsla::from(rgb(0xf3f99d)) },
        NamedColor::DimBlue => Hsla { a: 0.5, ..Hsla::from(rgb(0x57c7ff)) },
        NamedColor::DimMagenta => Hsla { a: 0.5, ..Hsla::from(rgb(0xff6ac1)) },
        NamedColor::DimCyan => Hsla { a: 0.5, ..Hsla::from(rgb(0x9aedfe)) },
        NamedColor::DimWhite => Hsla { a: 0.5, ..Hsla::from(rgb(0xf1f1f0)) },
        NamedColor::BrightForeground => cx.theme().foreground,
        NamedColor::DimForeground => Hsla { a: 0.5, ..cx.theme().foreground },
    }
}
