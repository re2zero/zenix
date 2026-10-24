//! Keyboard copy mode: a cursor that walks a pane's text, scrollback
//! included, marks a selection, and copies it, as Herdr's own copy mode does.
//!
//! Cell steps, line starts, pages, and the ends of history are plain
//! arithmetic on screen-buffer coordinates and happen here. Motions that
//! depend on the text (words, line ends, paragraphs) are the daemon's: they
//! go out as `pane.copy_motion`, one at a time, and keys typed meanwhile wait
//! in a bounded queue so a fast typist's sequence still runs in order.
//! Nothing here touches the window or the socket.
//!
//! Ported in shape from herdr-gpui (Apache-2.0)
//! `crates/herdr-gpui/src/copy_mode.rs`. The window's side — entering and
//! leaving the mode, sending motions, reading the marked range and copying
//! it — lives in [`super::view`].

use std::collections::VecDeque;
use std::ops::Range;

use herdr_client::protocol::PaneSurfacePane;
use herdr_client::scrollback::{
    CopyMotion, CopyMotionParams, CopyMotionResult, EndpointErrorCode, TextPoint, TextRange,
};

use super::viewport_top;

/// Keys typed while a motion is in flight. Past this, a held key stops
/// queueing rather than replaying long after it was released.
const MAX_QUEUED: usize = 32;

/// One copy-mode key, already parsed from the keystroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Rows and columns to step; negative moves up or left.
    Step {
        rows: i32,
        cols: i32,
    },
    LineStart,
    /// The first row of history, or the last row of the screen.
    History {
        top: bool,
    },
    /// A page, or half of one, up or down, scrolling the pane with it.
    Page {
        down: bool,
        half: bool,
    },
    Motion(CopyMotion),
    /// Starts (or restarts) a selection at the cursor: by cell, or by line.
    Mark {
        lines: bool,
    },
    /// Copies the selection, if any, and leaves copy mode.
    Copy,
    /// Clears the selection, or leaves copy mode when there is none.
    Cancel,
    Exit,
}

impl Command {
    /// The vi key for `key` with shift and control as given. Keys with any
    /// other modifier are not copy-mode keys.
    pub(crate) fn from_key(key: &str, shift: bool, control: bool) -> Option<Self> {
        use CopyMotion::*;
        let step = |rows, cols| Some(Self::Step { rows, cols });
        if control {
            return match key {
                "u" => Some(Self::Page {
                    down: false,
                    half: true,
                }),
                "d" => Some(Self::Page {
                    down: true,
                    half: true,
                }),
                "b" => Some(Self::Page {
                    down: false,
                    half: false,
                }),
                "f" => Some(Self::Page {
                    down: true,
                    half: false,
                }),
                _ => None,
            };
        }
        match (key, shift) {
            ("left", _) | ("h", false) => step(0, -1),
            ("down", _) | ("j", false) => step(1, 0),
            ("up", _) | ("k", false) => step(-1, 0),
            ("right", _) | ("l", false) => step(0, 1),
            ("pageup", _) => Some(Self::Page {
                down: false,
                half: false,
            }),
            ("pagedown", _) => Some(Self::Page {
                down: true,
                half: false,
            }),
            ("home", _) | ("0", false) => Some(Self::LineStart),
            ("end", _) | ("$", _) | ("4", true) => Some(Self::Motion(LineEnd)),
            ("^", _) | ("6", true) => Some(Self::Motion(FirstNonBlank)),
            ("w", false) => Some(Self::Motion(NextWordStart)),
            ("b", false) => Some(Self::Motion(PreviousWordStart)),
            ("e", false) => Some(Self::Motion(NextWordEnd)),
            ("w", true) => Some(Self::Motion(NextBigWordStart)),
            ("b", true) => Some(Self::Motion(PreviousBigWordStart)),
            ("e", true) => Some(Self::Motion(NextBigWordEnd)),
            ("{", _) | ("[", true) => Some(Self::Motion(PreviousParagraph)),
            ("}", _) | ("]", true) => Some(Self::Motion(NextParagraph)),
            ("g", false) => Some(Self::History { top: true }),
            ("g", true) => Some(Self::History { top: false }),
            ("v", false) | ("space", false) => Some(Self::Mark { lines: false }),
            ("v", true) => Some(Self::Mark { lines: true }),
            ("y", false) | ("enter", _) => Some(Self::Copy),
            ("escape", _) => Some(Self::Cancel),
            ("q", false) => Some(Self::Exit),
            _ => None,
        }
    }

