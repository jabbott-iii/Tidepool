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

//! A small font builder for the hand-made test vectors: it assembles an
//! sfnt from tables, and encodes the tables the parser reads, so that each
//! test states only what it is about. The fuzz target's seeds in
//! `fuzz/corpus/ttf/` were made with it too.
//!
//! Every table is built as the specification lays it out; tests that need
//! a malformed font build a valid one and then [`patch`] it, at offsets
//! found with [`table`].

// Each test file uses part of the builder.
#![allow(dead_code)]

/// A table: its tag and bytes.
pub(crate) type Table = ([u8; 4], Vec<u8>);

/// `value` as a `u16`: test data is that small.
pub(crate) fn u16_of(value: usize) -> u16 {
    u16::try_from(value).expect("test data fits in 16 bits")
}

/// `value` as a `u32`: test data is that small.
pub(crate) fn u32_of(value: usize) -> u32 {
    u32::try_from(value).expect("test data fits in 32 bits")
}

/// Appends a big-endian `u16`.
pub(crate) fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_be_bytes());
}

/// Appends a big-endian `i16`.
pub(crate) fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend(value.to_be_bytes());
}

/// Appends a big-endian `u32`.
pub(crate) fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}

/// Writes a big-endian `u16` at `at`.
pub(crate) fn patch(font: &mut [u8], at: usize, value: u16) {
    font[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

/// Writes a big-endian `u32` at `at`.
pub(crate) fn patch32(font: &mut [u8], at: usize, value: u32) {
    font[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// The big-endian `u16` at `at`.
pub(crate) fn read16(font: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([font[at], font[at + 1]])
}

/// The offset of table `tag` in `font`, and of its directory record.
pub(crate) fn table(font: &[u8], tag: &[u8; 4]) -> (usize, usize) {
    let count = usize::from(read16(font, 4));
    for index in 0..count {
        let record = 12 + index * 16;
        if &font[record..record + 4] == tag {
            let offset = u32::from_be_bytes(
                font[record + 8..record + 12]
                    .try_into()
                    .expect("four bytes"),
            );
            return (offset as usize, record);
        }
    }
    panic!("no table {}", String::from_utf8_lossy(tag));
}

/// An sfnt of version `version` holding `tables`, in that order, each at
/// an offset that is a multiple of 4. Checksums are left 0.
pub(crate) fn sfnt(version: [u8; 4], tables: &[Table]) -> Vec<u8> {
    let count = u16_of(tables.len());
    let mut out = Vec::new();
    out.extend(version);
    push16(&mut out, count);
    let power = if count == 0 {
        0
    } else {
        15 - count.leading_zeros() as u16
    };
    push16(&mut out, (1 << power) * 16);
    push16(&mut out, power);
    push16(&mut out, count * 16 - (1 << power) * 16);
    let mut offset = 12 + tables.len() * 16;
    for (tag, bytes) in tables {
        out.extend(tag);
        push32(&mut out, 0);
        push32(&mut out, u32_of(offset));
        push32(&mut out, u32_of(bytes.len()));
        offset += bytes.len().next_multiple_of(4);
    }
    for (_, bytes) in tables {
        out.extend(bytes);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// `head`, with the bounding box given.
pub(crate) fn head(units_per_em: u16, long_loca: bool, bbox: [i16; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push32(&mut out, 0x0001_0000); // fontRevision
    push32(&mut out, 0); // checksumAdjustment
    push32(&mut out, 0x5F0F_3CF5);
    push16(&mut out, 0x000b); // flags
    push16(&mut out, units_per_em);
    out.extend([0; 16]); // created, modified
    for value in bbox {
        push_i16(&mut out, value);
    }
    push16(&mut out, 0); // macStyle
    push16(&mut out, 8); // lowestRecPPEM
    push_i16(&mut out, 2); // fontDirectionHint
    push_i16(&mut out, i16::from(long_loca));
    push_i16(&mut out, 0); // glyphDataFormat
    assert_eq!(out.len(), 54);
    out
}

/// `maxp` version 1.0 with `glyphs` glyphs.
pub(crate) fn maxp(glyphs: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000);
    push16(&mut out, glyphs);
    out.extend([0; 26]);
    assert_eq!(out.len(), 32);
    out
}

/// `hhea` with these line metrics and `numberOfHMetrics`.
pub(crate) fn hhea(
    ascender: i16,
    descender: i16,
    line_gap: i16,
    max: u16,
    metrics: u16,
) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push_i16(&mut out, ascender);
    push_i16(&mut out, descender);
    push_i16(&mut out, line_gap);
    push16(&mut out, max);
    out.extend([0; 22]);
    push16(&mut out, metrics);
    assert_eq!(out.len(), 36);
    out
}

/// `hmtx`: the first `long` of `metrics` as full records, the rest as
/// bearings only.
pub(crate) fn hmtx(metrics: &[(u16, i16)], long: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, &(advance, bearing)) in metrics.iter().enumerate() {
        if index < long {
            push16(&mut out, advance);
        }
        push_i16(&mut out, bearing);
    }
    out
}

/// `loca` and `glyf` for these glyphs' data, each padded to an even
/// length (short offsets are halves).
pub(crate) fn loca_glyf(glyphs: &[Vec<u8>], long: bool) -> (Vec<u8>, Vec<u8>) {
    let (mut loca, mut glyf) = (Vec::new(), Vec::new());
    let entry = |loca: &mut Vec<u8>, offset: usize| {
        if long {
            push32(loca, u32_of(offset));
        } else {
            push16(loca, u16_of(offset / 2));
        }
    };
    for glyph in glyphs {
        entry(&mut loca, glyf.len());
        glyf.extend(glyph);
        glyf.resize(glyf.len().next_multiple_of(2), 0);
    }
    entry(&mut loca, glyf.len());
    (loca, glyf)
}

/// A point of a simple glyph: x, y, on the curve.
pub(crate) type Point = (i16, i16, bool);

/// A simple glyph's data with no instructions.
pub(crate) fn simple(contours: &[&[Point]]) -> Vec<u8> {
    simple_with_instructions(contours, &[])
}

/// A simple glyph's data, with each delta as short as it can be and runs
/// of equal flags repeated, so that every encoding the reader takes is
/// used.
pub(crate) fn simple_with_instructions(contours: &[&[Point]], instructions: &[u8]) -> Vec<u8> {
    let points: Vec<Point> = contours
        .iter()
        .flat_map(|contour| contour.iter().copied())
        .collect();
    let mut out = Vec::new();
    push_i16(
        &mut out,
        i16::try_from(contours.len()).expect("few contours"),
    );
    let xs = points.iter().map(|point| point.0);
    let ys = points.iter().map(|point| point.1);
    for value in [
        xs.clone().min().unwrap_or(0),
        ys.clone().min().unwrap_or(0),
        xs.max().unwrap_or(0),
        ys.max().unwrap_or(0),
    ] {
        push_i16(&mut out, value);
    }
    let mut end = 0;
    for contour in contours {
        end += contour.len();
        push16(&mut out, u16_of(end - 1));
    }
    push16(&mut out, u16_of(instructions.len()));
    out.extend(instructions);

    let (mut flags, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
    let (mut x, mut y) = (0i32, 0i32);
    for &(px, py, on_curve) in &points {
        let (dx, dy) = (i32::from(px) - x, i32::from(py) - y);
        (x, y) = (i32::from(px), i32::from(py));
        let mut flag = u8::from(on_curve);
        flag |= delta(dx, 0x02, 0x10, &mut xs);
        flag |= delta(dy, 0x04, 0x20, &mut ys);
        flags.push(flag);
    }
    let mut index = 0;
    while index < flags.len() {
        let flag = flags[index];
        let run = flags[index..]
            .iter()
            .take_while(|&&next| next == flag)
            .count()
            .min(256);
        if run > 1 {
            out.push(flag | 0x08);
            out.push(u8::try_from(run - 1).expect("runs of at most 256"));
        } else {
            out.push(flag);
        }
        index += run;
    }
    out.extend(xs);
    out.extend(ys);
    out
}

/// Encodes a delta as its flag bits, `short` and `same`, and bytes.
fn delta(value: i32, short: u8, same: u8, out: &mut Vec<u8>) -> u8 {
    if value == 0 {
        same
    } else if value.unsigned_abs() <= 255 {
        out.push(u8::try_from(value.unsigned_abs()).expect("a short delta"));
        if value > 0 { short | same } else { short }
    } else {
        push_i16(out, i16::try_from(value).expect("a delta of 16 bits"));
        0
    }
}

/// How a component is placed.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Placement {
    /// By an offset.
    Offset(i16, i16),
    /// By matching the composite's point to the component's.
    Points(u16, u16),
}

/// A component's transform, in `F2Dot14` steps of 1/16,384.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Transform {
    None,
    Scale(i16),
    XY(i16, i16),
    TwoByTwo([i16; 4]),
}

/// One component of a composite glyph.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Component {
    pub(crate) glyph: u16,
    pub(crate) placement: Placement,
    pub(crate) transform: Transform,
    /// Write the arguments as words even when they fit in bytes.
    pub(crate) words: bool,
    /// Flags to add: `SCALED_COMPONENT_OFFSET` and the like.
    pub(crate) flags: u16,
}

/// A component of `glyph` moved by `(dx, dy)`.
pub(crate) fn at(glyph: u16, dx: i16, dy: i16) -> Component {
    Component {
        glyph,
        placement: Placement::Offset(dx, dy),
        transform: Transform::None,
        words: false,
        flags: 0,
    }
}

/// A composite glyph's data; with `instructions`, the last component says
/// it has some, and they follow.
pub(crate) fn composite(components: &[Component], instructions: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    push_i16(&mut out, -1);
    out.extend([0; 8]);
    for (index, component) in components.iter().enumerate() {
        let (first, second, xy) = match component.placement {
            Placement::Offset(dx, dy) => (dx as u16, dy as u16, true),
            Placement::Points(parent, child) => (parent, child, false),
        };
        let fits = |value: u16| {
            if xy {
                i8::try_from(value as i16).is_ok()
            } else {
                value <= 255
            }
        };
        let words = component.words || !fits(first) || !fits(second);
        let mut flags = component.flags;
        if words {
            flags |= 0x0001;
        }
        if xy {
            flags |= 0x0002;
        }
        flags |= match component.transform {
            Transform::None => 0,
            Transform::Scale(_) => 0x0008,
            Transform::XY(..) => 0x0040,
            Transform::TwoByTwo(_) => 0x0080,
        };
        let last = index + 1 == components.len();
        if !last {
            flags |= 0x0020;
        }
        if last && instructions.is_some() {
            flags |= 0x0100;
        }
        push16(&mut out, flags);
        push16(&mut out, component.glyph);
        if words {
            push16(&mut out, first);
            push16(&mut out, second);
        } else {
            out.push(first as u8);
            out.push(second as u8);
        }
        let scales: Vec<i16> = match component.transform {
            Transform::None => vec![],
            Transform::Scale(scale) => vec![scale],
            Transform::XY(x, y) => vec![x, y],
            Transform::TwoByTwo(matrix) => matrix.to_vec(),
        };
        for scale in scales {
            push_i16(&mut out, scale);
        }
    }
    if let Some(instructions) = instructions {
        push16(&mut out, u16_of(instructions.len()));
        out.extend(instructions);
    }
    out
}

/// A segment of a format 4 subtable: its first and last character, and
/// either a delta (`idRangeOffset` 0) or a delta and the array entries of
/// its characters, to which the delta is added unless they are 0.
#[derive(Clone, Debug)]
pub(crate) enum Segment {
    Delta(u16, u16, i16),
    Glyphs(u16, u16, i16, Vec<u16>),
}

/// A format 4 subtable of `segments`, in order; the final `0xFFFF`
/// segment is added.
pub(crate) fn cmap4(segments: &[Segment]) -> Vec<u8> {
    let mut all = segments.to_vec();
    all.push(Segment::Delta(0xffff, 0xffff, 1));
    let count = all.len();
    let (mut ends, mut starts, mut deltas, mut ranges, mut glyphs) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (index, segment) in all.iter().enumerate() {
        match segment {
            Segment::Delta(first, last, delta) => {
                push16(&mut starts, *first);
                push16(&mut ends, *last);
                push_i16(&mut deltas, *delta);
                push16(&mut ranges, 0);
            }
            Segment::Glyphs(first, last, delta, ids) => {
                push16(&mut starts, *first);
                push16(&mut ends, *last);
                push_i16(&mut deltas, *delta);
                // From this range offset's place to the glyphs, after the
                // rest of the range offsets and the glyphs before.
                let offset = (count - index) * 2 + glyphs.len();
                push16(&mut ranges, u16_of(offset));
                for &id in ids {
                    push16(&mut glyphs, id);
                }
            }
        }
    }
    let mut out = Vec::new();
    push16(&mut out, 4);
    let length = 16 + 8 * count + glyphs.len();
    push16(&mut out, u16_of(length));
    push16(&mut out, 0); // language
    push16(&mut out, u16_of(count * 2));
    out.extend([0; 6]); // searchRange, entrySelector, rangeShift
    out.extend(ends);
    push16(&mut out, 0); // reservedPad
    out.extend(starts);
    out.extend(deltas);
    out.extend(ranges);
    out.extend(glyphs);
    assert_eq!(out.len(), length);
    out
}

/// A format 12 subtable of groups (first, last, first glyph).
pub(crate) fn cmap12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 12);
    push16(&mut out, 0);
    push32(&mut out, u32_of(16 + 12 * groups.len()));
    push32(&mut out, 0);
    push32(&mut out, u32_of(groups.len()));
    for &(first, last, glyph) in groups {
        push32(&mut out, first);
        push32(&mut out, last);
        push32(&mut out, glyph);
    }
    out
}

