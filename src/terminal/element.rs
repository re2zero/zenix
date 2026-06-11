//! GPUI terminal element — ported from zed's rendering approach.
//!
//! Key improvements over the original:
//! - Device-pixel-snapped origins prevent box-drawing character gaps
//! - Floor/ceil alignment on background rects eliminates subpixel seams
//! - Full IME InputHandler with marked-text (preedit) rendering
//! - BatchedTextRun uses shape_line correctly (no manual padding hack)

use std::ops::Range;

use alacritty_terminal::{
  term::cell::{Cell, Flags},
  vte::ansi::{Color as AnsiColor, CursorShape, NamedColor},
};
use gpui::{
  App, Bounds, Element, ElementId, FocusHandle, Font, FontFeatures, FontStyle, FontWeight,
  GlobalElementId, Hsla, InspectorElementId, InputHandler, IntoElement, LayoutId, Pixels, Point,
  Rgba, SharedString, StrikethroughStyle, TextAlign, TextRun, UTF16Selection, UnderlineStyle,
  Window, fill, point, px, relative, rgb, size,
};
use gpui_component::ActiveTheme as _;

use crate::app::ZenixApp;
use crate::terminal::RenderSnapshot;
// ── Metrics ───────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct TerminalMetrics {
  cell_width: Pixels,
  line_height: Pixels,
  ascent: Pixels,
  descent: Pixels,
}

// ── IME state (shared between app and element) ────────────────────────

#[derive(Clone, Debug)]
pub struct ImeState {
  pub marked_text: String,
}

// ── Layout rect (pixel-aligned background fill) ───────────────────────

#[derive(Clone)]
struct LayoutRect {
  line: i32,
  col: i32,
  cells: usize,
  color: Hsla,
}

impl LayoutRect {
  /// Paint with device-pixel alignment: floor x, ceil width.
  /// Zed's approach — eliminates subpixel seams between adjacent rects.
  fn paint(&self, origin: Point<Pixels>, metrics: TerminalMetrics, window: &mut Window) {
    let x = (origin.x + metrics.cell_width * self.col as f32).floor();
    let y = origin.y + metrics.line_height * self.line as f32;
    let w = (metrics.cell_width * self.cells as f32).ceil();
    window.paint_quad(fill(
      Bounds::new(point(x, y), size(w, metrics.line_height)),
      self.color,
    ));
  }
}

// ── Batched text run ──────────────────────────────────────────────────

#[derive(Clone)]
struct BatchedTextRun {
  line: i32,
  col: i32,
  cell_count: usize,
  text: String,
  style: TextRun,
  font_size: Pixels,
}

impl BatchedTextRun {
  fn new(line: i32, col: i32, ch: char, style: TextRun, font_size: Pixels) -> Self {
    let mut text = String::with_capacity(100);
    text.push(ch);
    let mut style = style;
    style.len = ch.len_utf8();
    Self { line, col, cell_count: 1, text, style, font_size }
  }

  fn can_append(&self, other: &TextRun, line: i32, col: i32) -> bool {
    self.line == line
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

  /// Paint using shape_line with cell_width constraint, same as zed.
  /// NO manual top-align padding — trust GPUI's line_height centering.
  fn paint(&self, origin: Point<Pixels>, metrics: TerminalMetrics, window: &mut Window, cx: &mut App) {
    let pos = point(
      origin.x + metrics.cell_width * self.col as f32,
      origin.y + metrics.line_height * self.line as f32,
    );
    let _ = window
      .text_system()
      .shape_line(
        self.text.clone().into(),
        self.font_size,
        std::slice::from_ref(&self.style),
        Some(metrics.cell_width),
      )
      .paint(pos, metrics.line_height, TextAlign::Left, None, window, cx);
  }
}

// ── Cursor ────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct CursorLayout {
  line: i32,
  col: i32,
  shape: CursorShape,
  color: Hsla,
}

impl CursorLayout {
  fn bounds(&self, origin: Point<Pixels>, metrics: TerminalMetrics) -> Bounds<Pixels> {
    let pos = point(
      origin.x + metrics.cell_width * self.col as f32,
      origin.y + metrics.line_height * self.line as f32,
    );
    Bounds::new(pos, size(metrics.cell_width, metrics.line_height))
  }
}

// ── Prepaint state ────────────────────────────────────────────────────

