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

//! Exact pixels for every primitive in `raster`.
//!
//! Anti-aliased shapes are drawn in opaque white over a transparent
//! framebuffer, so that every channel of a pixel is its coverage byte:
//! `scale(white, c)` is `(c, c, c, c)`, and over transparent it stays so.
//! The expected coverages are worked out in each test's comments.

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize};

use super::*;
use crate::clip::SPACE;

const WHITE: PremultipliedColor = PremultipliedColor {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Half-transparent white: over opaque black it gives (128, 128, 128, 255)
/// (`128 + round(0 × 127 / 255)` per channel and `128 + 127` alpha), and
/// twice it would give 128 + round(128 × 127 / 255) = 192.
const HALF_WHITE: PremultipliedColor = PremultipliedColor {
    r: 128,
    g: 128,
    b: 128,
    a: 128,
};

const BLACK: PremultipliedColor = PremultipliedColor {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

fn rect(x: i32, y: i32, width: i32, height: i32) -> DeviceRect {
    DeviceRect::new(x, y, width, height)
}

fn framebuffer(width: i32, height: i32) -> Framebuffer {
    Framebuffer::new(DeviceSize::new(width, height)).expect("small")
}

/// The red channel of every pixel, row by row.
fn red(fb: &Framebuffer) -> Vec<Vec<u8>> {
    let bounds = fb.bounds();
    (bounds.y..bounds.bottom())
        .map(|y| {
            (bounds.x..bounds.right())
                .map(|x| fb.pixel(DevicePoint::new(x, y)).map_or(0, |c| c.r))
                .collect()
        })
        .collect()
}

/// Checks that every pixel equals `on` where the picture has `#` and the
/// background elsewhere, with every channel and alpha.
fn assert_picture(
    fb: &Framebuffer,
    rows: &[&str],
    on: PremultipliedColor,
    off: PremultipliedColor,
) {
    let bounds = fb.bounds();
    for (y, row) in (bounds.y..).zip(rows) {
        for (x, mark) in (bounds.x..).zip(row.bytes()) {
            let expected = if mark == b'#' { on } else { off };
            assert_eq!(
                fb.pixel(DevicePoint::new(x, y)),
                Some(expected),
                "pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn opaque_and_translucent_fills() {
    let mut fb = framebuffer(4, 3);
    let background = PremultipliedColor {
        r: 200,
        g: 100,
        b: 50,
        a: 255,
    };
    fb.clear(background);
    let opaque = PremultipliedColor {
        r: 1,
        g: 2,
        b: 3,
        a: 255,
    };
    fill_rect(&mut fb, SPACE, rect(1, 0, 2, 2), opaque);
    assert_picture(&fb, &[".##.", ".##.", "...."], opaque, background);

    // Half-transparent blue (0, 0, 128, 128) over (200, 100, 50, 255):
    // r = round(200 × 127 / 255) = round(99.6) = 100,
    // g = round(100 × 127 / 255) = round(49.8) = 50,
    // b = 128 + round(50 × 127 / 255) = 128 + 25 = 153, a = 128 + 127.
    fb.clear(background);
    let blue = PremultipliedColor {
        r: 0,
        g: 0,
        b: 128,
        a: 128,
    };
    fill_rect(&mut fb, SPACE, rect(-5, 1, 100, 1), blue);
    let blended = PremultipliedColor {
        r: 100,
        g: 50,
        b: 153,
        a: 255,
    };
    assert_picture(&fb, &["....", "####", "...."], blended, background);
    assert_eq!(&fb.bytes()[16..20], &[153, 50, 100, 255]);
}

#[test]
fn fills_clip_and_ignore_bad_rects() {
    let mut fb = framebuffer(4, 4);
    fb.clear(BLACK);
    fill_rect(&mut fb, rect(1, 1, 2, 10), rect(0, 0, 4, 4), WHITE);
    assert_picture(&fb, &["....", ".##.", ".##.", ".##."], WHITE, BLACK);
    let before = fb.clone();
    for bad in [
        rect(0, 0, 0, 4),
        rect(0, 0, 4, -1),
        rect(4, 0, 4, 4),
        rect(i32::MIN, i32::MIN, 1, 1),
        rect(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
    ] {
        fill_rect(&mut fb, SPACE, bad, WHITE);
    }
    assert_eq!(fb, before);
    // A huge rect fills exactly the clip.
    fill_rect(
        &mut fb,
        rect(0, 0, 1, 1),
        rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
        HALF_WHITE,
    );
    assert_eq!(fb, before, "the huge rect ends at -1");
    fill_rect(
        &mut fb,
        rect(0, 0, 1, 1),
        rect(-5, -5, i32::MAX, i32::MAX),
        PremultipliedColor::TRANSPARENT,
    );
    assert_eq!(fb, before, "transparent changes nothing");
}

#[test]
fn hairlines_are_one_pixel() {
    let mut fb = framebuffer(5, 4);
    fb.clear(BLACK);
    hairline(
        &mut fb,
        SPACE,
        DevicePoint::new(1, 0),
        3,
        Orientation::Horizontal,
        WHITE,
    );
    hairline(
        &mut fb,
        SPACE,
        DevicePoint::new(4, 1),
        10,
        Orientation::Vertical,
        WHITE,
    );
    hairline(
        &mut fb,
        SPACE,
        DevicePoint::new(0, 2),
        0,
        Orientation::Horizontal,
        WHITE,
    );
    hairline(
        &mut fb,
        SPACE,
        DevicePoint::new(0, 2),
        -3,
        Orientation::Vertical,
        WHITE,
    );
    hairline(
        &mut fb,
        SPACE,
        DevicePoint::new(i32::MAX, 0),
        i32::MAX,
        Orientation::Horizontal,
        WHITE,
    );
    assert_picture(&fb, &[".###.", "....#", "....#", "....#"], WHITE, BLACK);
}

#[test]
fn a_translucent_border_blends_each_pixel_once() {
    let mut fb = framebuffer(7, 6);
    fb.clear(BLACK);
    border(&mut fb, SPACE, rect(1, 1, 5, 4), 1, HALF_WHITE);
    let once = PremultipliedColor {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    assert_picture(
        &fb,
        &[
            ".......", ".#####.", ".#...#.", ".#...#.", ".#####.", ".......",
        ],
        once,
        BLACK,
    );
    // Two pixels wide.
    fb.clear(BLACK);
    border(&mut fb, SPACE, rect(0, 0, 7, 6), 2, HALF_WHITE);
    assert_picture(
        &fb,
        &[
            "#######", "#######", "##...##", "##...##", "#######", "#######",
        ],
        once,
        BLACK,
    );
    // As wide as half the rect: it fills it, still once.
    fb.clear(BLACK);
    border(&mut fb, SPACE, rect(0, 0, 4, 6), 2, HALF_WHITE);
    assert_picture(
        &fb,
        &[
            "####...", "####...", "####...", "####...", "####...", "####...",
        ],
        once,
        BLACK,
    );
    let before = fb.clone();
    border(&mut fb, SPACE, rect(0, 0, 4, 6), 0, WHITE);
    border(&mut fb, SPACE, rect(0, 0, 0, 6), 1, WHITE);
    assert_eq!(fb, before);
}

#[test]
fn brackets_are_non_overlapping_ls() {
    let once = PremultipliedColor {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    let mut fb = framebuffer(8, 6);
    fb.clear(BLACK);
    let whole = fb.bounds();
    brackets(
        &mut fb,
        SPACE,
        whole,
        3,
        1,
        Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
        HALF_WHITE,
    );
    assert_picture(
        &fb,
        &[
            "###.....", "#.......", "#.......", ".......#", ".......#", ".....###",
        ],
        once,
        BLACK,
    );
    // The other two corners, two pixels thick.
    fb.clear(BLACK);
    let whole = fb.bounds();
    brackets(
        &mut fb,
        SPACE,
        whole,
        3,
        2,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
        HALF_WHITE,
    );
    assert_picture(
        &fb,
        &[
            ".....###", ".....###", "......##", "##......", "###.....", "###.....",
        ],
        once,
        BLACK,
    );
    // Arms are cut to half the side, so four corners never overlap.
    fb.clear(BLACK);
    brackets(
        &mut fb,
        SPACE,
        rect(0, 0, 4, 4),
        16,
        1,
        Corners::ALL,
        HALF_WHITE,
    );
    assert_picture(
        &fb,
        &[
            "####....", "#..#....", "#..#....", "####....", "........", "........",
        ],
        once,
        BLACK,
    );
    let before = fb.clone();
    brackets(&mut fb, SPACE, rect(0, 0, 1, 4), 3, 1, Corners::ALL, WHITE);
    brackets(&mut fb, SPACE, rect(0, 0, 4, 4), 0, 1, Corners::ALL, WHITE);
    brackets(&mut fb, SPACE, rect(0, 0, 4, 4), 3, -1, Corners::ALL, WHITE);
    brackets(&mut fb, SPACE, rect(0, 0, 4, 4), 3, 1, Corners::NONE, WHITE);
    assert_eq!(fb, before);
}

fn vertices(points: &[(f32, f32)]) -> Vec<Vertex> {
    points.iter().map(|&(x, y)| Vertex::new(x, y)).collect()
}

#[test]
fn an_edge_half_across_a_pixel_covers_it_half() {
    // The rect from x = 0 to 2.5: pixel 2 is half inside, coverage
    // floor(0.5 × 255 + 0.5) = 128.
    let mut fb = framebuffer(4, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.0, 0.0), (2.5, 0.0), (2.5, 1.0), (0.0, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[255, 255, 128, 0]]);
    assert_eq!(fb.pixel(DevicePoint::new(2, 0)), Some(HALF_WHITE));
    // The other way round gives the same coverage.
    let mut fb = framebuffer(4, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.0, 0.0), (0.0, 1.0), (2.5, 1.0), (2.5, 0.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[255, 255, 128, 0]]);
}

#[test]
fn integer_aligned_polygons_are_solid_inside_and_untouched_outside() {
    let mut fb = framebuffer(6, 5);
    fb.clear(BLACK);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(1.0, 1.0), (4.0, 1.0), (4.0, 3.0), (1.0, 3.0)]),
        HALF_WHITE,
    );
    let once = PremultipliedColor {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    assert_picture(
        &fb,
        &["......", ".###..", ".###..", "......", "......"],
        once,
        BLACK,
    );
    // An L-shaped hexagon.
    let mut fb = framebuffer(4, 4);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 3.0),
            (4.0, 3.0),
            (4.0, 4.0),
            (0.0, 4.0),
        ]),
        WHITE,
    );
    assert_picture(
        &fb,
        &["#...", "#...", "#...", "####"],
        WHITE,
        PremultipliedColor::TRANSPARENT,
    );
}

