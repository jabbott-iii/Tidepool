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

//! Hand-made test vectors for `outline`: exact coverage of shapes whose
//! area in each pixel is known, a disc against its area, the subpixel
//! shift, and every error.

use tidepool_font::outline::{self, Bitmap, ErrorKind, Outline, OutlinePoint};
use tidepool_font::{FontError, Limits};

fn on(x: f32, y: f32) -> OutlinePoint {
    OutlinePoint {
        x,
        y,
        on_curve: true,
    }
}

fn off(x: f32, y: f32) -> OutlinePoint {
    OutlinePoint {
        x,
        y,
        on_curve: false,
    }
}

/// An outline of `contours`.
fn outline(contours: &[Vec<OutlinePoint>]) -> Outline {
    let mut points = Vec::new();
    let mut ends = Vec::new();
    for contour in contours {
        points.extend(contour);
        ends.push(points.len());
    }
    Outline::new(points, ends).expect("a valid outline")
}

/// The rectangle from (x0, y0) to (x1, y1), clockwise (y up), as TrueType
/// winds outer contours.
fn rectangle(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<OutlinePoint> {
    vec![on(x0, y0), on(x0, y1), on(x1, y1), on(x1, y0)]
}

fn rasterize(outline: &Outline, scale: f32, shift: f32) -> Bitmap {
    outline::rasterize(outline, scale, shift, Limits::DEFAULT).expect("rasterizes")
}

/// The bitmap's rows, as written.
fn rows(bitmap: &Bitmap) -> Vec<Vec<u8>> {
    bitmap
        .pixels
        .chunks(bitmap.width as usize)
        .map(<[u8]>::to_vec)
        .collect()
}

#[test]
fn an_aligned_square_is_full_inside_and_empty_outside() {
    // A 2 × 2 square at (1, 1), and a point at (4, 4) that only widens the
    // control box: the bitmap covers x and y from 1 to 4, y up.
    let shape = outline(&[rectangle(1.0, 1.0, 3.0, 3.0), vec![on(4.0, 4.0)]]);
    let bitmap = rasterize(&shape, 1.0, 0.0);
    assert_eq!(
        (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
        (1, -4, 3, 3)
    );
    assert_eq!(rows(&bitmap), [[0, 0, 0], [255, 255, 0], [255, 255, 0]]);
    assert_eq!(bitmap.get(0, 1), Some(255));
    assert_eq!(bitmap.get(3, 0), None);
    assert_eq!(bitmap.get(0, 3), None);
}

#[test]
fn a_square_below_the_baseline_has_a_positive_top() {
    let bitmap = rasterize(&outline(&[rectangle(-2.0, -3.0, 0.0, -1.0)]), 1.0, 0.0);
    assert_eq!(
        (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
        (-2, 1, 2, 2)
    );
    assert!(bitmap.pixels.iter().all(|&pixel| pixel == 255));
}

#[test]
fn a_half_pixel_edge_is_half_covered() {
    // From x = 0.5 to 3 and y = 0 to 1.5, at scale 1: the left column and
    // the top row (y up: the row above y = 1) are half covered, their
    // corner a quarter.
    let bitmap = rasterize(&outline(&[rectangle(0.5, 0.0, 3.0, 1.5)]), 1.0, 0.0);
    assert_eq!(
        (bitmap.left, bitmap.top, bitmap.width, bitmap.height),
        (0, -2, 3, 2)
    );
    assert_eq!(rows(&bitmap), [[64, 128, 128], [128, 255, 255]]);
}

#[test]
fn a_triangle_covers_each_pixel_by_its_area() {
    // The right triangle (0, 0), (0, 4), (4, 0): each pixel on the
    // diagonal is half covered, those below it full.
    let bitmap = rasterize(
        &outline(&[vec![on(0.0, 0.0), on(0.0, 4.0), on(4.0, 0.0)]]),
        1.0,
        0.0,
    );
    assert_eq!(
        rows(&bitmap),
        [
            [128, 0, 0, 0],
            [255, 128, 0, 0],
            [255, 255, 128, 0],
            [255, 255, 255, 128]
        ]
    );
    // A slope of 1/2 from (0, 0) to (4, 2), closed along x = 4 and y = 0:
    // each pixel's area under y = x / 2 is 1/4 or 3/4 where the slope
    // crosses it.
    let bitmap = rasterize(
        &outline(&[vec![on(0.0, 0.0), on(4.0, 2.0), on(4.0, 0.0)]]),
        1.0,
        0.0,
    );
    assert_eq!(rows(&bitmap), [[0, 0, 64, 191], [64, 191, 255, 255]]);
}

#[test]
fn a_hole_winding_the_other_way_is_empty() {
    let outer = rectangle(0.0, 0.0, 6.0, 6.0);
    let mut inner = rectangle(2.0, 2.0, 4.0, 4.0);
    inner.reverse();
    let bitmap = rasterize(&outline(&[outer, inner]), 1.0, 0.0);
    for (row, pixels) in rows(&bitmap).iter().enumerate() {
        for (column, &pixel) in pixels.iter().enumerate() {
            let hole = (2..4).contains(&row) && (2..4).contains(&column);
            assert_eq!(pixel, if hole { 0 } else { 255 }, "({column}, {row})");
        }
    }
    // Winding the same way, it is not a hole: coverage is summed and
    // clamped, so the overlap is full.
    let same = outline(&[rectangle(0.0, 0.0, 6.0, 6.0), rectangle(2.0, 2.0, 4.0, 4.0)]);
    assert!(
        rasterize(&same, 1.0, 0.0)
            .pixels
            .iter()
            .all(|&pixel| pixel == 255)
    );
}

/// A disc of radius `r` about (`r`, `r`): eight quadratic arcs, each from
/// one point of a regular octagon on the circle to the next, through the
/// control point where the circle's tangents there meet.
fn disc(r: f32) -> Outline {
    // cos and sin of multiples of 45°, and the control points' distance,
    // r / cos(22.5°); written out so that no libm function is used.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let directions = [
        (1.0, 0.0),
        (half, -half),
        (0.0, -1.0),
        (-half, -half),
        (-1.0, 0.0),
        (-half, half),
        (0.0, 1.0),
        (half, half),
    ];
    let reach = 1.082_392_2; // 1 / cos(22.5°)
    let mut points = Vec::new();
    for (index, &(cx, cy)) in directions.iter().enumerate() {
        let (nx, ny) = directions[(index + 1) % 8];
        points.push(on(r + r * cx, r + r * cy));
        let (mx, my) = ((cx + nx) * 0.5, (cy + ny) * 0.5);
        let length = (mx * mx + my * my).sqrt();
        points.push(off(
            r + r * reach * mx / length,
            r + r * reach * my / length,
        ));
    }
    outline(&[points])
}

/// The exact area of a closed chain of quadratic arcs, by Green's theorem:
/// each arc from `p0` through `p1` to `p2` adds
/// `(2·(p0 × p1) + 2·(p1 × p2) + p0 × p2) / 6`.
fn quadratic_area(shape: &Outline) -> f64 {
    let points = shape.points();
    let mut area = 0.0;
    for pair in 0..points.len() / 2 {
        let p0 = points[pair * 2];
        let p1 = points[pair * 2 + 1];
        let p2 = points[(pair * 2 + 2) % points.len()];
        let cross = |a: OutlinePoint, b: OutlinePoint| {
            f64::from(a.x) * f64::from(b.y) - f64::from(a.y) * f64::from(b.x)
        };
        area += (2.0 * cross(p0, p1) + 2.0 * cross(p1, p2) + cross(p0, p2)) / 6.0;
    }
    area.abs()
}

#[test]
fn a_disc_covers_its_area() {
    for (radius, scale) in [(10.0, 1.0), (500.0, 0.04), (300.0, 1.0)] {
        let shape = disc(radius);
        let bitmap = rasterize(&shape, scale, 0.0);
        let covered: f64 = bitmap
            .pixels
            .iter()
            .map(|&pixel| f64::from(pixel) / 255.0)
            .sum();
        let area = quadratic_area(&shape) * f64::from(scale) * f64::from(scale);
        // The lines are within `TOLERANCE` of the curves, so they lose at
        // most that times the perimeter (`2πr` bounds it); each byte is
        // within half a step of its pixel's exact area, and about two
        // pixels a unit of perimeter are partly covered.
        let perimeter = 2.0 * std::f64::consts::PI * f64::from(radius * scale) * 1.01;
        let bound = perimeter * (outline::TOLERANCE + 1.0 / 255.0);
        let error = (covered - area).abs();
        assert!(
            error <= bound,
            "radius {radius} at {scale}: {covered} against {area}, off by {error} > {bound}"
        );
    }
}

#[test]
fn the_shift_moves_coverage_right() {
    let square = outline(&[rectangle(0.0, 0.0, 4.0, 1.0)]);
    let shifted = rasterize(&square, 1.0, 0.25);
    assert_eq!((shifted.left, shifted.width), (0, 5));
    assert_eq!(shifted.pixels, [191, 255, 255, 255, 64]);
    // A whole pixel moves the bitmap, not the coverage.
    let moved = rasterize(&square, 1.0, 1.0);
    let still = rasterize(&square, 1.0, 0.0);
    assert_eq!((moved.left, still.left), (1, 0));
    assert_eq!(moved.pixels, still.pixels);
    // Negative shifts too.
    let back = rasterize(&square, 1.0, -0.5);
    assert_eq!(
        (back.left, back.pixels.as_slice()),
        (-1, &[128, 255, 255, 255, 128][..])
    );
}

#[test]
fn rasterizing_twice_gives_the_same_bytes() {
    let shape = disc(37.0);
    let first = rasterize(&shape, 0.61, 0.75);
    assert_eq!(rasterize(&shape, 0.61, 0.75), first);
    assert!(first.pixels.iter().any(|&pixel| pixel > 0 && pixel < 255));
}

#[test]
fn an_empty_outline_is_an_empty_bitmap() {
    let empty = Outline::new(Vec::new(), Vec::new()).expect("empty");
    assert!(empty.is_empty());
    assert_eq!(empty, Outline::default());
    assert_eq!(rasterize(&empty, 1.0, 0.25), Bitmap::default());
    // A shape with no width has no columns.
    let line = outline(&[vec![on(2.0, 0.0), on(2.0, 5.0)]]);
    let bitmap = rasterize(&line, 1.0, 0.0);
    assert_eq!(
        (bitmap.width, bitmap.height, bitmap.pixels.len()),
        (0, 5, 0)
    );
}

fn fails(result: Result<Bitmap, FontError>, kind: ErrorKind) {
    assert_eq!(result, Err(FontError::Outline { kind }));
}

#[test]
fn the_scale_and_shift_must_be_finite() {
    let square = outline(&[rectangle(0.0, 0.0, 1.0, 1.0)]);
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, -0.0] {
        fails(
            outline::rasterize(&square, scale, 0.0, Limits::DEFAULT),
            ErrorKind::BadScale,
        );
    }
    for shift in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        fails(
            outline::rasterize(&square, 1.0, shift, Limits::DEFAULT),
            ErrorKind::BadShift,
        );
    }
    // Checked even for an empty outline.
    fails(
        outline::rasterize(&Outline::default(), 0.0, 0.0, Limits::DEFAULT),
        ErrorKind::BadScale,
    );
}

