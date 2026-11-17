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

//! The primitives jabbott.dev's look is drawn with: filled rects, 1 px
//! hairlines, borders, bracket corners, anti-aliased polygons and lines,
//! cut-corner rects and pills.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! Every function composites its color source-over and touches only the
//! pixels inside `clip` and the framebuffer's bounds. The ones that take
//! [`DeviceRect`]s and whole pixels are crisp; [`polygon`], [`line`](fn@line),
//! [`cut_corner_rect`] and [`pill`] are anti-aliased.
//!
//! # Bad geometry
//!
//! No call panics, whatever its geometry, and none writes outside the clip
//! or the framebuffer. An empty rect, a length, width, arm or thickness of
//! zero or less, a NaN or infinite coordinate, or fewer than three points
//! draws nothing. Geometry partly or wholly outside the clip or the
//! framebuffer draws the part inside: edges are computed in `i64` and
//! `f64`, where nothing overflows.
//!
//! # Accuracy far away
//!
//! Rects have `i32` coordinates, and are drawn exactly however far they
//! reach; so are the cuts of [`cut_corner_rect`] and the ends of [`pill`].
//! [`polygon`] and [`line`](fn@line) take `f32` vertices, which can lie
//! much farther out, and work in `f64`, whose precision is relative to the
//! coordinates: the farther a slanted edge's ends are from the
//! framebuffer, the less exactly it is placed. Measured on an 8 × 8
//! framebuffer over 80 slopes and offsets, a half-plane and a 2 px line
//! for each, against the exact area of each pixel:
//!
//! - with every vertex within about 1e11 px of the framebuffer, far beyond
//!   any window, each coverage byte is the exact area rounded to nearest
//!   (an area exactly half-way between two bytes may go either way); a
//!   test checks this bound;
//! - out to about 1e13 px a byte may be 1 off, and at 1e14 px a few off;
//! - from about 1e15 px a slanted edge or a line may come out far wrong,
//!   or not at all: with its ends 1e16 px away, where neighbouring `f64`
//!   values are 2 apart, a line's corners round onto its ends.
//!
//! Horizontal and vertical edges stay exact at any distance. Wherever the
//! vertices are, the result is the same on every CPU, and nothing is
//! written outside the clip.
//!
//! # Polygons
//!
//! [`polygon`] computes each pixel's coverage as the exact area of the
//! polygon inside it, by accumulation (as in font-rs): each edge adds, to
//! each cell of each row it crosses, the signed area it contributes, and a
//! prefix sum along the row turns that into coverage, `min(|sum|, 1)`.
//!
//! For a path that does not overlap itself that is the non-zero rule,
//! exactly, and so is a hole cut out by an inner part winding the other
//! way. Elsewhere it differs at the edges of overlaps, since coverage is
//! summed before it is clamped: a region the path winds round twice the
//! same way is filled once, but the pixels along its edge get up to twice
//! their coverage; and where parts winding opposite ways meet side by side
//! inside a pixel, their coverage cancels there, leaving a seam that the
//! non-zero rule would fill.
//!
//! Only the window `bbox ∩ clip ∩ bounds` is accumulated, 16 rows at a
//! time. Edges are split at the window's left and right sides: a part left
//! of the window still adds its winding, moved onto the left side, and a
//! part right of it is dropped, since it adds nothing visible. Rows are
//! independent, so a part above or below the window is dropped too.
//!
//! Arithmetic is `f64` with `+ − × ÷`, `floor`, `ceil` and `sqrt`, all
//! exact in IEEE 754, in a fixed order, so the coverage is the same on
//! every CPU. It becomes a byte as `floor(coverage × 255 + 0.5)`.

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect};

use crate::blend::{blend_pixel, fill_span, scale};
use crate::clip::Edges;
use crate::framebuffer::{BYTES_PER_PIXEL, Framebuffer, zeroed};

#[cfg(test)]
mod tests;

