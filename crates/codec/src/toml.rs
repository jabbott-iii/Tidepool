// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! TOML 1.1 (ADR-0026): a parser that reads a document into a [`Table`] of
//! [`Value`]s, each with the [`Span`] of the text it came from.
//!
//! Written for Tidepool; Nadir has no TOML parser to port. The compliance
//! cases are toml-test's TOML 1.1 list, in `tests/toml-test/`.
//!
//! [`parse`](fn@parse) (or [`parse_bytes`]) reads a whole document and
//! returns its root table. A [`Document`] keeps the text as well, so that a
//! value can be changed or added and the text taken back with everything
//! else in it as it was, comments included: see [Editing](#editing).
//!
//! ```
//! use tidepool_codec::Limits;
//! use tidepool_codec::toml::{self, Value};
//!
//! let text = "[package]\nname = \"tidepool\"\nversion = \"0.1.0\"\n";
//! let root = toml::parse(text, Limits::DEFAULT)?;
//! let package = root.get("package").expect("a [package] table");
//! let name = package.get("name").expect("a name");
//! assert_eq!(name.as_str(), Some("tidepool"));
//! // Every key and value knows where its text is.
//! assert_eq!(&text[name.span.range()], "\"tidepool\"");
//! assert_eq!(&text[package.span.range()], "[package]");
//! # Ok::<(), tidepool_codec::CodecError>(())
//! ```
//!
//! # What is accepted
//!
//! TOML 1.1.0, the whole of it: everything TOML 1.0.0 has, which 1.1 only
//! relaxes, plus newlines, comments and a trailing comma in inline tables,
//! the `\xHH` and `\e` escapes, and times written without seconds. There is
//! no strict 1.0 mode. Where the specification leaves room, the choices
//! are these:
//!
//! - The input must be UTF-8. [`parse_bytes`] rejects anything else
//!   ([`ErrorKind::InvalidUtf8`], at the first bad byte) and never repairs
//!   it, which the specification prefers to replacing bytes.
//! - **A leading byte-order mark (U+FEFF) is skipped.** TOML does not
//!   mention one, but editors on Windows write it, and Unicode treats it at
//!   the start of a UTF-8 text as a signature, not content. Anywhere else
//!   outside a string it is an error; inside a string or comment it is an
//!   ordinary character.
//! - A line ends at LF or CR LF. A CR that is not followed by LF is an
//!   error wherever it is ([`ErrorKind::BareCarriageReturn`]), in strings
//!   and comments too. Whitespace is space and tab only.
//! - **Multi-line strings turn each CR LF into LF**, so that a file has the
//!   same value whichever line endings it was saved with (TOML lets a
//!   parser normalize them). An escape such as `\r\n` is kept as written.
//! - Control characters other than tab and line breaks (U+0000 to U+001F,
//!   and U+007F) are rejected raw in strings and comments
//!   ([`ErrorKind::ControlCharacter`]). A line break ends a comment, is part
//!   of a multi-line string, and in a single-line string is an
//!   [`ErrorKind::UnexpectedCharacter`], since that string must close on its
//!   line. Escapes must name a Unicode scalar value: `\uD800` and
//!   `\U00110000` are [`ErrorKind::InvalidUnicodeEscape`].
//! - **Integers are `i64`.** One outside -2^63 to 2^63 - 1 is an error
//!   ([`ErrorKind::IntegerOutOfRange`]), in any base, as TOML requires;
//!   `0x`, `0o` and `0b` numbers are never negative.
//! - **Floats are `f64`.** A decimal float is read correctly rounded (by
//!   `str::parse::<f64>` once its underscores are removed). One too large
//!   for an `f64` (`1e400`) is an error ([`ErrorKind::FloatOutOfRange`])
//!   rather than infinity, which must be written `inf`; one too small
//!   (`1e-400`) becomes zero. `-0.0` keeps its sign. `nan`, `+nan` and
//!   `-nan` are NaN, with the sign bit as written and Rust's NaN payload.
//! - **Date-times** are a [`Datetime`] of one of TOML's four kinds, with
//!   every field checked: the day exists in its month (leap years follow
//!   the Gregorian rules, so `1900-02-29` is an error), hours are 00 to 23,
//!   minutes 00 to 59, and seconds 00 to 60 (a leap second is accepted in
//!   any minute); an offset's hours are 00 to 23 and its minutes 00 to 59.
//!   A fraction of a second keeps its first nine digits, as nanoseconds;
//!   the rest are dropped, not rounded, as TOML requires. Seconds left out
//!   (`07:32`) are zero, and whether they were written is not recorded;
//!   nor is whether the offset was `Z` or `z`, or the date and time were
//!   separated by `T`, `t` or a space. The span still has the text.
//! - Keys are compared exactly: decoded, case-sensitive, without Unicode
//!   normalization. Bare keys are ASCII letters, digits, `-` and `_`, and
//!   are strings even when they look like numbers (`3.14 = 1` defines `3`
//!   holding `14`).
//! - Every rule on defining keys and tables is enforced: no key or table is
//!   defined twice; a dotted key cannot add to a table defined by a header,
//!   nor a header redefine a table dotted keys made; inline tables and
//!   arrays written as values are complete as written, so nothing can be
//!   added to them later; a `[[header]]` adds only to an array of tables,
//!   and a `[header]` cannot name one. See the [`ErrorKind`] variants.
//!
//! # The value tree
//!
//! [`parse`](fn@parse) returns the root [`Table`]. A table keeps its
//! entries in document order and finds a key in constant time. Each entry
//! is a [`Key`] and a [`Value`], and both carry the [`Span`] of their text:
//! byte offsets into the input as given, a leading byte-order mark
//! included, so `&text[span.range()]` is the text.
//!
//! - A key's span is the key as written, quotes included: `"a.b"` in
//!   `"a.b" = 1`, or `b` in `a.b = 1` and in `[a.b]`.
//! - A scalar's span is its literal: a string with its quotes, a number,
//!   `true`, a date-time.
//! - An array written as a value spans `[` to `]`, an inline table `{` to
//!   `}`.
//! - A table defined by a header spans the header, `[` to `]`; each element
//!   of an array of tables spans its own `[[...]]` header, and the array
//!   spans the header that created it.
//! - A table that was created implicitly, as the prefix of a header (`a`
//!   and `a.b` in `[a.b.c]`) or of a dotted key (`a` in `a.b = 1`), spans
//!   what created it: that whole header, or that whole dotted key. If a
//!   header or dotted key defines it later, it takes that one's span
//!   instead, and its key the span of the name there.
//!
//! [`Table::kind`] says how each table was defined ([`TableKind`]), and
//! [`Array::is_array_of_tables`] tells an array of tables from an array
//! written as a value.
//!
//! # Editing
//!
//! A [`Document`] holds a document's text, its root table and the
//! [`Limits`](crate::Limits) it was read with. A caller reads values
//! through the tree ([`Document::root`], [`Document::get`]), changes or adds
//! them with [`Document::set`], and takes the text back
//! ([`Document::as_str`], [`Document::into_string`]). Until an edit, the
//! text is the input byte for byte. After edits, only the bytes of each
//! changed value, or of each added entry or table, are new: comments, blank
//! lines, order, quoting, layout and line endings elsewhere stay as they
//! were (ADR-0013, ADR-0031).
//!
//! ```
//! use tidepool_codec::Limits;
//! use tidepool_codec::toml::{Document, ValueKind};
//!
//! let limits = Limits { max_input_bytes: 1024 * 1024, ..Limits::DEFAULT };
//! let mut doc = Document::parse("# Mine\n[editor]\nfont-size = 14 # big\n", limits)?;
//! doc.set(&["editor".into(), "tab-size".into()], &ValueKind::Integer(4))?;
//! doc.set(&["editor".into(), "font-size".into()], &ValueKind::Integer(16))?;
//! doc.set(&["theme".into(), "name".into()], &ValueKind::String("tidepool-dark".into()))?;
//! assert_eq!(
//!     doc.as_str(),
//!     "# Mine\n[editor]\nfont-size = 16 # big\ntab-size = 4\n\n[theme]\nname = \"tidepool-dark\"\n"
//! );
//! # Ok::<(), tidepool_codec::CodecError>(())
//! ```
//!
//! - **The tree always matches the text:** [`Document::root`] is what
//!   [`parse`](fn@parse) gives for [`Document::as_str`] with the document's
//!   limits, and neither is ever handed out mutably. Every edit makes a new
//!   tree, so spans from before it are stale; the borrow checker stops a
//!   caller from holding a [`Value`] of the document across
//!   [`Document::set`].
//! - **Batches:** to make several edits all or nothing, clone the document,
//!   edit the clone, and keep it only if every edit succeeds.
//! - **New files:** an empty document is `Document::parse("", limits)`.
//!
//! ## Paths
//!
//! A path is a list of [`Step`]s from the root table: a [`Step::Key`] needs
//! a table, and a [`Step::Index`] an array. An array of tables is always
//! indexed explicitly: a key step on one is
//! [`EditErrorKind::NotATable`], and the editor never picks its last
//! element for the caller. A path follows the structure, not the spelling:
//! `editor.font-size = 14` at the root, `[editor]` with `font-size = 14`,
//! and `editor = { font-size = 14 }` are all `["editor", "font-size"]`.
//!
//! ## Building values
//!
//! [`Document::set`] takes a [`ValueKind`]. A value read from another
//! document, such as the defaults, is passed as `&value.kind`, without a
//! clone. To build one in code, `Value::from(kind)` gives a value with the
//! span `0..0`, [`Array::new`] an array written as a value, and
//! [`Table::new_inline`] an empty inline table, which [`Table::insert`]
//! fills. The writer ignores the spans and the [`TableKind`] of a value
//! built in code.
//!
//! ## Setting a value that exists
//!
//! - If the value at the path holds the same data, nothing changes. So a
//!   pass that saves every setting leaves an unchanged file byte for byte,
//!   and `0x10`, `1_000`, `'lit'` and `07:30` stay as written. The same
//!   data means the same kind and value, ignoring spans and table kinds;
//!   floats compared by their bits, with any two NaNs of one sign equal;
//!   arrays compared in order, ignoring whether they are arrays of tables;
//!   and tables compared entry by entry, in order.
//! - Otherwise, if the value is a scalar, an array written as a value or an
//!   inline table (at a key, or at an index of an array), exactly the bytes
//!   of its span are replaced with text the writer creates, and the type
//!   may change. The key, the spaces, a comment after the value and the
//!   line break stay.
//! - **A value written over several lines is replaced whole, by one line of
//!   TOML 1.0:** a multi-line string, an array laid over lines, or an inline
//!   table laid over lines (TOML 1.1). Comments inside it go with it, and so
//!   does its layout. To keep them, set its entries or elements instead:
//!   each of those edits replaces only its own span, so the layout around
//!   it stays (ADR-0031).
//! - Otherwise the edit is refused with [`EditErrorKind::NotReplaceable`],
//!   since the value's text is spread over lines or sections: a table
//!   defined by a header, an element of an array of tables, a table made by
//!   a header's prefix ([`TableKind::Implicit`]), a table made by dotted
//!   keys, or an array of tables. Set its entries instead.
//!
//! ## Adding a value
//!
//! When the path does not exist, the walk stops at N, the deepest table on
//! the path that does. After it come the missing tables t1…tm (m ≥ 0, made
//! by the edit) and the last key k. Only tables are made: an
//! [`Step::Index`] among the missing steps is
//! [`EditErrorKind::MissingArray`]. What is added, and where:
//!
//! | N | m | Added | Where |
//! | :- | :- | :- | :- |
//! | The root, with leaves; a header's table; an element of an array of tables | 0 | The line `k = v` | At the end of N's section: element 0 of three `[[a]]`s gets it before the second `[[a]]` |
//! | The root, without leaves | 0 | The line `k = v`, then a blank line | At the root point |
//! | A table made by dotted keys in a section | any | The line `p.t1….tm.k = v`, where p names the tables from the section's table (not included) down to N | On the line after N's last leaf, with that leaf's indent |
//! | An inline table C, or a table made by dotted keys inside one, with a leaf | any | `, q.t1….tm.k = v`, where q names the tables from C (not included) down to N | Right after N's last leaf, before the comma, comment or line break after it |
//! | An empty inline table | any | ` t1….tm.k = v`, and a space if `}` follows `{` directly | Right after `{`: `{}` becomes `{ k = v }`, and `{\n}` becomes `{ k = v\n}` |
//! | A table made by a header's prefix | 0 | The header `[path]` naming N, then `k = v`; N becomes a header's table | After N's subtree |
//! | The root, a header's table, an element of an array of tables, a table made by a header's prefix | ≥ 1 | The header `[path.t1….tm]`, path naming N, then `k = v` | After N's subtree |
//!
//! The terms in it:
//!
//! - **Leaves** of a table: its entries whose value is a scalar, an array
//!   written as a value or an inline table, and the leaves of its tables
//!   made by dotted keys, and theirs; never those of a header's table, an
//!   array of tables or a table made by a header's prefix. N's **last
//!   leaf** is the one whose text ends last.
//! - **The end of N's section** is the line after its last leaf, or with
//!   none, the line after its header.
//! - **The root point**, where a root without leaves takes a key: above the
//!   first header and the comment lines directly above it (a blank line
//!   stops them), so that a comment that belongs to the header stays with
//!   it. With no header, the end of the text.
//! - **After N's subtree** is the line after the last thing in N or under
//!   it: a leaf of any table that is not inline, or a header. Only blank
//!   and comment lines lie between there and the next header, so a new
//!   header takes no keys that were there.
//! - **path** is the keys from the root to N, the indexes of arrays of
//!   tables left out: a header names the last element of each array of
//!   tables on its path, and every section under element i comes before
//!   element i + 1's `[[...]]`, so a header placed after element i's
//!   subtree names element i.
//! - **The indent** is the spaces and tabs that start the last leaf's line;
//!   there is none after a header or at the root point.
//!
//! So a header is never written where dotted keys or an inline table have
//! made a table, nor a dotted key into a table a header defined.
//!
//! A new line goes in at the start of a line, or at the end of the text,
//! and ends with the text's line break: CR LF if its first line break is
//! one, otherwise LF. A blank line goes before a new header, unless there
//! is one already or the header starts the content, and after it, unless
//! there is one already or the header ends the text; a key added at the
//! root point is followed by one the same way. Lines added to an empty
//! document (or a byte-order mark alone) have no blank line. At the end of
//! a text that does not end in a line break, the new lines come after one,
//! and the text still does not end in one. Nothing is ever written before a
//! leading byte-order mark. Comments and blank lines after the line a new
//! line follows stay below it.
//!
//! ## The text the writer creates
//!
//! It is TOML 1.0 (ADR-0031), and fits on one line:
//!
//! | Kind | Written |
//! | :- | :- |
//! | Key | Bare when it is not empty and only ASCII letters, digits, `-` and `_` (so `3`, `true` and `tab-size` stay bare); otherwise quoted as a string is. A dotted key's parts are joined by `.`, without spaces, and a header is `[a.b]`; `[[...]]` is never written |
//! | String | A literal string, `'C:\Users\me'`, when the text holds a `\` or a `"` but no `'` and no control character (U+0000 to U+001F and U+007F). Otherwise a basic string: `\"`, `\\`, `\b`, `\t`, `\n`, `\f` and `\r`, then `\uXXXX` with uppercase hex for every other control character (`\u001B`), and everything else as it is. Never multi-line, and never `\e`, `\xHH` or `\U` |
//! | Integer | Decimal: `-` for a negative number, no `+`, underscores or base prefix |
//! | Float | A finite float as [`json::Number`](crate::json::Number) writes it: the shortest digits that read back as the same `f64`, with an exponent when the magnitude is not 0 and outside 1e-5 to 1e16 (`1e16`, `5e-324`, `1.5e300`), and with `.0` on a whole number (`3.0`, `-0.0`). Then `inf` and `-inf`, and `nan` or `-nan` by the sign bit (a NaN's payload is not kept). Every finite float reads back bit for bit |
//! | Boolean | `true` or `false` |
//! | Date-time | Checked first, since one built in code is not: a year from 0 to 9999, a month from 1 to 12, a day the month has, an hour to 23, a minute to 59, a second to 60, nanoseconds to 999,999,999 and an offset within 23:59 either way. Then as [`Datetime`]'s `Display` writes it: `T` between date and time, the seconds always written, a fraction without trailing zeros, and `Z` or `±HH:MM` (`+00:00` stays apart from `Z`). So a time read from `07:32` is written `07:32:00` once it is changed |
//! | Array | `[a, b]` or `[]`, without a trailing comma. A table in it is inline; an array of tables passed as a value is written as an array of inline tables |
//! | Table | `{ k = v, k2 = v2 }` or `{}`, the entries in order, a table in it inline too (never dotted keys). No comment, line break or trailing comma. A table passed to [`Document::set`] is always written inline: to get a header, set its entries one by one |
//!
//! ## Errors and verification
//!
//! [`Document::parse`] and [`Document::parse_bytes`] fail exactly as
//! [`parse`](fn@parse) and [`parse_bytes`] do. [`Document::set`] fails with
//! [`CodecError::TomlEdit`](crate::CodecError::TomlEdit), whose `step` is
//! the index of the step the problem concerns and whose
//! [`EditErrorKind`] says what it is, and the document is then unchanged.
//! Messages never quote keys or text.
//!
//! Every edit that changes something is planned as one splice, a range of
//! the text (empty to insert) and the text to put there, which is built
//! within the room [`Limits::max_input_bytes`](crate::Limits::max_input_bytes)
//! leaves. The new text is parsed with the document's limits, and the old
//! and new trees are compared along the path: every table on it keeps its
//! other entries, in order and with the same data; every array keeps its
//! length and its other elements; each table the edit makes holds only the
//! next step; and the value at the path holds the data that was set. Only
//! then do the text and the tree change. A new text that does not pass is
//! [`EditErrorKind::Unverified`], a bug in this module that refuses the
//! edit rather than corrupt the file. The text outside the splice is
//! unchanged by construction.
//!
//! An edit costs one parse and one walk of the tree, linear in the size of
//! the document, and holds two trees for a moment: a settings file of a few
//! kilobytes takes microseconds. A document may come from an untrusted
//! workspace, and nothing here panics on one: every access to the text is
//! checked, every walk keeps its own stack, and the writer recurses only
//! once the depth of the value, measured without recursing, is known to be
//! within [`Limits::max_depth`](crate::Limits::max_depth). Because of the
//! second tree, read settings from a workspace or a user with a lower
//! `max_input_bytes`, such as 1 MiB.
//!
//! ## Not supported
//!
//! Later tasks add these when a caller needs them:
//!
//! - Removing keys, tables or elements, and renaming, reordering or moving
//!   entries: a reset to a default can set the default's value, but not
//!   remove the key.
//! - Adding an element to an array, or a `[[...]]` element.
//! - Replacing as a whole a header's table, a table made by a header's
//!   prefix or by dotted keys, or an array of tables; and writing a table
//!   under a header.
//! - Created text over several lines, and keeping a changed value's
//!   spelling (its base, underscores, quotes, left-out seconds, `t` or
//!   `z`).
//! - An API for comments, telling 1.1 syntax from 1.0, reformatting, and
//!   making several edits with one parse.
//!
//! What a settings file means (layers, validation, version keys,
//! migrations) is M4.7's; reading and saving files is `text::file`'s, on a
//! worker.
//!
//! # Limits and errors
//!
//! Every [`Limits`](crate::Limits) field that applies is enforced before
//! the memory it guards is allocated: the input size; the nesting depth
//! of tables and arrays, however they nest (a dotted key or header with
//! too many parts, arrays of tables, inline tables and arrays); the length
//! of each string and key; the length of each number and date-time; and
//! the entries of each table and array, an array of tables included.
//!
//! Depth counts every table and array between the root table and a value:
//! `a = [[1]]`, `a.b.c = 1` and `[a.b]` are each 2 deep, and so is
//! `[[a]]`, an array and the table in it. A key's parts are checked as they
//! are read, so `a.b.c…` with thousands of parts stops at the first one
//! too deep. The parser keeps its own stack on the heap, so even deep
//! nesting cannot overflow the thread's stack, and when a parse fails it
//! takes apart what it built without recursing. Dropping, cloning,
//! comparing and `Debug` do recurse once per level, so keep
//! [`Limits::max_depth`](crate::Limits::max_depth) in the hundreds.
//!
//! A tree takes more memory than its text. Once a parse is over, every
//! table, array, string and key is at exactly its size, and a tree holds at
//! most about 49 bytes of heap per byte of text: chains of one-entry tables
//! (`a.b.b.b… = 0`) are the densest. With glibc's overhead such chains grow
//! the process by about 71 times their text, and `Cargo.lock` by about 7
//! (see [`Limits`](crate::Limits)). At the default
//! [`Limits::max_input_bytes`](crate::Limits::max_input_bytes) a tree can
//! take about 3 GiB of heap, about 4.5 GiB with glibc, so lower it to read
//! TOML from an untrusted source.
//!
//! Every parse error is a [`CodecError::Toml`](crate::CodecError::Toml)
//! with the byte offset, line and column of the problem and an
//! [`ErrorKind`] saying what was wrong and what was expected. For a string
//! or escape that the text ends inside, the position is where that
//! construct starts. Messages never quote the input beyond the one
//! character found. The parser never panics. An edit that
//! [`Document::set`] refuses is a
//! [`CodecError::TomlEdit`](crate::CodecError::TomlEdit) instead, with an
//! [`EditErrorKind`] (see [Editing](#errors-and-verification)).

