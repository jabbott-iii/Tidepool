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

//! Unit tests of the drawings: the box and the view box's placing, the
//! mask, exact coverage of strokes, joins, caps, rings, rounded rects and
//! gradients, the union within a shape and the compositing between shapes,
//! curve flattening, the site's drawings drawn still, and every pixel's
//! alpha the same by band, alone and by testing every shape.

use super::super::jabbott_dev;
use super::*;

/// A drawing on the left whose view box is 400 × 1000, as the site's
/// circuits, holding `shapes`.
fn left(shapes: Vec<Shape>) -> Drawing {
    Drawing {
        color: Color::rgb(0xcd, 0x7f, 0x32),
        side: Side::Left,
        view_box: ViewBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 1000.0,
        },
        max_width: 0.4,
        mask_solid: 0.4,
        shapes,
    }
}

fn stroke(width: f32, opacity: f32, cap: Cap) -> Option<Stroke> {
    Some(Stroke {
        width,
        opacity,
        cap,
    })
}

fn path(commands: &[PathCommand], width: f32, cap: Cap) -> Shape {
    Shape {
        geometry: Geometry::Path(commands.to_vec()),
        fill: Fill::None,
        stroke: stroke(width, 1.0, cap),
    }
}

fn disc(cx: f32, cy: f32, r: f32, opacity: f32) -> Shape {
    Shape {
        geometry: Geometry::Circle { cx, cy, r },
        fill: Fill::Solid { opacity },
        stroke: None,
    }
}

const fn m(x: f32, y: f32) -> PathCommand {
    PathCommand::Move { x, y }
}

const fn l(x: f32, y: f32) -> PathCommand {
    PathCommand::Line { x, y }
}

/// `drawing` in a window `width` × `height` at opacity 1.
fn paint(drawing: &Drawing, width: i32, height: i32) -> ArtPaint {
    ArtPaint::new(drawing, &CIRCUITS, DeviceSize::new(width, height), 1.0).expect("a small drawing")
}

/// The alphas of the pixels of column `x` from row `top`, `count` of them.
fn column(art: &ArtPaint, x: i64, top: i64, count: i64) -> Vec<f64> {
    (top..top + count).map(|y| art.alpha_at(x, y)).collect()
}

#[test]
fn the_box_is_as_tall_as_the_window_and_two_fifths_of_it_wide_at_most() {
    // Wider than tall: 0.4 × the height, the view box's shape. Taller
    // than wide: `max_width` × the width, 0.4 as an `f32`, the same
    // height.
    let circuits = jabbott_dev::circuits();
    let synapses = jabbott_dev::synapses();
    let capped = f64::from(0.4_f32) * 600.0;
    for (width, height, box_width) in [(1600, 900, 360.0), (600, 900, capped), (900, 900, 360.0)] {
        let art = paint(&circuits, width, height);
        assert_eq!(art.box_edges(), (0.0, box_width), "{width} × {height}");
        assert_eq!(art.window.top, 0);
        assert_eq!(art.window.bottom, i64::from(height));
        let art = paint(&synapses, width, height);
        let w = f64::from(width);
        assert_eq!(art.box_edges(), (w - box_width, w), "{width} × {height}");
    }
    // The pixels whose centres are in a box 100.3 px wide: columns 0 to
    // 99, since 100.5 is past it; and on the right of a 1000 px window,
    // from 900 (whose centre, 900.5, is past 899.7) to 999.
    let mut odd = left(vec![disc(1.0, 1.0, 1.0, 1.0)]);
    odd.view_box.width = 100.3;
    let art = paint(&odd, 1000, 1000);
    assert_eq!((art.window.left, art.window.right), (0, 100));
    odd.side = Side::Right;
    let art = paint(&odd, 1000, 1000);
    assert_eq!((art.window.left, art.window.right), (900, 1000));
}

