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

//! JSON (RFC 8259) and, as an option, JSONC: JSON with `//` and `/* */`
//! comments and a trailing comma after the last entry of an array or object.
//!
//! Ported from nadir-codec `json.rs`. Adapted for Tidepool: strict RFC 8259
//! is the default and JSONC is a [`Dialect`] the caller picks (Nadir always
//! accepted JSONC); numbers are a [`Number`] that keeps `u64`, `i64` and
//! `f64` apart and can never be NaN or infinite; repeated keys are kept
//! (Nadir rejected them); the parser takes bytes as well as text, and
//! enforces more [`Limits`](crate::Limits).
//!
//! [`parse`](fn@parse) (or [`parse_bytes`]) reads a document into a
//! [`Value`] tree; [`to_string_compact`] and [`to_string_pretty`] write one
//! back as strict JSON.
//!
//! ```
//! use tidepool_codec::Limits;
//! use tidepool_codec::json::{self, Dialect, Value};
//!
//! let text = r#"{
//!     // Editor settings.
//!     "theme": "tidepool-dark",
//!     "rulers": [80, 100,],
//! }"#;
//! let value = json::parse(text, Dialect::Jsonc, Limits::DEFAULT)?;
//! assert_eq!(value.get("theme").and_then(Value::as_str), Some("tidepool-dark"));
//! assert_eq!(
//!     json::to_string_compact(&value),
//!     r#"{"theme":"tidepool-dark","rulers":[80,100]}"#
//! );
//! // The same text is not strict JSON.
//! assert!(json::parse(text, Dialect::Json, Limits::DEFAULT).is_err());
//! # Ok::<(), tidepool_codec::CodecError>(())
//! ```
//!
//! # What is accepted
//!
//! [`Dialect::Json`] is RFC 8259, with these choices where the RFC leaves
//! room:
//!
//! - Any value at the top level, not only arrays and objects, and exactly
//!   one: anything but whitespace after it is an error.
//! - Whitespace is the RFC's four bytes: space, tab, LF and CR.
//! - A leading byte-order mark (U+FEFF) is skipped (RFC 8259 §8.1 allows
//!   this). One anywhere else outside a string is an error; inside a string
//!   it is an ordinary character.
//! - The input must be UTF-8, everywhere, strings included.
//!   [`parse_bytes`] rejects anything else; it is never repaired.
//! - Escapes are the RFC's: `\" \\ \/ \b \f \n \r \t` and `\uXXXX` with
//!   four hex digits of either case. A surrogate pair written as two `\u`
//!   escapes is one character.
//! - **Lone surrogates are rejected** ([`ErrorKind::LoneSurrogate`]): a
//!   `\u` escape for a high surrogate not followed by one for a low
//!   surrogate, or a low surrogate on its own. A Rust `String` cannot hold
//!   them, and replacing them would silently change the data. RFC 8259 §9
//!   lets a parser limit what strings contain, and JSONTestSuite counts
//!   these cases as implementation-defined. A peer that sends one, as a
//!   JavaScript program can when it splits a string between the halves of a
//!   pair, has its whole message rejected.
//! - **Repeated keys are kept.** RFC 8259 §4 says the names in an object
//!   SHOULD be unique but does not require it, and §9 requires a parser to
//!   accept every text the grammar allows, so an object may have the same
//!   key more than once. Every member is kept, in document order, and
//!   [`Value::get`] returns the last, as JavaScript's `JSON.parse` and most
//!   JSON libraries do. Keys are compared exactly: decoded, case-sensitive
//!   and without Unicode normalization.
//! - Raw control characters (U+0000 to U+001F) in strings are rejected,
//!   tabs and line breaks included. DEL and every other character are
//!   allowed raw.
//! - Numbers follow the RFC grammar: no leading zeros, no leading `+`, no
//!   bare `.5` or `5.`, no hex, no `NaN` or `Infinity`. See [`Number`] for
//!   how they are stored.
//!
//! [`Dialect::Jsonc`] accepts all of that and adds exactly two things:
//!
//! - Comments wherever whitespace may appear: `//` to the end of the line
//!   (the next LF or CR), and `/* ... */`, which does not nest. An
//!   unterminated block comment is an error.
//! - One trailing comma after the last entry of a non-empty array or
//!   object: `[1, 2,]`, `{"a": 1,}`. Not `[,]`, not `[1,,]`, and not after
//!   the top-level value.
//!
//! Comments are not part of the value tree, so they are lost when a
//! document is parsed and written back. Single quotes, unquoted keys and the
//! other JSON5 extensions are errors in both dialects.
//!
//! # Limits and errors
//!
//! Every [`Limits`](crate::Limits) field that applies is enforced: the
//! input size, nesting depth, string length, number length and entries per
//! container. The parser keeps its own stack on the heap, so even a deeply
//! nested document cannot overflow the thread's stack; it fails with
//! [`ErrorKind::TooDeep`] instead. When a parse fails, what it built is
//! taken apart without recursing, so a failure cannot overflow the stack
//! either, whatever the depth limit.
//!
//! Every error is a [`CodecError::Json`](crate::CodecError::Json) with the
//! byte offset, line and column of the problem and an [`ErrorKind`] saying
//! what was found and what was expected. For a string, comment or escape
//! that the text ends inside, the position is where that construct starts.
//! The parser never panics.
//!
//! # Writing
//!
//! [`to_string_compact`] and [`to_string_pretty`] write strict JSON (no
//! comments, no trailing commas), with object members in the order they are
//! stored, repeated keys included, so output is deterministic. Strings are
//! escaped as RFC 8259 requires and no more: `"`, `\` and the control
//! characters U+0000 to U+001F.
//!
//! Reading the output back gives an equal value, provided it fits the
//! [`Limits`](crate::Limits) it is read with. A value the parser produced
//! always fits the limits it was parsed with, except that its output may be
//! longer than its text (pretty output is indented, and `1e3` is written
//! `1000.0`). A value built in code may also be nested deeper, or have
//! longer strings or more entries, than the limits allow.

