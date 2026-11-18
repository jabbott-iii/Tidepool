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

//! Hand-made test vectors for `ttf`: fonts built by `common`'s builder,
//! valid ones and ones with one thing wrong, so that every [`ErrorKind`]
//! is reached ([`every_error_kind_is_reached`]).

mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;

use common::{
    Component, Placement, Script, Segment, Transform, at, class_def1, cmap, cmap4, cmap12,
    composite, coverage1, extension, gpos, kern, name, os2, pair_pos1, pair_pos2, patch, patch32,
    ranges, read16, script, sfnt, simple, simple_with_instructions, square, table,
};
use tidepool_font::outline::OutlinePoint;
use tidepool_font::ttf::{
    BoundingBox, ErrorKind, Font, GlyphId, HMetrics, LineMetrics, NameId, Os2, Os2LineMetrics, Tag,
};
use tidepool_font::{FontError, Limits};

thread_local! {
    /// The kinds of error the tests have seen, for
    /// [`every_error_kind_is_reached`].
    static SEEN: RefCell<BTreeSet<&'static str>> = const { RefCell::new(BTreeSet::new()) };
}

/// Every kind's name: a `match` without a wildcard, so a new kind does not
/// compile until it is listed here, and so tested.
fn kind_name(kind: &ErrorKind) -> &'static str {
    match kind {
        ErrorKind::FileTooLarge { .. } => "FileTooLarge",
        ErrorKind::Collection => "Collection",
        ErrorKind::Cff => "Cff",
        ErrorKind::BadSfntVersion { .. } => "BadSfntVersion",
        ErrorKind::UnexpectedEnd { .. } => "UnexpectedEnd",
        ErrorKind::TableOutOfBounds => "TableOutOfBounds",
        ErrorKind::DuplicateTable => "DuplicateTable",
        ErrorKind::MissingTable => "MissingTable",
        ErrorKind::BadVersion { .. } => "BadVersion",
        ErrorKind::BadMagic { .. } => "BadMagic",
        ErrorKind::BadUnitsPerEm { .. } => "BadUnitsPerEm",
        ErrorKind::BadLocaFormat { .. } => "BadLocaFormat",
        ErrorKind::BadGlyphDataFormat { .. } => "BadGlyphDataFormat",
        ErrorKind::NoGlyphs => "NoGlyphs",
        ErrorKind::BadMetricsCount { .. } => "BadMetricsCount",
        ErrorKind::LocaOutOfOrder => "LocaOutOfOrder",
        ErrorKind::LocaPastGlyf => "LocaPastGlyf",
        ErrorKind::NoUnicodeCmap => "NoUnicodeCmap",
        ErrorKind::OddSegCount { .. } => "OddSegCount",
        ErrorKind::BadLength { .. } => "BadLength",
        ErrorKind::NotSorted { .. } => "NotSorted",
        ErrorKind::BackwardRange { .. } => "BackwardRange",
        ErrorKind::BadFormat { .. } => "BadFormat",
        ErrorKind::IndexOutOfRange { .. } => "IndexOutOfRange",
        ErrorKind::OverlappingStructures => "OverlappingStructures",
        ErrorKind::ContoursOutOfOrder => "ContoursOutOfOrder",
        ErrorKind::FlagsPastPoints => "FlagsPastPoints",
        ErrorKind::TooManyPoints { .. } => "TooManyPoints",
        ErrorKind::TooDeep { .. } => "TooDeep",
        ErrorKind::TooManyComponents { .. } => "TooManyComponents",
        ErrorKind::GlyphOutOfRange { .. } => "GlyphOutOfRange",
        ErrorKind::TooManyKerningSubtables { .. } => "TooManyKerningSubtables",
        ErrorKind::CoordinateOutOfRange => "CoordinateOutOfRange",
        ErrorKind::PointOutOfRange { .. } => "PointOutOfRange",
    }
}

/// Every kind, once each.
const ALL_KINDS: usize = 34;

fn seen(kind: &ErrorKind) {
    SEEN.with(|seen| seen.borrow_mut().insert(kind_name(kind)));
}

/// Parses `font`, which must fail with `kind` in `table` at `offset`.
#[track_caller]
fn rejects(font: &[u8], table: Option<&[u8; 4]>, offset: usize, kind: ErrorKind) {
    rejects_with(font, Limits::DEFAULT, table, offset, kind);
}

#[track_caller]
fn rejects_with(
    font: &[u8],
    limits: Limits,
    table: Option<&[u8; 4]>,
    offset: usize,
    kind: ErrorKind,
) {
    let error = Font::parse(font.to_vec(), limits).expect_err("the font is malformed");
    let expected = FontError::Ttf {
        table: table.map(|tag| Tag(*tag)),
        offset,
        kind,
    };
    if let FontError::Ttf { kind, .. } = &error {
        seen(kind);
    }
    assert_eq!(error, expected, "{error}");
}

/// Reads glyph `glyph` of `font`, which must fail with `kind` in glyph
/// `in_glyph` at `offset`.
#[track_caller]
fn bad_glyph(font: &Font, glyph: u16, in_glyph: u16, offset: usize, kind: ErrorKind) {
    let error = font
        .outline(GlyphId(glyph))
        .expect_err("the glyph is malformed");
    if let FontError::Glyph { kind, .. } = &error {
        seen(kind);
    }
    assert_eq!(
        error,
        FontError::Glyph {
            glyph: GlyphId(in_glyph),
            offset,
            kind,
        },
        "{error}"
    );
}

fn parse(font: &[u8]) -> Font {
    Font::parse(font.to_vec(), Limits::DEFAULT).unwrap_or_else(|error| panic!("{error}"))
}

fn parse_with(font: &[u8], limits: Limits) -> Font {
    Font::parse(font.to_vec(), limits).unwrap_or_else(|error| panic!("{error}"))
}

/// The points of an outline as (x, y, on the curve).
fn points(font: &Font, glyph: u16) -> Vec<(f32, f32, bool)> {
    let outline = font
        .outline(GlyphId(glyph))
        .unwrap_or_else(|error| panic!("{error}"));
    outline
        .points()
        .iter()
        .map(|&OutlinePoint { x, y, on_curve }| (x, y, on_curve))
        .collect()
}

fn on(x: f32, y: f32) -> (f32, f32, bool) {
    (x, y, true)
}

// The file and its directory.

#[test]
fn a_basic_font_parses_and_reports_its_headers() {
    let font = parse(&common::Font::basic().build());
    assert_eq!(font.units_per_em(), 1000);
    assert_eq!(font.glyph_count(), 4);
    assert_eq!(
        font.bounding_box(),
        BoundingBox {
            x_min: -10,
            y_min: -20,
            x_max: 600,
            y_max: 800
        }
    );
    assert_eq!(
        font.line_metrics(),
        LineMetrics {
            ascender: 800,
            descender: -200,
            line_gap: 90,
            advance_width_max: 530,
        }
    );
    assert_eq!(font.os2(), None);
    assert_eq!(font.name(NameId::FAMILY), None);
    assert_eq!(font.limits(), Limits::DEFAULT);
    assert!(font.data().starts_with(&[0, 1, 0, 0]));
    assert!(format!("{font:?}").contains("glyph_count: 4"));
}

#[test]
fn a_borrowed_file_parses_in_place() {
    let bytes: &'static [u8] = common::Font::basic().build().leak();
    let font = Font::parse(bytes, Limits::DEFAULT).expect("parses");
    assert!(std::ptr::eq(font.data(), bytes));
}

#[test]
fn the_sfnt_version_must_be_truetype() {
    let mut font = common::Font::basic();
    font.version = *b"true";
    parse(&font.build());
    let rest = [0u8; 64];
    let with = |version: &[u8; 4]| [&version[..], &rest[..]].concat();
    rejects(&with(b"OTTO"), None, 0, ErrorKind::Cff);
    rejects(&with(b"ttcf"), None, 0, ErrorKind::Collection);
    rejects(
        &with(b"wOFF"),
        None,
        0,
        ErrorKind::BadSfntVersion { found: 0x774f_4646 },
    );
    rejects(
        &[0, 1],
        None,
        0,
        ErrorKind::UnexpectedEnd {
            expected: "the sfnt version",
        },
    );
}

#[test]
fn the_file_size_is_checked_first() {
    let font = common::Font::basic().build();
    let limits = Limits {
        max_file_bytes: font.len() - 1,
        ..Limits::DEFAULT
    };
    rejects_with(
        &font,
        limits,
        None,
        font.len() - 1,
        ErrorKind::FileTooLarge {
            limit: font.len() - 1,
        },
    );
    let limits = Limits {
        max_file_bytes: font.len(),
        ..Limits::DEFAULT
    };
    parse_with(&font, limits);
}

#[test]
fn the_directory_must_fit_and_hold_each_table_once() {
    let font = common::Font::basic().build();
    // Cut inside the records.
    rejects(
        &font[..40],
        None,
        12,
        ErrorKind::UnexpectedEnd {
            expected: "the table records",
        },
    );
    // A table that reaches past the end of the file.
    let mut long = font.clone();
    let (_, record) = table(&font, b"glyf");
    patch32(&mut long, record + 12, 1 << 20);
    rejects(&long, Some(b"glyf"), record, ErrorKind::TableOutOfBounds);
    // An offset that overflows with its length.
    let mut overflow = font.clone();
    patch32(&mut overflow, record + 8, u32::MAX);
    rejects(
        &overflow,
        Some(b"glyf"),
        record,
        ErrorKind::TableOutOfBounds,
    );
    // A table listed twice.
    let built = common::Font::basic();
    let tables: Vec<common::Table> = [
        b"head", b"hhea", b"maxp", b"hmtx", b"cmap", b"loca", b"glyf",
    ]
    .iter()
    .map(|tag| {
        let (offset, record) = table(&font, tag);
        let length = u32::from_be_bytes(font[record + 12..record + 16].try_into().unwrap());
        (**tag, font[offset..offset + length as usize].to_vec())
    })
    .collect();
    let mut twice = tables.clone();
    twice.push(tables[1].clone());
    let doubled = sfnt(built.version, &twice);
    rejects(
        &doubled,
        Some(b"hhea"),
        12 + 7 * 16,
        ErrorKind::DuplicateTable,
    );
    // A table that is not read may be listed twice, and lie anywhere.
    let mut ignored = tables.clone();
    ignored.push((*b"post", vec![0; 8]));
    ignored.push((*b"post", vec![1; 8]));
    let mut ignored = sfnt(built.version, &ignored);
    let (_, record) = table(&ignored, b"post");
    patch32(&mut ignored, record + 8, u32::MAX);
    parse(&ignored);
}

#[test]
fn every_required_table_must_be_there() {
    for tag in [
        b"head", b"hhea", b"maxp", b"hmtx", b"cmap", b"loca", b"glyf",
    ] {
        let font = common::Font::basic().without(tag).build();
        rejects(&font, Some(tag), 12 + 6 * 16, ErrorKind::MissingTable);
    }
}

// The small tables.