/// Which way a [`hairline`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// A choice of a rect's corners, for [`brackets`] and [`cut_corner_rect`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Corners {
    /// The top-left corner.
    pub top_left: bool,
    /// The top-right corner.
    pub top_right: bool,
    /// The bottom-right corner.
    pub bottom_right: bool,
    /// The bottom-left corner.
    pub bottom_left: bool,
}

impl Corners {
    /// Every corner.
    pub const ALL: Self = Self {
        top_left: true,
        top_right: true,
        bottom_right: true,
        bottom_left: true,
    };

    /// No corner.
    pub const NONE: Self = Self {
        top_left: false,
        top_right: false,
        bottom_right: false,
        bottom_left: false,
    };

    /// Top-left and bottom-right: the bracket corners of a focused panel
    /// (`.panel--brackets`).
    pub const TOP_LEFT_AND_BOTTOM_RIGHT: Self = Self {
        top_left: true,
        top_right: false,
        bottom_right: true,
        bottom_left: false,
    };

    /// Top-right and bottom-left: the cut corners of the amber primary
    /// button (`.btn--amber`).
    pub const TOP_RIGHT_AND_BOTTOM_LEFT: Self = Self {
        top_left: false,
        top_right: true,
        bottom_right: false,
        bottom_left: true,
    };
}

/// A point in device pixels with a fractional part, for [`polygon`] and
/// [`line`](fn@line). (0, 0) is the top-left corner of pixel (0, 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// Pixels to the right of the origin.
    pub x: f32,
    /// Pixels below the origin.
    pub y: f32,
}

