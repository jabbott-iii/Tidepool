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

//! The fuzz target of `tidepool-font`: `ttf`, one test named after it
//! (ADR-0027, `docs/04-quality.md` → "Fuzz smoke").
//!
//! A plain `cargo test` runs its saved crashes (`fuzz/crashes/ttf/`) and
//! its seeds (`fuzz/corpus/ttf/`, small fonts made by hand with
//! `tests/common`'s builder; none is copied from the bundled faces)
//! unchanged. `cargo xtask fuzz-smoke` runs them with mutated inputs too,
//! in an optimized profile with overflow checks.
//!
//! The target parses with small [`Limits`] ([`LIMITS`]), and among its
//! seeds are fonts that sit exactly at each limit (`edge-*`), so that one
//! mutation takes an input over it and into the check. Beyond "no panic",
//! it checks what [`FontError`] and the module docs promise:
//!
//! - a parse error is a [`FontError::Ttf`] whose offset is in the input:
//!   for the file's own kinds, where the kind says (the limit for a file
//!   too large, 0 for a version); for a table's, inside the range its
//!   directory record gives, or at a directory record (or its end) for
//!   the directory's kinds; and a limit's error names that limit;
//! - in a font that parses, for its first 64 glyphs and its last: the
//!   metrics exist; the outline reads, with at most [`LIMITS`]' points, its
//!   contours ending at its point count, or fails with a
//!   [`FontError::Glyph`] in the input, naming a glyph that exists (and a
//!   limit of [`LIMITS`] for a limit's kind); and each outline rasterizes
//!   at two scales and shifts into a bitmap within the limit, of
//!   `width × height` bytes whose four edges are those of the scaled
//!   control box rounded outwards, the same twice, or fails for the
//!   bitmap's size, its position or the work limit alone;
//! - characters map only to glyphs that exist and are not `.notdef`;
//!   kerning is 0 for glyph ids past the count; names and `OS/2` read.

use tidepool_core::testing::fuzz::{Outcome, Target};
use tidepool_font::outline::{self, Outline};
use tidepool_font::ttf::{ErrorKind, Font, GlyphId, NameId, Tag};
use tidepool_font::{FontError, Limits};

/// The repository root, where `fuzz/` is.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The limits the target parses with: 4 KiB of file, 256 points, a depth
/// of 4, 64 components, bitmaps of 64 pixels a side, 4,096 steps of
/// rasterizing and 8 kerning subtables, so that a mutated font is either
/// over a limit or reads in microseconds. Every seed is at or under each, and the `edge-*` seeds
/// exactly at one (the other seeds' glyphs take at most 1,984 steps).
/// Built from the defaults, so that a field added later keeps its
/// default.
const LIMITS: Limits = {
    let mut limits = Limits::DEFAULT;
    limits.max_file_bytes = 4 * 1024;
    limits.max_glyph_points = 256;
    limits.max_component_depth = 4;
    limits.max_components = 64;
    limits.max_bitmap_dimension = 64;
    limits.max_raster_work = 4096;
    limits.max_kerning_subtables = 8;
    limits
};

/// The sizes, in pixels per em, and shifts each glyph is rasterized at;
/// the last is the largest, which `edge-bitmap.ttf` and `edge-work.ttf`
/// are made for.
const RASTERS: [(f32, f32); 2] = [(12.0, 0.0), (40.0, 0.75)];

/// How many glyphs from the first are read, besides the last.
const GLYPHS: u16 = 64;

/// The cap on the rise of the written peak, as for codec's targets.
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

/// A run's result line for `fuzz-smoke`, then the verdict.
#[expect(
    clippy::print_stdout,
    reason = "`cargo xtask fuzz-smoke` reads the result line from the test's output; libtest \
              shows it only with --nocapture or when the test fails"
)]
fn report(outcome: &Outcome) {
    println!("{}", outcome.summary());
    outcome.assert_clean();
}

