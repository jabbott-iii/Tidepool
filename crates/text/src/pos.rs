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

//! Positions: byte offsets (canonical) and their conversions to char and
//! UTF-16 offsets, to lines, and to line/column pairs counted in bytes,
//! UTF-16 units or chars (ADR-0019).
//!
//! Definitions, for a text T of n bytes:
//!
//! - **Valid bytes** are the char boundaries of T in `0..=n`, including the
//!   offset between a CR and its LF.
//! - **Line breaks** are CR LF (one break of 2 bytes), and any other LF or
//!   CR (1 byte). VT, FF, NEL, LS and PS are ordinary characters. A text has
//!   one more line than it has breaks: the empty text is one line, and a
//!   text ending in a break ends with an empty line.
//! - **The line of a byte** is the last line starting at or before it, so the
//!   offset between a CR and its LF is on the line that CR LF ends.
//! - **The columns of a line** are the valid bytes from its start up to, not
//!   including, the next line's start (on the last line, up to and
//!   including n), each measured from the line's start in the chosen
//!   [`Unit`]. So a CR LF line's last column is its content length + 1,
//!   while LF and CR lines end at their content length.
//!
//! Every conversion is O(log n + 4 KiB): at most six descents, each with a
//! scan of at most one leaf, whatever the line length. The conversions
//! between bytes and offsets or lines take at most one descent, `line_span`
//! two and `byte_to_line_col` three; `line_col_to_byte` takes at most two,
//! or on a line that leaves its first leaf at most six (four for
//! [`Unit::Byte`]).
//! Conversions between valid positions are exact inverses; an invalid input
//! gives its documented error.

use std::ops::Range;

use crate::TextError;
use crate::rope::{Hit, Metric, Rope, breaks_before};

pub mod grapheme;

#[cfg(test)]
pub(crate) mod tests;

/// What an offset or column counts. These are LSP's position encodings:
/// `Byte` is "utf-8", `Utf16` is "utf-16" and `Char` is "utf-32".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    /// UTF-8 bytes.
    Byte,
    /// UTF-16 code units: one per char, two for a char above U+FFFF.
    Utf16,
    /// Chars (Unicode scalar values).
    Char,
}

/// A line and a column, both counted from 0. The column's [`Unit`] is given
/// to each conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineCol {
    /// The line, from 0.
    pub line: usize,
    /// The column, from 0.
    pub col: usize,
}

impl LineCol {
    /// The position at `line` and `col`.
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// A line break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineBreak {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
    /// A `\r` not followed by `\n`.
    Cr,
}

impl LineBreak {
    /// The break's length in bytes (also in chars and UTF-16 units).
    pub const fn byte_len(self) -> usize {
        match self {
            Self::Lf | Self::Cr => 1,
            Self::CrLf => 2,
        }
    }

    /// The break's text.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// One line of a text, in bytes: its content is `start..content_end`, and
/// its break, if any, follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineSpan {
    /// The byte offset of the line's first byte.
    pub start: usize,
    /// The byte offset just past the line's content, where its break starts.
    pub content_end: usize,
    /// The break ending the line; `None` only on the last line.
    pub line_break: Option<LineBreak>,
}

impl LineSpan {
    /// The byte offset just past the line's break: the next line's start, or
    /// the end of the text on the last line. Saturates at `usize::MAX`, so
    /// a hand-built span cannot overflow it.
    pub const fn end(&self) -> usize {
        match self.line_break {
            Some(line_break) => self.content_end.saturating_add(line_break.byte_len()),
            None => self.content_end,
        }
    }

    /// The line's content, without its break.
    pub const fn content(&self) -> Range<usize> {
        self.start..self.content_end
    }

    /// The line with its break.
    pub const fn full(&self) -> Range<usize> {
        self.start..self.end()
    }
}

impl Rope {
    /// The length of the text in `unit`. O(1).
    pub fn len_in(&self, unit: Unit) -> usize {
        let summary = self.summary();
        match unit {
            Unit::Byte => summary.bytes,
            Unit::Utf16 => summary.utf16,
            Unit::Char => summary.chars,
        }
    }

    /// The number of lines: line breaks + 1, so at least 1. O(1).
    pub fn len_lines(&self) -> usize {
        self.summary().breaks + 1
    }

