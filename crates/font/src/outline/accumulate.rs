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

//! Exact-area coverage accumulation: the same algorithm as
//! `tidepool_gfx::raster`'s polygon fill (`split_at_sides`,
//! `accumulate_piece`, `accumulate_row` and the coverage byte), ported
//! here because `tidepool-font` may depend only on `tidepool-core`. Keep
//! the two in step: a change to one is a change to the other.
//!
//! Each edge adds, to each cell of each row it crosses, the signed area it
//! contributes; a prefix sum along the row turns that into coverage. Edges
//! are in bitmap coordinates: x from the bitmap's left edge, y from its
//! top, in pixels.

/// A part of an edge inside the bitmap's columns, from top to bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Piece {
    /// The top end.
    x_top: f64,
    y_top: f64,
    /// The bottom end.
    x_bottom: f64,
    y_bottom: f64,
    /// 1 if the edge runs down, −1 if up.
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
    /// ends' x, so float error cannot move it outside the bitmap.
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

/// Splits the edge from `a` to `b` at the bitmap's left side, x = 0, and
/// right side, x = `width`, and hands each piece to `piece`: a part left of
/// the bitmap moved onto x = 0 (it still adds its winding), a part inside
/// as it is, and nothing for a part right of it (it adds nothing visible).
pub(super) fn split_at_sides(
    a: (f64, f64),
    b: (f64, f64),
    width: f64,
    mut piece: impl FnMut(&Piece),
) {
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
        let part = if middle < 0.0 {
            Piece::new(0.0, y0, 0.0, y1)
        } else if middle > width {
            None
        } else {
            Piece::new(x0.clamp(0.0, width), y0, x1.clamp(0.0, width), y1)
        };
        if let Some(part) = part {
            piece(&part);
        }
    }
}

/// Adds `piece`'s area to the cells of rows `band_top..band_bottom`, which
/// `cells` holds, `stride` cells to a row.
pub(super) fn accumulate_piece(
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

/// A coverage from 0 to 1 as a byte, `floor(coverage × 255 + 0.5)`; values
/// outside the range, and NaN, are clamped.
pub(super) fn to_coverage(coverage: f64) -> u8 {
    if coverage >= 1.0 {
        u8::MAX
    } else if coverage > 0.0 {
        // Below 255.5, so the cast never saturates.
        (coverage * 255.0 + 0.5).floor() as u8
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An edge from one point to another.
    type Edge = ((f64, f64), (f64, f64));

    /// The coverage of each cell of a one-row, `width`-cell bitmap after
    /// the edges `edges`.
    fn row(width: usize, edges: &[Edge]) -> Vec<f64> {
        let stride = width + 2;
        let mut cells = vec![0.0; stride];
        for &(a, b) in edges {
            split_at_sides(a, b, width as f64, |piece| {
                accumulate_piece(&mut cells, stride, 0, 1, piece);
            });
        }
        let mut sum = 0.0;
        cells
            .iter()
            .take(width)
            .map(|cell| {
                sum += cell;
                f64::abs(sum)
            })
            .collect()
    }

    #[test]
    fn a_vertical_edge_covers_what_is_right_of_it() {
        // Down at x = 1.25, back up at x = 3: cells 1 (three quarters) and 2.
        let coverage = row(4, &[((1.25, 0.0), (1.25, 1.0)), ((3.0, 1.0), (3.0, 0.0))]);
        assert_eq!(coverage, [0.0, 0.75, 1.0, 0.0]);
    }

    #[test]
    fn a_slanted_edge_covers_its_trapezoids() {
        // From (0, 0) down to (2, 1), closed by the right side: the
        // triangle right of the slope covers a quarter of the first cell
        // and three quarters of the second.
        let coverage = row(2, &[((0.0, 0.0), (2.0, 1.0)), ((2.0, 1.0), (2.0, 0.0))]);
        assert_eq!(coverage, [0.25, 0.75]);
    }

    #[test]
    fn edges_left_of_the_bitmap_keep_their_winding_and_right_of_it_add_nothing() {
        let coverage = row(2, &[((-5.0, 0.0), (-5.0, 1.0)), ((9.0, 1.0), (9.0, 0.0))]);
        assert_eq!(coverage, [1.0, 1.0]);
    }

    #[test]
    fn coverage_becomes_a_byte_rounded_to_nearest() {
        assert_eq!(to_coverage(0.0), 0);
        assert_eq!(to_coverage(-0.5), 0);
        assert_eq!(to_coverage(f64::NAN), 0);
        assert_eq!(to_coverage(0.5), 128);
        assert_eq!(to_coverage(1.0 / 255.0), 1);
        assert_eq!(to_coverage(0.999), 255);
        assert_eq!(to_coverage(2.0), 255);
    }
}
