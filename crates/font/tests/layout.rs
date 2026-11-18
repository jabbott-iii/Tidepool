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

//! `bundled`, `fallback` and `layout`: the faces compiled in, the chain a
//! missing character falls back through, and lines of positioned glyphs,
//! checked against the fonts' own metrics and kerning.

mod common;

use tidepool_font::bundled::{self, Face};
use tidepool_font::fallback::{self, FaceId, Faces, MAX_FACES, NOTDEF_ADVANCE};
use tidepool_font::layout::{self, Glyph, Line, MAX_LETTER_SPACING, PxSize, Style, VMetrics};
use tidepool_font::ttf::{Font, GlyphId, NameId};
use tidepool_font::{FontError, Limits};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");

fn faces() -> Faces {
    bundled::load().expect("the bundled faces load")
}

fn px(size: f32) -> PxSize {
    PxSize::new(size).expect("a size")
}

fn style(face: Face, size: f32) -> Style {
    Style::new(face.id(), px(size))
}

/// The characters of the text each glyph of `line` lays out.
fn text_of(text: &str, line: &Line) -> String {
    line.glyphs
        .iter()
        .map(|glyph| text[glyph.cluster..].chars().next().expect("a character"))
        .collect()
}

/// Whether `a` and `b` are within float error of each other.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * a.abs().max(b.abs()).max(1.0)
}

/// A face that maps only `中`, to a square, and whose `.notdef` is empty:
/// a system face, as the caller would add one.
fn system_face() -> Font {
    let mut font = common::Font::new(vec![Vec::new(), common::simple(&[&common::square(500)])]);
    font.cmap = common::cmap(&[(
        3,
        1,
        common::cmap4(&[common::Segment::Delta(0x4e2d, 0x4e2d, 1 - 0x4e2d)]),
    )]);
    Font::parse(font.build(), Limits::DEFAULT).expect("a valid font")
}

/// The advance of `c` in `face` at `size`, from the font's own metrics.
fn advance_of(faces: &Faces, face: Face, c: char, size: f32) -> f32 {
    let font = faces.font(face.id()).expect("loaded");
    let glyph = font.glyph_index(c).expect("mapped");
    let units = font.h_metrics(glyph).expect("a glyph").advance;
    f32::from(units) * size / f32::from(font.units_per_em())
}

#[test]
fn the_bundled_faces_are_the_files_in_assets() {
    let faces = faces();
    assert_eq!(faces.len(), 7);
    assert_eq!(faces.fallback(), [Face::MonoRegular.id()]);
    for (index, face) in Face::ALL.into_iter().enumerate() {
        assert_eq!(face.id(), FaceId(u16::try_from(index).expect("seven")));
        let on_disk = std::fs::read(format!("{ROOT}/{}", face.path())).expect("the file");
        assert_eq!(face.bytes(), on_disk.as_slice(), "{face:?}");
        let font = faces.font(face.id()).expect("loaded");
        assert_eq!(font.data(), face.bytes());
        let family = font
            .name(NameId::TYPOGRAPHIC_FAMILY)
            .or_else(|| font.name(NameId::FAMILY));
        assert_eq!(family.as_deref(), Some(face.family()));
        let os2 = font.os2().expect("an OS/2 table");
        assert_eq!(os2.weight_class, face.weight(), "{face:?}");
        assert_eq!(os2.italic, face.italic(), "{face:?}");
        // Their glyph 0 is a box: no need to make one.
        assert!(!faces.draws_own_notdef(face.id()), "{face:?}");
    }
}

