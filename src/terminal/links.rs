//! Links painted in a pane's cells, and what a click opens.
//!
//! A row is read locally: an explicit OSC 8 hyperlink is authoritative, and a
//! plain `http(s)://` address is found by scanning the row's text. The daemon's
//! `pane.link.activate` is asked first for a pane cell, so a plugin link
//! handler can claim the click and a URL wrapped across rows is read whole;
//! when nothing claims it the client opens the address it read itself.
//!
//! Ported in shape from herdr-gpui (Apache-2.0)
//! `crates/herdr-gpui/src/{links.rs,terminal/links.rs}`. File paths and
//! `pane.link.resolve` hover resolution stay upstream: this port opens web
//! addresses only.

use std::ops::Range;

use herdr_client::protocol::{FrameData, PaneSurfaceFrame, SurfaceRect};
use serde_json::{Value, json};

use crate::terminal::selection::shown;
use crate::terminal::{HIDDEN, InputTarget, popup_origin, wheel_target};

pub(crate) const MAX_ROW_BYTES: usize = 32768;

/// What a row-local link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// A web address that passed [`web_url`]'s check.
    Web(String),
}

/// One pane cell and the content it was read from. The daemon refuses a
/// request whose pane has since changed, scrolled, or moved, and this client
/// drops a press for a cell the pointer has left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkCell {
    boot: String,
    pane_id: String,
    inner_rect: SurfaceRect,
    content_revision: u64,
    offset_from_bottom: Option<u64>,
    /// Viewport row and column within the pane's inner rect.
    row: u16,
    col: u16,
}

impl LinkCell {
    /// The pane cell under a point in surface pixels. A popup covers every
    /// pane, and the daemon resolves pane content only, so a popup yields
    /// nothing here and keeps the local link.
    pub(crate) fn at(
        surface: &PaneSurfaceFrame,
        x: f32,
        y: f32,
        cell_width: f32,
        cell_height: f32,
    ) -> Option<Self> {
        let InputTarget::Pane(pane_id) =
            wheel_target(surface, x, y, cell_width, cell_height)?.target
        else {
            return None;
        };
        let pane = surface.panes.iter().find(|pane| pane.pane_id == pane_id)?;
        // An odd revision is the daemon's write-in-progress marker, which it
        // would refuse as stale.
        if pane.content_revision % 2 != 0 {
            return None;
        }
        let rect = pane.inner_rect;
        let col = ((x / cell_width).floor() as u16).checked_sub(rect.x)?;
        let row = ((y / cell_height).floor() as u16).checked_sub(rect.y)?;
        (col < rect.width && row < rect.height).then(|| Self {
            boot: surface.boot_id.clone(),
            pane_id,
            inner_rect: rect,
            content_revision: pane.content_revision,
            offset_from_bottom: pane.scroll.map(|scroll| scroll.offset_from_bottom),
            row,
            col,
        })
    }

    /// Whether both cells read the same pane content, wherever in it they are.
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        self.boot == other.boot
            && self.pane_id == other.pane_id
            && self.inner_rect == other.inner_rect
            && self.content_revision == other.content_revision
            && self.offset_from_bottom == other.offset_from_bottom
    }

    fn params(&self) -> Value {
        json!({
            "pane_id": self.pane_id,
            "viewport_row": self.row,
            "col": self.col,
            "content_revision": self.content_revision,
            "offset_from_bottom": self.offset_from_bottom,
        })
    }

    /// The request that lets a plugin link handler claim this cell.
    pub(crate) fn activate_request(&self) -> (herdr_client::Method, Value) {
        (herdr_client::Method::PaneLinkActivate, self.params())
    }
}

/// A link under a point, in the grid of the frame that paints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkHit {
    pub(crate) target: LinkTarget,
    /// The pane cell the daemon is asked to activate, when it can be.
    pub(crate) cell: Option<LinkCell>,
    /// The row of the frame the columns index, and the columns themselves.
    pub(crate) row: u16,
    pub(crate) columns: Range<u16>,
}

impl LinkHit {
    /// The address this hit opens, as the row read it.
    pub(crate) fn label(&self) -> &str {
        match &self.target {
            LinkTarget::Web(url) => url,
        }
    }

    /// Whether a release reads the same link as its press did: same address,
    /// same cells, and the same pane content behind them.
    pub(crate) fn same_link(&self, other: &Self) -> bool {
        self.target == other.target
            && self.row == other.row
            && self.columns == other.columns
            && match (&self.cell, &other.cell) {
                (Some(mine), Some(theirs)) => mine.same_content(theirs),
                (None, None) => true,
                _ => false,
            }
    }
}

/// A bounded web address: the schemes a browser can open, and no whitespace or
/// control characters, so a hostile pane cannot smuggle another scheme or an
/// argument list through a click.
pub(crate) fn web_url(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let scheme_ok = lower.starts_with("http://") || lower.starts_with("https://");
    let clean = !value.is_empty()
        && value.len() <= 8192
        && !value.chars().any(|c| c.is_whitespace() || c.is_control());
    (scheme_ok && clean).then(|| value.to_owned())
}