/// A `cmap` table of subtables, each with its platform and encoding.
pub(crate) fn cmap(subtables: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 0);
    push16(&mut out, u16_of(subtables.len()));
    let mut offset = 4 + 8 * subtables.len();
    for (platform, encoding, bytes) in subtables {
        push16(&mut out, *platform);
        push16(&mut out, *encoding);
        push32(&mut out, u32_of(offset));
        offset += bytes.len();
    }
    for (_, _, bytes) in subtables {
        out.extend(bytes);
    }
    out
}

/// A kerning pair: left glyph, right glyph, value.
pub(crate) type Pair = (u16, u16, i16);

/// A `kern` table of version 0, of format 0 subtables, each with its
/// coverage and its pairs (in the order given).
pub(crate) fn kern(subtables: &[(u16, &[Pair])]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 0);
    push16(&mut out, u16_of(subtables.len()));
    for (coverage, pairs) in subtables {
        push16(&mut out, 0);
        push16(&mut out, u16_of(14 + 6 * pairs.len()));
        push16(&mut out, *coverage);
        push16(&mut out, u16_of(pairs.len()));
        out.extend([0; 6]);
        for &(left, right, value) in *pairs {
            push16(&mut out, left);
            push16(&mut out, right);
            push_i16(&mut out, value);
        }
    }
    out
}

