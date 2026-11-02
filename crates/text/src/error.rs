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

//! The error type for `tidepool-text`.

use std::fmt;

use crate::pos::{LineCol, Unit};

/// Errors produced by `tidepool-text`: a byte offset, range, offset in
/// another unit, line or column that the text does not have; a change set
/// that does not fit a text or another change set, or would make a text too
/// long; a revision the history does not have, or a text it did not
/// produce.
///
/// Every function that returns one changes nothing. Messages hold numbers
/// only, never any of the text.
///
/// Non-exhaustive: loading and saving files (roadmap M1.3) add variants.
#[derive(Debug)]
#[non_exhaustive]
pub enum TextError {
    /// A byte offset is past the end of the text.
    ByteOutOfBounds {
        /// The offset.
        byte: usize,
        /// The length of the text in bytes.
        len: usize,
    },
    /// A byte offset is inside a character, not between two.
    NotCharBoundary {
        /// The offset.
        byte: usize,
    },
    /// A byte range ends before it starts.
    InvertedRange {
        /// The start of the range.
        start: usize,
        /// The end of the range, which is less than `start`.
        end: usize,
    },
    /// A char or UTF-16 offset is past the end of the text.
    OffsetOutOfBounds {
        /// The offset.
        offset: usize,
        /// What the offset counts.
        unit: Unit,
        /// The length of the text in `unit`.
        len: usize,
    },
    /// A UTF-16 offset is between the two halves of a surrogate pair.
    OffsetInsideChar {
        /// The offset.
        offset: usize,
        /// What the offset counts.
        unit: Unit,
    },
    /// A line index is not less than the number of lines.
    LineOutOfBounds {
        /// The line, counted from 0.
        line: usize,
        /// The number of lines in the text (at least 1).
        len_lines: usize,
    },
    /// A column is past the last column of its line.
    ColumnOutOfBounds {
        /// The line and column asked for.
        pos: LineCol,
        /// What the column counts.
        unit: Unit,
        /// The last column of that line, in `unit`.
        max: usize,
    },
    /// A column is inside a character of its line.
    ColumnInsideChar {
        /// The line and column asked for.
        pos: LineCol,
        /// What the column counts.
        unit: Unit,
    },
    /// A change set was given a text, or composed with a change set, of
    /// another length.
    LengthMismatch {
        /// The length in bytes the change set is for.
        expected: usize,
        /// The length in bytes of the text, or of the first change set's
        /// result, that it was given.
        actual: usize,
    },
    /// Two edits change some of the same bytes, or one inserts strictly
    /// inside the other's range.
    OverlappingEdits {
        /// The end of the earlier edit.
        end: usize,
        /// The start of the next edit, which is less than `end`.
        start: usize,
    },
    /// An edit would make the text longer than a text can be.
    TooLong {
        /// The most bytes a text can have: `isize::MAX`.
        max: usize,
    },
    /// A revision is not in the history.
    RevisionNotFound {
        /// The revision's sequence number.
        revision: u64,
        /// The number of revisions in the history.
        count: usize,
    },
    /// A rope is not the tree the history last produced: something edited
    /// it without going through the history.
    HistoryMismatch {
        /// The length in bytes of the rope given.
        len: usize,
        /// The length in bytes of the text the history last produced.
        expected: usize,
    },
}

/// The name of one `unit`, as in "UTF-16 offset 4".
fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Byte => "byte",
        Unit::Utf16 => "UTF-16",
        Unit::Char => "char",
    }
}

