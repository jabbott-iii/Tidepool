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

//! The glyph atlas: what it returns is what `atlas::rasterize` draws, it
//! keeps what was used last when it evicts, and it never mixes two sets
//! of faces.

mod common;

use tidepool_font::atlas::{self, Atlas, AtlasGlyph, GlyphKey, MAX_GLYPHS, Subpixel};
use tidepool_font::bundled::{self, Face};
use tidepool_font::fallback::{self, FaceId, Faces};
use tidepool_font::layout::PxSize;
use tidepool_font::outline::Bitmap;
use tidepool_font::ttf::{Font, GlyphId};
use tidepool_font::{FontError, Limits};

fn faces() -> Faces {
    bundled::load().expect("the bundled faces load")
}

fn key(faces: &Faces, face: Face, c: char, size: f32, quarters: u8) -> GlyphKey {
    let glyph = faces
        .font(face.id())
        .and_then(|font| font.glyph_index(c))
        .expect("mapped");
    GlyphKey {
        face: face.id(),
        glyph,
        size: PxSize::new(size).expect("a size"),
        subpixel: Subpixel::new(quarters).expect("a quarter"),
    }
}

/// Whether the atlas's image of a glyph is `bitmap`, pixel for pixel.
fn same(image: &AtlasGlyph<'_>, bitmap: &Bitmap) -> bool {
    if (image.width, image.height, image.left, image.top)
        != (bitmap.width, bitmap.height, bitmap.left, bitmap.top)
    {
        return false;
    }
    (0..bitmap.height).all(|row| {
        let start = (row * bitmap.width) as usize;
        image.row(row) == bitmap.pixels.get(start..start + bitmap.width as usize)
    })
}

