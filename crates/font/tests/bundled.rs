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

//! The bundled faces in `assets/fonts/`: each parses with the default
//! limits and is the style it should be, every glyph reads and
//! rasterizes, and the coverage of the printable ASCII glyphs is pinned by
//! its FNV-1a 64 hash (`tidepool_core::hash::fnv1a64`), so that every OS
//! CI runs on checks the same bytes. Not SHA-256: ADR-0016 keeps that to
//! checking downloads.

use tidepool_core::hash::fnv1a64;
use tidepool_font::Limits;
use tidepool_font::outline::{self, OutlinePoint};
use tidepool_font::ttf::{Font, GlyphId, NameId};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");

/// A bundled face and what it must be.
struct Face {
    file: &'static str,
    family: &'static str,
    subfamily: &'static str,
    weight: u16,
    italic: bool,
    glyphs: u16,
    /// Its composite glyphs, read from `glyf` without the crate, and how
    /// many of them only move their components by offsets.
    composites: (u32, u32),
    /// The FNV-1a 64 hash of the printable ASCII glyphs' coverage at 14 px (see
    /// [`coverage`]).
    at_14: &'static str,
}

const FACES: [Face; 7] = [
    Face {
        file: "saira/Saira-Regular.ttf",
        family: "Saira",
        subfamily: "Regular",
        weight: 400,
        italic: false,
        glyphs: 897,
        composites: (497, 497),
        at_14: "bce99397435c004e",
    },
    Face {
        file: "saira/Saira-Medium.ttf",
        family: "Saira",
        subfamily: "Medium",
        weight: 500,
        italic: false,
        glyphs: 897,
        composites: (497, 497),
        at_14: "18146e1294a39916",
    },
    Face {
        file: "saira/Saira-SemiBold.ttf",
        family: "Saira",
        subfamily: "SemiBold",
        weight: 600,
        italic: false,
        glyphs: 897,
        composites: (493, 493),
        at_14: "b87b41b051669e65",
    },
    Face {
        file: "saira/Saira-Bold.ttf",
        family: "Saira",
        subfamily: "Bold",
        weight: 700,
        italic: false,
        glyphs: 897,
        composites: (493, 493),
        at_14: "3c1fca2a166d39c7",
    },
    Face {
        file: "jetbrains-mono/JetBrainsMono-Regular.ttf",
        family: "JetBrains Mono",
        subfamily: "Regular",
        weight: 400,
        italic: false,
        glyphs: 1743,
        composites: (570, 570),
        at_14: "df097cee7c303f2a",
    },
    Face {
        file: "jetbrains-mono/JetBrainsMono-Italic.ttf",
        family: "JetBrains Mono",
        subfamily: "Italic",
        weight: 400,
        italic: true,
        glyphs: 1730,
        composites: (570, 570),
        at_14: "436f7437f1009be6",
    },
    Face {
        file: "jetbrains-mono/JetBrainsMono-SemiBold.ttf",
        family: "JetBrains Mono",
        subfamily: "SemiBold",
        weight: 600,
        italic: false,
        glyphs: 1743,
        composites: (570, 570),
        at_14: "49de3793e37e16dc",
    },
];

/// The FNV-1a 64 hash of Saira Regular's printable ASCII glyphs at 24 px, shifted
/// right a quarter of a pixel.
const SAIRA_AT_24_SHIFTED: &str = "71d3f15ed5d0e74b";