#[test]
fn bitmaps_are_bounded_before_they_are_made() {
    let wide = outline(&[rectangle(0.0, 0.0, 64.0, 1.0)]);
    let limits = |side| Limits {
        max_bitmap_dimension: side,
        ..Limits::DEFAULT
    };
    assert_eq!(
        outline::rasterize(&wide, 1.0, 0.0, limits(64)).map(|bitmap| bitmap.width),
        Ok(64)
    );
    fails(
        outline::rasterize(&wide, 1.0, 0.0, limits(63)),
        ErrorKind::BitmapTooLarge { limit: 63 },
    );
    // A shift that crosses a pixel widens it.
    fails(
        outline::rasterize(&wide, 1.0, 0.5, limits(64)),
        ErrorKind::BitmapTooLarge { limit: 64 },
    );
    let tall = outline(&[rectangle(0.0, 0.0, 1.0, 100.0)]);
    fails(
        outline::rasterize(&tall, 1.0, 0.0, limits(99)),
        ErrorKind::BitmapTooLarge { limit: 99 },
    );
    // Small, but beyond an `i32` to the right, or below.
    let right = outline(&[rectangle(3e9, 0.0, 3e9 + 1.0, 1.0)]);
    fails(
        outline::rasterize(&right, 1.0, 0.0, Limits::DEFAULT),
        ErrorKind::BitmapOutOfRange,
    );
    let below = outline(&[rectangle(0.0, -1e10, 1.0, -1e10)]);
    fails(
        outline::rasterize(&below, 1.0, 0.0, Limits::DEFAULT),
        ErrorKind::BitmapOutOfRange,
    );
    // The largest scale and coordinates `f32` has stay finite in `f64`.
    let huge = outline(&[rectangle(0.0, 0.0, f32::MAX, 1.0)]);
    fails(
        outline::rasterize(&huge, f32::MAX, 0.0, Limits::DEFAULT),
        ErrorKind::BitmapTooLarge {
            limit: Limits::DEFAULT.max_bitmap_dimension,
        },
    );
}