mod datetime;
mod document;
mod parse;
mod table;
mod write;

use std::fmt;
use std::ops::Range;

pub use datetime::{Date, Datetime, Offset, Time};
pub use document::{Document, Step};
pub use parse::{parse, parse_bytes};
pub use table::Table;

/// A range of bytes in the input a document was parsed from: `start` to
/// `end`, not including `end`. Offsets count a leading byte-order mark,
/// and are always at character boundaries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    /// The offset of the first byte.
    pub start: usize,
    /// The offset just past the last byte.
    pub end: usize,
}

impl Span {
    /// The span from `start` to `end`.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The span as a `Range`, for slicing the input: `&text[span.range()]`.
    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

/// A key of a [`Table`]: its name, decoded, and the span of the key as the
/// document writes it, quotes included. See the
/// [module documentation](self#the-value-tree) for which span a key of an
/// implicitly created table has.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    /// The name, with escapes decoded: `a"b` for the key `"a\"b"`.
    pub name: String,
    /// Where the key is written.
    pub span: Span,
}

/// A TOML value and the span of its text. See the
/// [module documentation](self#the-value-tree) for each kind's span.
///
/// A value from the parser is at most [`Limits::max_depth`] deep. Dropping,
/// cloning, comparing and formatting a value with `Debug` recurse once per
/// level of nesting, so a value built in code should stay similarly
/// shallow. Equality compares spans too, and a NaN is not equal to itself.
///
/// [`Limits::max_depth`]: crate::Limits::max_depth
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    /// What the value is.
    pub kind: ValueKind,
    /// Where its text is in the input.
    pub span: Span,
}

