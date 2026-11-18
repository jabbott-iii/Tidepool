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

//! The first frame (M2.6, ADR-0045): jabbott.dev's backdrop, and over it a
//! line in each of the seven bundled faces.
//!
//! New in Tidepool.
//!
//! [`FirstFrame::draw`] renders the backdrop for the window's size and
//! scale with `gfx` ([`Backdrop`], rebuilt only when either changes),
//! copies it into a new [`Framebuffer`], and sets the lines with `font`:
//! each names its face in `text-3`, then shows a sample in `text`, both in
//! that face, at a size and tracking from the typography table of
//! `docs/subsystems/ui-and-theme.md`, in the two text colors of the bundled
//! `tidepool-dark` theme ([`Theme::tidepool_dark`], M3.2, ADR-0047); the
//! backdrop is `gfx`'s [`BackdropStyle::jabbott_dev`], whose `bg` a test
//! holds to the theme's. Saira's chrome is uppercase; Saira
//! 400's body text and JetBrains Mono's code are as written. The labels
//! start 48 logical px from the left, the samples in one column after the
//! widest label, and the baselines are 32 logical px apart from 72 px
//! down. Glyphs come from one [`Atlas`] and are blended with
//! [`glyph::draw`]; what falls outside the window is clipped.
//!
//! It draws on the thread that calls it, and the backdrop is most of its
//! cost: about 100 ms for 1280 × 800 at scale 1 in a release build on the
//! reference machine, and 190 ms for 1920 × 1080 (ADR-0045).

use tidepool_core::color::Color;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Scale};
use tidepool_font::atlas::{Atlas, GlyphKey, Subpixel};
use tidepool_font::bundled::{self, Face};
use tidepool_font::fallback::Faces;
use tidepool_font::layout::{self, Line, PxSize, Style};
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gfx::glyph::{self, Mask};
use tidepool_gfx::image;

use tidepool_ui::theme::{Theme, Token};

use crate::GuiError;

/// One line of the frame: a face, how it is set, and what it says.
struct Sample {
    face: Face,
    /// Pixels per em, in logical px.
    size: f32,
    /// Letter-spacing, in ems.
    tracking: f32,
    /// The face's name, in `text-3`.
    label: &'static str,
    /// What it shows, in `text`.
    text: &'static str,
}

/// The lines, one a face, in the order of [`Face::ALL`], each set as its
/// role in the typography table: UI body, section title, the brand, a
/// tracked label (Saira 700 has no role of its own), and code, a comment
/// and emphasis in code.
const SAMPLES: [Sample; 7] = [
    Sample {
        face: Face::SairaRegular,
        size: 13.0,
        tracking: 0.0,
        label: "Saira Regular 400",
        text: "UI body: lists, the file tree and dialogs, as written",
    },
    Sample {
        face: Face::SairaMedium,
        size: 13.0,
        tracking: 0.04,
        label: "SAIRA MEDIUM 500",
        text: "SECTION TITLE",
    },
    Sample {
        face: Face::SairaSemiBold,
        size: 15.0,
        tracking: 0.28,
        label: "SAIRA SEMIBOLD 600",
        text: "TIDEPOOL",
    },
    Sample {
        face: Face::SairaBold,
        size: 12.0,
        tracking: 0.10,
        label: "SAIRA BOLD 700",
        text: "RUST · MARKDOWN · ANDROID · IOS",
    },
    Sample {
        face: Face::MonoRegular,
        size: 14.0,
        tracking: 0.0,
        label: "JetBrains Mono Regular 400",
        text: "fn main() -> Result<(), Error> { Ok(()) }",
    },
    Sample {
        face: Face::MonoItalic,
        size: 14.0,
        tracking: 0.0,
        label: "JetBrains Mono Italic 400",
        text: "// a comment, in italic",
    },
    Sample {
        face: Face::MonoSemiBold,
        size: 14.0,
        tracking: 0.0,
        label: "JetBrains Mono SemiBold 600",
        text: "let emphasis = 600;",
    },
];

/// The labels' left edge, in logical px.
const LEFT: f32 = 48.0;
/// The first line's baseline, in logical px from the top.
const TOP: f32 = 72.0;
/// From one baseline to the next, in logical px.
const PITCH: f32 = 32.0;
/// From the right of the widest label to the samples, in logical px.
const GAP: f32 = 32.0;
/// The atlas's side, in pixels: room for every glyph of the frame at
/// scale 2 without evicting, and a glyph limit of half that, 512 px.
const ATLAS_SIDE: u32 = 1024;

