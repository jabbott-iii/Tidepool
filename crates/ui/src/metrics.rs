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

//! Text measurement: [`TextStyle`], [`LineMetrics`], the [`TextMetrics`]
//! trait the frontends implement, and [`FixedMetrics`], a fixed-advance
//! implementation for tests and headless use.
//!
//! New in Tidepool; not ported.
//!
//! `ui` knows nothing about fonts or cells. Widgets measure text through a
//! [`TextMetrics`] that the frontend hands to [`Tree::layout`] and
//! [`Tree::frame`]: `gui` implements it with `font::layout`, in logical
//! pixels (M3.5); `tui` with cells, one unit to a cell (M7.1).
//!
//! [`Tree::layout`]: crate::tree::Tree::layout
//! [`Tree::frame`]: crate::tree::Tree::frame

use tidepool_core::geom::Size;

use crate::layout::MAX_EXTENT;

/// A typeface family: one of the two bundled faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    /// Saira: chrome, labels, buttons and UI body text.
    #[default]
    Ui,
    /// JetBrains Mono: code, the terminal, numbers and paths.
    Mono,
}

/// A font weight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weight {
    /// 400.
    #[default]
    Regular,
    /// 500.
    Medium,
    /// 600.
    SemiBold,
    /// 700.
    Bold,
}

/// How a run of text is set: family, weight, slant, size, tracking and
/// line height, as the spec's typography table gives them.
///
/// ```
/// use tidepool_ui::metrics::{Family, TextStyle, Weight};
///
/// // The eyebrow over a panel group: Saira 11 px, 600, tracked 0.28em.
/// let eyebrow = TextStyle::new(Family::Ui, 11.0).weight(Weight::SemiBold).letter_spacing(0.28);
/// assert_eq!(eyebrow.line_height, 1.5);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// The face.
    pub family: Family,
    /// The weight.
    pub weight: Weight,
    /// Italic, as for comments and ghost text.
    pub italic: bool,
    /// The size: logical units per em.
    pub size: f32,
    /// Extra space after each character but the last, in ems, as CSS's
    /// `letter-spacing: 0.28em`.
    pub letter_spacing: f32,
    /// The height of a line, as a multiple of [`size`](Self::size).
    pub line_height: f32,
}

impl TextStyle {
    /// A style of `family` at `size`: regular, upright, no letter-spacing,
    /// and a line height of 1.5.
    pub const fn new(family: Family, size: f32) -> Self {
        Self {
            family,
            weight: Weight::Regular,
            italic: false,
            size,
            letter_spacing: 0.0,
            line_height: 1.5,
        }
    }

    /// The same style at `weight`.
    pub const fn weight(self, weight: Weight) -> Self {
        Self { weight, ..self }
    }

    /// The same style, italic.
    pub const fn italic(self) -> Self {
        Self {
            italic: true,
            ..self
        }
    }

    /// The same style with `em` ems of letter-spacing.
    pub const fn letter_spacing(self, em: f32) -> Self {
        Self {
            letter_spacing: em,
            ..self
        }
    }

    /// The same style with lines `multiple` times its size apart.
    pub const fn line_height(self, multiple: f32) -> Self {
        Self {
            line_height: multiple,
            ..self
        }
    }
}

/// The vertical metrics of a line of text, in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineMetrics {
    /// From the baseline up to the top of the tallest glyphs.
    pub ascent: f32,
    /// From the baseline down to the bottom of the deepest glyphs.
    pub descent: f32,
    /// The height of the line box: the style's size times its line height.
    pub height: f32,
}

/// Measures text, for layout and for the bounds of a draw list's text.
///
/// `gui` implements it with `font::layout`, in logical pixels (M3.5);
/// `tui` with cells, one unit to a cell (M7.1). The methods are
/// infallible: an implementation that cannot measure something returns 0,
/// and never panics. Every value returned is finite and not negative.
pub trait TextMetrics {
    /// The vertical metrics of a line in `style`.
    fn line(&self, style: &TextStyle) -> LineMetrics;

    /// The advance of `text` set on one line in `style`, letter-spacing
    /// included.
    fn width(&self, text: &str, style: &TextStyle) -> f32;

    /// The size of `text` set on one line: its [`width`](Self::width) and
    /// its [line](Self::line)'s height.
    fn measure(&self, text: &str, style: &TextStyle) -> Size {
        Size::new(self.width(text, style), self.line(style).height)
    }

    /// How many device pixels a logical unit is in the window these
    /// metrics measure for: `gui`'s are its scale's (2 on a display at
    /// 200%). The tree keeps what the last
    /// [`layout`](crate::Tree::layout) was given, for what it measures in
    /// device pixels, such as the slop of the press guard
    /// ([`overlay`](crate::overlay#kinds), ADR-0056). 1 by default.
    fn scale(&self) -> f32 {
        1.0
    }
}

