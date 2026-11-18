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

//! Bounds-checked big-endian reads: [`Span`], a range of the file that
//! turns every read that does not fit into a [`FontError`] with its place,
//! for parsing; and the `be*` functions, which give `None` instead, for
//! lookups in what parsing has already checked.

use super::{ErrorKind, GlyphId, Tag};
use crate::FontError;

/// What a [`Span`]'s errors name: a table (or `None`, the file and its
/// directory), or one glyph's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// A table, or the file.
    Table(Option<Tag>),
    /// A glyph's data in `glyf`.
    Glyph(GlyphId),
}

/// A range `start..end` of the file, which lies inside it, and the place
/// its errors name. Reads take offsets relative to `start`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span<'a> {
    /// The whole file.
    data: &'a [u8],
    /// The first byte, absolute.
    start: usize,
    /// One past the last byte, absolute, at most `data.len()`.
    end: usize,
    place: Place,
}

impl<'a> Span<'a> {
    /// The whole file, whose errors name no table.
    pub(crate) fn file(data: &'a [u8]) -> Self {
        Self {
            data,
            start: 0,
            end: data.len(),
            place: Place::Table(None),
        }
    }

    /// `start..end` of `data`, kept inside it, naming `place`.
    pub(crate) fn new(data: &'a [u8], start: usize, end: usize, place: Place) -> Self {
        let end = end.min(data.len());
        Self {
            data,
            start: start.min(end),
            end,
            place,
        }
    }

    /// The absolute offset of the first byte.
    pub(crate) fn start(&self) -> usize {
        self.start
    }

    /// The absolute offset one past the last byte.
    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// The length in bytes.
    pub(crate) fn len(&self) -> usize {
        self.end - self.start
    }

    /// The absolute offset of `at`, relative to the start, kept at or
    /// before the end.
    pub(crate) fn absolute(&self, at: usize) -> usize {
        self.start.saturating_add(at).min(self.end)
    }

    /// An error of `kind` at `at`, relative to the start (kept at or
    /// before the end).
    pub(crate) fn error(&self, at: usize, kind: ErrorKind) -> FontError {
        let offset = self.absolute(at);
        match self.place {
            Place::Table(table) => FontError::Ttf {
                table,
                offset,
                kind,
            },
            Place::Glyph(glyph) => FontError::Glyph {
                glyph,
                offset,
                kind,
            },
        }
    }

    /// The `len` bytes at `at`, or an error that the data ends before
    /// `expected`.
    pub(crate) fn bytes(
        &self,
        at: usize,
        len: usize,
        expected: &'static str,
    ) -> Result<&'a [u8], FontError> {
        let range = self
            .start
            .checked_add(at)
            .and_then(|from| Some((from, from.checked_add(len)?)));
        match range {
            Some((from, to)) if to <= self.end => self
                .data
                .get(from..to)
                .ok_or_else(|| self.end_error(at, expected)),
            _ => Err(self.end_error(at, expected)),
        }
    }

    fn end_error(&self, at: usize, expected: &'static str) -> FontError {
        self.error(at, ErrorKind::UnexpectedEnd { expected })
    }

    /// The range of `len` bytes at `at`, naming the same place.
    pub(crate) fn sub(
        &self,
        at: usize,
        len: usize,
        expected: &'static str,
    ) -> Result<Self, FontError> {
        self.bytes(at, len, expected)?;
        let start = self.start + at;
        Ok(Self {
            start,
            end: start + len,
            ..*self
        })
    }

    /// The range from `at` to the end, naming the same place. `at` may be
    /// the end itself, which gives an empty range.
    pub(crate) fn from(&self, at: usize, expected: &'static str) -> Result<Self, FontError> {
        let len = self
            .len()
            .checked_sub(at)
            .ok_or_else(|| self.end_error(at, expected))?;
        self.sub(at, len, expected)
    }

    /// The byte at `at`.
    pub(crate) fn u8(&self, at: usize, expected: &'static str) -> Result<u8, FontError> {
        let bytes = self.bytes(at, 1, expected)?;
        bytes
            .first()
            .copied()
            .ok_or_else(|| self.end_error(at, expected))
    }

    /// The big-endian `u16` at `at`.
    pub(crate) fn u16(&self, at: usize, expected: &'static str) -> Result<u16, FontError> {
        let bytes = self.bytes(at, 2, expected)?;
        match *bytes {
            [a, b] => Ok(u16::from_be_bytes([a, b])),
            _ => Err(self.end_error(at, expected)),
        }
    }

    /// The big-endian `i16` at `at`.
    pub(crate) fn i16(&self, at: usize, expected: &'static str) -> Result<i16, FontError> {
        self.u16(at, expected).map(|value| value as i16)
    }

    /// The big-endian `u32` at `at`.
    pub(crate) fn u32(&self, at: usize, expected: &'static str) -> Result<u32, FontError> {
        let bytes = self.bytes(at, 4, expected)?;
        match *bytes {
            [a, b, c, d] => Ok(u32::from_be_bytes([a, b, c, d])),
            _ => Err(self.end_error(at, expected)),
        }
    }

    /// The tag at `at`.
    pub(crate) fn tag(&self, at: usize, expected: &'static str) -> Result<Tag, FontError> {
        let bytes = self.bytes(at, 4, expected)?;
        match *bytes {
            [a, b, c, d] => Ok(Tag([a, b, c, d])),
            _ => Err(self.end_error(at, expected)),
        }
    }
}