#[test]
fn head_is_checked() {
    let font = common::Font::basic().build();
    let (head, _) = table(&font, b"head");
    let with = |at: usize, value: u16| {
        let mut font = font.clone();
        patch(&mut font, head + at, value);
        font
    };
    rejects(
        &with(0, 2),
        Some(b"head"),
        head,
        ErrorKind::BadVersion { found: 2 },
    );
    rejects(
        &with(12, 0),
        Some(b"head"),
        head + 12,
        ErrorKind::BadMagic { found: 0x3cf5 },
    );
    rejects(
        &with(18, 15),
        Some(b"head"),
        head + 18,
        ErrorKind::BadUnitsPerEm { found: 15 },
    );
    rejects(
        &with(18, 16_385),
        Some(b"head"),
        head + 18,
        ErrorKind::BadUnitsPerEm { found: 16_385 },
    );
    parse(&with(18, 16));
    parse(&with(18, 16_384));
    rejects(
        &with(50, 2),
        Some(b"head"),
        head + 50,
        ErrorKind::BadLocaFormat { found: 2 },
    );
    rejects(
        &with(52, 1),
        Some(b"head"),
        head + 52,
        ErrorKind::BadGlyphDataFormat { found: 1 },
    );
    let short = common::Font::basic()
        .with(b"head", common::head(1000, false, [0; 4])[..53].to_vec())
        .build();
    let (head, _) = table(&short, b"head");
    rejects(
        &short,
        Some(b"head"),
        head,
        ErrorKind::UnexpectedEnd {
            expected: "the fields of `head`",
        },
    );
}

#[test]
fn maxp_and_hhea_are_checked() {
    let font = common::Font::basic().build();
    let (maxp, _) = table(&font, b"maxp");
    let mut cff = font.clone();
    patch32(&mut cff, maxp, 0x0000_5000);
    rejects(
        &cff,
        Some(b"maxp"),
        maxp,
        ErrorKind::BadVersion { found: 0x5000 },
    );
    let mut none = font.clone();
    patch(&mut none, maxp + 4, 0);
    rejects(&none, Some(b"maxp"), maxp + 4, ErrorKind::NoGlyphs);

    let (hhea, _) = table(&font, b"hhea");
    let mut version = font.clone();
    patch(&mut version, hhea, 2);
    rejects(
        &version,
        Some(b"hhea"),
        hhea,
        ErrorKind::BadVersion { found: 2 },
    );
    for count in [0, 5] {
        let mut bad = font.clone();
        patch(&mut bad, hhea + 34, count);
        rejects(
            &bad,
            Some(b"hhea"),
            hhea + 34,
            ErrorKind::BadMetricsCount { found: count },
        );
    }
}

#[test]
fn hmtx_gives_the_last_advance_to_the_glyphs_after_it() {
    let mut font = common::Font::basic();
    font.long_metrics = 2;
    let font = parse(&font.build());
    let metrics = |glyph| font.h_metrics(GlyphId(glyph));
    assert_eq!(
        metrics(0),
        Some(HMetrics {
            advance: 500,
            left_side_bearing: 0
        })
    );
    assert_eq!(
        metrics(1),
        Some(HMetrics {
            advance: 510,
            left_side_bearing: 1
        })
    );
    // Glyphs 2 and 3 take glyph 1's advance and keep their own bearings.
    assert_eq!(
        metrics(3),
        Some(HMetrics {
            advance: 510,
            left_side_bearing: 3
        })
    );
    assert_eq!(metrics(4), None);

    // The table must hold the bearings too.
    let mut short = common::Font::basic();
    short.long_metrics = 2;
    let bytes = short.build();
    let (hmtx, record) = table(&bytes, b"hmtx");
    let mut cut = bytes.clone();
    patch32(&mut cut, record + 12, 4 * 2 + 2 * 2 - 1);
    rejects(
        &cut,
        Some(b"hmtx"),
        hmtx,
        ErrorKind::UnexpectedEnd {
            expected: "the horizontal metrics",
        },
    );
}

#[test]
fn os2_is_read_as_long_as_its_version_says() {
    let metrics = [900, -250, 80, 1100, 300];
    let font = common::Font::basic()
        .with(b"OS/2", os2(5, 600, 0x00a1, metrics, [500, 700]))
        .build();
    assert_eq!(
        parse(&font).os2(),
        Some(Os2 {
            version: 5,
            weight_class: 600,
            italic: true,
            bold: true,
            use_typo_metrics: true,
            line_metrics: Some(Os2LineMetrics {
                typo_ascender: 900,
                typo_descender: -250,
                typo_line_gap: 80,
                win_ascent: 1100,
                win_descent: 300,
            }),
            x_height: Some(500),
            cap_height: Some(700),
        })
    );
    // Versions 0 and 1 have no heights.
    for version in [0, 1] {
        let font = common::Font::basic()
            .with(b"OS/2", os2(version, 400, 0, metrics, [500, 700]))
            .build();
        let read = parse(&font).os2().expect("an OS/2 table");
        assert_eq!(
            (read.version, read.x_height, read.cap_height),
            (version, None, None)
        );
        assert!(!read.italic && !read.bold && !read.use_typo_metrics);
    }
    // A table cut short for its version.
    let cut = os2(2, 400, 0, metrics, [0, 0])[..95].to_vec();
    let font = common::Font::basic().with(b"OS/2", cut).build();
    let (at, _) = table(&font, b"OS/2");
    rejects(
        &font,
        Some(b"OS/2"),
        at,
        ErrorKind::UnexpectedEnd {
            expected: "the fields of versions 2 to 4",
        },
    );
}

#[test]
fn a_legacy_version_0_os2_of_68_bytes_has_no_line_metrics() {
    let metrics = [900, -250, 80, 1100, 300];
    let whole = os2(0, 400, 0x0081, metrics, [0, 0]);
    assert_eq!(whole.len(), 78);
    let read = |bytes: &[u8]| {
        let font = common::Font::basic().with(b"OS/2", bytes.to_vec()).build();
        parse(&font).os2().expect("an OS/2 table")
    };
    let full = read(&whole);
    assert_eq!(
        full.line_metrics,
        Some(Os2LineMetrics {
            typo_ascender: 900,
            typo_descender: -250,
            typo_line_gap: 80,
            win_ascent: 1100,
            win_descent: 300,
        })
    );
    // Through usLastCharIndex: the style is read, the metrics are not.
    let short = read(&whole[..68]);
    assert_eq!(short.line_metrics, None);
    assert_eq!(
        (
            short.version,
            short.weight_class,
            short.italic,
            short.use_typo_metrics
        ),
        (0, 400, true, true)
    );
    assert_eq!(read(&whole[..77]).line_metrics, None);
    // Any shorter is an error.
    let font = common::Font::basic()
        .with(b"OS/2", whole[..67].to_vec())
        .build();
    let (at, _) = table(&font, b"OS/2");
    rejects(
        &font,
        Some(b"OS/2"),
        at,
        ErrorKind::UnexpectedEnd {
            expected: "the fields of version 0",
        },
    );
    // Version 1 still needs its 86.
    let font = common::Font::basic()
        .with(b"OS/2", os2(1, 400, 0, metrics, [0, 0])[..85].to_vec())
        .build();
    let (at, _) = table(&font, b"OS/2");
    rejects(
        &font,
        Some(b"OS/2"),
        at,
        ErrorKind::UnexpectedEnd {
            expected: "the fields of version 1",
        },
    );
}

#[test]
fn loca_is_checked_whole() {
    for long in [false, true] {
        let mut built = common::Font::basic();
        built.long_loca = long;
        let font = built.build();
        parse(&font);
        let (loca, _) = table(&font, b"loca");
        let size = if long { 4 } else { 2 };
        let set = |font: &mut Vec<u8>, index: usize, value: u32| {
            if long {
                patch32(font, loca + index * 4, value);
            } else {
                patch(font, loca + index * 2, u16::try_from(value / 2).unwrap());
            }
        };
        let mut down = font.clone();
        set(&mut down, 2, 0);
        rejects(
            &down,
            Some(b"loca"),
            loca + 2 * size,
            ErrorKind::LocaOutOfOrder,
        );
        let (_, glyf) = table(&font, b"glyf");
        let glyf_len = u32::from_be_bytes(font[glyf + 12..glyf + 16].try_into().unwrap());
        let mut past = font.clone();
        set(&mut past, 4, glyf_len + 2);
        rejects(
            &past,
            Some(b"loca"),
            loca + 4 * size,
            ErrorKind::LocaPastGlyf,
        );
    }
    let font = common::Font::basic().build();
    let (_, record) = table(&font, b"loca");
    let mut cut = font.clone();
    patch32(&mut cut, record + 12, 9);
    let (loca, _) = table(&font, b"loca");
    rejects(
        &cut,
        Some(b"loca"),
        loca,
        ErrorKind::UnexpectedEnd {
            expected: "an offset for each glyph and one more",
        },
    );
}

// The character map.

#[test]
fn format_4_maps_by_delta_and_by_glyph_array() {
    let subtable = cmap4(&[
        Segment::Delta(0x20, 0x21, -0x1f),
        // Entries 2, 0, 8 and 1, plus 1; 0xFFFF plus 1 wraps to glyph 0.
        Segment::Glyphs(0x41, 0x45, 1, vec![2, 0, 8, 1, 0xffff]),
        Segment::Delta(0x100, 0x100, -0xff),
    ]);
    let font = common::Font::basic().with(b"cmap", cmap(&[(3, 1, subtable)]));
    let font = parse(&font.build());
    let index = |c| font.glyph_index(c).map(|glyph| glyph.0);
    assert_eq!(index(' '), Some(1));
    assert_eq!(index('!'), Some(2));
    assert_eq!(index('"'), None);
    assert_eq!(index('A'), Some(3));
    // A 0 in the array is not mapped, whatever the delta; glyph 9 does not
    // exist, and the entry that wraps is glyph 0.
    assert_eq!(index('B'), None);
    assert_eq!(index('C'), None);
    assert_eq!(index('D'), Some(2));
    assert_eq!(index('E'), None);
    assert_eq!(index('\u{100}'), Some(1));
    // Format 4 holds only the Basic Multilingual Plane; the final segment
    // maps 0xFFFF to glyph 0.
    assert_eq!(index('\u{FFFF}'), None);
    assert_eq!(index('\u{1F600}'), None);
    assert_eq!(index('\0'), None);
}

#[test]
fn format_4_maps_a_range_offset_outside_the_subtable_to_nothing() {
    let mut subtable = cmap4(&[Segment::Glyphs(0x41, 0x42, 0, vec![1, 2])]);
    // The first segment's range offset (after the 14-byte header, two end
    // codes, the pad, two start codes and two deltas) points just past the
    // subtable for 'B', and inside it for 'A'. What follows the subtable is
    // a Macintosh subtable (never read) whose first two bytes would map 'B'
    // to glyph 2, were they read.
    let range_at = 14 + 4 + 2 + 4 + 4;
    let length = subtable.len();
    patch(
        &mut subtable,
        range_at,
        u16::try_from(length - range_at - 2).unwrap(),
    );
    let after = vec![0, 2, 0, 6, 0, 0];
    let font = parse(
        &common::Font::basic()
            .with(b"cmap", cmap(&[(3, 1, subtable), (1, 0, after)]))
            .build(),
    );
    assert!(font.glyph_index('A').is_some());
    assert_eq!(font.glyph_index('B'), None);
}

