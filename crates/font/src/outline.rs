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

//! Glyph outlines and their rasterizer: an [`Outline`] of quadratic
//! contours in font units, and [`rasterize`], which turns it into a
//! [`Bitmap`] of 8-bit coverage, grayscale and unhinted.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL). The
//! coverage accumulation is the same algorithm as `tidepool_gfx::raster`'s
//! polygon fill, ported here because this crate may depend only on
//! `tidepool-core`.
//!
//! ```
//! use tidepool_font::Limits;
//! use tidepool_font::outline::{self, Outline, OutlinePoint};
//!
//! // A 2 × 1 rectangle in font units, drawn at one pixel a unit.
//! let corner = |x, y| OutlinePoint { x, y, on_curve: true };
//! let rectangle = vec![corner(0.0, 0.0), corner(0.0, 1.0), corner(2.0, 1.0), corner(2.0, 0.0)];
//! let outline = Outline::new(rectangle, vec![4])?;
//! let bitmap = outline::rasterize(&outline, 1.0, 0.0, Limits::DEFAULT)?;
//! assert_eq!((bitmap.left, bitmap.top, bitmap.width, bitmap.height), (0, -1, 2, 1));
//! assert_eq!(bitmap.pixels, [255, 255]);
//! # Ok::<(), tidepool_font::FontError>(())
//! ```
//!
//! # Outlines
//!
//! An outline is in font units with y up, as TrueType has it: contours of
//! points on and off the curve. Between two points on the curve is a line;
//! a point off the curve is the control point of a quadratic Bézier curve
//! between its neighbours; and between two consecutive points off the
//! curve is an implied point on it, half-way between them. A contour may
//! start off the curve; one with no point on it at all is a closed chain
//! of curves through those implied points.
//!
//! # The bitmap
//!
//! [`rasterize`] scales the outline by `scale` pixels per font unit
//! (pixels per em ÷ units per em), adds `shift_x` pixels to every x (a
//! subpixel offset: the atlas, M2.5, keys glyphs by quarters of a pixel),
//! and flips y to grow downwards. The bitmap covers the scaled control box
//! of the outline (the box of all its points, off the curve too, which
//! holds the curves), each side rounded outwards to whole pixels. Its
//! pixel at column `c`, row `r` covers device x in
//! `[left + c, left + c + 1)` and y in `[top + r, top + r + 1)`, where the
//! glyph's origin (the pen, on the baseline) is (0, 0): so `top` is
//! negative for a glyph above the baseline. That is the layout of
//! `tidepool_gfx::glyph::Mask`: one byte a pixel, 0 for none and 255 for
//! full, rows from the top, `width` bytes apart.
//!
//! Each side is at most [`Limits::max_bitmap_dimension`], and the work of
//! drawing it at most [`Limits::max_raster_work`] (an estimate from the
//! scaled outline, see there), both checked before the bitmap is
//! allocated. The rasterizer then works in bands of rows, each of at least
//! one row and of at most [`MAX_CELLS`] cells of 8 bytes (8 MiB), `width +
//! 2` cells a row, and reads the outline once per band: a glyph a few
//! hundred pixels across is a single band; the largest bitmap the default
//! limits allow, 4,096 pixels a side, takes 17. A row wider than
//! `MAX_CELLS − 2` pixels, which only a raised
//! [`Limits::max_bitmap_dimension`] allows, is a band on its own, of more
//! than [`MAX_CELLS`] cells.
//!
//! # Curves
//!
//! Each curve is flattened into `n` lines of equal steps of its
//! parameter, with `n = ceil(sqrt(|p0 − 2·p1 + p2| / (4 · TOLERANCE)))`
//! (in pixels) clamped to 1 to [`MAX_SEGMENTS`]: then no point of the
//! curve is more than [`TOLERANCE`] (1/32 of a pixel) from its lines. The
//! clamp is reached only by a curve whose control points span more than
//! 11,585 pixels, beyond the bitmaps the default limits allow; past it the
//! lines are fewer, and further from the curve.
//!
//! # Coverage
//!
//! Each pixel's coverage is the exact area of the flattened outline inside
//! it, by accumulation (as in font-rs): each line adds, to each cell of
//! each row it crosses, the signed area it contributes, and a prefix sum
//! along the row turns that into coverage, `min(|sum|, 1)`. It becomes a
//! byte as `floor(coverage × 255 + 0.5)`.
//!
//! For contours that wind the same way and overlap neither themselves nor
//! one another, that is the non-zero winding rule exactly, and so is a
//! hole nested inside one that winds the other way (as TrueType's holes
//! do). Elsewhere it can differ, since the signed areas are summed in each
//! pixel before they are clamped:
//!
//! - where contours winding the same way overlap, the overlap is filled
//!   once, but a pixel its edges cross can get up to twice its coverage
//!   before the clamp, so it comes out darker than the non-zero rule's, up
//!   to full;
//! - where contours winding opposite ways share a pixel without
//!   overlapping (a mirrored component beside its original, say, touching
//!   it), their coverage cancels in that pixel, leaving a seam the non-zero
//!   rule would fill;
//! - a contour that crosses itself has lobes winding opposite ways, whose
//!   coverage cancels where they share a pixel, and one that winds twice
//!   round a region counts its edge pixels twice.
//!
//! Static fonts usually have their overlaps removed. Where a font keeps
//! them (a component placed over another, a simple glyph marked
//! `OVERLAP_SIMPLE`), or mirrors a component beside itself, only the
//! pixels those edges cross differ.
//!
//! # Determinism
//!
//! The arithmetic is `f64`, with IEEE 754 `+ − × ÷`, `sqrt`, `floor` and
//! `ceil`, which every CPU rounds the same, in a fixed order; never
//! `mul_add` or a libm function. So a glyph's coverage is the same, byte
//! for byte, on every run and every OS.

