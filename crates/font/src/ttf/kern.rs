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

//! The `kern` table: Microsoft's version 0, format 0 subtables
//! ([`Font::kerning`](super::Font::kerning) says which are read).

use super::read::{Span, be16, be32};
use super::{ErrorKind, GlyphId};
use crate::FontError;

/// The format 0 subtables that are read.
#[derive(Clone, Debug)]
pub(crate) struct KernTable {
    subtables: Vec<Pairs>,
}

/// A format 0 subtable's pairs.
#[derive(Clone, Copy, Debug)]
struct Pairs {
    /// The absolute offset of the first pair.
    at: usize,
    /// How many pairs there are.
    count: usize,
    /// The subtable's values replace those before it.
    replaces: bool,
}

// A subtable's coverage bits; its format is the high byte.
const HORIZONTAL: u16 = 0x0001;
const MINIMUM: u16 = 0x0002;
const CROSS_STREAM: u16 = 0x0004;
const OVERRIDE: u16 = 0x0008;

/// The size of a pair: the two glyphs and the value.
const PAIR: usize = 6;
/// The size of a subtable's header, and of a format 0 subtable's before its
/// pairs.
const HEADER: usize = 6;
const FORMAT_0_HEADER: usize = 14;

impl KernTable {
    /// Checks the table, keeping at most `max_subtables` of the subtables
    /// it reads (the limit `GPOS`'s subtables have,
    /// `Limits::max_kerning_subtables`); `None` for Apple's version 1.0,
    /// which is ignored.
    pub(crate) fn read(table: Span<'_>, max_subtables: usize) -> Result<Option<Self>, FontError> {
        let version = table.u16(0, "the version")?;
        if version == 1 && table.u16(2, "the version")? == 0 {
            return Ok(None);
        }
        if version != 0 {
            return Err(table.error(
                0,
                ErrorKind::BadVersion {
                    found: u32::from(version),
                },
            ));
        }
        let count = table.u16(2, "the number of subtables")?;
        let mut at = 4;
        let mut subtables = Vec::new();
        for _ in 0..count {
            let length = usize::from(table.u16(at + 2, "a subtable's length")?);
            let coverage = table.u16(at + 4, "a subtable's coverage")?;
            if coverage >> 8 != 0 {
                if length < HEADER {
                    return Err(table.error(
                        at + 2,
                        ErrorKind::BadLength {
                            what: "a kern subtable",
                        },
                    ));
                }
                table.bytes(at, length, "a subtable, as its length says")?;
                at += length;
                continue;
            }
            let count = usize::from(table.u16(at + HEADER, "nPairs")?);
            let pairs = table.sub(at + FORMAT_0_HEADER, count * PAIR, "a subtable's pairs")?;
            let used = coverage & HORIZONTAL != 0 && coverage & (MINIMUM | CROSS_STREAM) == 0;
            if used {
                if subtables.len() >= max_subtables {
                    return Err(table.error(
                        at,
                        ErrorKind::TooManyKerningSubtables {
                            limit: max_subtables,
                        },
                    ));
                }
                check_sorted(pairs, count)?;
                subtables.push(Pairs {
                    at: pairs.start(),
                    count,
                    replaces: coverage & OVERRIDE != 0,
                });
            }
            at += FORMAT_0_HEADER + count * PAIR;
        }
        subtables.shrink_to_fit();
        Ok(Some(Self { subtables }))
    }

    /// The kerning of the pair, in font units.
    pub(crate) fn kerning(&self, data: &[u8], left: GlyphId, right: GlyphId) -> i32 {
        let key = u32::from(left.0) << 16 | u32::from(right.0);
        let mut total = 0;
        for pairs in &self.subtables {
            if let Some(value) = find(data, pairs, key) {
                total = if pairs.replaces { value } else { total + value };
            }
        }
        total
    }
}

/// Checks that the pairs are in ascending order of their two glyphs.
fn check_sorted(pairs: Span<'_>, count: usize) -> Result<(), FontError> {
    let mut previous = None;
    for index in 0..count {
        let at = index * PAIR;
        let key = pairs.u32(at, "a pair's glyphs")?;
        if previous.is_some_and(|previous| key <= previous) {
            return Err(pairs.error(
                at,
                ErrorKind::NotSorted {
                    what: "pairs of a kern subtable",
                },
            ));
        }
        previous = Some(key);
    }
    Ok(())
}

/// The value of the pair whose glyphs are `key` (left in the high half).
fn find(data: &[u8], pairs: &Pairs, key: u32) -> Option<i32> {
    let (mut low, mut high) = (0, pairs.count);
    while low < high {
        let middle = low + (high - low) / 2;
        let at = pairs.at + middle * PAIR;
        let found = be32(data, at)?;
        if found < key {
            low = middle + 1;
        } else if found > key {
            high = middle;
        } else {
            return Some(i32::from(be16(data, at + 4)? as i16));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subtable_kept_is_small() {
        // What `Limits::max_kerning_subtables` documents for `kern`.
        assert!(size_of::<Pairs>() <= 24, "{}", size_of::<Pairs>());
    }

    #[test]
    fn pairs_are_found_by_binary_search() {
        let mut data = Vec::new();
        for (left, right, value) in [(1u16, 2u16, -10i16), (1, 5, 20), (3, 0, -1)] {
            data.extend(left.to_be_bytes());
            data.extend(right.to_be_bytes());
            data.extend(value.to_be_bytes());
        }
        let pairs = Pairs {
            at: 0,
            count: 3,
            replaces: false,
        };
        assert_eq!(find(&data, &pairs, 1 << 16 | 2), Some(-10));
        assert_eq!(find(&data, &pairs, 1 << 16 | 5), Some(20));
        assert_eq!(find(&data, &pairs, 3 << 16), Some(-1));
        assert_eq!(find(&data, &pairs, 1 << 16 | 3), None);
        assert_eq!(find(&data, &pairs, 4 << 16), None);
        assert!(check_sorted(Span::file(&data), 3).is_ok());
        // The first pair's left glyph becomes 9, after the second's.
        data[1] = 9;
        assert!(check_sorted(Span::file(&data), 3).is_err());
    }
}
