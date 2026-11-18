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

//! The golden scenes of `tidepool-font` (ADR-0044, `docs/04-quality.md` →
//! "Golden frames"): one test per scene, named after it. Glyph runs in
//! both bundled faces at 11, 12, 13, 14, 16 and 24 px; the uppercase,
//! letter-spaced labels of `docs/subsystems/ui-and-theme.md` →
//! Typography, with Saira's four weights; and fallback, truncation,
//! wrapping and tabs. Each at scale 1 and 2.
//!
//! Text is laid out with `layout`, its glyphs taken from an `Atlas`, and
//! their coverage blended here over an opaque background in the text's
//! color, `background × (255 − c) + color × c`, rounded: this crate may
//! use only `tidepool-core`, so the scenes write their own coverage rather
//! than draw it with `gfx` (ADR-0044). Each scene checks that no ink falls
//! outside its image.
//!
//! When [`DIR_VAR`] names a directory, a scene writes what it drew there
//! as `<scene>.pam`, opaque RGBA, rows from the top; `cargo xtask golden`
//! sets it, and compares each file with `tests/golden/<scene>.qoi`, within
//! a difference of 1 in a channel. Unset, as in `cargo test`, the scenes
//! draw and write nothing. `xtask`'s reader (`xtask/src/golden.rs`) is the
//! other copy of the format.

use std::env;
use std::fs;
use std::path::PathBuf;

use tidepool_font::atlas::{Atlas, GlyphKey, Subpixel};
use tidepool_font::bundled::{self, Face};
use tidepool_font::fallback::Faces;
use tidepool_font::layout::{self, Line, PxSize, Style};

/// Names the directory the scenes write to.
const DIR_VAR: &str = "TIDEPOOL_GOLDEN_DIR";

/// An opaque color: red, green and blue.
type Rgb = [u8; 3];

/// The background, as `gfx`'s scenes have it: jabbott.dev's panel color.
const BACKGROUND: Rgb = [0x10, 0x13, 0x15];
/// The theme's `text`, `text-2`, `text-3`, `teal` and `amber`
/// (`docs/subsystems/ui-and-theme.md` → Tokens).
const TEXT: Rgb = [0xee, 0xf2, 0xf3];
const TEXT_2: Rgb = [0xb3, 0xbd, 0xc1];
const TEXT_3: Rgb = [0x7d, 0x89, 0x8e];
const TEAL: Rgb = [0x5c, 0xcb, 0xcb];
const AMBER: Rgb = [0xf2, 0xc2, 0x00];

/// The sizes the glyph runs are drawn at, in logical pixels.
const SIZES: [f32; 6] = [11.0, 12.0, 13.0, 14.0, 16.0, 24.0];

/// An image being drawn, and what draws text into it.
struct Scene {
    width: i32,
    height: i32,
    pixels: Vec<Rgb>,
    scale: f32,
    faces: Faces,
    atlas: Atlas,
}

impl Scene {
    /// An image of `width × height` logical pixels at `scale`, cleared to
    /// [`BACKGROUND`].
    fn new(width: i32, height: i32, scale: i32) -> Self {
        let (width, height) = (width * scale, height * scale);
        Self {
            width,
            height,
            pixels: vec![BACKGROUND; usize::try_from(width * height).expect("positive")],
            scale: scale as f32,
            faces: bundled::load().expect("the bundled faces load"),
            atlas: Atlas::new(1024, 1024).expect("a size"),
        }
    }

    /// `face` at `size` logical pixels, tracked `tracking` ems.
    fn style(&self, face: Face, size: f32, tracking: f32) -> Style {
        Style {
            letter_spacing: tracking,
            ..Style::new(face.id(), PxSize::new(size * self.scale).expect("a size"))
        }
    }

