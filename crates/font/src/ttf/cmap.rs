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

//! `cmap`: characters to glyphs, through one format 4 or format 12
//! subtable (the module docs of [`ttf`](super) say which).

use super::read::{Span, be16, be32};
use super::{ErrorKind, GlyphId};
use crate::FontError;

/// The subtable used, and where its arrays are (absolute offsets).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Cmap {
    /// Segments of characters in the Basic Multilingual Plane.
    Format4 {
        /// The subtable's first byte.
        start: usize,
        /// One past its last byte, as its length says.
        end: usize,
        /// How many segments it has.
        segments: usize,
    },
    /// Groups of characters in any plane.
    Format12 {
        /// The first group.
        groups: usize,
        /// How many groups there are.
        count: usize,
    },
}

/// How a subtable ranks, best first; `None` for one that is not used.
fn rank(platform: u16, encoding: u16, format: u16) -> Option<u8> {
    let unicode = platform == 0;
    match format {
        12 if unicode || (platform == 3 && encoding == 10) => Some(0),
        4 if unicode || (platform == 3 && encoding == 1) => Some(1),
        _ => None,
    }
}

/// The size of a format 12 group.
const GROUP: usize = 12;

impl Cmap {
    /// Picks the subtable and checks what its lookups rely on.
    pub(crate) fn read(table: Span<'_>) -> Result<Self, FontError> {
        let version = table.u16(0, "the version")?;
        if version != 0 {
            return Err(table.error(
                0,
                ErrorKind::BadVersion {
                    found: u32::from(version),
                },
            ));
        }
        let count = usize::from(table.u16(2, "the number of encoding records")?);
        table.bytes(4, count * 8, "the encoding records")?;
        let mut best: Option<(u8, usize)> = None;
        for index in 0..count {
            let at = 4 + index * 8;
            let platform = table.u16(at, "a platform ID")?;
            let encoding = table.u16(at + 2, "an encoding ID")?;
            if platform != 0 && !(platform == 3 && matches!(encoding, 1 | 10)) {
                continue;
            }
            let offset =
                usize::try_from(table.u32(at + 4, "a subtable's offset")?).unwrap_or(usize::MAX);
            let format = table.u16(offset, "a subtable's format")?;
            if let Some(rank) = rank(platform, encoding, format)
                && best.is_none_or(|(best, _)| rank < best)
            {
                best = Some((rank, offset));
            }
        }
        match best {
            Some((0, offset)) => format12(table.from(offset, "a subtable")?),
            Some((_, offset)) => format4(table.from(offset, "a subtable")?),
            None => Err(table.error(0, ErrorKind::NoUnicodeCmap)),
        }
    }

    /// The glyph `c` maps to; `None` for none, glyph 0, or a glyph id that
    /// is not below `glyph_count`.
    pub(crate) fn lookup(&self, data: &[u8], c: u32, glyph_count: u16) -> Option<GlyphId> {
        let glyph = match *self {
            Self::Format4 {
                start,
                end,
                segments,
            } => lookup4(data, start, end, segments, c)?,
            Self::Format12 { groups, count } => lookup12(data, groups, count, c)?,
        };
        let glyph = u16::try_from(glyph).ok()?;
        (glyph != 0 && glyph < glyph_count).then_some(GlyphId(glyph))
    }
}

/// Checks a format 4 subtable, which starts `rest`.
fn format4(rest: Span<'_>) -> Result<Cmap, FontError> {
    let length = usize::from(rest.u16(2, "the subtable's length")?);
    let subtable = rest.sub(0, length, "the format 4 subtable, as its length says")?;
    let seg_x2 = subtable.u16(6, "segCountX2")?;
    if seg_x2 % 2 != 0 {
        return Err(subtable.error(6, ErrorKind::OddSegCount { found: seg_x2 }));
    }
    let segments = usize::from(seg_x2 / 2);
    // The header, the end codes, a pad, then the start codes, deltas and
    // range offsets: 16 bytes and 8 a segment, under 2^20.
    if length < 16 + 8 * segments {
        return Err(subtable.error(
            2,
            ErrorKind::BadLength {
                what: "a format 4 subtable",
            },
        ));
    }
    let mut previous = None;
    for index in 0..segments {
        let at = 14 + index * 2;
        let end = subtable.u16(at, "a segment's end code")?;
        if previous.is_some_and(|previous| end <= previous) {
            return Err(subtable.error(
                at,
                ErrorKind::NotSorted {
                    what: "segments' end codes",
                },
            ));
        }
        previous = Some(end);
    }
    Ok(Cmap::Format4 {
        start: subtable.start(),
        end: subtable.end(),
        segments,
    })
}