#[test]
fn the_work_is_bounded_before_the_bitmap_is_made() {
    // A 10 × 5 rectangle at scale 1: its edges' lengths add up to 30, four
    // edges add 4 × EDGE_WORK, and its one band reads 4 points.
    let rectangle = outline(&[rectangle(0.0, 0.0, 10.0, 5.0)]);
    let work = 30 + 4 * outline::EDGE_WORK + 4;
    assert_eq!(work, 50);
    let limits = |max_raster_work| Limits {
        max_raster_work,
        ..Limits::DEFAULT
    };
    assert!(outline::rasterize(&rectangle, 1.0, 0.0, limits(work)).is_ok());
    fails(
        outline::rasterize(&rectangle, 1.0, 0.0, limits(work - 1)),
        ErrorKind::TooMuchWork { limit: work - 1 },
    );
    // An edge's length is rounded up: at scale 1.05 the rectangle's sides
    // are 10.5 and 5.25 pixels, 11 and 6 each.
    assert!(outline::rasterize(&rectangle, 1.05, 0.0, limits(2 * 11 + 2 * 6 + 16 + 4)).is_ok());
    fails(
        outline::rasterize(&rectangle, 1.05, 0.0, limits(2 * 11 + 2 * 6 + 16 + 3)),
        ErrorKind::TooMuchWork {
            limit: 2 * 11 + 2 * 6 + 16 + 3,
        },
    );
    // An empty bitmap takes no work.
    let line = outline(&[vec![on(2.0, 0.0), on(2.0, 5.0)]]);
    assert!(outline::rasterize(&line, 1.0, 0.0, limits(0)).is_ok());
    // The worst outline the other limits allow, 65,536 points zigzagging
    // across 4,095 pixels, took seconds; now it is refused at once.
    let zigzag: Vec<OutlinePoint> = (0..65_536)
        .map(|index| {
            on(
                index as f32 / 16.0,
                if index % 2 == 0 { 0.0 } else { 4095.0 },
            )
        })
        .collect();
    let hostile = Outline::new(zigzag, vec![65_536]).expect("valid");
    let started = std::time::Instant::now();
    fails(
        outline::rasterize(&hostile, 1.0, 0.0, Limits::DEFAULT),
        ErrorKind::TooMuchWork {
            limit: Limits::DEFAULT.max_raster_work,
        },
    );
    assert!(started.elapsed().as_secs() < 2);
}

