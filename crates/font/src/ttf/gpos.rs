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

//! The `kern` feature of `GPOS`: pair adjustment lookups
//! ([`Font::kerning`](super::Font::kerning) says what is read and checked).

use std::collections::{BTreeMap, BTreeSet};

use super::read::{Span, be16, be16_signed};
use super::{ErrorKind, GlyphId, Tag};
use crate::FontError;

/// The `kern` feature's lookups, as the subtables they hold.
#[derive(Clone, Debug)]
pub(crate) struct Gpos {
    /// Each distinct pair adjustment subtable, once.
    subtables: Vec<PairPos>,
    /// Each distinct lookup table that holds one.
    lookups: Vec<Lookup>,
}

/// A lookup table of pair adjustments.
#[derive(Clone, Debug)]
struct Lookup {
    /// Its subtables, as indices into [`Gpos::subtables`], in order and
    /// each once: a repeat could never apply, since the first would have.
    subtables: Vec<u32>,
    /// How many of the `kern` feature's lookup indices lead to this table:
    /// each applies it.
    times: i64,
}

/// A pair adjustment subtable, checked; offsets are absolute.
#[derive(Clone, Copy, Debug)]
enum PairPos {
    /// Format 1: pairs of glyphs.
    Glyphs {
        at: usize,
        coverage: Coverage,
        value1: u16,
        value2: u16,
        sets: u16,
    },
    /// Format 2: pairs of classes.
    Classes {
        at: usize,
        coverage: Coverage,
        value1: u16,
        value2: u16,
        class1: ClassDef,
        class2: ClassDef,
        class1_count: u16,
        class2_count: u16,
    },
}

/// A coverage table, checked.
#[derive(Clone, Copy, Debug)]
struct Coverage {
    at: usize,
    /// Format 2 (ranges) rather than 1 (a list of glyphs).
    ranges: bool,
    count: u16,
}

/// A class definition, checked.
#[derive(Clone, Copy, Debug)]
enum ClassDef {
    /// Format 1: a class for each of `count` glyphs from `first`.
    List { at: usize, first: u16, count: u16 },
    /// Format 2: ranges of glyphs, each with its class.
    Ranges { at: usize, count: u16 },
}

const KERN: Tag = Tag(*b"kern");
const DFLT: Tag = Tag(*b"DFLT");
const LATN: Tag = Tag(*b"latn");

/// A lookup of pair adjustments, and the extension that wraps another
/// type.
const PAIR_ADJUSTMENT: u16 = 2;
const EXTENSION: u16 = 9;

/// The bit of a value format for `XAdvance`; the two below it come first.
const X_ADVANCE: u16 = 0x0004;

/// The size of a value record of format `format`: 2 bytes for each of its
/// eight fields. The reserved bits are ignored.
fn value_size(format: u16) -> usize {
    2 * (format & 0x00ff).count_ones() as usize
}

