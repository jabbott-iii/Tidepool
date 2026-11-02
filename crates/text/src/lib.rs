//! The text engine: rope, positions (including UTF-16), selections,
//! transactions, the undo tree, encodings and atomic save.
//!
//! Specification: `docs/subsystems/text-and-formats.md`.
//!
//! So far: [`rope`] (a persistent B-tree of UTF-8 chunks with O(1) snapshots),
//! [`pos`] (byte offsets and their char, UTF-16 and line/column
//! conversions, and in [`pos::grapheme`] extended grapheme cluster
//! boundaries), [`selection`] (multi-cursor selections, sorted and merged),
//! [`transaction`] (edits as canonical change sets that apply, invert,
//! compose and map positions, and multi-cursor transactions) and [`history`]
//! (the undo tree, which applies every edit, undo, redo and jump to the
//! buffer's rope, keeps redo branches, groups by command and by time, and
//! tracks the saved revision). Positions are byte offsets; lines end at LF,
//! CR LF or a lone CR; a bad position is an error, never a panic
//! (ADR-0019). Edits and undo follow ADR-0020. The other modules arrive
//! with their roadmap tasks.

mod error;
pub mod history;
pub mod pos;
pub mod rope;
pub mod selection;
pub mod transaction;

pub use error::TextError;
pub use history::{Checkpoint, Group, GroupId, History, RevisionId, Step};
pub use pos::{LineBreak, LineCol, LineSpan, Unit};
pub use rope::{Rope, RopeBuilder};
pub use selection::{Cursor, Selection};
pub use transaction::{Assoc, Change, ChangeSet, Changes, Place, Transaction};