    /// The command a committed character stands for, for text that arrives
    /// through an input method rather than as a keystroke.
    pub(crate) fn from_char(c: char) -> Option<Self> {
        let lower = c.to_ascii_lowercase().to_string();
        let shift = c.is_ascii_uppercase();
        match c {
            ' ' => Self::from_key("space", false, false),
            '$' | '^' | '{' | '}' | '0' => Self::from_key(&c.to_string(), false, false),
            _ => Self::from_key(&lower, shift, false),
        }
    }
}

/// Where a selection was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Cell(TextPoint),
    Line(u32),
}

/// What the window has to do after a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing visible changed, or the command waits behind a motion.
    Nothing,
    /// The cursor or the selection moved; repaint and keep the cursor shown.
    Moved,
    /// Ask the daemon for this motion.
    Motion(CopyMotionParams),
    /// Read this range and copy it, then leave copy mode.
    Copy(TextRange),
    /// Leave copy mode without copying.
    Exit,
}

/// One row of the selection or the cursor, in the paint grid of the pane's
/// frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) row: u16,
    pub(crate) columns: Range<u16>,
    /// Whether this is the cursor cell rather than part of the selection.
    pub(crate) cursor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InFlight {
    request: String,
    origin: TextPoint,
    motion: CopyMotion,
    content_revision: u64,
}

#[derive(Debug)]
pub(crate) struct CopyMode {
    pane_id: String,
    cursor: TextPoint,
    mark: Option<Mark>,
    /// The pane's offset when copy mode began, restored when it ends.
    entry_offset: Option<u64>,
    /// The offset last asked for, so a held key asks once per change.
    asked_offset: Option<u64>,
    in_flight: Option<InFlight>,
    queued: VecDeque<Command>,
    /// A motion the daemon refused as stale, sent again once the surface
    /// shows a newer revision than this.
    retry: Option<(CopyMotion, u64)>,
}

impl CopyMode {
    /// Starts at the terminal's cursor when the pane shows it, otherwise at
    /// the start of the pane's last visible row.
    pub(crate) fn new(pane: &PaneSurfacePane, cursor: Option<(u16, u16)>) -> Self {
        let inner = pane.inner_rect;
        let top = viewport_top(pane);
        let cursor = cursor
            .filter(|(x, y)| {
                (inner.x..inner.x.saturating_add(inner.width)).contains(x)
                    && (inner.y..inner.y.saturating_add(inner.height)).contains(y)
            })
            .map(|(x, y)| TextPoint {
                row: top.saturating_add(u32::from(y - inner.y)),
                col: x - inner.x,
            })
            .unwrap_or(TextPoint {
                row: top.saturating_add(u32::from(inner.height.saturating_sub(1))),
                col: 0,
            });
        Self {
            pane_id: pane.pane_id.clone(),
            cursor,
            mark: None,
            entry_offset: pane.scroll.map(|scroll| scroll.offset_from_bottom),
            asked_offset: None,
            in_flight: None,
            queued: VecDeque::new(),
            retry: None,
        }
    }

    pub(crate) fn pane_id(&self) -> &str {
        &self.pane_id
    }

    pub(crate) fn entry_offset(&self) -> Option<u64> {
        self.entry_offset
    }

    pub(crate) fn in_flight(&self) -> Option<&str> {
        self.in_flight.as_ref().map(|sent| sent.request.as_str())
    }