#[test]
fn user_units_scale_with_the_height_and_line_up_with_the_side() {
    // 1000 px tall: one device pixel per user unit. A disc of radius 20 at
    // (100, 500) is solid at its centre and gone 30 px below it.
    let drawing = left(vec![disc(100.0, 500.0, 20.0, 1.0)]);
    let art = paint(&drawing, 1600, 1000);
    assert_eq!(art.alpha_at(100, 500), 1.0);
    assert_eq!(art.alpha_at(100, 530), 0.0);
    // 500 px tall: half a pixel per unit, so it is at (50, 250), radius
    // 10: covered at 7 px from the centre, not at 12.
    let art = paint(&drawing, 1600, 500);
    assert_eq!(art.alpha_at(50, 250), 1.0);
    assert_eq!(art.alpha_at(50, 257), 1.0);
    assert_eq!(art.alpha_at(50, 262), 0.0);
    // On the right, the view box from x = 160 to 560 ends at the window's
    // right edge: a disc at x = 470 is 90 units in from it.
    let mut right = left(vec![disc(470.0, 90.0, 6.0, 1.0)]);
    right.side = Side::Right;
    right.view_box.x = 160.0;
    let art = paint(&right, 1600, 1000);
    assert_eq!(art.alpha_at(1600 - 90, 90), 1.0);
    // Past the box's edge nothing is drawn, though the disc reaches there:
    // a narrow window shows less of the drawing.
    let mut wide = left(vec![disc(100.0, 500.0, 60.0, 1.0)]);
    wide.max_width = 0.1;
    let art = paint(&wide, 1000, 1000);
    assert_eq!(art.window.right, 100);
    assert_eq!(art.alpha_at(99, 500), 1.0);
    assert_eq!(art.alpha_at(100, 500), 0.0);
    assert_eq!(art.alpha_by_brute_force(100, 500), 0.0);
}

#[test]
fn the_view_box_may_start_anywhere() {
    // A view box from (−100, 200), 400 × 1000, in a window 500 px tall:
    // half a pixel a unit, the view box's corner at the box's. A disc at
    // (100, 700) lands at (100, 250), radius 4 at 2 px.
    let mut moved = left(vec![disc(100.0, 700.0, 4.0, 1.0)]);
    moved.view_box.x = -100.0;
    moved.view_box.y = 200.0;
    let art = paint(&moved, 1000, 500);
    assert_eq!(art.box_edges(), (0.0, 200.0));
    assert_eq!(art.alpha_at(100, 250), 1.0);
    assert_eq!(art.alpha_at(99, 249), 1.0);
    // The pixel centred 3.5 px away is clear.
    assert_eq!(art.alpha_at(103, 250), 0.0);
}

#[test]
fn a_drawing_with_nothing_to_show_has_no_elements() {
    let shapes = vec![disc(100.0, 500.0, 20.0, 1.0)];
    let edits: [fn(&mut Drawing); 7] = [
        |drawing: &mut Drawing| drawing.view_box.width = 0.0,
        |drawing: &mut Drawing| drawing.view_box.height = 0.0,
        |drawing: &mut Drawing| drawing.shapes.clear(),
        |drawing: &mut Drawing| drawing.max_width = 0.0,
        |drawing: &mut Drawing| drawing.shapes = vec![disc(100.0, 500.0, 0.0, 1.0)],
        |drawing: &mut Drawing| drawing.shapes = vec![disc(100.0, 500.0, 20.0, 0.0)],
        // Far outside the box.
        |drawing: &mut Drawing| drawing.shapes = vec![disc(-1e30, 500.0, 20.0, 1.0)],
    ];
    for edit in edits {
        let mut drawing = left(shapes.clone());
        edit(&mut drawing);
        let art = paint(&drawing, 1600, 1000);
        assert_eq!(art.counts(), (0, 0), "{drawing:?}");
        assert_eq!(art.alpha_at(100, 500), 0.0);
    }
    // A window with no pixels.
    let art = paint(&left(shapes), 0, 0);
    assert_eq!(art.counts(), (0, 0));
}

#[test]
fn the_mask_fades_from_two_fifths_of_the_box_to_its_far_edge() {
    // 250 px tall: a box 100 px wide. Solid up to 40 px from the side,
    // then (1 − t) / (1 − 0.4) for t the pixel centre's fraction of the
    // width; 0.4 is an `f32`.
    let solid = f64::from(0.4_f32);
    let mut drawing = left(vec![disc(1.0, 1.0, 1.0, 1.0)]);
    let art = paint(&drawing, 1000, 250);
    assert_eq!(art.weight(0), 1.0);
    assert_eq!(art.weight(39), 1.0);
    // Column 40's centre is at t = 0.405.
    assert_eq!(art.weight(40), (1.0 - 0.405) / (1.0 - solid));
    assert_eq!(art.weight(69), (1.0 - 0.695) / (1.0 - solid));
    assert!(art.weight(99) > 0.0 && art.weight(99) < 0.01);
    assert_eq!(art.weight(100), 0.0);
    // Mirrored on the right: column 999 is solid, 900 nearly clear.
    drawing.side = Side::Right;
    let art = paint(&drawing, 1000, 250);
    assert_eq!(art.weight(999), 1.0);
    assert_eq!(art.weight(960), 1.0);
    assert_eq!(art.weight(930), (1.0 - 0.695) / (1.0 - solid));
    assert!(art.weight(900) < 0.01);
    // A mask solid across the box, and one that fades all the way.
    drawing.mask_solid = 1.0;
    assert_eq!(paint(&drawing, 1000, 250).weight(901), 1.0);
    drawing.mask_solid = 0.0;
    drawing.side = Side::Left;
    let fading = 1.0 - 49.5 / 100.0;
    assert_eq!(paint(&drawing, 1000, 250).weight(49), fading);
    // The opacity multiplies it.
    let art = ArtPaint::new(&drawing, &CIRCUITS, DeviceSize::new(1000, 250), 0.7).expect("small");
    assert_eq!(art.weight(49), fading * 0.7);
}