#[test]
fn characters_fall_back_through_the_chain_then_to_notdef() {
    let mut faces = faces();
    let saira = Face::SairaRegular.id();
    let mono = Face::MonoRegular.id();
    let found = faces.resolve(saira, 'A').expect("a face");
    assert_eq!(found.face, saira);
    assert!(!found.is_notdef());
    // Saira lacks ✓, which JetBrains Mono has.
    let check = faces.resolve(saira, '✓').expect("a face");
    assert_eq!(check.face, mono);
    let font = faces.font(mono).expect("loaded");
    assert_eq!(Some(check.glyph), font.glyph_index('✓'));
    // Neither has 中.
    let missing = faces.resolve(saira, '中').expect("a face");
    assert_eq!((missing.face, missing.glyph), (saira, GlyphId(0)));
    // The italic face falls back to the regular one, and a face in its
    // own chain is skipped.
    let italic = Face::MonoItalic.id();
    assert_eq!(faces.resolve(italic, '✓').expect("a face").face, italic);
    assert_eq!(faces.resolve(mono, '中').expect("a face").face, mono);

    // A system face, configured after JetBrains Mono.
    let system = faces.add(system_face()).expect("room");
    assert_eq!(system, FaceId(7));
    assert!(faces.draws_own_notdef(system), "its glyph 0 is empty");
    faces.set_fallback(&[mono, system]).expect("a chain");
    let found = faces.resolve(saira, '中').expect("a face");
    assert_eq!((found.face, found.glyph), (system, GlyphId(1)));
    assert_eq!(faces.resolve(saira, '✓').expect("a face").face, mono);
    // Its own missing characters are its own `.notdef`.
    let missing = faces.resolve(system, 'ℵ').expect("a face");
    assert_eq!((missing.face, missing.glyph), (system, GlyphId(0)));
}

#[test]
fn faces_refuse_unknown_ids_repeats_and_too_many() {
    let mut faces = faces();
    let unknown = FaceId(7);
    let error = |kind| FontError::Faces { kind };
    assert_eq!(
        faces.resolve(unknown, 'A'),
        Err(error(fallback::ErrorKind::UnknownFace { face: unknown }))
    );
    assert_eq!(faces.font(unknown).map(Font::glyph_count), None);
    assert_eq!(
        faces.set_fallback(&[Face::MonoRegular.id(), unknown]),
        Err(error(fallback::ErrorKind::UnknownFace { face: unknown }))
    );
    let mono = Face::MonoRegular.id();
    assert_eq!(
        faces.set_fallback(&[mono, Face::SairaBold.id(), mono]),
        Err(error(fallback::ErrorKind::RepeatedFallback { face: mono }))
    );
    assert_eq!(faces.fallback(), [mono], "unchanged by an error");
    faces.set_fallback(&[]).expect("an empty chain");
    assert_eq!(
        faces
            .resolve(Face::SairaRegular.id(), '✓')
            .expect("a face")
            .glyph,
        GlyphId(0)
    );

    let font = Face::SairaRegular.parse().expect("parses");
    let mut many = Faces::new();
    for index in 0..MAX_FACES {
        assert_eq!(
            many.add(font.clone()),
            Ok(FaceId(u16::try_from(index).expect("small")))
        );
    }
    assert_eq!(
        many.add(font),
        Err(error(fallback::ErrorKind::TooManyFaces {
            limit: MAX_FACES
        }))
    );
    assert_eq!(many.len(), MAX_FACES);
    assert!(Faces::default().is_empty());
    assert_eq!(
        format!("{:?}", Faces::new()),
        "Faces { faces: 0, fallback: [], .. }"
    );
}

#[test]
fn sizes_are_64ths_of_a_pixel() {
    assert_eq!(px(14.0).units(), 896);
    assert_eq!(px(14.0).px(), 14.0);
    assert_eq!(px(13.0 * 1.25).px(), 16.25);
    assert_eq!(px(0.1).px(), 0.09375, "to the nearest 64th");
    assert_eq!(px(1.0 / 128.0).units(), 1, "a half rounds up");
    assert_eq!(PxSize::new(1.0 / 256.0), None);
    assert_eq!(PxSize::new(0.0), None);
    assert_eq!(PxSize::new(-3.0), None);
    assert_eq!(PxSize::new(f32::NAN), None);
    assert_eq!(PxSize::new(f32::INFINITY), None);
    assert_eq!(PxSize::new(2048.0), Some(PxSize::MAX));
    assert_eq!(PxSize::new(2048.01), None);
    assert_eq!(PxSize::from_units(0), None);
    assert_eq!(PxSize::from_units(PxSize::MAX.units() + 1), None);
    assert_eq!(PxSize::from_units(64), Some(px(1.0)));
}

