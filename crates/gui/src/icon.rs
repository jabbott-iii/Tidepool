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

//! [`Icon`]: a vector icon, the outline the [painter](crate::paint) fills
//! for a `ui` icon command, and [`IconError`] (M3.5, ADR-0048).
//!
//! New in Tidepool.
//!
//! An icon is drawn in a square viewbox `side` units across, x growing
//! right and y down from its top-left corner, as SVG's `viewBox="0 0 side
//! side"` is. Its contours are closed paths of `font`'s [`OutlinePoint`]s,
//! as a TrueType glyph's are: between two points on the curve is a line, a
//! point off the curve is the control point of a quadratic Bézier curve,
//! and between two points off the curve is an implied point on it, half-way
//! (`font::outline`'s "Outlines"). Every point lies inside the viewbox.
//!
//! It is filled by [`font::outline::rasterize`]'s exact-area coverage, so
//! the coverage is the non-zero rule for contours that wind the same way
//! and overlap neither themselves nor one another, with holes winding the
//! other way, and differs from it only along the edges of overlaps, as
//! there.
//!
//! No roadmap task builds Tidepool's icon set, `assets/icons`, yet: a
//! caller makes its icons and registers them with
//! [`Painter::set_icon`](crate::paint::Painter::set_icon).
//!
//! [`font::outline::rasterize`]: tidepool_font::outline::rasterize

use std::fmt;

use tidepool_font::Limits;
use tidepool_font::outline::{self, Bitmap, Outline, OutlinePoint};

use crate::GuiError;

/// The most points an icon has: `font`'s default limit for a glyph's,
/// 65,536.
pub const MAX_POINTS: usize = Limits::DEFAULT.max_glyph_points;

/// The smallest side a viewbox has: 1/65,536 of a unit, small enough for
/// any icon, and large enough that drawing one at `font`'s largest
/// bitmap, 4,096 px, needs a scale far inside `f32`'s range.
pub const MIN_SIDE: f32 = 1.0 / 65_536.0;

/// A vector icon in a square viewbox (the [module documentation](self)).
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    side: f32,
    /// The contours with y negated, so that `font`'s rasterizer, whose y
    /// grows up, puts the viewbox's top-left corner at the origin and its
    /// rows downwards.
    outline: Outline,
}

impl Icon {
    /// An icon of `points` in a viewbox `side` units across, y growing
    /// down, in contours that end where `contour_ends` says, as
    /// [`Outline::new`] takes them: contour `i` is
    /// `points[contour_ends[i - 1]..contour_ends[i]]`, the first starting
    /// at 0. No points and no ends is an icon that draws nothing.
    ///
    /// ```
    /// use tidepool_font::outline::OutlinePoint;
    /// use tidepool_gui::Icon;
    ///
    /// let on = |x, y| OutlinePoint { x, y, on_curve: true };
    /// // A ring: a square, and a square hole winding the other way.
    /// let points = vec![
    ///     on(2.0, 2.0), on(22.0, 2.0), on(22.0, 22.0), on(2.0, 22.0),
    ///     on(8.0, 8.0), on(8.0, 16.0), on(16.0, 16.0), on(16.0, 8.0),
    /// ];
    /// let ring = Icon::new(24.0, points, vec![4, 8]).expect("an icon");
    /// assert_eq!((ring.side(), ring.point_count()), (24.0, 8));
    /// ```
    ///
    /// # Errors
    ///
    /// [`GuiError::Icon`], in this order: with [`IconError::Side`] unless
    /// `side` is finite and at least [`MIN_SIDE`]; with [`IconError::TooManyPoints`] for
    /// more than [`MAX_POINTS`]; with [`IconError::Point`] for a point that
    /// is not finite or lies outside the viewbox, from 0 to `side` on each
    /// axis; with [`IconError::ContourEnds`] for contour ends that do not
    /// go up, each by at least 1, from above 0 to the number of points, as
    /// [`Outline::new`] takes them.
    pub fn new(
        side: f32,
        points: Vec<OutlinePoint>,
        contour_ends: Vec<usize>,
    ) -> Result<Self, GuiError> {
        if !(side.is_finite() && side >= MIN_SIDE) {
            return Err(GuiError::Icon(IconError::Side { side }));
        }
        if points.len() > MAX_POINTS {
            return Err(GuiError::Icon(IconError::TooManyPoints {
                count: points.len(),
            }));
        }
        let inside = |value: f32| (0.0..=side).contains(&value);
        if let Some(index) = points
            .iter()
            .position(|point| !inside(point.x) || !inside(point.y))
        {
            return Err(GuiError::Icon(IconError::Point { index }));
        }
        let flipped = points
            .into_iter()
            .map(|point| OutlinePoint {
                y: -point.y,
                ..point
            })
            .collect();
        // The points are finite, so only the ends can be refused.
        let outline = Outline::new(flipped, contour_ends)
            .map_err(|_| GuiError::Icon(IconError::ContourEnds))?;
        Ok(Self { side, outline })
    }