pub struct PrepaintState {
  bounds: Bounds<Pixels>,
  metrics: TerminalMetrics,
  origin: Point<Pixels>,
  rects: Vec<LayoutRect>,
  runs: Vec<BatchedTextRun>,
  cursor: Option<CursorLayout>,
  ime_cursor_bounds: Option<Bounds<Pixels>>,
}

// ── Terminal Input Handler (IME support) ──────────────────────────────

struct TerminalInputHandler {
  app: gpui::Entity<ZenixApp>,
  cursor_bounds: Option<Bounds<Pixels>>,
}

impl InputHandler for TerminalInputHandler {
  fn selected_text_range(
    &mut self,
    _ignore_disabled_input: bool,
    _window: &mut Window,
    _cx: &mut App,
  ) -> Option<UTF16Selection> {
    Some(UTF16Selection { range: 0..0, reversed: false })
  }

  fn marked_text_range(
    &mut self,
    _window: &mut Window,
    cx: &mut App,
  ) -> Option<Range<usize>> {
    self.app.read(cx).marked_text_range()
  }

  fn text_for_range(
    &mut self,
    _range_utf16: Range<usize>,
    _adjusted_range: &mut Option<Range<usize>>,
    _window: &mut Window,
    _cx: &mut App,
  ) -> Option<String> {
    None
  }

  fn replace_text_in_range(
    &mut self,
    _replacement_range: Option<Range<usize>>,
    text: &str,
    _window: &mut Window,
    cx: &mut App,
  ) {
    self.app.update(cx, |this, cx| {
      this.clear_marked_text(cx);
      this.commit_text(text, cx);
    });
  }

  fn replace_and_mark_text_in_range(
    &mut self,
    _range_utf16: Option<Range<usize>>,
    new_text: &str,
    _new_selected_range: Option<Range<usize>>,
    _window: &mut Window,
    cx: &mut App,
  ) {
    self.app.update(cx, |this, cx| {
      this.set_marked_text(new_text.to_string(), cx);
    });
  }

  fn unmark_text(&mut self, _window: &mut Window, cx: &mut App) {
    self.app.update(cx, |this, cx| {
      this.clear_marked_text(cx);
    });
  }

  fn bounds_for_range(
    &mut self,
    _range_utf16: Range<usize>,
    _window: &mut Window,
    _cx: &mut App,
  ) -> Option<Bounds<Pixels>> {
    self.cursor_bounds
  }

  fn character_index_for_point(
    &mut self,
    _point: Point<Pixels>,
    _window: &mut Window,
    _cx: &mut App,
  ) -> Option<usize> {
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

// ── Terminal Element ──────────────────────────────────────────────────
pub struct TerminalElement {
  snapshot: RenderSnapshot,
  app: gpui::Entity<ZenixApp>,
  focus_handle: FocusHandle,
  font_family: SharedString,
  font_size: Pixels,
  line_height: Pixels,
  cell_width: Pixels,
  ascent: Pixels,
  descent: Pixels,
  ime_state: Option<ImeState>,
}

impl TerminalElement {
  pub fn new(
    snapshot: RenderSnapshot,
    app: gpui::Entity<ZenixApp>,
    focus_handle: FocusHandle,
    font_family: impl Into<SharedString>,
    font_size: Pixels,
    line_height: Pixels,
    cell_width: Pixels,
    ascent: Pixels,
    descent: Pixels,
    ime_state: Option<ImeState>,
  ) -> Self {
    Self {
      snapshot,
      app,
      focus_handle,
      font_family: font_family.into(),
      font_size,
      line_height,
      cell_width,
      ascent,
      descent,
      ime_state,
    }
  }

  fn text_run_style(&self, cell: &Cell, fg: Hsla) -> TextRun {
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
      len: 0,
      color: fg,
      background_color: None,
      font: Font {
        family: self.font_family.clone(),
        features: FontFeatures::disable_ligatures(),
        weight,
        style,
        ..Font::default()
      },
      underline: cell
        .flags
        .intersects(Flags::ALL_UNDERLINES)
        .then(|| UnderlineStyle {
          color: None,
          thickness: px(1.0),
          wavy: cell.flags.contains(Flags::UNDERCURL),
        }),
      strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle {
        color: None,
        thickness: px(1.0),
      }),
    }
  }