impl Gpos {
    /// Reads and checks the `kern` feature's lookups, keeping at most
    /// `max_subtables` (lookup, subtable) entries; `None` when the language
    /// system read has no `kern` feature (or there is none).
    pub(crate) fn read(table: Span<'_>, max_subtables: usize) -> Result<Option<Self>, FontError> {
        let major = table.u16(0, "the major version")?;
        if major != 1 {
            return Err(table.error(
                0,
                ErrorKind::BadVersion {
                    found: u32::from(major),
                },
            ));
        }
        let scripts_at = usize::from(table.u16(4, "the script list's offset")?);
        let features_at = usize::from(table.u16(6, "the feature list's offset")?);
        let lookups_at = usize::from(table.u16(8, "the lookup list's offset")?);
        let mut checker = Checker {
            left: table.len(),
            length: table.len(),
            coverages: BTreeMap::new(),
            classes: BTreeMap::new(),
            start: table.start(),
            pair_sets: Vec::new(),
            positions: BTreeMap::new(),
            kept: Vec::new(),
        };
        let Some(language) = language_system(table.from(scripts_at, "the script list")?)? else {
            return Ok(None);
        };
        let features = table.from(features_at, "the feature list")?;
        let Some(indices) = kern_lookups(table.start(), language, features, &mut checker)? else {
            return Ok(None);
        };

        let lookup_list = table.from(lookups_at, "the lookup list")?;
        let count = lookup_list.u16(0, "the number of lookups")?;
        lookup_list.bytes(2, usize::from(count) * 2, "the lookups' offsets")?;
        // Where each index leads, as (lookup table's start, where the index
        // was found), both offsets into the table, sorted by start: indices
        // that lead to one table are then together, and the table is read
        // once.
        let mut starts: Vec<(u32, u32)> = Vec::with_capacity(indices.len());
        for &(index, at) in &indices {
            if index >= count {
                return Err(table.error(
                    into_table(at),
                    ErrorKind::IndexOutOfRange {
                        what: "lookup",
                        index,
                        count,
                    },
                ));
            }
            let offset = lookup_list.u16(2 + usize::from(index) * 2, "a lookup's offset")?;
            let lookup = lookup_list.from(usize::from(offset), "a lookup")?;
            starts.push((within(table.start(), lookup.start()), at));
        }
        drop(indices);
        starts.sort_unstable();
        let mut lookups: Vec<Lookup> = Vec::new();
        let limit = max_subtables;
        let mut entries = 0;
        for group in starts.chunk_by(|a, b| a.0 == b.0) {
            let Some(&(start, _)) = group.first() else {
                continue;
            };
            let lookup = table.from(into_table(start), "a lookup")?;
            let kind = lookup.u16(0, "the lookup's type")?;
            let subtable_count = usize::from(lookup.u16(4, "the number of subtables")?);
            lookup.bytes(6, subtable_count * 2, "the subtables' offsets")?;
            if kind != PAIR_ADJUSTMENT && kind != EXTENSION {
                continue;
            }
            checker.spend(lookup, 6, subtable_count * 2)?;
            let mut subtables = Vec::new();
            // The subtables met in this lookup, by start: a repeat is
            // dropped, since only the first of two can apply.
            let mut seen = BTreeSet::new();
            for number in 0..subtable_count {
                let at = 6 + number * 2;
                let offset = usize::from(lookup.u16(at, "a subtable's offset")?);
                let subtable = lookup.from(offset, "a subtable")?;
                let subtable = if kind == EXTENSION {
                    let format = subtable.u16(0, "the extension's format")?;
                    if format != 1 {
                        return Err(subtable.error(
                            0,
                            ErrorKind::BadFormat {
                                what: "an extension subtable",
                                found: format,
                            },
                        ));
                    }
                    if subtable.u16(2, "the extension's lookup type")? != PAIR_ADJUSTMENT {
                        continue;
                    }
                    let offset = subtable.u32(4, "the extension's offset")?;
                    subtable.from(
                        usize::try_from(offset).unwrap_or(usize::MAX),
                        "the extended subtable",
                    )?
                } else {
                    subtable
                };
                if !seen.insert(subtable.start()) {
                    continue;
                }
                // Before the subtable is checked or kept.
                if entries >= limit {
                    return Err(lookup.error(at, ErrorKind::TooManyKerningSubtables { limit }));
                }
                entries += 1;
                subtables.push(checker.pair_pos(subtable)?);
            }
            if !subtables.is_empty() {
                subtables.shrink_to_fit();
                lookups.push(Lookup {
                    subtables,
                    // At most 65,536 lookup indices.
                    times: i64::try_from(group.len()).unwrap_or(i64::MAX),
                });
            }
        }
        // Exactly what is kept, so that it takes the bytes documented.
        let mut subtables = checker.kept;
        subtables.shrink_to_fit();
        lookups.shrink_to_fit();
        Ok(Some(Self { subtables, lookups }))
    }

    /// The kerning of the pair, in font units: the sum over the lookups of
    /// the value of the first subtable of each that applies, as many times
    /// as lookup indices lead to it. At most `max_kerning_subtables`
    /// subtables are tried, each a few binary searches.
    pub(crate) fn kerning(&self, data: &[u8], left: GlyphId, right: GlyphId) -> i32 {
        let mut total: i64 = 0;
        for lookup in &self.lookups {
            let value = lookup.subtables.iter().find_map(|&position| {
                let subtable = self.subtables.get(usize::try_from(position).ok()?)?;
                subtable.apply(data, left, right)
            });
            if let Some(value) = value {
                total += i64::from(value) * lookup.times;
            }
        }
        // At most 65,535 lookup indices of 16-bit values: within `i32`.
        i32::try_from(total).unwrap_or(if total < 0 { i32::MIN } else { i32::MAX })
    }
}