#[test]
fn a_line_advances_kerns_and_spaces_its_glyphs() {
    let faces = faces();
    let font = faces.font(Face::SairaRegular.id()).expect("loaded");
    assert_eq!(font.units_per_em(), 1000);
    let size = 20.0;
    let plain = layout::line(&faces, "AV", &style(Face::SairaRegular, size)).expect("a line");
    let a = font.glyph_index('A').expect("an A");
    let v = font.glyph_index('V').expect("a V");
    let kern = font.kerning(a, v);
    assert!(kern < 0, "Saira kerns AV");
    let first = advance_of(&faces, Face::SairaRegular, 'A', size);
    assert_eq!(
        plain.glyphs,
        [
            Glyph {
                face: Face::SairaRegular.id(),
                glyph: Some(a),
                x: 0.0,
                advance: first,
                cluster: 0,
            },
            Glyph {
                face: Face::SairaRegular.id(),
                glyph: Some(v),
                x: first + kern as f32 * size / 1000.0,
                advance: advance_of(&faces, Face::SairaRegular, 'V', size),
                cluster: 1,
            },
        ]
    );
    assert_eq!(plain.width, plain.glyphs[1].x + plain.glyphs[1].advance);
    assert_eq!(
        (plain.pen, plain.range.clone(), plain.truncated),
        (plain.width, 0..2, false)
    );

    // Letter-spacing between glyphs, not after the last.
    let tracked = Style {
        letter_spacing: 0.28,
        ..style(Face::SairaRegular, size)
    };
    let label = layout::line(&faces, "AV", &tracked).expect("a line");
    assert!(close(label.glyphs[1].x, plain.glyphs[1].x + 0.28 * size));
    assert!(close(label.width, plain.width + 0.28 * size));
    // Spacing that is not finite counts as none.
    let broken = Style {
        letter_spacing: f32::NAN,
        ..tracked
    };
    assert_eq!(layout::line(&faces, "AV", &broken).expect("a line"), plain);
    // Spacing beyond 64 ems is 64 ems, so the line stays finite.
    for (huge, clamped) in [(1e38, MAX_LETTER_SPACING), (-1e38, -MAX_LETTER_SPACING)] {
        let style = |letter_spacing| Style {
            letter_spacing,
            ..tracked
        };
        let line = layout::line(&faces, "AV\tA", &style(huge)).expect("a line");
        assert!(
            line.glyphs
                .iter()
                .all(|glyph| glyph.x.is_finite() && glyph.advance.is_finite())
        );
        assert_eq!(
            line,
            layout::line(&faces, "AV\tA", &style(clamped)).expect("a line")
        );
    }
}

#[test]
fn kerning_never_crosses_faces_and_fallbacks_take_their_own_advance() {
    let faces = faces();
    let size = 16.0;
    // ✓ comes from JetBrains Mono, at its advance, with no kerning.
    let line = layout::line(&faces, "A✓A", &style(Face::SairaRegular, size)).expect("a line");
    let [first, check, last] = line.glyphs.as_slice() else {
        panic!("three glyphs");
    };
    assert_eq!(check.face, Face::MonoRegular.id());
    assert_eq!(check.x, first.advance);
    assert_eq!(
        check.advance,
        advance_of(&faces, Face::MonoRegular, '✓', size)
    );
    assert_eq!(last.x, check.x + check.advance);
    // 中 is Saira's `.notdef`, at its glyph 0's advance.
    let line = layout::line(&faces, "中", &style(Face::SairaRegular, size)).expect("a line");
    let font = faces.font(Face::SairaRegular.id()).expect("loaded");
    let notdef = f32::from(font.h_metrics(GlyphId(0)).expect("a glyph 0").advance);
    assert_eq!(line.glyphs[0].glyph, Some(GlyphId(0)));
    assert_eq!(line.glyphs[0].advance, notdef * size / 1000.0);
}

#[test]
fn a_box_is_made_for_a_notdef_with_no_outline() {
    let mut faces = Faces::new();
    let system = faces.add(system_face()).expect("room");
    let size = 10.0;
    let line = layout::line(&faces, "x中", &Style::new(system, px(size))).expect("a line");
    assert_eq!(line.glyphs[0].glyph, Some(GlyphId(0)));
    assert_eq!(line.glyphs[0].advance, NOTDEF_ADVANCE * size);
    assert_eq!(line.glyphs[1].x, NOTDEF_ADVANCE * size);
    // Glyph 1 is 510 units wide.
    assert_eq!(line.glyphs[1].advance, 5.1);
}