/// The sfnt's structure: its versions, the tags of the tables read and of
/// scripts and features, `head`'s magic, format numbers, and values on the
/// edges of 16 bits and of the limits.
const TOKENS: &[&[u8]] = &[
    b"\0\x01\0\0",
    b"true",
    b"OTTO",
    b"ttcf",
    b"head",
    b"hhea",
    b"maxp",
    b"hmtx",
    b"cmap",
    b"loca",
    b"glyf",
    b"kern",
    b"GPOS",
    b"OS/2",
    b"name",
    b"post",
    b"DFLT",
    b"latn",
    b"mark",
    b"\x5f\x0f\x3c\xf5",
    b"\x03\xe8",
    b"\0\x03\0\x01",
    b"\0\x03\0\x0a",
    b"\0\0\0\x04",
    b"\0\x04",
    b"\0\x0c",
    b"\0\x02",
    b"\0\x09",
    b"\xff\xff",
    b"\xff\xfe",
    b"\x7f\xff",
    b"\x80\0",
    b"\0\0",
    b"\0\x01",
    b"\x01\0",
    b"\x40\0",
    b"\xc0\0",
    b"\x08\xff",
    b"\x37",
    b"\0\x22",
    b"\x0f\xff",
    b"\0\0\x10\0",
];

/// `ttf`: [`Font::parse`], then reads and rasterizes what it can (see the
/// module docs).
fn parse_ttf(input: &[u8]) {
    match Font::parse(input.to_vec(), LIMITS) {
        Ok(font) => check_font(input, &font),
        Err(error) => check_parse_error(input, &error),
    }
}