#[test]
fn diagonal_edges_cover_by_area() {
    // The triangle (0, 0), (2, 0), (0, 2): pixel (0, 0) lies wholly under
    // the diagonal x + y = 2, pixels (1, 0) and (0, 1) are cut through
    // their corners, half each, and pixel (1, 1) is outside.
    let mut fb = framebuffer(3, 3);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[255, 128, 0], [128, 0, 0], [0, 0, 0]]);
}

#[test]
fn a_shallow_edge_across_four_pixels_covers_each_by_area() {
    // The triangle (0, 0), (4, 0), (0, 1) is one row deep; its slanted
    // edge, x = 4(1 − y), crosses four cells in that row, which takes the
    // general branch of the accumulation (first cell, middle cells, the
    // cell before the last, the last). Above the edge the column at x is
    // 1 − x/4 tall, so pixel i covers
    // ∫ from i to i + 1 of (1 − x/4) dx = 1 − (2i + 1)/8:
    // 7/8 → floor(223.125 + 0.5) = 223, 5/8 → floor(159.375 + 0.5) = 159,
    // 3/8 → floor(95.625 + 0.5) = 96, 1/8 → floor(31.875 + 0.5) = 32.
    let mut fb = framebuffer(5, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.0, 0.0), (4.0, 0.0), (0.0, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[223, 159, 96, 32, 0]]);
}