    /// The viewbox's side, in the icon's units.
    pub const fn side(&self) -> f32 {
        self.side
    }

    /// How many points it has.
    pub fn point_count(&self) -> usize {
        self.outline.point_count()
    }

    /// How many contours it has.
    pub fn contour_count(&self) -> usize {
        self.outline.contour_count()
    }

    /// Its coverage with the viewbox `pixels` device pixels across, its
    /// top-left corner at the bitmap's origin, the bitmap no wider or
    /// taller than `pixels`; `None` when `pixels` is not above 0, or
    /// `font`'s default [`Limits`] refuse the bitmap (over 4,096 px a side,
    /// or too much work). An icon of no points gives an empty bitmap.
    pub(crate) fn rasterize(&self, pixels: i32) -> Option<Bitmap> {
        if pixels <= 0 {
            return None;
        }
        outline::rasterize(&self.outline, self.scale(pixels)?, 0.0, Limits::DEFAULT).ok()
    }

    /// The device pixels per unit that put the viewbox's far edge at most
    /// `pixels` from its near one: `pixels / side`, rounded in `f32`, and
    /// stepped down while that rounding put the edge past `pixels`, so that
    /// `font` does not round it up to one pixel more. `None` if no finite
    /// scale does: drawn over 4,096 px, which `font` refuses anyway.
    fn scale(&self, pixels: i32) -> Option<f32> {
        let fits = |scale: f32| f64::from(self.side) * f64::from(scale) <= f64::from(pixels);
        let mut scale = pixels as f32 / self.side;
        if !scale.is_finite() {
            return None;
        }
        // `side × scale` is exact in `f64`: two 24-bit significands. A
        // correctly rounded quotient is within a step of the largest scale
        // that fits; two more allow for `pixels` rounding to `f32`.
        for _ in 0..3 {
            if fits(scale) {
                return Some(scale);
            }
            scale = f32::from_bits(scale.to_bits().checked_sub(1)?);
        }
        fits(scale).then_some(scale)
    }
}

/// Why [`Icon::new`] refused an icon, in [`GuiError::Icon`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum IconError {
    /// The viewbox's side is not finite and at least [`MIN_SIDE`].
    Side {
        /// The side given.
        side: f32,
    },
    /// There are more than [`MAX_POINTS`] points.
    TooManyPoints {
        /// How many there are.
        count: usize,
    },
    /// A point is not finite or lies outside the viewbox.
    Point {
        /// Its index among the points.
        index: usize,
    },
    /// The contours' ends do not go up, each by at least 1, from above 0
    /// to the number of points.
    ContourEnds,
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Side { side } => write!(
                f,
                "a viewbox of side {side}: it must be finite and at least 1/65536"
            ),
            Self::TooManyPoints { count } => {
                write!(f, "{count} points: at most {MAX_POINTS}")
            }
            Self::Point { index } => {
                write!(f, "point {index} is not finite or lies outside the viewbox")
            }
            Self::ContourEnds => f.write_str(
                "the contours' ends do not go up, each by at least 1, from above 0 to the \
                 number of points",
            ),
        }
    }
}