impl Vertex {
    /// Creates a vertex.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Fills `rect`.
pub fn fill_rect(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    rect: DeviceRect,
    color: PremultipliedColor,
) {
    fill_edges(fb, clip, Edges::of(rect), color);
}

/// Draws a 1 px line `length` pixels long from the pixel `start`,
/// rightwards or downwards. A length of zero or less draws nothing.
pub fn hairline(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    start: DevicePoint,
    length: i32,
    orientation: Orientation,
    color: PremultipliedColor,
) {
    if length <= 0 {
        return;
    }
    let (x, y, length) = (i64::from(start.x), i64::from(start.y), i64::from(length));
    let edges = match orientation {
        Orientation::Horizontal => Edges {
            left: x,
            top: y,
            right: x + length,
            bottom: y + 1,
        },
        Orientation::Vertical => Edges {
            left: x,
            top: y,
            right: x + 1,
            bottom: y + length,
        },
    };
    fill_edges(fb, clip, edges, color);
}

/// Draws a border `width` pixels wide inside `rect`, as four rects that do
/// not overlap (top and bottom full width, left and right between them), so
/// a translucent border blends each pixel once. A border as wide as half
/// the rect fills it.
pub fn border(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    rect: DeviceRect,
    width: i32,
    color: PremultipliedColor,
) {
    let outer = Edges::of(rect);
    if width <= 0 || outer.is_empty() {
        return;
    }
    let width = i64::from(width);
    if 2 * width >= outer.width() || 2 * width >= outer.height() {
        fill_edges(fb, clip, outer, color);
        return;
    }
    let Edges {
        left,
        top,
        right,
        bottom,
    } = outer;
    for part in [
        Edges {
            left,
            top,
            right,
            bottom: top + width,
        },
        Edges {
            left,
            top: bottom - width,
            right,
            bottom,
        },
        Edges {
            left,
            top: top + width,
            right: left + width,
            bottom: bottom - width,
        },
        Edges {
            left: right - width,
            top: top + width,
            right,
            bottom: bottom - width,
        },
    ] {
        fill_edges(fb, clip, part, color);
    }
}

/// Draws L-shaped brackets at the chosen corners of `rect`, inside it:
/// each a bar `arm` pixels long and `thickness` thick along each of the
/// corner's two edges, drawn as two rects that do not overlap.
///
/// jabbott.dev's focused panels use 16 px arms, 1 px thick, at
/// [`Corners::TOP_LEFT_AND_BOTTOM_RIGHT`]. An arm longer than half the
/// rect's side is shortened to half, rounded down, and the thickness to
/// the arm, so that no two brackets overlap: a rect narrower or shorter
/// than 2 px gets none.
pub fn brackets(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    rect: DeviceRect,
    arm: i32,
    thickness: i32,
    corners: Corners,
    color: PremultipliedColor,
) {
    let outer = Edges::of(rect);
    if arm <= 0 || thickness <= 0 || outer.is_empty() {
        return;
    }
    let across = i64::from(arm).min(outer.width() / 2);
    let down = i64::from(arm).min(outer.height() / 2);
    let thick = i64::from(thickness).min(across).min(down);
    if thick <= 0 {
        return;
    }
    let Edges {
        left,
        top,
        right,
        bottom,
    } = outer;
    let mut parts = Vec::with_capacity(8);
    if corners.top_left {
        parts.push(Edges {
            left,
            top,
            right: left + across,
            bottom: top + thick,
        });
        parts.push(Edges {
            left,
            top: top + thick,
            right: left + thick,
            bottom: top + down,
        });
    }
    if corners.top_right {
        parts.push(Edges {
            left: right - across,
            top,
            right,
            bottom: top + thick,
        });
        parts.push(Edges {
            left: right - thick,
            top: top + thick,
            right,
            bottom: top + down,
        });
    }
    if corners.bottom_right {
        parts.push(Edges {
            left: right - across,
            top: bottom - thick,
            right,
            bottom,
        });
        parts.push(Edges {
            left: right - thick,
            top: bottom - down,
            right,
            bottom: bottom - thick,
        });
    }
    if corners.bottom_left {
        parts.push(Edges {
            left,
            top: bottom - thick,
            right: left + across,
            bottom,
        });
        parts.push(Edges {
            left,
            top: bottom - down,
            right: left + thick,
            bottom: bottom - thick,
        });
    }
    for part in parts {
        fill_edges(fb, clip, part, color);
    }
}

/// Fills the polygon through `points`, closed from the last back to the
/// first, anti-aliased by exact area: the non-zero rule for a path that
/// does not overlap itself, and close to it for one that does (see the
/// [module documentation](self#polygons)). Fewer than three points, or
/// any NaN or infinite coordinate, draws nothing. A slanted edge is exact
/// while its ends are within about 1e11 px of the framebuffer, and may
/// come out wrong far beyond that (see
/// [Accuracy far away](self#accuracy-far-away)).
pub fn polygon(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    points: &[Vertex],
    color: PremultipliedColor,
) {
    if points.len() < 3 || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return;
    }
    let mut path = Vec::new();
    if path.try_reserve_exact(points.len()).is_err() {
        return;
    }
    path.extend(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))));
    fill_path(fb, clip, &path, color);
}

/// Draws an anti-aliased line `width` pixels wide from `from` to `to`, with
/// butt caps: a rect along the line, as a [`polygon`]. A width of zero or
/// less, a line of length zero, or a NaN or infinite value draws nothing.
/// A slanted line is exact while its ends are within about 1e11 px of the
/// framebuffer, and may come out wrong, or not at all, far beyond that
/// (see [Accuracy far away](self#accuracy-far-away)).
pub fn line(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    from: Vertex,
    to: Vertex,
    width: f32,
    color: PremultipliedColor,
) {
    let values = [from.x, from.y, to.x, to.y, width];
    if values.iter().any(|value| !value.is_finite()) || width <= 0.0 {
        return;
    }
    let (fx, fy) = (f64::from(from.x), f64::from(from.y));
    let (tx, ty) = (f64::from(to.x), f64::from(to.y));
    let (dx, dy) = (tx - fx, ty - fy);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 0.0 {
        return;
    }
    let half = f64::from(width) / 2.0;
    let (nx, ny) = (-dy / length * half, dx / length * half);
    let path = [
        (fx + nx, fy + ny),
        (tx + nx, ty + ny),
        (tx - nx, ty - ny),
        (fx - nx, fy - ny),
    ];
    fill_path(fb, clip, &path, color);
}