    /// The offset in `unit` of the valid byte `byte`: the length in `unit`
    /// of the text before it.
    ///
    /// # Errors
    ///
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) if `byte > len`,
    /// then [`NotCharBoundary`](TextError::NotCharBoundary).
    pub fn byte_to_offset(&self, byte: usize, unit: Unit) -> Result<usize, TextError> {
        let hit = self.valid_hit(byte)?;
        Ok(offset_in(&hit, byte, unit))
    }

    /// The valid byte whose offset in `unit` is `offset`; the inverse of
    /// [`byte_to_offset`](Rope::byte_to_offset).
    ///
    /// # Errors
    ///
    /// For [`Unit::Byte`], as for `byte_to_offset`. Otherwise
    /// [`OffsetOutOfBounds`](TextError::OffsetOutOfBounds) if `offset` is
    /// past [`len_in(unit)`](Rope::len_in), then
    /// [`OffsetInsideChar`](TextError::OffsetInsideChar) for a UTF-16
    /// offset between the halves of a surrogate pair.
    pub fn offset_to_byte(&self, offset: usize, unit: Unit) -> Result<usize, TextError> {
        let metric = match unit {
            Unit::Byte => return self.check_byte(offset).map(|()| offset),
            Unit::Utf16 => Metric::Utf16,
            Unit::Char => Metric::Chars,
        };
        let len = self.len_in(unit);
        if offset > len {
            return Err(TextError::OffsetOutOfBounds { offset, unit, len });
        }
        let hit = self.find(metric, offset);
        let wanted = offset - hit.before.get(metric);
        match byte_at_units(hit.leaf.as_bytes(), hit.summary.get(metric), wanted, unit) {
            Some(local) => Ok(hit.start + local),
            None => Err(TextError::OffsetInsideChar { offset, unit }),
        }
    }

    /// The line of the valid byte `byte`: the last line that starts at or
    /// before it.
    ///
    /// # Errors
    ///
    /// As for [`byte_to_offset`](Rope::byte_to_offset).
    pub fn byte_to_line(&self, byte: usize) -> Result<usize, TextError> {
        let hit = self.valid_hit(byte)?;
        Ok(hit.before.breaks + breaks_before(hit.leaf, hit.summary.breaks, byte - hit.start))
    }

    /// The byte offset where `line` starts: 0 for line 0, otherwise just
    /// past the `line`-th break.
    ///
    /// # Errors
    ///
    /// [`LineOutOfBounds`](TextError::LineOutOfBounds) unless
    /// `line < len_lines()`.
    pub fn line_to_byte(&self, line: usize) -> Result<usize, TextError> {
        self.check_line(line)?;
        Ok(self.line_start(line))
    }

    /// The content and break of `line`.
    ///
    /// # Errors
    ///
    /// As for [`line_to_byte`](Rope::line_to_byte).
    pub fn line_span(&self, line: usize) -> Result<LineSpan, TextError> {
        self.check_line(line)?;
        let start = self.line_start(line);
        if line + 1 == self.len_lines() {
            return Ok(LineSpan {
                start,
                content_end: self.len(),
                line_break: None,
            });
        }
        let (end, line_break) = self.nth_break(line + 1);
        Ok(LineSpan {
            start,
            content_end: end - line_break.byte_len(),
            line_break: Some(line_break),
        })
    }

    /// The line and column of the valid byte `byte`, the column counted in
    /// `unit` from the line's start.
    ///
    /// # Errors
    ///
    /// As for [`byte_to_offset`](Rope::byte_to_offset).
    pub fn byte_to_line_col(&self, byte: usize, unit: Unit) -> Result<LineCol, TextError> {
        let hit = self.valid_hit(byte)?;
        let bytes = hit.leaf.as_bytes();
        let off = byte - hit.start;
        let line = hit.before.breaks + breaks_before(hit.leaf, hit.summary.breaks, off);
        // Walk back from `off` to the line's start or the leaf's start,
        // counting the column. A CR whose LF is at `off` is part of the
        // line, so the offset between them is one column past the content.
        let mut col = 0;
        let mut at = off;
        while let Some(&prev) = at.checked_sub(1).and_then(|i| bytes.get(i)) {
            if prev == b'\n' || (prev == b'\r' && bytes.get(at) != Some(&b'\n')) {
                return Ok(LineCol { line, col });
            }
            at = char_start_before(bytes, at);
            col += bytes.get(at).map_or(0, |&lead| char_units(lead, unit));
        }
        // The line starts at or before this leaf: add the part of it before
        // the leaf. At most two more descents.
        let before_leaf = match unit {
            Unit::Byte => hit.start,
            Unit::Utf16 => hit.before.utf16,
            Unit::Char => hit.before.chars,
        };
        let line_offset = match line {
            0 => 0,
            _ => self.offset_of(self.line_start(line), unit),
        };
        Ok(LineCol {
            line,
            col: col + before_leaf - line_offset,
        })
    }

    /// The byte at `pos`, its column counted in `unit`: the strict inverse
    /// of [`byte_to_line_col`](Rope::byte_to_line_col).
    ///
    /// # Errors
    ///
    /// Checked in this order:
    /// [`LineOutOfBounds`](TextError::LineOutOfBounds);
    /// [`ColumnOutOfBounds`](TextError::ColumnOutOfBounds) with the line's
    /// last column; [`ColumnInsideChar`](TextError::ColumnInsideChar).
    pub fn line_col_to_byte(&self, pos: LineCol, unit: Unit) -> Result<usize, TextError> {
        self.check_line(pos.line)?;
        let start = self.line_start(pos.line);
        match self.column_in_leaf(start, pos, unit) {
            Some(result) => result,
            None => self.column_across_leaves(start, pos, unit),
        }
    }

    /// `line_col_to_byte` by a scan from the line's start through its leaf;
    /// `None` if the leaf ends first, before the end of the text.
    fn column_in_leaf(
        &self,
        start: usize,
        pos: LineCol,
        unit: Unit,
    ) -> Option<Result<usize, TextError>> {
        let past = |max| Err(TextError::ColumnOutOfBounds { pos, unit, max });
        if start == self.len() {
            return Some(if pos.col == 0 { Ok(start) } else { past(0) });
        }
        // The leaf holding the byte at `start`.
        let hit = self.find(Metric::Bytes, start + 1);
        let bytes = hit.leaf.as_bytes();
        let mut at = start - hit.start;
        let mut col = 0;
        loop {
            if col == pos.col {
                return Some(Ok(hit.start + at));
            }
            let Some(&byte) = bytes.get(at) else {
                // At the end of the text the line ends too.
                return (hit.start + at == self.len()).then(|| past(col));
            };
            match byte {
                b'\n' => return Some(past(col)),
                // A CR at the end of a leaf is a lone CR (invariant 4).
                b'\r' if bytes.get(at + 1) == Some(&b'\n') => {
                    return Some(if pos.col == col + 1 {
                        Ok(hit.start + at + 1)
                    } else {
                        past(col + 1)
                    });
                }
                b'\r' => return Some(past(col)),
                lead => {
                    let width = char_units(lead, unit);
                    if col + width > pos.col {
                        return Some(Err(TextError::ColumnInsideChar { pos, unit }));
                    }
                    col += width;
                    at += char_units(lead, Unit::Byte);
                }
            }
        }
    }

    /// `line_col_to_byte` for a line that does not end in its first leaf.
    fn column_across_leaves(
        &self,
        start: usize,
        pos: LineCol,
        unit: Unit,
    ) -> Result<usize, TextError> {
        // The last valid byte before the next line's start is just before
        // the break's last byte (an ASCII LF or CR), so it is a boundary.
        let last = if pos.line + 1 == self.len_lines() {
            self.len()
        } else {
            self.nth_break(pos.line + 1).0 - 1
        };
        let base = self.offset_of(start, unit);
        let max = self.offset_of(last, unit) - base;
        if pos.col > max {
            return Err(TextError::ColumnOutOfBounds { pos, unit, max });
        }
        match self.offset_to_byte(base + pos.col, unit) {
            Err(TextError::NotCharBoundary { .. } | TextError::OffsetInsideChar { .. }) => {
                Err(TextError::ColumnInsideChar { pos, unit })
            }
            result => result,
        }
    }

    /// `byte_to_offset` for a valid byte.
    fn offset_of(&self, byte: usize, unit: Unit) -> usize {
        match unit {
            Unit::Byte => byte,
            _ => offset_in(&self.find(Metric::Bytes, byte), byte, unit),
        }
    }

    /// The leaf holding the valid byte `byte`, found by one descent that
    /// also checks it.
    fn valid_hit(&self, byte: usize) -> Result<Hit<'_>, TextError> {
        if byte > self.len() {
            return Err(TextError::ByteOutOfBounds {
                byte,
                len: self.len(),
            });
        }
        let hit = self.find(Metric::Bytes, byte);
        if hit.leaf.is_char_boundary(byte - hit.start) {
            Ok(hit)
        } else {
            Err(TextError::NotCharBoundary { byte })
        }
    }

    fn check_line(&self, line: usize) -> Result<(), TextError> {
        let len_lines = self.len_lines();
        if line < len_lines {
            Ok(())
        } else {
            Err(TextError::LineOutOfBounds { line, len_lines })
        }
    }

    /// `line_to_byte` for a valid line.
    fn line_start(&self, line: usize) -> usize {
        if line == 0 { 0 } else { self.nth_break(line).0 }
    }

    /// The end and kind of break number `k`, counted from 1, for
    /// `1 <= k <= breaks`. A break is never split between leaves, so it is
    /// in the leaf the descent finds; that leaf is scanned from its nearer
    /// end.
    fn nth_break(&self, k: usize) -> (usize, LineBreak) {
        let hit = self.find(Metric::Breaks, k);
        let wanted = k - hit.before.breaks;
        let total = hit.summary.breaks;
        let bytes = hit.leaf.as_bytes();
        let found = if wanted > total / 2 {
            nth_break_from_end(bytes, total + 1 - wanted.min(total))
        } else {
            nth_break_from_start(bytes, wanted)
        };
        match found {
            Some((end, kind)) => (hit.start + end, kind),
            // Only a broken tree gets here.
            None => (self.len(), LineBreak::Lf),
        }
    }
}