#[test]
fn where_the_coverage_differs_from_the_non_zero_rule() {
    // These pin what the module docs say about it. A bow-tie's lobes wind
    // opposite ways and cancel in the pixel where they cross, which the
    // non-zero rule covers by half.
    let bow_tie = outline(&[vec![on(0.0, 0.0), on(3.0, 3.0), on(3.0, 0.0), on(0.0, 3.0)]]);
    assert_eq!(rows(&rasterize(&bow_tie, 1.0, 0.0))[1], [255, 0, 255]);
    // A rectangle and its mirror image side by side in one pixel, winding
    // opposite ways without overlapping: they cancel, where the non-zero
    // rule gives 0.8.
    let mut mirrored = rectangle(0.6, 0.0, 1.0, 1.0);
    mirrored.reverse();
    let pair = outline(&[rectangle(0.0, 0.0, 0.4, 1.0), mirrored]);
    assert_eq!(rasterize(&pair, 1.0, 0.0).pixels, [0]);
    // A contour winding twice round a square counts its edge pixels twice:
    // the half-covered columns come out full.
    let mut twice = rectangle(0.0, 0.0, 3.0, 3.0);
    twice.extend(rectangle(0.0, 0.0, 3.0, 3.0));
    let doubled = outline(&[twice]);
    assert!(
        rasterize(&doubled, 1.0, 0.5)
            .pixels
            .iter()
            .all(|&pixel| pixel == 255)
    );
}