use std::fmt;

use crate::{FontError, Limits};

mod accumulate;

use self::accumulate::{accumulate_piece, split_at_sides, to_coverage};

/// The most a flattened curve may be from its lines, in pixels: 1/32.
pub const TOLERANCE: f64 = 1.0 / 32.0;

/// The most lines one curve is flattened into. With [`TOLERANCE`], enough
/// for any curve whose control points lie in a box of up to 11,585 pixels
/// a side: its `|p0 − 2·p1 + p2|` is at most twice the box's diagonal, and
/// `sqrt(2 × 11,585 × √2 / (4 · TOLERANCE))` is just under 512. The
/// default limits' largest bitmap, 4,096 pixels a side, needs at most 305.
pub const MAX_SEGMENTS: u32 = 512;

/// The most cells (of 8 bytes) the rasterizer accumulates at a time: a
/// band of rows of `width + 2` cells each, 2^20 cells, 8 MiB. A band is at
/// least one row, so a row wider than `MAX_CELLS − 2` pixels is
/// accumulated alone, in more cells than this.
pub const MAX_CELLS: usize = 1 << 20;

/// What [`Limits::max_raster_work`] charges for each edge of an outline's
/// control polygon, besides its length: the lines it is flattened into and
/// the rows they start in.
pub const EDGE_WORK: u64 = 4;

/// A point of an [`Outline`], in font units, y up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutlinePoint {
    /// Rightwards.
    pub x: f32,
    /// Upwards.
    pub y: f32,
    /// On the curve, rather than a quadratic curve's control point.
    pub on_curve: bool,
}

/// A box in font units, y up: the control box of an [`Outline`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// The leftmost x.
    pub x_min: f32,
    /// The lowest y.
    pub y_min: f32,
    /// The rightmost x.
    pub x_max: f32,
    /// The highest y.
    pub y_max: f32,
}

/// A glyph's outline: closed contours of TrueType quadratic points (the
/// module docs, "Outlines"), in font units, y up. Every coordinate is
/// finite.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    points: Vec<OutlinePoint>,
    /// One past each contour's last point, ascending; the last is the
    /// point count.
    ends: Vec<usize>,
}