    /// Runs `command` against `pane` as the surface shows it now.
    pub(crate) fn command(&mut self, command: Command, pane: &PaneSurfacePane) -> Outcome {
        if self.in_flight.is_some() || self.retry.is_some() {
            if self.queued.len() < MAX_QUEUED {
                self.queued.push_back(command);
            }
            return Outcome::Nothing;
        }
        let inner = pane.inner_rect;
        let last_col = inner.width.saturating_sub(1);
        let last_row = last_row(pane);
        match command {
            Command::Step { rows, cols } => {
                self.cursor.row = offset(self.cursor.row, rows).min(last_row);
                self.cursor.col = u16::try_from(offset(u32::from(self.cursor.col), cols))
                    .unwrap_or(u16::MAX)
                    .min(last_col);
            }
            Command::LineStart => self.cursor.col = 0,
            Command::History { top } => {
                self.cursor.row = if top { 0 } else { last_row };
                self.cursor.col = 0;
            }
            Command::Page { down, half } => {
                let height = inner.height;
                let lines = if height <= 2 {
                    1
                } else if half {
                    height / 2
                } else {
                    height - 2
                };
                let lines = i32::from(lines);
                self.cursor.row =
                    offset(self.cursor.row, if down { lines } else { -lines }).min(last_row);
            }
            Command::Motion(motion) => {
                return Outcome::Motion(self.motion_params(motion, pane.content_revision));
            }
            Command::Mark { lines } => {
                self.mark = Some(if lines {
                    Mark::Line(self.cursor.row)
                } else {
                    Mark::Cell(self.cursor)
                });
            }
            Command::Copy => {
                return match self.selection(last_col) {
                    Some(range) => Outcome::Copy(range),
                    None => Outcome::Exit,
                };
            }
            Command::Cancel if self.mark.is_some() => self.mark = None,
            Command::Cancel | Command::Exit => return Outcome::Exit,
        }
        Outcome::Moved
    }

    fn motion_params(&self, motion: CopyMotion, content_revision: u64) -> CopyMotionParams {
        CopyMotionParams {
            pane_id: self.pane_id.clone(),
            cursor: self.cursor,
            motion,
            content_revision: Some(content_revision),
        }
    }

    pub(crate) fn sent(&mut self, request: String, params: &CopyMotionParams) {
        self.retry = None;
        self.in_flight = Some(InFlight {
            request,
            origin: params.cursor,
            motion: params.motion,
            content_revision: params.content_revision.unwrap_or_default(),
        });
    }

    /// The motion could not be queued: drop it and what waited behind it.
    pub(crate) fn send_failed(&mut self) {
        self.in_flight = None;
        self.retry = None;
        self.queued.clear();
    }

    /// Applies the daemon's answer to `request`. `true` when the cursor
    /// moved. A stale answer is retried once the surface moves on.
    pub(crate) fn answer(
        &mut self,
        request: &str,
        answer: herdr_client::Result<CopyMotionResult>,
    ) -> herdr_client::Result<bool> {
        if self
            .in_flight
            .as_ref()
            .is_none_or(|sent| sent.request != request)
        {
            return Ok(false);
        }
        let Some(sent) = self.in_flight.take() else {
            return Ok(false);
        };
        match answer {
            Ok(result) if result.pane_id == self.pane_id && self.cursor == sent.origin => {
                let moved = self.cursor != result.cursor;
                self.cursor = result.cursor;
                Ok(moved)
            }
            Ok(_) => Ok(false),
            Err(herdr_client::Error::Endpoint {
                code: EndpointErrorCode::StaleContent,
                ..
            }) => {
                self.retry = Some((sent.motion, sent.content_revision));
                Ok(false)
            }
            Err(error) => {
                self.queued.clear();
                Err(error)
            }
        }
    }

    /// A refused motion to send again, once `pane` shows settled content
    /// newer than the revision it was refused at.
    pub(crate) fn due_retry(&self, pane: &PaneSurfacePane) -> Option<CopyMotionParams> {
        let (motion, refused) = self.retry?;
        (self.in_flight.is_none()
            && pane.content_revision != refused
            && pane.content_revision.is_multiple_of(2))
        .then(|| self.motion_params(motion, pane.content_revision))
    }

    /// The next key that waited behind a motion, once none is in flight.
    pub(crate) fn next_queued(&mut self) -> Option<Command> {
        if self.in_flight.is_some() || self.retry.is_some() {
            return None;
        }
        self.queued.pop_front()
    }

    /// The offset that brings the cursor on screen, scrolling as little as
    /// possible, when it is off screen and not already asked for.
    pub(crate) fn reveal(&mut self, pane: &PaneSurfacePane) -> Option<u64> {
        let scroll = pane.scroll?;
        let top = viewport_top(pane);
        let height = u32::from(pane.inner_rect.height.max(1));
        let wanted_top = if self.cursor.row < top {
            self.cursor.row
        } else if self.cursor.row >= top.saturating_add(height) {
            self.cursor.row.saturating_sub(height - 1)
        } else {
            self.asked_offset = None;
            return None;
        };
        let offset = scroll
            .max_offset_from_bottom
            .saturating_sub(u64::from(wanted_top));
        if self.asked_offset == Some(offset) || offset == scroll.offset_from_bottom {
            return None;
        }
        self.asked_offset = Some(offset);
        Some(offset)
    }

