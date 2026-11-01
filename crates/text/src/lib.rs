//! The text engine: rope, positions (including UTF-16), selections,
//! transactions, the undo tree, encodings and atomic save.
//!
//! Specification: `docs/subsystems/text-and-formats.md`.
//!
//! So far: [`rope`] (a persistent B-tree of UTF-8 chunks with O(1) snapshots)
//! and [`pos`] (byte offsets and their char, UTF-16 and line/column
//! conversions, and in [`pos::grapheme`] extended grapheme cluster
//! boundaries). Positions are byte offsets; lines end at LF, CR LF or a
//! lone CR; a bad position is an error, never a panic (ADR-0019). The other
//! modules arrive with their roadmap tasks.

mod error;
pub mod pos;
pub mod rope;

pub use error::TextError;
pub use pos::{LineBreak, LineCol, LineSpan, Unit};
pub use rope::{Rope, RopeBuilder};