/// The offset in `unit` of `byte`, a char boundary in the leaf of `hit`.
/// Counts from the nearer end of the leaf.
fn offset_in(hit: &Hit<'_>, byte: usize, unit: Unit) -> usize {
    let metric = match unit {
        Unit::Byte => return byte,
        Unit::Utf16 => Metric::Utf16,
        Unit::Char => Metric::Chars,
    };
    let bytes = hit.leaf.as_bytes();
    let off = byte - hit.start;
    let local = if off > bytes.len() / 2 {
        let tail = count_units(bytes.get(off..).unwrap_or_default(), unit);
        hit.summary.get(metric).saturating_sub(tail)
    } else {
        count_units(bytes.get(..off).unwrap_or(bytes), unit)
    };
    hit.before.get(metric) + local
}

/// The end (local) and kind of the `wanted`-th break of `bytes`, from 1.
fn nth_break_from_start(bytes: &[u8], wanted: usize) -> Option<(usize, LineBreak)> {
    let mut seen = 0;
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let kind = match byte {
            b'\n' => Some(LineBreak::Lf),
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                index += 1;
                Some(LineBreak::CrLf)
            }
            b'\r' => Some(LineBreak::Cr),
            _ => None,
        };
        index += 1;
        if let Some(kind) = kind {
            seen += 1;
            if seen == wanted {
                return Some((index, kind));
            }
        }
    }
    None
}

