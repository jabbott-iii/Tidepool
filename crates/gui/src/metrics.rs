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

//! [`FontMetrics`]: `ui`'s [`TextMetrics`] with `font::layout`, in logical
//! units, and [`face`], the bundled face a [`TextStyle`] is set in (M3.5,
//! ADR-0048).
//!
//! New in Tidepool.
//!
//! Text is measured as the [painter](crate::paint) draws it: laid out in
//! device pixels at the scale the metrics are made for, then divided by
//! that scale. So a text's measured advance is the advance of the glyphs
//! drawn, and the bounds `ui` damages hold them.
//!
//! # Faces
//!
//! A style is set in one of the seven bundled faces, chosen as CSS's font
//! matching chooses among a family's faces: the slant first, then the
//! weight. JetBrains Mono's one italic face is taken for an italic style at
//! any weight; otherwise a weight the family lacks takes the nearest it
//! has, lighter first for 500 and heavier first above it (500 is JetBrains
//! Mono Regular's 400, 700 its SemiBold's 600). Saira has no italic, so an
//! italic Saira style is upright: no slant is synthesized. The fallback
//! chain of the bundled faces (`font::fallback`) draws what a face lacks.

use tidepool_core::geom::Scale;
use tidepool_font::bundled::Face;
use tidepool_font::fallback::Faces;
use tidepool_font::layout::{self, Line, PxSize, Style};
use tidepool_ui::layout::MAX_EXTENT;
use tidepool_ui::metrics::{Family, LineMetrics, TextMetrics, TextStyle, Weight};

/// The bundled face `style` is set in (the [module documentation](self#faces)).
pub const fn face(style: &TextStyle) -> Face {
    match style.family {
        Family::Ui => match style.weight {
            Weight::Regular => Face::SairaRegular,
            Weight::Medium => Face::SairaMedium,
            Weight::SemiBold => Face::SairaSemiBold,
            Weight::Bold => Face::SairaBold,
        },
        Family::Mono if style.italic => Face::MonoItalic,
        Family::Mono => match style.weight {
            Weight::Regular | Weight::Medium => Face::MonoRegular,
            Weight::SemiBold | Weight::Bold => Face::MonoSemiBold,
        },
    }
}

/// `font`'s style for `style` at `scale`: its face, its size in device
/// pixels and its letter-spacing; `None` when the size times the scale is
/// not a size `font` sets: NaN, or one that, rounded to the nearest 64th
/// of a pixel, is not from 1/64 px to 2,048 px.
pub(crate) fn font_style(style: &TextStyle, scale: Scale) -> Option<Style> {
    let size = PxSize::new(style.size * scale.factor())?;
    Some(Style {
        letter_spacing: style.letter_spacing,
        ..Style::new(face(style).id(), size)
    })
}

/// Measures text with the bundled faces at a scale, in logical units, as
/// the [painter](crate::paint::Painter) draws it.
///
/// Every value is finite, from 0 to `ui`'s [`MAX_EXTENT`]. A text whose
/// size `font` cannot set ([`PxSize::new`] refuses it) measures 0 wide with
/// no ascent or descent, though its line keeps the style's height, and is
/// not drawn. Laying a text out costs a [`font::layout`] glyph a character
/// (see its "Cost"), so a widget measures what it shows.
///
/// ```
/// use tidepool_core::geom::Scale;
/// use tidepool_font::bundled;
/// use tidepool_gui::metrics::FontMetrics;
/// use tidepool_ui::metrics::{Family, TextMetrics, TextStyle};
///
/// let faces = bundled::load().expect("the bundled faces load");
/// let metrics = FontMetrics::new(&faces, Scale::ONE);
/// let style = TextStyle::new(Family::Mono, 10.0);
/// // JetBrains Mono's advance is 0.6 em.
/// assert_eq!(metrics.width("abcd", &style), 24.0);
/// assert_eq!(metrics.line(&style).height, 15.0);
/// ```
///
/// [`font::layout`]: tidepool_font::layout
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics<'a> {
    faces: &'a Faces,
    scale: Scale,
}

impl<'a> FontMetrics<'a> {
    /// Metrics of the faces in `faces`, for text drawn at `scale`.
    pub const fn new(faces: &'a Faces, scale: Scale) -> Self {
        Self { faces, scale }
    }