impl std::error::Error for IconError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(x: f32, y: f32) -> OutlinePoint {
        OutlinePoint {
            x,
            y,
            on_curve: true,
        }
    }

    fn square(left: f32, top: f32, side: f32) -> Vec<OutlinePoint> {
        vec![
            on(left, top),
            on(left + side, top),
            on(left + side, top + side),
            on(left, top + side),
        ]
    }

    #[test]
    fn the_viewbox_maps_onto_the_bitmap_from_its_top_left_corner() {
        // A square in the top-left quarter of a 4-unit viewbox, drawn 8 px
        // across: rows and columns 0 to 4 are full.
        let icon = Icon::new(4.0, square(0.0, 0.0, 2.0), vec![4]).expect("an icon");
        let bitmap = icon.rasterize(8).expect("a bitmap");
        assert_eq!(
            (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
            (0, 0, 4, 4)
        );
        assert!(bitmap.pixels.iter().all(|&coverage| coverage == 255));
        // One in the bottom-right quarter lands there.
        let icon = Icon::new(4.0, square(2.0, 2.0, 2.0), vec![4]).expect("an icon");
        let bitmap = icon.rasterize(8).expect("a bitmap");
        assert_eq!(
            (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
            (4, 4, 4, 4)
        );
    }

    #[test]
    fn a_hole_winding_the_other_way_is_empty() {
        let mut points = square(0.0, 0.0, 6.0);
        // The same square inside, backwards.
        let mut hole = square(2.0, 2.0, 2.0);
        hole.reverse();
        points.extend(hole);
        let icon = Icon::new(6.0, points, vec![4, 8]).expect("an icon");
        let bitmap = icon.rasterize(6).expect("a bitmap");
        let at = |column: u32, row: u32| bitmap.get(column, row);
        assert_eq!(at(0, 0), Some(255));
        assert_eq!(at(2, 2), Some(0));
        assert_eq!(at(3, 3), Some(0));
        assert_eq!(at(4, 4), Some(255));
    }

    #[test]
    fn a_curve_is_filled_through_its_control_point() {
        // A semicircle-like bump: base on the bottom edge, a control point
        // at the top.
        let points = vec![
            on(0.0, 16.0),
            OutlinePoint {
                x: 8.0,
                y: 0.0,
                on_curve: false,
            },
            on(16.0, 16.0),
        ];
        let icon = Icon::new(16.0, points, vec![3]).expect("an icon");
        let bitmap = icon.rasterize(16).expect("a bitmap");
        // The curve peaks half-way to its control point: row 8 is filled in
        // the middle, row 7 is not.
        let column = 8 - u32::try_from(bitmap.left).expect("left of 0");
        let row = |y: i32| u32::try_from(y - bitmap.top).expect("a row");
        assert_eq!(bitmap.get(column, row(9)), Some(255));
        assert_eq!(bitmap.get(column, row(7)), Some(0));
    }

    #[test]
    fn the_rules_are_checked_in_order() {
        let error = |side, points, ends| Icon::new(side, points, ends).expect_err("refused");
        for side in [0.0, -1.0, f32::NAN, f32::INFINITY, 1e-30, MIN_SIDE / 2.0] {
            assert!(
                matches!(
                    error(side, vec![], vec![]),
                    GuiError::Icon(IconError::Side { .. })
                ),
                "{side}"
            );
        }
        let many = vec![on(0.0, 0.0); MAX_POINTS + 1];
        assert!(matches!(
            error(1.0, many, vec![MAX_POINTS + 1]),
            GuiError::Icon(IconError::TooManyPoints { count }) if count == MAX_POINTS + 1
        ));
        for (index, point) in [on(-0.1, 0.0), on(0.0, 4.5), on(f32::NAN, 1.0)]
            .into_iter()
            .enumerate()
        {
            let mut points = square(0.0, 0.0, 4.0);
            points.push(point);
            let at = 4;
            assert!(
                matches!(
                    error(4.0, points, vec![5]),
                    GuiError::Icon(IconError::Point { index }) if index == at
                ),
                "{index}"
            );
        }
        for ends in [vec![3], vec![0, 4], vec![2, 2, 4], vec![5]] {
            assert!(
                matches!(
                    error(4.0, square(0.0, 0.0, 4.0), ends.clone()),
                    GuiError::Icon(IconError::ContourEnds)
                ),
                "{ends:?}"
            );
        }
        // With two faults, the earlier rule's.
        let mut nan_and_many = vec![on(0.0, 0.0); MAX_POINTS + 1];
        nan_and_many[0] = on(f32::NAN, 0.0);
        assert!(matches!(
            error(f32::NAN, nan_and_many.clone(), vec![3]),
            GuiError::Icon(IconError::Side { .. })
        ));
        assert!(matches!(
            error(1.0, nan_and_many, vec![3]),
            GuiError::Icon(IconError::TooManyPoints { .. })
        ));
        let mut outside = square(0.0, 0.0, 4.0);
        outside[2] = on(9.0, 9.0);
        assert!(matches!(
            error(4.0, outside, vec![2]),
            GuiError::Icon(IconError::Point { index: 2 })
        ));
        // The edges of the viewbox are inside it, and so is an empty icon.
        assert!(Icon::new(4.0, square(0.0, 0.0, 4.0), vec![4]).is_ok());
        let empty = Icon::new(4.0, vec![], vec![]).expect("an empty icon");
        assert_eq!(empty.rasterize(16), Some(Bitmap::default()));
    }

    #[test]
    fn sizes_outside_fonts_limits_draw_nothing() {
        let icon = Icon::new(1.0, square(0.0, 0.0, 1.0), vec![4]).expect("an icon");
        assert!(icon.rasterize(0).is_none());
        assert!(icon.rasterize(-5).is_none());
        assert!(icon.rasterize(4096).is_some());
        assert!(icon.rasterize(4097).is_none());
    }

    #[test]
    fn a_viewbox_drawn_n_pixels_across_is_at_most_n_pixels_wide() {
        // `n / side` rounds up in `f32` for most sides; the far edge must
        // still land on pixel `n`, not one past it.
        for side in [3.0, 5.0, 7.0, 9.0, 13.0, 20.0, 24.0, 48.0, 0.1, 1e-3] {
            let icon = Icon::new(side, square(0.0, 0.0, side), vec![4]).expect("an icon");
            for pixels in [1, 7, 13, 16, 24, 100, 4095, 4096] {
                let bitmap = icon.rasterize(pixels).expect("within font's limits");
                let full = u32::try_from(pixels).expect("positive");
                assert_eq!(
                    (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
                    (0, 0, full, full),
                    "side {side} at {pixels} px"
                );
            }
        }
        // The smallest side draws whole at 4,096 px; past `f32`, nothing.
        let tiny =
            Icon::new(MIN_SIDE, square(0.0, 0.0, MIN_SIDE), vec![4]).expect("a tiny viewbox");
        let bitmap = tiny.rasterize(4096).expect("within font's limits");
        assert_eq!((bitmap.width, bitmap.height), (4096, 4096));
        assert!(tiny.rasterize(i32::MAX).is_none());
    }

    #[test]
    fn errors_say_what_is_wrong() {
        assert_eq!(
            IconError::Side { side: -2.0 }.to_string(),
            "a viewbox of side -2: it must be finite and at least 1/65536"
        );
        assert_eq!(
            IconError::TooManyPoints { count: 70_000 }.to_string(),
            "70000 points: at most 65536"
        );
        assert_eq!(
            IconError::Point { index: 3 }.to_string(),
            "point 3 is not finite or lies outside the viewbox"
        );
        assert_eq!(
            IconError::ContourEnds.to_string(),
            "the contours' ends do not go up, each by at least 1, from above 0 to the number \
             of points"
        );
    }
}
