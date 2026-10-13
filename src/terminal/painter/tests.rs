use super::*;
// `use super::*` re-exports GPUI's `test` attribute macro, which would
// otherwise shadow Rust's built-in one and recurse: name the builtin.
use core::prelude::v1::test;

mod cells;
mod composition;
mod diagnostics;
mod glyph_cache;
mod grid_edges;
mod layers;
mod placed_images;
mod prompt_geometry;

fn cell(symbol: &str) -> CellData {
    CellData {
        symbol: symbol.into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    }
}