#[test]
fn format_12_maps_every_plane_and_is_preferred() {
    let wide = cmap12(&[
        (0x41, 0x42, 1),
        (0x1F600, 0x1F600, 3),
        (0x1F601, 0x1F602, 4),
    ]);
    let narrow = cmap4(&[Segment::Delta(0x41, 0x41, -0x41 + 2)]);
    for (platform, encoding) in [(3, 10), (0, 4)] {
        let both = cmap(&[(3, 1, narrow.clone()), (platform, encoding, wide.clone())]);
        let font = parse(&common::Font::basic().with(b"cmap", both).build());
        let index = |c| font.glyph_index(c).map(|glyph| glyph.0);
        assert_eq!(index('A'), Some(1));
        assert_eq!(index('B'), Some(2));
        assert_eq!(index('C'), None);
        assert_eq!(index('\u{1F600}'), Some(3));
        // Glyph 4 would be the last one's; glyph 5 does not exist.
        assert_eq!(index('\u{1F601}'), None);
        assert_eq!(index('\u{1F602}'), None);
    }
    // Format 12 under (3, 1) is not read: the format 4 subtable is.
    let odd = cmap(&[(3, 1, wide.clone()), (0, 3, narrow.clone())]);
    let font = parse(&common::Font::basic().with(b"cmap", odd).build());
    assert_eq!(font.glyph_index('A'), Some(GlyphId(2)));
}

#[test]
fn cmap_needs_a_unicode_subtable() {
    let symbol = cmap(&[(3, 0, cmap4(&[])), (1, 0, vec![0, 0, 0, 6, 0, 0])]);
    let font = common::Font::basic().with(b"cmap", symbol).build();
    let (at, _) = table(&font, b"cmap");
    rejects(&font, Some(b"cmap"), at, ErrorKind::NoUnicodeCmap);
    let mut version = common::Font::basic().build();
    let (at, _) = table(&version, b"cmap");
    patch(&mut version, at, 1);
    rejects(
        &version,
        Some(b"cmap"),
        at,
        ErrorKind::BadVersion { found: 1 },
    );
}

#[test]
fn cmap_subtables_are_checked() {
    let build = |subtable: Vec<u8>| {
        common::Font::basic()
            .with(b"cmap", cmap(&[(3, 1, subtable)]))
            .build()
    };
    let subtable = |font: &[u8]| table(font, b"cmap").0 + 12;
    let good = cmap4(&[
        Segment::Delta(0x41, 0x42, -0x40),
        Segment::Delta(0x50, 0x51, -0x40),
    ]);

    let mut odd = good.clone();
    patch(&mut odd, 6, 5);
    let font = build(odd);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 6,
        ErrorKind::OddSegCount { found: 5 },
    );

    let mut unsorted = good.clone();
    patch(&mut unsorted, 14 + 2, 0x40);
    let font = build(unsorted);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 16,
        ErrorKind::NotSorted {
            what: "segments' end codes",
        },
    );

    let mut short = good.clone();
    patch(&mut short, 2, 16 + 8 * 3 - 2);
    let font = build(short);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 2,
        ErrorKind::BadLength {
            what: "a format 4 subtable",
        },
    );

    let mut long = good.clone();
    patch(&mut long, 2, 1000);
    let font = build(long);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font),
        ErrorKind::UnexpectedEnd {
            expected: "the format 4 subtable, as its length says",
        },
    );

    let build12 = |subtable: Vec<u8>| {
        common::Font::basic()
            .with(b"cmap", cmap(&[(3, 10, subtable)]))
            .build()
    };
    let backward = cmap12(&[(0x42, 0x41, 1)]);
    let font = build12(backward);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 16,
        ErrorKind::BackwardRange {
            what: "a format 12 subtable",
        },
    );
    let overlapping = cmap12(&[(0x41, 0x45, 1), (0x45, 0x46, 1)]);
    let font = build12(overlapping);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 28,
        ErrorKind::NotSorted {
            what: "groups of a format 12 subtable",
        },
    );
    let mut many = cmap12(&[(0x41, 0x45, 1)]);
    patch32(&mut many, 12, 2);
    let font = build12(many);
    rejects(
        &font,
        Some(b"cmap"),
        subtable(&font) + 4,
        ErrorKind::BadLength {
            what: "a format 12 subtable",
        },
    );
}

// Names.

#[test]
fn names_prefer_windows_english_then_windows_then_unicode() {
    let records = [
        (1, 0, 0, 1, "Mac Family"),
        (0, 3, 0, 1, "Unicode Family"),
        (3, 1, 0x0407, 1, "Deutsche Familie"),
        (3, 1, 0x0409, 1, "Family"),
        (0, 3, 0, 2, "Unicode Style"),
        (3, 10, 0x0407, 2, "Stil"),
        (0, 4, 0, 4, "Full \u{1F600}"),
        (1, 0, 0, 5, "Mac only"),
        (3, 0, 0x0409, 6, "Symbol only"),
    ];
    let font = parse(&common::Font::basic().with(b"name", name(&records)).build());
    assert_eq!(font.name(NameId::FAMILY).as_deref(), Some("Family"));
    assert_eq!(font.name(NameId::SUBFAMILY).as_deref(), Some("Stil"));
    assert_eq!(
        font.name(NameId::FULL_NAME).as_deref(),
        Some("Full \u{1F600}")
    );
    assert_eq!(font.name(NameId::VERSION), None);
    assert_eq!(font.name(NameId::POSTSCRIPT_NAME), None);
    assert_eq!(font.name(NameId::TYPOGRAPHIC_FAMILY), None);
}

#[test]
fn names_are_checked() {
    let font = common::Font::basic()
        .with(b"name", name(&[(3, 1, 0x409, 1, "Family")]))
        .build();
    let (at, _) = table(&font, b"name");
    let mut format = font.clone();
    patch(&mut format, at, 2);
    rejects(
        &format,
        Some(b"name"),
        at,
        ErrorKind::BadVersion { found: 2 },
    );
    // A string past the end of the table.
    let mut past = font.clone();
    patch(&mut past, at + 6 + 8, 100);
    rejects(
        &past,
        Some(b"name"),
        at + 18,
        ErrorKind::UnexpectedEnd {
            expected: "a name's string",
        },
    );
    // A Mac record's string is never read, so it is not checked.
    let mac = common::Font::basic()
        .with(b"name", name(&[(1, 0, 0, 1, "Family")]))
        .build();
    let (at, _) = table(&mac, b"name");
    let mut mac = mac.clone();
    patch(&mut mac, at + 6 + 8, 100);
    assert_eq!(parse(&mac).name(NameId::FAMILY), None);
}

// Outlines.

#[test]
fn simple_glyphs_read_every_encoding() {
    let contour: Vec<common::Point> = vec![
        (0, 0, true),
        (0, 0, false),
        (300, 0, true),
        (-300, 2, false),
        (-300, 1000, true),
        (-299, 1000, true),
    ];
    let triangle = [(10, 10, true), (20, 30, true), (30, 10, true)];
    let glyph = simple_with_instructions(&[&contour, &triangle], &[0xb0, 0x00, 0x2c]);
    let font = parse(&common::Font::new(vec![simple(&[&square(500)]), glyph]).build());
    let outline = font.outline(GlyphId(1)).expect("reads");
    assert_eq!(outline.contour_ends(), [6, 9]);
    assert_eq!(outline.contour_count(), 2);
    let read: Vec<(f32, f32, bool)> = outline
        .points()
        .iter()
        .map(|point| (point.x, point.y, point.on_curve))
        .collect();
    let expected: Vec<(f32, f32, bool)> = contour
        .iter()
        .chain(&triangle)
        .map(|&(x, y, on_curve)| (f32::from(x), f32::from(y), on_curve))
        .collect();
    assert_eq!(read, expected);
    let contours: Vec<usize> = outline.contours().map(<[OutlinePoint]>::len).collect();
    assert_eq!(contours, [6, 3]);
    assert_eq!(
        outline.control_box().map(|bounds| (
            bounds.x_min,
            bounds.y_min,
            bounds.x_max,
            bounds.y_max
        )),
        Some((-300.0, 0.0, 300.0, 1000.0))
    );
}

#[test]
fn empty_glyphs_have_empty_outlines() {
    let zero_contours = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let font =
        parse(&common::Font::new(vec![simple(&[&square(500)]), Vec::new(), zero_contours]).build());
    for glyph in [1, 2] {
        let outline = font.outline(GlyphId(glyph)).expect("reads");
        assert!(outline.is_empty());
        assert_eq!((outline.point_count(), outline.contour_count()), (0, 0));
        assert_eq!(outline.control_box(), None);
    }
}

#[test]
fn simple_glyphs_are_checked() {
    let glyphs = |glyph: Vec<u8>| common::Font::new(vec![simple(&[&square(500)]), glyph]);
    let glyph_at = |font: &[u8]| {
        let (glyf, _) = table(font, b"glyf");
        glyf + simple(&[&square(500)]).len().next_multiple_of(2)
    };

    // End points that go down.
    let mut glyph = simple(&[&square(10), &square(20)]);
    patch(&mut glyph, 12, 2);
    let built = glyphs(glyph).build();
    let at = glyph_at(&built);
    bad_glyph(&parse(&built), 1, 1, at + 12, ErrorKind::ContoursOutOfOrder);

    // A repeat past the last point: one contour of 4 points, with a flag
    // repeated 4 more times.
    let mut glyph = Vec::new();
    common::push_i16(&mut glyph, 1);
    glyph.extend([0; 8]);
    common::push16(&mut glyph, 3);
    common::push16(&mut glyph, 0);
    glyph.extend([0x01 | 0x08 | 0x10 | 0x20, 4, 0, 0]);
    let built = glyphs(glyph).build();
    let at = glyph_at(&built);
    bad_glyph(&parse(&built), 1, 1, at + 15, ErrorKind::FlagsPastPoints);

    // Cut in the header, the instructions, the flags and the coordinates:
    // the end point at 10, the instructions' length at 12, three bytes of
    // instructions, four flags from 17, an x coordinate at 21 and two y
    // coordinates from 23. Glyphs are padded to an even length, so each
    // cut is even.
    let whole = simple_with_instructions(&[&square(1000)], &[1, 2, 3]);
    assert_eq!(whole.len(), 27);
    for (cut, at, expected) in [
        (8, 0, "the glyph's header"),
        (12, 12, "the instructions' length"),
        (16, 14, "the instructions"),
        (18, 18, "a point's flags"),
        (22, 21, "an x coordinate"),
        (24, 23, "a y coordinate"),
    ] {
        let built = glyphs(whole[..cut].to_vec()).build();
        let start = glyph_at(&built);
        bad_glyph(
            &parse(&built),
            1,
            1,
            start + at,
            ErrorKind::UnexpectedEnd { expected },
        );
    }
}

