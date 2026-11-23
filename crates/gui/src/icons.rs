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

//! Tidepool's icon set, the first six: the activity bar's (M3.4,
//! ADR-0060).
//!
//! New in Tidepool.
//!
//! Each function makes one [`Icon`] in a viewbox [`SIDE`] (24) units
//! square. The icons are line art: shapes drawn as if with a pen 2 units
//! wide, made as closed contours that `font`'s rasterizer fills, each hole
//! winding the other way from the contour around it
//! ([`icon`](crate::icon)'s module documentation). At 24 logical pixels the
//! pen is 2 device pixels wide at scale 1, 3 at 1.5 and 4 at 2.
//!
//! # Why every straight edge is on an even unit
//!
//! A horizontal or vertical edge is crisp only where it falls on a whole
//! device pixel. Drawn 24 logical pixels across, a unit is 1, 1.5 or 2
//! device pixels at scale 1, 1.5 or 2, so an edge on an even unit lands on
//! a whole pixel at all three, and one on an odd unit would be blurred
//! across two pixels at 1.5. Curves and diagonals are anti-aliased
//! (their edge pixels partly covered) at any scale.
//!
//! # The look
//!
//! jabbott.dev's is squared and technical: its panels have square corners,
//! and its buttons corners cut at 45° (the theme's `cut`). So the
//! explorer's page and the phone have corners cut at 45°, and the source
//! control's branch turns its corner with a 45° cut. Only what must be
//! round is: the magnifier's lens, the branch's rings, the wrench's head
//! and the spark's curved sides.
//!
//! # The same on every OS
//!
//! The outlines are computed in `f32` with `+ − × ÷` and `sqrt` alone,
//! which IEEE 754 rounds the same on every CPU, and never a library
//! function such as `sin`: so each icon's points, and the pixels `font`
//! fills from them, are the same everywhere, which the golden frames that
//! draw them need.
//!
//! # Registering them
//!
//! An icon draws where a `ui` [`IconId`](tidepool_ui::draw::IconId) is
//! registered for it, with
//! [`Painter::set_icon`](crate::Painter::set_icon) or
//! [`Gui::set_icon`](crate::Gui::set_icon); which id names which icon is
//! the caller's choice. [`ALL`] lists them with their names.

use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};

use tidepool_font::outline::OutlinePoint;

use crate::Icon;

/// The side of every icon's viewbox, in units: 24, as the activity bar
/// draws them 24 logical pixels across.
pub const SIDE: f32 = 24.0;

/// A function that makes an icon, such as [`files`].
pub type MakeIcon = fn() -> Icon;

/// Each icon, with its name, in the activity bar's order.
pub const ALL: [(&str, MakeIcon); 6] = [
    ("files", files),
    ("search", search),
    ("branch", branch),
    ("wrench", wrench),
    ("device", device),
    ("spark", spark),
];

/// The width of the pen the icons are drawn with, in units.
const PEN: f32 = 2.0;

/// Explorer: a page with its top-right corner cut at 45°, as jabbott.dev
/// cuts its corners, and two lines of text on it.
///
/// The page is 16 units wide and 20 tall (x 4 to 20, y 2 to 22), drawn
/// with the 2-unit pen; the cut takes 6 units off each of the corner's
/// sides. The lines of text are bars 2 units high, 8 and 6 units long
/// (from x 8), at y 10 to 12 and 14 to 16.
pub fn files() -> Icon {
    let mut shape = Shape::default();
    // The cut's outer edge is the line x − y = 12; its inner edge, the
    // pen's width further in, x − y = 12 − 2√2.
    let inner = 12.0 - PEN * SQRT_2;
    shape.polygon(&[
        (4.0, 2.0),
        (14.0, 2.0),
        (20.0, 8.0),
        (20.0, 22.0),
        (4.0, 22.0),
    ]);
    shape.polygon(&[
        (6.0, 4.0),
        (6.0, 20.0),
        (18.0, 20.0),
        (18.0, 18.0 - inner),
        (4.0 + inner, 4.0),
    ]);
    shape.rect(8.0, 10.0, 16.0, 12.0);
    shape.rect(8.0, 14.0, 14.0, 16.0);
    shape.icon()
}