  fn layout_grid(&self, cx: &App) -> (Vec<LayoutRect>, Vec<BatchedTextRun>) {
    let estimated = self.snapshot.cells.len();
    let mut rects: Vec<LayoutRect> = Vec::with_capacity(estimated / 4);
    let mut runs: Vec<BatchedTextRun> = Vec::with_capacity(estimated / 10);
    let mut current_run: Option<BatchedTextRun> = None;

    for rc in &self.snapshot.cells {
      let cell = &rc.cell;

      // Skip hidden and spacer cells.
      if cell
        .flags
        .intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
      {
        continue;
      }

      // Background rect (skip default bg).
      if !is_default_bg(cell.bg) {
        let color = color_to_hsla(cell.bg, cx);
        // Try to extend last rect on same line, same color, adjacent column.
        if let Some(last) = rects.last_mut() {
          if last.line == rc.row
            && last.color == color
            && last.col + last.cells as i32 == rc.col
          {
            last.cells += 1;
          } else {
            rects.push(LayoutRect { line: rc.row, col: rc.col, cells: 1, color });
          }
        } else {
          rects.push(LayoutRect { line: rc.row, col: rc.col, cells: 1, color });
        }
      }

      // Skip blank cells (spaces without decorations).
      if is_blank(cell) {
        if let Some(run) = current_run.take() {
          runs.push(run);
        }
        continue;
      }

      // Resolve foreground color with inverse.
      let (mut fg_color, mut bg_color) = (cell.fg, cell.bg);
      if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg_color, &mut bg_color);
      }
      let fg = color_to_hsla(fg_color, cx);
      // Dim: reduce alpha.
      let fg = if cell.flags.contains(Flags::DIM) { Hsla { a: fg.a * 0.7, ..fg } } else { fg };

      let style = self.text_run_style(cell, fg);

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