impl Outline {
    /// An outline of `points`, in contours that end where `contour_ends`
    /// says: contour `i` is `points[contour_ends[i - 1]..contour_ends[i]]`,
    /// the first starting at 0. Both empty is the empty outline.
    ///
    /// # Errors
    ///
    /// [`FontError::Outline`] with [`ErrorKind::BadContourEnds`] unless
    /// the ends go up, each by at least 1, from above 0 to the number of
    /// points; with [`ErrorKind::NonFinitePoint`] for a point with a NaN or
    /// infinite coordinate.
    pub fn new(points: Vec<OutlinePoint>, contour_ends: Vec<usize>) -> Result<Self, FontError> {
        let ascending = contour_ends
            .iter()
            .try_fold(0, |previous, &end| (end > previous).then_some(end));
        if ascending != Some(points.len()) {
            return Err(FontError::Outline {
                kind: ErrorKind::BadContourEnds,
            });
        }
        if let Some(index) = points
            .iter()
            .position(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(FontError::Outline {
                kind: ErrorKind::NonFinitePoint { index },
            });
        }
        Ok(Self {
            points,
            ends: contour_ends,
        })
    }

    /// An outline from what the font reader built, which keeps the rules
    /// [`Outline::new`] checks.
    pub(crate) fn from_parts(points: Vec<OutlinePoint>, ends: Vec<usize>) -> Self {
        debug_assert!(Self::new(points.clone(), ends.clone()).is_ok());
        Self { points, ends }
    }

    /// The capacities of its vectors of points and of contour ends, for
    /// tests of what an outline holds.
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> (usize, usize) {
        (self.points.capacity(), self.ends.capacity())
    }

    /// Whether it has no points, and so draws nothing.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Every point, contour after contour.
    pub fn points(&self) -> &[OutlinePoint] {
        &self.points
    }

    /// How many points it has.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// One past each contour's last point, as [`Outline::new`] takes them.
    pub fn contour_ends(&self) -> &[usize] {
        &self.ends
    }

    /// How many contours it has.
    pub fn contour_count(&self) -> usize {
        self.ends.len()
    }

    /// Each contour's points, in order.
    pub fn contours(&self) -> impl ExactSizeIterator<Item = &[OutlinePoint]> + '_ {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let contour = self.points.get(start..end).unwrap_or_default();
            start = end;
            contour
        })
    }

    /// The box of every point, on the curve or not, which holds the
    /// outline; `None` for an empty outline.
    pub fn control_box(&self) -> Option<Bounds> {
        let (first, rest) = self.points.split_first()?;
        let mut bounds = Bounds {
            x_min: first.x,
            y_min: first.y,
            x_max: first.x,
            y_max: first.y,
        };
        for point in rest {
            bounds.x_min = bounds.x_min.min(point.x);
            bounds.y_min = bounds.y_min.min(point.y);
            bounds.x_max = bounds.x_max.max(point.x);
            bounds.y_max = bounds.y_max.max(point.y);
        }
        Some(bounds)
    }
}

/// A glyph's coverage, as [`rasterize`] makes it (the module docs, "The
/// bitmap"): one byte a pixel, 0 for none and 255 for full, rows from the
/// top, `width` bytes apart.
///
/// `Debug` shows the position and size, not the pixels.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Bitmap {
    /// The device x of column 0's left edge, from the glyph's origin.
    pub left: i32,
    /// The device y of row 0's top edge, from the glyph's origin, growing
    /// downwards: negative above the baseline.
    pub top: i32,
    /// Columns.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// `width × height` bytes of coverage.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// The coverage of the pixel at `column`, `row`, or `None` outside the
    /// bitmap.
    pub fn get(&self, column: u32, row: u32) -> Option<u8> {
        if column >= self.width || row >= self.height {
            return None;
        }
        let index = row as usize * self.width as usize + column as usize;
        self.pixels.get(index).copied()
    }
}

impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bitmap")
            .field("left", &self.left)
            .field("top", &self.top)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixels", &self.pixels.len())
            .finish()
    }
}