#[test]
fn an_edge_across_two_pixels_covers_each_by_area() {
    // The triangle (0, 0), (2, 0), (0, 1): its edge x = 2(1 − y) crosses
    // exactly two cells, the branch with no middle cells. The column at x
    // is 1 − x/2 tall: pixel 0 covers ∫ from 0 to 1 = 1 − 1/4 = 3/4,
    // floor(191.25 + 0.5) = 191; pixel 1 covers ∫ from 1 to 2 = 1 − 3/4 =
    // 1/4, floor(63.75 + 0.5) = 64; pixel 2 nothing.
    let mut fb = framebuffer(3, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.0, 0.0), (2.0, 0.0), (0.0, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[191, 64, 0]]);
}

#[test]
fn an_edge_starting_and_ending_inside_pixels_covers_each_by_area() {
    // The triangle (0.5, 0), (3.5, 0), (0.5, 1): its slanted edge,
    // x = 3.5 − 3y, starts and ends half-way across a cell, so the first
    // and last cells' fractions are not 0. Between x = 0.5 and 3.5 the
    // column at x is (3.5 − x)/3 tall, and
    // ∫ (3.5 − x)/3 dx = (3.5x − x²/2)/3:
    // pixel 0, from 0.5 to 1: (3 − 1.625)/3 = 0.4583,
    //   floor(116.875 + 0.5) = 117;
    // pixel 1, from 1 to 2: (5 − 3)/3 = 2/3, floor(170 + 0.5) = 170;
    // pixel 2, from 2 to 3: (6 − 5)/3 = 1/3, floor(85 + 0.5) = 85;
    // pixel 3, from 3 to 3.5: (6.125 − 6)/3 = 0.04167,
    //   floor(10.625 + 0.5) = 11.
    let mut fb = framebuffer(5, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(0.5, 0.0), (3.5, 0.0), (0.5, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[117, 170, 85, 11, 0]]);
}