    (rects, runs)
  }

  fn cursor_layout(&self, cx: &App) -> Option<CursorLayout> {
    self.snapshot.cursor.map(|c| CursorLayout {
      line: c.row as i32,
      col: c.col as i32,
      shape: c.shape,
      color: cx.theme().primary,
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

  fn id(&self) -> Option<ElementId> {
    None
  }

  fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
    None
  }

  fn request_layout(
    &mut self,
    _id: Option<&GlobalElementId>,
    _inspector_id: Option<&InspectorElementId>,
    window: &mut Window,
    cx: &mut App,
  ) -> (LayoutId, Self::RequestLayoutState) {
    let mut style = gpui::Style::default();
    style.size.width = relative(1.).into();
    style.size.height = relative(1.).into();
    (window.request_layout(style, None, cx), ())
  }

  fn prepaint(
    &mut self,
    _id: Option<&GlobalElementId>,
    _inspector_id: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    _request_layout: &mut Self::RequestLayoutState,
    window: &mut Window,
    cx: &mut App,
  ) -> Self::PrepaintState {
    // Snap origin to device pixels — prevents subpixel flicker (zed's approach).
    let sf = window.scale_factor();
    let snap = |v: Pixels| Pixels::from((f32::from(v) * sf).floor() / sf.max(1.0));
    let origin = point(snap(bounds.origin.x), snap(bounds.origin.y));

    let metrics = TerminalMetrics {
      cell_width: self.cell_width,
      line_height: self.line_height,
      ascent: self.ascent,
      descent: self.descent,
    };

    let (rects, runs) = self.layout_grid(cx);
    let cursor = self.cursor_layout(cx);
    let ime_cursor_bounds = cursor.map(|c| c.bounds(origin, metrics));

    PrepaintState { bounds, metrics, origin, rects, runs, cursor, ime_cursor_bounds }
  }

  fn paint(
    &mut self,
    _id: Option<&GlobalElementId>,
    _inspector_id: Option<&InspectorElementId>,
    _bounds: Bounds<Pixels>,
    _request_layout: &mut Self::RequestLayoutState,
    prepaint: &mut Self::PrepaintState,
    window: &mut Window,
    cx: &mut App,
  ) {
    let origin = prepaint.origin;
    let metrics = prepaint.metrics;

    // Background fill.
    window.paint_quad(fill(prepaint.bounds, cx.theme().background));

    // Register IME input handler — this is the key fix for Chinese input.
    let handler = TerminalInputHandler {
      app: self.app.clone(),
      cursor_bounds: prepaint.ime_cursor_bounds,
    };
    window.handle_input(&self.focus_handle, handler, cx);

    // Paint background rects (non-default cell backgrounds).
    for rect in &prepaint.rects {
      rect.paint(origin, metrics, window);
    }

    // Paint batched text runs.
    for run in &prepaint.runs {
      run.paint(origin, metrics, window, cx);
    }

    // Paint IME marked text (preedit) with underline — hides cursor during composition.
    if let Some(ime_state) = &self.ime_state {
      if !ime_state.marked_text.is_empty() {
        if let Some(ime_bounds) = prepaint.ime_cursor_bounds {
          let ime_pos = ime_bounds.origin;
          let marked = &ime_state.marked_text;

          let ime_style = TextRun {
            len: marked.len(),
            font: Font {
              family: self.font_family.clone(),
              ..Font::default()
            },
            color: cx.theme().foreground,
            underline: Some(UnderlineStyle {
              color: Some(cx.theme().foreground),
              thickness: px(1.0),
              wavy: false,
            }),
            ..Default::default()
          };

          let shaped = window.text_system().shape_line(
            marked.clone().into(),
            self.font_size,
            std::slice::from_ref(&ime_style),
            None,
          );

          // Background to cover terminal text behind marked text.
          window.paint_quad(fill(
            Bounds::new(ime_pos, size(shaped.width, metrics.line_height)),
            cx.theme().background,
          ));

          let _ = shaped
            .paint(ime_pos, metrics.line_height, TextAlign::Left, None, window, cx);
        }
      }
    }

    // Paint cursor (hidden during IME composition).
    if self.ime_state.is_none() || self.ime_state.as_ref().is_some_and(|s| s.marked_text.is_empty()) {
      if let Some(cursor) = &prepaint.cursor {
        if !matches!(cursor.shape, CursorShape::Hidden) {
          let pos = point(
            origin.x + metrics.cell_width * cursor.col as f32,
            origin.y + metrics.line_height * cursor.line as f32,
          );
          let sz = size(metrics.cell_width, metrics.line_height);
          match cursor.shape {
            CursorShape::Block | CursorShape::HollowBlock => {
              window.paint_quad(fill(Bounds::new(pos, sz), cursor.color));
            }
            CursorShape::Beam => {
              window.paint_quad(fill(
                Bounds::new(pos, size(px(2.0), sz.height)),
                cursor.color,
              ));
            }
            CursorShape::Underline => {
              window.paint_quad(fill(
                Bounds::new(
                  point(pos.x, pos.y + sz.height - px(2.0)),
                  size(sz.width, px(2.0)),
                ),
                cursor.color,
              ));
            }
            CursorShape::Hidden => {}
          }
        }
      }
    }
  }
}

// ── Helpers ───────────────────────────────────────────────────────────

fn is_blank(cell: &Cell) -> bool {
  cell.c == ' ' && !cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
}

fn is_default_bg(color: AnsiColor) -> bool {
  matches!(color, AnsiColor::Named(NamedColor::Background))
}

fn color_to_hsla(color: AnsiColor, cx: &App) -> Hsla {
  match color {
    AnsiColor::Spec(rgb) => Hsla::from(Rgba {
      r: rgb.r as f32 / 255.0,
      g: rgb.g as f32 / 255.0,
      b: rgb.b as f32 / 255.0,
      a: 1.0,
    }),
    AnsiColor::Indexed(index) => ansi_index_color(index),
    AnsiColor::Named(named) => named_color(named, cx),
  }
}

fn ansi_index_color(index: u8) -> Hsla {
  const ANSI_16: [u32; 16] = [
    0x1f2430, 0xff5c57, 0x5af78e, 0xf3f99d, 0x57c7ff, 0xff6ac1, 0x9aedfe, 0xf1f1f0,
    0x686868, 0xff5c57, 0x5af78e, 0xf3f99d, 0x57c7ff, 0xff6ac1, 0x9aedfe, 0xffffff,
  ];
  if (index as usize) < ANSI_16.len() {
    return Hsla::from(rgb(ANSI_16[index as usize]));
  }
  if index >= 232 {
    let gray = 8 + (index - 232) * 10;
    return Hsla::from(Rgba {
      r: gray as f32 / 255.0,
      g: gray as f32 / 255.0,
      b: gray as f32 / 255.0,
      a: 1.0,
    });
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