fn load(file: &str) -> Font {
    let path = format!("{ROOT}/{file}");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    Font::parse(bytes, Limits::DEFAULT).unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// The FNV-1a 64 hash, in 16 hex digits, of the coverage of the glyphs of `' '` to `'~'` at
/// `px` pixels per em, shifted right `shift` pixels: for each character in
/// order, its glyph id (`u16`), then its bitmap's left and top (`i32`),
/// width and height (`u32`), all little-endian, then its pixels.
fn coverage(font: &Font, px: f32, shift: f32) -> String {
    format!("{:016x}", fnv1a64(&coverage_bytes(font, px, shift)))
}

/// The bytes [`coverage`] hashes.
fn coverage_bytes(font: &Font, px: f32, shift: f32) -> Vec<u8> {
    let scale = px / f32::from(font.units_per_em());
    let mut bytes = Vec::new();
    for c in ' '..='~' {
        let glyph = font.glyph_index(c).unwrap_or(GlyphId::NOTDEF);
        let outline = font
            .outline(glyph)
            .unwrap_or_else(|error| panic!("{c:?}: {error}"));
        let bitmap = outline::rasterize(&outline, scale, shift, Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{c:?}: {error}"));
        bytes.extend(glyph.0.to_le_bytes());
        bytes.extend(bitmap.left.to_le_bytes());
        bytes.extend(bitmap.top.to_le_bytes());
        bytes.extend(bitmap.width.to_le_bytes());
        bytes.extend(bitmap.height.to_le_bytes());
        bytes.extend(&bitmap.pixels);
    }
    bytes
}

#[test]
fn every_face_parses_and_is_the_style_it_should_be() {
    for face in &FACES {
        let font = load(face.file);
        let name = |typographic, plain| font.name(typographic).or_else(|| font.name(plain));
        assert_eq!(
            name(NameId::TYPOGRAPHIC_FAMILY, NameId::FAMILY).as_deref(),
            Some(face.family),
            "{}",
            face.file
        );
        assert_eq!(
            name(NameId::TYPOGRAPHIC_SUBFAMILY, NameId::SUBFAMILY).as_deref(),
            Some(face.subfamily),
            "{}",
            face.file
        );
        let os2 = font.os2().expect("an OS/2 table");
        assert_eq!(os2.weight_class, face.weight, "{}", face.file);
        assert_eq!(os2.italic, face.italic, "{}", face.file);
        assert_eq!(os2.bold, face.weight == 700, "{}", face.file);
        assert_eq!(font.units_per_em(), 1000, "{}", face.file);
        assert_eq!(font.glyph_count(), face.glyphs, "{}", face.file);
    }
}

#[test]
fn every_glyph_reads_and_rasterizes_at_14_px() {
    for face in &FACES {
        let font = load(face.file);
        let scale = 14.0 / f32::from(font.units_per_em());
        for glyph in 0..font.glyph_count() {
            let glyph = GlyphId(glyph);
            let outline = font
                .outline(glyph)
                .unwrap_or_else(|error| panic!("{}: {error}", face.file));
            let bitmap = outline::rasterize(&outline, scale, 0.0, Limits::DEFAULT)
                .unwrap_or_else(|error| panic!("{}: {error}", face.file));
            assert_eq!(
                bitmap.pixels.len(),
                bitmap.width as usize * bitmap.height as usize
            );
            assert!(font.h_metrics(glyph).is_some());
        }
    }
}

/// A face's glyph data, read without the crate: from the table directory,
/// `head`'s `indexToLocFormat`, `loca` and `glyf`.
struct Raw<'a> {
    data: &'a [u8],
    loca: usize,
    glyf: usize,
    long: bool,
}

impl<'a> Raw<'a> {
    fn new(data: &'a [u8]) -> Self {
        let be16 = |at: usize| u16::from_be_bytes([data[at], data[at + 1]]);
        let be32 =
            |at: usize| u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let count = usize::from(be16(4));
        let table = |tag: &[u8; 4]| {
            let record = (0..count)
                .map(|index| 12 + index * 16)
                .find(|&record| &data[record..record + 4] == tag)
                .expect("the table");
            be32(record + 8) as usize
        };
        Self {
            data,
            loca: table(b"loca"),
            glyf: table(b"glyf"),
            long: be16(table(b"head") + 50) == 1,
        }
    }

    fn be16(&self, at: usize) -> u16 {
        u16::from_be_bytes([self.data[at], self.data[at + 1]])
    }

    /// The data of `glyph`.
    fn glyph(&self, glyph: u16) -> &'a [u8] {
        let offset = |index: usize| {
            if self.long {
                let at = self.loca + index * 4;
                u32::from_be_bytes(self.data[at..at + 4].try_into().expect("four bytes")) as usize
            } else {
                usize::from(self.be16(self.loca + index * 2)) * 2
            }
        };
        let index = usize::from(glyph);
        &self.data[self.glyf + offset(index)..self.glyf + offset(index + 1)]
    }
}

/// The components of a composite glyph's data, as (glyph, dx, dy), if each
/// is only moved by an offset (no scale, no point matching).
fn translations(data: &[u8]) -> Option<Vec<(u16, i16, i16)>> {
    let be16 = |at: usize| u16::from_be_bytes([data[at], data[at + 1]]);
    let mut components = Vec::new();
    let mut at = 10;
    loop {
        let flags = be16(at);
        let glyph = be16(at + 2);
        at += 4;
        let (dx, dy) = if flags & 0x0001 != 0 {
            at += 4;
            (be16(at - 4) as i16, be16(at - 2) as i16)
        } else {
            at += 2;
            (i16::from(data[at - 2] as i8), i16::from(data[at - 1] as i8))
        };
        if flags & 0x0002 == 0 || flags & (0x0008 | 0x0040 | 0x0080) != 0 {
            return None;
        }
        components.push((glyph, dx, dy));
        if flags & 0x0020 == 0 {
            return Some(components);
        }
    }
}