#[test]
fn the_point_limit_is_checked_before_the_points_are_read() {
    let built = common::Font::new(vec![
        simple(&[&square(500)]),
        simple(&[&square(10), &square(20)]),
    ])
    .build();
    let limits = Limits {
        max_glyph_points: 7,
        ..Limits::DEFAULT
    };
    let font = parse_with(&built, limits);
    let (glyf, _) = table(&built, b"glyf");
    let at = glyf + simple(&[&square(500)]).len() + 12;
    bad_glyph(&font, 1, 1, at, ErrorKind::TooManyPoints { limit: 7 });
    // Checked before anything after the end points is read: a glyph that
    // ends right after them still fails for its points.
    let mut cut = Vec::new();
    common::push_i16(&mut cut, 1);
    cut.extend([0; 8]);
    common::push16(&mut cut, 9);
    let short = common::Font::new(vec![simple(&[&square(500)]), cut]).build();
    let (glyf, _) = table(&short, b"glyf");
    let at = glyf + simple(&[&square(500)]).len() + 10;
    bad_glyph(
        &parse_with(&short, limits),
        1,
        1,
        at,
        ErrorKind::TooManyPoints { limit: 7 },
    );
    let limits = Limits {
        max_glyph_points: 8,
        ..Limits::DEFAULT
    };
    assert_eq!(
        parse_with(&built, limits)
            .outline(GlyphId(1))
            .map(|o| o.point_count()),
        Ok(8)
    );
}

#[test]
fn glyph_ids_past_the_count_are_errors() {
    let built = common::Font::basic().build();
    let font = parse(&built);
    let (maxp, _) = table(&built, b"maxp");
    bad_glyph(
        &font,
        4,
        4,
        maxp + 4,
        ErrorKind::GlyphOutOfRange { found: 4, count: 4 },
    );
    assert_eq!(font.h_metrics(GlyphId(4)), None);
    assert_eq!(font.kerning(GlyphId(4), GlyphId(1)), 0);
}

/// The basic font's glyphs and composites of them: 4 is glyph 1 moved by
/// (10, 20) and glyph 3 by (-5, 300).
fn composites(more: Vec<Vec<u8>>) -> common::Font {
    let mut glyphs = common::Font::basic().glyphs;
    glyphs.push(composite(&[at(1, 10, 20), at(3, -5, 300)], None));
    glyphs.extend(more);
    common::Font::new(glyphs)
}

#[test]
fn composites_place_their_components_by_offset() {
    let font = parse(&composites(vec![]).build());
    assert_eq!(
        points(&font, 4),
        [
            on(10.0, 20.0),
            on(10.0, 120.0),
            on(110.0, 120.0),
            on(110.0, 20.0),
            on(-5.0, 300.0),
            on(45.0, 400.0),
            on(95.0, 300.0),
        ]
    );
    let outline = font.outline(GlyphId(4)).expect("reads");
    assert_eq!(outline.contour_ends(), [4, 7]);
}

#[test]
fn composites_take_word_arguments_and_instructions() {
    let far = Component {
        words: false,
        ..at(1, 1000, -2000)
    };
    let forced = Component {
        words: true,
        ..at(3, 1, 2)
    };
    let font = parse(&composites(vec![composite(&[far, forced], Some(&[0xb0, 0x01]))]).build());
    let read = points(&font, 5);
    assert_eq!(read[0], on(1000.0, -2000.0));
    assert_eq!(read[4], on(1.0, 2.0));
}

#[test]
fn composites_scale_their_components() {
    let half = 8192;
    let scaled = |transform| Component {
        transform,
        ..at(1, 0, 0)
    };
    let font = parse(
        &composites(vec![
            composite(&[scaled(Transform::Scale(half))], None),
            composite(&[scaled(Transform::XY(half, -16_384))], None),
            // x' = x + 0.5·y, y' = 0.25·x + y.
            composite(
                &[scaled(Transform::TwoByTwo([16_384, 4096, half, 16_384]))],
                None,
            ),
        ])
        .build(),
    );
    assert_eq!(points(&font, 5)[2], on(50.0, 50.0));
    assert_eq!(points(&font, 6)[2], on(50.0, -100.0));
    assert_eq!(
        points(&font, 7),
        [
            on(0.0, 0.0),
            on(50.0, 100.0),
            on(150.0, 125.0),
            on(100.0, 25.0)
        ]
    );
}

#[test]
fn offsets_are_scaled_only_when_a_component_asks() {
    let half = 8192;
    let with = |flags| Component {
        transform: Transform::Scale(half),
        flags,
        ..at(1, 100, 40)
    };
    let font = parse(
        &composites(vec![
            composite(&[with(0)], None),
            composite(&[with(0x0800)], None),
            composite(&[with(0x0800 | 0x1000)], None),
            composite(&[with(0x1000)], None),
        ])
        .build(),
    );
    // The square's first point is (0, 0): where it lands is the offset.
    assert_eq!(points(&font, 5)[0], on(100.0, 40.0));
    assert_eq!(points(&font, 6)[0], on(50.0, 20.0));
    assert_eq!(points(&font, 7)[0], on(100.0, 40.0));
    assert_eq!(points(&font, 8)[0], on(100.0, 40.0));
}

#[test]
fn composites_place_components_by_matching_points() {
    // The triangle (glyph 3), then the square (glyph 1) placed so that its
    // point 2, (100, 100), lands on the triangle's point 1, (50, 100).
    let matched = Component {
        placement: Placement::Points(1, 2),
        ..at(1, 0, 0)
    };
    let scaled = Component {
        placement: Placement::Points(1, 2),
        transform: Transform::Scale(8192),
        ..at(1, 0, 0)
    };
    let font = parse(
        &composites(vec![
            composite(&[at(3, 0, 0), matched], None),
            composite(&[at(3, 0, 0), scaled], None),
        ])
        .build(),
    );
    let read = points(&font, 5);
    assert_eq!(
        &read[3..],
        [
            on(-50.0, 0.0),
            on(-50.0, 100.0),
            on(50.0, 100.0),
            on(50.0, 0.0)
        ]
    );
    // Scaled first: its point 2 is (50, 50), moved onto (50, 100).
    let read = points(&font, 6);
    assert_eq!(
        &read[3..],
        [
            on(0.0, 50.0),
            on(0.0, 100.0),
            on(50.0, 100.0),
            on(50.0, 50.0)
        ]
    );
}

#[test]
fn nested_composites_match_their_own_points() {
    // 5 matches points as in the test above; 6 places glyph 1, then 5
    // moved by (1000, 0): 5's matching counts from 5's own first point,
    // not 6's.
    let matched = Component {
        placement: Placement::Points(1, 2),
        ..at(1, 0, 0)
    };
    let font = parse(
        &composites(vec![
            composite(&[at(3, 0, 0), matched], None),
            composite(&[at(1, 0, 0), at(5, 1000, 0)], None),
        ])
        .build(),
    );
    let read = points(&font, 6);
    assert_eq!(read.len(), 4 + 3 + 4);
    assert_eq!(&read[..4], points(&font, 1));
    assert_eq!(
        &read[4..],
        [
            on(1000.0, 0.0),
            on(1050.0, 100.0),
            on(1100.0, 0.0),
            on(950.0, 0.0),
            on(950.0, 100.0),
            on(1050.0, 100.0),
            on(1050.0, 0.0),
        ]
    );
}

#[test]
fn matched_points_must_exist() {
    let bad_parent = Component {
        placement: Placement::Points(3, 0),
        ..at(1, 0, 0)
    };
    let bad_child = Component {
        placement: Placement::Points(0, 4),
        ..at(1, 0, 0)
    };
    let empty_child = Component {
        placement: Placement::Points(0, 0),
        ..at(2, 0, 0)
    };
    let built = composites(vec![
        composite(&[at(3, 0, 0), bad_parent], None),
        composite(&[at(3, 0, 0), bad_child], None),
        composite(&[at(3, 0, 0), empty_child], None),
    ])
    .build();
    let font = parse(&built);
    let (glyf, _) = table(&built, b"glyf");
    let (loca, _) = table(&built, b"loca");
    let start = |glyph: usize| glyf + usize::from(read16(&built, loca + glyph * 2)) * 2;
    // Each composite's second component starts at 10 + 6; its arguments at
    // 4 into it.
    bad_glyph(
        &font,
        5,
        5,
        start(5) + 20,
        ErrorKind::PointOutOfRange { index: 3, count: 3 },
    );
    bad_glyph(
        &font,
        6,
        6,
        start(6) + 21,
        ErrorKind::PointOutOfRange { index: 4, count: 4 },
    );
    bad_glyph(
        &font,
        7,
        7,
        start(7) + 21,
        ErrorKind::PointOutOfRange { index: 0, count: 0 },
    );
}

#[test]
fn composites_nest_and_are_bounded() {
    // 5 holds 4 (depth 2), 6 holds 5 (depth 3); 7 holds itself; 8 holds an
    // unknown glyph.
    let built = composites(vec![
        composite(&[at(4, 1, 1)], None),
        composite(&[at(5, 1, 1), at(1, 0, 0)], None),
        composite(&[at(7, 0, 0)], None),
        composite(&[at(1, 0, 0), at(99, 0, 0)], None),
    ])
    .build();
    let font = parse(&built);
    let nested = points(&font, 6);
    assert_eq!(nested.len(), 11);
    assert_eq!(nested[0], on(12.0, 22.0));
    assert_eq!(nested[7], on(0.0, 0.0));

    let (glyf, _) = table(&built, b"glyf");
    let (loca, _) = table(&built, b"loca");
    let start = |glyph: usize| glyf + usize::from(read16(&built, loca + glyph * 2)) * 2;
    let limits = Limits {
        max_component_depth: 2,
        ..Limits::DEFAULT
    };
    let shallow = parse_with(&built, limits);
    shallow.outline(GlyphId(5)).expect("depth 2");
    // 6's component 5 is depth 2, and 5's component 4 would be depth 3.
    bad_glyph(
        &shallow,
        6,
        5,
        start(5) + 10,
        ErrorKind::TooDeep { limit: 2 },
    );
    bad_glyph(&font, 7, 7, start(7) + 10, ErrorKind::TooDeep { limit: 8 });
    let flat = Limits {
        max_component_depth: 0,
        ..Limits::DEFAULT
    };
    bad_glyph(
        &parse_with(&built, flat),
        4,
        4,
        start(4),
        ErrorKind::TooDeep { limit: 0 },
    );
    // A simple glyph has depth 0.
    parse_with(&built, flat)
        .outline(GlyphId(1))
        .expect("simple");
    bad_glyph(
        &font,
        8,
        8,
        start(8) + 10 + 6 + 2,
        ErrorKind::GlyphOutOfRange {
            found: 99,
            count: 9,
        },
    );
}

#[test]
fn coordinates_beyond_f32_are_refused() {
    // Glyph 1 reaches (32767, 32767); each composite after it wraps the one
    // before in a matrix of -2s, which multiplies its points by -4: the
    // 57th level passes f32::MAX. Allowed only by a raised depth limit.
    let mut glyphs = vec![
        simple(&[&square(500)]),
        simple(&[&[(32767, 32767, true), (32767, 0, true), (0, 0, true)]]),
    ];
    for level in 1..=60u16 {
        glyphs.push(composite(
            &[Component {
                transform: Transform::TwoByTwo([-32768; 4]),
                ..at(level, 0, 0)
            }],
            None,
        ));
    }
    let built = common::Font::new(glyphs).build();
    let limits = Limits {
        max_component_depth: 64,
        ..Limits::DEFAULT
    };
    let font = parse_with(&built, limits);
    let deep = font.outline(GlyphId(57)).expect("level 56 is within f32");
    assert!(
        deep.points()
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
    );
    let (glyf, _) = table(&built, b"glyf");
    let (loca, _) = table(&built, b"loca");
    let start = glyf + usize::from(read16(&built, loca + 58 * 2)) * 2;
    for top in [58, 61] {
        bad_glyph(&font, top, 58, start + 10, ErrorKind::CoordinateOutOfRange);
    }
}