/// Why an outline cannot be built or rasterized. See
/// [`FontError::Outline`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// [`rasterize`]'s scale is NaN, infinite, or not above 0.
    BadScale,
    /// [`rasterize`]'s shift is NaN or infinite.
    BadShift,
    /// The bitmap would be wider or taller than
    /// [`Limits::max_bitmap_dimension`].
    BitmapTooLarge {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// The bitmap's edges would be beyond the range of an `i32`. (The
    /// scaled outline itself is always finite: `f32` coordinates times an
    /// `f32` scale stay within `f64`.)
    BitmapOutOfRange,
    /// [`Outline::new`]'s contour ends do not go up from above 0 to the
    /// number of points.
    BadContourEnds,
    /// Rasterizing would take more work than [`Limits::max_raster_work`]
    /// (see there for how it is counted). Checked before the bitmap is
    /// allocated.
    TooMuchWork {
        /// The limit that was exceeded.
        limit: u64,
    },
    /// A point given to [`Outline::new`] has a NaN or infinite coordinate.
    NonFinitePoint {
        /// The point's index.
        index: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadScale => f.write_str("the scale is not a finite number above 0"),
            Self::BadShift => f.write_str("the shift is not a finite number"),
            Self::BitmapTooLarge { limit } => write!(
                f,
                "the bitmap would be wider or taller than the limit of {limit} pixels"
            ),
            Self::BitmapOutOfRange => {
                f.write_str("the bitmap's edges would be beyond the range of an i32")
            }
            Self::BadContourEnds => {
                f.write_str("the contours' ends do not go up from above 0 to the number of points")
            }
            Self::TooMuchWork { limit } => write!(
                f,
                "rasterizing would take more work than the limit of {limit} steps"
            ),
            Self::NonFinitePoint { index } => {
                write!(f, "point {index} has a coordinate that is not finite")
            }
        }
    }
}

fn error(kind: ErrorKind) -> FontError {
    FontError::Outline { kind }
}

/// Rasterizes `outline` at `scale` pixels per font unit, moved right by
/// `shift_x` pixels, as the module docs say.
///
/// An empty outline gives an empty bitmap (0 × 0, at 0, 0).
///
/// # Errors
///
/// [`FontError::Outline`] with:
///
/// - [`ErrorKind::BadScale`] for a scale that is not finite and above 0;
/// - [`ErrorKind::BadShift`] for a shift that is not finite;
/// - [`ErrorKind::BitmapTooLarge`] for a bitmap wider or taller than
///   [`Limits::max_bitmap_dimension`];
/// - [`ErrorKind::BitmapOutOfRange`] when its edges do not fit in an `i32`;
/// - [`ErrorKind::TooMuchWork`] when the estimated work is more than
///   [`Limits::max_raster_work`].
///
/// They are checked in that order, all before the bitmap and the
/// rasterizer's working rows are allocated. Only the copy of the points in
/// device pixels that the work is estimated from (16 bytes a point) is made
/// before the last check.
pub fn rasterize(
    outline: &Outline,
    scale: f32,
    shift_x: f32,
    limits: Limits,
) -> Result<Bitmap, FontError> {
    rasterize_in_bands(outline, scale, shift_x, limits, MAX_CELLS)
}