/// The language system read: the default one of `latn`, else of `DFLT`,
/// else of the first script; `None` if none of them has one. Text in Latin
/// script uses `latn`'s when the font has it, as the OpenType
/// specification says (`DFLT` is for scripts the font has no table for).
fn language_system(scripts: Span<'_>) -> Result<Option<Span<'_>>, FontError> {
    let count = usize::from(scripts.u16(0, "the number of scripts")?);
    scripts.bytes(2, count * 6, "the script records")?;
    let mut tags = Vec::with_capacity(count);
    for index in 0..count {
        tags.push(scripts.tag(2 + index * 6, "a script's tag")?);
    }
    let position = |tag: Tag| tags.iter().position(|&found| found == tag);
    let first = (count > 0).then_some(0);
    for index in [position(LATN), position(DFLT), first]
        .into_iter()
        .flatten()
    {
        let offset = scripts.u16(2 + index * 6 + 4, "a script's offset")?;
        let script = scripts.from(usize::from(offset), "a script")?;
        let default = script.u16(0, "the default language system's offset")?;
        if default != 0 {
            return Ok(Some(
                script.from(usize::from(default), "a language system")?,
            ));
        }
    }
    Ok(None)
}

/// The offset of absolute position `at` from the start of the table that
/// starts at `table`, as a `u32`. What is kept this way always fits,
/// wherever the table is in the file: a lookup starts at most 65,535 +
/// 65,535 bytes into `GPOS`, and a feature's lookup index at most about
/// 262,000 (two 16-bit offsets, then an index of 65,535). A position that
/// did not fit would be kept as `u32::MAX`, which reads past the table's
/// end, and so is reported there.
fn within(table: usize, at: usize) -> u32 {
    u32::try_from(at.saturating_sub(table)).unwrap_or(u32::MAX)
}

/// An offset that [`within`] made, as an index into the table.
fn into_table(offset: u32) -> usize {
    usize::try_from(offset).unwrap_or(usize::MAX)
}

/// A set of 16-bit values, a bit each: 8 KiB.
struct Bits(Vec<u64>);

impl Bits {
    fn new() -> Self {
        Self(vec![0; 1024])
    }

    /// Adds `value`: whether it was not there before.
    fn insert(&mut self, value: u16) -> bool {
        let (word, bit) = (usize::from(value / 64), value % 64);
        let Some(word) = self.0.get_mut(word) else {
            return false;
        };
        let new = *word & (1 << bit) == 0;
        *word |= 1 << bit;
        new
    }
}

/// The lookup indices of the `kern` features `language` names, each once,
/// with where it was first found, as an offset into `GPOS` (which starts
/// at `table`), in ascending order of index; `None` if it names no `kern`
/// feature. At most 65,536, 8 bytes each.
///
/// Each feature table is read once, however many indices name it or
/// records lead to it, and the language system's indices and each
/// feature's lookup indices are charged to `checker`'s budget: so a
/// language system that names one feature many times, or many records of
/// one feature, costs no more than the bytes it takes.
fn kern_lookups(
    table: usize,
    language: Span<'_>,
    features: Span<'_>,
    checker: &mut Checker,
) -> Result<Option<Vec<(u16, u32)>>, FontError> {
    let feature_count = features.u16(0, "the number of features")?;
    features.bytes(2, usize::from(feature_count) * 6, "the feature records")?;
    let required = language.u16(2, "the required feature's index")?;
    let count = usize::from(language.u16(4, "the number of features")?);
    language.bytes(6, count * 2, "the feature indices")?;
    checker.spend(language, 6, count * 2)?;
    let mut kern = false;
    let mut indices = Vec::new();
    let mut named = Bits::new();
    let mut expanded = Bits::new();
    // The required feature first, then the others.
    for number in 0..=count {
        let (index, at) = if number == 0 {
            if required == 0xffff {
                continue;
            }
            (required, 2)
        } else {
            let at = 6 + (number - 1) * 2;
            (language.u16(at, "a feature index")?, at)
        };
        if index >= feature_count {
            return Err(language.error(
                at,
                ErrorKind::IndexOutOfRange {
                    what: "feature",
                    index,
                    count: feature_count,
                },
            ));
        }
        let record = 2 + usize::from(index) * 6;
        if features.tag(record, "a feature's tag")? != KERN {
            continue;
        }
        kern = true;
        let offset = features.u16(record + 4, "a feature's offset")?;
        let feature = features.from(usize::from(offset), "a feature")?;
        if !expanded.insert(offset) {
            continue;
        }
        let lookups = usize::from(feature.u16(2, "the number of lookups")?);
        feature.bytes(4, lookups * 2, "the lookup indices")?;
        checker.spend(feature, 4, lookups * 2)?;
        for number in 0..lookups {
            let at = 4 + number * 2;
            let lookup = feature.u16(at, "a lookup index")?;
            if named.insert(lookup) {
                indices.push((lookup, within(table, feature.absolute(at))));
            }
        }
    }
    indices.sort_unstable_by_key(|&(index, _)| index);
    Ok(kern.then_some(indices))
}

/// Checks the structures the lookups read, each once, within a budget of
/// the table's length in bytes.
struct Checker {
    /// How many more bytes the checks may read.
    left: usize,
    /// The table's length.
    length: usize,
    coverages: BTreeMap<usize, Coverage>,
    classes: BTreeMap<usize, ClassDef>,
    /// The table's start, which `pair_sets` counts from.
    start: usize,
    /// For each byte of the table, the record size a pair set starting
    /// there was checked with, or 0: a byte a byte of the table, allocated
    /// when the first pair set is checked. (A set of what was checked could
    /// hold an entry for every two bytes, several times the table.) A pair
    /// set met again with another record size, which no font has reason to
    /// do, is checked again, and charged again.
    pair_sets: Vec<u8>,
    /// Each subtable checked, by its start: its index in `kept`.
    positions: BTreeMap<usize, u32>,
    /// Each subtable checked, once.
    kept: Vec<PairPos>,
}

impl Checker {
    /// Counts `bytes` read at `at` in `span` against the budget.
    fn spend(&mut self, span: Span<'_>, at: usize, bytes: usize) -> Result<(), FontError> {
        match self.left.checked_sub(bytes) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => Err(span.error(at, ErrorKind::OverlappingStructures)),
        }
    }

    /// Checks a pair adjustment subtable, once: its index in `kept`.
    fn pair_pos(&mut self, subtable: Span<'_>) -> Result<u32, FontError> {
        if let Some(&position) = self.positions.get(&subtable.start()) {
            return Ok(position);
        }
        let format = subtable.u16(0, "the subtable's format")?;
        if format != 1 && format != 2 {
            return Err(subtable.error(
                0,
                ErrorKind::BadFormat {
                    what: "a pair adjustment subtable",
                    found: format,
                },
            ));
        }
        let coverage_at = usize::from(subtable.u16(2, "the coverage's offset")?);
        let coverage = self.coverage(subtable.from(coverage_at, "a coverage table")?)?;
        let value1 = subtable.u16(4, "the first value format")?;
        let value2 = subtable.u16(6, "the second value format")?;
        let values = value_size(value1) + value_size(value2);
        let at = subtable.start();
        let checked = if format == 1 {
            let sets = subtable.u16(8, "the number of pair sets")?;
            let offsets = usize::from(sets) * 2;
            subtable.bytes(10, offsets, "the pair sets' offsets")?;
            self.spend(subtable, 10, offsets)?;
            for number in 0..usize::from(sets) {
                let offset = subtable.u16(10 + number * 2, "a pair set's offset")?;
                let set = subtable.from(usize::from(offset), "a pair set")?;
                self.pair_set(set, 2 + values)?;
            }
            PairPos::Glyphs {
                at,
                coverage,
                value1,
                value2,
                sets,
            }
        } else {
            let class1_at = usize::from(subtable.u16(8, "the first class definition's offset")?);
            let class2_at = usize::from(subtable.u16(10, "the second class definition's offset")?);
            let class1_count = subtable.u16(12, "the number of first classes")?;
            let class2_count = subtable.u16(14, "the number of second classes")?;
            let records = usize::from(class1_count)
                .checked_mul(usize::from(class2_count))
                .and_then(|records| records.checked_mul(values))
                .unwrap_or(usize::MAX);
            subtable.bytes(16, records, "the class records")?;
            let class1 = self.class_def(subtable.from(class1_at, "a class definition")?)?;
            let class2 = self.class_def(subtable.from(class2_at, "a class definition")?)?;
            PairPos::Classes {
                at,
                coverage,
                value1,
                value2,
                class1,
                class2,
                class1_count,
                class2_count,
            }
        };
        // Fewer than the table's bytes, so within `u32`.
        let position = u32::try_from(self.kept.len()).unwrap_or(u32::MAX);
        self.kept.push(checked);
        self.positions.insert(at, position);
        Ok(position)
    }

    /// Checks a pair set of records `record` bytes long: inside the table,
    /// and sorted by their second glyph.
    fn pair_set(&mut self, set: Span<'_>, record: usize) -> Result<(), FontError> {
        // A record is at most 2 + 2 × 16 bytes.
        let size = u8::try_from(record).unwrap_or(u8::MAX);
        if self.pair_sets.is_empty() {
            self.pair_sets = vec![0; self.length];
        }
        let at = set.start().saturating_sub(self.start);
        if let Some(seen) = self.pair_sets.get_mut(at) {
            if *seen == size {
                return Ok(());
            }
            *seen = size;
        }
        let count = usize::from(set.u16(0, "the number of pairs")?);
        let bytes = count * record;
        set.bytes(2, bytes, "a pair set's records")?;
        self.spend(set, 2, bytes)?;
        let mut previous = None;
        for index in 0..count {
            let at = 2 + index * record;
            let second = set.u16(at, "a pair's second glyph")?;
            if previous.is_some_and(|previous| second <= previous) {
                return Err(set.error(
                    at,
                    ErrorKind::NotSorted {
                        what: "second glyphs of a pair set",
                    },
                ));
            }
            previous = Some(second);
        }
        Ok(())
    }

    /// Checks a coverage table.
    fn coverage(&mut self, table: Span<'_>) -> Result<Coverage, FontError> {
        if let Some(&checked) = self.coverages.get(&table.start()) {
            return Ok(checked);
        }
        let format = table.u16(0, "the coverage's format")?;
        let count = table.u16(2, "the coverage's count")?;
        let checked = match format {
            1 => {
                let bytes = usize::from(count) * 2;
                table.bytes(4, bytes, "the coverage's glyphs")?;
                self.spend(table, 4, bytes)?;
                let mut previous = None;
                for index in 0..usize::from(count) {
                    let at = 4 + index * 2;
                    let glyph = table.u16(at, "a covered glyph")?;
                    if previous.is_some_and(|previous| glyph <= previous) {
                        return Err(table.error(
                            at,
                            ErrorKind::NotSorted {
                                what: "glyphs of a coverage table",
                            },
                        ));
                    }
                    previous = Some(glyph);
                }
                Coverage {
                    at: table.start(),
                    ranges: false,
                    count,
                }
            }
            2 => {
                self.ranges(
                    table,
                    count,
                    "a coverage table",
                    "ranges of a coverage table",
                )?;
                Coverage {
                    at: table.start(),
                    ranges: true,
                    count,
                }
            }
            found => {
                return Err(table.error(
                    0,
                    ErrorKind::BadFormat {
                        what: "a coverage table",
                        found,
                    },
                ));
            }
        };
        self.coverages.insert(table.start(), checked);
        Ok(checked)
    }

    /// Checks a class definition.
    fn class_def(&mut self, table: Span<'_>) -> Result<ClassDef, FontError> {
        if let Some(&checked) = self.classes.get(&table.start()) {
            return Ok(checked);
        }
        let format = table.u16(0, "the class definition's format")?;
        let checked = match format {
            1 => {
                let first = table.u16(2, "the first glyph")?;
                let count = table.u16(4, "the number of glyphs")?;
                let bytes = usize::from(count) * 2;
                table.bytes(6, bytes, "the glyphs' classes")?;
                self.spend(table, 6, bytes)?;
                ClassDef::List {
                    at: table.start(),
                    first,
                    count,
                }
            }
            2 => {
                let count = table.u16(2, "the number of ranges")?;
                self.ranges(
                    table,
                    count,
                    "a class definition",
                    "ranges of a class definition",
                )?;
                ClassDef::Ranges {
                    at: table.start(),
                    count,
                }
            }
            found => {
                return Err(table.error(
                    0,
                    ErrorKind::BadFormat {
                        what: "a class definition",
                        found,
                    },
                ));
            }
        };
        self.classes.insert(table.start(), checked);
        Ok(checked)
    }

    /// Checks `count` ranges of 6 bytes from byte 4 of `table` (a format 2
    /// coverage table or class definition): each starts no later than it
    /// ends, and after the one before ends.
    fn ranges(
        &mut self,
        table: Span<'_>,
        count: u16,
        part_of: &'static str,
        ranges: &'static str,
    ) -> Result<(), FontError> {
        let bytes = usize::from(count) * 6;
        table.bytes(4, bytes, "the ranges")?;
        self.spend(table, 4, bytes)?;
        let mut previous = None;
        for index in 0..usize::from(count) {
            let at = 4 + index * 6;
            let first = table.u16(at, "a range's first glyph")?;
            let last = table.u16(at + 2, "a range's last glyph")?;
            if last < first {
                return Err(table.error(at, ErrorKind::BackwardRange { what: part_of }));
            }
            if previous.is_some_and(|previous| first <= previous) {
                return Err(table.error(at, ErrorKind::NotSorted { what: ranges }));
            }
            previous = Some(last);
        }
        Ok(())
    }
}