/// The link under a point, if any.
pub(crate) fn link_at(
    surface: &PaneSurfaceFrame,
    x: f32,
    y: f32,
    cell_width: f32,
    cell_height: f32,
) -> Option<LinkHit> {
    let hit = wheel_target(surface, x, y, cell_width, cell_height)?;
    let (frame, x, y, start, end, cell) = match &hit.target {
        InputTarget::Popup(_) => {
            let popup = surface.popup.as_ref()?;
            let origin = popup_origin(&surface.frame, &popup.frame, cell_width, cell_height);
            (
                &popup.frame,
                x - origin.x.to_f64() as f32,
                y - origin.y.to_f64() as f32,
                0,
                popup.frame.width,
                None,
            )
        }
        InputTarget::Pane(id) => {
            let pane = surface.panes.iter().find(|pane| pane.pane_id == *id)?;
            (
                &surface.frame,
                x,
                y,
                pane.inner_rect.x,
                pane.inner_rect.x.saturating_add(pane.inner_rect.width),
                LinkCell::at(surface, x, y, cell_width, cell_height),
            )
        }
    };
    let column = (x / cell_width).floor() as u16;
    let row = (y / cell_height).floor() as u16;
    let (target, columns) = frame_link(frame, column, row, start, end)?;
    Some(LinkHit {
        target,
        cell,
        row,
        columns,
    })
}

/// The link in one row, with its columns counted in the frame's grid.
fn frame_link(
    frame: &FrameData,
    column: u16,
    row: u16,
    start: u16,
    end: u16,
) -> Option<(LinkTarget, Range<u16>)> {
    if column < start || column >= end || end > frame.width || row >= frame.height {
        return None;
    }
    let offset = usize::from(row) * usize::from(frame.width);
    let cells = frame
        .cells
        .get(offset + usize::from(start)..offset + usize::from(end))?;
    let selected = usize::from(column - start);
    if selected >= cells.len() {
        return None;
    }
    let mut source = selected;
    while cells.get(source)?.skip && source > 0 {
        source -= 1;
    }
    let cell = cells.get(source)?;
    if cell.modifier & HIDDEN != 0 {
        return None;
    }
    let columns = |cells: Range<usize>| start + cells.start as u16..start + cells.end as u16;
    // An explicit link is authoritative, even if its destination is disallowed.
    if let Some(index) = cells[selected].hyperlink.or(cell.hyperlink) {
        let destination = frame.hyperlinks.get(index as usize)?;
        let target = LinkTarget::Web(web_url(destination)?);
        let on_link = |i: &usize| cells[*i].skip || cells[*i].hyperlink == Some(index);
        let first = (0..=source)
            .rev()
            .take_while(on_link)
            .last()
            .unwrap_or(source);
        let last = (selected..cells.len())
            .take_while(on_link)
            .last()
            .unwrap_or(selected);
        return Some((target, columns(first..last + 1)));
    }
    // Byte offsets into the row's text, so a link maps back to its cells.
    let mut text = String::new();
    let mut starts = Vec::with_capacity(cells.len());
    for cell in cells {
        starts.push(text.len());
        if cell.skip {
            continue;
        }
        let symbol = shown(cell);
        if text.len() + symbol.len() > MAX_ROW_BYTES {
            return None;
        }
        text.push_str(symbol);
    }
    let hit = *starts.get(source)?;
    let (range, _) = plain_url(&text, hit)?;
    let url = web_url(&text[range.clone()])?;
    let first = (0..cells.len()).find(|&i| !cells[i].skip && starts[i] >= range.start)?;
    let last = (0..cells.len())
        .rev()
        .find(|&i| !cells[i].skip && starts[i] < range.end)?;
    // A wide character's continuation cells belong to the link too.
    let end = (last + 1..cells.len())
        .find(|&i| !cells[i].skip)
        .unwrap_or(cells.len());
    Some((LinkTarget::Web(url), columns(first..end)))
}

/// The byte range of the plain web URL in one row's `text` that covers the
/// byte at `hit`, trimmed of the prose punctuation around it, and whether the
/// URL runs to the end of the row, where it may continue off-screen or on the
/// next row.
pub(crate) fn plain_url(text: &str, hit: usize) -> Option<(Range<usize>, bool)> {
    for (start, _) in text.match_indices("http") {
        if start > hit {
            break;
        }
        let tail = &text[start..];
        if !tail.starts_with("http://") && !tail.starts_with("https://") {
            continue;
        }
        let end = tail
            .find(|c: char| {
                c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\'' | '`')
            })
            .unwrap_or(tail.len());
        // Check the original token: punctuation at the edge may be part of a
        // destination continuing off-screen or on the next row.
        let open = end == tail.len();
        let mut candidate = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            let excess = candidate
                .matches(close)
                .count()
                .saturating_sub(candidate.matches(open).count());
            for _ in 0..excess {
                let Some(trimmed) = candidate.strip_suffix(close) else {
                    break;
                };
                candidate = trimmed;
            }
        }
        if hit < start + candidate.len() {
            return Some((start..start + candidate.len(), open));
        }
        if open {
            return None;
        }
    }
    None
}

/// The address a click should open after the daemon was asked to activate:
/// nothing when a plugin handled it, otherwise the daemon's own reading of the
/// link, and failing that the row-local `fallback`. Only bounded web addresses
/// are ever opened.
pub(crate) fn activation_fallback(response: &Value, fallback: Option<&str>) -> Option<String> {
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return fallback.and_then(web_url);
    }
    let result = &response["result"];
    if result["type"] != "pane_link_activated" {
        return fallback.and_then(web_url);
    }
    if result["handled"] == true {
        return None;
    }
    result["url"]
        .as_str()
        .and_then(web_url)
        .or_else(|| fallback.and_then(web_url))
}