/// The name of several `unit`s, as in "10 UTF-16 units".
fn unit_plural(unit: Unit) -> &'static str {
    match unit {
        Unit::Byte => "bytes",
        Unit::Utf16 => "UTF-16 units",
        Unit::Char => "chars",
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ByteOutOfBounds { byte, len } => write!(
                f,
                "byte offset {byte} is past the end of the text ({len} bytes)"
            ),
            Self::NotCharBoundary { byte } => {
                write!(f, "byte offset {byte} is inside a character")
            }
            Self::InvertedRange { start, end } => {
                write!(f, "the range {start}..{end} ends before it starts")
            }
            Self::OffsetOutOfBounds { offset, unit, len } => write!(
                f,
                "{} offset {offset} is past the end of the text ({len} {})",
                unit_name(unit),
                unit_plural(unit)
            ),
            Self::OffsetInsideChar { offset, unit } => write!(
                f,
                "{} offset {offset} is inside a character",
                unit_name(unit)
            ),
            Self::LineOutOfBounds { line, len_lines } => write!(
                f,
                "line {line} (counted from 0) is past the end of the text ({len_lines} lines)"
            ),
            Self::ColumnOutOfBounds { pos, unit, max } => write!(
                f,
                "{} column {} is past the end of line {} (its last column is {max})",
                unit_name(unit),
                pos.col,
                pos.line
            ),
            Self::ColumnInsideChar { pos, unit } => write!(
                f,
                "{} column {} of line {} is inside a character",
                unit_name(unit),
                pos.col,
                pos.line
            ),
            Self::LengthMismatch { expected, actual } => write!(
                f,
                "the change set is for a text of {expected} bytes, not {actual}"
            ),
            Self::OverlappingEdits { end, start } => write!(
                f,
                "an edit ending at byte {end} overlaps the next, which starts at byte {start}"
            ),
            Self::TooLong { max } => {
                write!(f, "the edited text would be longer than {max} bytes")
            }
            Self::RevisionNotFound { revision, count } => write!(
                f,
                "revision {revision} is not in this history ({count} revisions)"
            ),
            Self::HistoryMismatch { len, expected } => write!(
                f,
                "the text ({len} bytes) is not the one the history last produced ({expected} bytes)"
            ),
        }
    }
}

impl std::error::Error for TextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_names_its_numbers() {
        let pos = LineCol::new(3, 9);
        let cases = [
            (
                TextError::ByteOutOfBounds { byte: 12, len: 10 },
                "byte offset 12 is past the end of the text (10 bytes)",
            ),
            (
                TextError::NotCharBoundary { byte: 5 },
                "byte offset 5 is inside a character",
            ),
            (
                TextError::InvertedRange { start: 7, end: 2 },
                "the range 7..2 ends before it starts",
            ),
            (
                TextError::OffsetOutOfBounds {
                    offset: 11,
                    unit: Unit::Utf16,
                    len: 10,
                },
                "UTF-16 offset 11 is past the end of the text (10 UTF-16 units)",
            ),
            (
                TextError::OffsetOutOfBounds {
                    offset: 4,
                    unit: Unit::Char,
                    len: 3,
                },
                "char offset 4 is past the end of the text (3 chars)",
            ),
            (
                TextError::OffsetInsideChar {
                    offset: 2,
                    unit: Unit::Utf16,
                },
                "UTF-16 offset 2 is inside a character",
            ),
            (
                TextError::LineOutOfBounds {
                    line: 4,
                    len_lines: 4,
                },
                "line 4 (counted from 0) is past the end of the text (4 lines)",
            ),
            (
                TextError::ColumnOutOfBounds {
                    pos,
                    unit: Unit::Byte,
                    max: 8,
                },
                "byte column 9 is past the end of line 3 (its last column is 8)",
            ),
            (
                TextError::ColumnInsideChar {
                    pos,
                    unit: Unit::Utf16,
                },
                "UTF-16 column 9 of line 3 is inside a character",
            ),
            (
                TextError::LengthMismatch {
                    expected: 12,
                    actual: 10,
                },
                "the change set is for a text of 12 bytes, not 10",
            ),
            (
                TextError::OverlappingEdits { end: 8, start: 5 },
                "an edit ending at byte 8 overlaps the next, which starts at byte 5",
            ),
            (
                TextError::TooLong { max: 99 },
                "the edited text would be longer than 99 bytes",
            ),
            (
                TextError::RevisionNotFound {
                    revision: 7,
                    count: 3,
                },
                "revision 7 is not in this history (3 revisions)",
            ),
            (
                TextError::HistoryMismatch {
                    len: 4,
                    expected: 6,
                },
                "the text (4 bytes) is not the one the history last produced (6 bytes)",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn it_is_a_send_sync_std_error() {
        fn assert_traits<T: std::error::Error + Send + Sync + 'static>() {}
        assert_traits::<TextError>();
        let error: Box<dyn std::error::Error> = Box::new(TextError::NotCharBoundary { byte: 1 });
        assert!(error.source().is_none());
    }
}
