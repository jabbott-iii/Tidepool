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

//! `loca` and `glyf`: where each glyph's data is, and its outline, simple
//! or composite ([`Font::outline`](super::Font::outline) says what is
//! read).
//!
//! Composites are resolved without recursion: the composites being read
//! are a stack on the heap, at most [`Limits::max_component_depth`] deep.

use super::read::{Place, Span, be16, be32};
use super::{ErrorKind, GlyphId};
use crate::outline::{Outline, OutlinePoint};
use crate::{FontError, Limits};

/// Where the glyphs are.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyphs {
    /// The absolute offset of `loca`.
    loca: usize,
    /// `loca` holds 32-bit offsets, not 16-bit halves.
    long: bool,
    /// The absolute offset of `glyf`.
    glyf: usize,
    glyph_count: u16,
}

// A simple glyph's point flags.
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_OR_POSITIVE: u8 = 0x10;
const Y_SAME_OR_POSITIVE: u8 = 0x20;

// A component's flags.
const ARGS_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
const SCALED_COMPONENT_OFFSET: u16 = 0x0800;
const UNSCALED_COMPONENT_OFFSET: u16 = 0x1000;

/// The size of a glyph's header: the contour count and its box.
const HEADER: usize = 10;

impl Glyphs {
    /// Checks that `loca` holds an offset for each glyph and one more, that
    /// none is less than the one before, and none past the end of `glyf`.
    pub(crate) fn read(
        loca: Span<'_>,
        glyf: Span<'_>,
        long: bool,
        glyph_count: u16,
    ) -> Result<Self, FontError> {
        let entries = usize::from(glyph_count) + 1;
        let size = if long { 4 } else { 2 };
        loca.bytes(0, entries * size, "an offset for each glyph and one more")?;
        let mut previous = 0;
        for index in 0..entries {
            let at = index * size;
            let offset = if long {
                usize::try_from(loca.u32(at, "a glyph's offset")?).unwrap_or(usize::MAX)
            } else {
                usize::from(loca.u16(at, "a glyph's offset")?) * 2
            };
            if offset < previous {
                return Err(loca.error(at, ErrorKind::LocaOutOfOrder));
            }
            if offset > glyf.len() {
                return Err(loca.error(at, ErrorKind::LocaPastGlyf));
            }
            previous = offset;
        }
        Ok(Self {
            loca: loca.start(),
            long,
            glyf: glyf.start(),
            glyph_count,
        })
    }

    /// The data of `glyph`, which is below the glyph count, naming it.
    fn span<'a>(&self, data: &'a [u8], glyph: GlyphId) -> Span<'a> {
        let index = usize::from(glyph.0);
        let offset = |index: usize| -> usize {
            if self.long {
                be32(data, self.loca + index * 4)
                    .and_then(|offset| usize::try_from(offset).ok())
                    .unwrap_or(0)
            } else {
                be16(data, self.loca + index * 2).map_or(0, |half| usize::from(half) * 2)
            }
        };
        let (start, end) = (offset(index), offset(index + 1));
        Span::new(
            data,
            self.glyf.saturating_add(start),
            self.glyf.saturating_add(end),
            Place::Glyph(glyph),
        )
    }

    /// The outline of `glyph`, which is below the glyph count.
    pub(crate) fn outline(
        &self,
        data: &[u8],
        glyph: GlyphId,
        limits: Limits,
    ) -> Result<Outline, FontError> {
        let mut builder = Builder {
            glyphs: *self,
            data,
            limits,
            points: Vec::new(),
            ends: Vec::new(),
            components: 0,
        };
        builder.glyph(glyph)?;
        // The vectors grow by doubling as components are added: shrink them
        // so that the outline holds 12 bytes a point and 8 a contour.
        builder.points.shrink_to_fit();
        builder.ends.shrink_to_fit();
        Ok(Outline::from_parts(builder.points, builder.ends))
    }
}

/// A component's transform: `x' = a·x + c·y`, `y' = b·x + d·y`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl Matrix {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }
}