    /// The selection as a range of cells: by cell from the mark to the
    /// cursor, or whole rows by line.
    fn selection(&self, last_col: u16) -> Option<TextRange> {
        Some(match self.mark? {
            Mark::Cell(mark) => TextRange {
                start: mark.min(self.cursor),
                end: mark.max(self.cursor),
            },
            Mark::Line(row) => TextRange {
                start: TextPoint {
                    row: row.min(self.cursor.row),
                    col: 0,
                },
                end: TextPoint {
                    row: row.max(self.cursor.row),
                    col: last_col,
                },
            },
        })
    }

    /// The selection and the cursor, in the surface frame's grid. Rows are
    /// mapped through the pane's scroll position, so the mode keeps its place
    /// as the pane scrolls under it.
    pub(crate) fn hits(&self, pane: &PaneSurfacePane) -> Vec<Hit> {
        let mut hits = Vec::new();
        if pane.pane_id != self.pane_id {
            return hits;
        }
        if let Some(range) = self.selection(pane.inner_rect.width.saturating_sub(1)) {
            push_range(pane, range, false, &mut hits);
        }
        push_range(
            pane,
            TextRange {
                start: self.cursor,
                end: self.cursor,
            },
            true,
            &mut hits,
        );
        hits
    }
}

/// The last row the pane holds: its history plus its screen.
fn last_row(pane: &PaneSurfacePane) -> u32 {
    let history = pane.scroll.map_or(0, |scroll| {
        u32::try_from(scroll.max_offset_from_bottom).unwrap_or(u32::MAX)
    });
    history.saturating_add(u32::from(pane.inner_rect.height.saturating_sub(1)))
}

fn offset(value: u32, by: i32) -> u32 {
    if by < 0 {
        value.saturating_sub(by.unsigned_abs())
    } else {
        value.saturating_add(by.unsigned_abs())
    }
}