impl PairPos {
    /// The first glyph's `XAdvance` for the pair, if the subtable applies.
    fn apply(&self, data: &[u8], left: GlyphId, right: GlyphId) -> Option<i16> {
        match *self {
            Self::Glyphs {
                at,
                coverage,
                value1,
                value2,
                sets,
            } => {
                let index = coverage.index(data, left)?;
                if index >= u32::from(sets) {
                    return None;
                }
                let offset = be16(data, at + 10 + index as usize * 2)?;
                let set = at + usize::from(offset);
                let count = usize::from(be16(data, set)?);
                let record = 2 + value_size(value1) + value_size(value2);
                let (mut low, mut high) = (0, count);
                while low < high {
                    let middle = low + (high - low) / 2;
                    let pair = set + 2 + middle * record;
                    let second = be16(data, pair)?;
                    if second < right.0 {
                        low = middle + 1;
                    } else if second > right.0 {
                        high = middle;
                    } else {
                        return x_advance(data, value1, pair + 2);
                    }
                }
                None
            }
            Self::Classes {
                at,
                coverage,
                value1,
                value2,
                class1,
                class2,
                class1_count,
                class2_count,
            } => {
                coverage.index(data, left)?;
                let first = class1.class(data, left);
                let second = class2.class(data, right);
                // A second glyph in class 0 is not kerned by this subtable,
                // as HarfBuzz has it (the specification's class 0 records
                // are "no contexts"): the lookup tries its next one. A
                // first glyph in class 0, being covered, is.
                if second == 0 || first >= class1_count || second >= class2_count {
                    return None;
                }
                let index = usize::from(first) * usize::from(class2_count) + usize::from(second);
                let values = value_size(value1) + value_size(value2);
                x_advance(data, value1, at + 16 + index * values)
            }
        }
    }
}