/// How a component is moved once transformed.
#[derive(Clone, Copy, Debug)]
enum Placement {
    /// By an offset in font units, scaled by the matrix when `scaled`.
    Offset { dx: f64, dy: f64, scaled: bool },
    /// So that the component's point `child`, transformed, lands on the
    /// composite's point `parent` (counted from the composite's first).
    Points {
        parent: u16,
        child: u16,
        /// The absolute offsets of the two arguments.
        parent_at: usize,
        child_at: usize,
    },
}

/// A component whose points are being added, from index `start`.
#[derive(Clone, Copy, Debug)]
struct Component {
    start: usize,
    matrix: Matrix,
    placement: Placement,
    /// Its record, relative to the composite's data.
    record: usize,
}

/// A composite being read.
#[derive(Clone, Copy, Debug)]
struct Frame<'a> {
    /// Its data, which names it.
    data: Span<'a>,
    /// The next component's record, relative to the data's start.
    cursor: usize,
    /// Whether another component follows.
    more: bool,
    /// The index of the composite's first point.
    base: usize,
    /// The component a composite above it on the stack is adding.
    pending: Option<Component>,
}

/// Builds one glyph's outline.
struct Builder<'a> {
    glyphs: Glyphs,
    data: &'a [u8],
    limits: Limits,
    points: Vec<OutlinePoint>,
    /// Where each contour ends: one past its last point.
    ends: Vec<usize>,
    /// Components read so far, over every level.
    components: usize,
}

impl<'a> Builder<'a> {
    /// Adds `glyph`'s points and contours.
    fn glyph(&mut self, glyph: GlyphId) -> Result<(), FontError> {
        let span = self.glyphs.span(self.data, glyph);
        let Some(contours) = header(span)? else {
            return Ok(());
        };
        if contours >= 0 {
            return self.simple(span, contours.unsigned_abs());
        }
        let limit = self.limits.max_component_depth;
        if limit == 0 {
            return Err(span.error(0, ErrorKind::TooDeep { limit }));
        }
        let mut stack = vec![Frame {
            data: span,
            cursor: HEADER,
            more: true,
            base: 0,
            pending: None,
        }];
        while let Some(top) = stack.last_mut() {
            if let Some(component) = top.pending.take() {
                place(&mut self.points, top.base, component, top.data)?;
            }
            if !top.more {
                stack.pop();
                continue;
            }
            let frame = *top;
            let (child, component, record) = self.component(&mut stack, frame)?;
            let span = self.glyphs.span(self.data, child);
            match header(span)? {
                None => place(&mut self.points, frame.base, component, frame.data)?,
                Some(contours) if contours >= 0 => {
                    self.simple(span, contours.unsigned_abs())?;
                    place(&mut self.points, frame.base, component, frame.data)?;
                }
                Some(_) => {
                    if stack.len() >= limit {
                        return Err(frame.data.error(record, ErrorKind::TooDeep { limit }));
                    }
                    if let Some(top) = stack.last_mut() {
                        top.pending = Some(component);
                    }
                    stack.push(Frame {
                        data: span,
                        cursor: HEADER,
                        more: true,
                        base: self.points.len(),
                        pending: None,
                    });
                }
            }
        }
        Ok(())
    }

