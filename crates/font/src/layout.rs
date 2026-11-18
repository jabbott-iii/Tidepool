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

//! Laying out text: [`line`], [`truncate`] and [`wrap`] turn a string in a
//! [`Style`] into [`Line`]s of positioned [`Glyph`]s, and [`metrics`] gives
//! a style's ascent and descent.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL).
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::bundled::{self, Face};
//! use tidepool_font::layout::{self, PxSize, Style};
//!
//! let faces = bundled::load()?;
//! let size = PxSize::new(12.0).expect("a size");
//! // A button's label: Saira SemiBold, 12 px, uppercase, tracked 0.10em.
//! let style = Style {
//!     letter_spacing: 0.10,
//!     ..Style::new(Face::SairaSemiBold.id(), size)
//! };
//! let label = layout::line(&faces, "OPEN FOLDER", &style)?;
//! assert_eq!(label.glyphs.len(), 11);
//! assert!(label.width > 80.0 && label.width < 100.0);
//! // Too wide for 60 px: cut, with an ellipsis.
//! let cut = layout::truncate(&faces, "OPEN FOLDER", &style, 60.0)?;
//! assert!(cut.truncated && cut.width <= 60.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Units and positions
//!
//! Everything is in device pixels, f32, x growing to the right from the
//! line's start: a [`Style`]'s size is pixels per em at the window's
//! scale (13 logical pixels at scale 2 is 26). A [`Glyph`]'s `x` is where
//! the pen is when it is drawn, on the baseline; the painter adds the
//! line's origin, splits that into a whole pixel and a quarter
//! ([`Subpixel::split`](crate::atlas::Subpixel::split)), and draws the
//! glyph's [`atlas`](crate::atlas) image there, the baseline on a whole
//! pixel. The arithmetic is IEEE 754 `+ − × ÷` and `floor`, in a fixed
//! order, so a layout is the same on every OS.
//!
//! # What a line does
//!
//! Each character is resolved through the [`Faces`]' fallback chain
//! ([`Faces::resolve`](crate::fallback::Faces::resolve)), one glyph a
//! character: there is no shaping, so no ligatures, marks placed over
//! their bases, bidi or complex scripts (out of scope in v1,
//! `docs/subsystems/rendering.md`). Then, from the start of the line:
//!
//! - **Advance:** the pen moves by each glyph's advance (`hmtx`), scaled
//!   to the size; a `.notdef` box this crate makes is
//!   [`NOTDEF_ADVANCE`] of an em.
//! - **Kerning:** between two glyphs of the same face, the face's kerning
//!   ([`Font::kerning`](crate::ttf::Font::kerning)) moves the second; never
//!   across faces, and never with `.notdef`.
//! - **Letter-spacing:** [`Style::letter_spacing`] ems between each glyph
//!   and the next, as CSS's `letter-spacing` adds it, but not after the
//!   last glyph, so a label's width is its ink: uppercase chrome labels
//!   are tracked this way (`docs/subsystems/ui-and-theme.md` →
//!   Typography). Case is the caller's: `layout` never changes it.
//! - **Tabs:** a tab moves the pen to the next stop, every
//!   [`Style::tab_size`] advances of a space from the line's start (a pen
//!   already within 1/256 of a stop of one goes to the next); it draws
//!   nothing, and neither kerning nor letter-spacing comes before or
//!   after it.
//! - Every other character, control characters too, is drawn by the glyph
//!   it resolves to; in [`line`] and [`truncate`], that includes line
//!   breaks, which only [`wrap`] treats as such.
//!
//! A line's [`width`](Line::width) leaves out trailing spaces and tabs,
//! which [`pen`](Line::pen) counts.
//!
//! # Wrapping
//!
//! [`wrap`] breaks lines at LF, CR LF and a lone CR, as `tidepool-text`
//! does (ADR-0019), then each paragraph within the width: it may break
//! after a run of spaces and tabs, which hang past the width at the end
//! of the line; a word wider than the line is broken before the first
//! character that does not fit. A line always holds at least one
//! character, so a width too narrow for any gives one a line. A width that
//! is NaN or infinite never breaks. Breaking opportunities are only those:
//! not after hyphens, nor between ideographs (UAX #14 is not followed),
//! and a no-break space never breaks.
//!
//! # Cost
//!
//! A layout holds one [`Glyph`] (32 bytes) per character, and [`wrap`] a
//! list of the paragraph's characters besides, so its memory grows with
//! the text; lay out what is shown, not a whole file.