/// Search: a magnifier, a ring with a handle running from it to the
/// bottom-right corner at 45°.
///
/// The lens is a ring round (10, 10), 16 units across outside and 12
/// inside. The handle is a bar 3 units wide with square ends, along the
/// line from the lens's center to the bottom-right corner: it starts
/// inside the ring's stroke, so the two join, and its end's middle is at
/// about (20.7, 20.7).
pub fn search() -> Icon {
    let mut shape = Shape::default();
    let center = V::new(10.0, 10.0);
    shape.ring(center, 8.0, 6.0);
    // Down and to the right, and across the handle.
    let along = V::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    let across = V::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
    let half = 1.5;
    let (start, end) = (center + along * 6.9, center + along * 15.2);
    shape.points_polygon(&[
        start + across * half,
        end + across * half,
        end - across * half,
        start - across * half,
    ]);
    shape.icon()
}

/// Source control: a git branch, three rings joined by lines.
///
/// Two rings stand on a trunk at the left, one at the top (round (7, 5))
/// and one at the bottom (round (7, 19)), and the third at the top right
/// (round (17, 5)); each is 8 units across outside and 4 inside. The
/// branch runs from the top-right ring straight down, turns its corner
/// with a 45° cut, and runs left into the bottom ring, as a branch merges.
/// The trunk and the branch are 2 units wide and end inside the rings'
/// strokes, so that they join them.
pub fn branch() -> Icon {
    let mut shape = Shape::default();
    for center in [V::new(7.0, 5.0), V::new(7.0, 19.0), V::new(17.0, 5.0)] {
        shape.ring(center, 4.0, 2.0);
    }
    // The trunk, x 6 to 8, from inside the top ring's stroke to inside the
    // bottom one's.
    shape.rect(6.0, 8.0, 8.0, 16.0);
    // The branch's centerline runs down x = 17, then along the cut
    // x + y = 32, then left along y = 19. Its sides are half a pen either
    // side: x 16 and 18, y 18 and 20, and the cut's x + y = 32 ± √2 × PEN / 2.
    let (outer, inner) = (32.0 + SQRT_2 * PEN / 2.0, 32.0 - SQRT_2 * PEN / 2.0);
    shape.polygon(&[
        (16.0, 8.0),
        (18.0, 8.0),
        (18.0, outer - 18.0),
        (outer - 20.0, 20.0),
        (10.0, 20.0),
        (10.0, 18.0),
        (inner - 18.0, 18.0),
        (16.0, inner - 16.0),
    ]);
    shape.icon()
}

/// Toolchains: a wrench at 45°, its head at the top right and its handle
/// running to the bottom left.
///
/// The head is a ring round (15, 9), 13 units across outside and 9
/// inside, with a jaw cut out of it towards the top-right corner: a slot
/// 5 units wide, its sides parallel to the handle, which leaves the head
/// a C. The handle is a solid bar 4 units wide, twice the pen's width as
/// a wrench's handle is thicker than its head's rim: it starts inside the
/// ring's stroke opposite the jaw and ends in a half circle round
/// (5, 19).
pub fn wrench() -> Icon {
    let mut shape = Shape::default();
    let center = V::new(15.0, 9.0);
    // Towards the jaw (up and to the right), and across it.
    let along = V::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
    let across = V::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    let (outer, inner, slot) = (6.5_f32, 4.5_f32, 2.5_f32);
    // Where each side of the slot meets the outer and the inner circle,
    // as directions from the center.
    let tip = |radius: f32, side: f32| {
        let reach = (radius * radius - slot * slot).sqrt();
        (along * reach + across * (side * slot)) * (1.0 / radius)
    };
    // The head, a C: round the outside the long way, from the slot's side
    // across to its other side, in along that side, back round the inside,
    // and out along the first side.
    shape
        .arc(
            center,
            outer,
            tip(outer, 1.0),
            tip(outer, -1.0),
            Turn::Clockwise,
        )
        .arc(
            center,
            inner,
            tip(inner, -1.0),
            tip(inner, 1.0),
            Turn::Counterclockwise,
        )
        .close();
    // The handle, from inside the stroke opposite the jaw to a round end.
    let half = 2.0;
    let start = center - along * 5.4;
    let end = V::new(5.0, 19.0);
    shape
        .line(start + across * half)
        .line(end + across * half)
        .arc(end, half, across, -across, Turn::Clockwise)
        .line(start - across * half)
        .close();
    shape.icon()
}