#[test]
fn winding_fills_overlaps_once_and_leaves_opposite_holes() {
    // Two clockwise squares overlapping: the overlap winds twice and is
    // drawn once, at full coverage.
    let mut fb = framebuffer(4, 1);
    let both = vertices(&[
        (0.0, 0.0),
        (3.0, 0.0),
        (3.0, 1.0),
        (0.0, 1.0),
        (0.0, 0.0),
        (1.0, 0.0),
        (4.0, 0.0),
        (4.0, 1.0),
        (1.0, 1.0),
        (1.0, 0.0),
    ]);
    fb.clear(BLACK);
    polygon(&mut fb, SPACE, &both, HALF_WHITE);
    let once = PremultipliedColor {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    assert_picture(&fb, &["####"], once, BLACK);
    // A square with a counter-clockwise square inside: a hole.
    let mut fb = framebuffer(5, 5);
    let ring = vertices(&[
        (0.0, 0.0),
        (5.0, 0.0),
        (5.0, 5.0),
        (0.0, 5.0),
        (0.0, 0.0),
        (1.0, 1.0),
        (1.0, 4.0),
        (4.0, 4.0),
        (4.0, 1.0),
        (1.0, 1.0),
    ]);
    polygon(&mut fb, SPACE, &ring, WHITE);
    assert_picture(
        &fb,
        &["#####", "#...#", "#...#", "#...#", "#####"],
        WHITE,
        PremultipliedColor::TRANSPARENT,
    );
}

#[test]
fn the_fill_rule_at_the_edges_of_overlaps() {
    // As the module documentation says. A hole cut out by an inner part
    // winding the other way is exact: [0, 4] with [1.5, 2.5] cut out
    // leaves half of pixels 1 and 2, 128 each.
    let mut fb = framebuffer(4, 1);
    let hole = vertices(&[
        (0.0, 0.0),
        (4.0, 0.0),
        (4.0, 1.0),
        (0.0, 1.0),
        (0.0, 0.0),
        (1.5, 0.0),
        (1.5, 1.0),
        (2.5, 1.0),
        (2.5, 0.0),
        (1.5, 0.0),
    ]);
    polygon(&mut fb, SPACE, &hole, WHITE);
    assert_eq!(red(&fb), [[255, 128, 128, 255]]);
    // Two parts side by side, [0, 2.5] clockwise and [2.5, 5] the other
    // way, meet inside pixel 2: the non-zero rule would fill it (half from
    // each), but their coverage cancels, leaving a seam.
    let mut fb = framebuffer(5, 1);
    let seam = vertices(&[
        (0.0, 0.0),
        (2.5, 0.0),
        (2.5, 1.0),
        (0.0, 1.0),
        (0.0, 0.0),
        (2.5, 0.0),
        (2.5, 1.0),
        (5.0, 1.0),
        (5.0, 0.0),
        (2.5, 0.0),
    ]);
    polygon(&mut fb, SPACE, &seam, WHITE);
    assert_eq!(red(&fb), [[255, 255, 0, 255, 255]]);
    // [0, 2.25] traced twice the same way: inside it is filled once, but
    // pixel 2, a quarter covered (64 by the non-zero rule), gets twice
    // that, floor(0.5 × 255 + 0.5) = 128.
    let mut fb = framebuffer(3, 1);
    let twice = vertices(&[
        (0.0, 0.0),
        (2.25, 0.0),
        (2.25, 1.0),
        (0.0, 1.0),
        (0.0, 0.0),
        (2.25, 0.0),
        (2.25, 1.0),
        (0.0, 1.0),
    ]);
    polygon(&mut fb, SPACE, &twice, WHITE);
    assert_eq!(red(&fb), [[255, 255, 128]]);
}

#[test]
fn a_polygon_left_of_the_window_still_adds_its_winding() {
    let shape = vertices(&[(-10.0, 0.0), (3.5, 0.0), (3.5, 1.0), (-10.0, 1.0)]);
    let mut fb = framebuffer(6, 1);
    polygon(&mut fb, SPACE, &shape, WHITE);
    assert_eq!(red(&fb), [[255, 255, 255, 128, 0, 0]]);
    // Clipped on the left: the part left of the clip moves onto its side.
    let mut fb = framebuffer(6, 1);
    polygon(&mut fb, rect(2, 0, 4, 1), &shape, WHITE);
    assert_eq!(red(&fb), [[0, 0, 255, 128, 0, 0]]);
    // A framebuffer whose bounds start left of the origin.
    let mut fb = Framebuffer::new_at(rect(-4, 0, 6, 1)).expect("small");
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(-10.0, 0.0), (-1.5, 0.0), (-1.5, 1.0), (-10.0, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[255, 255, 128, 0, 0, 0]]);
    // A slanted edge crossing the left side: the triangle (−2, 0), (2, 0),
    // (−2, 4) in a window from x = 0 covers, in row 0, pixel 0 wholly
    // (x + y < 2 everywhere in it), pixel 1 half, and nothing right of it.
    let mut fb = framebuffer(3, 2);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(-2.0, 0.0), (2.0, 0.0), (-2.0, 4.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[255, 128, 0], [128, 0, 0]]);
}