#[test]
fn a_glyph_in_the_atlas_is_its_rasterized_image() {
    let faces = faces();
    let mut atlas = Atlas::new(256, 256).expect("a size");
    for face in Face::ALL {
        for quarters in 0..4 {
            let key = key(&faces, face, 'g', 14.0, quarters);
            let bitmap = atlas::rasterize(&faces, key).expect("rasterizes");
            let image = atlas.glyph(&faces, key).expect("fits");
            assert!(same(&image, &bitmap), "{face:?} at {quarters} quarters");
            // Its place in the atlas's image.
            let first = (image.y * image.atlas_width() + image.x) as usize;
            assert_eq!(
                image.pixels().get(first),
                image.row(0).and_then(|row| row.first())
            );
            assert_eq!(image.coverage(image.width, 0), None);
            assert_eq!(image.row(image.height), None);
            assert_eq!((image.atlas_width(), image.atlas_height()), (256, 256));
        }
    }
    assert_eq!(atlas.len(), 28);
    // Each quarter is drawn moved, so they differ.
    let images: Vec<Bitmap> = (0..4)
        .map(|quarters| {
            atlas::rasterize(&faces, key(&faces, Face::SairaRegular, 'l', 14.0, quarters))
                .expect("rasterizes")
        })
        .collect();
    assert!(images.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn a_glyph_is_rasterized_once() {
    let faces = faces();
    let mut atlas = Atlas::new(128, 128).expect("a size");
    let a = key(&faces, Face::SairaRegular, 'a', 13.0, 1);
    let place = {
        let image = atlas.glyph(&faces, a).expect("fits");
        (image.x, image.y, image.width, image.height)
    };
    assert!(atlas.contains(&a));
    let again = atlas.glyph(&faces, a).expect("fits");
    assert_eq!((again.x, again.y, again.width, again.height), place);
    assert_eq!(atlas.len(), 1);
    // A space takes no room, and is held all the same.
    let space = key(&faces, Face::SairaRegular, ' ', 13.0, 0);
    let image = atlas.glyph(&faces, space).expect("fits");
    assert_eq!((image.width, image.height, image.row(0)), (0, 0, None));
    assert_eq!(atlas.len(), 2);
    assert!(!atlas.contains(&key(&faces, Face::SairaRegular, 'a', 13.0, 2)));
    atlas.clear();
    assert!(atlas.is_empty());
    assert!(format!("{atlas:?}").starts_with("Atlas { width: 128, height: 128, glyphs: 0"));
}

#[test]
fn eviction_keeps_the_most_recently_used_and_moves_nothing_it_returns() {
    let faces = faces();
    // Room for a few dozen glyphs at a time, each at most 40 pixels a side.
    let mut atlas = Atlas::new(80, 80).expect("a size");
    let keep = key(&faces, Face::SairaBold, 'W', 16.0, 0);
    let mut seen = 0;
    for size in [11.0, 12.0, 13.0, 14.0, 16.0, 24.0] {
        for c in ('!'..='~').filter(|c| *c != ' ') {
            for face in [Face::SairaRegular, Face::MonoItalic] {
                let key = key(&faces, face, c, size, (seen % 4) as u8);
                seen += 1;
                let bitmap = atlas::rasterize(&faces, key).expect("rasterizes");
                let image = atlas.glyph(&faces, key).expect("fits");
                assert!(same(&image, &bitmap), "{face:?} {c:?} at {size}");
                // The glyph used just before this one is never evicted.
                assert!(
                    seen == 1 || atlas.contains(&keep),
                    "{face:?} {c:?} at {size}"
                );
                let kept = atlas.glyph(&faces, keep).expect("fits");
                assert!(kept.width > 0);
            }
        }
    }
    assert!(atlas.evictions() > 0, "the atlas filled");
    assert!(atlas.len() < seen);
    assert!(atlas.contains(&keep));
    // Every glyph still held is still its image, wherever it moved.
    for size in [24.0] {
        for c in '!'..='~' {
            let key = key(&faces, Face::MonoItalic, c, size, 0);
            if atlas.contains(&key) {
                let bitmap = atlas::rasterize(&faces, key).expect("rasterizes");
                assert!(same(&atlas.glyph(&faces, key).expect("held"), &bitmap));
            }
        }
    }
}

#[test]
fn text_drawn_through_a_tiny_atlas_is_text_drawn_through_a_large_one() {
    let faces = faces();
    let draw = |atlas: &mut Atlas| {
        let mut canvas = vec![0u8; 640 * 40];
        let text = "The quick brown fox jumps over the lazy dog. 0123456789 AVATAR";
        let mut x = 2.0;
        for c in text.chars() {
            let key0 = key(&faces, Face::SairaRegular, c, 16.0, 0);
            let (pixel, subpixel) = Subpixel::split(x);
            let key = GlyphKey { subpixel, ..key0 };
            let image = atlas.glyph(&faces, key).expect("fits");
            for row in 0..image.height {
                for column in 0..image.width {
                    let at_x = pixel + image.left + column as i32;
                    let at_y = 28 + image.top + row as i32;
                    let index = usize::try_from(at_y * 640 + at_x).expect("on the canvas");
                    let coverage = image.coverage(column, row).expect("inside");
                    canvas[index] = canvas[index].saturating_add(coverage);
                }
            }
            let font = faces.font(Face::SairaRegular.id()).expect("loaded");
            let advance = font.h_metrics(key.glyph).expect("a glyph").advance;
            x += f32::from(advance) * 16.0 / 1000.0;
        }
        canvas
    };
    let mut large = Atlas::new(512, 512).expect("a size");
    let mut tiny = Atlas::new(40, 40).expect("a size");
    assert_eq!(draw(&mut large), draw(&mut tiny));
    assert_eq!(large.evictions(), 0);
    assert!(tiny.evictions() > 0);
}

#[test]
fn a_glyph_over_half_the_atlas_is_refused_and_remembered() {
    let faces = faces();
    let mut atlas = Atlas::new(32, 16).expect("a size");
    assert_eq!(atlas.glyph_limit(), 8);
    let small = key(&faces, Face::SairaRegular, '.', 12.0, 0);
    atlas.glyph(&faces, small).expect("fits");
    let large = key(&faces, Face::SairaRegular, 'M', 24.0, 0);
    let bitmap = atlas::rasterize(&faces, large).expect("rasterizes");
    assert!(
        bitmap.width > 8 && bitmap.width <= 32,
        "it would fit the atlas"
    );
    let refused = Err(FontError::Atlas {
        kind: atlas::ErrorKind::GlyphTooLarge { limit: 8 },
    });
    assert_eq!(atlas.glyph(&faces, large).map(|_| ()), refused);
    assert_eq!(
        (atlas.len(), atlas.evictions(), atlas.failures()),
        (1, 0, 1)
    );
    assert!(atlas.contains(&small));
    // Asked for again, it fails at once, from what was remembered.
    assert_eq!(atlas.glyph(&faces, large).map(|_| ()), refused);
    assert_eq!(atlas.failures(), 1);
    atlas.clear();
    assert_eq!(atlas.failures(), 0);
}

#[test]
fn a_hostile_face_costs_its_work_once() {
    // 16 units to the em, and a glyph 4,000 units square: 3,500 pixels at
    // 14 px, which the atlas refuses from its control box, before it makes
    // the 12 MB image.
    let mut font = common::Font::new(vec![
        common::simple(&[&common::square(8)]),
        common::simple(&[&common::square(4000)]),
    ]);
    font.units_per_em = 16;
    let font = Font::parse(font.build(), Limits::DEFAULT).expect("a valid font");
    let mut faces = faces();
    let hostile = faces.add(font).expect("room");
    let key = GlyphKey {
        face: hostile,
        glyph: GlyphId(1),
        size: PxSize::new(14.0).expect("a size"),
        subpixel: Subpixel::ZERO,
    };
    let mut atlas = Atlas::new(1024, 1024).expect("a size");
    let refused = Err(FontError::Atlas {
        kind: atlas::ErrorKind::GlyphTooLarge { limit: 512 },
    });
    let start = std::time::Instant::now();
    for _ in 0..100 {
        assert_eq!(atlas.glyph(&faces, key).map(|_| ()), refused);
    }
    // A full rasterize of it takes milliseconds each; the 100 refusals
    // take far less than one would (measured: microseconds).
    assert!(start.elapsed() < std::time::Duration::from_millis(500));
    assert_eq!((atlas.len(), atlas.failures()), (0, 1));
    // `rasterize` itself, with the face's own limits, draws it.
    let bitmap = atlas::rasterize(&faces, key).expect("within the face's limits");
    assert_eq!((bitmap.width, bitmap.height), (3500, 3500));
}

#[test]
fn errors_leave_the_atlas_as_it_was() {
    let faces = faces();
    let mut atlas = Atlas::new(64, 64).expect("a size");
    let held = key(&faces, Face::SairaRegular, 'x', 12.0, 0);
    atlas.glyph(&faces, held).expect("fits");
    let unknown = GlyphKey {
        face: FaceId(30),
        ..held
    };
    assert_eq!(
        atlas.glyph(&faces, unknown).map(|_| ()),
        Err(FontError::Faces {
            kind: fallback::ErrorKind::UnknownFace { face: FaceId(30) },
        })
    );
    // A glyph id past the font's glyphs.
    let past = GlyphKey {
        glyph: GlyphId(60_000),
        ..held
    };
    assert!(atlas.glyph(&faces, past).is_err());
    assert_eq!(atlas.len(), 1);
    assert!(atlas.contains(&held));
}

#[test]
fn another_set_of_faces_empties_the_atlas() {
    let saira_first = faces();
    let mut mono_first = Faces::new();
    for face in [Face::MonoRegular, Face::SairaRegular] {
        mono_first.add(face.parse().expect("parses")).expect("room");
    }
    let mut atlas = Atlas::new(64, 64).expect("a size");
    let key = GlyphKey {
        face: FaceId(0),
        glyph: GlyphId(36),
        size: PxSize::new(14.0).expect("a size"),
        subpixel: Subpixel::ZERO,
    };
    let saira = atlas::rasterize(&saira_first, key).expect("rasterizes");
    let mono = atlas::rasterize(&mono_first, key).expect("rasterizes");
    assert_ne!(saira, mono, "face 0 is another face in each set");
    assert!(same(&atlas.glyph(&saira_first, key).expect("fits"), &saira));
    assert!(same(&atlas.glyph(&mono_first, key).expect("fits"), &mono));
    assert_eq!(atlas.len(), 1);
    assert!(same(&atlas.glyph(&saira_first, key).expect("fits"), &saira));
}

#[test]
fn the_atlas_holds_at_most_its_glyph_count() {
    // Spaces take no room, so only the count bounds them.
    let faces = faces();
    let mut atlas = Atlas::new(16, 16).expect("a size");
    let space = key(&faces, Face::SairaRegular, ' ', 1.0, 0);
    for units in 1..=u32::try_from(MAX_GLYPHS).expect("small") + 1 {
        let key = GlyphKey {
            size: PxSize::from_units(units).expect("a size"),
            ..space
        };
        atlas.glyph(&faces, key).expect("an empty glyph");
        assert!(atlas.len() <= MAX_GLYPHS);
    }
    assert_eq!(atlas.len(), MAX_GLYPHS / 2 + 1);
    assert_eq!(atlas.evictions(), (MAX_GLYPHS / 2) as u64);
}

#[test]
fn a_notdef_with_no_outline_is_drawn_as_a_box() {
    // A face whose glyph 0 is empty: a box is made for it.
    let font = common::Font::new(vec![Vec::new(), common::simple(&[&common::square(500)])]);
    let font = Font::parse(font.build(), Limits::DEFAULT).expect("a valid font");
    let mut own = Faces::new();
    let face = own.add(font).expect("room");
    assert!(own.draws_own_notdef(face));
    // As for a glyph 0 that reaches more than an em beyond the em square,
    // or that is flat.
    for glyph in [
        common::simple(&[&common::square(2500)]),
        common::simple(&[&[(0, 0, true), (0, 500, true), (0, 900, true)]]),
    ] {
        let font = common::Font::new(vec![glyph, common::simple(&[&common::square(500)])]);
        let font = Font::parse(font.build(), Limits::DEFAULT).expect("a valid font");
        let face = own.add(font).expect("room");
        assert!(own.draws_own_notdef(face));
    }
    let notdef = GlyphKey {
        face,
        glyph: GlyphId(0),
        size: PxSize::new(20.0).expect("a size"),
        subpixel: Subpixel::ZERO,
    };
    let bitmap = atlas::rasterize(&own, notdef).expect("rasterizes");
    // From 1 to 11 px across, and from 14 px above the baseline to it (each
    // a hair less, as 0.02 is in an f32); a stroke of 1.2 px.
    assert_eq!(
        (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
        (0, -14, 11, 14)
    );
    assert_eq!(bitmap.get(1, 7), Some(255), "the left stroke");
    assert_eq!(bitmap.get(6, 7), Some(0), "the hole");
    assert_eq!(bitmap.get(6, 0), Some(255), "the top stroke");
    assert_eq!(bitmap.get(6, 13), Some(255), "the bottom stroke");
    // A bundled face's own box.
    let faces = faces();
    let own = atlas::rasterize(
        &faces,
        GlyphKey {
            face: Face::SairaRegular.id(),
            ..notdef
        },
    )
    .expect("rasterizes");
    let font = faces.font(Face::SairaRegular.id()).expect("loaded");
    let outline = font.outline(GlyphId(0)).expect("an outline");
    assert_eq!(
        own,
        tidepool_font::outline::rasterize(&outline, 0.02, 0.0, Limits::DEFAULT)
            .expect("rasterizes")
    );
}

#[test]
fn a_notdef_that_cannot_be_rasterized_is_drawn_as_a_box() {
    // Glyph 0 is a zigzag in the em square, too much work for the limits
    // its face is parsed with; the box is not.
    let mut zigzag = Vec::new();
    for step in 0..50 {
        zigzag.push((step * 20, if step % 2 == 0 { 0 } else { 700 }, true));
    }
    let limits = Limits {
        max_raster_work: 600,
        ..Limits::DEFAULT
    };
    let font = common::Font::new(vec![common::simple(&[&zigzag]), Vec::new()]);
    let font = Font::parse(font.build(), limits).expect("a valid font");
    let mut faces = Faces::new();
    let face = faces.add(font).expect("room");
    assert!(
        !faces.draws_own_notdef(face),
        "its glyph 0 reads, in the em square"
    );
    let notdef = GlyphKey {
        face,
        glyph: GlyphId(0),
        size: PxSize::new(20.0).expect("a size"),
        subpixel: Subpixel::ZERO,
    };
    let outline = faces
        .font(face)
        .and_then(|font| font.outline(GlyphId(0)).ok())
        .expect("an outline");
    assert!(tidepool_font::outline::rasterize(&outline, 0.02, 0.0, limits).is_err());
    // The box, as a face with an empty glyph 0 draws it.
    let empty = common::Font::new(vec![Vec::new(), Vec::new()]);
    let empty = Font::parse(empty.build(), Limits::DEFAULT).expect("a valid font");
    let mut boxes = Faces::new();
    let boxed = boxes.add(empty).expect("room");
    let expected = atlas::rasterize(
        &boxes,
        GlyphKey {
            face: boxed,
            ..notdef
        },
    )
    .expect("a box");
    assert_eq!(atlas::rasterize(&faces, notdef), Ok(expected));
}
