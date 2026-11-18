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

//! The suites of `tidepool-font`: the glyph atlas's fill that
//! `docs/subsystems/rendering.md` → Tests lists (M2.5).
//!
//! - `font/atlas/fill_ascii/14px`: the 95 printable ASCII glyphs of the
//!   bundled Saira Regular at 14 px, with no subpixel offset, read,
//!   rasterized and packed into an empty 256 × 256 atlas: what drawing a
//!   first line of UI text costs. The atlas, 64 KB, holds every glyph
//!   with room to spare. Each iteration starts from a new atlas,
//!   made and dropped outside the timing; the faces are parsed once, in
//!   the setup.
//!
//! Before it is measured, the suite checks, untimed, that once one fill is
//! done the atlas holds every glyph, none evicted, each the image
//! `atlas::rasterize` draws.

use tidepool_font::atlas::{self, Atlas, GlyphKey, Subpixel};
use tidepool_font::bundled::{self, Face};
use tidepool_font::fallback::Faces;
use tidepool_font::layout::PxSize;

use crate::{BenchError, Bencher, Suite};

/// The font suites, in the order a run takes them.
pub const SUITES: &[Suite] = &[Suite {
    name: "font/atlas/fill_ascii/14px",
    run: fill_ascii,
}];

/// The atlas's side, in pixels: room for every glyph, so nothing is
/// evicted.
const SIDE: u32 = 256;

/// The printable ASCII characters, space to tilde.
const ASCII: std::ops::RangeInclusive<char> = ' '..='~';

/// Filling an empty atlas with Saira Regular's printable ASCII at 14 px.
fn fill_ascii(b: &mut Bencher) -> Result<(), BenchError> {
    let faces = bundled::load()?;
    let keys = keys(&faces, Face::SairaRegular, 14.0)?;
    check(keys.len() == 95, "95 printable ASCII characters")?;
    let mut atlas = Atlas::new(SIDE, SIDE)?;
    for &key in &keys {
        atlas.glyph(&faces, key)?;
    }
    check(
        atlas.len() == keys.len() && atlas.evictions() == 0,
        "one fill holds every glyph, none evicted",
    )?;
    // Once the fill is done: asking again finds each glyph in place, and
    // writes nothing.
    for &key in &keys {
        let bitmap = atlas::rasterize(&faces, key)?;
        let image = atlas.glyph(&faces, key)?;
        let rows = (0..bitmap.height).all(|row| {
            let start = (row * bitmap.width) as usize;
            image.row(row) == bitmap.pixels.get(start..start + bitmap.width as usize)
        });
        check(
            (image.width, image.height, image.left, image.top)
                == (bitmap.width, bitmap.height, bitmap.left, bitmap.top)
                && rows,
            "after the fill, each glyph in the atlas is the image `atlas::rasterize` draws",
        )?;
    }
    check(
        atlas.len() == keys.len() && atlas.evictions() == 0,
        "asking again added and evicted nothing",
    )?;
    b.iter_batched(
        || Atlas::new(SIDE, SIDE),
        |atlas| {
            let mut atlas = atlas?;
            for &key in &keys {
                atlas.glyph(&faces, key)?;
            }
            Ok::<Atlas, tidepool_font::FontError>(atlas)
        },
    );
    Ok(())
}

/// The atlas keys of the printable ASCII characters in `face` at `px`
/// pixels per em, with no subpixel offset.
fn keys(faces: &Faces, face: Face, px: f32) -> Result<Vec<GlyphKey>, BenchError> {
    let size = PxSize::new(px).ok_or_else(|| failed(&format!("{px} px is not a size")))?;
    let font = faces
        .font(face.id())
        .ok_or_else(|| failed(&format!("{face:?} is not loaded")))?;
    ASCII
        .map(|c| {
            let glyph = font
                .glyph_index(c)
                .ok_or_else(|| failed(&format!("{face:?} has no glyph for {c:?}")))?;
            Ok(GlyphKey {
                face: face.id(),
                glyph,
                size,
                subpixel: Subpixel::ZERO,
            })
        })
        .collect()
}

/// An error naming what a suite's untimed check found wrong, unless it
/// holds.
fn check(holds: bool, what: &str) -> Result<(), BenchError> {
    if holds { Ok(()) } else { Err(failed(what)) }
}

/// The error of a suite's untimed check that found `what` wrong.
fn failed(what: &str) -> BenchError {
    BenchError::Check(format!("its check failed: {what}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::harness::{Settings, run_suite};

    /// Settings that finish at once, for unoptimized test builds.
    const QUICK: Settings = Settings {
        warm_up: Duration::ZERO,
        sample_target: Duration::from_micros(10),
        budget: Duration::ZERO,
        min_samples: 2,
        max_samples: 2,
    };

    #[test]
    fn each_suite_checks_its_fill_and_measures_one_routine() {
        for suite in SUITES {
            let summary = run_suite(suite, &QUICK).expect("the suite runs");
            assert_eq!(summary.samples, 2, "{}", suite.name);
        }
    }

    #[test]
    fn the_keys_are_the_printable_ascii_glyphs() {
        let faces = bundled::load().expect("the bundled faces load");
        let ascii = keys(&faces, Face::SairaRegular, 14.0).expect("every character is mapped");
        assert_eq!(ascii.len(), 95);
        assert!(ascii.iter().all(|key| key.size.px() == 14.0));
        let mut glyphs: Vec<_> = ascii.iter().map(|key| key.glyph).collect();
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), 95, "a glyph each");
        // A size that is none, and a face that is not loaded, say so.
        let error = keys(&faces, Face::SairaRegular, 0.0).expect_err("0 px is no size");
        assert_eq!(error.to_string(), "its check failed: 0 px is not a size");
        let error = keys(&Faces::new(), Face::MonoRegular, 14.0).expect_err("no faces");
        assert_eq!(
            error.to_string(),
            "its check failed: MonoRegular is not loaded"
        );
    }
}