/// What a [`Value`] is: one of TOML's types.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueKind {
    /// A string, of any of the four kinds, with escapes decoded.
    String(String),
    /// An integer, written in any base.
    Integer(i64),
    /// A float, including `inf`, `nan` and signed zeros.
    Float(f64),
    /// `true` or `false`.
    Boolean(bool),
    /// An offset date-time, local date-time, local date or local time.
    Datetime(Datetime),
    /// An array written as a value, or an array of tables.
    Array(Array),
    /// A table: defined by a header, implicitly, by dotted keys, or inline.
    Table(Table),
}

/// How a [`Table`] was defined, which decides what may be added to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableKind {
    /// The document's root table, which [`parse`](fn@parse) returns.
    Root,
    /// Defined by a `[header]`.
    Header,
    /// An element of an array of tables, defined by an `[[header]]`.
    ArrayHeader,
    /// Created as a prefix of headers (`a` in `[a.b]`) and never defined
    /// itself. A header or a dotted key may still define it.
    Implicit,
    /// Created or defined by a dotted key (`a` in `a.b = 1`), at the top
    /// level, under a header, or in an inline table.
    Dotted,
    /// An inline table, `{ ... }`, complete at its closing brace.
    Inline,
}

/// The elements of an array: an array written as a value (`[1, 2]`), or an
/// array of tables (`[[header]]`), whose elements are all tables of kind
/// [`TableKind::ArrayHeader`].
#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    items: Vec<Value>,
    of_tables: bool,
}