#[test]
fn tabs_go_to_the_next_stop_from_the_line_start() {
    let faces = faces();
    let mono = style(Face::MonoRegular, 10.0);
    let space = advance_of(&faces, Face::MonoRegular, ' ', 10.0);
    assert_eq!(space, 6.0);
    let stops = |text: &str, style: &Style| {
        let line = layout::line(&faces, text, style).expect("a line");
        line.glyphs.iter().map(|glyph| glyph.x).collect::<Vec<_>>()
    };
    assert_eq!(stops("a\tb", &mono), [0.0, 6.0, 24.0]);
    // A pen on a stop goes to the next one.
    assert_eq!(stops("abcd\tx", &mono), [0.0, 6.0, 12.0, 18.0, 24.0, 48.0]);
    assert_eq!(stops("\t\tx", &mono), [0.0, 24.0, 48.0]);
    let two = Style {
        tab_size: 2,
        ..mono
    };
    assert_eq!(stops("a\tb", &two), [0.0, 6.0, 12.0]);
    let none = Style {
        tab_size: 0,
        ..mono
    };
    assert_eq!(stops("a\tb", &none), [0.0, 6.0, 12.0], "0 counts as 1");
    let line = layout::line(&faces, "a\t", &mono).expect("a line");
    assert_eq!(line.glyphs[1].glyph, None, "a tab draws nothing");
    assert_eq!(line.glyphs[1].advance, 18.0);
    assert_eq!(
        (line.width, line.pen),
        (6.0, 24.0),
        "the width leaves it out"
    );
    // No letter-spacing comes before or after a tab.
    let tracked = Style {
        letter_spacing: 0.5,
        ..mono
    };
    assert_eq!(stops("a\tb", &tracked), [0.0, 6.0, 24.0]);
    assert_eq!(stops("ab", &tracked), [0.0, 11.0]);
    // Stops every 4 spaces of Saira.
    let saira = style(Face::SairaRegular, 10.0);
    let saira_space = advance_of(&faces, Face::SairaRegular, ' ', 10.0);
    assert_eq!(stops("\tx", &saira), [0.0, 4.0 * saira_space]);
}

#[test]
fn line_breaks_are_drawn_in_a_line_and_break_in_a_wrap() {
    let faces = faces();
    let saira = style(Face::SairaRegular, 12.0);
    let line = layout::line(&faces, "a\nb", &saira).expect("a line");
    assert_eq!(line.glyphs.len(), 3);
    assert_eq!(
        line.glyphs[1].glyph,
        Some(GlyphId(0)),
        "Saira has no glyph for LF"
    );
    let lines = layout::wrap(&faces, "a\nb\r\nc\rd\n", &saira, f32::INFINITY).expect("lines");
    let texts: Vec<String> = lines
        .iter()
        .map(|line| text_of("a\nb\r\nc\rd\n", line))
        .collect();
    assert_eq!(texts, ["a", "b", "c", "d", ""]);
    let ranges: Vec<_> = lines.iter().map(|line| line.range.clone()).collect();
    assert_eq!(ranges, [0..1, 2..3, 5..6, 7..8, 9..9]);
    let empty = layout::wrap(&faces, "", &saira, 100.0).expect("lines");
    assert_eq!(empty.len(), 1);
    assert_eq!(
        (
            empty[0].glyphs.len(),
            empty[0].width,
            empty[0].range.clone()
        ),
        (0, 0.0, 0..0)
    );
}