#[test]
fn an_offset_that_takes_a_point_beyond_f32_is_refused() {
    // Glyph 1 reaches ±32,768; 56 levels of -2 matrices take it to about
    // ±1.7e38, still finite. Glyph 58 places glyph 57, then glyph 57 again
    // so that its point 2 lands on the first copy's point 0: an offset of
    // about 3.4e38, with no transform, which takes the second copy's point
    // 0 past f32::MAX.
    let mut glyphs = vec![
        simple(&[&square(500)]),
        simple(&[&[(32767, 32767, true), (0, 0, true), (-32768, -32768, true)]]),
    ];
    for level in 1..=56u16 {
        glyphs.push(composite(
            &[Component {
                transform: Transform::TwoByTwo([-32768; 4]),
                ..at(level, 0, 0)
            }],
            None,
        ));
    }
    let matched = Component {
        placement: Placement::Points(0, 2),
        ..at(57, 0, 0)
    };
    glyphs.push(composite(&[at(57, 0, 0), matched], None));
    let built = common::Font::new(glyphs).build();
    let limits = Limits {
        max_component_depth: 64,
        ..Limits::DEFAULT
    };
    let font = parse_with(&built, limits);
    let level = font.outline(GlyphId(57)).expect("finite");
    assert!(level.points()[0].x > 1e38 && level.points()[2].x < -1e38);
    let (glyf, _) = table(&built, b"glyf");
    let (loca, _) = table(&built, b"loca");
    let start = glyf + usize::from(read16(&built, loca + 58 * 2)) * 2;
    // The second component's record: after the header and the first's 6
    // bytes.
    bad_glyph(
        &font,
        58,
        58,
        start + 10 + 6,
        ErrorKind::CoordinateOutOfRange,
    );
}

#[test]
fn components_are_counted_over_every_level() {
    // 5 holds 4 twice: 2 components, and 2 each inside, 6 in all.
    let built = composites(vec![composite(&[at(4, 0, 0), at(4, 0, 0)], None)]).build();
    let limits = |max_components| Limits {
        max_components,
        ..Limits::DEFAULT
    };
    assert!(parse_with(&built, limits(6)).outline(GlyphId(5)).is_ok());
    let (glyf, _) = table(&built, b"glyf");
    let (loca, _) = table(&built, b"loca");
    let start = |glyph: usize| glyf + usize::from(read16(&built, loca + glyph * 2)) * 2;
    // The sixth is 4's second component, met in the second copy.
    bad_glyph(
        &parse_with(&built, limits(5)),
        5,
        4,
        start(4) + 10 + 6,
        ErrorKind::TooManyComponents { limit: 5 },
    );
    // The point limit counts every component's points.
    let points = Limits {
        max_glyph_points: 13,
        ..Limits::DEFAULT
    };
    bad_glyph(
        &parse_with(&built, points),
        5,
        3,
        start(3) + 10,
        ErrorKind::TooManyPoints { limit: 13 },
    );
}

#[test]
fn composite_data_is_bounds_checked() {
    // Word arguments and a 2 × 2 matrix: flags at 10, the glyph at 12, the
    // arguments at 14 and 16, the matrix from 18 to 26. Glyphs are padded
    // to an even length, so each cut is even.
    let whole = composite(
        &[Component {
            transform: Transform::TwoByTwo([1, 2, 3, 4]),
            words: true,
            ..at(1, 0, 0)
        }],
        None,
    );
    assert_eq!(whole.len(), 26);
    for (cut, at, expected) in [
        (10, 10, "a component's flags"),
        (12, 12, "a component's glyph"),
        (14, 14, "a component's first argument"),
        (16, 16, "a component's second argument"),
        (18, 18, "a component's scale"),
        (24, 24, "a component's scale"),
    ] {
        let built = composites(vec![whole[..cut].to_vec()]).build();
        let (glyf, _) = table(&built, b"glyf");
        let (loca, _) = table(&built, b"loca");
        let start = glyf + usize::from(read16(&built, loca + 5 * 2)) * 2;
        bad_glyph(
            &parse(&built),
            5,
            5,
            start + at,
            ErrorKind::UnexpectedEnd { expected },
        );
    }
}

// Kerning.

#[test]
fn the_kern_table_adds_its_horizontal_subtables() {
    let pairs: &[(u16, u16, i16)] = &[(1, 2, -50), (1, 3, 20), (3, 1, -7)];
    let more: &[(u16, u16, i16)] = &[(1, 2, -5)];
    let replace: &[(u16, u16, i16)] = &[(3, 1, 100)];
    let vertical: &[(u16, u16, i16)] = &[(1, 2, 1000)];
    let minimum: &[(u16, u16, i16)] = &[(1, 2, 2000)];
    let cross: &[(u16, u16, i16)] = &[(1, 2, 3000)];
    let table = kern(&[
        (0x0001, pairs),
        (0x0001, more),
        (0x0009, replace),
        (0x0000, vertical),
        (0x0003, minimum),
        (0x0005, cross),
    ]);
    let font = parse(&common::Font::basic().with(b"kern", table).build());
    let kerning = |left, right| font.kerning(GlyphId(left), GlyphId(right));
    assert_eq!(kerning(1, 2), -55);
    assert_eq!(kerning(1, 3), 20);
    assert_eq!(kerning(3, 1), 100);
    assert_eq!(kerning(2, 1), 0);
    assert_eq!(kerning(0, 0), 0);
}

#[test]
fn the_kern_table_skips_other_formats_and_ignores_apple_s() {
    // A format 2 subtable (its length says how long), then a format 0.
    let mut table = kern(&[(0x0001, &[(1, 2, -50)])]);
    patch(&mut table, 2, 2);
    let mut other = vec![0, 0, 0, 8, 0x02, 0x01, 0xaa, 0xbb];
    other.extend(&table[4..]);
    table.truncate(4);
    table.extend(other);
    let font = parse(&common::Font::basic().with(b"kern", table.clone()).build());
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -50);

    let mut short = table.clone();
    patch(&mut short, 4 + 2, 5);
    let built = common::Font::basic().with(b"kern", short).build();
    let (at, _) = common::table(&built, b"kern");
    rejects(
        &built,
        Some(b"kern"),
        at + 6,
        ErrorKind::BadLength {
            what: "a kern subtable",
        },
    );

    // A format 0 subtable ends after its pairs, whatever its length says
    // (it wraps at 65,536 in large fonts): the second is still found.
    let two = kern(&[
        (0x0001, &[(1, 2, -50), (1, 3, -60)]),
        (0x0001, &[(3, 1, -70)]),
    ]);
    for length in [0, 6, 0xffff] {
        let mut wrong = two.clone();
        patch(&mut wrong, 4 + 2, length);
        let font = parse(&common::Font::basic().with(b"kern", wrong).build());
        assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -60, "{length}");
        assert_eq!(font.kerning(GlyphId(3), GlyphId(1)), -70, "{length}");
    }

    let mut apple = vec![0, 1, 0, 0, 0, 0, 0, 0];
    apple.extend([0xff; 8]);
    let font = parse(&common::Font::basic().with(b"kern", apple).build());
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);

    let mut version = kern(&[]);
    patch(&mut version, 0, 2);
    let built = common::Font::basic().with(b"kern", version).build();
    let (at, _) = common::table(&built, b"kern");
    rejects(
        &built,
        Some(b"kern"),
        at,
        ErrorKind::BadVersion { found: 2 },
    );
}

#[test]
fn the_kern_table_is_checked() {
    let unsorted = kern(&[(0x0001, &[(1, 3, -50), (1, 2, 20)])]);
    let built = common::Font::basic().with(b"kern", unsorted).build();
    let (at, _) = table(&built, b"kern");
    rejects(
        &built,
        Some(b"kern"),
        at + 4 + 14 + 6,
        ErrorKind::NotSorted {
            what: "pairs of a kern subtable",
        },
    );
    let mut many = kern(&[(0x0001, &[(1, 3, -50)])]);
    patch(&mut many, 4 + 6, 2);
    let built = common::Font::basic().with(b"kern", many).build();
    let (at, _) = table(&built, b"kern");
    rejects(
        &built,
        Some(b"kern"),
        at + 18,
        ErrorKind::UnexpectedEnd {
            expected: "a subtable's pairs",
        },
    );
    // A vertical subtable's pairs are not read, so not checked for order.
    let vertical = kern(&[(0x0000, &[(1, 3, -50), (1, 2, 20)])]);
    parse(&common::Font::basic().with(b"kern", vertical).build());
}

/// A `GPOS` table whose script `DFLT` uses feature 0, `kern`, with
/// `lookups`.
fn kern_gpos(lookups: &[(u16, Vec<Vec<u8>>)]) -> Vec<u8> {
    let indices: Vec<u16> = (0..common::u16_of(lookups.len())).collect();
    gpos(&[script(b"DFLT", &[0])], &[(*b"kern", &indices)], lookups)
}

fn gpos_font(table: Vec<u8>) -> Font {
    parse(&common::Font::basic().with(b"GPOS", table).build())
}

#[test]
fn pair_adjustment_format_1_kerns_pairs_of_glyphs() {
    let subtable = pair_pos1(
        &coverage1(&[1, 3]),
        &[&[(1, -10), (2, -20), (3, -30)], &[(1, 15)]],
        0x0004,
        0,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![subtable])]));
    let kerning = |left, right| font.kerning(GlyphId(left), GlyphId(right));
    assert_eq!(kerning(1, 1), -10);
    assert_eq!(kerning(1, 2), -20);
    assert_eq!(kerning(1, 3), -30);
    assert_eq!(kerning(3, 1), 15);
    assert_eq!(kerning(3, 2), 0);
    assert_eq!(kerning(2, 1), 0);
    // A format 2 coverage of two ranges, the second from coverage index 1:
    // glyph 3 takes the second pair set.
    let ranged = pair_pos1(
        &ranges(&[(1, 1, 0), (3, 3, 1)]),
        &[&[(2, -5)], &[(2, -7)]],
        0x0004,
        0,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![ranged])]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -5);
    assert_eq!(font.kerning(GlyphId(3), GlyphId(2)), -7);
    assert_eq!(font.kerning(GlyphId(2), GlyphId(2)), 0);
}

#[test]
fn value_records_skip_their_other_fields() {
    // XPlacement, YPlacement and XAdvance first, every device offset
    // second; the reader takes XAdvance alone.
    // Several records, so that each is found at its stride of 2 + 6 + 8
    // bytes.
    let subtable = pair_pos1(
        &coverage1(&[1]),
        &[&[(1, -41), (2, -42), (3, -43)]],
        0x0007,
        0x00f0,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![subtable])]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(1)), -41);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -42);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -43);
    // The second value record counts in the stride even when the first
    // has XAdvance alone.
    let second = pair_pos1(
        &coverage1(&[1]),
        &[&[(1, -51), (2, -52), (3, -53)]],
        0x0004,
        0x0001,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![second])]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -52);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -53);
    // No XAdvance: no kerning, though the subtable applies.
    let none = pair_pos1(&coverage1(&[1]), &[&[(2, -42)]], 0x0001, 0);
    let fallback = pair_pos1(&coverage1(&[1]), &[&[(2, -9)]], 0x0004, 0);
    let font = gpos_font(kern_gpos(&[(2, vec![none, fallback])]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);
}