mod number;
mod parse;
mod write;

use std::fmt;

pub use number::Number;
pub use parse::{parse, parse_bytes};
pub use write::{to_string_compact, to_string_pretty};

/// Which flavor of JSON [`parse`](fn@parse) accepts. See the
/// [module documentation](self) for exactly what each one allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// Strict RFC 8259 JSON. The default.
    #[default]
    Json,
    /// JSON with `//` and `/* */` comments and trailing commas.
    Jsonc,
}

/// A JSON value.
///
/// Objects keep their members in document order, in a `Vec`, so that a
/// document that is read and written back keeps its member order (and any
/// repeated keys) and output is deterministic.
///
/// A value from the parser is at most [`Limits::max_depth`] deep. Dropping,
/// cloning, comparing and writing a value recurse once per level of
/// nesting, so a value built in code should stay similarly shallow.
///
/// [`Limits::max_depth`]: crate::Limits::max_depth
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object: `(key, value)` members in document order. A key may
    /// appear more than once; the parser keeps every member. See
    /// [`Value::get`].
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Returns `true` for [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the boolean, if this is a [`Value::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the number, if this is a [`Value::Number`].
    pub fn as_number(&self) -> Option<Number> {
        match *self {
            Self::Number(number) => Some(number),
            _ => None,
        }
    }

    /// Returns the number as an `i64`, if this is an integer
    /// [`Value::Number`] that fits one. See [`Number::as_i64`].
    pub fn as_i64(&self) -> Option<i64> {
        self.as_number()?.as_i64()
    }

    /// Returns the number as a `u64`, if this is an integer
    /// [`Value::Number`] that fits one. See [`Number::as_u64`].
    pub fn as_u64(&self) -> Option<u64> {
        self.as_number()?.as_u64()
    }

    /// Returns the number as an `f64`, if this is a [`Value::Number`]. See
    /// [`Number::as_f64`].
    pub fn as_f64(&self) -> Option<f64> {
        Some(self.as_number()?.as_f64())
    }

    /// Returns the string, if this is a [`Value::String`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the elements, if this is a [`Value::Array`].
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Returns the members in document order, if this is a
    /// [`Value::Object`].
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Returns the value of the member named `key`, if this is a
    /// [`Value::Object`] that has one.
    ///
    /// If the object has the key more than once, the last member wins, as
    /// in JavaScript's `JSON.parse` and most JSON libraries, so that
    /// Tidepool reads the same value from a document as other programs do.
    /// Use [`Value::as_object`] to see every member. Keys are compared
    /// exactly (case-sensitive, no Unicode normalization). The search is
    /// linear.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

impl From<bool> for Value {
    /// [`Value::Bool`].
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<Number> for Value {
    /// [`Value::Number`].
    fn from(value: Number) -> Self {
        Self::Number(value)
    }
}

impl From<i64> for Value {
    /// An integer [`Value::Number`].
    fn from(value: i64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl From<u64> for Value {
    /// An integer [`Value::Number`].
    fn from(value: u64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl From<&str> for Value {
    /// [`Value::String`].
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for Value {
    /// [`Value::String`].
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<Vec<Value>> for Value {
    /// [`Value::Array`].
    fn from(items: Vec<Value>) -> Self {
        Self::Array(items)
    }
}

/// What is wrong with a piece of JSON text. See
/// [`CodecError::Json`](crate::CodecError::Json), which adds where.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input ended too soon.
    ///
    /// Usually reported at the end of the input. When the input ends inside
    /// a string, a comment or an escape sequence, it is reported where that
    /// starts instead, and `expected` says so: for example "`\"` to close
    /// the string that starts here".
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
    /// A word that looks like `null`, `true` or `false` but is not one.
    InvalidLiteral,
    /// A backslash in a string followed by something that is not an escape.
    InvalidEscape,
    /// A `\u` escape that is half of a surrogate pair without its partner.
    LoneSurrogate,
    /// A raw control character (U+0000 to U+001F) inside a string, such as
    /// a tab or a line break.
    ControlCharacter,
    /// A number with leading zeros, such as `007`.
    LeadingZero,
    /// A number too large in magnitude for an `f64`, such as `1e400`.
    NumberOutOfRange,
    /// A comment in [`Dialect::Json`], which has none.
    CommentNotAllowed,
    /// A comma before `]` or `}` in [`Dialect::Json`], which allows trailing
    /// commas only in [`Dialect::Jsonc`].
    TrailingCommaNotAllowed,
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
    /// Arrays and objects nest deeper than [`Limits::max_depth`].
    ///
    /// [`Limits::max_depth`]: crate::Limits::max_depth
    TooDeep {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A string is longer than [`Limits::max_string_bytes`].
    ///
    /// [`Limits::max_string_bytes`]: crate::Limits::max_string_bytes
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A number is longer than [`Limits::max_number_bytes`].
    ///
    /// [`Limits::max_number_bytes`]: crate::Limits::max_number_bytes
    NumberTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// An array or object has more than [`Limits::max_entries`] entries.
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
            Self::InvalidLiteral => {
                f.write_str("unknown word; the only bare words are `null`, `true` and `false`")
            }
            Self::InvalidEscape => f.write_str("invalid escape sequence in a string"),
            Self::LoneSurrogate => f.write_str("a `\\u` escape is half of a surrogate pair"),
            Self::ControlCharacter => {
                f.write_str("control character in a string; write it as an escape such as `\\n`")
            }
            Self::LeadingZero => f.write_str("numbers cannot have leading zeros"),
            Self::NumberOutOfRange => f.write_str("the number is too large"),
            Self::CommentNotAllowed => f.write_str("comments are allowed in JSONC, not in JSON"),
            Self::TrailingCommaNotAllowed => {
                f.write_str("a trailing comma is allowed in JSONC, not in JSON")
            }
            Self::InputTooLarge { length, limit } => write!(
                f,
                "the text is {length} bytes, more than the limit of {limit} bytes"
            ),
            Self::TooDeep { limit } => write!(
                f,
                "arrays and objects nest deeper than the limit of {limit}"
            ),
            Self::StringTooLong { limit } => {
                write!(f, "a string is longer than the limit of {limit} bytes")
            }
            Self::NumberTooLong { limit } => {
                write!(f, "a number is longer than the limit of {limit} bytes")
            }
            Self::TooManyEntries { limit } => write!(
                f,
                "an array or object has more than the limit of {limit} entries"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        Value::Object(vec![
            ("name".to_string(), Value::from("tidepool-codec")),
            ("crates".to_string(), Value::from(26i64)),
            (
                "ratio".to_string(),
                Value::Number(Number::from_f64(1.5).expect("finite")),
            ),
            ("published".to_string(), Value::from(false)),
            ("source".to_string(), Value::Null),
            (
                "ids".to_string(),
                Value::from(vec![Value::from(1i64), Value::from(u64::MAX)]),
            ),
        ])
    }

    #[test]
    fn accessors_return_only_the_matching_kind() {
        let value = sample();
        assert_eq!(
            value.get("name").and_then(Value::as_str),
            Some("tidepool-codec")
        );
        assert_eq!(value.get("crates").and_then(Value::as_i64), Some(26));
        assert_eq!(value.get("crates").and_then(Value::as_u64), Some(26));
        assert_eq!(value.get("ratio").and_then(Value::as_f64), Some(1.5));
        assert_eq!(value.get("published").and_then(Value::as_bool), Some(false));
        assert!(value.get("source").is_some_and(Value::is_null));
        assert_eq!(
            value.get("ids").and_then(Value::as_array),
            Some(&[Value::from(1i64), Value::from(u64::MAX)][..])
        );
        assert_eq!(value.as_object().map(<[_]>::len), Some(6));

        let name = value.get("name").expect("has a name");
        assert_eq!(name.as_number(), None);
        assert_eq!(name.as_i64(), None);
        assert_eq!(name.as_u64(), None);
        assert_eq!(name.as_f64(), None);
        assert_eq!(name.as_bool(), None);
        assert_eq!(name.as_array(), None);
        assert_eq!(name.as_object(), None);
        assert!(!name.is_null());
        assert_eq!(Value::from(1i64).as_str(), None);
        assert_eq!(Value::from(String::from("x")), Value::from("x"));
    }

    #[test]
    fn get_finds_members_of_objects_only() {
        let value = sample();
        assert_eq!(value.get("missing"), None);
        assert_eq!(value.get("ids").and_then(|ids| ids.get("0")), None);
        assert_eq!(Value::Null.get("name"), None);
    }

    #[test]
    fn get_returns_the_last_member_with_the_key() {
        let repeated = Value::Object(vec![
            ("k".to_string(), Value::from(1i64)),
            ("K".to_string(), Value::from(2i64)),
            ("k".to_string(), Value::from(3i64)),
            ("other".to_string(), Value::Null),
        ]);
        assert_eq!(repeated.get("k"), Some(&Value::from(3i64)));
        // Keys are compared exactly.
        assert_eq!(repeated.get("K"), Some(&Value::from(2i64)));
        assert_eq!(repeated.get("o"), None);
        assert_eq!(repeated.as_object().map(<[_]>::len), Some(4));
    }

    #[test]
    fn the_default_dialect_is_strict_json() {
        assert_eq!(Dialect::default(), Dialect::Json);
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
                ErrorKind::UnexpectedCharacter {
                    found: '\n',
                    expected: "`,` or `]`",
                },
                "found '\\n' where `,` or `]` was expected",
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
                "arrays and objects nest deeper than the limit of 64",
            ),
            (
                ErrorKind::StringTooLong { limit: 8 },
                "a string is longer than the limit of 8 bytes",
            ),
            (
                ErrorKind::NumberTooLong { limit: 4 },
                "a number is longer than the limit of 4 bytes",
            ),
            (
                ErrorKind::TooManyEntries { limit: 2 },
                "an array or object has more than the limit of 2 entries",
            ),
            (
                ErrorKind::CommentNotAllowed,
                "comments are allowed in JSONC, not in JSON",
            ),
            (
                ErrorKind::TrailingCommaNotAllowed,
                "a trailing comma is allowed in JSONC, not in JSON",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
        // The rest have fixed messages; they must at least not be empty.
        for kind in [
            ErrorKind::InvalidUtf8,
            ErrorKind::InvalidLiteral,
            ErrorKind::InvalidEscape,
            ErrorKind::LoneSurrogate,
            ErrorKind::ControlCharacter,
            ErrorKind::LeadingZero,
            ErrorKind::NumberOutOfRange,
        ] {
            assert!(!kind.to_string().is_empty());
        }
    }
}