use std::ops::Range;

use crate::FontError;
use crate::fallback::{self, FaceId, Faces, NOTDEF_ADVANCE, Resolved};
use crate::ttf::{Font, GlyphId};

/// The most letter-spacing a [`Style`] has, either way, in ems: 64. More
/// is clamped to it, so that positions stay finite.
pub const MAX_LETTER_SPACING: f32 = 64.0;

/// A text size: pixels per em, in steps of 1/64 of a pixel, from 1/64 to
/// 2,048 pixels.
///
/// Layout and the [`atlas`](crate::atlas) both use the size this gives, so
/// that a glyph's advance and its image agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PxSize(u32);

impl PxSize {
    /// The steps in a pixel.
    pub const UNITS_PER_PX: u32 = 64;

    /// The largest size: 2,048 pixels per em.
    pub const MAX: Self = Self(2048 * Self::UNITS_PER_PX);

    /// `px` pixels per em, to the nearest 1/64; `None` when that is NaN,
    /// not above 0, or above [`PxSize::MAX`].
    pub fn new(px: f32) -> Option<Self> {
        let units = (f64::from(px) * f64::from(Self::UNITS_PER_PX) + 0.5).floor();
        if !(units >= 1.0 && units <= f64::from(Self::MAX.0)) {
            return None;
        }
        // In range, and whole: the conversion is exact.
        Some(Self(units as u32))
    }

    /// `units` 64ths of a pixel; `None` for 0 or above [`PxSize::MAX`].
    pub const fn from_units(units: u32) -> Option<Self> {
        if units == 0 || units > Self::MAX.0 {
            None
        } else {
            Some(Self(units))
        }
    }

    /// The size in 64ths of a pixel.
    pub const fn units(self) -> u32 {
        self.0
    }

    /// The size in pixels, exactly.
    pub fn px(self) -> f32 {
        // At most 2^17: exact in an f32, as is the division by 64.
        self.0 as f32 / Self::UNITS_PER_PX as f32
    }
}

/// How text is set: the face asked for, the size, the letter-spacing and
/// the tab stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// The face asked for; characters it lacks fall back
    /// ([`Faces::resolve`](crate::fallback::Faces::resolve)).
    pub face: FaceId,
    /// Pixels per em, in device pixels.
    pub size: PxSize,
    /// Space added between each glyph and the next, in ems (0.28 for
    /// CSS's `0.28em`); 0 by default. One that is NaN or infinite counts
    /// as 0, and one beyond ±[`MAX_LETTER_SPACING`] as that.
    pub letter_spacing: f32,
    /// Tab stops, in advances of a space; 4 by default, and 0 counts as 1.
    pub tab_size: u32,
}

impl Style {
    /// `face` at `size`, with no letter-spacing and tab stops every 4
    /// spaces.
    pub const fn new(face: FaceId, size: PxSize) -> Self {
        Self {
            face,
            size,
            letter_spacing: 0.0,
            tab_size: 4,
        }
    }
}

/// A glyph placed on a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// The face that draws it.
    pub face: FaceId,
    /// The glyph, or `None` for a tab, which draws nothing. Glyph 0 is
    /// `.notdef`: no face maps the character.
    pub glyph: Option<GlyphId>,
    /// The pen's x when it is drawn, in pixels from the line's start.
    pub x: f32,
    /// Its own advance, in pixels: for a tab, to the stop. Kerning and
    /// letter-spacing are in the next glyph's `x`, not here.
    pub advance: f32,
    /// The byte offset of its character in the text laid out; for the
    /// ellipsis of a [`truncate`]d line, where the text was cut.
    pub cluster: usize,
}