impl Array {
    /// An array written as a value, holding `items`: one built in code,
    /// to pass to [`Document::set`]. It is never an array of tables.
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items,
            of_tables: false,
        }
    }

    /// The elements, in document order.
    pub fn items(&self) -> &[Value] {
        &self.items
    }

    /// The element at `index`, if there is one.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.items.get(index)
    }

    /// The number of elements.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the array has no elements. An array of tables always has
    /// at least one.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether this is an array of tables, made by `[[header]]`s, rather
    /// than an array written as a value.
    pub fn is_array_of_tables(&self) -> bool {
        self.of_tables
    }
}

impl Value {
    /// Returns the string, if this is a [`ValueKind::String`].
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the integer, if this is a [`ValueKind::Integer`]. A float
    /// is never converted.
    pub fn as_integer(&self) -> Option<i64> {
        match self.kind {
            ValueKind::Integer(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the float, if this is a [`ValueKind::Float`]. An integer is
    /// never converted.
    pub fn as_float(&self) -> Option<f64> {
        match self.kind {
            ValueKind::Float(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the boolean, if this is a [`ValueKind::Boolean`].
    pub fn as_bool(&self) -> Option<bool> {
        match self.kind {
            ValueKind::Boolean(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the date-time, if this is a [`ValueKind::Datetime`].
    pub fn as_datetime(&self) -> Option<&Datetime> {
        match &self.kind {
            ValueKind::Datetime(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the array, if this is a [`ValueKind::Array`].
    pub fn as_array(&self) -> Option<&Array> {
        match &self.kind {
            ValueKind::Array(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the table, if this is a [`ValueKind::Table`].
    pub fn as_table(&self) -> Option<&Table> {
        match &self.kind {
            ValueKind::Table(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the value of the entry named `name`, if this is a table that
    /// has one. See [`Table::get`].
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.as_table()?.get(name)
    }
}

impl From<ValueKind> for Value {
    /// A value built in code, with the span `0..0`, since it has no text.
    fn from(kind: ValueKind) -> Self {
        Self {
            kind,
            span: Span::default(),
        }
    }
}

/// What is wrong with a piece of TOML text. See
/// [`CodecError::Toml`](crate::CodecError::Toml), which adds where.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input ended too soon.
    ///
    /// Usually reported at the end of the input. When the input ends inside
    /// a string or an escape sequence, it is reported where that starts
    /// instead, and `expected` says so: for example "`\"` to close the
    /// string that starts here".
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A character that cannot appear at this point.
    UnexpectedCharacter {
        /// The character.
        found: char,
        /// What should have been there instead.
        expected: &'static str,
    },
    /// The input is not UTF-8 from this byte on.
    InvalidUtf8,
    /// A raw control character in a string or comment: one of U+0000 to
    /// U+001F other than tab, LF and CR, or U+007F. In a basic string, write
    /// it as an escape.
    ///
    /// Line breaks are not this: a line break ends a comment and is part of
    /// a multi-line string, and one in a single-line string is an
    /// [`ErrorKind::UnexpectedCharacter`] (that string must close on its
    /// line). A CR alone is an [`ErrorKind::BareCarriageReturn`].
    ControlCharacter {
        /// The character.
        found: char,
    },
    /// A carriage return that is not followed by a line feed, wherever it
    /// is: a line ends at LF or CR LF, never at CR alone.
    BareCarriageReturn,
    /// A word where a value should be that starts like `true`, `false`,
    /// `inf` or `nan` but is not one of them, such as `tru` or `nope`.
    /// Strings need quotes; those four are the only bare words. A value that
    /// starts with any other letter, such as `yes` or `True`, is an
    /// [`ErrorKind::UnexpectedCharacter`].
    InvalidLiteral,
    /// A backslash in a basic string followed by something that is not an
    /// escape.
    InvalidEscape,
    /// A `\u` or `\U` escape for a code point that is not a Unicode scalar
    /// value: a surrogate (`\uD800`) or one above U+10FFFF.
    InvalidUnicodeEscape,
    /// A multi-line string (`"""` or `'''`) used as a key.
    MultilineKey,
    /// A decimal number with leading zeros, such as `007` or `-01.5`.
    LeadingZero,
    /// An underscore in a number that is not between two digits: `1_`,
    /// `1__000`, `1_.5`, `1e_5`, `+_1`, `0x_1`. A value cannot start with
    /// one: `_1` is an [`ErrorKind::UnexpectedCharacter`].
    MisplacedUnderscore,
    /// An integer outside the range of `i64`, -2^63 to 2^63 - 1.
    IntegerOutOfRange,
    /// A float too large in magnitude for an `f64`, such as `1e400`.
    FloatOutOfRange,
    /// A date or time with a field out of range, such as `2023-02-29` or
    /// `24:00:00`. Reported at the field.
    InvalidDatetime {
        /// The range that field must be in.
        expected: &'static str,
    },
    /// A key that its table already has, as a value or as a table. Reported
    /// at the key.
    DuplicateKey,
    /// A `[header]` for a table that is already defined: by another
    /// header, by dotted keys, as an inline table, or as an array of
    /// tables. Reported at the header's last key.
    DuplicateTable,
    /// A dotted key or header that goes through a key holding something
    /// other than a table, so nothing can be defined under it. Reported at
    /// that key.
    NotATable {
        /// What the key holds, such as "an integer" or "an array".
        found: &'static str,
    },
    /// An `[[header]]` for a key that holds something other than an array
    /// of tables. Reported at the header's last key.
    NotAnArrayOfTables {
        /// What the key holds, such as "a table" or "an array".
        found: &'static str,
    },
    /// A header or dotted key that adds to an inline table after its
    /// closing brace. Reported at the key that names the inline table.
    ExtendsInlineTable,
    /// A dotted key that adds to a table defined by a `[header]` or an
    /// `[[header]]`, outside that header's own section. Reported at the key
    /// that names the table.
    ExtendsDefinedTable,
    /// The input is longer than [`Limits::max_input_bytes`]. Reported at the
    /// first character that does not fit: the one that contains the first
    /// byte past the limit.
    ///
    /// [`Limits::max_input_bytes`]: crate::Limits::max_input_bytes
    InputTooLarge {
        /// The input's length in bytes.
        length: usize,
        /// The limit that was exceeded.
        limit: usize,
    },
    /// Tables and arrays nest deeper than [`Limits::max_depth`]. Reported at
    /// the key, bracket or brace that goes too deep.
    ///
    /// [`Limits::max_depth`]: crate::Limits::max_depth
    TooDeep {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A string or key is longer than [`Limits::max_string_bytes`].
    ///
    /// [`Limits::max_string_bytes`]: crate::Limits::max_string_bytes
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A number or date-time is longer than [`Limits::max_number_bytes`].
    ///
    /// [`Limits::max_number_bytes`]: crate::Limits::max_number_bytes
    NumberTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// An array or table has more than [`Limits::max_entries`] entries.
    ///
    /// [`Limits::max_entries`]: crate::Limits::max_entries
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "the text ends where {expected} was expected")
            }
            Self::UnexpectedCharacter { found, expected } => {
                write!(f, "found {found:?} where {expected} was expected")
            }
            Self::InvalidUtf8 => f.write_str("the text is not valid UTF-8"),
            Self::ControlCharacter { found } => write!(
                f,
                "found the control character {found:?}, which TOML allows only as an escape in a basic string"
            ),
            Self::BareCarriageReturn => {
                f.write_str("a carriage return must be followed by a line feed")
            }
            Self::InvalidLiteral => f.write_str(
                "unknown word; strings need quotes, and the only bare words are \
                 `true`, `false`, `inf` and `nan`",
            ),
            Self::InvalidEscape => f.write_str("invalid escape sequence in a string"),
            Self::InvalidUnicodeEscape => f.write_str(
                "the escape is not a Unicode scalar value (a surrogate, or above U+10FFFF)",
            ),
            Self::MultilineKey => f.write_str("a multi-line string cannot be a key"),
            Self::LeadingZero => f.write_str("numbers cannot have leading zeros"),
            Self::MisplacedUnderscore => {
                f.write_str("an underscore in a number must be between two digits")
            }
            Self::IntegerOutOfRange => f.write_str(
                "the integer does not fit in 64 bits (-9223372036854775808 to 9223372036854775807)",
            ),
            Self::FloatOutOfRange => {
                f.write_str("the float is too large for 64 bits; write `inf` for infinity")
            }
            Self::InvalidDatetime { expected } => {
                write!(f, "no such date or time; expected {expected}")
            }
            Self::DuplicateKey => f.write_str("the key is already defined"),
            Self::DuplicateTable => f.write_str("the table is already defined"),
            Self::NotATable { found } => write!(
                f,
                "the key holds {found}, not a table, so nothing can be defined under it"
            ),
            Self::NotAnArrayOfTables { found } => write!(
                f,
                "`[[...]]` adds to an array of tables, but the key holds {found}"
            ),
            Self::ExtendsInlineTable => f.write_str(
                "an inline table is complete at its closing brace; nothing can be added to it",
            ),
            Self::ExtendsDefinedTable => f.write_str(
                "a dotted key cannot add to a table defined by a header; \
                 write the key under that header",
            ),
            Self::InputTooLarge { length, limit } => write!(
                f,
                "the text is {length} bytes, more than the limit of {limit} bytes"
            ),
            Self::TooDeep { limit } => {
                write!(f, "tables and arrays nest deeper than the limit of {limit}")
            }
            Self::StringTooLong { limit } => {
                write!(
                    f,
                    "a string or key is longer than the limit of {limit} bytes"
                )
            }
            Self::NumberTooLong { limit } => write!(
                f,
                "a number or date-time is longer than the limit of {limit} bytes"
            ),
            Self::TooManyEntries { limit } => write!(
                f,
                "an array or table has more than the limit of {limit} entries"
            ),
        }
    }
}

/// Why [`Document::set`] refused an edit. See
/// [`CodecError::TomlEdit`](crate::CodecError::TomlEdit), which adds the
/// step of the path it concerns, and the
/// [module documentation](self#errors-and-verification).
///
/// The document is unchanged after any of them.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditErrorKind {
    /// The path has no steps. Reported at step 0.
    EmptyPath,
    /// A [`Step::Key`] on something other than a table: a scalar, an array
    /// written as a value, or an array of tables, whose elements need a
    /// [`Step::Index`] first.
    NotATable {
        /// What the path reaches there, such as "an integer" or "an array
        /// of tables".
        found: &'static str,
    },
    /// A [`Step::Index`] on something other than an array.
    NotAnArray {
        /// What the path reaches there, such as "a table".
        found: &'static str,
    },
    /// A [`Step::Index`] past the end of an array. An edit never adds
    /// elements.
    IndexOutOfRange {
        /// The number of elements the array has.
        len: usize,
    },
    /// A [`Step::Index`] after a key that does not exist. An edit makes
    /// only tables, never arrays. Reported at the index.
    MissingArray,
    /// The value at the path is one whose text is spread over lines or
    /// sections, and the new value differs from it: a table defined by a
    /// header, an element of an array of tables, a table made by a header's
    /// prefix or by dotted keys, or an array of tables. Set its entries
    /// instead. Reported at the last step.
    NotReplaceable {
        /// What the value is, such as "a table defined by a header".
        found: &'static str,
    },
    /// A date-time in the new value has a field out of range, which only a
    /// value built in code can have. Reported at the last step.
    InvalidDatetime {
        /// The range that field must be in, as in
        /// [`ErrorKind::InvalidDatetime`].
        expected: &'static str,
    },
    /// The edited text would be longer than
    /// [`Limits::max_input_bytes`](crate::Limits::max_input_bytes). Found
    /// before more than that is written. Reported at `path.len()`, the edit
    /// as a whole.
    TooLarge {
        /// The limit.
        limit: usize,
    },
    /// The new value nests deeper than
    /// [`Limits::max_depth`](crate::Limits::max_depth) allows where it
    /// goes ([`ErrorKind::TooDeep`], found before anything is written, at
    /// the last step), or the edited text breaks another limit when it is
    /// read again ([`ErrorKind::TooDeep`], [`ErrorKind::StringTooLong`],
    /// [`ErrorKind::NumberTooLong`] or [`ErrorKind::TooManyEntries`], at
    /// `path.len()`).
    Limit(ErrorKind),
    /// The edited text did not parse, or its data was not the old data
    /// with exactly this edit made. This is a bug in the document model:
    /// the edit is refused rather than corrupt the text. Reported at
    /// `path.len()`.
    Unverified {
        /// What went wrong, without quoting the text.
        reason: &'static str,
    },
}

impl fmt::Display for EditErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => f.write_str("the path has no steps"),
            Self::NotATable { found } => {
                write!(f, "a key needs a table, but the path reaches {found} there")
            }
            Self::NotAnArray { found } => write!(
                f,
                "an index needs an array, but the path reaches {found} there"
            ),
            Self::IndexOutOfRange { len } => write!(
                f,
                "the index is past the end of an array of {len} elements; an edit never adds elements"
            ),
            Self::MissingArray => {
                f.write_str("an index follows a key that does not exist; an edit makes only tables")
            }
            Self::NotReplaceable { found } => write!(
                f,
                "the value is {found}, whose text is spread over lines or sections, so it cannot \
                 be replaced whole; set its entries instead"
            ),
            Self::InvalidDatetime { expected } => write!(
                f,
                "a date-time in the new value is out of range; expected {expected}"
            ),
            Self::TooLarge { limit } => write!(
                f,
                "the edited text would be longer than the limit of {limit} bytes"
            ),
            Self::Limit(kind) => write!(f, "the edited text breaks a limit: {kind}"),
            Self::Unverified { reason } => write!(
                f,
                "the edit was refused because {reason}; this is a bug in the document model"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    fn sample() -> Table {
        let text = "s = 'x'\ni = 1\nf = 1.5\nb = true\nd = 1979-05-27\na = [1]\n[t]\n";
        parse(text, Limits::DEFAULT).expect("parses")
    }

    #[test]
    fn accessors_return_only_the_matching_kind() {
        let root = sample();
        let get = |name| root.get(name).expect("present");
        assert_eq!(get("s").as_str(), Some("x"));
        assert_eq!(get("i").as_integer(), Some(1));
        assert_eq!(get("f").as_float(), Some(1.5));
        assert_eq!(get("b").as_bool(), Some(true));
        assert_eq!(
            get("d").as_datetime(),
            Some(&Datetime::LocalDate(Date {
                year: 1979,
                month: 5,
                day: 27
            }))
        );
        assert_eq!(get("a").as_array().map(Array::len), Some(1));
        assert_eq!(
            get("t").as_table().map(Table::kind),
            Some(TableKind::Header)
        );

        // Nothing converts, not even an integer to a float.
        let i = get("i");
        assert_eq!(i.as_float(), None);
        assert_eq!(i.as_str(), None);
        assert_eq!(i.as_bool(), None);
        assert_eq!(i.as_datetime(), None);
        assert_eq!(i.as_array(), None);
        assert_eq!(i.as_table(), None);
        assert_eq!(i.get("x"), None);
        assert_eq!(get("f").as_integer(), None);
    }

    #[test]
    fn arrays_say_what_they_hold() {
        let root = parse("a = [1, 2]\ne = []\n[[t]]\n[[t]]\n", Limits::DEFAULT).expect("parses");
        let a = root.get("a").and_then(Value::as_array).expect("a");
        assert_eq!(
            (a.len(), a.is_empty(), a.is_array_of_tables()),
            (2, false, false)
        );
        assert_eq!(a.get(1).and_then(Value::as_integer), Some(2));
        assert_eq!(a.get(2), None);
        assert_eq!(a.items().len(), 2);
        let e = root.get("e").and_then(Value::as_array).expect("e");
        assert!(e.is_empty());
        let t = root.get("t").and_then(Value::as_array).expect("t");
        assert!(t.is_array_of_tables());
        assert!(
            t.items()
                .iter()
                .all(|item| { item.as_table().map(Table::kind) == Some(TableKind::ArrayHeader) })
        );
    }

    #[test]
    fn spans_slice_the_input() {
        let text = "key = \"value\"";
        let root = parse(text, Limits::DEFAULT).expect("parses");
        let (key, value) = root.get_key_value("key").expect("present");
        assert_eq!(key.span, Span::new(0, 3));
        assert_eq!(&text[key.span.range()], "key");
        assert_eq!(&text[value.span.range()], "\"value\"");
        assert_eq!(Span::default(), Span::new(0, 0));
    }

    #[test]
    fn error_kinds_explain_themselves() {
        let cases = [
            (
                ErrorKind::UnexpectedEnd {
                    expected: "a value",
                },
                "the text ends where a value was expected",
            ),
            (
                ErrorKind::UnexpectedCharacter {
                    found: '}',
                    expected: "a value",
                },
                "found '}' where a value was expected",
            ),
            (
                ErrorKind::ControlCharacter { found: '\u{7}' },
                "found the control character '\\u{7}', which TOML allows only as an escape in a basic string",
            ),
            (
                ErrorKind::InvalidDatetime {
                    expected: "a month from 01 to 12",
                },
                "no such date or time; expected a month from 01 to 12",
            ),
            (
                ErrorKind::NotATable {
                    found: "an integer",
                },
                "the key holds an integer, not a table, so nothing can be defined under it",
            ),
            (
                ErrorKind::NotAnArrayOfTables { found: "a table" },
                "`[[...]]` adds to an array of tables, but the key holds a table",
            ),
            (
                ErrorKind::InputTooLarge {
                    length: 10,
                    limit: 8,
                },
                "the text is 10 bytes, more than the limit of 8 bytes",
            ),
            (
                ErrorKind::TooDeep { limit: 64 },
                "tables and arrays nest deeper than the limit of 64",
            ),
            (
                ErrorKind::StringTooLong { limit: 8 },
                "a string or key is longer than the limit of 8 bytes",
            ),
            (
                ErrorKind::NumberTooLong { limit: 4 },
                "a number or date-time is longer than the limit of 4 bytes",
            ),
            (
                ErrorKind::TooManyEntries { limit: 2 },
                "an array or table has more than the limit of 2 entries",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
        // The rest have fixed messages; they must at least not be empty.
        for kind in [
            ErrorKind::InvalidUtf8,
            ErrorKind::BareCarriageReturn,
            ErrorKind::InvalidLiteral,
            ErrorKind::InvalidEscape,
            ErrorKind::InvalidUnicodeEscape,
            ErrorKind::MultilineKey,
            ErrorKind::LeadingZero,
            ErrorKind::MisplacedUnderscore,
            ErrorKind::IntegerOutOfRange,
            ErrorKind::FloatOutOfRange,
            ErrorKind::DuplicateKey,
            ErrorKind::DuplicateTable,
            ErrorKind::ExtendsInlineTable,
            ErrorKind::ExtendsDefinedTable,
        ] {
            assert!(!kind.to_string().is_empty());
        }

        // And the edits a document refuses.
        let edits = [
            (EditErrorKind::EmptyPath, "the path has no steps"),
            (
                EditErrorKind::NotATable {
                    found: "an array of tables",
                },
                "a key needs a table, but the path reaches an array of tables there",
            ),
            (
                EditErrorKind::NotAnArray { found: "a table" },
                "an index needs an array, but the path reaches a table there",
            ),
            (
                EditErrorKind::IndexOutOfRange { len: 3 },
                "the index is past the end of an array of 3 elements; an edit never adds elements",
            ),
            (
                EditErrorKind::MissingArray,
                "an index follows a key that does not exist; an edit makes only tables",
            ),
            (
                EditErrorKind::NotReplaceable {
                    found: "a table defined by a header",
                },
                "the value is a table defined by a header, whose text is spread over lines or \
                 sections, so it cannot be replaced whole; set its entries instead",
            ),
            (
                EditErrorKind::InvalidDatetime {
                    expected: "a month from 01 to 12",
                },
                "a date-time in the new value is out of range; expected a month from 01 to 12",
            ),
            (
                EditErrorKind::TooLarge { limit: 8 },
                "the edited text would be longer than the limit of 8 bytes",
            ),
            (
                EditErrorKind::Limit(ErrorKind::TooManyEntries { limit: 2 }),
                "the edited text breaks a limit: an array or table has more than the limit of 2 \
                 entries",
            ),
            (
                EditErrorKind::Unverified {
                    reason: "the edited text does not parse",
                },
                "the edit was refused because the edited text does not parse; this is a bug in \
                 the document model",
            ),
        ];
        for (kind, expected) in edits {
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn values_and_arrays_can_be_built_in_code() {
        let value = Value::from(ValueKind::Integer(7));
        assert_eq!(value.span, Span::new(0, 0));
        assert_eq!(value.as_integer(), Some(7));
        let array = Array::new(vec![value.clone(), value]);
        assert_eq!(array.len(), 2);
        assert!(!array.is_array_of_tables());
        assert!(Array::new(Vec::new()).is_empty());
    }
}