/// The big-endian `u16` and `u32` at `at`, if the input holds them.
fn be16(input: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        input.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn be32(input: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        input.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// The directory records of the input, as (tag, record offset, start,
/// end), so far as it holds them.
fn records(input: &[u8]) -> Vec<(Tag, usize, u64, u64)> {
    let count = be16(input, 4).map_or(0, usize::from);
    (0..count)
        .filter_map(|index| {
            let at = 12 + index * 16;
            let tag = Tag(input.get(at..at + 4)?.try_into().ok()?);
            let start = u64::from(be32(input, at + 8)?);
            let length = u64::from(be32(input, at + 12)?);
            Some((tag, at, start, start + length))
        })
        .collect()
}

/// Checks that a limit's error names that limit of [`LIMITS`].
fn check_limit(kind: &ErrorKind) {
    let fine = match kind {
        ErrorKind::FileTooLarge { limit } => *limit == LIMITS.max_file_bytes,
        ErrorKind::TooManyPoints { limit } => *limit == LIMITS.max_glyph_points,
        ErrorKind::TooDeep { limit } => *limit == LIMITS.max_component_depth,
        ErrorKind::TooManyComponents { limit } => *limit == LIMITS.max_components,
        ErrorKind::TooManyKerningSubtables { limit } => *limit == LIMITS.max_kerning_subtables,
        _ => true,
    };
    assert!(fine, "a limit's error names another limit: {kind:?}");
}

/// Checks that a parse error is in the input, where its kind and table
/// say.
fn check_parse_error(input: &[u8], error: &FontError) {
    let FontError::Ttf {
        table,
        offset,
        kind,
    } = error
    else {
        panic!("a parse failed with a {error:?}");
    };
    let offset = *offset;
    assert!(
        offset <= input.len(),
        "a parse error at offset {offset} of {} bytes: {error}",
        input.len()
    );
    check_limit(kind);
    if input.len() > LIMITS.max_file_bytes {
        assert!(
            matches!(kind, ErrorKind::FileTooLarge { .. }) && offset == LIMITS.max_file_bytes,
            "a file over the limit failed with {error}"
        );
        return;
    }
    let version = input.get(..4);
    let fine = match kind {
        ErrorKind::FileTooLarge { .. } => false,
        ErrorKind::Cff => offset == 0 && version == Some(b"OTTO"),
        ErrorKind::Collection => offset == 0 && version == Some(b"ttcf"),
        ErrorKind::BadSfntVersion { found } => offset == 0 && be32(input, 0) == Some(*found),
        _ => true,
    };
    assert!(fine, "{error}");
    let records = records(input);
    let directory_end = 12 + records.len() * 16;
    let Some(tag) = table else {
        // The file's own errors are in its header and directory.
        assert!(offset <= directory_end, "{error}");
        return;
    };
    let mine: Vec<_> = records.iter().filter(|record| record.0 == *tag).collect();
    let fine = match kind {
        ErrorKind::MissingTable => mine.is_empty() && offset == directory_end,
        ErrorKind::TableOutOfBounds => mine
            .iter()
            .any(|record| record.1 == offset && record.3 > input.len() as u64),
        ErrorKind::DuplicateTable => {
            mine.len() > 1 && mine.iter().skip(1).any(|record| record.1 == offset)
        }
        // Anything else is inside the table, as its first record has it.
        _ => mine
            .first()
            .is_some_and(|record| record.2 <= offset as u64 && offset as u64 <= record.3),
    };
    assert!(fine, "{error}, but the table's records are {mine:?}");
}

/// The checks of [`parse_ttf`] on a font that parsed.
fn check_font(input: &[u8], font: &Font) {
    let count = font.glyph_count();
    assert!(count >= 1, "a font with no glyphs parsed");
    assert!((16..=16_384).contains(&font.units_per_em()));
    assert!(input.len() <= LIMITS.max_file_bytes);
    assert!(font.data() == input, "the font's bytes are not its input");
    let _ = (font.line_metrics(), font.bounding_box(), font.os2());
    for id in 0..=20 {
        let _ = font.name(NameId(id));
    }
    for c in [
        '\0',
        ' ',
        'A',
        'a',
        '0',
        '\u{e9}',
        '\u{FFFF}',
        '\u{10000}',
        '\u{1F600}',
        '\u{10FFFF}',
    ] {
        check_glyph_index(font, c);
    }
    // And a character the input picks.
    let picked = input
        .iter()
        .rev()
        .take(3)
        .fold(0u32, |c, &byte| c << 8 | u32::from(byte));
    if let Some(c) = char::from_u32(picked % 0x11_0000) {
        check_glyph_index(font, c);
    }

    let last = count - 1;
    let glyphs = (0..count.min(GLYPHS)).chain((last >= GLYPHS).then_some(last));
    for glyph in glyphs {
        let glyph = GlyphId(glyph);
        assert!(
            font.h_metrics(glyph).is_some(),
            "no metrics for glyph {}",
            glyph.0
        );
        match font.outline(glyph) {
            Ok(outline) => check_outline(font, &outline),
            Err(error) => check_glyph_error(input, font, &error),
        }
    }
    assert_eq!(font.h_metrics(GlyphId(count)), None);
    assert!(font.outline(GlyphId(count)).is_err());

    let few = count.min(8);
    for left in 0..few {
        for right in 0..few {
            let _ = font.kerning(GlyphId(left), GlyphId(right));
        }
    }
    assert_eq!(font.kerning(GlyphId(count), GlyphId(0)), 0);
    assert_eq!(font.kerning(GlyphId(0), GlyphId(u16::MAX)), 0);
}

fn check_glyph_index(font: &Font, c: char) {
    if let Some(glyph) = font.glyph_index(c) {
        assert!(
            glyph.0 != 0 && glyph.0 < font.glyph_count(),
            "{c:?} maps to glyph {} of {}",
            glyph.0,
            font.glyph_count()
        );
    }
}

/// Checks a glyph's error: in the input, naming a glyph that exists, and
/// a limit of [`LIMITS`].
fn check_glyph_error(input: &[u8], font: &Font, error: &FontError) {
    let FontError::Glyph {
        glyph,
        offset,
        kind,
    } = error
    else {
        panic!("a glyph failed with a {error:?}");
    };
    assert!(*offset <= input.len(), "{error} in {} bytes", input.len());
    assert!(glyph.0 < font.glyph_count(), "{error}");
    check_limit(kind);
    let fine = !matches!(
        kind,
        ErrorKind::FileTooLarge { .. }
            | ErrorKind::Cff
            | ErrorKind::Collection
            | ErrorKind::BadSfntVersion { .. }
            | ErrorKind::MissingTable
            | ErrorKind::TableOutOfBounds
            | ErrorKind::DuplicateTable
    );
    assert!(fine, "a glyph failed with a file's error: {error}");
}

/// Checks an outline, and rasterizes it at each of [`RASTERS`].
fn check_outline(font: &Font, outline: &Outline) {
    assert!(outline.point_count() <= LIMITS.max_glyph_points);
    assert_eq!(
        outline.contour_ends().last().copied().unwrap_or(0),
        outline.point_count()
    );
    assert!(
        outline
            .points()
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
    );
    for (px, shift) in RASTERS {
        let scale = px / f32::from(font.units_per_em());
        let result = outline::rasterize(outline, scale, shift, LIMITS);
        match &result {
            Ok(bitmap) => {
                let limit = LIMITS.max_bitmap_dimension;
                assert!(
                    bitmap.width <= limit && bitmap.height <= limit,
                    "{bitmap:?}"
                );
                assert_eq!(
                    bitmap.pixels.len(),
                    bitmap.width as usize * bitmap.height as usize
                );
                if let Some(bounds) = outline.control_box() {
                    // Each side of the scaled control box, rounded outwards.
                    let (scale, shift) = (f64::from(scale), f64::from(shift));
                    let left = (f64::from(bounds.x_min) * scale + shift).floor();
                    let right = (f64::from(bounds.x_max) * scale + shift).ceil();
                    let top = (-(f64::from(bounds.y_max) * scale)).floor();
                    let bottom = (-(f64::from(bounds.y_min) * scale)).ceil();
                    let edges = [
                        f64::from(bitmap.left),
                        f64::from(bitmap.left) + f64::from(bitmap.width),
                        f64::from(bitmap.top),
                        f64::from(bitmap.top) + f64::from(bitmap.height),
                    ];
                    assert!(
                        edges == [left, right, top, bottom],
                        "{bitmap:?} is not {bounds:?} at {scale} and {shift}, rounded outwards"
                    );
                } else {
                    assert_eq!(*bitmap, outline::Bitmap::default());
                }
                let again = outline::rasterize(outline, scale, shift, LIMITS);
                assert!(again.as_ref() == Ok(bitmap), "rasterized again, it differs");
            }
            Err(FontError::Outline {
                kind: outline::ErrorKind::BitmapTooLarge { limit },
            }) => assert_eq!(*limit, LIMITS.max_bitmap_dimension),
            Err(FontError::Outline {
                kind: outline::ErrorKind::TooMuchWork { limit },
            }) => assert_eq!(*limit, LIMITS.max_raster_work),
            Err(FontError::Outline {
                kind: outline::ErrorKind::BitmapOutOfRange,
            }) => {}
            Err(error) => panic!("rasterizing failed with {error}"),
        }
    }
}

#[test]
fn ttf() {
    let outcome = Target::new(ROOT, "ttf", parse_ttf)
        .dictionary(TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

fn seed(file: &str) -> Vec<u8> {
    std::fs::read(format!("{ROOT}/fuzz/corpus/ttf/{file}"))
        .unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// Parses `file`, reads every glyph and rasterizes it at the largest of
/// [`RASTERS`] with `limits`: the first error, if any.
fn read_all(file: &str, limits: Limits) -> Result<(), FontError> {
    let font = Font::parse(seed(file), limits)?;
    let (px, shift) = RASTERS[RASTERS.len() - 1];
    let scale = px / f32::from(font.units_per_em());
    for glyph in 0..font.glyph_count() {
        let outline = font.outline(GlyphId(glyph))?;
        outline::rasterize(&outline, scale, shift, limits)?;
    }
    Ok(())
}

/// The `edge-*` seeds sit exactly at each limit: they read whole with
/// [`LIMITS`], and fail with that limit one lower.
#[test]
fn the_edge_seeds_sit_exactly_at_each_limit() {
    let lower = |field: &str| {
        let mut limits = LIMITS;
        match field {
            "file" => limits.max_file_bytes -= 1,
            "points" => limits.max_glyph_points -= 1,
            "depth" => limits.max_component_depth -= 1,
            "components" => limits.max_components -= 1,
            "bitmap" => limits.max_bitmap_dimension -= 1,
            "work" => limits.max_raster_work -= 1,
            "kerning" => limits.max_kerning_subtables -= 1,
            _ => panic!("{field}"),
        }
        limits
    };
    // `edge-work.ttf` is a zigzag of 64 points, 21 units apart and 1,429
    // tall, closed below: its estimate at 40 px an em, shifted 0.75, is
    // exactly 4,096 steps (`outline`'s `work`).
    // `edge-kerning.ttf`'s `kern` lookup holds 8 distinct subtables. The
    // limit bounds the `kern` table's subtables too, with the same check
    // and error (`tests/ttf.rs`, `the_kern_table_s_subtables_are_bounded_too`);
    // one seed at it, through `GPOS`, is enough.
    for field in [
        "file",
        "points",
        "depth",
        "components",
        "bitmap",
        "work",
        "kerning",
    ] {
        let file = format!("edge-{field}.ttf");
        read_all(&file, LIMITS).unwrap_or_else(|error| panic!("{file}: {error}"));
        let error = read_all(&file, lower(field)).expect_err("over the limit");
        let fine = match (field, &error) {
            ("file", FontError::Ttf { kind, .. }) => matches!(kind, ErrorKind::FileTooLarge { .. }),
            ("points", FontError::Glyph { kind, .. }) => {
                matches!(kind, ErrorKind::TooManyPoints { .. })
            }
            ("depth", FontError::Glyph { kind, .. }) => matches!(kind, ErrorKind::TooDeep { .. }),
            ("components", FontError::Glyph { kind, .. }) => {
                matches!(kind, ErrorKind::TooManyComponents { .. })
            }
            ("bitmap", FontError::Outline { kind }) => {
                matches!(kind, outline::ErrorKind::BitmapTooLarge { .. })
            }
            ("work", FontError::Outline { kind }) => {
                matches!(kind, outline::ErrorKind::TooMuchWork { .. })
            }
            ("kerning", FontError::Ttf { kind, .. }) => {
                matches!(kind, ErrorKind::TooManyKerningSubtables { .. })
            }
            _ => false,
        };
        assert!(fine, "{file}: {error}");
    }
    assert_eq!(seed("edge-file.ttf").len(), LIMITS.max_file_bytes);
}

/// The seeds meant to be valid read whole, so that mutations start from
/// fonts that reach deep into the parser; the others fail where they are
/// meant to; and every seed is one of them.
#[test]
fn the_hand_made_seeds_parse_as_they_are_meant_to() {
    let whole = [
        "basic.ttf",
        "cmap.ttf",
        "cmap4-glyph-array.ttf",
        "curves.ttf",
        "composite.ttf",
        "kern.ttf",
        "gpos.ttf",
        "names-os2.ttf",
        "os2-v1.ttf",
        "edge-file.ttf",
        "edge-points.ttf",
        "edge-depth.ttf",
        "edge-components.ttf",
        "edge-bitmap.ttf",
        "edge-work.ttf",
        "edge-kerning.ttf",
    ];
    for file in whole {
        read_all(file, LIMITS).unwrap_or_else(|error| panic!("{file}: {error}"));
    }
    // A font that parses, with a glyph that holds itself and one that holds
    // a glyph that does not exist.
    let font = Font::parse(seed("composite-cycle.ttf"), LIMITS).expect("parses");
    assert!(matches!(
        font.outline(GlyphId(4)),
        Err(FontError::Glyph {
            kind: ErrorKind::TooDeep { .. },
            ..
        })
    ));
    assert!(matches!(
        font.outline(GlyphId(5)),
        Err(FontError::Glyph {
            kind: ErrorKind::GlyphOutOfRange { found: 77, .. },
            ..
        })
    ));
    let cut = Font::parse(seed("truncated.ttf"), LIMITS);
    assert!(
        matches!(
            cut,
            Err(FontError::Ttf {
                kind: ErrorKind::TableOutOfBounds,
                ..
            })
        ),
        "{cut:?}"
    );
    // What the seeds hold: kerning from each source, both cmap formats,
    // names and OS/2.
    let read = |file: &str| Font::parse(seed(file), LIMITS).expect("parses");
    let kern = read("kern.ttf");
    assert_eq!(kern.kerning(GlyphId(1), GlyphId(1)), -3);
    let gpos = read("gpos.ttf");
    assert_eq!(gpos.kerning(GlyphId(1), GlyphId(3)), -30);
    assert_eq!(gpos.kerning(GlyphId(2), GlyphId(2)), -21 - 5);
    let cmap = read("cmap.ttf");
    assert_eq!(cmap.glyph_index('\u{1F600}'), Some(GlyphId(2)));
    let names = read("names-os2.ttf");
    assert_eq!(
        names.name(NameId::TYPOGRAPHIC_FAMILY).as_deref(),
        Some("Seed Family")
    );
    assert_eq!(names.os2().map(|os2| os2.weight_class), Some(300));

    let files = std::fs::read_dir(format!("{ROOT}/fuzz/corpus/ttf")).expect("the seeds");
    assert_eq!(files.count(), whole.len() + 2);
}

#[test]
fn errors_are_checked_where_their_kind_says() {
    // A file over the limit, a version, a missing table.
    let mut big = seed("basic.ttf");
    big.resize(LIMITS.max_file_bytes + 1, 0);
    check_parse_error(
        &big,
        &Font::parse(big.clone(), LIMITS).expect_err("too large"),
    );
    let otto = [b"OTTO".as_slice(), &[0; 20]].concat();
    check_parse_error(&otto, &Font::parse(otto.clone(), LIMITS).expect_err("CFF"));
    let mut missing = seed("basic.ttf");
    missing[12..16].copy_from_slice(b"xxxx");
    let error = Font::parse(missing.clone(), LIMITS).expect_err("no head");
    assert!(matches!(
        error,
        FontError::Ttf {
            kind: ErrorKind::MissingTable,
            ..
        }
    ));
    check_parse_error(&missing, &error);
}