/// Devices: a phone, upright, its corners cut at 45°, with a notch at
/// the top of its screen and a gesture bar near the bottom.
///
/// The phone is 12 units wide and 20 tall (x 6 to 18, y 2 to 22), drawn
/// with the 2-unit pen, each corner cut by 2 units along each side. The
/// notch hangs 2 units into the screen from the middle of its top edge,
/// 4 units wide (x 10 to 14); the gesture bar, of a phone with no home
/// button, is 4 units long and 2 high (x 10 to 14, y 16 to 18), 2 units
/// above the screen's bottom edge.
pub fn device() -> Icon {
    let mut shape = Shape::default();
    let cut = 2.0;
    // Inside, each cut's edge is the pen's width further in along its
    // normal, so it meets the screen's sides `cut + PEN × √2 − 2 × PEN`
    // units from the corner.
    let inside = cut + PEN * SQRT_2 - 2.0 * PEN;
    shape.points_polygon(&cut_rect(6.0, 2.0, 18.0, 22.0, cut));
    // The screen, a hole, with the notch in its top edge, between that
    // edge's two ends (the first two corners).
    let [first, second, rest @ ..] = cut_rect(8.0, 4.0, 16.0, 20.0, inside);
    let mut screen = vec![
        first,
        V::new(10.0, 4.0),
        V::new(10.0, 6.0),
        V::new(14.0, 6.0),
        V::new(14.0, 4.0),
        second,
    ];
    screen.extend(rest);
    screen.reverse();
    shape.points_polygon(&screen);
    shape.rect(10.0, 16.0, 14.0, 18.0);
    shape.icon()
}

/// Agents: a four-pointed spark, its points up, right, down and left, its
/// sides curving in towards the center.
///
/// Its points reach 10 units from the center, (12, 12), to 2 units from
/// each side of the viewbox. Each side is a quadratic curve whose control
/// point is 2 units across and 2 along from the center towards the corner
/// between its points: (14, 10) for the side from the top point to the
/// right one. Its hole is a smaller spark, its points 4.6 units from the
/// center and its control points 1.85 units across and along, which keeps
/// the outline about the pen's width where the sides are closest to the
/// center; towards the points, where the spark is narrower than two pens,
/// it is solid.
pub fn spark() -> Icon {
    let mut shape = Shape::default();
    let center = V::new(12.0, 12.0);
    shape.spark(center, 10.0, 2.0, Turn::Clockwise);
    shape.spark(center, 4.6, 1.85, Turn::Counterclockwise);
    shape.icon()
}

/// A rect's corners, `left` to `right` and `top` to `bottom`, each corner
/// cut by `cut` units along both its sides, clockwise from the top edge's
/// left end.
fn cut_rect(left: f32, top: f32, right: f32, bottom: f32, cut: f32) -> [V; 8] {
    [
        V::new(left + cut, top),
        V::new(right - cut, top),
        V::new(right, top + cut),
        V::new(right, bottom - cut),
        V::new(right - cut, bottom),
        V::new(left + cut, bottom),
        V::new(left, bottom - cut),
        V::new(left, top + cut),
    ]
}

/// A point or a direction in the viewbox, y growing down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct V {
    x: f32,
    y: f32,
}

impl V {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Positive when `other` is clockwise from `self` on screen (y down) by
    /// less than a half turn.
    fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    /// `self` scaled to a length of 1.
    fn unit(self) -> Self {
        self * (1.0 / self.dot(self).sqrt())
    }

    /// `self` turned a quarter turn `turn`'s way on screen.
    const fn quarter(self, turn: Turn) -> Self {
        match turn {
            Turn::Clockwise => Self::new(-self.y, self.x),
            Turn::Counterclockwise => Self::new(self.y, -self.x),
        }
    }
}