/// [`rasterize`], accumulating at most `max_cells` cells at a time (at
/// least one row): the result does not depend on it.
fn rasterize_in_bands(
    outline: &Outline,
    scale: f32,
    shift_x: f32,
    limits: Limits,
    max_cells: usize,
) -> Result<Bitmap, FontError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(error(ErrorKind::BadScale));
    }
    if !shift_x.is_finite() {
        return Err(error(ErrorKind::BadShift));
    }
    let Some(bounds) = outline.control_box() else {
        return Ok(Bitmap::default());
    };
    let (scale, shift) = (f64::from(scale), f64::from(shift_x));
    // The device transform; the control box goes through the same, so
    // every point lies inside the box's pixels.
    let device_x = |x: f32| f64::from(x) * scale + shift;
    let device_y = |y: f32| -(f64::from(y) * scale);
    let left = device_x(bounds.x_min).floor();
    let right = device_x(bounds.x_max).ceil();
    let top = device_y(bounds.y_max).floor();
    let bottom = device_y(bounds.y_min).ceil();
    // Never true, since `f32` times `f32` stays within `f64`; kept so that
    // no NaN can reach the casts below.
    if ![left, right, top, bottom]
        .iter()
        .all(|edge| edge.is_finite())
    {
        return Err(error(ErrorKind::BitmapOutOfRange));
    }
    let limit = limits.max_bitmap_dimension;
    let (width, height) = (right - left, bottom - top);
    if width > f64::from(limit) || height > f64::from(limit) {
        return Err(error(ErrorKind::BitmapTooLarge { limit }));
    }
    let range = f64::from(i32::MIN)..=f64::from(i32::MAX);
    if ![left, right, top, bottom]
        .iter()
        .all(|edge| range.contains(edge))
    {
        return Err(error(ErrorKind::BitmapOutOfRange));
    }
    // Whole numbers within the limit, and within `i32`: the casts are
    // exact.
    let bitmap_width = width as u32;
    let bitmap_height = height as u32;
    let Some(len) = (bitmap_width as usize).checked_mul(bitmap_height as usize) else {
        return Err(error(ErrorKind::BitmapTooLarge { limit }));
    };
    if len == 0 {
        return Ok(Bitmap {
            left: left as i32,
            top: top as i32,
            width: bitmap_width,
            height: bitmap_height,
            pixels: Vec::new(),
        });
    }

    // The points in bitmap coordinates.
    let points: Vec<(f64, f64)> = outline
        .points
        .iter()
        .map(|point| (device_x(point.x) - left, device_y(point.y) - top))
        .collect();
    let columns = bitmap_width as usize;
    let rows = i64::from(bitmap_height);
    let stride = columns + 2;
    let band = i64::try_from((max_cells / stride).max(1))
        .unwrap_or(rows)
        .min(rows);
    let bands = (rows + band - 1) / band;
    let work = work(&points, &outline.ends, bands.unsigned_abs());
    if work > limits.max_raster_work {
        return Err(error(ErrorKind::TooMuchWork {
            limit: limits.max_raster_work,
        }));
    }
    let mut bitmap = Bitmap {
        left: left as i32,
        top: top as i32,
        width: bitmap_width,
        height: bitmap_height,
        pixels: vec![0; len],
    };
    let mut cells = vec![0.0; stride * band as usize];
    let mut band_top = 0;
    while band_top < rows {
        let band_bottom = (band_top + band).min(rows);
        cells.fill(0.0);
        let mut start = 0;
        for &end in &outline.ends {
            let contour = (
                points.get(start..end).unwrap_or_default(),
                outline.points.get(start..end).unwrap_or_default(),
            );
            start = end;
            segments(contour.0, contour.1, band_top, band_bottom, |a, b| {
                split_at_sides(a, b, width, |piece| {
                    accumulate_piece(&mut cells, stride, band_top, band_bottom, piece);
                });
            });
        }
        let offset = band_top as usize * columns;
        let pixels = bitmap.pixels.get_mut(offset..).unwrap_or_default();
        for (cells_row, pixels_row) in cells
            .chunks_exact(stride)
            .zip(pixels.chunks_exact_mut(columns))
        {
            let mut sum = 0.0;
            for (cell, pixel) in cells_row.iter().zip(pixels_row.iter_mut()) {
                sum += cell;
                *pixel = to_coverage(f64::abs(sum));
            }
        }
        band_top = band_bottom;
    }
    Ok(bitmap)
}

/// The work of rasterizing the contours that end at `ends` of `points`
/// (bitmap coordinates) in `bands` bands, as [`Limits::max_raster_work`]
/// counts it: for each edge of the control polygon (each point to the
/// next, the last back to the first), its length in x plus its length in
/// y, rounded up, plus [`EDGE_WORK`]; and for each band, every point once.
///
/// It bounds what accumulating takes. A curve's lines move no further, in
/// x or in y, than its control points do (a quadratic Bézier curve is in
/// the hull of its control points, and each coordinate goes up and down at
/// most as often as theirs), and a line visits each row it crosses and each
/// cell it crosses in each, plus two; it is flattened into at most its
/// length plus 3 lines. So the cells visited are at most about twice the
/// sum of the lengths, plus a few for each edge; and each band reads the
/// outline again, for each point a step. A curve is flattened again in
/// every band its control points reach, up to 17 times under the default
/// limits; that is not counted, and the measurements in
/// [`Limits::max_raster_work`] include it.
fn work(points: &[(f64, f64)], ends: &[usize], bands: u64) -> u64 {
    let mut total = bands.saturating_mul(points.len() as u64);
    let mut start = 0;
    for &end in ends {
        let contour = points.get(start..end).unwrap_or_default();
        start = end;
        let next = contour.iter().skip(1).chain(contour.first());
        for (&(x0, y0), &(x1, y1)) in contour.iter().zip(next) {
            // Finite and within the bitmap's box, so the cast is exact.
            let length = ((x1 - x0).abs() + (y1 - y0).abs()).ceil() as u64;
            total = total.saturating_add(length).saturating_add(EDGE_WORK);
        }
    }
    total
}