#[test]
fn wrapping_breaks_after_spaces_that_hang() {
    let faces = faces();
    let mono = style(Face::MonoRegular, 10.0);
    // Six pixels a character: 60 pixels hold ten.
    let text = "one two three four";
    let lines = layout::wrap(&faces, text, &mono, 60.0).expect("lines");
    let texts: Vec<String> = lines.iter().map(|line| text_of(text, line)).collect();
    assert_eq!(texts, ["one two ", "three four"]);
    assert_eq!(lines[0].width, 42.0, "the space hangs");
    assert_eq!(lines[0].pen, 48.0);
    assert_eq!(lines[1].glyphs[0].x, 0.0, "each line starts at 0");
    assert_eq!(lines[1].width, 60.0, "exactly the width fits");
    let ranges: Vec<_> = lines.iter().map(|line| line.range.clone()).collect();
    assert_eq!(ranges, [0..8, 8..18]);
    // Spaces hang however many, and a long word breaks where it overflows.
    let text = "a    abcdefghijklmnop q";
    let lines = layout::wrap(&faces, text, &mono, 30.0).expect("lines");
    let texts: Vec<String> = lines.iter().map(|line| text_of(text, line)).collect();
    assert_eq!(texts, ["a    ", "abcde", "fghij", "klmno", "p q"]);
    // Too narrow for anything: a character a line.
    let lines = layout::wrap(&faces, "ab c", &mono, 0.0).expect("lines");
    let texts: Vec<String> = lines.iter().map(|line| text_of("ab c", line)).collect();
    assert_eq!(texts, ["a", "b ", "c"]);
    // NaN and infinity never break.
    for width in [f32::NAN, f32::INFINITY] {
        assert_eq!(
            layout::wrap(&faces, text, &mono, width)
                .expect("lines")
                .len(),
            1
        );
    }
    // A no-break space does not break.
    let text = "aaaa\u{a0}bbbb";
    let lines = layout::wrap(&faces, text, &mono, 30.0).expect("lines");
    let texts: Vec<String> = lines.iter().map(|line| text_of(text, line)).collect();
    assert_eq!(texts, ["aaaa\u{a0}", "bbbb"]);
}

#[test]
fn tab_stops_restart_on_each_wrapped_line() {
    let faces = faces();
    let mono = style(Face::MonoRegular, 10.0);
    let text = "abcdef\tx gh\tyz";
    let lines = layout::wrap(&faces, text, &mono, 60.0).expect("lines");
    let texts: Vec<String> = lines.iter().map(|line| text_of(text, line)).collect();
    assert_eq!(texts, ["abcdef\tx ", "gh\tyz"]);
    // From the paragraph's start, the second tab would stop at 72.
    let xs: Vec<f32> = lines[1].glyphs.iter().map(|glyph| glyph.x).collect();
    assert_eq!(xs, [0.0, 6.0, 12.0, 24.0, 30.0]);
}

#[test]
fn wrapped_lines_match_lines_of_their_text() {
    // Each wrapped line is the line of its range, laid out on its own.
    let faces = faces();
    let tracked = Style {
        letter_spacing: 0.1,
        ..style(Face::SairaSemiBold, 13.0)
    };
    let text = "The quick brown fox, AVATAR Wave To; jumps over the lazy dog ✓ 中 again.";
    for width in [40.0, 75.5, 120.0, 300.0] {
        let lines = layout::wrap(&faces, text, &tracked, width).expect("lines");
        let mut end = 0;
        for line in &lines {
            assert_eq!(line.range.start, end, "the ranges tile the text");
            end = line.range.end;
            let alone = layout::line(&faces, &text[line.range.clone()], &tracked).expect("a line");
            assert_eq!(line.glyphs.len(), alone.glyphs.len());
            for (wrapped, alone) in line.glyphs.iter().zip(&alone.glyphs) {
                assert_eq!(wrapped.cluster, alone.cluster + line.range.start);
                assert_eq!((wrapped.x, wrapped.glyph), (alone.x, alone.glyph));
            }
            assert_eq!(line.width, alone.width);
            if line.glyphs.len() > 1 {
                assert!(line.width <= width, "{width}: {} is too wide", line.width);
            }
        }
        assert_eq!(end, text.len());
    }
}

