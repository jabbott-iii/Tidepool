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
//! returns its root table. There is no writer yet, and comments are not
//! kept: the document model that preserves them arrives with roadmap task
//! M1.5.
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
//! Every error is a [`CodecError::Toml`](crate::CodecError::Toml) with the
//! byte offset, line and column of the problem and an [`ErrorKind`] saying
//! what was wrong and what was expected. For a string or escape that the
//! text ends inside, the position is where that construct starts. Messages
//! never quote the input beyond the one character found. The parser never
//! panics.

mod datetime;
mod parse;
mod table;

use std::fmt;
use std::ops::Range;

pub use datetime::{Date, Datetime, Offset, Time};
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
    }
}