/// Hands each line of a contour, flattened, to `line`: `points` are its
/// points in bitmap coordinates, and `kinds` the outline's own, for which
/// are on the curve. A curve wholly outside the rows `band_top..band_bottom`
/// (with a pixel to spare, so rounding never drops one that reaches in) is
/// handed over as its chord, which adds nothing to those rows either.
fn segments(
    points: &[(f64, f64)],
    kinds: &[OutlinePoint],
    band_top: i64,
    band_bottom: i64,
    mut line: impl FnMut((f64, f64), (f64, f64)),
) {
    let count = points.len().min(kinds.len());
    if count == 0 {
        return;
    }
    let point = |index: usize| -> ((f64, f64), bool) {
        let at = index % count;
        (
            points.get(at).copied().unwrap_or_default(),
            kinds.get(at).is_some_and(|kind| kind.on_curve),
        )
    };
    let middle = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (top, bottom) = ((band_top - 1) as f64, (band_bottom + 1) as f64);
    // A line (no control point) or a curve, flattened.
    let mut edge = |from: (f64, f64), control: Option<(f64, f64)>, to: (f64, f64)| {
        let Some(control) = control else {
            line(from, to);
            return;
        };
        let low = from.1.min(control.1).min(to.1);
        let high = from.1.max(control.1).max(to.1);
        if high < top || low > bottom {
            line(from, to);
        } else {
            flatten(from, control, to, &mut line);
        }
    };
    // Start at the first point on the curve; with none, at the implied
    // point between the last and the first.
    let first_on = (0..count).find(|&index| point(index).1);
    let (start, first) = match first_on {
        Some(index) => (point(index).0, index + 1),
        None => (middle(point(count - 1).0, point(0).0), 0),
    };
    let mut current = start;
    let mut control: Option<(f64, f64)> = None;
    for step in 0..count - usize::from(first_on.is_some()) {
        let (here, on_curve) = point(first + step);
        if on_curve {
            edge(current, control, here);
            current = here;
            control = None;
        } else if let Some(previous) = control {
            let implied = middle(previous, here);
            edge(current, Some(previous), implied);
            current = implied;
            control = Some(here);
        } else {
            control = Some(here);
        }
    }
    edge(current, control, start);
}