    /// Draws `line`, laid out in `style`, from `x` logical pixels with its
    /// baseline at `baseline`, in `color`.
    fn draw(&mut self, line: &Line, style: &Style, x: f32, baseline: i32, color: Rgb) {
        let baseline = baseline * self.scale as i32;
        for glyph in &line.glyphs {
            let Some(id) = glyph.glyph else {
                continue;
            };
            let (pixel, subpixel) = Subpixel::split(x * self.scale + glyph.x);
            let key = GlyphKey {
                face: glyph.face,
                glyph: id,
                size: style.size,
                subpixel,
            };
            let image = self.atlas.glyph(&self.faces, key).expect("a glyph");
            for row in 0..image.height {
                let coverage = image.row(row).expect("inside the glyph");
                let y = baseline + image.top + i32::try_from(row).expect("small");
                for (column, &amount) in coverage.iter().enumerate() {
                    if amount == 0 {
                        continue;
                    }
                    let x = pixel + image.left + i32::try_from(column).expect("small");
                    assert!(
                        (0..self.width).contains(&x) && (0..self.height).contains(&y),
                        "ink at ({x}, {y}) is outside the {} × {} image",
                        self.width,
                        self.height
                    );
                    let index = usize::try_from(y * self.width + x).expect("inside");
                    let target = &mut self.pixels[index];
                    for (channel, &value) in target.iter_mut().zip(&color) {
                        *channel = blend(*channel, value, amount);
                    }
                }
            }
        }
    }

    /// Lays `text` out in `style` and draws it from `x` with its baseline
    /// at `baseline`, in `color`; returns the line.
    fn text(&mut self, text: &str, style: &Style, x: f32, baseline: i32, color: Rgb) -> Line {
        let line = layout::line(&self.faces, text, style).expect("a line");
        self.draw(&line, style, x, baseline, color);
        line
    }

    /// Writes the image as `<name>.pam` into the directory [`DIR_VAR`]
    /// names, if it names one.
    fn save(&self, name: &str) {
        let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
            return;
        };
        let path = PathBuf::from(dir).join(format!("{name}.pam"));
        fs::write(&path, self.pam()).unwrap_or_else(|error| {
            panic!("cannot write {}: {error}", path.display());
        });
    }

    /// The image as a PAM file: opaque RGBA, rows from the top.
    fn pam(&self) -> Vec<u8> {
        let mut file = format!(
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
            self.width, self.height
        )
        .into_bytes();
        for &[r, g, b] in &self.pixels {
            file.extend_from_slice(&[r, g, b, 255]);
        }
        file
    }
}

/// `color` over `under` with `coverage` of 255, rounded to the nearest.
fn blend(under: u8, color: u8, coverage: u8) -> u8 {
    let (under, color, coverage) = (u32::from(under), u32::from(color), u32::from(coverage));
    let value = (under * (255 - coverage) + color * coverage + 127) / 255;
    u8::try_from(value).expect("at most 255")
}

/// Each bundled Saira size, a line each, in Saira Regular.
fn saira(scale: i32) -> Scene {
    let mut scene = Scene::new(688, 172, scale);
    let mut baseline = 6;
    for size in SIZES {
        baseline += (size * 1.5) as i32;
        let style = scene.style(Face::SairaRegular, size, 0.0);
        let text = format!("{size} px · Tidepool: quick brown fox, AVA Wave To 0123456789");
        scene.text(&text, &style, 8.0, baseline, TEXT);
    }
    scene
}

#[test]
fn font_saira() {
    saira(1).save("font_saira");
}

#[test]
fn font_saira_scale_2() {
    saira(2).save("font_saira_scale_2");
}

/// Each size in JetBrains Mono Regular, then its italic and semibold and a
/// line of tabs.
fn mono(scale: i32) -> Scene {
    let mut scene = Scene::new(672, 236, scale);
    let mut baseline = 6;
    for size in SIZES {
        baseline += (size * 1.5) as i32;
        let style = scene.style(Face::MonoRegular, size, 0.0);
        let text = format!("{size:>2} px  fn main() {{ let x = 0x1F; }} // -> != ✓");
        scene.text(&text, &style, 8.0, baseline, TEXT);
    }
    let lines = [
        (Face::MonoItalic, "// a comment, in italic", TEXT_3),
        (Face::MonoSemiBold, "pub fn emphasis(&self) -> bool", TEAL),
        (Face::MonoRegular, "\tone\ttwo\tthree\tfour", TEXT_2),
        (Face::MonoRegular, "a\tbb\tccc\tdddd\teeeee|", TEXT_2),
    ];
    for (face, text, color) in lines {
        baseline += 21;
        let style = scene.style(face, 14.0, 0.0);
        scene.text(text, &style, 8.0, baseline, color);
    }
    scene
}

#[test]
fn font_mono() {
    mono(1).save("font_mono");
}

#[test]
fn font_mono_scale_2() {
    mono(2).save("font_mono_scale_2");
}