    /// Reads the next component of `frame`, the top of `stack`, and moves
    /// the frame's cursor past it: the component's glyph, how it is placed,
    /// and its record's offset relative to the frame's data.
    fn component(
        &mut self,
        stack: &mut [Frame<'a>],
        frame: Frame<'a>,
    ) -> Result<(GlyphId, Component, usize), FontError> {
        let span = frame.data;
        let record = frame.cursor;
        let flags = span.u16(record, "a component's flags")?;
        let limit = self.limits.max_components;
        if self.components >= limit {
            return Err(span.error(record, ErrorKind::TooManyComponents { limit }));
        }
        self.components += 1;
        let child = span.u16(record + 2, "a component's glyph")?;
        if child >= self.glyphs.glyph_count {
            return Err(span.error(
                record + 2,
                ErrorKind::GlyphOutOfRange {
                    found: child,
                    count: self.glyphs.glyph_count,
                },
            ));
        }
        let mut at = record + 4;
        let words = flags & ARGS_ARE_WORDS != 0;
        let xy = flags & ARGS_ARE_XY_VALUES != 0;
        let (first, second, size) = if words {
            let first = span.u16(at, "a component's first argument")?;
            let second = span.u16(at + 2, "a component's second argument")?;
            (first, second, 4)
        } else {
            let first = span.u8(at, "a component's first argument")?;
            let second = span.u8(at + 1, "a component's second argument")?;
            // Offsets are signed bytes; point numbers unsigned.
            if xy {
                (first as i8 as u16, second as i8 as u16, 2)
            } else {
                (u16::from(first), u16::from(second), 2)
            }
        };
        let placement = if xy {
            Placement::Offset {
                dx: f64::from(first as i16),
                dy: f64::from(second as i16),
                scaled: flags & SCALED_COMPONENT_OFFSET != 0
                    && flags & UNSCALED_COMPONENT_OFFSET == 0,
            }
        } else {
            Placement::Points {
                parent: first,
                child: second,
                parent_at: span.absolute(at),
                child_at: span.absolute(at + size / 2),
            }
        };
        at += size;
        let f2dot14 = |at: usize| -> Result<f64, FontError> {
            Ok(f64::from(span.i16(at, "a component's scale")?) / 16_384.0)
        };
        let matrix = if flags & WE_HAVE_A_SCALE != 0 {
            let scale = f2dot14(at)?;
            at += 2;
            Matrix {
                a: scale,
                b: 0.0,
                c: 0.0,
                d: scale,
            }
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            let (x, y) = (f2dot14(at)?, f2dot14(at + 2)?);
            at += 4;
            Matrix {
                a: x,
                b: 0.0,
                c: 0.0,
                d: y,
            }
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            let matrix = Matrix {
                a: f2dot14(at)?,
                b: f2dot14(at + 2)?,
                c: f2dot14(at + 4)?,
                d: f2dot14(at + 6)?,
            };
            at += 8;
            matrix
        } else {
            Matrix::IDENTITY
        };
        if let Some(top) = stack.last_mut() {
            top.cursor = at;
            top.more = flags & MORE_COMPONENTS != 0;
        }
        let component = Component {
            start: self.points.len(),
            matrix,
            placement,
            record,
        };
        Ok((GlyphId(child), component, record))
    }

    /// Adds a simple glyph's points and contours; `span` is its data, and
    /// it has `contours` contours.
    fn simple(&mut self, span: Span<'_>, contours: u16) -> Result<(), FontError> {
        let contours = usize::from(contours);
        if contours == 0 {
            return Ok(());
        }
        let mut last: Option<u16> = None;
        for index in 0..contours {
            let at = HEADER + index * 2;
            let end = span.u16(at, "a contour's end point")?;
            if last.is_some_and(|last| end <= last) {
                return Err(span.error(at, ErrorKind::ContoursOutOfOrder));
            }
            last = Some(end);
        }
        let count = last.map_or(0, |last| usize::from(last) + 1);
        let last_at = HEADER + (contours - 1) * 2;
        let limit = self.limits.max_glyph_points;
        let base = self.points.len();
        if count > limit.saturating_sub(base) {
            return Err(span.error(last_at, ErrorKind::TooManyPoints { limit }));
        }
        let instructions_at = HEADER + contours * 2;
        let instructions = usize::from(span.u16(instructions_at, "the instructions' length")?);
        let mut at = instructions_at + 2;
        span.bytes(at, instructions, "the instructions")?;
        at += instructions;

        let mut flags = Vec::with_capacity(count);
        while flags.len() < count {
            let flag = span.u8(at, "a point's flags")?;
            at += 1;
            flags.push(flag);
            if flag & REPEAT != 0 {
                let repeat = usize::from(span.u8(at, "a flag's repeat count")?);
                if repeat > count - flags.len() {
                    return Err(span.error(at, ErrorKind::FlagsPastPoints));
                }
                at += 1;
                flags.extend(std::iter::repeat_n(flag, repeat));
            }
        }

        self.points.reserve(count);
        let mut x: i64 = 0;
        for &flag in &flags {
            x += coordinate(
                span,
                &mut at,
                flag,
                X_SHORT,
                X_SAME_OR_POSITIVE,
                "an x coordinate",
            )?;
            self.points.push(OutlinePoint {
                x: x as f32,
                y: 0.0,
                on_curve: flag & ON_CURVE != 0,
            });
        }
        let mut y: i64 = 0;
        for (point, &flag) in self.points.iter_mut().skip(base).zip(&flags) {
            y += coordinate(
                span,
                &mut at,
                flag,
                Y_SHORT,
                Y_SAME_OR_POSITIVE,
                "a y coordinate",
            )?;
            point.y = y as f32;
        }

        self.ends.reserve(contours);
        for index in 0..contours {
            let end = span.u16(HEADER + index * 2, "a contour's end point")?;
            self.ends.push(base + usize::from(end) + 1);
        }
        Ok(())
    }
}

/// A glyph's contour count, or `None` for an empty glyph (no data).
fn header(span: Span<'_>) -> Result<Option<i16>, FontError> {
    if span.len() == 0 {
        return Ok(None);
    }
    span.bytes(0, HEADER, "the glyph's header")?;
    Ok(Some(span.i16(0, "the glyph's contour count")?))
}

/// Reads one coordinate's delta at `at`, as `flag` says, and moves `at`
/// past it.
fn coordinate(
    span: Span<'_>,
    at: &mut usize,
    flag: u8,
    short: u8,
    same_or_positive: u8,
    expected: &'static str,
) -> Result<i64, FontError> {
    if flag & short != 0 {
        let delta = i64::from(span.u8(*at, expected)?);
        *at += 1;
        Ok(if flag & same_or_positive != 0 {
            delta
        } else {
            -delta
        })
    } else if flag & same_or_positive != 0 {
        Ok(0)
    } else {
        let delta = i64::from(span.i16(*at, expected)?);
        *at += 2;
        Ok(delta)
    }
}

/// Transforms and moves the points `component` added (from its start to
/// the end), in the composite whose first point is `base` and whose data
/// is `composite`; an error if a point leaves the range of `f32`.
///
/// A coordinate can grow by up to 4 times a level (a matrix of −2s), and
/// point matching can add the composite's own points to it, so up to 9
/// times: with a high enough [`Limits::max_component_depth`], a composite
/// of simple glyphs, whose coordinates reach 2^31, could overflow, which
/// would leave a point infinite (or, a level further, NaN). Under the
/// default depth of 8 they stay below 9^8 × 2^31, about 2^57, far inside
/// `f32`.
fn place(
    points: &mut [OutlinePoint],
    base: usize,
    component: Component,
    composite: Span<'_>,
) -> Result<(), FontError> {
    let Component {
        start,
        matrix,
        placement,
        record,
    } = component;
    let added = points.get_mut(start..).unwrap_or_default();
    if matrix != Matrix::IDENTITY {
        for point in added.iter_mut() {
            let (x, y) = matrix.apply(f64::from(point.x), f64::from(point.y));
            point.x = x as f32;
            point.y = y as f32;
        }
    }
    let (dx, dy) = match placement {
        Placement::Offset { dx, dy, scaled } => {
            if scaled {
                matrix.apply(dx, dy)
            } else {
                (dx, dy)
            }
        }
        Placement::Points {
            parent,
            child,
            parent_at,
            child_at,
        } => {
            let before = start.saturating_sub(base);
            let target = points
                .get(base + usize::from(parent))
                .filter(|_| usize::from(parent) < before)
                .ok_or_else(|| point_error(composite, parent_at, parent, before))?;
            let (target_x, target_y) = (target.x, target.y);
            let count = points.len().saturating_sub(start);
            let moved = points
                .get(start + usize::from(child))
                .ok_or_else(|| point_error(composite, child_at, child, count))?;
            (
                f64::from(target_x) - f64::from(moved.x),
                f64::from(target_y) - f64::from(moved.y),
            )
        }
    };
    if dx != 0.0 || dy != 0.0 {
        for point in points.get_mut(start..).unwrap_or_default() {
            point.x = (f64::from(point.x) + dx) as f32;
            point.y = (f64::from(point.y) + dy) as f32;
        }
    }
    // An infinite coordinate stays infinite, or becomes NaN, through the
    // offset: so checking once, after it, catches every overflow.
    let finite = points
        .get(start..)
        .unwrap_or_default()
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite());
    if !finite {
        return Err(composite.error(record, ErrorKind::CoordinateOutOfRange));
    }
    Ok(())
}