/// Metrics in which every character has the same advance: for tests, and
/// for headless use such as `editor`'s command tests.
///
/// The fields are in ems. A text's width is its characters times the
/// advance times the size, plus the letter-spacing times the size for each
/// character but the last. Characters are Unicode scalar values
/// (`str::chars`), not grapheme clusters: `ui` depends only on `core`,
/// which has no grapheme tables. A size, field or line height that is not
/// finite or is negative counts as 0, as does a letter-spacing that is not
/// finite; every result is clamped to 0 to [`MAX_EXTENT`].
///
/// ```
/// use tidepool_ui::metrics::{Family, FixedMetrics, TextMetrics, TextStyle};
///
/// let style = TextStyle::new(Family::Mono, 10.0);
/// assert_eq!(FixedMetrics::DEFAULT.width("abcd", &style), 24.0);
/// assert_eq!(FixedMetrics::DEFAULT.line(&style).height, 15.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedMetrics {
    /// The advance of every character, in ems.
    pub advance: f32,
    /// The ascent, in ems.
    pub ascent: f32,
    /// The descent, in ems.
    pub descent: f32,
}

impl FixedMetrics {
    /// An advance of 0.6 em, an ascent of 0.8 em and a descent of 0.2 em,
    /// close to JetBrains Mono's.
    pub const DEFAULT: Self = Self {
        advance: 0.6,
        ascent: 0.8,
        descent: 0.2,
    };
}

impl Default for FixedMetrics {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TextMetrics for FixedMetrics {
    fn line(&self, style: &TextStyle) -> LineMetrics {
        let size = non_negative(style.size);
        LineMetrics {
            ascent: extent(non_negative(self.ascent) * size),
            descent: extent(non_negative(self.descent) * size),
            height: extent(size * non_negative(style.line_height)),
        }
    }

    fn width(&self, text: &str, style: &TextStyle) -> f32 {
        let size = non_negative(style.size);
        let chars = text.chars().count() as f32;
        let spacing = if style.letter_spacing.is_finite() {
            style.letter_spacing
        } else {
            0.0
        };
        let gaps = (chars - 1.0).max(0.0);
        extent(chars * non_negative(self.advance) * size + spacing * size * gaps)
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
    use super::*;

    #[test]
    fn fixed_metrics_count_scalar_values_and_add_spacing_between_them() {
        let metrics = FixedMetrics::DEFAULT;
        let style = TextStyle::new(Family::Ui, 10.0);
        assert_eq!(metrics.width("", &style), 0.0);
        assert_eq!(metrics.width("a", &style), 6.0);
        // "é" as `e` and a combining accent is two scalar values.
        assert_eq!(metrics.width("e\u{301}", &style), 12.0);
        // 0.5 em of spacing at 10 units is 5 units, between 3 characters.
        let tracked = style.letter_spacing(0.5);
        assert_eq!(metrics.width("abc", &tracked), 18.0 + 10.0);
        assert_eq!(metrics.width("a", &tracked), 6.0);
        // Negative spacing is allowed but never makes a width negative.
        let tight = style.letter_spacing(-5.0);
        assert_eq!(metrics.width("abc", &tight), 0.0);

        let line = metrics.line(&style);
        assert_eq!(
            line,
            LineMetrics {
                ascent: 8.0,
                descent: 2.0,
                height: 15.0
            }
        );
        assert_eq!(metrics.line(&style.line_height(2.0)).height, 20.0);
        assert_eq!(metrics.measure("ab", &style), Size::new(12.0, 15.0));
        assert_eq!(FixedMetrics::default(), FixedMetrics::DEFAULT);
    }

    #[test]
    fn fixed_metrics_are_finite_and_non_negative_for_any_style() {
        let odd = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            f32::MAX,
            f32::MIN,
            1e-40,
            12.0,
        ];
        let texts = ["", "x", "hello, world", "\u{1F600}\u{1F600}"];
        for &size in &odd {
            for &spacing in &odd {
                for &line_height in &odd {
                    for &field in &odd {
                        let metrics = FixedMetrics {
                            advance: field,
                            ascent: field,
                            descent: field,
                        };
                        let style = TextStyle::new(Family::Mono, size)
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

    #[test]
    fn text_style_builders_set_one_field_each() {
        let base = TextStyle::new(Family::Mono, 14.0);
        assert_eq!(
            base,
            TextStyle {
                family: Family::Mono,
                weight: Weight::Regular,
                italic: false,
                size: 14.0,
                letter_spacing: 0.0,
                line_height: 1.5,
            }
        );
        let styled = base
            .weight(Weight::Bold)
            .italic()
            .letter_spacing(0.1)
            .line_height(1.2);
        assert_eq!(styled.weight, Weight::Bold);
        assert!(styled.italic);
        assert_eq!(styled.letter_spacing, 0.1);
        assert_eq!(styled.line_height, 1.2);
        assert_eq!((styled.family, styled.size), (Family::Mono, 14.0));
    }
}