/// A coverage table of format 1.
pub(crate) fn coverage1(glyphs: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, u16_of(glyphs.len()));
    for &glyph in glyphs {
        push16(&mut out, glyph);
    }
    out
}

/// A table of format 2 ranges (first, last, value): a coverage table, with
/// the start coverage index as value, or a class definition, with the
/// class.
pub(crate) fn ranges(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 2);
    push16(&mut out, u16_of(ranges.len()));
    for &(first, last, value) in ranges {
        push16(&mut out, first);
        push16(&mut out, last);
        push16(&mut out, value);
    }
    out
}

/// A class definition of format 1: classes for glyphs from `first`.
pub(crate) fn class_def1(first: u16, classes: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, first);
    push16(&mut out, u16_of(classes.len()));
    for &class in classes {
        push16(&mut out, class);
    }
    out
}

/// A value record of `format` whose `XAdvance` is `x_advance`; every other
/// field holds 1,000 plus its bit's index, which the reader must skip.
pub(crate) fn value(format: u16, x_advance: i16) -> Vec<u8> {
    let mut out = Vec::new();
    for bit in 0..8 {
        if format & (1 << bit) != 0 {
            if bit == 2 {
                push_i16(&mut out, x_advance);
            } else {
                push16(&mut out, 1000 + bit);
            }
        }
    }
    out
}