fn point_error(composite: Span<'_>, at: usize, index: u16, count: usize) -> FontError {
    composite.error(
        at.saturating_sub(composite.start()),
        ErrorKind::PointOutOfRange { index, count },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matrix_maps_x_and_y_as_the_specification_says() {
        // (xscale, scale01, scale10, yscale): x' = a·x + c·y, y' = b·x + d·y.
        let matrix = Matrix {
            a: 1.0,
            b: 0.5,
            c: 0.25,
            d: 2.0,
        };
        assert_eq!(matrix.apply(4.0, 8.0), (6.0, 18.0));
        assert_eq!(Matrix::IDENTITY.apply(-3.0, 7.0), (-3.0, 7.0));
    }

    #[test]
    fn an_outline_holds_exactly_its_points_and_contours() {
        // Glyph 0: three one-point contours, every point at (0, 0). Glyph 1:
        // a composite of glyph 0 three times, so that the vectors grow past
        // their length (3, then 6, then 9) before they are shrunk.
        let mut simple = vec![0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0];
        simple.extend([ON_CURVE | X_SAME_OR_POSITIVE | Y_SAME_OR_POSITIVE; 3]);
        simple.push(0);
        let mut composite = vec![0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0];
        for more in [true, true, false] {
            let flags = ARGS_ARE_XY_VALUES | if more { MORE_COMPONENTS } else { 0 };
            composite.extend(flags.to_be_bytes());
            composite.extend([0, 0, 0, 0]);
        }
        let mut data = Vec::new();
        for offset in [0, simple.len(), simple.len() + composite.len()] {
            data.extend(u32::try_from(offset).expect("small").to_be_bytes());
        }
        let glyf = data.len();
        data.extend(&simple);
        data.extend(&composite);
        let glyphs = Glyphs {
            loca: 0,
            long: true,
            glyf,
            glyph_count: 2,
        };
        let outline = glyphs
            .outline(&data, GlyphId(1), Limits::DEFAULT)
            .expect("reads");
        assert_eq!((outline.point_count(), outline.contour_count()), (9, 9));
        assert_eq!(outline.capacities(), (9, 9));
    }

    #[test]
    fn coordinates_read_their_flags() {
        let data = [5, 0xff, 0xfe];
        let span = Span::file(&data);
        let mut at = 0;
        assert_eq!(
            coordinate(span, &mut at, X_SHORT, X_SHORT, X_SAME_OR_POSITIVE, "x"),
            Ok(-5)
        );
        assert_eq!(at, 1);
        assert_eq!(
            coordinate(
                span,
                &mut at,
                X_SAME_OR_POSITIVE,
                X_SHORT,
                X_SAME_OR_POSITIVE,
                "x"
            ),
            Ok(0)
        );
        assert_eq!(at, 1);
        assert_eq!(
            coordinate(span, &mut at, 0, X_SHORT, X_SAME_OR_POSITIVE, "x"),
            Ok(-2)
        );
        assert_eq!(at, 3);
        let mut at = 0;
        assert_eq!(
            coordinate(
                span,
                &mut at,
                X_SHORT | X_SAME_OR_POSITIVE,
                X_SHORT,
                X_SAME_OR_POSITIVE,
                "x"
            ),
            Ok(5)
        );
    }
}