    /// The scale it measures for.
    pub const fn scale(&self) -> Scale {
        self.scale
    }

    /// `text` laid out in `style` at the scale, in device pixels, with
    /// `font`'s style; `None` when `font` cannot set its size.
    pub(crate) fn set(&self, text: &str, style: &TextStyle) -> Option<(Line, Style)> {
        let style = font_style(style, self.scale)?;
        let line = layout::line(self.faces, text, &style).ok()?;
        Some((line, style))
    }

    /// The advance of `line`, laid out at the scale, in logical units.
    pub(crate) fn advance(&self, line: &Line) -> f32 {
        extent(line.pen / self.scale.factor())
    }
}

impl TextMetrics for FontMetrics<'_> {
    fn line(&self, style: &TextStyle) -> LineMetrics {
        let size = non_negative(style.size);
        let height = extent(size * non_negative(style.line_height));
        let Some(font) = font_style(style, self.scale) else {
            return LineMetrics {
                ascent: 0.0,
                descent: 0.0,
                height,
            };
        };
        let Ok(vertical) = layout::metrics(self.faces, &font) else {
            return LineMetrics {
                ascent: 0.0,
                descent: 0.0,
                height,
            };
        };
        let factor = self.scale.factor();
        LineMetrics {
            ascent: extent(vertical.ascent / factor),
            descent: extent(vertical.descent / factor),
            height,
        }
    }

    fn width(&self, text: &str, style: &TextStyle) -> f32 {
        self.set(text, style)
            .map_or(0.0, |(line, _)| self.advance(&line))
    }
}