#[test]
fn pair_adjustment_format_2_kerns_pairs_of_classes() {
    // Glyphs 1 and 2 in first classes 1 and 2 (3 is covered, unlisted:
    // class 0); second classes by ranges: 1..=2 class 1, 3 class 2.
    let subtable = pair_pos2(
        &ranges(&[(1, 3, 0)]),
        &class_def1(1, &[1, 2]),
        &ranges(&[(1, 2, 1), (3, 3, 2)]),
        &[&[0, -1, -2], &[-10, -11, -12], &[-20, -21, -22]],
        0x0004,
        0x0001,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![subtable])]));
    let kerning = |left, right| font.kerning(GlyphId(left), GlyphId(right));
    assert_eq!(kerning(1, 1), -11);
    assert_eq!(kerning(1, 3), -12);
    assert_eq!(kerning(2, 2), -21);
    // A second glyph in class 0 is not kerned by a format 2 subtable,
    // though a first glyph in class 0 (3, covered) is.
    assert_eq!(kerning(2, 0), 0);
    assert_eq!(kerning(3, 3), -2);
    assert_eq!(kerning(0, 1), 0);
}

#[test]
fn format_1_and_format_2_fall_through_when_they_do_not_apply() {
    let pairs = pair_pos1(&coverage1(&[1]), &[&[(2, -50)]], 0x0004, 0);
    let classes = pair_pos2(
        &coverage1(&[1, 2]),
        &class_def1(1, &[1, 1]),
        &class_def1(1, &[1, 1, 1]),
        &[&[0, 0], &[-5, -7]],
        0x0004,
        0,
    );
    let last = pair_pos1(&coverage1(&[1, 2]), &[&[(3, -99)], &[(0, -98)]], 0x0004, 0);
    let font = gpos_font(kern_gpos(&[(2, vec![pairs, classes, last])]));
    let kerning = |left, right| font.kerning(GlyphId(left), GlyphId(right));
    // Format 1 has the pair.
    assert_eq!(kerning(1, 2), -50);
    // It covers glyph 1 but has no record for 3: format 2 applies.
    assert_eq!(kerning(1, 3), -7);
    // Glyph 0 is in class 0 of format 2's second classes, so format 2 does
    // not apply, and the last subtable kerns the pair.
    assert_eq!(kerning(2, 0), -98);
    assert_eq!(kerning(2, 3), -7);
    // A first glyph in class 0 that is covered is kerned by format 2, and
    // the last subtable is not reached.
    let unlisted_first = pair_pos2(
        &coverage1(&[1, 2]),
        &class_def1(2, &[1]),
        &class_def1(1, &[1, 1, 1]),
        &[&[0, -6], &[0, -8]],
        0x0004,
        0,
    );
    let last = pair_pos1(&coverage1(&[1]), &[&[(3, -99)]], 0x0004, 0);
    let font = gpos_font(kern_gpos(&[(2, vec![unlisted_first, last])]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -6);
}

#[test]
fn a_second_glyph_in_class_0_falls_through_to_a_later_subtable() {
    // As in FreeSerif: a format 2 subtable covers the first glyph but its
    // second class definition does not list the second; a later subtable
    // kerns the pair. HarfBuzz gives the later value, even when class 0's
    // record is not 0.
    for record in [0, -10] {
        let classes = pair_pos2(
            &coverage1(&[1]),
            &class_def1(1, &[1]),
            &class_def1(3, &[1]),
            &[&[0, 0], &[record, -30]],
            0x0004,
            0,
        );
        let later = pair_pos1(&coverage1(&[1]), &[&[(2, -50)]], 0x0004, 0);
        let font = gpos_font(kern_gpos(&[(2, vec![classes, later])]));
        assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -50, "{record}");
        assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -30, "{record}");
    }
}

#[test]
fn several_lookups_add_and_extensions_are_read() {
    let first = pair_pos1(&coverage1(&[1]), &[&[(2, -50)]], 0x0004, 0);
    let second = pair_pos1(&coverage1(&[1]), &[&[(2, -3)]], 0x0004, 0);
    let single = vec![0, 1, 0, 6, 0, 4, 0, 1, 0, 1, 0, 1];
    let font = gpos_font(kern_gpos(&[
        (2, vec![first]),
        (9, vec![extension(2, &second)]),
        (9, vec![extension(1, &single)]),
        (1, vec![single.clone()]),
    ]));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -53);
}

#[test]
fn the_language_system_is_latn_then_dflt_then_the_first() {
    let subtable = |value| pair_pos1(&coverage1(&[1]), &[&[(2, value)]], 0x0004, 0);
    let lookups = [
        (2, vec![subtable(-1)]),
        (2, vec![subtable(-2)]),
        (2, vec![subtable(-4)]),
    ];
    let features: [([u8; 4], &[u16]); 4] = [
        (*b"kern", &[0]),
        (*b"kern", &[1]),
        (*b"kern", &[2]),
        (*b"mark", &[0, 1, 2]),
    ];
    let cases: [(Vec<Script<'_>>, i32); 5] = [
        (
            vec![
                script(b"cyrl", &[2]),
                script(b"DFLT", &[0]),
                script(b"latn", &[1]),
            ],
            -2,
        ),
        (vec![script(b"cyrl", &[2]), script(b"DFLT", &[0])], -1),
        (vec![script(b"cyrl", &[2]), script(b"grek", &[1])], -4),
        // latn without a default language system: DFLT's.
        (
            vec![
                Script {
                    tag: *b"latn",
                    features: None,
                    required: 0xffff,
                },
                script(b"DFLT", &[1, 0]),
            ],
            -3,
        ),
        // The required feature counts.
        (
            vec![Script {
                tag: *b"latn",
                features: Some(&[3]),
                required: 2,
            }],
            -4,
        ),
    ];
    for (scripts, expected) in cases {
        let font = gpos_font(gpos(&scripts, &features, &lookups));
        assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), expected);
    }
}

#[test]
fn latn_is_read_when_dflt_has_fewer_kern_lookups() {
    // As in DejaVu Sans: DFLT's kern feature has only lookup 1, latn's has
    // the Latin pairs of lookup 0 too. Latin text is kerned by latn's.
    let latin = pair_pos1(&coverage1(&[1]), &[&[(2, -100)]], 0x0004, 0);
    let other = pair_pos1(&coverage1(&[3]), &[&[(1, -7)]], 0x0004, 0);
    let table = gpos(
        &[script(b"DFLT", &[0]), script(b"latn", &[1])],
        &[(*b"kern", &[1]), (*b"kern", &[0, 1])],
        &[(2, vec![latin]), (2, vec![other])],
    );
    let font = gpos_font(table);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -100);
    assert_eq!(font.kerning(GlyphId(3), GlyphId(1)), -7);
}

/// A `GPOS` whose `DFLT` language system names `features` feature
/// records, every one a `kern` record leading to one feature table of
/// `lookups` indices, all of lookup 0, which kerns (1, 2) by -9.
fn aliased_gpos(features: usize, lookups: usize) -> Vec<u8> {
    use common::push16;
    let subtable = pair_pos1(&coverage1(&[1]), &[&[(2, -9)]], 0x0004, 0);
    let language = 22;
    let lookup_list = language + 6 + 2 * features;
    let feature_list = lookup_list + 12 + subtable.len();
    let mut out = Vec::new();
    for value in [
        1,
        0,
        10,
        common::u16_of(feature_list),
        common::u16_of(lookup_list),
    ] {
        push16(&mut out, value);
    }
    // The script list at 10, DFLT's script at 18, its language system at
    // 22.
    push16(&mut out, 1);
    out.extend(*b"DFLT");
    push16(&mut out, 8);
    push16(&mut out, 4);
    push16(&mut out, 0);
    push16(&mut out, 0);
    push16(&mut out, 0xffff);
    push16(&mut out, common::u16_of(features));
    for index in 0..features {
        push16(&mut out, common::u16_of(index));
    }
    // One lookup of one subtable.
    assert_eq!(out.len(), lookup_list);
    for value in [1, 4, 2, 0, 1, 8] {
        push16(&mut out, value);
    }
    out.extend(&subtable);
    // The feature records, all leading to the table after them.
    assert_eq!(out.len(), feature_list);
    push16(&mut out, common::u16_of(features));
    for _ in 0..features {
        out.extend(*b"kern");
        push16(&mut out, common::u16_of(2 + 6 * features));
    }
    push16(&mut out, 0);
    push16(&mut out, common::u16_of(lookups));
    out.extend(std::iter::repeat_n(0, 2 * lookups));
    out
}

#[test]
fn a_feature_named_many_times_is_read_once() {
    // 10,000 feature records of one table of 30,000 lookup indices: read
    // for each record, that would be 3 × 10^8 indices.
    let table = aliased_gpos(10_000, 30_000);
    let started = std::time::Instant::now();
    let font = gpos_font(table);
    let elapsed = started.elapsed();
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -9);
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
}

#[test]
fn a_language_system_that_is_also_a_feature_s_lookups_is_refused() {
    // The language system names feature 0 65,535 times, and feature 0's
    // lookup indices are the same 65,535 zeros: the bytes are read twice,
    // more than the table holds.
    let mut table = Vec::new();
    for value in [1, 0, 10, 18, 26] {
        common::push16(&mut table, value);
    }
    common::push16(&mut table, 1);
    table.extend(*b"DFLT");
    common::push16(&mut table, 18);
    common::push16(&mut table, 1);
    table.extend(*b"kern");
    common::push16(&mut table, 16);
    common::push16(&mut table, 0);
    // The script at 28, its language system at 32; the feature at 34
    // shares the language system's count and indices.
    common::push16(&mut table, 4);
    common::push16(&mut table, 0);
    common::push16(&mut table, 0);
    common::push16(&mut table, 0xffff);
    common::push16(&mut table, 0xffff);
    table.extend(std::iter::repeat_n(0, 2 * 0xffff));
    let started = std::time::Instant::now();
    bad_gpos(table, 38, ErrorKind::OverlappingStructures);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn gpos_without_a_kern_feature_leaves_kerning_to_the_kern_table() {
    let subtable = pair_pos1(&coverage1(&[1]), &[&[(2, -50)]], 0x0004, 0);
    let no_kern = gpos(
        &[script(b"DFLT", &[0])],
        &[(*b"mark", &[0])],
        &[(2, vec![subtable.clone()])],
    );
    let kern_table = kern(&[(0x0001, &[(1, 2, -7)])]);
    let font = parse(
        &common::Font::basic()
            .with(b"GPOS", no_kern)
            .with(b"kern", kern_table.clone())
            .build(),
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -7);
    let no_scripts = gpos(&[], &[(*b"kern", &[0])], &[(2, vec![subtable.clone()])]);
    let font = gpos_font(no_scripts);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);
    // With a kern feature, GPOS wins.
    let font = parse(
        &common::Font::basic()
            .with(b"GPOS", kern_gpos(&[(2, vec![subtable])]))
            .with(b"kern", kern_table)
            .build(),
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -50);
}