/// What draws the first frame: the bundled faces, a glyph atlas and the
/// backdrop's cache.
#[derive(Debug)]
pub struct FirstFrame {
    faces: Faces,
    atlas: Atlas,
    backdrop: Backdrop,
    style: BackdropStyle,
    /// The theme's `text`, for the samples.
    text: Color,
    /// The theme's `text-3`, for the labels.
    muted: Color,
}

impl FirstFrame {
    /// Loads the bundled faces, under 1 ms (M2.5), and the bundled theme's
    /// two text colors, and makes an empty atlas and backdrop cache;
    /// nothing is drawn yet.
    ///
    /// # Errors
    ///
    /// [`GuiError::Font`] if a bundled face does not parse or the atlas
    /// cannot be made, which would be a bug.
    pub fn new() -> Result<Self, GuiError> {
        let theme = Theme::tidepool_dark();
        Ok(Self {
            faces: bundled::load()?,
            atlas: Atlas::new(ATLAS_SIDE, ATLAS_SIDE)?,
            backdrop: Backdrop::new(),
            style: BackdropStyle::jabbott_dev(),
            text: theme.color(Token::Text),
            muted: theme.color(Token::Text3),
        })
    }

    /// Draws the frame for a window of `size` device pixels at `scale`,
    /// into a new framebuffer of that size.
    ///
    /// The backdrop is rendered only if the size or the scale differ from
    /// the last call's. A line whose size in device pixels `font` cannot
    /// set (beyond 2,048 px) is left out.
    ///
    /// # Errors
    ///
    /// - [`GuiError::Gfx`] if the size has a negative side or one over
    ///   `gfx`'s 16,384 px, or the memory for the pixels cannot be had.
    /// - [`GuiError::Font`] if a line cannot be laid out or a glyph cannot
    ///   be rasterized into the atlas.
    pub fn draw(&mut self, size: DeviceSize, scale: Scale) -> Result<Framebuffer, GuiError> {
        self.backdrop.ensure(size, scale, &self.style)?;
        let mut frame = Framebuffer::new(size)?;
        let all = frame.bounds();
        image::copy(
            &mut frame,
            all,
            &self.backdrop.base().view(),
            DevicePoint::ZERO,
        );
        self.draw_lines(&mut frame, scale)?;
        Ok(frame)
    }

    /// Sets and draws [`SAMPLES`] into `frame`.
    fn draw_lines(&mut self, frame: &mut Framebuffer, scale: Scale) -> Result<(), GuiError> {
        let factor = scale.factor();
        let mut lines = Vec::with_capacity(SAMPLES.len());
        for (index, sample) in SAMPLES.iter().enumerate() {
            let Some(size) = PxSize::new(sample.size * factor) else {
                continue;
            };
            let style = Style {
                letter_spacing: sample.tracking,
                ..Style::new(sample.face.id(), size)
            };
            let label = layout::line(&self.faces, sample.label, &style)?;
            let text = layout::line(&self.faces, sample.text, &style)?;
            lines.push((index, style, label, text));
        }
        let widest = lines
            .iter()
            .map(|(_, _, label, _)| label.width)
            .fold(0.0, f32::max);
        let left = LEFT * factor;
        let column = left + widest + GAP * factor;
        let clip = frame.bounds();
        for (index, style, label, text) in &lines {
            // At most 7 lines: the index is exact as an `f32`.
            let baseline = device((TOP + PITCH * *index as f32) * factor);
            let (text_color, muted) = (self.text, self.muted);
            self.draw_line(frame, clip, (label, style), (left, baseline), muted)?;
            self.draw_line(frame, clip, (text, style), (column, baseline), text_color)?;
        }
        Ok(())
    }