/// How many lines the curve from `p0` through control `p1` to `p2` (in
/// pixels) is flattened into: `ceil(sqrt(|p0 − 2·p1 + p2| / (4 ·
/// TOLERANCE)))`, from 1 to [`MAX_SEGMENTS`].
fn segment_count(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> u32 {
    let dx = p0.0 - 2.0 * p1.0 + p2.0;
    let dy = p0.1 - 2.0 * p1.1 + p2.1;
    let deviation = (dx * dx + dy * dy).sqrt();
    let count = (deviation / (4.0 * TOLERANCE)).sqrt().ceil();
    if count >= f64::from(MAX_SEGMENTS) {
        MAX_SEGMENTS
    } else if count >= 1.0 {
        count as u32
    } else {
        1
    }
}

/// Hands the lines of the curve from `p0` through control `p1` to `p2`
/// to `line`, at equal steps of its parameter; the last ends at `p2`
/// exactly.
fn flatten(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    line: &mut impl FnMut((f64, f64), (f64, f64)),
) {
    let count = segment_count(p0, p1, p2);
    let mut from = p0;
    for step in 1..count {
        let t = f64::from(step) / f64::from(count);
        let u = 1.0 - t;
        let (a, b, c) = (u * u, 2.0 * u * t, t * t);
        let to = (
            a * p0.0 + b * p1.0 + c * p2.0,
            a * p0.1 + b * p1.1 + c * p2.1,
        );
        line(from, to);
        from = to;
    }
    line(from, p2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_flatten_within_the_tolerance() {
        // A straight "curve" is one line.
        assert_eq!(segment_count((0.0, 0.0), (1.0, 1.0), (2.0, 2.0)), 1);
        // |p0 − 2·p1 + p2| = 2: sqrt(2 / (4 / 32)) = 4.
        assert_eq!(segment_count((0.0, 0.0), (1.0, 1.0), (2.0, 0.0)), 4);
        // A little more needs one more.
        assert_eq!(segment_count((0.0, 0.0), (1.0, 1.01), (2.0, 0.0)), 5);
        // A huge curve is clamped.
        assert_eq!(
            segment_count((0.0, 0.0), (1e9, 1e9), (2e9, 0.0)),
            MAX_SEGMENTS
        );
        // The worst curve in a box of 11,585 pixels a side (its ends at one
        // corner, its control point at the other) needs every line, and
        // the clamp leaves it within the tolerance; in the default limits'
        // box of 4,096, 305 do.
        assert_eq!(
            segment_count((0.0, 0.0), (11_585.0, 11_585.0), (0.0, 0.0)),
            MAX_SEGMENTS
        );
        let deviation = 2.0 * 11_585.0 * 2f64.sqrt();
        assert!(deviation / (4.0 * f64::from(MAX_SEGMENTS).powi(2)) <= TOLERANCE);
        let side = f64::from(Limits::DEFAULT.max_bitmap_dimension);
        assert_eq!(segment_count((0.0, 0.0), (side, side), (0.0, 0.0)), 305);
    }

    #[test]
    fn a_flattened_curve_stays_within_the_tolerance_of_it() {
        let (p0, p1, p2) = ((0.0, 0.0), (10.0, 20.0), (20.0, 0.0));
        let mut lines = Vec::new();
        flatten(p0, p1, p2, &mut |a, b| lines.push((a, b)));
        assert_eq!(lines.len(), segment_count(p0, p1, p2) as usize);
        assert_eq!(lines.first().map(|line| line.0), Some(p0));
        assert_eq!(lines.last().map(|line| line.1), Some(p2));
        // The curve's height above each line's middle is at most the
        // tolerance: y(t) = 40·t·(1 − t) at x = 20·t.
        for &((x0, y0), (x1, y1)) in &lines {
            let t = (x0 + x1) / 2.0 / 20.0;
            let curve = 40.0 * t * (1.0 - t);
            assert!(curve - (y0 + y1) / 2.0 <= TOLERANCE + 1e-12);
        }
    }

    #[test]
    fn contours_close_through_implied_points() {
        let kind = |on_curve| OutlinePoint {
            on_curve,
            ..OutlinePoint::default()
        };
        let mut lines = Vec::new();
        // A triangle of points on the curve: three lines, back to the start.
        let points = [(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)];
        segments(&points, &[kind(true); 3], 0, 10, |a, b| lines.push((a, b)));
        assert_eq!(
            lines,
            [
                ((0.0, 0.0), (4.0, 0.0)),
                ((4.0, 0.0), (0.0, 4.0)),
                ((0.0, 4.0), (0.0, 0.0))
            ]
        );
        // Starting off the curve: it starts at the next point on it.
        lines.clear();
        let kinds = [kind(false), kind(true), kind(true)];
        segments(&points, &kinds, 0, 10, |a, b| lines.push((a, b)));
        assert_eq!(lines.first().map(|line| line.0), Some((4.0, 0.0)));
        assert_eq!(lines.last().map(|line| line.1), Some((4.0, 0.0)));
        // Points all off the curve: it starts between the last and the
        // first, and ends there.
        lines.clear();
        segments(&points, &[kind(false); 3], 0, 10, |a, b| lines.push((a, b)));
        assert_eq!(lines.first().map(|line| line.0), Some((0.0, 2.0)));
        assert_eq!(lines.last().map(|line| line.1), Some((0.0, 2.0)));
        // Each line starts where the one before ends.
        for pair in lines.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
    }

    #[test]
    fn bands_never_change_the_coverage() {
        // A ring of curves (8 quadratic arcs outside, 8 inside winding the
        // other way) and a slanted bar, at a scale and shift that put
        // edges everywhere inside pixels.
        let mut points = Vec::new();
        let mut ends = Vec::new();
        for (radius, direction) in [(40.0f32, 1.0f32), (25.0, -1.0)] {
            for step in 0..16 {
                let angle = direction * step as f32 * std::f32::consts::PI / 8.0;
                // Even steps on the circle, odd ones off it (further out).
                let reach = if step % 2 == 0 {
                    radius
                } else {
                    radius * 1.082
                };
                points.push(OutlinePoint {
                    x: 50.0 + reach * angle.cos(),
                    y: 50.0 + reach * angle.sin(),
                    on_curve: step % 2 == 0,
                });
            }
            ends.push(points.len());
        }
        for (x, y) in [(0.0, 0.0), (10.0, 100.0), (14.0, 100.0), (4.0, 0.0)] {
            points.push(OutlinePoint {
                x,
                y,
                on_curve: true,
            });
        }
        ends.push(points.len());
        let outline = Outline::new(points, ends).expect("valid");
        let whole = rasterize_in_bands(&outline, 0.73, 0.3, Limits::DEFAULT, usize::MAX)
            .expect("rasterizes");
        assert!(whole.height > 40 && whole.pixels.iter().any(|&p| p > 0 && p < 255));
        for cells in [
            0,
            1,
            whole.width as usize + 2,
            5 * (whole.width as usize + 2) + 3,
            MAX_CELLS,
        ] {
            let banded = rasterize_in_bands(&outline, 0.73, 0.3, Limits::DEFAULT, cells)
                .expect("rasterizes");
            assert!(banded == whole, "{cells} cells");
        }
    }

    #[test]
    fn work_counts_lengths_edges_and_bands() {
        // A 10 × 5 rectangle and a 1-point contour: lengths 30 and 0, five
        // edges, five points a band.
        let points = [(0.0, 0.0), (0.0, 5.0), (10.0, 5.0), (10.0, 0.0), (3.0, 3.0)];
        assert_eq!(work(&points, &[4, 5], 1), 30 + 5 * EDGE_WORK + 5);
        assert_eq!(work(&points, &[4, 5], 7), 30 + 5 * EDGE_WORK + 7 * 5);
        // Lengths are rounded up, each edge's x and y together.
        let slanted = [(0.0, 0.0), (0.25, 0.5)];
        assert_eq!(work(&slanted, &[2], 1), 2 + 2 * EDGE_WORK + 2);
        // An edge's length is its x length plus its y length, not the
        // larger: from (0, 0) to (3, 4) and back is 7 each way, not 4.
        let diagonal = [(0.0, 0.0), (3.0, 4.0)];
        assert_eq!(work(&diagonal, &[2], 1), 14 + 2 * EDGE_WORK + 2);
        // Through the rasterizer, one-row bands count every row.
        let outline = Outline::new(
            vec![
                OutlinePoint {
                    x: 0.0,
                    y: 0.0,
                    on_curve: true,
                },
                OutlinePoint {
                    x: 0.0,
                    y: 5.0,
                    on_curve: true,
                },
                OutlinePoint {
                    x: 10.0,
                    y: 5.0,
                    on_curve: true,
                },
                OutlinePoint {
                    x: 10.0,
                    y: 0.0,
                    on_curve: true,
                },
            ],
            vec![4],
        )
        .expect("valid");
        let exactly = |max_raster_work| Limits {
            max_raster_work,
            ..Limits::DEFAULT
        };
        let rows = 30 + 4 * EDGE_WORK + 5 * 4;
        assert!(rasterize_in_bands(&outline, 1.0, 0.0, exactly(rows), 0).is_ok());
        assert!(rasterize_in_bands(&outline, 1.0, 0.0, exactly(rows - 1), 0).is_err());
    }

    #[test]
    fn a_curve_outside_the_band_is_its_chord() {
        let kind = |on_curve| OutlinePoint {
            on_curve,
            ..OutlinePoint::default()
        };
        let points = [(0.0, 50.0), (10.0, 60.0), (20.0, 50.0)];
        let kinds = [kind(true), kind(false), kind(true)];
        let mut lines = Vec::new();
        segments(&points, &kinds, 0, 16, |a, b| lines.push((a, b)));
        assert_eq!(
            lines,
            [((0.0, 50.0), (20.0, 50.0)), ((20.0, 50.0), (0.0, 50.0))]
        );
    }
}