/// A pair adjustment subtable of format 1: `coverage` (its bytes), and for
/// each covered glyph a pair set of (second glyph, `XAdvance`).
pub(crate) fn pair_pos1(
    coverage: &[u8],
    sets: &[&[(u16, i16)]],
    format1: u16,
    format2: u16,
) -> Vec<u8> {
    let header = 10 + 2 * sets.len();
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, u16_of(header));
    push16(&mut out, format1);
    push16(&mut out, format2);
    push16(&mut out, u16_of(sets.len()));
    let mut body = coverage.to_vec();
    let mut set_bytes = Vec::new();
    for set in sets {
        push16(&mut out, u16_of(header + body.len()));
        set_bytes.clear();
        push16(&mut set_bytes, u16_of(set.len()));
        for &(second, x_advance) in *set {
            push16(&mut set_bytes, second);
            set_bytes.extend(value(format1, x_advance));
            set_bytes.extend(value(format2, -1));
        }
        body.extend(&set_bytes);
    }
    out.extend(body);
    out
}

/// A pair adjustment subtable of format 2: `coverage` and the two class
/// definitions (their bytes), and the first glyph's `XAdvance` of each
/// pair of classes, `values[class1][class2]`.
pub(crate) fn pair_pos2(
    coverage: &[u8],
    class1: &[u8],
    class2: &[u8],
    values: &[&[i16]],
    format1: u16,
    format2: u16,
) -> Vec<u8> {
    let class1_count = values.len();
    let class2_count = values.first().map_or(0, |row| row.len());
    let mut records = Vec::new();
    for row in values {
        assert_eq!(row.len(), class2_count);
        for &x_advance in *row {
            records.extend(value(format1, x_advance));
            records.extend(value(format2, -1));
        }
    }
    let header = 16 + records.len();
    let mut out = Vec::new();
    push16(&mut out, 2);
    push16(&mut out, u16_of(header));
    push16(&mut out, format1);
    push16(&mut out, format2);
    push16(&mut out, u16_of(header + coverage.len()));
    push16(&mut out, u16_of(header + coverage.len() + class1.len()));
    push16(&mut out, u16_of(class1_count));
    push16(&mut out, u16_of(class2_count));
    out.extend(records);
    out.extend(coverage);
    out.extend(class1);
    out.extend(class2);
    out
}

