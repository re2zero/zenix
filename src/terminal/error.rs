//! Errors the terminal renderer raises.
//!
//! Ported from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/error.rs`; only
//! the variants the renderer uses are carried over.

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("pane image decode failed")]
    PaneImageDecode(#[source] image::ImageError),
    #[error("pane image exceeds the limits set for it")]
    PaneImageLimit,
}

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;