/// The end (local) and kind of the `wanted`-th break of `bytes` counted
/// from the end, from 1. A CR met going backwards is lone, because a CR
/// LF is taken whole at its LF.
fn nth_break_from_end(bytes: &[u8], wanted: usize) -> Option<(usize, LineBreak)> {
    let mut seen = 0;
    let mut index = bytes.len();
    while let Some(next) = index.checked_sub(1) {
        index = next;
        let end = index + 1;
        let kind = match bytes.get(index) {
            Some(b'\n') if index > 0 && bytes.get(index - 1) == Some(&b'\r') => {
                index -= 1;
                Some(LineBreak::CrLf)
            }
            Some(b'\n') => Some(LineBreak::Lf),
            Some(b'\r') => Some(LineBreak::Cr),
            _ => None,
        };
        if let Some(kind) = kind {
            seen += 1;
            if seen == wanted {
                return Some((end, kind));
            }
        }
    }
    None
}

/// The local byte of `bytes` (a leaf holding `total` of `unit`) at which
/// `wanted` of `unit` have passed, or `None` if that is inside a char.
/// Scans from the nearer end.
fn byte_at_units(bytes: &[u8], total: usize, wanted: usize, unit: Unit) -> Option<usize> {
    if wanted > total / 2 {
        let goal = total.checked_sub(wanted)?;
        let mut units = 0;
        let mut at = bytes.len();
        while units < goal {
            if at == 0 {
                return None;
            }
            at = char_start_before(bytes, at);
            units += bytes.get(at).map_or(1, |&lead| char_units(lead, unit));
        }
        return (units == goal).then_some(at);
    }
    let mut units = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if is_continuation(byte) {
            continue;
        }
        if units == wanted {
            return Some(index);
        }
        if units > wanted {
            return None;
        }
        units += char_units(byte, unit);
    }
    (units == wanted).then_some(bytes.len())
}

/// The start of the char that ends at `at` (`at > 0`, a char boundary).
fn char_start_before(bytes: &[u8], at: usize) -> usize {
    let mut start = at.saturating_sub(1);
    while start > 0 && bytes.get(start).is_some_and(|&byte| is_continuation(byte)) {
        start -= 1;
    }
    start
}

/// The length in `unit` of the chars of `bytes`.
fn count_units(bytes: &[u8], unit: Unit) -> usize {
    let mut units = 0;
    for &byte in bytes {
        if !is_continuation(byte) {
            units += char_units(byte, unit);
        }
    }
    units
}

/// Whether `byte` continues a UTF-8 sequence rather than starting a char.
fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

/// The length in `unit` of the char whose UTF-8 lead byte is `lead`.
fn char_units(lead: u8, unit: Unit) -> usize {
    match unit {
        Unit::Byte => match lead {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        },
        Unit::Utf16 => 1 + usize::from(lead >= 0xF0),
        Unit::Char => 1,
    }
}