/// An extension subtable (format 1) wrapping a subtable of lookup type
/// `kind`.
pub(crate) fn extension(kind: u16, subtable: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, kind);
    push32(&mut out, 8);
    out.extend(subtable);
    out
}

/// A script of a `GPOS` table: its tag, the feature indices of its default
/// language system (`None` for none), and its required feature (`0xFFFF`
/// for none).
pub(crate) struct Script<'a> {
    pub(crate) tag: [u8; 4],
    pub(crate) features: Option<&'a [u16]>,
    pub(crate) required: u16,
}

/// A script whose default language system uses `features`.
pub(crate) fn script<'a>(tag: &[u8; 4], features: &'a [u16]) -> Script<'a> {
    Script {
        tag: *tag,
        features: Some(features),
        required: 0xffff,
    }
}

/// A `GPOS` table version 1.0 of `scripts`, `features` (tag and lookup
/// indices) and `lookups` (type and subtables' bytes).
pub(crate) fn gpos(
    scripts: &[Script<'_>],
    features: &[([u8; 4], &[u16])],
    lookups: &[(u16, Vec<Vec<u8>>)],
) -> Vec<u8> {
    gpos_raw(scripts, features, &lookup_list(lookups))
}

/// A lookup list of `lookups` (type and subtables' bytes).
pub(crate) fn lookup_list(lookups: &[(u16, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut list = Vec::new();
    push16(&mut list, u16_of(lookups.len()));
    let mut body = Vec::new();
    let offsets = 2 + 2 * lookups.len();
    for (kind, subtables) in lookups {
        push16(&mut list, u16_of(offsets + body.len()));
        let mut lookup = Vec::new();
        push16(&mut lookup, *kind);
        push16(&mut lookup, 0); // flag
        push16(&mut lookup, u16_of(subtables.len()));
        let mut offset = 6 + 2 * subtables.len();
        for subtable in subtables {
            push16(&mut lookup, u16_of(offset));
            offset += subtable.len();
        }
        for subtable in subtables {
            lookup.extend(subtable);
        }
        body.extend(lookup);
    }
    list.extend(body);
    list
}

/// A `GPOS` table version 1.0 of `scripts`, `features` and the lookup list
/// `lookup_list` as it is.
pub(crate) fn gpos_raw(
    scripts: &[Script<'_>],
    features: &[([u8; 4], &[u16])],
    lookup_list: &[u8],
) -> Vec<u8> {
    // The script list.
    let mut list = Vec::new();
    push16(&mut list, u16_of(scripts.len()));
    let mut body = Vec::new();
    let records = 2 + 6 * scripts.len();
    for script in scripts {
        list.extend(script.tag);
        push16(&mut list, u16_of(records + body.len()));
        match script.features {
            Some(indices) => {
                push16(&mut body, 4); // the language system follows
                push16(&mut body, 0); // no other language systems
                push16(&mut body, 0); // lookupOrderOffset
                push16(&mut body, script.required);
                push16(&mut body, u16_of(indices.len()));
                for &index in indices {
                    push16(&mut body, index);
                }
            }
            None => {
                push16(&mut body, 0);
                push16(&mut body, 0);
            }
        }
    }
    list.extend(body);
    let script_list = list;

    // The feature list.
    let mut list = Vec::new();
    push16(&mut list, u16_of(features.len()));
    let mut body = Vec::new();
    let records = 2 + 6 * features.len();
    for (tag, indices) in features {
        list.extend(tag);
        push16(&mut list, u16_of(records + body.len()));
        push16(&mut body, 0); // featureParamsOffset
        push16(&mut body, u16_of(indices.len()));
        for &index in *indices {
            push16(&mut body, index);
        }
    }
    list.extend(body);
    let feature_list = list;

    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push16(&mut out, 10);
    push16(&mut out, u16_of(10 + script_list.len()));
    push16(
        &mut out,
        u16_of(10 + script_list.len() + feature_list.len()),
    );
    out.extend(script_list);
    out.extend(feature_list);
    out.extend(lookup_list);
    out
}

/// A lookup list of one type 2 lookup of `count` distinct format 1
/// subtables that share one coverage table (glyph 1) and one pair set
/// (glyph 3 at -4).
pub(crate) fn distinct_subtables(count: usize) -> Vec<u8> {
    let first = 6 + 2 * count;
    let shared = first + 12 * count;
    let mut list = Vec::new();
    for value in [1, 4, 2, 0, u16_of(count)] {
        push16(&mut list, value);
    }
    for index in 0..count {
        push16(&mut list, u16_of(first + 12 * index));
    }
    for index in 0..count {
        let at = first + 12 * index;
        for value in [
            1,
            u16_of(shared - at),
            0x0004,
            0,
            1,
            u16_of(shared + 6 - at),
        ] {
            push16(&mut list, value);
        }
    }
    for value in [1, 1, 1, 1, 3] {
        push16(&mut list, value);
    }
    push_i16(&mut list, -4);
    list
}

/// A `name` table of format 0: records of (platform, encoding, language,
/// name ID) and their strings, UTF-16BE for platforms 0 and 3, the bytes
/// as they are for others.
pub(crate) fn name(records: &[(u16, u16, u16, u16, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 0);
    push16(&mut out, u16_of(records.len()));
    push16(&mut out, u16_of(6 + 12 * records.len()));
    let mut strings = Vec::new();
    for &(platform, encoding, language, id, text) in records {
        let bytes: Vec<u8> = if platform == 0 || platform == 3 {
            text.encode_utf16().flat_map(u16::to_be_bytes).collect()
        } else {
            text.as_bytes().to_vec()
        };
        push16(&mut out, platform);
        push16(&mut out, encoding);
        push16(&mut out, language);
        push16(&mut out, id);
        push16(&mut out, u16_of(bytes.len()));
        push16(&mut out, u16_of(strings.len()));
        strings.extend(bytes);
    }
    out.extend(strings);
    out
}

/// An `OS/2` table of `version`, as long as that version's fields, with
/// these values and the rest 0.
pub(crate) fn os2(
    version: u16,
    weight: u16,
    selection: u16,
    metrics: [i16; 5],
    heights: [i16; 2],
) -> Vec<u8> {
    let length = match version {
        0 => 78,
        1 => 86,
        2..=4 => 96,
        _ => 100,
    };
    let mut out = vec![0; length];
    patch(&mut out, 0, version);
    patch(&mut out, 4, weight);
    patch(&mut out, 62, selection);
    for (index, value) in metrics.iter().enumerate() {
        patch(&mut out, 68 + index * 2, *value as u16);
    }
    if version >= 2 {
        patch(&mut out, 86, heights[0] as u16);
        patch(&mut out, 88, heights[1] as u16);
    }
    out
}

/// The square from (0, 0) to (size, size), clockwise as TrueType winds
/// outer contours.
pub(crate) fn square(size: i16) -> Vec<Point> {
    vec![
        (0, 0, true),
        (0, size, true),
        (size, size, true),
        (size, 0, true),
    ]
}

/// A font to build: its glyphs (encoded) and the tables made from them,
/// with tables to add, replace or leave out.
#[derive(Clone, Debug)]
pub(crate) struct Font {
    pub(crate) version: [u8; 4],
    pub(crate) units_per_em: u16,
    pub(crate) glyphs: Vec<Vec<u8>>,
    /// Each glyph's advance and left side bearing.
    pub(crate) metrics: Vec<(u16, i16)>,
    /// `numberOfHMetrics`.
    pub(crate) long_metrics: usize,
    pub(crate) long_loca: bool,
    pub(crate) cmap: Vec<u8>,
    /// Tables that replace one of the same tag, or are added after the
    /// required ones.
    pub(crate) tables: Vec<Table>,
    /// Tables left out.
    pub(crate) without: Vec<[u8; 4]>,
}

impl Font {
    /// A font of `glyphs`, whose glyph `i` has advance `500 + 10 × i` and
    /// bearing `i`, with a format 4 `cmap` mapping `A` onwards to glyphs 1
    /// onwards.
    pub(crate) fn new(glyphs: Vec<Vec<u8>>) -> Self {
        let count = glyphs.len();
        let metrics = (0..count)
            .map(|index| (500 + 10 * index as u16, index as i16))
            .collect();
        let last = u16_of(b'A' as usize + count.max(2) - 2);
        let cmap = cmap(&[(
            3,
            1,
            cmap4(&[Segment::Delta(u16::from(b'A'), last, 1 - b'A' as i16)]),
        )]);
        Self {
            version: [0, 1, 0, 0],
            units_per_em: 1000,
            glyphs,
            metrics,
            long_metrics: count,
            long_loca: false,
            cmap,
            tables: Vec::new(),
            without: Vec::new(),
        }
    }

    /// The usual test font: `.notdef` (a 500-unit square), a 100-unit
    /// square, an empty glyph and a triangle.
    pub(crate) fn basic() -> Self {
        Self::new(vec![
            simple(&[&square(500)]),
            simple(&[&square(100)]),
            Vec::new(),
            simple(&[&[(0, 0, true), (50, 100, true), (100, 0, true)]]),
        ])
    }

    /// Adds `table`, or replaces the one with its tag.
    pub(crate) fn with(mut self, tag: &[u8; 4], bytes: Vec<u8>) -> Self {
        self.tables.retain(|(found, _)| found != tag);
        self.tables.push((*tag, bytes));
        self
    }

    /// Leaves table `tag` out.
    pub(crate) fn without(mut self, tag: &[u8; 4]) -> Self {
        self.without.push(*tag);
        self
    }

    /// The file.
    pub(crate) fn build(&self) -> Vec<u8> {
        let count = u16_of(self.glyphs.len());
        let (loca, glyf) = loca_glyf(&self.glyphs, self.long_loca);
        let max = self
            .metrics
            .iter()
            .map(|metric| metric.0)
            .max()
            .unwrap_or(0);
        let long = u16_of(self.long_metrics);
        let mut tables: Vec<Table> = vec![
            (
                *b"head",
                head(self.units_per_em, self.long_loca, [-10, -20, 600, 800]),
            ),
            (*b"hhea", hhea(800, -200, 90, max, long)),
            (*b"maxp", maxp(count)),
            (*b"hmtx", hmtx(&self.metrics, self.long_metrics)),
            (*b"cmap", self.cmap.clone()),
            (*b"loca", loca),
            (*b"glyf", glyf),
        ];
        for (tag, bytes) in &self.tables {
            match tables.iter_mut().find(|(found, _)| found == tag) {
                Some(table) => table.1 = bytes.clone(),
                None => tables.push((*tag, bytes.clone())),
            }
        }
        tables.retain(|(tag, _)| !self.without.contains(tag));
        sfnt(self.version, &tables)
    }
}