/// The `XAdvance` of the value record at `at` of format `format`: 0 if it
/// has none.
fn x_advance(data: &[u8], format: u16, at: usize) -> Option<i16> {
    if format & X_ADVANCE == 0 {
        return Some(0);
    }
    let before = value_size(format & (X_ADVANCE - 1));
    be16_signed(data, at + before)
}

impl Coverage {
    /// `glyph`'s coverage index, if it is covered.
    fn index(&self, data: &[u8], glyph: GlyphId) -> Option<u32> {
        let count = usize::from(self.count);
        let entries = self.at + 4;
        if !self.ranges {
            let (mut low, mut high) = (0, count);
            while low < high {
                let middle = low + (high - low) / 2;
                let found = be16(data, entries + middle * 2)?;
                if found < glyph.0 {
                    low = middle + 1;
                } else if found > glyph.0 {
                    high = middle;
                } else {
                    return u32::try_from(middle).ok();
                }
            }
            return None;
        }
        let range = find_range(data, entries, count, glyph)?;
        let first = be16(data, range)?;
        let start_index = be16(data, range + 4)?;
        Some(u32::from(start_index) + u32::from(glyph.0 - first))
    }
}

impl ClassDef {
    /// `glyph`'s class: 0 for a glyph not listed.
    fn class(&self, data: &[u8], glyph: GlyphId) -> u16 {
        match *self {
            Self::List { at, first, count } => {
                let Some(index) = glyph.0.checked_sub(first).filter(|&index| index < count) else {
                    return 0;
                };
                be16(data, at + 6 + usize::from(index) * 2).unwrap_or(0)
            }
            Self::Ranges { at, count } => find_range(data, at + 4, usize::from(count), glyph)
                .and_then(|range| be16(data, range + 4))
                .unwrap_or(0),
        }
    }
}