/// A line of laid-out text.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its glyphs, one a character, in the text's order, the ellipsis last
    /// in a [`truncate`]d line.
    pub glyphs: Vec<Glyph>,
    /// From the line's start to the right of the last glyph that is not a
    /// space or a tab, in pixels: what it shows. 0 for a line of none.
    pub width: f32,
    /// From the line's start to the pen after the last glyph, trailing
    /// spaces and tabs included, in pixels.
    pub pen: f32,
    /// The bytes of the text it lays out: for [`wrap`], without the line
    /// break, and with the spaces that hang at its end; for [`truncate`],
    /// up to the cut.
    pub range: Range<usize>,
    /// Whether [`truncate`] cut it and ended it with an ellipsis.
    pub truncated: bool,
}

/// A style's vertical metrics in pixels, from its face's `hhea`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VMetrics {
    /// From the baseline up to the top of the line.
    pub ascent: f32,
    /// From the baseline down to the bottom of the line, positive.
    pub descent: f32,
    /// The gap the face asks for between lines.
    pub line_gap: f32,
}

impl VMetrics {
    /// The face's own line height: ascent, descent and line gap.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }

    /// Where the baseline is from the top of a line `line_height` pixels
    /// tall, with the space left over (or missing) shared above and below,
    /// as CSS's half-leading does.
    pub fn baseline(&self, line_height: f32) -> f32 {
        (line_height - (self.ascent + self.descent)) / 2.0 + self.ascent
    }
}

/// The vertical metrics of `style`'s face at its size.
///
/// # Errors
///
/// [`FontError::Faces`] when `faces` does not hold the style's face.
pub fn metrics(faces: &Faces, style: &Style) -> Result<VMetrics, FontError> {
    let font = face(faces, style.face)?;
    let line = font.line_metrics();
    let px = style.size.px();
    Ok(VMetrics {
        ascent: scaled(font, i32::from(line.ascender), px),
        descent: -scaled(font, i32::from(line.descender), px),
        line_gap: scaled(font, i32::from(line.line_gap), px),
    })
}

/// Lays `text` out as one line, however long (the module docs, "What a
/// line does").
///
/// # Errors
///
/// [`FontError::Faces`] when `faces` does not hold the style's face.
pub fn line(faces: &Faces, text: &str, style: &Style) -> Result<Line, FontError> {
    let mut pen = Pen::new(faces, style)?;
    for (at, c) in text.char_indices() {
        pen.push(c, at);
    }
    Ok(pen.finish(text, 0..text.len()))
}

/// Lays `text` out as one line, cut to fit `max_width` pixels if it is
/// wider: as many characters as fit with an ellipsis (`…`, resolved like
/// any character) after them, without the spaces and tabs before the cut.
/// When not even the ellipsis fits, the line is the ellipsis alone, wider
/// than `max_width`. A `max_width` that is NaN never cuts, nor is an
/// empty text ever cut.
///
/// # Errors
///
/// [`FontError::Faces`] when `faces` does not hold the style's face.
pub fn truncate(
    faces: &Faces,
    text: &str,
    style: &Style,
    max_width: f32,
) -> Result<Line, FontError> {
    let full = line(faces, text, style)?;
    if full.glyphs.is_empty() || max_width.is_nan() || full.width <= max_width {
        return Ok(full);
    }
    let pen = Pen::new(faces, style)?;
    let ellipsis = faces.resolve_in(style.face, pen.font, '…');
    let ellipsis_advance = advance(faces, ellipsis, pen.px);
    // The longest prefix, ending on a glyph that is not blank, after which
    // the ellipsis fits; or none.
    let mut keep = 0;
    let mut at = 0.0;
    for end in (1..full.glyphs.len()).rev() {
        let Some(last) = full.glyphs.get(end - 1) else {
            continue;
        };
        if is_blank(text, last.cluster) {
            continue;
        }
        let x = last.x + last.advance + pen.spacing + pen.kerning(last.face, last.glyph, ellipsis);
        if x + ellipsis_advance <= max_width {
            keep = end;
            at = x;
            break;
        }
    }
    let cut = full
        .glyphs
        .get(keep)
        .map_or(text.len(), |glyph| glyph.cluster);
    let mut glyphs = full.glyphs;
    glyphs.truncate(keep);
    glyphs.push(Glyph {
        face: ellipsis.face,
        glyph: Some(ellipsis.glyph),
        x: at,
        advance: ellipsis_advance,
        cluster: cut,
    });
    Ok(Line {
        glyphs,
        width: at + ellipsis_advance,
        pen: at + ellipsis_advance,
        range: 0..cut,
        truncated: true,
    })
}