impl std::ops::Add for V {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for V {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Neg for V {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl std::ops::Mul<f32> for V {
    type Output = Self;
    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

/// Which way round an arc goes, as seen on screen (y down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// From right to down: how an icon's outer contours go.
    Clockwise,
    /// From right to up: how holes go.
    Counterclockwise,
}

impl Turn {
    const fn sign(self) -> f32 {
        match self {
            Self::Clockwise => 1.0,
            Self::Counterclockwise => -1.0,
        }
    }
}

/// The cosine of the widest arc one quadratic curve draws, 50°: a
/// quadratic through an arc of 45° strays from the circle by 0.3% of its
/// radius, under a twentieth of a pixel for every arc here, at scale 2.
const COS_WIDEST_ARC: f32 = 0.642_787_6;

/// An icon's contours, as they are built.
#[derive(Default)]
struct Shape {
    points: Vec<OutlinePoint>,
    ends: Vec<usize>,
}

impl Shape {
    /// The first point of the contour being built.
    fn start(&self) -> usize {
        self.ends.last().copied().unwrap_or(0)
    }

    /// Adds a point on the curve: a line from the one before. A point the
    /// same as the one before is left out.
    fn line(&mut self, at: V) -> &mut Self {
        let point = OutlinePoint {
            x: at.x,
            y: at.y,
            on_curve: true,
        };
        if self.points.len() == self.start() || self.points.last() != Some(&point) {
            self.points.push(point);
        }
        self
    }

    /// Adds a control point off the curve.
    fn control(&mut self, at: V) -> &mut Self {
        self.points.push(OutlinePoint {
            x: at.x,
            y: at.y,
            on_curve: false,
        });
        self
    }

    /// Ends the contour being built: it closes back to its first point,
    /// which a last point the same as the first would repeat, so that one
    /// is left out.
    fn close(&mut self) -> &mut Self {
        let start = self.start();
        if self.points.len() > start + 1 && self.points.last() == self.points.get(start) {
            self.points.pop();
        }
        self.ends.push(self.points.len());
        self
    }

    /// Adds an arc of the circle round `center`, `radius` across, from
    /// direction `from` to direction `to` (each of length 1) going `turn`'s
    /// way, the whole circle when they are the same: a line to its start,
    /// then quadratic curves of at most 50° each, each control point where
    /// the tangents at its ends meet.
    fn arc(&mut self, center: V, radius: f32, from: V, to: V, turn: Turn) -> &mut Self {
        self.line(center + from * radius);
        let mut pieces = Vec::new();
        split(from, to, turn, 0, &mut pieces);
        for (a, b) in pieces {
            let middle = (a + b).unit();
            self.control(center + middle * (radius / a.dot(middle)));
            self.line(center + b * radius);
        }
        self
    }

    /// Adds a closed contour of straight lines through `corners`.
    fn points_polygon(&mut self, corners: &[V]) -> &mut Self {
        for &corner in corners {
            self.line(corner);
        }
        self.close()
    }

    /// [`Shape::points_polygon`] from pairs of coordinates.
    fn polygon(&mut self, corners: &[(f32, f32)]) -> &mut Self {
        for &(x, y) in corners {
            self.line(V::new(x, y));
        }
        self.close()
    }

    /// Adds a rect from `left`, `top` to `right`, `bottom`, clockwise.
    fn rect(&mut self, left: f32, top: f32, right: f32, bottom: f32) -> &mut Self {
        self.polygon(&[(left, top), (right, top), (right, bottom), (left, bottom)])
    }

    /// Adds a ring round `center`: a circle `outer` units in radius,
    /// clockwise, and a hole `inner` units in radius, counterclockwise.
    fn ring(&mut self, center: V, outer: f32, inner: f32) -> &mut Self {
        let east = V::new(1.0, 0.0);
        self.arc(center, outer, east, east, Turn::Clockwise).close();
        self.arc(center, inner, east, east, Turn::Counterclockwise)
            .close()
    }