/// The absolute offset of the range of 6 bytes (first glyph, last glyph,
/// value), of the `count` at `entries`, that holds `glyph`.
fn find_range(data: &[u8], entries: usize, count: usize, glyph: GlyphId) -> Option<usize> {
    // The first range whose last glyph is at least `glyph`.
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if be16(data, entries + middle * 6 + 2)? < glyph.0 {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let range = entries + low * 6;
    (low < count && be16(data, range)? <= glyph.0).then_some(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_kept_from_the_table_s_start_wherever_it_is() {
        // A table past 4 GiB in the file: its offsets still fit, and come
        // back as they were. (Absolute ones, kept as `u32`, would all have
        // been u32::MAX.)
        let start = 0x1_0000_0010;
        assert_eq!(within(start, start + 0x20), 0x20);
        assert_eq!(within(start, start + 131_070), 131_070);
        assert_eq!(into_table(within(start, start + 262_000)), 262_000);
        // One that does not fit reads past the table's end.
        assert_eq!(within(0, 1 << 40), u32::MAX);
        assert_eq!(within(start, 0), 0);
    }

    #[test]
    fn what_is_kept_is_small() {
        // What `Limits::max_kerning_subtables` documents: a subtable kept
        // takes at most 64 bytes, an entry of a lookup 4, and a lookup 32
        // besides.
        assert!(size_of::<PairPos>() <= 64, "{}", size_of::<PairPos>());
        assert!(size_of::<Lookup>() <= 32, "{}", size_of::<Lookup>());
    }

    #[test]
    fn value_records_take_two_bytes_a_field() {
        assert_eq!(value_size(0), 0);
        assert_eq!(value_size(0x0004), 2);
        assert_eq!(value_size(0x00ff), 16);
        // Reserved bits are ignored.
        assert_eq!(value_size(0xff04), 2);
        // XAdvance comes after XPlacement and YPlacement.
        let data = [0, 1, 0, 2, 0xff, 0xfd];
        assert_eq!(x_advance(&data, 0x0007, 0), Some(-3));
        assert_eq!(x_advance(&data, 0x0005, 0), Some(2));
        assert_eq!(x_advance(&data, 0x0004, 4), Some(-3));
        assert_eq!(x_advance(&data, 0x0003, 0), Some(0));
    }

    #[test]
    fn ranges_are_found_by_binary_search() {
        let mut data = Vec::new();
        for (first, last, value) in [(2u16, 4u16, 0u16), (10, 10, 3), (12, 20, 4)] {
            data.extend(first.to_be_bytes());
            data.extend(last.to_be_bytes());
            data.extend(value.to_be_bytes());
        }
        let find = |glyph| find_range(&data, 0, 3, GlyphId(glyph));
        assert_eq!(find(1), None);
        assert_eq!(find(2), Some(0));
        assert_eq!(find(4), Some(0));
        assert_eq!(find(5), None);
        assert_eq!(find(10), Some(6));
        assert_eq!(find(11), None);
        assert_eq!(find(20), Some(12));
        assert_eq!(find(21), None);
    }
}