/// Appends the cells of `range` that `pane` shows, in the surface frame's
/// grid; the rows between its ends run the pane's full width.
fn push_range(pane: &PaneSurfacePane, range: TextRange, cursor: bool, hits: &mut Vec<Hit>) {
    let inner = pane.inner_rect;
    let top = viewport_top(pane);
    let bottom = top.saturating_add(u32::from(inner.height));
    if range.end.row < top || range.start.row >= bottom || range.end < range.start {
        return;
    }
    for row in range.start.row.max(top)..=range.end.row.min(bottom.saturating_sub(1)) {
        let start = if row == range.start.row {
            range.start.col
        } else {
            0
        };
        let end = if row == range.end.row {
            range.end.col.saturating_add(1)
        } else {
            inner.width
        }
        .min(inner.width);
        let Ok(offset) = u16::try_from(row - top) else {
            continue;
        };
        if start >= end {
            continue;
        }
        hits.push(Hit {
            row: inner.y.saturating_add(offset),
            columns: inner.x.saturating_add(start)..inner.x.saturating_add(end),
            cursor,
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use herdr_client::protocol::{PaneSurfaceScrollMetrics, SurfaceRect};

    /// A 20x10 pane with 50 rows of history, scrolled `offset` up.
    fn pane(offset: u64) -> PaneSurfacePane {
        let rect = SurfaceRect {
            x: 2,
            y: 1,
            width: 20,
            height: 10,
        };
        PaneSurfacePane {
            pane_id: "p".into(),
            content_revision: 4,
            rect,
            inner_rect: rect,
            scrollbar_rect: None,
            scroll: Some(PaneSurfaceScrollMetrics {
                offset_from_bottom: offset,
                max_offset_from_bottom: 50,
                viewport_rows: 10,
            }),
            focused: true,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 200,
            pixel_height: 100,
        }
    }

    fn point(row: u32, col: u16) -> TextPoint {
        TextPoint { row, col }
    }

    #[test]
    fn keys_parse_as_herdr_copy_mode_defines_them() {
        use CopyMotion::*;
        let key = |key, shift| Command::from_key(key, shift, false);
        assert_eq!(key("w", false), Some(Command::Motion(NextWordStart)));
        assert_eq!(key("w", true), Some(Command::Motion(NextBigWordStart)));
        assert_eq!(key("b", true), Some(Command::Motion(PreviousBigWordStart)));
        assert_eq!(key("e", true), Some(Command::Motion(NextBigWordEnd)));
        assert_eq!(key("4", true), Some(Command::Motion(LineEnd)));
        assert_eq!(key("6", true), Some(Command::Motion(FirstNonBlank)));
        assert_eq!(key("[", true), Some(Command::Motion(PreviousParagraph)));
        assert_eq!(key("g", true), Some(Command::History { top: false }));
        assert_eq!(key("v", true), Some(Command::Mark { lines: true }));
        assert_eq!(key("x", false), None);
        assert_eq!(
            Command::from_key("u", false, true),
            Some(Command::Page {
                down: false,
                half: true
            })
        );
        assert_eq!(Command::from_key("w", false, true), None);
        // Characters an input method commits read the same way.
        assert_eq!(
            Command::from_char('W'),
            Some(Command::Motion(NextBigWordStart))
        );
        assert_eq!(Command::from_char('$'), Some(Command::Motion(LineEnd)));
        assert_eq!(
            Command::from_char('}'),
            Some(Command::Motion(NextParagraph))
        );
        assert_eq!(
            Command::from_char(' '),
            Some(Command::Mark { lines: false })
        );
        assert_eq!(Command::from_char('界'), None);
    }

    #[test]
    fn starts_at_the_terminal_cursor_or_the_last_row() {
        let shown = pane(5);
        // Rows 45..55 show; the cursor at grid (7, 4) is row 48, column 5.
        let mode = CopyMode::new(&shown, Some((7, 4)));
        assert_eq!(mode.cursor, point(48, 5));
        assert_eq!(mode.entry_offset(), Some(5));
        let mode = CopyMode::new(&shown, Some((0, 0)));
        assert_eq!(mode.cursor, point(54, 0), "a cursor outside the pane");
        assert_eq!(CopyMode::new(&shown, None).cursor, point(54, 0));
    }

    #[test]
    fn local_steps_clamp_to_the_pane_and_its_history() {
        let shown = pane(0);
        let mut mode = CopyMode::new(&shown, None);
        assert_eq!(mode.cursor, point(59, 0));
        assert_eq!(
            mode.command(Command::Step { rows: 1, cols: -1 }, &shown),
            Outcome::Moved
        );
        mode.command(Command::Step { rows: 0, cols: 40 }, &shown);
        mode.command(
            Command::Page {
                down: false,
                half: true,
            },
            &shown,
        );
        assert_eq!(mode.cursor, point(54, 19));
        mode.command(
            Command::Page {
                down: false,
                half: false,
            },
            &shown,
        );
        assert_eq!(mode.cursor, point(46, 19));
        mode.command(Command::History { top: true }, &shown);
        assert_eq!(mode.cursor, point(0, 0));
        mode.command(Command::Step { rows: -3, cols: -3 }, &shown);
        assert_eq!(mode.cursor, point(0, 0));
        mode.command(Command::History { top: false }, &shown);
        assert_eq!(mode.cursor, point(59, 0));
    }

    #[test]
    fn motions_go_to_the_daemon_one_at_a_time_and_keys_wait_in_order() {
        let shown = pane(0);
        let mut mode = CopyMode::new(&shown, None);
        let Outcome::Motion(params) =
            mode.command(Command::Motion(CopyMotion::PreviousWordStart), &shown)
        else {
            panic!("a word motion asks the daemon");
        };
        assert_eq!(params.cursor, point(59, 0));
        assert_eq!(params.content_revision, Some(4));
        mode.sent("1".into(), &params);
        assert_eq!(
            mode.command(Command::Step { rows: 0, cols: 1 }, &shown),
            Outcome::Nothing
        );
        assert_eq!(mode.next_queued(), None, "still in flight");
        let landed = CopyMotionResult {
            pane_id: "p".into(),
            cursor: point(58, 12),
            content_revision: 4,
        };
        assert!(!mode.answer("other", Ok(landed.clone())).unwrap());
        assert!(mode.answer("1", Ok(landed)).unwrap());
        assert_eq!(mode.cursor, point(58, 12));
        let next = mode.next_queued().unwrap();
        assert_eq!(mode.command(next, &shown), Outcome::Moved);
        assert_eq!(mode.cursor, point(58, 13));
        // A held key stops queueing at the bound.
        let params = mode.motion_params(CopyMotion::NextWordEnd, 4);
        mode.sent("2".into(), &params);
        for _ in 0..100 {
            mode.command(Command::Step { rows: 1, cols: 0 }, &shown);
        }
        assert_eq!(mode.queued.len(), MAX_QUEUED);
    }

    #[test]
    fn a_stale_motion_retries_on_newer_settled_content() {
        let mut shown = pane(0);
        let mut mode = CopyMode::new(&shown, None);
        let params = mode.motion_params(CopyMotion::NextParagraph, 4);
        mode.sent("1".into(), &params);
        let stale = herdr_client::Error::Endpoint {
            code: EndpointErrorCode::StaleContent,
            message: "pane content changed".into(),
        };
        assert!(!mode.answer("1", Err(stale)).unwrap());
        mode.command(Command::Step { rows: -1, cols: 0 }, &shown);
        assert!(mode.due_retry(&shown).is_none(), "same revision");
        shown.content_revision = 5;
        assert!(mode.due_retry(&shown).is_none(), "mid-write");
        shown.content_revision = 6;
        let retry = mode.due_retry(&shown).unwrap();
        assert_eq!(retry.motion, CopyMotion::NextParagraph);
        assert_eq!(retry.content_revision, Some(6));
        mode.sent("2".into(), &retry);
        // Any other failure drops what waited behind it.
        let failure = herdr_client::Error::Endpoint {
            code: EndpointErrorCode::Other("copy_motion_unavailable".into()),
            message: "terminal row is unavailable".into(),
        };
        assert!(mode.answer("2", Err(failure)).is_err());
        assert_eq!(mode.next_queued(), None);
    }

    #[test]
    fn marks_select_by_cell_or_line_and_copy_or_cancel() {
        let shown = pane(0);
        let mut mode = CopyMode::new(&shown, Some((5, 9)));
        assert_eq!(
            mode.command(Command::Copy, &shown),
            Outcome::Exit,
            "nothing marked"
        );
        mode.command(Command::Mark { lines: false }, &shown);
        mode.command(Command::Step { rows: -2, cols: -1 }, &shown);
        assert_eq!(
            mode.command(Command::Copy, &shown),
            Outcome::Copy(TextRange {
                start: point(56, 2),
                end: point(58, 3),
            })
        );
        mode.command(Command::Mark { lines: true }, &shown);
        mode.command(Command::Step { rows: -2, cols: 0 }, &shown);
        assert_eq!(
            mode.command(Command::Copy, &shown),
            Outcome::Copy(TextRange {
                start: point(54, 0),
                end: point(56, 19),
            })
        );
        mode.command(Command::Mark { lines: true }, &shown);
        mode.command(Command::Cancel, &shown);
        assert_eq!(
            mode.command(Command::Cancel, &shown),
            Outcome::Exit,
            "a second cancel leaves"
        );
    }

    #[test]
    fn the_cursor_scrolls_into_view_and_the_selection_paints() {
        let mut shown = pane(0);
        let mut mode = CopyMode::new(&shown, None);
        // The cursor starts on the last visible row and asks nothing.
        assert_eq!(mode.reveal(&shown), None);
        mode.command(Command::History { top: true }, &shown);
        // Row 0 is above the viewport (rows 50..60): scroll to the top.
        assert_eq!(mode.reveal(&shown), Some(50));
        assert_eq!(mode.reveal(&shown), None, "asked once");
        shown.scroll.as_mut().unwrap().offset_from_bottom = 50;
        mode.command(Command::Mark { lines: false }, &shown);
        mode.command(Command::Step { rows: 1, cols: 2 }, &shown);
        let hits = mode.hits(&shown);
        assert_eq!(
            hits,
            vec![
                Hit {
                    row: 1,
                    columns: 2..22,
                    cursor: false,
                },
                Hit {
                    row: 2,
                    columns: 2..5,
                    cursor: false,
                },
                Hit {
                    row: 2,
                    columns: 4..5,
                    cursor: true,
                },
            ]
        );
    }
}