/// Fills `rect` with 45° cuts `cut` pixels long (along each edge) at the
/// chosen corners, anti-aliased on the diagonals, as a [`polygon`]. The
/// amber primary button cuts 10 px at
/// [`Corners::TOP_RIGHT_AND_BOTTOM_LEFT`].
///
/// A cut longer than half the shorter side is shortened to that, so cuts
/// never cross; a cut of zero or less, or NaN, fills the plain rect.
pub fn cut_corner_rect(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    rect: DeviceRect,
    cut: f32,
    corners: Corners,
    color: PremultipliedColor,
) {
    let edges = Edges::of(rect);
    if edges.is_empty() {
        return;
    }
    let most = edges.width().min(edges.height()) as f64 / 2.0;
    let cut = if cut > 0.0 {
        f64::from(cut).min(most)
    } else {
        0.0
    };
    if cut == 0.0 || corners == Corners::NONE {
        fill_edges(fb, clip, edges, color);
        return;
    }
    let (l, t) = (edges.left as f64, edges.top as f64);
    let (r, b) = (edges.right as f64, edges.bottom as f64);
    let mut path = [(0.0, 0.0); 8];
    let mut len = 0;
    let mut push = |point: (f64, f64)| {
        if let Some(slot) = path.get_mut(len) {
            *slot = point;
            len += 1;
        }
    };
    // Clockwise from the top-left corner.
    if corners.top_left {
        push((l, t + cut));
        push((l + cut, t));
    } else {
        push((l, t));
    }
    if corners.top_right {
        push((r - cut, t));
        push((r, t + cut));
    } else {
        push((r, t));
    }
    if corners.bottom_right {
        push((r, b - cut));
        push((r - cut, b));
    } else {
        push((r, b));
    }
    if corners.bottom_left {
        push((l + cut, b));
        push((l, b - cut));
    } else {
        push((l, b));
    }
    fill_path(fb, clip, path.get(..len).unwrap_or(&[]), color);
}

/// Fills `rect` fully rounded: a capsule whose radius is half the shorter
/// side, so a square gives a circle (a status dot). Anti-aliased from the
/// distance `d` of each pixel's centre to the capsule's core segment, with
/// coverage `clamp(radius − d + 0.5, 0, 1)`: a 1 px ramp centred on the
/// edge.
pub fn pill(fb: &mut Framebuffer, clip: DeviceRect, rect: DeviceRect, color: PremultipliedColor) {
    let edges = Edges::of(rect);
    let window = fb.window(clip, edges);
    if window.is_empty() || color == PremultipliedColor::TRANSPARENT {
        return;
    }
    let (width, height) = (edges.width() as f64, edges.height() as f64);
    let radius = width.min(height) / 2.0;
    let (left, top) = (edges.left as f64, edges.top as f64);
    // The core segment, from (x0, y0) to (x1, y1), horizontal or vertical.
    let (x0, y0, x1, y1) = if width >= height {
        let y = top + height / 2.0;
        (left + radius, y, left + width - radius, y)
    } else {
        let x = left + width / 2.0;
        (x, top + radius, x, top + height - radius)
    };
    for y in window.top..window.bottom {
        let Some((first, span)) = fb.span_mut(clip, y, window.left, window.right) else {
            continue;
        };
        let cy = y as f64 + 0.5;
        let dy = outside(cy, y0, y1);
        for (column, pixel) in (first..).zip(span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut()) {
            let cx = column as f64 + 0.5;
            let dx = outside(cx, x0, x1);
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = to_coverage(radius - distance + 0.5);
            blend_covered(pixel, color, coverage);
        }
    }
}