/// Checks a format 12 subtable, which starts `rest`.
fn format12(rest: Span<'_>) -> Result<Cmap, FontError> {
    let length = usize::try_from(rest.u32(4, "the subtable's length")?).unwrap_or(usize::MAX);
    let subtable = rest.sub(0, length, "the format 12 subtable, as its length says")?;
    let count = usize::try_from(subtable.u32(12, "numGroups")?).unwrap_or(usize::MAX);
    let fits = count
        .checked_mul(GROUP)
        .and_then(|bytes| bytes.checked_add(16))
        .is_some_and(|needed| needed <= length);
    if !fits {
        return Err(subtable.error(
            4,
            ErrorKind::BadLength {
                what: "a format 12 subtable",
            },
        ));
    }
    let mut previous_end: Option<u32> = None;
    for index in 0..count {
        let at = 16 + index * GROUP;
        let first = subtable.u32(at, "a group's first character")?;
        let last = subtable.u32(at + 4, "a group's last character")?;
        if last < first {
            return Err(subtable.error(
                at,
                ErrorKind::BackwardRange {
                    what: "a format 12 subtable",
                },
            ));
        }
        if previous_end.is_some_and(|previous| first <= previous) {
            return Err(subtable.error(
                at,
                ErrorKind::NotSorted {
                    what: "groups of a format 12 subtable",
                },
            ));
        }
        previous_end = Some(last);
    }
    Ok(Cmap::Format12 {
        groups: subtable.start() + 16,
        count,
    })
}

/// Looks `c` up in a format 4 subtable at `start..end` with `segments`
/// segments, which [`format4`] checked.
fn lookup4(data: &[u8], start: usize, end: usize, segments: usize, c: u32) -> Option<u32> {
    let c = u16::try_from(c).ok()?;
    let ends = start + 14;
    // The first segment whose end code is at least `c`.
    let (mut low, mut high) = (0, segments);
    while low < high {
        let middle = low + (high - low) / 2;
        if be16(data, ends + middle * 2)? < c {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low == segments {
        return None;
    }
    let starts = ends + segments * 2 + 2;
    let deltas = starts + segments * 2;
    let range_offsets = deltas + segments * 2;
    let first = be16(data, starts + low * 2)?;
    if c < first {
        return None;
    }
    let delta = be16(data, deltas + low * 2)?;
    let range_at = range_offsets + low * 2;
    let range_offset = be16(data, range_at)?;
    if range_offset == 0 {
        return Some(u32::from(c.wrapping_add(delta)));
    }
    // The glyph is in the array the range offset points into, counted from
    // the range offset's own place.
    let at = range_at + usize::from(range_offset) + usize::from(c - first) * 2;
    if at + 2 > end {
        return None;
    }
    match be16(data, at)? {
        0 => None,
        glyph => Some(u32::from(glyph.wrapping_add(delta))),
    }
}

/// Looks `c` up in `count` format 12 groups at `groups`, which
/// [`format12`] checked.
fn lookup12(data: &[u8], groups: usize, count: usize, c: u32) -> Option<u32> {
    // The first group whose last character is at least `c`.
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if be32(data, groups + middle * GROUP + 4)? < c {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low == count {
        return None;
    }
    let at = groups + low * GROUP;
    let first = be32(data, at)?;
    if c < first {
        return None;
    }
    be32(data, at + 8)?.checked_add(c - first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_12_ranks_above_format_4_and_others_are_not_used() {
        assert_eq!(rank(3, 10, 12), Some(0));
        assert_eq!(rank(0, 4, 12), Some(0));
        assert_eq!(rank(3, 1, 4), Some(1));
        assert_eq!(rank(0, 3, 4), Some(1));
        assert_eq!(rank(3, 1, 12), None);
        assert_eq!(rank(3, 10, 4), None);
        assert_eq!(rank(3, 0, 4), None);
        assert_eq!(rank(0, 5, 14), None);
        assert_eq!(rank(1, 0, 4), None);
    }

    #[test]
    fn format_12_groups_are_searched() {
        // Two groups: 0x41..=0x43 from glyph 5, 0x1F600..=0x1F600 at 9.
        let mut data = Vec::new();
        for (first, last, glyph) in [(0x41u32, 0x43u32, 5u32), (0x1F600, 0x1F600, 9)] {
            data.extend(first.to_be_bytes());
            data.extend(last.to_be_bytes());
            data.extend(glyph.to_be_bytes());
        }
        assert_eq!(lookup12(&data, 0, 2, 0x40), None);
        assert_eq!(lookup12(&data, 0, 2, 0x41), Some(5));
        assert_eq!(lookup12(&data, 0, 2, 0x43), Some(7));
        assert_eq!(lookup12(&data, 0, 2, 0x44), None);
        assert_eq!(lookup12(&data, 0, 2, 0x1F600), Some(9));
        assert_eq!(lookup12(&data, 0, 2, 0x1F601), None);
        assert_eq!(lookup12(&data, 0, 0, 0x41), None);
    }
}
