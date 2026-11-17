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

//! The error type for `tidepool-codec`.
//!
//! Ported from nadir-codec `error.rs`. Adapted for Tidepool: only the JSON,
//! TOML and QOI variants exist so far (other formats add theirs with their
//! roadmap tasks), and the JSON and TOML parse errors carry the byte offset
//! as well as the line and column. TOML also has a variant of its own for an
//! edit that `toml::Document` refuses, which has a path step instead of a
//! position. QOI's two variants and their messages are Nadir's.

use std::fmt;

use crate::{json, qoi, toml};

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
    /// TOML text is malformed, breaks a rule of TOML 1.1, or exceeds a
    /// [`Limits`](crate::Limits) bound.
    Toml {
        /// The byte offset of the problem in the input as given, counting a
        /// leading byte-order mark if there is one. Always at the start of
        /// a character when the input before it is UTF-8.
        ///
        /// For a string or escape sequence that the input ends inside, this
        /// is where that construct starts, not the end of the input; see
        /// [`toml::ErrorKind::UnexpectedEnd`].
        offset: usize,
        /// The line of the problem, starting at 1. Lines end at each LF
        /// (`\n`); a CR-LF pair ends one line.
        line: usize,
        /// The column of the problem, starting at 1 and counted in
        /// characters (Unicode scalar values), not bytes. A leading
        /// byte-order mark is not counted.
        column: usize,
        /// What is wrong.
        kind: toml::ErrorKind,
    },
    /// [`toml::Document::set`] refused an edit, and the document is
    /// unchanged. Kept apart from [`CodecError::Toml`], so that a caller can
    /// tell a file that does not parse from an edit that cannot be made.
    TomlEdit {
        /// The index of the step of the path that the problem concerns: the
        /// step that could not be taken, `path.len() - 1` for the value at
        /// the end of it, or `path.len()` for the edit as a whole. See
        /// [`toml::EditErrorKind`] for which.
        step: usize,
        /// What is wrong.
        kind: toml::EditErrorKind,
    },
    /// A QOI file is malformed or exceeds a [`Limits`](crate::Limits) bound.
    Qoi {
        /// The byte offset of the problem in the file. For a file cut short
        /// ([`qoi::ErrorKind::UnexpectedEnd`]), its length.
        offset: usize,
        /// What is wrong.
        kind: qoi::ErrorKind,
    },
    /// An image cannot be encoded as QOI.
    QoiEncode {
        /// Why not.
        kind: qoi::EncodeErrorKind,
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
            Self::Toml {
                offset,
                line,
                column,
                kind,
            } => write!(
                f,
                "TOML error at line {line}, column {column} (byte {offset}): {kind}"
            ),
            Self::TomlEdit { step, kind } => {
                write!(f, "TOML edit refused at step {step} of its path: {kind}")
            }
            Self::Qoi { offset, kind } => write!(f, "QOI error at byte {offset}: {kind}"),
            Self::QoiEncode { kind } => write!(f, "cannot encode as QOI: {kind}"),
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
    fn toml_errors_display_the_same_way() {
        let error = CodecError::Toml {
            offset: 40,
            line: 5,
            column: 1,
            kind: toml::ErrorKind::DuplicateKey,
        };
        assert_eq!(
            error.to_string(),
            "TOML error at line 5, column 1 (byte 40): the key is already defined"
        );
    }

    #[test]
    fn toml_edits_display_their_step() {
        let error = CodecError::TomlEdit {
            step: 2,
            kind: toml::EditErrorKind::IndexOutOfRange { len: 2 },
        };
        assert_eq!(
            error.to_string(),
            "TOML edit refused at step 2 of its path: the index is past the end of an array of 2 \
             elements; an edit never adds elements"
        );
    }

    #[test]
    fn qoi_errors_display_the_byte_or_what_cannot_be_encoded() {
        let error = CodecError::Qoi {
            offset: 12,
            kind: qoi::ErrorKind::BadChannels { found: 5 },
        };
        assert_eq!(
            error.to_string(),
            "QOI error at byte 12: the channel count is 5, not 3 or 4"
        );
        let error = CodecError::QoiEncode {
            kind: qoi::EncodeErrorKind::AlphaInRgb { pixel: 3 },
        };
        assert_eq!(
            error.to_string(),
            "cannot encode as QOI: pixel 3 of an RGB image has an alpha other than 255"
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