/// How far `value` is outside `[low, high]`: 0 inside.
fn outside(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low - value
    } else if value > high {
        value - high
    } else {
        0.0
    }
}

/// A coverage from 0 to 1 as a byte, `floor(coverage × 255 + 0.5)`; values
/// outside the range, and NaN, are clamped.
pub(crate) fn to_coverage(coverage: f64) -> u8 {
    if coverage >= 1.0 {
        u8::MAX
    } else if coverage > 0.0 {
        // Below 255.5, so the cast never saturates.
        (coverage * 255.0 + 0.5).floor() as u8
    } else {
        0
    }
}

/// Composites `color` at `coverage` over `pixel`.
fn blend_covered(pixel: &mut [u8], color: PremultipliedColor, coverage: u8) {
    match coverage {
        0 => {}
        u8::MAX => blend_pixel(pixel, color),
        partial => blend_pixel(pixel, scale(color, partial)),
    }
}

/// Fills the pixels inside `area`.
fn fill_edges(fb: &mut Framebuffer, clip: DeviceRect, area: Edges, color: PremultipliedColor) {
    let window = fb.window(clip, area);
    if window.is_empty() || color == PremultipliedColor::TRANSPARENT {
        return;
    }
    for y in window.top..window.bottom {
        if let Some((_, span)) = fb.span_mut(clip, y, window.left, window.right) {
            fill_span(span, color);
        }
    }
}

/// How many rows [`fill_path`] accumulates at a time: at most
/// `(16,384 + 2) × 16` cells of 8 bytes, 2 MiB.
const BAND: i64 = 16;

/// A part of a polygon's edge inside the window's columns, in coordinates
/// relative to the window's top-left corner, from top to bottom.
#[derive(Clone, Copy, Debug)]
struct Piece {
    /// The top end.
    x_top: f64,
    y_top: f64,
    /// The bottom end.
    x_bottom: f64,
    y_bottom: f64,
    /// 1 if the edge runs down the window, −1 if up.
    winding: f64,
}

impl Piece {
    /// A piece from `(xa, ya)` to `(xb, yb)` in the edge's own direction,
    /// or `None` if it is horizontal.
    fn new(xa: f64, ya: f64, xb: f64, yb: f64) -> Option<Self> {
        if ya < yb {
            Some(Self {
                x_top: xa,
                y_top: ya,
                x_bottom: xb,
                y_bottom: yb,
                winding: 1.0,
            })
        } else if yb < ya {
            Some(Self {
                x_top: xb,
                y_top: yb,
                x_bottom: xa,
                y_bottom: ya,
                winding: -1.0,
            })
        } else {
            None
        }
    }

    /// The piece's x at `y`, which is between its ends; kept between the
    /// ends' x, so float error cannot move it outside the window.
    fn x_at(&self, y: f64) -> f64 {
        let (low, high) = if self.x_top < self.x_bottom {
            (self.x_top, self.x_bottom)
        } else {
            (self.x_bottom, self.x_top)
        };
        let x = if y <= self.y_top {
            self.x_top
        } else if y >= self.y_bottom {
            self.x_bottom
        } else {
            let t = (y - self.y_top) / (self.y_bottom - self.y_top);
            self.x_top + (self.x_bottom - self.x_top) * t
        };
        x.clamp(low, high)
    }
}