/// The big-endian `u16` at absolute offset `at` of `data`, if it holds one
/// there.
pub(crate) fn be16(data: &[u8], at: usize) -> Option<u16> {
    match *data.get(at..at.checked_add(2)?)? {
        [a, b] => Some(u16::from_be_bytes([a, b])),
        _ => None,
    }
}

/// The big-endian `i16` at absolute offset `at` of `data`.
pub(crate) fn be16_signed(data: &[u8], at: usize) -> Option<i16> {
    be16(data, at).map(|value| value as i16)
}

/// The big-endian `u32` at absolute offset `at` of `data`.
pub(crate) fn be32(data: &[u8], at: usize) -> Option<u32> {
    match *data.get(at..at.checked_add(4)?)? {
        [a, b, c, d] => Some(u32::from_be_bytes([a, b, c, d])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &[u8] = &[0x00, 0x01, 0xff, 0xfe, 0x12, 0x34, 0x56, 0x78];

    #[test]
    fn reads_are_big_endian_and_relative_to_the_start() {
        let span = Span::new(DATA, 2, 8, Place::Table(Some(Tag(*b"test"))));
        assert_eq!(span.len(), 6);
        assert_eq!(span.u16(0, "x"), Ok(0xfffe));
        assert_eq!(span.i16(0, "x"), Ok(-2));
        assert_eq!(span.u32(2, "x"), Ok(0x1234_5678));
        assert_eq!(span.u8(5, "x"), Ok(0x78));
        assert_eq!(span.tag(2, "x"), Ok(Tag([0x12, 0x34, 0x56, 0x78])));
    }

    #[test]
    fn a_read_that_does_not_fit_is_an_error_at_its_start_or_the_end() {
        let span = Span::new(DATA, 2, 6, Place::Table(Some(Tag(*b"test"))));
        let error = |offset| FontError::Ttf {
            table: Some(Tag(*b"test")),
            offset,
            kind: ErrorKind::UnexpectedEnd { expected: "x" },
        };
        // The span ends at 6, though the data goes on.
        assert_eq!(span.u32(2, "x"), Err(error(4)));
        assert_eq!(span.u16(3, "x"), Err(error(5)));
        // A read starting past the end is reported at the end.
        assert_eq!(span.u8(100, "x"), Err(error(6)));
        assert_eq!(span.bytes(usize::MAX, 2, "x"), Err(error(6)));
        assert_eq!(span.bytes(1, usize::MAX, "x"), Err(error(3)));
        let glyph = Span::new(DATA, 2, 5, Place::Glyph(GlyphId(3)));
        assert_eq!(
            glyph.u32(0, "y"),
            Err(FontError::Glyph {
                glyph: GlyphId(3),
                offset: 2,
                kind: ErrorKind::UnexpectedEnd { expected: "y" },
            })
        );
    }

    #[test]
    fn sub_ranges_stay_inside() {
        let span = Span::file(DATA);
        let sub = span.sub(2, 4, "x").expect("fits");
        assert_eq!((sub.start(), sub.end()), (2, 6));
        assert!(sub.sub(1, 4, "x").is_err());
        let rest = sub.from(4, "x").expect("the end itself");
        assert_eq!(rest.len(), 0);
        assert!(sub.from(5, "x").is_err());
        // A span made past the data is clamped to it.
        let clamped = Span::new(DATA, 20, 30, Place::Table(None));
        assert_eq!((clamped.start(), clamped.end()), (8, 8));
    }

    #[test]
    fn lookups_give_none_outside_the_data() {
        assert_eq!(be16(DATA, 6), Some(0x5678));
        assert_eq!(be16(DATA, 7), None);
        assert_eq!(be16(DATA, usize::MAX), None);
        assert_eq!(be16_signed(DATA, 2), Some(-2));
        assert_eq!(be32(DATA, 4), Some(0x1234_5678));
        assert_eq!(be32(DATA, 5), None);
    }
}