#[test]
fn a_polygon_right_of_the_window_adds_nothing_there() {
    let mut fb = framebuffer(4, 1);
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(1.5, 0.0), (1e30, 0.0), (1e30, 1.0), (1.5, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[0, 128, 255, 255]]);
    let mut fb = framebuffer(4, 1);
    polygon(
        &mut fb,
        rect(0, 0, 2, 1),
        &vertices(&[(1.5, 0.0), (9.0, 0.0), (9.0, 1.0), (1.5, 1.0)]),
        WHITE,
    );
    assert_eq!(red(&fb), [[0, 128, 0, 0]]);
}

/// The area of the unit square at (`x`, `y`) where `(q − p)·n + c ≥ 0`
/// for every `(n, c)` of `planes`: the square clipped by each half-plane
/// in turn (Sutherland–Hodgman), then the shoelace formula. Everything is
/// within a few pixels of `p`, so this is exact to about 1e-15.
fn exact_area(x: f64, y: f64, p: (f64, f64), planes: &[((f64, f64), f64)]) -> f64 {
    let mut shape = vec![(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
    for &(n, c) in planes {
        let side = |q: (f64, f64)| (q.0 - p.0) * n.0 + (q.1 - p.1) * n.1 + c;
        let cut = |a: (f64, f64), b: (f64, f64)| {
            let t = side(a) / (side(a) - side(b));
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let mut kept = Vec::new();
        for (index, &b) in shape.iter().enumerate() {
            let a = shape[(index + shape.len() - 1) % shape.len()];
            if side(b) >= 0.0 {
                if side(a) < 0.0 {
                    kept.push(cut(a, b));
                }
                kept.push(b);
            } else if side(a) >= 0.0 {
                kept.push(cut(a, b));
            }
        }
        shape = kept;
    }
    let twice: f64 = (0..shape.len())
        .map(|index| {
            let (a, b) = (shape[index], shape[(index + 1) % shape.len()]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    twice.abs() / 2.0
}

#[test]
fn slanted_edges_far_away_are_exact_to_the_documented_bound() {
    // The module documentation promises that with every vertex within
    // about 1e11 px of the framebuffer each coverage byte is the exact area
    // rounded to nearest. Check it for slanted edges through points inside
    // an 8 by 8 framebuffer at many slopes, with their ends 1e11 px away:
    // a half-plane (a triangle with two vertices 1e11 px along the edge
    // either way and the third 1e11 px to one side), and a 2 px line along
    // the same edge, built as `line` builds it. They go through the `f64`
    // path that `polygon` and `line` share, since `f32` vertices that far
    // out cannot hold a fraction of a pixel.
    const FAR: f64 = 1e11;
    // Directions as rationals, so the test needs no `sin` or `cos`.
    let directions = [
        (1.0, 1.0),
        (2.0, 1.0),
        (1.0, 3.0),
        (5.0, -2.0),
        (-3.0, 7.0),
        (1.0, -9.0),
        (8.0, 1.0),
        (-6.0, -5.0),
    ];
    let points = [
        (3.3, 4.7),
        (0.123, 7.77),
        (6.51, 1.49),
        (4.0, 4.0),
        (2.71, 0.5),
    ];
    for (dx, dy) in directions {
        let longest = f64::max(f64::abs(dx), f64::abs(dy));
        let d = (dx / longest, dy / longest);
        let n = (-d.1, d.0);
        let length = (n.0 * n.0 + n.1 * n.1).sqrt();
        let unit = (n.0 / length, n.1 / length);
        for p in points {
            let start = (p.0 - FAR * d.0, p.1 - FAR * d.1);
            let end = (p.0 + FAR * d.0, p.1 + FAR * d.1);
            let mut plane = framebuffer(8, 8);
            fill_path(
                &mut plane,
                SPACE,
                &[start, end, (start.0 + FAR * n.0, start.1 + FAR * n.1)],
                WHITE,
            );
            // `line`'s corners for width 2: one pixel either side.
            let (sx, sy) = (end.0 - start.0, end.1 - start.1);
            let span = (sx * sx + sy * sy).sqrt();
            let (ox, oy) = (-sy / span, sx / span);
            let mut strip = framebuffer(8, 8);
            fill_path(
                &mut strip,
                SPACE,
                &[
                    (start.0 + ox, start.1 + oy),
                    (end.0 + ox, end.1 + oy),
                    (end.0 - ox, end.1 - oy),
                    (start.0 - ox, start.1 - oy),
                ],
                WHITE,
            );
            for y in 0..8 {
                for x in 0..8 {
                    let (fx, fy) = (f64::from(x), f64::from(y));
                    let at = DevicePoint::new(x, y);
                    let half = exact_area(fx, fy, p, &[(unit, 0.0)]);
                    let band = exact_area(fx, fy, p, &[(unit, 1.0), ((-unit.0, -unit.1), 1.0)]);
                    for (fb, area, what) in [(&plane, half, "half-plane"), (&strip, band, "line")] {
                        let byte = f64::from(fb.pixel(at).map_or(0, |color| color.r));
                        // Rounded to nearest: within half a step, plus a
                        // little for an area exactly half-way.
                        assert!(
                            (byte - 255.0 * area).abs() <= 0.5 + 1e-3,
                            "{what} along {d:?} through {p:?}: {at:?} is {byte}, exact {}",
                            255.0 * area
                        );
                    }
                }
            }
        }
    }

    // Through the public functions, where `f32` vertices on the diagonal
    // stay on it however far out: the same bytes as with ends 100 px away.
    let draw = |s: f32| {
        let mut triangle = framebuffer(8, 8);
        polygon(
            &mut triangle,
            SPACE,
            &vertices(&[(-s, -s), (s, s), (-s, s)]),
            WHITE,
        );
        let mut diagonal = framebuffer(8, 8);
        line(
            &mut diagonal,
            SPACE,
            Vertex::new(-s, -s),
            Vertex::new(s, s),
            2.0,
            WHITE,
        );
        (triangle, diagonal)
    };
    let (near_triangle, near_line) = draw(100.0);
    let (far_triangle, far_line) = draw(1e11);
    assert_eq!(far_triangle, near_triangle);
    assert_eq!(far_line, near_line);
    assert!(near_line.bytes().contains(&255), "the line is drawn");

    // Past the bound a slanted edge may come out wrong, but it still never
    // writes outside the clip or panics.
    for s in [1e15_f32, 1e17, 3e38] {
        let mut fb = framebuffer(8, 8);
        fb.clear(BLACK);
        let clip = rect(2, 2, 4, 4);
        polygon(
            &mut fb,
            clip,
            &vertices(&[(-s, -s), (s, s), (-s, s)]),
            WHITE,
        );
        line(
            &mut fb,
            clip,
            Vertex::new(-s, s),
            Vertex::new(s, -s),
            2.0,
            WHITE,
        );
        for y in 0..8 {
            for x in 0..8 {
                let at = DevicePoint::new(x, y);
                if !clip.contains(at) {
                    assert_eq!(fb.pixel(at), Some(BLACK), "{at:?} at {s}");
                }
            }
        }
    }
}

#[test]
fn bad_polygons_draw_nothing() {
    let mut fb = framebuffer(4, 4);
    let before = fb.clone();
    for points in [
        vec![],
        vertices(&[(0.0, 0.0), (4.0, 4.0)]),
        vertices(&[(0.0, 0.0), (4.0, 0.0), (f32::NAN, 4.0)]),
        vertices(&[(0.0, 0.0), (f32::INFINITY, 0.0), (0.0, 4.0)]),
        vertices(&[(0.0, 0.0), (4.0, 0.0), (8.0, 0.0)]),
        vertices(&[(10.0, 10.0), (14.0, 10.0), (14.0, 14.0)]),
    ] {
        polygon(&mut fb, SPACE, &points, WHITE);
    }
    assert_eq!(fb, before);
    // Huge coordinates draw the part inside without panicking.
    polygon(
        &mut fb,
        SPACE,
        &vertices(&[(-3e38, -3e38), (3e38, -3e38), (3e38, 3e38), (-3e38, 3e38)]),
        WHITE,
    );
    assert!(fb.bytes().iter().all(|&byte| byte == 255));
}

#[test]
fn a_slanted_line_is_the_rect_along_it() {
    // From (3, 0) to (7, 3) is a 3-4-5 triangle's hypotenuse: length 5,
    // direction (0.8, 0.6), unit normal (−0.6, 0.8). Ten pixels wide, its
    // sides are 5 × (−0.6, 0.8) = (−3, 4) either side of the ends, so it is
    // exactly the polygon (0, 4), (4, 7), (10, −1), (6, −4).
    let mut drawn = framebuffer(12, 9);
    line(
        &mut drawn,
        SPACE,
        Vertex::new(3.0, 0.0),
        Vertex::new(7.0, 3.0),
        10.0,
        WHITE,
    );
    let mut expected = framebuffer(12, 9);
    polygon(
        &mut expected,
        SPACE,
        &vertices(&[(0.0, 4.0), (4.0, 7.0), (10.0, -1.0), (6.0, -4.0)]),
        WHITE,
    );
    assert_eq!(drawn, expected);

    // And it is drawn. The pixel (5, 1) lies wholly inside: relative to the
    // line's middle (5, 1.5) its corners are at most 1.1 along the line
    // (half-length 2.5) and 1.0 across it (half-width 5). The pixel (0, 0)
    // lies wholly before the start: its corners are 3.5 to 4.9 behind the
    // middle along the line. The pixel (11, 8) is past the end.
    let coverage = red(&drawn);
    assert_eq!(coverage[1][5], 255);
    assert_eq!(coverage[0][0], 0);
    assert_eq!(coverage[8][11], 0);
    // In the frame the rect is the quadrilateral (3, 0), (0, 4), (4, 7),
    // (9.25, 0) (it leaves the top edge where its sides cross y = 0), of
    // area |12 − 16 − 64.75 + 0| / 2 = 34.375 by the shoelace formula. The
    // coverage adds up to that, within the rounding of the edge pixels to
    // bytes (half of 1/255 each).
    let total: f64 = coverage
        .iter()
        .flatten()
        .map(|&byte| f64::from(byte) / 255.0)
        .sum();
    assert!((total - 34.375).abs() < 0.1, "{total}");
}

#[test]
fn lines_are_rects_along_them_with_butt_caps() {
    // Two pixels wide, centred on y = 2: rows 1 and 2, from x = 0 to 4.
    let mut fb = framebuffer(5, 4);
    line(
        &mut fb,
        SPACE,
        Vertex::new(0.0, 2.0),
        Vertex::new(4.0, 2.0),
        2.0,
        WHITE,
    );
    assert_picture(
        &fb,
        &[".....", "####.", "####.", "....."],
        WHITE,
        PremultipliedColor::TRANSPARENT,
    );
    // One pixel wide on y = 2 spans y = 1.5 to 2.5: half of rows 1 and 2.
    let mut fb = framebuffer(5, 4);
    line(
        &mut fb,
        SPACE,
        Vertex::new(4.0, 2.0),
        Vertex::new(0.0, 2.0),
        1.0,
        WHITE,
    );
    assert_eq!(
        red(&fb),
        [
            [0; 5],
            [128, 128, 128, 128, 0],
            [128, 128, 128, 128, 0],
            [0; 5]
        ]
    );
    // Vertical on x = 1.25, 1 px wide: x = 0.75 to 1.75, so a quarter of
    // column 0, floor(0.25 × 255 + 0.5) = 64, and three quarters of
    // column 1, floor(191.25 + 0.5) = 191.
    let mut fb = framebuffer(3, 2);
    line(
        &mut fb,
        SPACE,
        Vertex::new(1.25, 0.0),
        Vertex::new(1.25, 2.0),
        1.0,
        WHITE,
    );
    assert_eq!(red(&fb), [[64, 191, 0], [64, 191, 0]]);
    let before = fb.clone();
    let nan = f32::NAN;
    line(
        &mut fb,
        SPACE,
        Vertex::new(1.0, 1.0),
        Vertex::new(1.0, 1.0),
        2.0,
        WHITE,
    );
    line(
        &mut fb,
        SPACE,
        Vertex::new(0.0, 0.0),
        Vertex::new(2.0, 2.0),
        0.0,
        WHITE,
    );
    line(
        &mut fb,
        SPACE,
        Vertex::new(0.0, 0.0),
        Vertex::new(2.0, 2.0),
        nan,
        WHITE,
    );
    line(
        &mut fb,
        SPACE,
        Vertex::new(nan, 0.0),
        Vertex::new(2.0, 2.0),
        1.0,
        WHITE,
    );
    assert_eq!(fb, before);
}

#[test]
fn cut_corners_are_anti_aliased_diagonals() {
    // A 4 px square cut 2 px at the top-right and bottom-left. The cut
    // from (2, 0) to (4, 2) halves pixels (2, 0) and (3, 1) along their
    // diagonals and leaves (3, 0) outside; the cut from (2, 4) to (0, 2)
    // does the same at the bottom-left.
    let mut fb = framebuffer(4, 4);
    cut_corner_rect(
        &mut fb,
        SPACE,
        rect(0, 0, 4, 4),
        2.0,
        Corners::TOP_RIGHT_AND_BOTTOM_LEFT,
        WHITE,
    );
    assert_eq!(
        red(&fb),
        [
            [255, 255, 128, 0],
            [255, 255, 255, 128],
            [128, 255, 255, 255],
            [0, 128, 255, 255]
        ]
    );
    // All four corners, 1 px: each corner pixel is half.
    let mut fb = framebuffer(4, 3);
    cut_corner_rect(&mut fb, SPACE, rect(0, 0, 4, 3), 1.0, Corners::ALL, WHITE);
    assert_eq!(
        red(&fb),
        [
            [128, 255, 255, 128],
            [255, 255, 255, 255],
            [128, 255, 255, 128]
        ]
    );
    // A cut too long is shortened to half the shorter side, 1.5 px here.
    let mut long = framebuffer(4, 3);
    cut_corner_rect(
        &mut long,
        SPACE,
        rect(0, 0, 4, 3),
        99.0,
        Corners::ALL,
        WHITE,
    );
    let mut half = framebuffer(4, 3);
    cut_corner_rect(&mut half, SPACE, rect(0, 0, 4, 3), 1.5, Corners::ALL, WHITE);
    assert_eq!(long, half);
    // No cut, a NaN cut, or no corners: the plain rect.
    for (cut, corners) in [
        (0.0, Corners::ALL),
        (f32::NAN, Corners::ALL),
        (2.0, Corners::NONE),
    ] {
        let mut fb = framebuffer(3, 2);
        cut_corner_rect(&mut fb, SPACE, rect(0, 0, 2, 2), cut, corners, WHITE);
        assert_picture(&fb, &["##.", "##."], WHITE, PremultipliedColor::TRANSPARENT);
    }
}

#[test]
fn pills_and_circles_are_anti_aliased_by_distance() {
    // A 4 px circle: radius 2 about (2, 2). Corner pixel centres are
    // sqrt(1.5² + 1.5²) = 2.1213 away, coverage 2 − 2.1213 + 0.5 = 0.3787,
    // floor(96.57 + 0.5) = 97; edge pixels sqrt(1.5² + 0.5²) = 1.5811,
    // coverage 0.9189, floor(234.32 + 0.5) = 234; inner pixels 0.7071,
    // coverage over 1.
    let mut fb = framebuffer(4, 4);
    pill(&mut fb, SPACE, rect(0, 0, 4, 4), WHITE);
    assert_eq!(
        red(&fb),
        [
            [97, 234, 234, 97],
            [234, 255, 255, 234],
            [234, 255, 255, 234],
            [97, 234, 234, 97]
        ]
    );
    // An 8 by 4 pill: radius 2, core segment from (2, 2) to (6, 2). Pixels
    // above the segment are 1.5 or 0.5 px from it vertically, full.
    let mut fb = framebuffer(8, 4);
    pill(&mut fb, SPACE, rect(0, 0, 8, 4), WHITE);
    let ends = [
        [97, 234, 255, 255, 255, 255, 234, 97],
        [234, 255, 255, 255, 255, 255, 255, 234],
        [234, 255, 255, 255, 255, 255, 255, 234],
        [97, 234, 255, 255, 255, 255, 234, 97],
    ];
    assert_eq!(red(&fb), ends);
    // Upright, it is the same pill turned.
    let mut fb = framebuffer(4, 8);
    pill(&mut fb, SPACE, rect(0, 0, 4, 8), WHITE);
    let turned: Vec<Vec<u8>> = (0..8)
        .map(|y| (0..4).map(|x| ends[x][y]).collect())
        .collect();
    assert_eq!(red(&fb), turned);
    // Clipped to its right half.
    let mut fb = framebuffer(4, 4);
    pill(&mut fb, rect(2, 0, 2, 4), rect(0, 0, 4, 4), WHITE);
    assert_eq!(
        red(&fb),
        [
            [0, 0, 234, 97],
            [0, 0, 255, 234],
            [0, 0, 255, 234],
            [0, 0, 234, 97]
        ]
    );
    let before = fb.clone();
    pill(&mut fb, SPACE, rect(0, 0, 0, 4), WHITE);
    pill(&mut fb, SPACE, rect(0, 0, 4, -4), WHITE);
    assert_eq!(fb, before);
}

#[test]
fn coverage_bytes_round_to_nearest() {
    assert_eq!(to_coverage(0.5), 128);
    assert_eq!(to_coverage(0.25), 64);
    assert_eq!(to_coverage(1.0 - 1e-12), 255);
    assert_eq!(to_coverage(2.0), 255);
    assert_eq!(to_coverage(-1.0), 0);
    assert_eq!(to_coverage(f64::NAN), 0);
}