#[test]
fn a_level_stroke_leaves_its_width_in_ink() {
    // 1000 px tall, one pixel a unit. A line 2 wide along y = 100, on the
    // boundary between rows 99 and 100, covers those two rows fully.
    let line = |y: f32, width: f32| {
        let drawing = left(vec![path(&[m(-10.0, y), l(300.0, y)], width, Cap::Butt)]);
        column(&paint(&drawing, 1600, 1000), 50, 97, 6)
    };
    assert_eq!(line(100.0, 2.0), [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
    // A quarter pixel lower: 0.75, 1, 0.25 (the ramp at 0.25 past the
    // edge), 2 in all.
    assert_eq!(line(100.25, 2.0), [0.0, 0.0, 0.75, 1.0, 0.25, 0.0]);
    // 3 wide about the centre of row 100: a half, a whole and a half
    // pixel either side.
    assert_eq!(line(100.5, 3.0), [0.0, 0.0, 1.0, 1.0, 1.0, 0.0]);
    // Under a pixel wide, a 1 px line at the opacity times the width: 0.5
    // wide on the boundary gives each row 0.5 × 0.5.
    assert_eq!(line(100.0, 0.5), [0.0, 0.0, 0.25, 0.25, 0.0, 0.0]);
    // On a row's centre: 0.5 there, and no ramp beside it.
    assert_eq!(line(100.5, 0.5), [0.0, 0.0, 0.0, 0.5, 0.0, 0.0]);
}

#[test]
fn butt_ends_stop_at_the_point_and_round_caps_reach_past_it() {
    // A line 4 wide from x = 100 to 200 along the centre of row 500.
    let end = |cap| {
        let drawing = left(vec![path(&[m(100.0, 500.5), l(200.0, 500.5)], 4.0, cap)]);
        let art = paint(&drawing, 1600, 1000);
        (197..206).map(|x| art.alpha_at(x, 500)).collect::<Vec<_>>()
    };
    // Butt: whole to column 199, whose centre is 0.5 inside the end, then
    // nothing (column 200's centre is 0.5 past it).
    assert_eq!(
        end(Cap::Butt),
        [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    // Round: a half disc of radius 2 past the end: columns 200 and 201
    // whole (their centres 0.5 and 1.5 past it), 202 clear (2.5 past).
    assert_eq!(
        end(Cap::Round),
        [1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]
    );
    let drawing = left(vec![path(
        &[m(100.0, 500.5), l(200.0, 500.5)],
        4.0,
        Cap::Round,
    )]);
    let art = paint(&drawing, 1600, 1000);
    assert_eq!(art.alpha_at(201, 500), 1.0);
    assert_eq!(art.alpha_at(202, 500), 0.0);
    // Diagonally past the end, a round cap is round: the pixel centred on
    // (201.5, 502.5) is 2.5 px from the end, on the ramp's outer edge, so
    // clear, where a square cap's corner would cover it half; the one on
    // (201.5, 501.5), √3.25 px away, is covered by 0.5 − (√3.25 − 2).
    assert_eq!(art.alpha_at(201, 502), 0.0);
    assert_eq!(
        art.alpha_at(201, 501),
        0.5 - ((1.5_f64 * 1.5 + 1.0).sqrt() - 2.0)
    );
}

#[test]
fn corners_are_joined_round() {
    // A right angle 4 wide: along y = 500 from x = 100 to 200, then down
    // to y = 600. Outside the corner a miter join would fill the square up
    // to (202, 498); the round join fills a quarter disc of radius 2 about
    // (200, 500).
    let drawing = left(vec![path(
        &[m(100.0, 500.0), l(200.0, 500.0), l(200.0, 600.0)],
        4.0,
        Cap::Butt,
    )]);
    let art = paint(&drawing, 1600, 1000);
    // The pixel centred on (201.5, 498.5) is 2.12 px from the corner:
    // 0.38 covered, where a miter join would cover it whole.
    let corner = art.alpha_at(201, 498);
    assert!((corner - (0.5 - (2.0 * 1.5_f64 * 1.5).sqrt() + 2.0)).abs() < 1e-12);
    assert!(corner > 0.3 && corner < 0.4, "{corner}");
    // Inside the bend and along both legs, solid; the union never
    // exceeds 1.
    assert_eq!(art.alpha_at(199, 501), 1.0);
    assert_eq!(art.alpha_at(150, 500), 1.0);
    assert_eq!(art.alpha_at(200, 550), 1.0);
    for y in 490..510 {
        for x in 190..210 {
            let alpha = art.alpha_at(x, y);
            assert!((0.0..=1.0).contains(&alpha), "({x}, {y}): {alpha}");
        }
    }
}

#[test]
fn a_shape_is_one_layer_and_shapes_composite() {
    // One path whose two subpaths cross: the crossing is covered once, at
    // the stroke's opacity.
    let mut crossing = path(
        &[
            m(100.0, 500.0),
            l(300.0, 500.0),
            m(200.0, 400.0),
            l(200.0, 600.0),
        ],
        4.0,
        Cap::Butt,
    );
    crossing.stroke = stroke(4.0, 0.5, Cap::Butt);
    let art = paint(&left(vec![crossing.clone()]), 1600, 1000);
    assert_eq!(art.alpha_at(200, 500), 0.5);
    assert_eq!(art.alpha_at(150, 500), 0.5);
    // The same lines as two shapes: the second over the first, 0.5 + 0.5
    // × 0.5.
    let mut across = crossing.clone();
    across.geometry = Geometry::Path(vec![m(100.0, 500.0), l(300.0, 500.0)]);
    let mut down = crossing;
    down.geometry = Geometry::Path(vec![m(200.0, 400.0), l(200.0, 600.0)]);
    let art = paint(&left(vec![across, down]), 1600, 1000);
    assert_eq!(art.alpha_at(200, 500), 0.75);
    assert_eq!(art.alpha_at(150, 500), 0.5);
}

#[test]
fn rings_rects_and_fills() {
    // A ring of radius 10, 2 wide, at (100.5, 500.5), from 9 to 11 px out:
    // the pixel centres 11, 10 and 9 px above the centre are covered by
    // 0.5, 1 and 0.5.
    let ring = Shape {
        geometry: Geometry::Circle {
            cx: 100.5,
            cy: 500.5,
            r: 10.0,
        },
        fill: Fill::None,
        stroke: stroke(2.0, 1.0, Cap::Butt),
    };
    let art = paint(&left(vec![ring.clone()]), 1600, 1000);
    assert_eq!(column(&art, 100, 487, 6), [0.0, 0.0, 0.5, 1.0, 0.5, 0.0]);
    assert_eq!(art.alpha_at(100, 500), 0.0, "the hole");
    // Filled under the stroke: the fill, then the ring over it.
    let mut filled = ring;
    filled.fill = Fill::Solid { opacity: 0.5 };
    let art = paint(&left(vec![filled]), 1600, 1000);
    assert_eq!(art.alpha_at(100, 500), 0.5);
    assert_eq!(art.alpha_at(100, 490), 1.0);
    // A rounded rect from (100, 400), 100 × 50, rx 10, stroked 2 wide: its
    // straight edges are crisp, its corners round.
    let rect = Shape {
        geometry: Geometry::Rect {
            x: 100.0,
            y: 400.0,
            width: 100.0,
            height: 50.0,
            rx: 10.0,
        },
        fill: Fill::Solid { opacity: 0.25 },
        stroke: stroke(2.0, 1.0, Cap::Butt),
    };
    let art = paint(&left(vec![rect.clone()]), 1600, 1000);
    assert_eq!(column(&art, 150, 398, 4), [0.0, 1.0, 1.0, 0.25]);
    assert_eq!(art.alpha_at(150, 425), 0.25);
    // At a corner the band follows a circle of radius 10 about (110, 410):
    // the pixel centred on (102.5, 402.5), 10.61 px from it, is covered by
    // 0.5 − (10.61 − 10 − 1) = 0.89.
    let corner = art.alpha_at(102, 402);
    assert!(corner > 0.88 && corner < 0.9, "{corner}");
    // The pixel in the square corner, which a sharp rect's stroke would
    // cover whole, is clear.
    assert_eq!(art.alpha_at(100, 400), 0.0);
    // A corner radius past half the height is cut to it: a pill.
    let mut pill = rect;
    pill.geometry = Geometry::Rect {
        x: 100.0,
        y: 400.0,
        width: 100.0,
        height: 50.0,
        rx: 1000.0,
    };
    pill.stroke = None;
    pill.fill = Fill::Solid { opacity: 1.0 };
    let art = paint(&left(vec![pill]), 1600, 1000);
    assert_eq!(art.alpha_at(125, 425), 1.0);
    // (100.5, 400.5) is 24.5 × √2 − 25 = 9.6 px outside the end's circle.
    assert_eq!(art.alpha_at(100, 400), 0.0);
    assert_eq!(art.alpha_at(101, 424), 1.0);
}

#[test]
fn a_halo_fades_from_its_centre() {
    // `fill-opacity` 0.45 and a centre stop of 0.55, over a circle of
    // radius 20 at (100.5, 500.5).
    let halo = Shape {
        geometry: Geometry::Circle {
            cx: 100.5,
            cy: 500.5,
            r: 20.0,
        },
        fill: Fill::Radial {
            opacity: 0.45,
            center: 0.55,
        },
        stroke: None,
    };
    let art = paint(&left(vec![halo]), 1600, 1000);
    let (opacity, center) = (f64::from(0.45_f32), f64::from(0.55_f32));
    assert_eq!(art.alpha_at(100, 500), opacity * center);
    assert_eq!(art.alpha_at(110, 500), opacity * (center * 0.5));
    assert_eq!(art.alpha_at(120, 500), 0.0);
    assert_eq!(art.alpha_at(100, 520), 0.0);
}

#[test]
fn curves_flatten_within_the_tolerance() {
    // A straight curve is one line; |p0 − 2·p1 + p2| = 2 needs
    // sqrt(2 / (4 / 32)) = 4.
    assert_eq!(segment_count(0.0), 1);
    assert_eq!(segment_count(2.0), 4);
    assert_eq!(segment_count(2.01), 5);
    assert_eq!(segment_count(1e12), MAX_SEGMENTS);
    assert_eq!(segment_count(f64::NAN), 1);
    // The points of a quadratic and a cubic curve end exactly at their
    // ends, and no point of the curve between them is further than the
    // tolerance from the lines.
    let distance_to = |points: &[(f64, f64)], (px, py): (f64, f64)| {
        points
            .windows(2)
            .map(|pair| {
                let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                length(px - (ax + t * dx), py - (ay + t * dy))
            })
            .fold(f64::INFINITY, f64::min)
    };
    let (p0, p1, p2, p3) = ((10.0, 10.0), (60.0, 200.0), (180.0, -40.0), (250.0, 90.0));
    let mut points = vec![p0];
    flatten_quad(p0, p1, p3, &mut points).expect("a few points");
    assert_eq!(points.last(), Some(&p3));
    for step in 0..=1000 {
        let t = f64::from(step) / 1000.0;
        let u = 1.0 - t;
        let on = (
            u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p3.0,
            u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p3.1,
        );
        assert!(distance_to(&points, on) <= TOLERANCE + 1e-9, "{t}");
    }
    let mut points = vec![p0];
    flatten_cubic(p0, p1, p2, p3, &mut points).expect("a few points");
    assert_eq!(points.last(), Some(&p3));
    for step in 0..=1000 {
        let t = f64::from(step) / 1000.0;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        let on = (
            a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
            a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
        );
        assert!(distance_to(&points, on) <= TOLERANCE + 1e-9, "{t}");
    }
}

#[test]
fn a_path_before_its_first_move_and_a_lone_point_draw_nothing() {
    let drawing = left(vec![path(
        &[l(100.0, 500.0), l(200.0, 500.0)],
        4.0,
        Cap::Round,
    )]);
    assert_eq!(paint(&drawing, 1600, 1000).counts(), (0, 0));
    let drawing = left(vec![path(&[m(100.0, 500.0)], 4.0, Cap::Round)]);
    assert_eq!(paint(&drawing, 1600, 1000).counts(), (0, 0));
    // A line of no length with round caps is a dot.
    let drawing = left(vec![path(
        &[m(100.5, 500.5), l(100.5, 500.5)],
        4.0,
        Cap::Round,
    )]);
    let art = paint(&drawing, 1600, 1000);
    assert_eq!(art.alpha_at(100, 500), 1.0);
    assert_eq!(art.alpha_at(103, 500), 0.0);
}

#[test]
fn the_sites_drawings_are_drawn_still() {
    // Every element of the two still drawings, and none of the pulses:
    // 25 traces, 28 pads and the chip's 2 rects, 1 circle and 6 pins; 24
    // axons, 52 dendrites, and 15 cells of a halo and a body each.
    let circuits = jabbott_dev::circuits();
    let synapses = jabbott_dev::synapses();
    assert_eq!(circuits.shapes.len(), 25 + 28 + 2 + 1 + 6);
    assert_eq!(synapses.shapes.len(), 24 + 52 + 30);
    // The chip's two rects and its circle are filled and stroked; every
    // other shape is one or the other.
    let elements = 25 + 28 + 2 * 3 + 6;
    let art = paint(&circuits, 2500, 1000);
    assert_eq!(art.counts().0, elements);
    let art = paint(&synapses, 2500, 1000);
    assert_eq!(art.counts().0, 24 + 52 + 30);
    // Where the second spark of the circuits lands, a burst of radius 16
    // at (360, 400) shows only in motion: 12 units below it, clear of the
    // pad and its trace, there is nothing.
    let art = paint(&circuits, 2500, 1000);
    assert_eq!(art.alpha_at(360, 412), 0.0);
    assert!(art.alpha_at(360, 404) > 0.0, "the pad's ring, 4.5 out");
    // On the right, a cell's body over its halo.
    let art = paint(&synapses, 2500, 1000);
    let cell = (2500 - (560 - 470), 90);
    assert!(art.alpha_at(cell.0, cell.1) > 0.4, "a cell's body");
}

#[test]
fn every_pixel_is_the_same_by_band_alone_and_by_brute_force() {
    // The site's drawings at sizes that cut shapes at the box and the
    // window, in bands of 32 rows as `render` works, then each pixel on
    // its own and with every piece of every element tested.
    for drawing in [jabbott_dev::circuits(), jabbott_dev::synapses()] {
        for (width, height) in [(480, 300), (300, 420), (1000, 167)] {
            let art = paint(&drawing, width, height);
            let window = art.window;
            let columns = usize::try_from(window.width()).expect("a width");
            let mut alpha = vec![0.0; columns * 32];
            let mut scratch = alpha.clone();
            let mut nonzero = 0;
            for top in (0..i64::from(height)).step_by(32) {
                let band = Edges {
                    top,
                    bottom: (top + 32).min(i64::from(height)),
                    ..window
                };
                art.fill_window(band, &mut alpha, &mut scratch);
                for y in band.top..band.bottom {
                    for x in band.left..band.right {
                        let index = usize::try_from((y - top) * window.width() + x - window.left)
                            .expect("inside");
                        let banded = alpha[index];
                        assert_eq!(banded.to_bits(), art.alpha_at(x, y).to_bits(), "({x}, {y})");
                        assert_eq!(
                            banded.to_bits(),
                            art.alpha_by_brute_force(x, y).to_bits(),
                            "({x}, {y}) at {width} × {height}"
                        );
                        assert!((0.0..=1.0).contains(&banded));
                        nonzero += usize::from(banded > 0.0);
                    }
                }
            }
            assert!(
                nonzero > 100,
                "{nonzero} pixels drawn at {width} × {height}"
            );
        }
    }
}

#[test]
fn a_drawing_is_the_same_twice() {
    let first = paint(&jabbott_dev::synapses(), 700, 400);
    let second = paint(&jabbott_dev::synapses(), 700, 400);
    for y in (0..400).step_by(7) {
        for x in first.window.left..first.window.right {
            assert_eq!(
                first.alpha_at(x, y).to_bits(),
                second.alpha_at(x, y).to_bits()
            );
        }
    }
}

#[test]
fn huge_and_tiny_values_draw_without_panicking() {
    let huge = f32::MAX;
    let shapes = vec![
        path(
            &[m(-huge, -huge), l(huge, huge), l(0.0, huge)],
            2.0,
            Cap::Round,
        ),
        path(
            &[
                m(0.0, 0.0),
                PathCommand::Cubic {
                    x1: huge,
                    y1: -huge,
                    x2: -huge,
                    y2: huge,
                    x: 10.0,
                    y: 10.0,
                },
                PathCommand::Quad {
                    x1: huge,
                    y1: huge,
                    x: 1.0,
                    y: 1.0,
                },
            ],
            huge,
            Cap::Butt,
        ),
        disc(0.0, 0.0, huge, 1.0),
        Shape {
            geometry: Geometry::Rect {
                x: -huge,
                y: -huge,
                width: huge,
                height: huge,
                rx: huge,
            },
            fill: Fill::Radial {
                opacity: 1.0,
                center: 1.0,
            },
            stroke: stroke(huge, 1.0, Cap::Round),
        },
    ];
    // Each either draws, its alphas within 0 to 1, or runs out of budget
    // at a shape.
    let mut refused = 0;
    for view_box in [1e-30, 1.0, 1000.0, huge] {
        let mut drawing = left(shapes.clone());
        drawing.view_box.width = view_box;
        drawing.view_box.height = view_box;
        drawing.max_width = huge;
        let art = match ArtPaint::new(&drawing, &CIRCUITS, DeviceSize::new(64, 48), 1.0) {
            Ok(art) => art,
            Err(error) => {
                assert!(
                    matches!(
                        error,
                        GfxError::InvalidStyle {
                            field: "circuits.shapes",
                            index: Some(_),
                        }
                    ),
                    "{error:?}"
                );
                refused += 1;
                continue;
            }
        };
        for y in 0..48 {
            for x in 0..64 {
                let alpha = art.alpha_at(x, y);
                assert!((0.0..=1.0).contains(&alpha), "{alpha}");
            }
        }
    }
    assert!(refused < 4, "some draw");
}

/// A stroked path of `commands`, `width` units wide.
fn stroked(commands: Vec<PathCommand>, width: f32) -> Shape {
    Shape {
        geometry: Geometry::Path(commands),
        fill: Fill::None,
        stroke: stroke(width, 0.5, Cap::Butt),
    }
}

#[test]
fn each_limit_is_refused_one_past_it() {
    let refused = |field, index| Err(GfxError::InvalidStyle { field, index });
    // Shapes: as many as the limit, then one more.
    let mut drawing = left(vec![disc(100.0, 500.0, 4.0, 1.0); MAX_SHAPES]);
    assert_eq!(drawing.validate(&CIRCUITS), Ok(()));
    drawing.shapes.push(disc(100.0, 500.0, 4.0, 1.0));
    assert_eq!(
        drawing.validate(&CIRCUITS),
        refused("circuits.shapes", Some(MAX_SHAPES))
    );
    // Commands in a path.
    let path = |count: usize| {
        let mut commands = vec![m(10.0, 10.0)];
        commands.extend((1..count).map(|step| l(10.0 + step as f32 % 7.0, 20.0)));
        commands
    };
    let mut drawing = left(vec![stroked(path(MAX_PATH_COMMANDS), 2.0)]);
    assert_eq!(drawing.validate(&SYNAPSES), Ok(()));
    drawing.shapes = vec![stroked(path(MAX_PATH_COMMANDS + 1), 2.0)];
    assert_eq!(
        drawing.validate(&SYNAPSES),
        refused("synapses.shapes.geometry", Some(0))
    );
    // Commands in all: 16 full paths reach the limit; a 17th path of one
    // command passes it, and is the one named.
    let full = MAX_DRAWING_COMMANDS / MAX_PATH_COMMANDS;
    let mut drawing = left(vec![stroked(path(MAX_PATH_COMMANDS), 2.0); full]);
    assert_eq!(drawing.validate(&CIRCUITS), Ok(()));
    drawing.shapes.insert(3, disc(1.0, 1.0, 1.0, 1.0));
    drawing.shapes.push(stroked(path(1), 2.0));
    assert_eq!(
        drawing.validate(&CIRCUITS),
        refused("circuits.shapes.geometry", Some(full + 1))
    );
    // A stroke as wide as the view box's longer side, 1,000, and the next
    // `f32` up.
    let mut drawing = left(vec![stroked(path(2), 1000.0)]);
    assert_eq!(drawing.validate(&CIRCUITS), Ok(()));
    drawing.shapes = vec![stroked(path(2), f32::from_bits(1000.0_f32.to_bits() + 1))];
    assert_eq!(
        drawing.validate(&CIRCUITS),
        refused("circuits.shapes.stroke.width", Some(0))
    );
    // So is a circle's ring.
    let mut ring = disc(100.0, 500.0, 4.0, 1.0);
    ring.stroke = stroke(1000.5, 1.0, Cap::Butt);
    drawing.shapes = vec![disc(1.0, 1.0, 1.0, 1.0), ring];
    assert_eq!(
        drawing.validate(&CIRCUITS),
        refused("circuits.shapes.stroke.width", Some(1))
    );
}

#[test]
fn preparing_stops_at_the_piece_budget() {
    // A path within every limit whose 255 cubic curves fold back and forth
    // inside the box: in a window 16,384 px a side each is flattened into
    // 512 lines, about 1,023 pieces, so the 65th runs the budget out.
    let folded = PathCommand::Cubic {
        x1: 600.0,
        y1: 500.0,
        x2: -200.0,
        y2: 500.0,
        x: 200.0,
        y: 500.0,
    };
    let mut commands = vec![m(200.0, 500.0)];
    commands.extend([folded; MAX_PATH_COMMANDS - 1]);
    let drawing = left(vec![disc(1.0, 1.0, 1.0, 1.0), stroked(commands, 1.0)]);
    assert_eq!(drawing.validate(&CIRCUITS), Ok(()));
    let size = DeviceSize::new(16_384, 16_384);
    assert_eq!(
        ArtPaint::new(&drawing, &CIRCUITS, size, 1.0).map(|art| art.counts()),
        Err(GfxError::InvalidStyle {
            field: "circuits.shapes",
            index: Some(1),
        })
    );
    // With 63 curves, it fits.
    let mut few = drawing.clone();
    if let Geometry::Path(commands) = &mut few.shapes[1].geometry {
        commands.truncate(64);
    }
    let art = ArtPaint::new(&few, &CIRCUITS, size, 1.0).expect("within the budget");
    let (pieces_left, _) = art.budget_left();
    assert!(pieces_left < MAX_PIECES / 50, "{pieces_left}");
}

#[test]
fn preparing_stops_at_the_test_budget() {
    // A zigzag stroke 1,000 units wide, as wide as a stroke may be: every
    // piece reaches the whole box, 400 × 1,000 px in a window 1,000 px a
    // side, so 64 of them use up the 25,600,000 tests it may take.
    let mut commands = vec![m(0.0, 0.0)];
    commands.extend(
        (1..100).map(|step| l(if step % 2 == 0 { 0.0 } else { 400.0 }, 10.0 * step as f32)),
    );
    let drawing = left(vec![stroked(commands.clone(), 1000.0)]);
    assert_eq!(drawing.validate(&CIRCUITS), Ok(()));
    assert_eq!(
        ArtPaint::new(&drawing, &CIRCUITS, DeviceSize::new(1000, 1000), 1.0)
            .map(|art| art.counts()),
        Err(GfxError::InvalidStyle {
            field: "circuits.shapes",
            index: Some(0),
        })
    );
    assert_eq!(
        test_budget(Edges {
            left: 0,
            top: 0,
            right: 400,
            bottom: 1000
        }),
        25_600_000
    );
    // Thin, it fits; and a small box may always take `MIN_TESTS`.
    let thin = left(vec![stroked(commands, 2.0)]);
    assert!(ArtPaint::new(&thin, &CIRCUITS, DeviceSize::new(1000, 1000), 1.0).is_ok());
    assert_eq!(
        test_budget(Edges {
            left: 0,
            top: 0,
            right: 4,
            bottom: 10
        }),
        MIN_TESTS
    );
}

#[test]
fn a_translucent_color_composites_each_shape_as_svg_does() {
    // Two opaque discs on the same spot in white at alpha 128: each adds
    // 128/255 over the other, as SVG paints a shape in a translucent color,
    // 1 − (1 − a)², not the color's alpha once over both.
    let mut drawing = left(vec![disc(100.0, 500.0, 40.0, 1.0); 2]);
    drawing.color = Color::rgba(255, 255, 255, 128);
    let art = paint(&drawing, 1600, 1000);
    let a = 128.0 / 255.0;
    assert_eq!(art.alpha_at(100, 500), a + a * (1.0 - a));
    // The color is opaque: its alpha is in the elements'.
    assert_eq!(art.color, [1.0, 1.0, 1.0, 1.0]);
    drawing.shapes.truncate(1);
    assert_eq!(paint(&drawing, 1600, 1000).alpha_at(100, 500), a);
    // An opaque color leaves the opacities as they are.
    drawing.color = Color::rgb(255, 255, 255);
    drawing.shapes = vec![disc(100.0, 500.0, 40.0, 0.25); 2];
    assert_eq!(
        paint(&drawing, 1600, 1000).alpha_at(100, 500),
        f64::from(0.25_f32) + f64::from(0.25_f32) * (1.0 - f64::from(0.25_f32))
    );
    // A transparent one draws nothing.
    drawing.color = Color::rgba(255, 255, 255, 0);
    assert_eq!(paint(&drawing, 1600, 1000).counts(), (0, 0));
}