#[test]
fn outlines_are_checked() {
    let points = rectangle(0.0, 0.0, 1.0, 1.0);
    let bad = |ends: Vec<usize>| Outline::new(points.clone(), ends);
    let kind = Err(FontError::Outline {
        kind: ErrorKind::BadContourEnds,
    });
    assert_eq!(bad(vec![]), kind);
    assert_eq!(bad(vec![0, 4]), kind);
    assert_eq!(bad(vec![2, 2, 4]), kind);
    assert_eq!(bad(vec![3, 2, 4]), kind);
    assert_eq!(bad(vec![3]), kind);
    assert_eq!(bad(vec![5]), kind);
    assert_eq!(Outline::new(Vec::new(), vec![0]), kind);
    let ok = bad(vec![1, 4]).expect("two contours");
    let contours: Vec<usize> = ok.contours().map(<[OutlinePoint]>::len).collect();
    assert_eq!(contours, [1, 3]);
    let mut nan = points.clone();
    nan[2].y = f32::NAN;
    assert_eq!(
        Outline::new(nan, vec![4]),
        Err(FontError::Outline {
            kind: ErrorKind::NonFinitePoint { index: 2 }
        })
    );
    let mut infinite = points;
    infinite[1].x = f32::NEG_INFINITY;
    assert_eq!(
        Outline::new(infinite, vec![4]),
        Err(FontError::Outline {
            kind: ErrorKind::NonFinitePoint { index: 1 }
        })
    );
}

#[test]
fn curves_and_implied_points_are_drawn() {
    // A "square" of four off-curve points: four arcs through the implied
    // midpoints of its sides, a rounded diamond inside it.
    let round = outline(&[vec![
        off(0.0, 0.0),
        off(0.0, 8.0),
        off(8.0, 8.0),
        off(8.0, 0.0),
    ]]);
    let bitmap = rasterize(&round, 1.0, 0.0);
    assert_eq!((bitmap.width, bitmap.height), (8, 8));
    // Its middle is covered, its corners are not.
    assert_eq!(bitmap.get(3, 3), Some(255));
    assert_eq!(bitmap.get(0, 0), Some(0));
    assert_eq!(bitmap.get(7, 7), Some(0));
    // The same shape from points on the curve at the midpoints, and the
    // corners as control points, starting on the curve where the other
    // starts (between its last point and its first), so that the arcs come
    // in the same order.
    let same = outline(&[vec![
        on(4.0, 0.0),
        off(0.0, 0.0),
        on(0.0, 4.0),
        off(0.0, 8.0),
        on(4.0, 8.0),
        off(8.0, 8.0),
        on(8.0, 4.0),
        off(8.0, 0.0),
    ]]);
    assert_eq!(rasterize(&same, 1.0, 0.0).pixels, bitmap.pixels);
}

#[test]
fn errors_display_what_is_wrong() {
    let error = FontError::Outline {
        kind: ErrorKind::NonFinitePoint { index: 3 },
    };
    assert_eq!(
        error.to_string(),
        "outline error: point 3 has a coordinate that is not finite"
    );
    assert_eq!(
        format!("{:?}", Bitmap::default()),
        "Bitmap { left: 0, top: 0, width: 0, height: 0, pixels: 0 }"
    );
}
