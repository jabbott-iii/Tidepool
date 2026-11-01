//! The error type for `tidepool-codec`.
//!
//! Ported from nadir-codec `error.rs`. Adapted for Tidepool: only the JSON
//! variant exists so far (other formats add theirs with their roadmap
//! tasks), and a JSON error carries the byte offset as well as the line and
//! column.

use std::fmt;

use crate::json;

/// Errors produced by `tidepool-codec`.
///
/// Non-exhaustive: each format adds its own variant when it arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    /// JSON or JSONC text is malformed or exceeds a [`Limits`](crate::Limits)
    /// bound.
    Json {
        /// The byte offset of the problem in the input as given, counting a
        /// leading byte-order mark if there is one. Always at the start of
        /// a character when the input before it is UTF-8.
        ///
        /// For a string, comment or escape sequence that the input ends
        /// inside, this is where that construct starts, not the end of the
        /// input; see [`json::ErrorKind::UnexpectedEnd`].
        offset: usize,
        /// The line of the problem, starting at 1. Lines end at each LF
        /// (`\n`); a CR-LF pair ends one line.
        line: usize,
        /// The column of the problem, starting at 1 and counted in
        /// characters (Unicode scalar values), not bytes. A leading
        /// byte-order mark is not counted.
        column: usize,
        /// What is wrong.
        kind: json::ErrorKind,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json {
                offset,
                line,
                column,
                kind,
            } => write!(
                f,
                "JSON error at line {line}, column {column} (byte {offset}): {kind}"
            ),
        }
    }
}

impl std::error::Error for CodecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_format_the_position_and_the_problem() {
        let error = CodecError::Json {
            offset: 17,
            line: 3,
            column: 14,
            kind: json::ErrorKind::LeadingZero,
        };
        assert_eq!(
            error.to_string(),
            "JSON error at line 3, column 14 (byte 17): numbers cannot have leading zeros"
        );
    }

    #[test]
    fn it_is_a_std_error() {
        let error: Box<dyn std::error::Error> = Box::new(CodecError::Json {
            offset: 0,
            line: 1,
            column: 1,
            kind: json::ErrorKind::InvalidLiteral,
        });
        assert!(error.source().is_none());
        assert!(
            error
                .to_string()
                .starts_with("JSON error at line 1, column 1")
        );
    }
}