#[test]
fn composites_are_their_components_moved() {
    for face in &FACES {
        let bytes = std::fs::read(format!("{ROOT}/{}", face.file)).expect("the face");
        let font = load(face.file);
        let raw = Raw::new(&bytes);
        let (mut composites, mut checked) = (0, 0);
        for glyph in 0..font.glyph_count() {
            let data = raw.glyph(glyph);
            if data.len() < 10 || (u16::from_be_bytes([data[0], data[1]]) as i16) >= 0 {
                continue;
            }
            composites += 1;
            let outline = font.outline(GlyphId(glyph)).expect("reads");
            assert!(
                !outline.is_empty(),
                "{}: composite {glyph} is empty",
                face.file
            );
            let Some(components) = translations(data) else {
                continue;
            };
            let mut points = Vec::new();
            let mut ends = Vec::new();
            for (child, dx, dy) in components {
                let part = font.outline(GlyphId(child)).expect("reads");
                for &end in part.contour_ends() {
                    ends.push(points.len() + end);
                }
                points.extend(part.points().iter().map(|point| OutlinePoint {
                    x: point.x + f32::from(dx),
                    y: point.y + f32::from(dy),
                    on_curve: point.on_curve,
                }));
            }
            assert_eq!(outline.points(), points, "{}: composite {glyph}", face.file);
            assert_eq!(
                outline.contour_ends(),
                ends,
                "{}: composite {glyph}",
                face.file
            );
            checked += 1;
        }
        assert_eq!(
            (composites, checked),
            face.composites,
            "{}: composites, and those moved only by offsets",
            face.file
        );
    }
}

#[test]
fn common_characters_map() {
    for face in &FACES {
        let font = load(face.file);
        for c in ['A', 'a', '0', ' '] {
            assert!(font.glyph_index(c).is_some(), "{}: {c:?}", face.file);
        }
        let space = font.glyph_index(' ').expect("a space");
        assert!(font.outline(space).is_ok_and(|outline| outline.is_empty()));
        assert!(
            font.h_metrics(space)
                .is_some_and(|metrics| metrics.advance > 0)
        );
        let a = font.glyph_index('A').expect("an A");
        assert!(!font.outline(a).expect("reads").is_empty());
    }
}

#[test]
fn saira_kerns_and_jetbrains_mono_does_not() {
    let pair = |font: &Font, left, right| {
        let glyph = |c| font.glyph_index(c).expect("mapped");
        font.kerning(glyph(left), glyph(right))
    };
    for face in &FACES[..4] {
        let font = load(face.file);
        assert!(pair(&font, 'A', 'V') < 0, "{}", face.file);
        assert!(pair(&font, 'T', 'o') < 0, "{}", face.file);
        assert_eq!(pair(&font, 'o', 'o'), 0, "{}", face.file);
    }
    // Saira Regular's, as an independent reader of its GPOS gave them.
    let regular = load(FACES[0].file);
    assert_eq!(pair(&regular, 'A', 'V'), -49);
    assert_eq!(pair(&regular, 'L', 'T'), -100);
    assert_eq!(pair(&regular, 'Y', 'o'), -65);
    for face in &FACES[4..] {
        let font = load(face.file);
        for left in ' '..='~' {
            for right in ' '..='~' {
                assert_eq!(pair(&font, left, right), 0, "{}", face.file);
            }
        }
    }
}

#[test]
fn ascii_coverage_is_pinned() {
    let mut failures = Vec::new();
    for face in &FACES {
        let font = load(face.file);
        let found = coverage(&font, 14.0, 0.0);
        if found != face.at_14 {
            failures.push(format!("{} at 14 px: {found}", face.file));
        }
    }
    let found = coverage(&load(FACES[0].file), 24.0, 0.25);
    if found != SAIRA_AT_24_SHIFTED {
        failures.push(format!("{} at 24 px, shifted 0.25: {found}", FACES[0].file));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn coverage_is_the_same_twice() {
    let font = load(FACES[4].file);
    assert_eq!(coverage(&font, 13.0, 0.5), coverage(&font, 13.0, 0.5));
}