/// Lays `text` out in lines no wider than `max_width` pixels where it can
/// (the module docs, "Wrapping"). Gives at least one line, and one more
/// for each line break: `"a\n"` is two lines, the second empty.
///
/// # Errors
///
/// [`FontError::Faces`] when `faces` does not hold the style's face.
pub fn wrap(
    faces: &Faces,
    text: &str,
    style: &Style,
    max_width: f32,
) -> Result<Vec<Line>, FontError> {
    let mut lines = Vec::new();
    for paragraph in paragraphs(text) {
        wrap_paragraph(&mut lines, faces, text, paragraph, style, max_width)?;
    }
    Ok(lines)
}

/// Wraps the paragraph `range` of `text` onto `lines`.
fn wrap_paragraph(
    lines: &mut Vec<Line>,
    faces: &Faces,
    text: &str,
    range: Range<usize>,
    style: &Style,
    max_width: f32,
) -> Result<(), FontError> {
    let Some(paragraph) = text.get(range.clone()) else {
        return Ok(());
    };
    let chars: Vec<(usize, char)> = paragraph
        .char_indices()
        .map(|(offset, c)| (range.start + offset, c))
        .collect();
    let mut pen = Pen::new(faces, style)?;
    // The index in `chars` of the line's first character, and its byte.
    let mut first = 0;
    let mut start = range.start;
    // The index of the last character that follows a run of blanks on this
    // line: where it may break.
    let mut opportunity = None;
    let mut index = 0;
    while let Some(&(at, c)) = chars.get(index) {
        let blank = c == ' ' || c == '\t';
        if !blank
            && index > first
            && chars
                .get(index - 1)
                .is_some_and(|&(_, before)| before == ' ' || before == '\t')
        {
            opportunity = Some(index);
        }
        pen.push(c, at);
        if !blank && index > first && pen.x > max_width {
            // Break at the last opportunity, else before this character,
            // and lay the rest out again on a new line.
            let cut = opportunity.unwrap_or(index);
            let cut_at = chars.get(cut).map_or(range.end, |&(at, _)| at);
            pen.glyphs.truncate(cut - first);
            let done = std::mem::replace(&mut pen, Pen::new(faces, style)?);
            lines.push(done.finish(text, start..cut_at));
            first = cut;
            start = cut_at;
            opportunity = None;
            index = cut;
            continue;
        }
        index += 1;
    }
    lines.push(pen.finish(text, start..range.end));
    Ok(())
}

/// The ranges of `text` between line breaks: LF, CR LF and a lone CR.
fn paragraphs(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\n' => {
                ranges.push(start..at);
                at += 1;
                start = at;
            }
            b'\r' => {
                ranges.push(start..at);
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            }
            _ => at += 1,
        }
    }
    ranges.push(start..text.len());
    ranges
}

/// Whether the character at byte `at` of `text` is a space or a tab.
fn is_blank(text: &str, at: usize) -> bool {
    text.get(at..)
        .is_some_and(|rest| rest.starts_with([' ', '\t']))
}

/// The face `face` of `faces`, or the error for one it does not hold.
fn face(faces: &Faces, face: FaceId) -> Result<&Font, FontError> {
    faces.font(face).ok_or(FontError::Faces {
        kind: fallback::ErrorKind::UnknownFace { face },
    })
}

/// `units` of `font` at `px` pixels per em, in pixels.
fn scaled(font: &Font, units: i32, px: f32) -> f32 {
    // Font units are 16-bit, and kerning sums of them: exact in an f32.
    units as f32 * px / f32::from(font.units_per_em())
}