    /// Adds a four-pointed spark round `center`, going `turn`'s way: its
    /// points `reach` units up, right, down and left of the center, each
    /// side a quadratic curve whose control point is `waist` units across
    /// and `waist` along from the center towards the corner between its
    /// points.
    fn spark(&mut self, center: V, reach: f32, waist: f32, turn: Turn) -> &mut Self {
        let mut direction = V::new(0.0, -1.0);
        for _ in 0..4 {
            let next = direction.quarter(turn);
            self.line(center + direction * reach);
            self.control(center + (direction + next) * waist);
            direction = next;
        }
        self.close()
    }

    /// The icon: its contours, which are inside the viewbox by
    /// construction (the unit tests check each).
    fn icon(self) -> Icon {
        Icon::new(SIDE, self.points, self.ends)
            .expect("an icon's points are inside its viewbox and its contours closed")
    }
}

/// Splits the arc from direction `a` to direction `b`, going `turn`'s way
/// (the whole circle when they are the same), into pieces of at most 50°,
/// appended to `pieces` in order. `depth` bounds the halving: an arc is a
/// whole circle at most, which 3 halvings take to 45°.
fn split(a: V, b: V, turn: Turn, depth: u32, pieces: &mut Vec<(V, V)>) {
    let sine = turn.sign() * a.cross(b);
    let cosine = a.dot(b);
    if (sine > 0.0 && cosine >= COS_WIDEST_ARC) || depth >= 8 {
        pieces.push((a, b));
        return;
    }
    let middle = if sine > 0.0 {
        // Under a half turn: half-way between them.
        (a + b).unit()
    } else if sine < 0.0 {
        // Over a half turn: half-way round the long way.
        -(a + b).unit()
    } else if cosine < 0.0 {
        // A half turn.
        a.quarter(turn)
    } else {
        // The whole circle.
        -a
    };
    split(a, middle, turn, depth + 1, pieces);
    split(middle, b, turn, depth + 1, pieces);
}

#[cfg(test)]
mod tests {
    use tidepool_font::outline::Bitmap;

    use super::*;

    /// `icon`'s coverage drawn `pixels` across, as a whole square of
    /// `pixels × pixels` bytes, checking that the bitmap lies inside it.
    fn coverage(icon: &Icon, pixels: i32) -> Vec<u8> {
        let bitmap: Bitmap = icon.rasterize(pixels).expect("within font's limits");
        let (width, height) = (
            i32::try_from(bitmap.width).expect("a width"),
            i32::try_from(bitmap.height).expect("a height"),
        );
        assert!(
            bitmap.left >= 0
                && bitmap.top >= 0
                && bitmap.left + width <= pixels
                && bitmap.top + height <= pixels,
            "{bitmap:?} inside {pixels} px"
        );
        let side = usize::try_from(pixels).expect("positive");
        let mut square = vec![0; side * side];
        for row in 0..bitmap.height {
            for column in 0..bitmap.width {
                let x = usize::try_from(bitmap.left).expect("inside") + column as usize;
                let y = usize::try_from(bitmap.top).expect("inside") + row as usize;
                square[y * side + x] = bitmap.get(column, row).expect("in the bitmap");
            }
        }
        square
    }