#[test]
fn truncating_cuts_before_an_ellipsis_that_fits() {
    let faces = faces();
    let mono = style(Face::MonoRegular, 10.0);
    // Fits: unchanged.
    let fits = layout::truncate(&faces, "abcdef", &mono, 36.0).expect("a line");
    assert_eq!(fits, layout::line(&faces, "abcdef", &mono).expect("a line"));
    // Six pixels a character, the ellipsis too.
    let cut = layout::truncate(&faces, "abcdefgh", &mono, 36.0).expect("a line");
    let clusters: Vec<usize> = cut.glyphs.iter().map(|glyph| glyph.cluster).collect();
    assert_eq!(clusters, [0, 1, 2, 3, 4, 5]);
    assert_eq!(
        (cut.width, cut.pen, cut.range.clone(), cut.truncated),
        (36.0, 36.0, 0..5, true)
    );
    let ellipsis = cut.glyphs.last().expect("the ellipsis");
    let font = faces.font(Face::MonoRegular.id()).expect("loaded");
    assert_eq!(ellipsis.glyph, font.glyph_index('…'));
    assert_eq!((ellipsis.x, ellipsis.cluster), (30.0, 5));
    // The spaces before the cut go.
    let cut = layout::truncate(&faces, "abc   defgh", &mono, 45.0).expect("a line");
    assert_eq!(cut.glyphs.len(), 4);
    assert_eq!((cut.width, cut.range.clone()), (24.0, 0..3));
    // Not even the ellipsis fits: it alone.
    let cut = layout::truncate(&faces, "abcdefgh", &mono, 3.0).expect("a line");
    assert_eq!(
        (cut.glyphs.len(), cut.width, cut.range.clone()),
        (1, 6.0, 0..0)
    );
    // NaN never cuts, nor is empty text cut.
    let whole = layout::truncate(&faces, "abcdefgh", &mono, f32::NAN).expect("a line");
    assert!(!whole.truncated);
    let empty = layout::truncate(&faces, "", &mono, -1.0).expect("a line");
    assert_eq!((empty.glyphs.len(), empty.truncated), (0, false));
    // Letter-spacing comes before the ellipsis, and Saira's ellipsis is its
    // own.
    let label = Style {
        letter_spacing: 0.28,
        ..style(Face::SairaSemiBold, 11.0)
    };
    let full = layout::line(&faces, "EXPLORER PANEL", &label).expect("a line");
    let cut = layout::truncate(&faces, "EXPLORER PANEL", &label, full.width - 1.0).expect("a line");
    assert!(cut.truncated && cut.width <= full.width - 1.0);
    let [.., before, ellipsis] = cut.glyphs.as_slice() else {
        panic!("glyphs and the ellipsis");
    };
    assert_eq!(ellipsis.face, Face::SairaSemiBold.id());
    let kern = faces
        .font(Face::SairaSemiBold.id())
        .map(|font| font.kerning(before.glyph.expect("a glyph"), ellipsis.glyph.expect("one")))
        .expect("loaded");
    assert_eq!(
        ellipsis.x,
        before.x + before.advance + 0.28 * 11.0 + kern as f32 * 11.0 / 1000.0
    );
}

#[test]
fn metrics_come_from_hhea() {
    let faces = faces();
    let saira = layout::metrics(&faces, &style(Face::SairaRegular, 10.0)).expect("metrics");
    // Saira: ascender 1135, descender −439, no line gap, to 1000 an em.
    assert_eq!(
        saira,
        VMetrics {
            ascent: 11.35,
            descent: 4.39,
            line_gap: 0.0,
        }
    );
    assert!(close(saira.height(), 15.74));
    // In a line 20 px tall, the 4.26 px left over is shared.
    assert!(close(saira.baseline(20.0), 2.13 + 11.35));
    assert!(
        close(saira.baseline(10.0), 11.35 - 2.87),
        "or what is missing"
    );
    let mono = layout::metrics(&faces, &style(Face::MonoRegular, 10.0)).expect("metrics");
    assert_eq!((mono.ascent, mono.descent), (10.2, 3.0));
}

#[test]
fn layout_refuses_a_face_it_is_not_given() {
    let faces = faces();
    let unknown = Style::new(FaceId(40), px(12.0));
    let expected = Err(FontError::Faces {
        kind: fallback::ErrorKind::UnknownFace { face: FaceId(40) },
    });
    assert_eq!(layout::line(&faces, "a", &unknown), expected);
    assert_eq!(layout::truncate(&faces, "a", &unknown, 1.0), expected);
    assert_eq!(
        layout::wrap(&faces, "a", &unknown, 1.0).map(|_| ()),
        Err(FontError::Faces {
            kind: fallback::ErrorKind::UnknownFace { face: FaceId(40) },
        })
    );
    assert!(layout::metrics(&faces, &unknown).is_err());
}

#[test]
fn layouts_are_the_same_every_time() {
    let faces = faces();
    let text = "Tidepool\t0.1 — AVAVA ✓ 中 fi fl “quotes” ‘and’ more…";
    for face in Face::ALL {
        for size in [11.0, 12.0, 13.0, 14.0, 16.0, 24.0, 16.25] {
            let style = Style {
                letter_spacing: 0.1,
                ..style(face, size)
            };
            let first = layout::wrap(&faces, text, &style, 120.0).expect("lines");
            let again = layout::wrap(&faces, text, &style, 120.0).expect("lines");
            assert_eq!(first, again);
        }
    }
}