/// Builds a font with `table` as its `GPOS` and expects `kind` at `at`
/// into the table.
#[track_caller]
fn bad_gpos(table: Vec<u8>, at: usize, kind: ErrorKind) {
    let built = common::Font::basic().with(b"GPOS", table).build();
    let (start, _) = common::table(&built, b"GPOS");
    rejects(&built, Some(b"GPOS"), start + at, kind);
}

#[test]
fn gpos_is_checked() {
    let subtable = pair_pos1(&coverage1(&[1]), &[&[(2, -50)]], 0x0004, 0);
    let good = kern_gpos(&[(2, vec![subtable.clone()])]);
    let mut version = good.clone();
    patch(&mut version, 0, 2);
    bad_gpos(version, 0, ErrorKind::BadVersion { found: 2 });

    // A feature index past the feature list, and a lookup index past the
    // lookup list. The script list is 2 + 6 + 10 bytes from 10; its
    // language system's first feature index is at 10 + 8 + 4 + 6.
    let features = gpos(
        &[script(b"DFLT", &[1])],
        &[(*b"kern", &[0])],
        &[(2, vec![subtable.clone()])],
    );
    bad_gpos(
        features,
        28,
        ErrorKind::IndexOutOfRange {
            what: "feature",
            index: 1,
            count: 1,
        },
    );
    let lookups = gpos(
        &[script(b"DFLT", &[0])],
        &[(*b"kern", &[1])],
        &[(2, vec![subtable.clone()])],
    );
    // The feature list starts at 30: 2 + 6 bytes of records, then the
    // feature, whose first lookup index is 4 into it.
    bad_gpos(
        lookups,
        42,
        ErrorKind::IndexOutOfRange {
            what: "lookup",
            index: 1,
            count: 1,
        },
    );

    let ext = kern_gpos(&[(9, vec![extension(2, &subtable)])]);
    // The lookup list starts after the feature list (30 + 8 + 6), and its
    // lookup after 4 bytes: the extension at 6 + 2 into that.
    let lookup_list = 30 + 8 + 6;
    let extension_at = lookup_list + 4 + 8;
    let mut bad = ext.clone();
    patch(&mut bad, extension_at, 2);
    bad_gpos(
        bad,
        extension_at,
        ErrorKind::BadFormat {
            what: "an extension subtable",
            found: 2,
        },
    );
    let subtable_at = lookup_list + 4 + 8;
    let mut format = good.clone();
    patch(&mut format, subtable_at, 3);
    bad_gpos(
        format,
        subtable_at,
        ErrorKind::BadFormat {
            what: "a pair adjustment subtable",
            found: 3,
        },
    );
}

#[test]
fn coverage_and_class_definitions_are_checked() {
    let lookup_list = 30 + 8 + 6;
    let subtable_at = lookup_list + 4 + 8;
    let bad = |subtable: Vec<u8>, at: usize, kind| {
        bad_gpos(kern_gpos(&[(2, vec![subtable])]), subtable_at + at, kind)
    };
    // Format 1's coverage follows its 10 + 2 bytes of header.
    // With two pair sets, the coverage starts at 14.
    let mut unsorted = coverage1(&[2, 1]);
    bad(
        pair_pos1(&unsorted, &[&[], &[]], 0x0004, 0),
        14 + 6,
        ErrorKind::NotSorted {
            what: "glyphs of a coverage table",
        },
    );
    patch(&mut unsorted, 0, 3);
    bad(
        pair_pos1(&unsorted, &[&[], &[]], 0x0004, 0),
        14,
        ErrorKind::BadFormat {
            what: "a coverage table",
            found: 3,
        },
    );
    bad(
        pair_pos1(&ranges(&[(2, 1, 0)]), &[&[]], 0x0004, 0),
        12 + 4,
        ErrorKind::BackwardRange {
            what: "a coverage table",
        },
    );
    bad(
        pair_pos1(&ranges(&[(1, 2, 0), (2, 3, 2)]), &[&[]], 0x0004, 0),
        12 + 10,
        ErrorKind::NotSorted {
            what: "ranges of a coverage table",
        },
    );
    // A pair set whose second glyphs go down; it follows the coverage.
    bad(
        pair_pos1(&coverage1(&[1]), &[&[(3, -1), (2, -1)]], 0x0004, 0),
        12 + 6 + 2 + 4,
        ErrorKind::NotSorted {
            what: "second glyphs of a pair set",
        },
    );
    let mut cut = pair_pos1(&coverage1(&[1]), &[&[(3, -1)]], 0x0004, 0);
    let set_at = 12 + 6;
    patch(&mut cut, set_at, 9);
    bad(
        cut,
        set_at + 2,
        ErrorKind::UnexpectedEnd {
            expected: "a pair set's records",
        },
    );

    // Format 2: its records (2 × 1 values of 2 bytes) follow the 16 bytes
    // of header, then the coverage (6 bytes), then the class definitions.
    let classes = |class1: &[u8], class2: &[u8]| {
        pair_pos2(&coverage1(&[1]), class1, class2, &[&[0], &[0]], 0x0004, 0)
    };
    let mut format = class_def1(1, &[1]);
    patch(&mut format, 0, 3);
    bad(
        classes(&format, &class_def1(1, &[0])),
        16 + 4 + 6,
        ErrorKind::BadFormat {
            what: "a class definition",
            found: 3,
        },
    );
    bad(
        classes(&ranges(&[(1, 1, 1), (1, 2, 1)]), &class_def1(1, &[0])),
        16 + 4 + 6 + 4 + 6,
        ErrorKind::NotSorted {
            what: "ranges of a class definition",
        },
    );
    bad(
        classes(&class_def1(1, &[1]), &ranges(&[(5, 4, 0)])),
        16 + 4 + 6 + 8 + 4,
        ErrorKind::BackwardRange {
            what: "a class definition",
        },
    );
    let mut records = classes(&class_def1(1, &[1]), &class_def1(1, &[0]));
    patch(&mut records, 12, 200);
    bad(
        records,
        16,
        ErrorKind::UnexpectedEnd {
            expected: "the class records",
        },
    );
}

#[test]
fn class_and_coverage_indices_past_their_arrays_do_not_apply() {
    // The coverage gives glyph 2 index 5, past the one pair set; the class
    // definitions give classes past the counts.
    let pairs = pair_pos1(&ranges(&[(1, 2, 0)]), &[&[(1, -1)]], 0x0004, 0);
    let classes = pair_pos2(
        &coverage1(&[1, 2, 3]),
        &class_def1(1, &[0, 5, 0]),
        &class_def1(1, &[1, 1, 9]),
        &[&[0, -3]],
        0x0004,
        0,
    );
    let fallback = pair_pos1(
        &coverage1(&[1, 2, 3]),
        &[&[(1, -7), (3, -7)], &[(1, -8)], &[(3, -9)]],
        0x0004,
        0,
    );
    let font = gpos_font(kern_gpos(&[(2, vec![pairs, classes, fallback])]));
    let kerning = |left, right| font.kerning(GlyphId(left), GlyphId(right));
    assert_eq!(kerning(1, 1), -1);
    // Glyph 2: index 1 in the format 1 subtable's coverage, past its one
    // pair set; class 5 in format 2, past its one class.
    assert_eq!(kerning(2, 1), -8);
    // Glyph 3 with 3: second class 9, past the count of 2.
    assert_eq!(kerning(3, 3), -9);
    assert_eq!(kerning(1, 2), -3);
}

#[test]
fn a_lookup_table_named_twice_is_read_once_and_applied_twice() {
    // Lookups 0 and 1 are at one offset, with 200 subtables that are all
    // one: the table is read once, and kept as the one subtable, which the
    // two lookup indices each apply.
    let subtable = pair_pos1(&coverage1(&[1]), &[&[(2, -1)]], 0x0004, 0);
    let mut list = Vec::new();
    for value in [2, 6, 6, 2, 0, 200] {
        common::push16(&mut list, value);
    }
    for _ in 0..200 {
        common::push16(&mut list, 6 + 400);
    }
    list.extend(&subtable);
    let one_entry = Limits {
        max_kerning_subtables: 1,
        ..Limits::DEFAULT
    };
    let both = common::gpos_raw(&[script(b"DFLT", &[0])], &[(*b"kern", &[0, 1])], &list);
    let font = parse_with(
        &common::Font::basic().with(b"GPOS", both).build(),
        one_entry,
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -2);
    let one = common::gpos_raw(&[script(b"DFLT", &[0])], &[(*b"kern", &[0])], &list);
    assert_eq!(gpos_font(one).kerning(GlyphId(1), GlyphId(2)), -1);
}

#[test]
fn lookup_indices_that_alias_one_table_apart_count_it_once() {
    // The kern feature names lookups 0, 1 and 2; 0 and 2 are one table A,
    // (1, 2) by -1, and 1 is table B, by -10. Two entries are kept, A's and
    // B's, and A applies twice.
    let a = pair_pos1(&coverage1(&[1]), &[&[(2, -1)]], 0x0004, 0);
    let b = pair_pos1(&coverage1(&[1]), &[&[(2, -10)]], 0x0004, 0);
    let two = common::lookup_list(&[(2, vec![a]), (2, vec![b])]);
    // Three offsets instead of two: the header grows by 2 bytes, and every
    // lookup moves 2 bytes further.
    let (first, second) = (read16(&two, 2) + 2, read16(&two, 4) + 2);
    let mut list = Vec::new();
    for value in [3, first, second, first] {
        common::push16(&mut list, value);
    }
    list.extend(&two[6..]);
    let table = common::gpos_raw(&[script(b"latn", &[0])], &[(*b"kern", &[0, 1, 2])], &list);
    let two_entries = Limits {
        max_kerning_subtables: 2,
        ..Limits::DEFAULT
    };
    let font = parse_with(
        &common::Font::basic().with(b"GPOS", table).build(),
        two_entries,
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -12);
}