    /// Draws `line`, laid out in `style`, from the pen at `x` device pixels
    /// on the baseline's row, in `color`, inside `clip`.
    fn draw_line(
        &mut self,
        frame: &mut Framebuffer,
        clip: DeviceRect,
        (line, style): (&Line, &Style),
        (x, baseline): (f32, i32),
        color: Color,
    ) -> Result<(), GuiError> {
        let color = color.premultiply();
        for placed in &line.glyphs {
            // A tab draws nothing.
            let Some(id) = placed.glyph else {
                continue;
            };
            let (pixel, subpixel) = Subpixel::split(x + placed.x);
            let key = GlyphKey {
                face: placed.face,
                glyph: id,
                size: style.size,
                subpixel,
            };
            let image = self.atlas.glyph(&self.faces, key)?;
            if image.width == 0 || image.height == 0 {
                continue;
            }
            let atlas = Mask::new(
                image.pixels(),
                DeviceSize::new(side(image.atlas_width()), side(image.atlas_height())),
                image.atlas_width() as usize,
            )?;
            let mask = atlas.region(DeviceRect::new(
                side(image.x),
                side(image.y),
                side(image.width),
                side(image.height),
            ))?;
            let at = DevicePoint::new(
                pixel.saturating_add(image.left),
                baseline.saturating_add(image.top),
            );
            glyph::draw(frame, clip, &mask, at, color);
        }
        Ok(())
    }
}

/// A logical position times the scale, rounded to a whole device pixel.
fn device(value: f32) -> i32 {
    // `as` saturates, and NaN becomes 0.
    (value + 0.5).floor() as i32
}