    #[test]
    fn every_icon_builds_in_a_24_unit_viewbox_with_its_own_name() {
        let mut names = Vec::new();
        for (name, make) in ALL {
            let icon = make();
            assert_eq!(icon.side(), SIDE, "{name}");
            assert!(icon.point_count() > 0 && icon.contour_count() > 1, "{name}");
            assert_eq!(make(), icon, "{name} is the same each time");
            names.push(name);
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn every_icon_inks_inside_its_viewbox_at_scales_1_1_5_and_2() {
        // Drawn 24 logical pixels across.
        for (name, make) in ALL {
            let icon = make();
            for pixels in [24, 36, 48] {
                let square = coverage(&icon, pixels);
                assert!(
                    square.contains(&u8::MAX),
                    "{name} at {pixels} px has fully covered pixels"
                );
            }
        }
    }

    #[test]
    fn no_two_icons_are_the_same() {
        let drawn: Vec<_> = ALL
            .iter()
            .map(|(name, make)| (name, make(), coverage(&make(), 24)))
            .collect();
        for (i, (a, icon_a, square_a)) in drawn.iter().enumerate() {
            for (b, icon_b, square_b) in &drawn[i + 1..] {
                assert_ne!(icon_a, icon_b, "{a} and {b}");
                assert_ne!(square_a, square_b, "{a} and {b} at 24 px");
            }
        }
    }

    #[test]
    fn every_icon_is_line_art_not_a_solid_block() {
        // Pen strokes and holes: between 5% and 45% of the 24 × 24 pixels
        // are fully covered, and at least a third of them are empty.
        for (name, make) in ALL {
            let square = coverage(&make(), 24);
            let full = square.iter().filter(|&&c| c == u8::MAX).count();
            let empty = square.iter().filter(|&&c| c == 0).count();
            assert!(
                (29..=259).contains(&full),
                "{name}: {full} of 576 pixels full"
            );
            assert!(empty >= 192, "{name}: {empty} of 576 pixels empty");
        }
    }

    #[test]
    fn strokes_and_holes_land_where_each_icon_draws_them() {
        // At 24 px, a pixel each icon's pen fully covers, and one in a hole
        // or a gap that it leaves empty.
        let cases = [
            // The page's left side; the page below its lines.
            ((4, 12), (12, 18)),
            // The ring's left side; the lens's center.
            ((3, 10), (10, 10)),
            // The trunk; the top-left ring's hole.
            ((6, 12), (6, 4)),
            // The handle's middle; the jaw's slot.
            ((8, 15), (18, 4)),
            // The notch; the screen between the notch and the bar.
            ((11, 4), (12, 10)),
            // The left point's arm; the hole at the center.
            ((5, 11), (11, 11)),
        ];
        for ((name, make), ((x, y), (hole_x, hole_y))) in ALL.into_iter().zip(cases) {
            let square = coverage(&make(), 24);
            assert_eq!(square[y * 24 + x], u8::MAX, "{name}: ({x}, {y}) is drawn");
            assert_eq!(
                square[hole_y * 24 + hole_x],
                0,
                "{name}: ({hole_x}, {hole_y}) is empty"
            );
        }
    }

    #[test]
    fn straight_edges_on_even_units_are_crisp_at_scale_1_5() {
        // Drawn 36 px across, a unit is 1.5 px: the page's left side, x 4 to
        // 6 units, covers columns 6 to 8 whole and nothing either side; the
        // phone's gesture bar, y 16 to 18, rows 24 to 26.
        let page = coverage(&files(), 36);
        let row = 18 * 36;
        assert_eq!(page[row + 5..row + 10], [0, 255, 255, 255, 0]);
        let phone = coverage(&device(), 36);
        let column = |y: usize| phone[y * 36 + 18];
        assert_eq!([23, 24, 25, 26, 27].map(column), [0, 255, 255, 255, 0]);
    }

    #[test]
    fn arcs_are_split_into_pieces_of_at_most_50_degrees() {
        let east = V::new(1.0, 0.0);
        let mut pieces = Vec::new();
        split(east, east, Turn::Clockwise, 0, &mut pieces);
        assert_eq!(pieces.len(), 8, "a whole circle, in eighths");
        assert_eq!(pieces[0].0, east);
        assert_eq!(pieces[7].1, east);
        // Clockwise on screen goes from right to down; the other way, up.
        assert!(pieces[0].1.y > 0.0);
        pieces.clear();
        split(east, east, Turn::Counterclockwise, 0, &mut pieces);
        assert!(pieces[0].1.y < 0.0);
        // A half turn and a short arc.
        pieces.clear();
        split(east, -east, Turn::Clockwise, 0, &mut pieces);
        assert_eq!(pieces.len(), 4);
        pieces.clear();
        let down = V::new(0.0, 1.0);
        split(east, down, Turn::Clockwise, 0, &mut pieces);
        assert_eq!(pieces.len(), 2);
        // The long way round, three quarters: halved to 135°, 67.5° and
        // then 33.75°.
        pieces.clear();
        split(east, down, Turn::Counterclockwise, 0, &mut pieces);
        assert_eq!(pieces.len(), 8);
        for (a, b) in pieces {
            assert!(a.dot(b) >= COS_WIDEST_ARC);
        }
    }
}