/// The advance of `glyph` at `px` pixels per em, in pixels.
fn advance(faces: &Faces, glyph: Resolved, px: f32) -> f32 {
    let Some(font) = faces.font(glyph.face) else {
        return 0.0;
    };
    if glyph.is_notdef() && faces.draws_own_notdef(glyph.face) {
        return NOTDEF_ADVANCE * px;
    }
    let units = font
        .h_metrics(glyph.glyph)
        .map_or(0, |metrics| metrics.advance);
    scaled(font, i32::from(units), px)
}

/// A line being laid out.
struct Pen<'f> {
    faces: &'f Faces,
    primary: FaceId,
    font: &'f Font,
    px: f32,
    /// Letter-spacing, in pixels.
    spacing: f32,
    /// The distance between tab stops, in pixels.
    tab: f32,
    /// The pen's x.
    x: f32,
    /// The glyph before the pen, which kerning and letter-spacing follow:
    /// `None` at the start of the line and after a tab.
    before: Option<Resolved>,
    glyphs: Vec<Glyph>,
}

impl<'f> Pen<'f> {
    fn new(faces: &'f Faces, style: &Style) -> Result<Self, FontError> {
        let font = face(faces, style.face)?;
        let px = style.size.px();
        let spacing = if style.letter_spacing.is_finite() {
            style
                .letter_spacing
                .clamp(-MAX_LETTER_SPACING, MAX_LETTER_SPACING)
                * px
        } else {
            0.0
        };
        let space = advance(faces, faces.resolve_in(style.face, font, ' '), px);
        Ok(Self {
            faces,
            primary: style.face,
            font,
            px,
            spacing,
            // At most 2^32 spaces: f32 rounds it, which no tab size needs.
            tab: space * style.tab_size.max(1) as f32,
            x: 0.0,
            before: None,
            glyphs: Vec::new(),
        })
    }

    /// The kerning between `face`'s `glyph` and `next`, in pixels: 0
    /// across faces, for a tab, and for `.notdef`.
    fn kerning(&self, face: FaceId, glyph: Option<GlyphId>, next: Resolved) -> f32 {
        let Some(glyph) = glyph else {
            return 0.0;
        };
        if face != next.face || glyph == GlyphId(0) || next.is_notdef() {
            return 0.0;
        }
        self.faces.font(face).map_or(0.0, |font| {
            scaled(font, font.kerning(glyph, next.glyph), self.px)
        })
    }

    /// Places `c`, the character at byte `at`.
    fn push(&mut self, c: char, at: usize) {
        if c == '\t' {
            let stop = if self.tab > 0.0 {
                ((self.x / self.tab + 1.0 / 256.0).floor() + 1.0) * self.tab
            } else {
                self.x
            };
            self.glyphs.push(Glyph {
                face: self.primary,
                glyph: None,
                x: self.x,
                advance: stop - self.x,
                cluster: at,
            });
            self.x = stop;
            self.before = None;
            return;
        }
        let resolved = self.faces.resolve_in(self.primary, self.font, c);
        if let Some(before) = self.before {
            self.x += self.spacing + self.kerning(before.face, Some(before.glyph), resolved);
        }
        let advance = advance(self.faces, resolved, self.px);
        self.glyphs.push(Glyph {
            face: resolved.face,
            glyph: Some(resolved.glyph),
            x: self.x,
            advance,
            cluster: at,
        });
        self.x += advance;
        self.before = Some(resolved);
    }

    /// The line, laying out `range` of `text`.
    fn finish(self, text: &str, range: Range<usize>) -> Line {
        let width = self
            .glyphs
            .iter()
            .rev()
            .find(|glyph| !is_blank(text, glyph.cluster))
            .map_or(0.0, |glyph| glyph.x + glyph.advance);
        let pen = self
            .glyphs
            .last()
            .map_or(0.0, |glyph| glyph.x + glyph.advance);
        Line {
            glyphs: self.glyphs,
            width,
            pen,
            range,
            truncated: false,
        }
    }
}