#[test]
fn a_padded_table_of_aliased_lookups_keeps_one_entry() {
    // The review's shape: latn's kern feature names lookups 0 to 1,031,
    // every one the same table of 32,000 offsets to one subtable, and the
    // table is padded so the read budget would allow much more. One entry
    // is kept, and each call tries one subtable.
    let subtable = pair_pos1(&coverage1(&[1]), &[&[(3, -2)]], 0x0004, 0);
    let (lookups, offsets) = (1032usize, 32_000usize);
    let lookup_at = 2 + 2 * lookups;
    let mut list = Vec::new();
    common::push16(&mut list, common::u16_of(lookups));
    for _ in 0..lookups {
        common::push16(&mut list, common::u16_of(lookup_at));
    }
    for value in [2, 0, common::u16_of(offsets)] {
        common::push16(&mut list, value);
    }
    for _ in 0..offsets {
        common::push16(&mut list, common::u16_of(6 + 2 * offsets));
    }
    list.extend(&subtable);
    let indices: Vec<u16> = (0..common::u16_of(lookups)).collect();
    let mut table = common::gpos_raw(&[script(b"latn", &[0])], &[(*b"kern", &indices)], &list);
    table.resize(table.len() + (1 << 20), 0);
    let one_entry = Limits {
        max_kerning_subtables: 1,
        ..Limits::DEFAULT
    };
    let started = std::time::Instant::now();
    let font = parse_with(
        &common::Font::basic().with(b"GPOS", table).build(),
        one_entry,
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -2 * 1032);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn a_pair_set_shared_by_many_glyphs_is_checked_once() {
    // 100 covered glyphs all lead to one pair set of 100 records: charged
    // for each, the checks would read 100 times the pair set, far more than
    // the table holds. Checked once, the font parses and kerns.
    use common::{push_i16, push16, u16_of};
    let glyphs = 101;
    let header = 10 + 2 * glyphs;
    let coverage = header;
    let set = coverage + 4 + 2 * glyphs;
    let mut subtable = Vec::new();
    for value in [1, u16_of(coverage), 0x0004, 0, u16_of(glyphs)] {
        push16(&mut subtable, value);
    }
    for _ in 0..glyphs {
        push16(&mut subtable, u16_of(set));
    }
    push16(&mut subtable, 1);
    push16(&mut subtable, u16_of(glyphs));
    for glyph in 0..glyphs {
        push16(&mut subtable, u16_of(glyph));
    }
    push16(&mut subtable, u16_of(glyphs));
    for second in 0..glyphs {
        push16(&mut subtable, u16_of(second));
        push_i16(&mut subtable, -i16::try_from(second).expect("small"));
    }
    let table = kern_gpos(&[(2, vec![subtable])]);
    assert!(100 * 400 > table.len(), "{}", table.len());
    let font = parse(
        &common::Font::new(vec![simple(&[&square(500)]); glyphs])
            .with(b"GPOS", table)
            .build(),
    );
    assert_eq!(font.kerning(GlyphId(1), GlyphId(50)), -50);
    assert_eq!(font.kerning(GlyphId(77), GlyphId(100)), -100);
}

#[test]
fn a_shared_pair_set_is_checked_again_for_another_record_size() {
    // Two subtables share a pair set of 2 records at the table's end: read
    // with records of 4 bytes it fits, with records of 6 it does not.
    use common::push16;
    let mut list = Vec::new();
    // The lookup list, its lookup at 4: two subtables at 10 and 22, the
    // shared coverage at 34 and the pair set at 40, from the lookup.
    for value in [1, 4, 2, 0, 2, 10, 22] {
        push16(&mut list, value);
    }
    for value in [1, 24, 0x0004, 0, 1, 30] {
        push16(&mut list, value);
    }
    for value in [1, 12, 0x0004, 0x0001, 1, 18] {
        push16(&mut list, value);
    }
    for value in [1, 1, 1] {
        push16(&mut list, value);
    }
    for value in [2, 2, 0xfffb, 3, 0xfffa] {
        push16(&mut list, value);
    }
    let table = common::gpos_raw(&[script(b"latn", &[0])], &[(*b"kern", &[0])], &list);
    let set = usize::from(read16(&table, 8)) + 4 + 40;
    assert_eq!(set + 10, table.len());
    bad_gpos(
        table,
        set + 2,
        ErrorKind::UnexpectedEnd {
            expected: "a pair set's records",
        },
    );
}

#[test]
fn each_lookup_keeps_its_own_order_of_shared_subtables() {
    // Lookup 0 is [S1, S2] and lookup 1 is [S2, S1]: each applies its own
    // first, -1 and -10, as HarfBuzz does.
    use common::push16;
    let first = pair_pos1(&coverage1(&[1]), &[&[(2, -1)]], 0x0004, 0);
    let second = pair_pos1(&coverage1(&[1]), &[&[(2, -10)]], 0x0004, 0);
    let (lookup0, lookup1, subtables) = (6usize, 16usize, 26usize);
    let mut list = Vec::new();
    for value in [2, lookup0, lookup1] {
        push16(&mut list, common::u16_of(value));
    }
    let s1 = subtables;
    let s2 = subtables + first.len();
    for (lookup, order) in [(lookup0, [s1, s2]), (lookup1, [s2, s1])] {
        for value in [2, 0, 2] {
            push16(&mut list, value);
        }
        for at in order {
            push16(&mut list, common::u16_of(at - lookup));
        }
    }
    assert_eq!(list.len(), subtables);
    list.extend(&first);
    list.extend(&second);
    let table = common::gpos_raw(&[script(b"latn", &[0])], &[(*b"kern", &[0, 1])], &list);
    assert_eq!(gpos_font(table).kerning(GlyphId(1), GlyphId(2)), -11);
}

#[test]
fn the_kern_table_s_subtables_are_bounded_too() {
    // Three horizontal subtables are read, and a vertical one is skipped
    // and not counted.
    let four = kern(&[
        (0x0001, &[(1, 2, -1)]),
        (0x0000, &[(1, 2, -50)]),
        (0x0001, &[(1, 3, -2)]),
        (0x0001, &[(2, 1, -3)]),
    ]);
    let built = common::Font::basic().with(b"kern", four).build();
    let limits = |max_kerning_subtables| Limits {
        max_kerning_subtables,
        ..Limits::DEFAULT
    };
    let font = parse_with(&built, limits(3));
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), -1);
    assert_eq!(font.kerning(GlyphId(2), GlyphId(1)), -3);
    // The third read, the fourth subtable, is past a limit of 2: at its
    // header, after the table's 4 bytes and three subtables of 20.
    let (at, _) = table(&built, b"kern");
    rejects_with(
        &built,
        limits(2),
        Some(b"kern"),
        at + 4 + 3 * 20,
        ErrorKind::TooManyKerningSubtables { limit: 2 },
    );
    // The message names no `GPOS` lookups.
    let error = Font::parse(built, limits(2)).expect_err("over the limit");
    assert_eq!(
        error.to_string(),
        format!(
            "TrueType error in table `kern` at byte {}: the kerning reads more subtables than \
             the limit of 2",
            at + 4 + 3 * 20
        )
    );
    // The limit is checked before the subtable's own checks: a third
    // subtable whose pairs are out of order is past a limit of 2 first.
    let unsorted = kern(&[
        (0x0001, &[(1, 2, -1)]),
        (0x0001, &[(1, 3, -2)]),
        (0x0001, &[(2, 3, -1), (2, 1, -2)]),
    ]);
    let built = common::Font::basic().with(b"kern", unsorted).build();
    let (at, _) = table(&built, b"kern");
    rejects_with(
        &built,
        limits(2),
        Some(b"kern"),
        at + 4 + 2 * 20,
        ErrorKind::TooManyKerningSubtables { limit: 2 },
    );
    rejects_with(
        &built,
        limits(3),
        Some(b"kern"),
        at + 4 + 2 * 20 + 14 + 6,
        ErrorKind::NotSorted {
            what: "pairs of a kern subtable",
        },
    );
}

#[test]
fn the_kerning_subtables_kept_are_bounded() {
    let limits = |max_kerning_subtables| Limits {
        max_kerning_subtables,
        ..Limits::DEFAULT
    };
    let five = common::gpos_raw(
        &[script(b"latn", &[0])],
        &[(*b"kern", &[0])],
        &common::distinct_subtables(5),
    );
    let built = common::Font::basic().with(b"GPOS", five.clone()).build();
    let font = parse_with(&built, limits(5));
    // Every subtable covers glyph 1; none has a pair for glyph 2.
    assert_eq!(font.kerning(GlyphId(1), GlyphId(3)), -4);
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);
    // The fifth is one too many: at its offset, in the lookup that starts
    // 4 bytes into the lookup list.
    let (start, _) = table(&built, b"GPOS");
    let lookup = start + usize::from(read16(&five, 8)) + 4;
    rejects_with(
        &built,
        limits(4),
        Some(b"GPOS"),
        lookup + 6 + 2 * 4,
        ErrorKind::TooManyKerningSubtables { limit: 4 },
    );
    // The limit is checked before the subtable's own checks: a fifth
    // subtable of format 3 is past a limit of 4 first. It starts 20 bytes
    // into the lookup list (4 of the list, 6 of the lookup, 10 of offsets,
    // then 12 a subtable).
    let mut list = common::distinct_subtables(5);
    patch(&mut list, 20 + 12 * 4, 3);
    let bad_fifth = common::gpos_raw(&[script(b"latn", &[0])], &[(*b"kern", &[0])], &list);
    let built = common::Font::basic()
        .with(b"GPOS", bad_fifth.clone())
        .build();
    let (start, _) = table(&built, b"GPOS");
    let lookup = start + usize::from(read16(&bad_fifth, 8)) + 4;
    rejects_with(
        &built,
        limits(4),
        Some(b"GPOS"),
        lookup + 6 + 2 * 4,
        ErrorKind::TooManyKerningSubtables { limit: 4 },
    );
    rejects_with(
        &built,
        limits(5),
        Some(b"GPOS"),
        lookup + 20 - 4 + 12 * 4,
        ErrorKind::BadFormat {
            what: "a pair adjustment subtable",
            found: 3,
        },
    );
    // At the default, a font of as many distinct subtables parses, and a
    // call that tries them all is quick.
    let most = Limits::DEFAULT.max_kerning_subtables;
    let many = common::gpos_raw(
        &[script(b"latn", &[0])],
        &[(*b"kern", &[0])],
        &common::distinct_subtables(most),
    );
    let font = gpos_font(many);
    let started = std::time::Instant::now();
    assert_eq!(font.kerning(GlyphId(1), GlyphId(2)), 0);
    assert!(started.elapsed().as_secs() < 1);
}

// Every kind.

#[test]
fn every_error_kind_is_reached() {
    // The tests above run on other threads; run them here too, in this
    // thread, so that what they see is counted.
    a_basic_font_parses_and_reports_its_headers();
    the_sfnt_version_must_be_truetype();
    the_file_size_is_checked_first();
    the_directory_must_fit_and_hold_each_table_once();
    every_required_table_must_be_there();
    head_is_checked();
    maxp_and_hhea_are_checked();
    hmtx_gives_the_last_advance_to_the_glyphs_after_it();
    os2_is_read_as_long_as_its_version_says();
    loca_is_checked_whole();
    cmap_needs_a_unicode_subtable();
    cmap_subtables_are_checked();
    names_are_checked();
    simple_glyphs_are_checked();
    the_point_limit_is_checked_before_the_points_are_read();
    a_legacy_version_0_os2_of_68_bytes_has_no_line_metrics();
    coordinates_beyond_f32_are_refused();
    a_language_system_that_is_also_a_feature_s_lookups_is_refused();
    the_kerning_subtables_kept_are_bounded();
    a_shared_pair_set_is_checked_again_for_another_record_size();
    the_kern_table_s_subtables_are_bounded_too();
    glyph_ids_past_the_count_are_errors();
    matched_points_must_exist();
    composites_nest_and_are_bounded();
    components_are_counted_over_every_level();
    composite_data_is_bounds_checked();
    the_kern_table_skips_other_formats_and_ignores_apple_s();
    the_kern_table_is_checked();
    gpos_is_checked();
    coverage_and_class_definitions_are_checked();
    a_lookup_table_named_twice_is_read_once_and_applied_twice();
    let seen = SEEN.with(|seen| seen.borrow().clone());
    assert_eq!(seen.len(), ALL_KINDS, "{seen:?}");
}

#[test]
fn errors_display_their_place() {
    let font = common::Font::basic().without(b"glyf").build();
    let error = Font::parse(font, Limits::DEFAULT).expect_err("no glyf");
    assert_eq!(
        error.to_string(),
        "TrueType error in table `glyf` at byte 108: the table is missing, and it is required"
    );
}