/// Fills the closed path through `points` (device pixels), as [`polygon`]
/// describes. Every coordinate is finite.
fn fill_path(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    points: &[(f64, f64)],
    color: PremultipliedColor,
) {
    if points.len() < 3 || color == PremultipliedColor::TRANSPARENT {
        return;
    }
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(x, y) in points {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    // Float-to-integer casts saturate, so a coordinate far outside the
    // `i64` range still gives a bounding box that clips correctly.
    let bbox = Edges {
        left: min_x.floor() as i64,
        top: min_y.floor() as i64,
        right: max_x.ceil() as i64,
        bottom: max_y.ceil() as i64,
    };
    let window = fb.window(clip, bbox);
    if window.is_empty() {
        return;
    }
    let (origin_x, origin_y) = (window.left as f64, window.top as f64);
    let width = window.width() as f64;
    let mut pieces = Vec::new();
    if pieces.try_reserve(points.len().saturating_mul(3)).is_err() {
        return;
    }
    let Some(&first) = points.first() else {
        return;
    };
    for (index, &(xa, ya)) in points.iter().enumerate() {
        let (xb, yb) = points.get(index + 1).copied().unwrap_or(first);
        split_at_sides(
            (xa - origin_x, ya - origin_y),
            (xb - origin_x, yb - origin_y),
            width,
            &mut pieces,
        );
    }

    // One row of cells is the window's width plus two: an edge on the
    // right side adds to the cell just past it, and the one after that
    // keeps the indexing simple. Those cells are never shown.
    let Ok(stride) = usize::try_from(window.width() + 2) else {
        return;
    };
    let band_rows = window.height().min(BAND);
    let Ok(cells_len) = usize::try_from(band_rows) else {
        return;
    };
    let Ok(mut cells) = zeroed::<f64>(stride.saturating_mul(cells_len)) else {
        return;
    };
    let rows = window.height();
    let mut band_top = 0;
    while band_top < rows {
        let band_bottom = (band_top + BAND).min(rows);
        cells.fill(0.0);
        for piece in &pieces {
            accumulate_piece(&mut cells, stride, band_top, band_bottom, piece);
        }
        for (row, cells_row) in (band_top..band_bottom).zip(cells.chunks_exact(stride)) {
            let y = window.top + row;
            let Some((first, span)) = fb.span_mut(clip, y, window.left, window.right) else {
                continue;
            };
            // `first` is the window's left side, as the window lies inside
            // the clip and the bounds; the offset is there for safety.
            let skip = usize::try_from(first - window.left).unwrap_or(0);
            let mut sum = 0.0;
            for (index, &cell) in cells_row.iter().enumerate().take(stride - 2) {
                sum += cell;
                if index < skip {
                    continue;
                }
                let start = (index - skip) * BYTES_PER_PIXEL;
                if let Some(pixel) = span.get_mut(start..start + BYTES_PER_PIXEL) {
                    blend_covered(pixel, color, to_coverage(sum.abs()));
                }
            }
        }
        band_top = band_bottom;
    }
}

/// Splits the edge from `a` to `b` (window coordinates) at the window's
/// left side, x = 0, and right side, x = `width`, and pushes its pieces: a
/// part left of the window moved onto x = 0, a part inside as it is, and
/// nothing for a part right of it.
fn split_at_sides(a: (f64, f64), b: (f64, f64), width: f64, pieces: &mut Vec<Piece>) {
    let ((xa, ya), (xb, yb)) = (a, b);
    if ya == yb {
        // A horizontal edge adds no area.
        return;
    }
    // The parameters at which the edge crosses a side, in order.
    let mut cuts = [0.0, 1.0, 1.0, 1.0];
    let mut count = 1;
    if xa != xb {
        let mut crossings = [f64::NAN; 2];
        for (slot, side) in crossings.iter_mut().zip([0.0, width]) {
            let t = (side - xa) / (xb - xa);
            if t > 0.0 && t < 1.0 {
                *slot = t;
            }
        }
        if crossings[0] > crossings[1] {
            crossings.swap(0, 1);
        }
        for t in crossings {
            if !t.is_nan()
                && let Some(slot) = cuts.get_mut(count)
            {
                *slot = t;
                count += 1;
            }
        }
    }
    if let Some(slot) = cuts.get_mut(count) {
        *slot = 1.0;
    }
    let point = |t: f64| -> (f64, f64) {
        if t <= 0.0 {
            (xa, ya)
        } else if t >= 1.0 {
            (xb, yb)
        } else {
            (xa + (xb - xa) * t, ya + (yb - ya) * t)
        }
    };
    for pair in cuts.get(..=count).unwrap_or(&[]).windows(2) {
        let &[t0, t1] = pair else { continue };
        let (x0, y0) = point(t0);
        let (x1, y1) = point(t1);
        let middle = (x0 + x1) / 2.0;
        let piece = if middle < 0.0 {
            Piece::new(0.0, y0, 0.0, y1)
        } else if middle > width {
            None
        } else {
            Piece::new(x0.clamp(0.0, width), y0, x1.clamp(0.0, width), y1)
        };
        if let Some(piece) = piece {
            pieces.push(piece);
        }
    }
}

/// Adds `piece`'s area to the cells of rows `band_top..band_bottom`
/// (window rows), which `cells` holds, `stride` cells to a row.
fn accumulate_piece(
    cells: &mut [f64],
    stride: usize,
    band_top: i64,
    band_bottom: i64,
    piece: &Piece,
) {
    // Saturating casts: a piece far above or below gives an empty range.
    let first = (piece.y_top.floor() as i64).max(band_top);
    let last = (piece.y_bottom.ceil() as i64).min(band_bottom);
    for row in first..last {
        let top = (row as f64).max(piece.y_top);
        let bottom = ((row + 1) as f64).min(piece.y_bottom);
        if bottom <= top {
            continue;
        }
        let Ok(offset) = usize::try_from(row - band_top) else {
            continue;
        };
        let start = offset * stride;
        let Some(cells_row) = cells.get_mut(start..start + stride) else {
            continue;
        };
        accumulate_row(
            cells_row,
            piece.x_at(top),
            piece.x_at(bottom),
            (bottom - top) * piece.winding,
        );
    }
}

/// Adds to one row's cells the area of an edge crossing it from x `xa`
/// (where it enters) to `xb` (where it leaves), over a signed height `d`:
/// the differences of each cell's coverage, which a prefix sum turns back
/// into coverage.
fn accumulate_row(cells: &mut [f64], xa: f64, xb: f64, d: f64) {
    let mut add = |index: i64, value: f64| {
        if let Some(cell) = usize::try_from(index).ok().and_then(|i| cells.get_mut(i)) {
            *cell += value;
        }
    };
    let (x0, x1) = if xa < xb { (xa, xb) } else { (xb, xa) };
    let x0_floor = x0.floor();
    let x0_cell = x0_floor as i64;
    let x1_ceil = x1.ceil();
    let x1_cell = x1_ceil as i64;
    if x1_cell <= x0_cell + 1 {
        // Within one cell: it covers the part right of the edge's middle.
        let middle = 0.5 * (xa + xb) - x0_floor;
        add(x0_cell, d - d * middle);
        add(x0_cell + 1, d * middle);
        return;
    }
    // Across several cells: the area right of the edge grows as a
    // quadratic in the first and last cells and linearly between.
    let slope = 1.0 / (x1 - x0);
    let x0_fraction = x0 - x0_floor;
    let first = 0.5 * slope * (1.0 - x0_fraction) * (1.0 - x0_fraction);
    let x1_fraction = x1 - x1_ceil + 1.0;
    let last = 0.5 * slope * x1_fraction * x1_fraction;
    add(x0_cell, d * first);
    if x1_cell == x0_cell + 2 {
        add(x0_cell + 1, d * (1.0 - first - last));
    } else {
        let second = slope * (1.5 - x0_fraction);
        add(x0_cell + 1, d * (second - first));
        for cell in x0_cell + 2..x1_cell - 1 {
            add(cell, d * slope);
        }
        let before_last = second + (x1_cell - x0_cell - 3) as f64 * slope;
        add(x1_cell - 1, d * (1.0 - before_last - last));
    }
    add(x1_cell, d * last);
}