/// `value` if it is finite and positive, else 0.
fn non_negative(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// `value` clamped to 0 to [`MAX_EXTENT`], with NaN as 0.
fn extent(value: f32) -> f32 {
    if value.is_nan() || value <= 0.0 {
        0.0
    } else {
        value.min(MAX_EXTENT)
    }
}

#[cfg(test)]
mod tests {
    use tidepool_font::bundled;

    use super::*;

    fn faces() -> Faces {
        bundled::load().expect("the bundled faces load")
    }

    #[test]
    fn each_style_takes_the_face_css_would() {
        let ui = |weight| TextStyle::new(Family::Ui, 12.0).weight(weight);
        let mono = |weight| TextStyle::new(Family::Mono, 12.0).weight(weight);
        assert_eq!(face(&ui(Weight::Regular)), Face::SairaRegular);
        assert_eq!(face(&ui(Weight::Medium)), Face::SairaMedium);
        assert_eq!(face(&ui(Weight::SemiBold)), Face::SairaSemiBold);
        assert_eq!(face(&ui(Weight::Bold)), Face::SairaBold);
        // No Saira italic: upright, at its weight.
        assert_eq!(face(&ui(Weight::Bold).italic()), Face::SairaBold);
        assert_eq!(face(&mono(Weight::Regular)), Face::MonoRegular);
        assert_eq!(face(&mono(Weight::Medium)), Face::MonoRegular);
        assert_eq!(face(&mono(Weight::SemiBold)), Face::MonoSemiBold);
        assert_eq!(face(&mono(Weight::Bold)), Face::MonoSemiBold);
        // The slant before the weight.
        for weight in [
            Weight::Regular,
            Weight::Medium,
            Weight::SemiBold,
            Weight::Bold,
        ] {
            assert_eq!(face(&mono(weight).italic()), Face::MonoItalic);
        }
    }

    #[test]
    fn widths_are_the_drawn_advance_in_logical_units() {
        let faces = faces();
        let style = TextStyle::new(Family::Ui, 13.0);
        let one = FontMetrics::new(&faces, Scale::ONE);
        let font = Style::new(Face::SairaRegular.id(), PxSize::new(13.0).expect("a size"));
        let line = layout::line(&faces, "Open folder ", &font).expect("a line");
        // The pen, trailing space included.
        assert_eq!(one.width("Open folder ", &style), line.pen);
        assert!(line.pen > line.width);
        assert_eq!(one.width("", &style), 0.0);

        // At scale 2, laid out at 26 px and halved.
        let two = FontMetrics::new(&faces, Scale::new(2.0).expect("a scale"));
        let font = Style::new(Face::SairaRegular.id(), PxSize::new(26.0).expect("a size"));
        let line = layout::line(&faces, "Open folder", &font).expect("a line");
        assert_eq!(two.width("Open folder", &style), line.pen / 2.0);

        // Letter-spacing between characters, in ems, as `font` adds it.
        let tracked = style.letter_spacing(0.28);
        let spaced = Style {
            letter_spacing: 0.28,
            ..Style::new(Face::SairaRegular.id(), PxSize::new(13.0).expect("a size"))
        };
        let line = layout::line(&faces, "EXPLORER", &spaced).expect("a line");
        assert_eq!(one.width("EXPLORER", &tracked), line.pen);
        assert!(one.width("EXPLORER", &tracked) > one.width("EXPLORER", &style));
    }

    #[test]
    fn line_metrics_are_the_faces_and_the_line_height_the_styles() {
        let faces = faces();
        let style = TextStyle::new(Family::Mono, 14.0);
        let one = FontMetrics::new(&faces, Scale::ONE);
        let font = Style::new(Face::MonoRegular.id(), PxSize::new(14.0).expect("a size"));
        let vertical = layout::metrics(&faces, &font).expect("metrics");
        let line = one.line(&style);
        assert_eq!(
            (line.ascent, line.descent),
            (vertical.ascent, vertical.descent)
        );
        assert_eq!(line.height, 21.0);
        assert_eq!(one.line(&style.line_height(2.0)).height, 28.0);
        assert_eq!(one.measure("ab", &style).height, 21.0);
        // Within a 64th of a device pixel at scale 2.
        let two = FontMetrics::new(&faces, Scale::new(2.0).expect("a scale")).line(&style);
        assert!((two.ascent - line.ascent).abs() < 1.0 / 64.0, "{two:?}");
        assert_eq!(two.height, 21.0);
    }

    #[test]
    fn a_size_font_cannot_set_is_0_wide_with_no_ascent_and_keeps_its_line_height() {
        let faces = faces();
        let one = FontMetrics::new(&faces, Scale::ONE);
        let four = FontMetrics::new(&faces, Scale::new(4.0).expect("a scale"));
        for size in [0.0, -3.0, f32::NAN, f32::INFINITY, 1e-4, 4096.0] {
            let style = TextStyle::new(Family::Ui, size);
            assert_eq!(one.width("abc", &style), 0.0, "{size}");
            let line = one.line(&style);
            assert_eq!((line.ascent, line.descent), (0.0, 0.0), "{size}");
        }
        // Too large to set, or under 1/128 px: its line keeps its height.
        for size in [4096.0, 1.0 / 256.0] {
            let style = TextStyle::new(Family::Ui, size);
            assert_eq!(one.width("abc", &style), 0.0, "{size}");
            assert_eq!(one.line(&style).height, size * 1.5, "{size}");
        }
        // 600 logical px is 2,400 device px at scale 4: beyond `font`.
        let large = TextStyle::new(Family::Ui, 600.0);
        assert!(one.width("abc", &large) > 0.0);
        assert_eq!(four.width("abc", &large), 0.0);
    }

    #[test]
    fn every_value_is_finite_and_within_the_extent() {
        let faces = faces();
        let odd = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            f32::MAX,
            1e-30,
            12.0,
        ];
        let texts = ["", "x", "hello, world", "\u{1F600}\t\u{301}"];
        for scale in [
            Scale::ONE,
            Scale::new(1e-6).expect("tiny"),
            Scale::new(1e6).expect("huge"),
        ] {
            let metrics = FontMetrics::new(&faces, scale);
            for &size in &odd {
                for &spacing in &odd {
                    for &line_height in &odd {
                        let style = TextStyle::new(Family::Ui, size)
                            .letter_spacing(spacing)
                            .line_height(line_height);
                        let line = metrics.line(&style);
                        for value in [line.ascent, line.descent, line.height] {
                            assert!((0.0..=MAX_EXTENT).contains(&value), "{value} for {style:?}");
                        }
                        for text in texts {
                            let width = metrics.width(text, &style);
                            assert!((0.0..=MAX_EXTENT).contains(&width), "{width} for {style:?}");
                        }
                    }
                }
            }
        }
    }
}