/// A place or a side in the atlas, as `gfx` takes it. The atlas is at most
/// `font::atlas::MAX_SIDE` (4,096) a side, so it fits; anything else would
/// make the mask refuse it.
fn side(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(size: DeviceSize, scale: f32) -> Framebuffer {
        let scale = Scale::new(scale).expect("a scale");
        FirstFrame::new()
            .expect("the faces load")
            .draw(size, scale)
            .expect("the frame draws")
    }

    /// The backdrop alone, as the frame copies it.
    fn backdrop(size: DeviceSize, scale: f32) -> Framebuffer {
        let mut backdrop = Backdrop::new();
        let scale = Scale::new(scale).expect("a scale");
        backdrop
            .ensure(size, scale, &BackdropStyle::jabbott_dev())
            .expect("the backdrop");
        backdrop.base().try_clone().expect("a copy")
    }

    /// The bounding box of the pixels where `a` and `b` differ.
    fn changed(a: &Framebuffer, b: &Framebuffer) -> Option<DeviceRect> {
        let size = a.size();
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..size.height {
            for x in 0..size.width {
                let at = DevicePoint::new(x, y);
                if a.pixel(at) != b.pixel(at) {
                    (left, top) = (left.min(x), top.min(y));
                    (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
                }
            }
        }
        (left <= right).then(|| DeviceRect::new(left, top, right - left, bottom - top))
    }

    #[test]
    fn every_bundled_face_has_a_line_in_order() {
        let faces: Vec<Face> = SAMPLES.iter().map(|sample| sample.face).collect();
        assert_eq!(faces, Face::ALL);
    }

    #[test]
    fn saira_chrome_is_uppercase_and_body_and_code_are_as_written() {
        for sample in &SAMPLES {
            let chrome = matches!(
                sample.face,
                Face::SairaMedium | Face::SairaSemiBold | Face::SairaBold
            );
            for text in [sample.label, sample.text] {
                assert_eq!(chrome, text == text.to_uppercase(), "{text}");
            }
        }
    }

    #[test]
    fn the_text_is_drawn_over_the_backdrop_where_the_lines_go() {
        let size = DeviceSize::new(1232, 662);
        let drawn = frame(size, 1.0);
        assert_eq!(drawn.size(), size);
        let ink = changed(&drawn, &backdrop(size, 1.0)).expect("the lines change pixels");
        // From the labels' left edge, about the first line's ascent above
        // its baseline, to below the last.
        let last = device(TOP + PITCH * 6.0);
        assert!((48..=50).contains(&ink.x), "{ink:?}");
        assert!((56..72).contains(&ink.y), "{ink:?}");
        assert!(ink.bottom() > last && ink.bottom() <= last + 8, "{ink:?}");
        assert!(ink.right() < 1232, "{ink:?}");
    }

    #[test]
    fn the_lines_use_the_themes_text_colors() {
        let size = DeviceSize::new(800, 300);
        let drawn = frame(size, 1.0);
        let mut colors = std::collections::HashSet::new();
        for y in 0..size.height {
            for x in 0..size.width {
                if let Some(pixel) = drawn.pixel(DevicePoint::new(x, y)) {
                    colors.insert(pixel);
                }
            }
        }
        // Fully covered pixels of a glyph are the tokens' colors exactly:
        // the spec's `#eef2f3` and `#7d898e`.
        let theme = Theme::tidepool_dark();
        assert_eq!(theme.color(Token::Text), Color::rgb(0xee, 0xf2, 0xf3));
        assert_eq!(theme.color(Token::Text3), Color::rgb(0x7d, 0x89, 0x8e));
        assert!(colors.contains(&theme.color(Token::Text).premultiply()));
        assert!(colors.contains(&theme.color(Token::Text3).premultiply()));
    }

    #[test]
    fn the_backdrops_bg_is_the_themes() {
        // The theme carries no backdrop style yet (ADR-0047), so the frame
        // draws `gfx`'s jabbott.dev style: its `bg` must stay the token
        // that `cargo xtask theme` measures text against.
        let theme = Theme::tidepool_dark();
        assert_eq!(BackdropStyle::jabbott_dev().bg, theme.color(Token::Bg));
    }

    #[test]
    fn drawing_again_gives_the_same_bytes_and_scale_two_draws_twice_as_far() {
        let mut first = FirstFrame::new().expect("the faces load");
        let size = DeviceSize::new(640, 400);
        let once = first.draw(size, Scale::ONE).expect("a frame");
        let twice = first.draw(size, Scale::ONE).expect("a frame");
        assert_eq!(once.bytes(), twice.bytes());
        assert_eq!(once.bytes(), frame(size, 1.0).bytes());

        let large = DeviceSize::new(1280, 800);
        let double = first
            .draw(large, Scale::new(2.0).expect("a scale"))
            .expect("a frame");
        let ink = changed(&double, &backdrop(large, 2.0)).expect("the lines");
        assert!((96..=100).contains(&ink.x), "{ink:?}");
        assert!((112..144).contains(&ink.y), "{ink:?}");
    }

    #[test]
    fn small_and_empty_windows_get_a_frame_of_their_size() {
        for (width, height) in [(1, 1), (40, 40), (300, 90)] {
            let size = DeviceSize::new(width, height);
            assert_eq!(frame(size, 1.0).size(), size);
        }
        let empty = frame(DeviceSize::new(0, 0), 1.0);
        assert!(empty.bytes().is_empty());
    }

    #[test]
    fn a_line_lands_on_its_quarter_pixel_and_moves_with_whole_ones() {
        let mut first = FirstFrame::new().expect("the faces load");
        let style = Style::new(Face::SairaRegular.id(), PxSize::new(14.0).expect("a size"));
        let line = layout::line(&first.faces, "Hamburgefonts", &style).expect("a line");
        let size = DeviceSize::new(200, 40);
        let mut drawn_at = |x: f32| {
            let mut frame = Framebuffer::new(size).expect("a frame");
            frame.clear(Color::rgb(0, 0, 0).premultiply());
            let clip = frame.bounds();
            first
                .draw_line(&mut frame, clip, (&line, &style), (x, 30), Color::WHITE)
                .expect("the line draws");
            frame
        };
        let whole = drawn_at(10.0);
        let quarter = drawn_at(10.25);
        let next = drawn_at(11.0);
        assert_ne!(
            whole.bytes(),
            quarter.bytes(),
            "a quarter pixel changes the coverage"
        );
        assert_ne!(whole.bytes(), next.bytes());
        for y in 0..size.height {
            for x in 0..size.width - 1 {
                assert_eq!(
                    whole.pixel(DevicePoint::new(x, y)),
                    next.pixel(DevicePoint::new(x + 1, y)),
                    "a whole pixel moves the line by one, at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_size_gfx_cannot_draw_is_an_error() {
        let mut first = FirstFrame::new().expect("the faces load");
        let error = first
            .draw(DeviceSize::new(-1, 10), Scale::ONE)
            .expect_err("a negative side");
        assert!(matches!(error, GuiError::Gfx(_)), "{error:?}");
        // The cache is still usable.
        assert!(first.draw(DeviceSize::new(8, 8), Scale::ONE).is_ok());
    }
}
