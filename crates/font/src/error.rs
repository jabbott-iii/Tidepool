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

//! The error type for `tidepool-font`.
//!
//! New in Tidepool, in the shape of `tidepool_codec::CodecError`: one
//! variant per place a problem can be, each with what is wrong as a `kind`.

use std::fmt;

use crate::ttf::{self, GlyphId, Tag};
use crate::{atlas, fallback, outline};

/// Errors produced by `tidepool-font`.
///
/// Non-exhaustive: later tasks may add their own.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontError {
    /// A font file, its table directory or one of its tables is malformed,
    /// unsupported, or exceeds a [`Limits`](crate::Limits) bound. Returned
    /// by [`Font::parse`](ttf::Font::parse).
    Ttf {
        /// The table the problem is in, or `None` for the file itself and
        /// its table directory.
        table: Option<Tag>,
        /// The byte offset of the problem in the file, never past its end.
        /// For a read that does not fit, where the read starts, or the end
        /// of the table (or file) if it starts beyond it; see
        /// [`ttf::ErrorKind`] for the others.
        offset: usize,
        /// What is wrong.
        kind: ttf::ErrorKind,
    },
    /// One glyph's data is malformed, or its outline exceeds a
    /// [`Limits`](crate::Limits) bound. Returned by
    /// [`Font::outline`](ttf::Font::outline).
    Glyph {
        /// The glyph whose data holds the problem: for a composite, the
        /// component (or composite of components) where it is, not
        /// necessarily the glyph that was asked for.
        glyph: GlyphId,
        /// The byte offset of the problem in the file, never past its end,
        /// as for [`FontError::Ttf`].
        offset: usize,
        /// What is wrong.
        kind: ttf::ErrorKind,
    },
    /// An outline cannot be built or rasterized as asked: see
    /// [`Outline::new`](outline::Outline::new) and
    /// [`outline::rasterize`].
    Outline {
        /// What is wrong.
        kind: outline::ErrorKind,
    },
    /// A face is not in a [`Faces`](crate::fallback::Faces), or cannot be
    /// added to one or to its fallback chain. Returned by `Faces`, and by
    /// layout and the atlas for a face they are not given.
    Faces {
        /// What is wrong.
        kind: fallback::ErrorKind,
    },
    /// An [`Atlas`](crate::atlas::Atlas) cannot be made, or cannot hold a
    /// glyph.
    Atlas {
        /// What is wrong.
        kind: atlas::ErrorKind,
    },
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ttf {
                table: Some(table),
                offset,
                kind,
            } => write!(
                f,
                "TrueType error in table `{table}` at byte {offset}: {kind}"
            ),
            Self::Ttf {
                table: None,
                offset,
                kind,
            } => write!(f, "TrueType error at byte {offset}: {kind}"),
            Self::Glyph {
                glyph,
                offset,
                kind,
            } => write!(
                f,
                "TrueType error in glyph {} at byte {offset}: {kind}",
                glyph.0
            ),
            Self::Outline { kind } => write!(f, "outline error: {kind}"),
            Self::Faces { kind } => write!(f, "faces error: {kind}"),
            Self::Atlas { kind } => write!(f, "atlas error: {kind}"),
        }
    }
}

impl std::error::Error for FontError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_format_the_place_and_the_problem() {
        let error = FontError::Ttf {
            table: Some(Tag(*b"glyf")),
            offset: 1234,
            kind: ttf::ErrorKind::UnexpectedEnd {
                expected: "a glyph's header",
            },
        };
        assert_eq!(
            error.to_string(),
            "TrueType error in table `glyf` at byte 1234: the data ends before a glyph's header"
        );
        let error = FontError::Ttf {
            table: None,
            offset: 0,
            kind: ttf::ErrorKind::Collection,
        };
        assert_eq!(
            error.to_string(),
            "TrueType error at byte 0: the file is a font collection (`ttcf`), which is not \
             supported"
        );
        let error = FontError::Glyph {
            glyph: GlyphId(7),
            offset: 40,
            kind: ttf::ErrorKind::TooDeep { limit: 8 },
        };
        assert_eq!(
            error.to_string(),
            "TrueType error in glyph 7 at byte 40: composite glyphs nest deeper than the limit \
             of 8"
        );
        let error = FontError::Outline {
            kind: outline::ErrorKind::BitmapTooLarge { limit: 64 },
        };
        assert_eq!(
            error.to_string(),
            "outline error: the bitmap would be wider or taller than the limit of 64 pixels"
        );
        let error = FontError::Faces {
            kind: fallback::ErrorKind::UnknownFace {
                face: fallback::FaceId(9),
            },
        };
        assert_eq!(error.to_string(), "faces error: there is no face 9");
        let error = FontError::Faces {
            kind: fallback::ErrorKind::TooManyFaces { limit: 64 },
        };
        assert_eq!(
            error.to_string(),
            "faces error: a set of faces holds at most 64"
        );
        let error = FontError::Faces {
            kind: fallback::ErrorKind::RepeatedFallback {
                face: fallback::FaceId(4),
            },
        };
        assert_eq!(
            error.to_string(),
            "faces error: face 4 is in the fallback chain twice"
        );
        let error = FontError::Atlas {
            kind: atlas::ErrorKind::BadSize {
                width: 0,
                height: 8,
            },
        };
        assert_eq!(
            error.to_string(),
            "atlas error: an atlas of 0 × 8 pixels has a side of 0 or above 4096"
        );
        let error = FontError::Atlas {
            kind: atlas::ErrorKind::GlyphTooLarge { limit: 512 },
        };
        assert_eq!(
            error.to_string(),
            "atlas error: the glyph's image would be wider or taller than 512 pixels, half the \
             atlas's shorter side"
        );
    }

    #[test]
    fn it_is_a_std_error() {
        let error: Box<dyn std::error::Error> = Box::new(FontError::Outline {
            kind: outline::ErrorKind::BadScale,
        });
        assert!(error.source().is_none());
        assert!(error.to_string().starts_with("outline error: "));
    }
}