/// The chrome's uppercase, letter-spaced labels, as the typography table
/// sets them, then UI body and status text, and Saira's four weights.
fn labels(scale: i32) -> Scene {
    let mut scene = Scene::new(360, 300, scale);
    let rows: [(Face, f32, f32, &str, Rgb); 10] = [
        (Face::SairaSemiBold, 15.0, 0.28, "TIDEPOOL", TEXT),
        (Face::SairaMedium, 10.0, 0.42, "WORKSPACE", TEXT_3),
        (Face::SairaMedium, 13.0, 0.04, "SECTION TITLE", TEXT),
        (Face::SairaSemiBold, 11.0, 0.28, "EXPLORER", TEXT_3),
        (Face::SairaSemiBold, 11.0, 0.16, "TOOLCHAINS", TEAL),
        (Face::SairaSemiBold, 12.0, 0.10, "OPEN FOLDER", AMBER),
        (Face::SairaSemiBold, 12.0, 0.08, "READY · UPDATE", TEXT),
        (
            Face::SairaSemiBold,
            11.0,
            0.10,
            "PACKAGE    VERSION    STATUS",
            TEAL,
        ),
        (
            Face::SairaRegular,
            13.0,
            0.0,
            "Lists, trees and dialogs are as written.",
            TEXT,
        ),
        (
            Face::MonoRegular,
            12.0,
            0.0,
            "main.rs  12:4  UTF-8  LF  3 problems",
            TEXT_2,
        ),
    ];
    let mut baseline = 4;
    for (face, size, tracking, text, color) in rows {
        baseline += (size * 1.7) as i32;
        let style = scene.style(face, size, tracking);
        scene.text(text, &style, 8.0, baseline, color);
    }
    let weights = [
        (Face::SairaRegular, "Regular 400 — Tidepool"),
        (Face::SairaMedium, "Medium 500 — Tidepool"),
        (Face::SairaSemiBold, "SemiBold 600 — Tidepool"),
        (Face::SairaBold, "Bold 700 — Tidepool"),
    ];
    for (face, text) in weights {
        baseline += 22;
        let style = scene.style(face, 14.0, 0.0);
        scene.text(text, &style, 8.0, baseline, TEXT);
    }
    scene
}

#[test]
fn font_labels() {
    labels(1).save("font_labels");
}

#[test]
fn font_labels_scale_2() {
    labels(2).save("font_labels_scale_2");
}

/// Fallback to JetBrains Mono and to `.notdef`'s box, labels truncated to
/// three widths, a paragraph wrapped, and tab stops.
fn layout_scene(scale: i32) -> Scene {
    let mut scene = Scene::new(400, 330, scale);
    let body = scene.style(Face::SairaRegular, 14.0, 0.0);
    scene.text(
        "Saira lacks λ, ✓ and ●: JetBrains Mono has them; 中 is a box.",
        &body,
        8.0,
        24,
        TEXT,
    );
    let label = scene.style(Face::SairaSemiBold, 12.0, 0.10);
    let mut baseline = 30;
    for width in [320.0, 160.0, 90.0, 12.0] {
        baseline += 20;
        let line = layout::truncate(
            &scene.faces,
            "A LONG LABEL, CUT TO FIT ITS WIDTH",
            &label,
            width * scene.scale,
        )
        .expect("a line");
        scene.draw(&line, &label, 8.0, baseline, TEAL);
    }
    let paragraph = scene.style(Face::SairaRegular, 13.0, 0.0);
    let text = "Wrapping breaks after spaces, which hang at the end of a line; \
                a word wider than the line, like \
                Pneumonoultramicroscopicsilicovolcanoconiosis, breaks where it \
                overflows.\nA line break starts a new line.";
    let lines = layout::wrap(&scene.faces, text, &paragraph, 180.0 * scene.scale).expect("lines");
    for line in &lines {
        baseline += 19;
        scene.draw(line, &paragraph, 8.0, baseline, TEXT_2);
    }
    let code = scene.style(Face::MonoRegular, 13.0, 0.0);
    for text in ["a\tbb\tccc\tdddd", "\tfn tabbed() {}"] {
        baseline += 19;
        scene.text(text, &code, 8.0, baseline, TEXT);
    }
    scene
}

#[test]
fn font_layout() {
    layout_scene(1).save("font_layout");
}

#[test]
fn font_layout_scale_2() {
    layout_scene(2).save("font_layout_scale_2");
}
