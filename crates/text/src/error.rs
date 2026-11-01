//! The error type for `tidepool-text`.

use std::fmt;

use crate::pos::{LineCol, Unit};

/// Errors produced by `tidepool-text`: a byte offset, range, offset in
/// another unit, line or column that the text does not have.
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
