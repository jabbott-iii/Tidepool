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

//! The backdrop's two SVG drawings (`svg.space__art` on jabbott.dev): the
//! [`Drawing`] type and its parts, and their painting at a window's size,
//! which the [backdrop's module documentation](super#the-drawings)
//! describes.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! A drawing is prepared once per window size ([`ArtPaint::new`]): each
//! shape becomes one or two elements (its fill, then its stroke), each a
//! list of simple pieces in device pixels whose signed distance is known
//! (a straight piece of a stroke, a disc, a ring, a rounded rect), with
//! the pixels each piece can touch. [`ArtPaint::fill_window`] then works
//! out the drawing's alpha in any window of pixels, a band of rows at a
//! time as the backdrop renders, or one pixel; the result at a pixel does
//! not depend on the window, since every piece is evaluated at the pixel's
//! centre alone.

use tidepool_core::color::Color;
use tidepool_core::geom::DeviceSize;

use super::{Rgba, Rule, check, premultiply};
use crate::GfxError;
use crate::clip::Edges;

#[cfg(test)]
mod tests;

/// Which edge of the window a [`Drawing`] stands against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The left edge (`left: 0`), with the view box's left edge on the
    /// box's (`preserveAspectRatio="xMinYMid slice"`).
    Left,
    /// The right edge (`right: 0`), with the view box's right edge on the
    /// box's (`xMaxYMid slice`).
    Right,
}

/// An SVG `viewBox`: the rectangle of user units (the drawing's own
/// coordinates) that a [`Drawing`]'s box shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBox {
    /// Its left edge, in user units.
    pub x: f32,
    /// Its top edge, in user units.
    pub y: f32,
    /// Its width, in user units; a drawing whose view box has no width or
    /// height draws nothing, as in SVG.
    pub width: f32,
    /// Its height, in user units.
    pub height: f32,
}

/// One command of an SVG path, absolute, in user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    /// `M x y`: starts a new subpath at (`x`, `y`).
    Move {
        /// The point's x.
        x: f32,
        /// The point's y.
        y: f32,
    },
    /// `L x y`: a straight line to (`x`, `y`). SVG's `H x` is a line to
    /// `x` at the same y.
    Line {
        /// The end's x.
        x: f32,
        /// The end's y.
        y: f32,
    },
    /// `Q x1 y1 x y`: a quadratic Bézier curve through the control point
    /// (`x1`, `y1`) to (`x`, `y`).
    Quad {
        /// The control point's x.
        x1: f32,
        /// The control point's y.
        y1: f32,
        /// The end's x.
        x: f32,
        /// The end's y.
        y: f32,
    },
    /// `C x1 y1 x2 y2 x y`: a cubic Bézier curve through the control points
    /// (`x1`, `y1`) and (`x2`, `y2`) to (`x`, `y`).
    Cubic {
        /// The first control point's x.
        x1: f32,
        /// The first control point's y.
        y1: f32,
        /// The second control point's x.
        x2: f32,
        /// The second control point's y.
        y2: f32,
        /// The end's x.
        x: f32,
        /// The end's y.
        y: f32,
    },
}

/// What a [`Shape`] is, in user units.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// `<path d="…">`. It is only stroked: its fill is not drawn, since
    /// none of the site's paths encloses anything (the chip's pins, which
    /// the site's CSS fills, are straight lines). A command before the
    /// first [`PathCommand::Move`] is skipped.
    Path(Vec<PathCommand>),
    /// `<circle>`: its centre and radius; a radius of 0 draws nothing.
    Circle {
        /// The centre's x.
        cx: f32,
        /// The centre's y.
        cy: f32,
        /// The radius.
        r: f32,
    },
    /// `<rect>` with corners rounded by `rx` (and the same `ry`), which is
    /// cut to half the shorter side; a width or height of 0 draws
    /// nothing.
    Rect {
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The corners' radius.
        rx: f32,
    },
}

/// How a [`Shape`] is filled, in its drawing's color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    /// `fill: none`.
    None,
    /// The color at `opacity`, from 0 to 1: the `fill-opacity` times the
    /// alpha the CSS gives the color (`color-mix(in srgb, var(--art) 6%,
    /// transparent)` is 0.06).
    Solid {
        /// The fill's opacity.
        opacity: f32,
    },
    /// A `radialGradient` across the shape's bounding box (SVG's
    /// defaults: centred, with a radius of half the box each way): the
    /// color at `center` opacity at its centre (its first stop), fading
    /// linearly to transparent at the box's edge (its last), all at
    /// `opacity` (the shape's `fill-opacity`). Both are from 0 to 1.
    Radial {
        /// The `fill-opacity`.
        opacity: f32,
        /// The `stop-opacity` at the centre.
        center: f32,
    },
}

/// The ends of a stroke's open subpaths (`stroke-linecap`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cap {
    /// `butt`, SVG's default: the stroke ends square at the end point.
    Butt,
    /// `round`: a half disc past the end point, half the width across.
    Round,
}

/// How a [`Shape`] is stroked, in its drawing's color. Corners are always
/// joined round (`stroke-linejoin: round`, as the site's traces are); the
/// site's other paths have no corners, each being one line or one curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    /// The width, in user units, scaled with the drawing.
    pub width: f32,
    /// The opacity, from 0 to 1 (`stroke-opacity`).
    pub opacity: f32,
    /// The ends of open subpaths.
    pub cap: Cap,
}

/// One element of a [`Drawing`]: its geometry, filled and then stroked, as
/// SVG paints it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// What it is.
    pub geometry: Geometry,
    /// Its fill, painted first.
    pub fill: Fill,
    /// Its stroke, painted over the fill, if it has one.
    pub stroke: Option<Stroke>,
}

/// One of the backdrop's SVG drawings (`svg.space__art`): shapes in one
/// color, in a box as tall as the window at one of its sides, faded toward
/// the middle by a mask. The [backdrop's module
/// documentation](super#the-drawings) says how it is sized and painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// The color every shape is painted in (`--art`), opaque in the site's
    /// drawings; each shape's [`Fill`] and [`Stroke`] give its opacities.
    /// A translucent color's alpha multiplies them, so each shape is
    /// painted in the translucent color over the ones before it, as SVG
    /// paints it.
    pub color: Color,
    /// The side of the window the box stands against.
    pub side: Side,
    /// The user units the box shows. The box is as tall as the window and
    /// as wide as the view box's shape makes it (the site's `aspect-ratio:
    /// 2 / 5` is its view box's, 400 × 1000), up to
    /// [`Drawing::max_width`].
    pub view_box: ViewBox,
    /// The box's greatest width, as a fraction of the window's width
    /// (`max-width: 40vw` is 0.4).
    pub max_width: f32,
    /// How much of the box, as a fraction of its width from its side, the
    /// mask keeps whole, from 0 to 1 (`linear-gradient(90deg, #000 40%,
    /// transparent)` is 0.4); from there it fades linearly to transparent
    /// at the box's other edge.
    pub mask_solid: f32,
    /// The shapes, in the order they are painted: each over the ones
    /// before it.
    pub shapes: Vec<Shape>,
}

/// The names [`Drawing::validate`] gives the fields of one drawing.
pub(super) struct Fields {
    pub(super) view_box_x: &'static str,
    pub(super) view_box_y: &'static str,
    pub(super) view_box_width: &'static str,
    pub(super) view_box_height: &'static str,
    pub(super) max_width: &'static str,
    pub(super) mask_solid: &'static str,
    pub(super) shapes: &'static str,
    pub(super) geometry: &'static str,
    pub(super) fill: &'static str,
    pub(super) stroke_width: &'static str,
    pub(super) stroke_opacity: &'static str,
}

/// The names of [`BackdropStyle::circuits`](super::BackdropStyle::circuits)'s fields.
pub(super) const CIRCUITS: Fields = Fields {
    view_box_x: "circuits.view_box.x",
    view_box_y: "circuits.view_box.y",
    view_box_width: "circuits.view_box.width",
    view_box_height: "circuits.view_box.height",
    max_width: "circuits.max_width",
    mask_solid: "circuits.mask_solid",
    shapes: "circuits.shapes",
    geometry: "circuits.shapes.geometry",
    fill: "circuits.shapes.fill",
    stroke_width: "circuits.shapes.stroke.width",
    stroke_opacity: "circuits.shapes.stroke.opacity",
};

/// The names of [`BackdropStyle::synapses`](super::BackdropStyle::synapses)'s fields.
pub(super) const SYNAPSES: Fields = Fields {
    view_box_x: "synapses.view_box.x",
    view_box_y: "synapses.view_box.y",
    view_box_width: "synapses.view_box.width",
    view_box_height: "synapses.view_box.height",
    max_width: "synapses.max_width",
    mask_solid: "synapses.mask_solid",
    shapes: "synapses.shapes",
    geometry: "synapses.shapes.geometry",
    fill: "synapses.shapes.fill",
    stroke_width: "synapses.shapes.stroke.width",
    stroke_opacity: "synapses.shapes.stroke.opacity",
};

impl Drawing {
    /// Checks every value and limit as [`BackdropStyle::validate`]
    /// describes, naming fields as `fields` does; a shape's field carries
    /// the shape's index.
    ///
    /// [`BackdropStyle::validate`]: super::BackdropStyle::validate
    pub(super) fn validate(&self, fields: &Fields) -> Result<(), GfxError> {
        use Rule::{Finite, NonNegative, Opacity};
        let view_box = &self.view_box;
        for (value, field, rule) in [
            (view_box.x, fields.view_box_x, Finite),
            (view_box.y, fields.view_box_y, Finite),
            (view_box.width, fields.view_box_width, NonNegative),
            (view_box.height, fields.view_box_height, NonNegative),
            (self.max_width, fields.max_width, NonNegative),
            (self.mask_solid, fields.mask_solid, Opacity),
        ] {
            check(value, field, None, rule)?;
        }
        let over = |field, index| Err(GfxError::InvalidStyle { field, index });
        if self.shapes.len() > MAX_SHAPES {
            return over(fields.shapes, Some(MAX_SHAPES));
        }
        let widest = view_box.width.max(view_box.height);
        let mut commands_in_all = 0_usize;
        for (index, shape) in self.shapes.iter().enumerate() {
            let index = Some(index);
            let geometry = |value, rule| check(value, fields.geometry, index, rule);
            match &shape.geometry {
                Geometry::Path(commands) => {
                    commands_in_all = commands_in_all.saturating_add(commands.len());
                    if commands.len() > MAX_PATH_COMMANDS || commands_in_all > MAX_DRAWING_COMMANDS
                    {
                        return over(fields.geometry, index);
                    }
                    for command in commands {
                        for value in command.values() {
                            geometry(value, Finite)?;
                        }
                    }
                }
                &Geometry::Circle { cx, cy, r } => {
                    geometry(cx, Finite)?;
                    geometry(cy, Finite)?;
                    geometry(r, NonNegative)?;
                }
                &Geometry::Rect {
                    x,
                    y,
                    width,
                    height,
                    rx,
                } => {
                    geometry(x, Finite)?;
                    geometry(y, Finite)?;
                    geometry(width, NonNegative)?;
                    geometry(height, NonNegative)?;
                    geometry(rx, NonNegative)?;
                }
            }
            match shape.fill {
                Fill::None => {}
                Fill::Solid { opacity } => check(opacity, fields.fill, index, Opacity)?,
                Fill::Radial { opacity, center } => {
                    check(opacity, fields.fill, index, Opacity)?;
                    check(center, fields.fill, index, Opacity)?;
                }
            }
            if let Some(stroke) = &shape.stroke {
                check(stroke.width, fields.stroke_width, index, NonNegative)?;
                if stroke.width > widest {
                    return over(fields.stroke_width, index);
                }
                check(stroke.opacity, fields.stroke_opacity, index, Opacity)?;
            }
        }
        Ok(())
    }
}

impl PathCommand {
    /// Every coordinate of the command.
    fn values(self) -> impl Iterator<Item = f32> {
        let (values, len) = match self {
            Self::Move { x, y } | Self::Line { x, y } => ([x, y, 0.0, 0.0, 0.0, 0.0], 2),
            Self::Quad { x1, y1, x, y } => ([x1, y1, x, y, 0.0, 0.0], 4),
            Self::Cubic {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => ([x1, y1, x2, y2, x, y], 6),
        };
        values.into_iter().take(len)
    }
}

/// The most shapes a [`Drawing`] may have: 1,024, where the site's have 62
/// and 106.
pub const MAX_SHAPES: usize = 1024;

/// The most commands one path may have: 256, where the site's have at
/// most 6.
pub const MAX_PATH_COMMANDS: usize = 256;

/// The most path commands a [`Drawing`] may have in all: 4,096, where the
/// site's have 106 and 212.
pub const MAX_DRAWING_COMMANDS: usize = 4096;

/// The most a curve may be from the lines it is flattened into, in device
/// pixels: 1/32, as `tidepool-font` flattens glyphs.
const TOLERANCE: f64 = 1.0 / 32.0;

/// The most lines one curve is flattened into. At [`TOLERANCE`], a
/// quadratic curve needs 512 when its `|p0 − 2·p1 + p2|` is 32,768 px, and
/// a cubic one when its larger second difference is about 10,923 px: far
/// beyond the site's drawings in a window of `MAX_DIMENSION` px.
const MAX_SEGMENTS: u32 = 512;

/// The most pieces a drawing is prepared into at one size: 65,536, 7.5 MiB
/// of them on a 64-bit CPU.
pub const MAX_PIECES: usize = 1 << 16;

/// The coverage tests a drawing may need at one size, per pixel of its box:
/// 64. Each piece tests the pixels it may reach, and each element (a
/// shape's fill or stroke) once more the pixels of the rect that holds its
/// pieces.
pub const MAX_TESTS_PER_PIXEL: u64 = 64;

/// The coverage tests a drawing may need at any size, however small its
/// box: 2^20.
pub const MIN_TESTS: u64 = 1 << 20;

/// The coverage tests a drawing whose box covers `window` may need:
/// [`MAX_TESTS_PER_PIXEL`] a pixel, and at least [`MIN_TESTS`].
fn test_budget(window: Edges) -> u64 {
    let side = |length: i64| u64::try_from(length).unwrap_or(u64::MAX);
    MAX_TESTS_PER_PIXEL
        .saturating_mul(side(window.width()).saturating_mul(side(window.height())))
        .max(MIN_TESTS)
}

/// How far past a piece's shape, in pixels, the pixels it may touch reach:
/// the ramp's half pixel, half a pixel from a pixel's centre to its edge,
/// and half a pixel to spare.
const MARGIN: f64 = 1.5;

/// The shape of one piece, in device pixels, by its signed distance: below
/// 0 inside, above outside.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sdf {
    /// A straight piece of a stroke, from (`ax`, `ay`) to (`bx`, `by`),
    /// `half` across each way, its ends square: a rect along the line,
    /// centred on (`mx`, `my`), in the direction (`ux`, `uy`) of length 1,
    /// `half_length` each way.
    Segment {
        ax: f64,
        ay: f64,
        bx: f64,
        by: f64,
        mx: f64,
        my: f64,
        ux: f64,
        uy: f64,
        half_length: f64,
        half: f64,
    },
    /// A disc of radius `r` centred on (`x`, `y`).
    Disc { x: f64, y: f64, r: f64 },
    /// A ring `half` each way across a circle of radius `r` centred on
    /// (`x`, `y`).
    Ring { x: f64, y: f64, r: f64, half: f64 },
    /// A rect centred on (`x`, `y`), `hx` and `hy` each way, its corners
    /// rounded with radius `r`.
    RoundRect {
        x: f64,
        y: f64,
        hx: f64,
        hy: f64,
        r: f64,
    },
    /// A band `half` each way across a [`Sdf::RoundRect`]'s edge.
    RoundRectRing {
        x: f64,
        y: f64,
        hx: f64,
        hy: f64,
        r: f64,
        half: f64,
    },
}

impl Sdf {
    /// A straight piece from `a` to `b`, `half` across each way, or `None`
    /// if it has no length.
    fn segment(a: (f64, f64), b: (f64, f64), half: f64) -> Option<Self> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt();
        if !(length > 0.0 && length.is_finite()) {
            return None;
        }
        Some(Self::Segment {
            ax: a.0,
            ay: a.1,
            bx: b.0,
            by: b.1,
            mx: (a.0 + b.0) / 2.0,
            my: (a.1 + b.1) / 2.0,
            ux: dx / length,
            uy: dy / length,
            half_length: length / 2.0,
            half,
        })
    }

    /// The signed distance from (`px`, `py`) to the shape's edge.
    fn distance(&self, px: f64, py: f64) -> f64 {
        match *self {
            Self::Segment {
                mx,
                my,
                ux,
                uy,
                half_length,
                half,
                ..
            } => {
                let (dx, dy) = (px - mx, py - my);
                let along = (dx * ux + dy * uy).abs() - half_length;
                let across = (dy * ux - dx * uy).abs() - half;
                outside_and_inside(along, across)
            }
            Self::Disc { x, y, r } => length(px - x, py - y) - r,
            Self::Ring { x, y, r, half } => (length(px - x, py - y) - r).abs() - half,
            Self::RoundRect { x, y, hx, hy, r } => round_rect(px - x, py - y, hx, hy, r),
            Self::RoundRectRing {
                x,
                y,
                hx,
                hy,
                r,
                half,
            } => round_rect(px - x, py - y, hx, hy, r).abs() - half,
        }
    }

    /// How much of the pixel centred on (`px`, `py`) the shape covers: the
    /// 1 px ramp `clamp(0.5 − distance, 0, 1)`.
    fn coverage(&self, px: f64, py: f64) -> f64 {
        ramp(self.distance(px, py))
    }

    /// The smallest box holding the shape: left, top, right, bottom.
    fn extent(&self) -> (f64, f64, f64, f64) {
        match *self {
            Self::Segment {
                ax,
                ay,
                bx,
                by,
                half,
                ..
            } => (
                ax.min(bx) - half,
                ay.min(by) - half,
                ax.max(bx) + half,
                ay.max(by) + half,
            ),
            Self::Disc { x, y, r } => (x - r, y - r, x + r, y + r),
            Self::Ring { x, y, r, half } => {
                let reach = r + half;
                (x - reach, y - reach, x + reach, y + reach)
            }
            Self::RoundRect { x, y, hx, hy, .. } => (x - hx, y - hy, x + hx, y + hy),
            Self::RoundRectRing {
                x, y, hx, hy, half, ..
            } => (x - hx - half, y - hy - half, x + hx + half, y + hy + half),
        }
    }

    /// The pixels the shape may cover: every pixel whose coverage is not 0
    /// is inside, with room to spare.
    fn bounds(&self) -> Edges {
        let (left, top, right, bottom) = self.extent();
        // The casts saturate, so a shape far outside the window still
        // gives bounds that clip correctly.
        Edges {
            left: (left - MARGIN).floor() as i64,
            top: (top - MARGIN).floor() as i64,
            right: (right + MARGIN).ceil() as i64,
            bottom: (bottom + MARGIN).ceil() as i64,
        }
    }
}

/// `sqrt(x² + y²)`.
fn length(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// The signed distance to a box's edge from a point `along` and `across`
/// past its sides (negative inside).
fn outside_and_inside(along: f64, across: f64) -> f64 {
    length(along.max(0.0), across.max(0.0)) + along.max(across).min(0.0)
}

/// The signed distance from (`dx`, `dy`), relative to the centre, to the
/// edge of a rect `hx` and `hy` each way with corners of radius `r`.
fn round_rect(dx: f64, dy: f64, hx: f64, hy: f64, r: f64) -> f64 {
    outside_and_inside(dx.abs() - (hx - r), dy.abs() - (hy - r)) - r
}

/// The 1 px coverage ramp of a signed distance: `clamp(0.5 − d, 0, 1)`, 0
/// for NaN.
fn ramp(distance: f64) -> f64 {
    let coverage = 0.5 - distance;
    if coverage >= 1.0 {
        1.0
    } else if coverage > 0.0 {
        coverage
    } else {
        0.0
    }
}

/// A piece of an element, with the pixels it may cover.
#[derive(Debug)]
struct Piece {
    bounds: Edges,
    sdf: Sdf,
}

impl Piece {
    /// The columns, between `left` and `right`, in which the piece may
    /// cover pixels of the row centred on `centre_y`: for a slanted
    /// straight piece, those within `(half + MARGIN) / |uy|` across of its
    /// line, which hold every pixel it covers; for the others, all.
    fn span(&self, centre_y: f64, left: i64, right: i64) -> (i64, i64) {
        if let Sdf::Segment {
            mx,
            my,
            ux,
            uy,
            half,
            ..
        } = self.sdf
            && uy != 0.0
        {
            let reach = (half + MARGIN) / uy.abs();
            let middle = mx + (centre_y - my) * ux / uy;
            if reach.is_finite() && middle.is_finite() {
                // A pixel's centre is half a pixel right of its column.
                let from = ((middle - reach - 0.5).floor() as i64).max(left);
                let to = ((middle + reach).ceil() as i64)
                    .saturating_add(1)
                    .min(right);
                return (from, to.max(from));
            }
        }
        (left, right)
    }
}

/// What an element's coverage is.
#[derive(Debug)]
enum Kind {
    /// The union of the pieces: at each pixel, the largest of their
    /// coverages.
    Union(Vec<Piece>),
    /// A shape filled with a radial gradient: its coverage times `center ×
    /// (1 − t)` (0 past the edge), for `t` the pixel centre's distance
    /// from (`x`, `y`) in radii `rx` and `ry`.
    Gradient {
        shape: Sdf,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        center: f64,
    },
}

/// One fill or stroke of a shape, ready to paint.
#[derive(Debug)]
struct Element {
    /// The pixels it may cover, inside the drawing's box.
    bounds: Edges,
    /// The alpha it adds where it covers a pixel fully.
    alpha: f64,
    kind: Kind,
}

/// Where a drawing's user units land in device pixels: `x × k + tx` and
/// `y × k + ty`.
#[derive(Clone, Copy, Debug)]
struct Place {
    k: f64,
    tx: f64,
    ty: f64,
}

impl Place {
    fn point(self, x: f32, y: f32) -> (f64, f64) {
        (
            f64::from(x) * self.k + self.tx,
            f64::from(y) * self.k + self.ty,
        )
    }
}

/// A stroke `width` device pixels wide as the half width its pieces are
/// drawn with and the factor its opacity is scaled by: itself and 1, or,
/// under 1 px, a 1 px stroke at the opacity times its width, which leaves
/// a 1 px stroke's ink times the width (exact along the rows or the
/// columns, right on average when slanted, as the
/// [backdrop's module documentation](super#approximations) says).
fn stroke_half(width: f64) -> (f64, f64) {
    if width >= 1.0 {
        (width / 2.0, 1.0)
    } else {
        (0.5, width.max(0.0))
    }
}

/// How many lines a curve is flattened into so that no point of it is more
/// than [`TOLERANCE`] from them: `ceil(sqrt(deviation / (4 ×
/// TOLERANCE)))`, from 1 to [`MAX_SEGMENTS`].
///
/// With `n` equal steps of the parameter, a curve is at most `max|B″| /
/// (8n²)` from its lines, `B″` being its second derivative. A quadratic
/// curve's is twice its second difference, `|p0 − 2·p1 + p2|`, which it
/// passes as `deviation`; a cubic curve's is at most 6 times the larger of
/// its two, so it passes 3 times that.
fn segment_count(deviation: f64) -> u32 {
    let count = (deviation / (4.0 * TOLERANCE)).sqrt().ceil();
    if count >= f64::from(MAX_SEGMENTS) {
        MAX_SEGMENTS
    } else if count >= 1.0 {
        count as u32
    } else {
        1
    }
}

/// Adds to `points` the points a quadratic curve from `p0` through `p1` to
/// `p2` is flattened into, at equal steps of its parameter, after `p0`:
/// the last is `p2` exactly.
fn flatten_quad(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    points: &mut Vec<(f64, f64)>,
) -> Result<(), GfxError> {
    let count = segment_count(length(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1));
    reserve(points, count)?;
    for step in 1..count {
        let t = f64::from(step) / f64::from(count);
        let u = 1.0 - t;
        let (a, b, c) = (u * u, 2.0 * u * t, t * t);
        points.push((
            a * p0.0 + b * p1.0 + c * p2.0,
            a * p0.1 + b * p1.1 + c * p2.1,
        ));
    }
    points.push(p2);
    Ok(())
}

/// Adds to `points` the points a cubic curve from `p0` through `p1` and
/// `p2` to `p3` is flattened into, at equal steps of its parameter, after
/// `p0`: the last is `p3` exactly.
fn flatten_cubic(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    points: &mut Vec<(f64, f64)>,
) -> Result<(), GfxError> {
    let first = length(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1);
    let second = length(p1.0 - 2.0 * p2.0 + p3.0, p1.1 - 2.0 * p2.1 + p3.1);
    let count = segment_count(3.0 * first.max(second));
    reserve(points, count)?;
    for step in 1..count {
        let t = f64::from(step) / f64::from(count);
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        points.push((
            a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
            a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
        ));
    }
    points.push(p3);
    Ok(())
}

/// Makes room in `values` for `count` more.
fn reserve<T>(values: &mut Vec<T>, count: impl TryInto<usize>) -> Result<(), GfxError> {
    let count = count.try_into().unwrap_or(usize::MAX);
    values
        .try_reserve(count)
        .map_err(|_| GfxError::OutOfMemory {
            bytes: count.saturating_mul(size_of::<T>()),
        })
}

/// The smallest edges holding both.
fn union(a: Edges, b: Edges) -> Edges {
    Edges {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// A [`Drawing`] at one window size and scale, ready to paint.
#[derive(Debug)]
pub(super) struct ArtPaint {
    /// The drawing's color, opaque and premultiplied: its alpha is in
    /// each element's.
    pub(super) color: Rgba,
    side: Side,
    /// The box's left and right edges, and its width, in device pixels.
    left: f64,
    right: f64,
    width: f64,
    solid: f64,
    /// The drawing's opacity: 1, or the narrow window's.
    pub(super) opacity: f64,
    /// The columns of the pixels whose centres are in the box, and the
    /// window's rows: what the drawing may paint.
    pub(super) window: Edges,
    elements: Vec<Element>,
    /// The color's alpha, which each element's alpha is multiplied by.
    color_alpha: f64,
    /// What preparing the drawing may still spend.
    budget: Budget,
    /// The style's name of the drawing's shapes, and the shape being
    /// prepared: what the error names when the budget runs out.
    shapes_field: &'static str,
    shape: usize,
}

/// What preparing a drawing may still spend: pieces kept, and coverage
/// tests, the pixels they and the elements reach.
#[derive(Debug)]
struct Budget {
    pieces: usize,
    tests: u64,
}

impl ArtPaint {
    /// `drawing` in a window `size` device pixels, at `opacity` (the
    /// narrow window's, or 1), whose fields `fields` names.
    ///
    /// # Errors
    ///
    /// [`GfxError::InvalidStyle`] naming the shape at which the drawing
    /// needs more than [`MAX_PIECES`] pieces, or more coverage tests than
    /// its budget allows; [`GfxError::OutOfMemory`].
    pub(super) fn new(
        drawing: &Drawing,
        fields: &Fields,
        size: DeviceSize,
        opacity: f64,
    ) -> Result<Self, GfxError> {
        let (w, h) = (f64::from(size.width.max(0)), f64::from(size.height.max(0)));
        let view_box = drawing.view_box;
        let (vx, vy) = (f64::from(view_box.x), f64::from(view_box.y));
        let (vw, vh) = (f64::from(view_box.width), f64::from(view_box.height));
        // As tall as the window, with the view box's shape, but no wider
        // than `max_width` of the window.
        let width = if vw > 0.0 && vh > 0.0 {
            (h * vw / vh).min(f64::from(drawing.max_width) * w)
        } else {
            0.0
        };
        let (left, right) = match drawing.side {
            Side::Left => (0.0, width),
            Side::Right => (w - width, w),
        };
        let color = drawing.color;
        let mut paint = Self {
            color: premultiply(Color::rgb(color.r, color.g, color.b)),
            side: drawing.side,
            left,
            right,
            width,
            solid: f64::from(drawing.mask_solid),
            opacity,
            window: Edges {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            elements: Vec::new(),
            color_alpha: f64::from(color.a) / 255.0,
            budget: Budget {
                pieces: MAX_PIECES,
                tests: 0,
            },
            shapes_field: fields.shapes,
            shape: 0,
        };
        if !(width > 0.0 && h > 0.0) {
            return Ok(paint);
        }
        // `slice` scales the view box to cover the box: by the larger of
        // the box's width and height over the view box's. The box is as
        // tall as the window and never wider than the view box's shape, so
        // that is the height's, and `YMid` has nothing to centre. `xMin`
        // or `xMax` puts the view box's side on the box's.
        let k = h / vh;
        let tx = match drawing.side {
            Side::Left => left - vx * k,
            Side::Right => right - (vx + vw) * k,
        };
        let place = Place { k, tx, ty: -vy * k };
        // A pixel is in the box when its centre is: from the first column
        // whose centre is at or right of the left edge to the last left of
        // the right edge. The bounds are within [0, w], so the casts are
        // exact.
        let column = |edge: f64| (edge - 0.5).ceil().clamp(0.0, w) as i64;
        paint.window = Edges {
            left: column(left),
            top: 0,
            right: column(right),
            bottom: size.height.max(0).into(),
        };
        if paint.window.is_empty() {
            return Ok(paint);
        }
        paint.budget.tests = test_budget(paint.window);
        reserve(&mut paint.elements, drawing.shapes.len().saturating_mul(2))?;
        for (index, shape) in drawing.shapes.iter().enumerate() {
            paint.shape = index;
            paint.add_shape(shape, place)?;
        }
        Ok(paint)
    }

    /// Adds the elements of `shape`: its fill, then its stroke.
    fn add_shape(&mut self, shape: &Shape, place: Place) -> Result<(), GfxError> {
        let k = place.k;
        let stroke = shape
            .stroke
            .filter(|stroke| stroke.width > 0.0 && stroke.opacity > 0.0)
            .map(|stroke| {
                let (half, scale) = stroke_half(f64::from(stroke.width) * k);
                (half, f64::from(stroke.opacity) * scale, stroke.cap)
            });
        match &shape.geometry {
            Geometry::Path(commands) => {
                if let Some((half, alpha, cap)) = stroke {
                    let pieces = self.stroke_path(commands, place, half, cap)?;
                    self.push(alpha, Kind::Union(pieces))?;
                }
            }
            &Geometry::Circle { cx, cy, r } => {
                if r <= 0.0 {
                    return Ok(());
                }
                let (x, y) = place.point(cx, cy);
                let r = f64::from(r) * k;
                self.add_fill(shape.fill, Sdf::Disc { x, y, r }, (x, y, r, r))?;
                if let Some((half, alpha, _)) = stroke {
                    let ring = Sdf::Ring { x, y, r, half };
                    let pieces = self.pieces(&[ring])?;
                    self.push(alpha, Kind::Union(pieces))?;
                }
            }
            &Geometry::Rect {
                x,
                y,
                width,
                height,
                rx,
            } => {
                if width <= 0.0 || height <= 0.0 {
                    return Ok(());
                }
                let (left, top) = place.point(x, y);
                let (hx, hy) = (f64::from(width) * k / 2.0, f64::from(height) * k / 2.0);
                let (x, y) = (left + hx, top + hy);
                let r = (f64::from(rx) * k).min(hx).min(hy);
                let rect = Sdf::RoundRect { x, y, hx, hy, r };
                self.add_fill(shape.fill, rect, (x, y, hx, hy))?;
                if let Some((half, alpha, _)) = stroke {
                    let ring = Sdf::RoundRectRing {
                        x,
                        y,
                        hx,
                        hy,
                        r,
                        half,
                    };
                    let pieces = self.pieces(&[ring])?;
                    self.push(alpha, Kind::Union(pieces))?;
                }
            }
        }
        Ok(())
    }

    /// Adds the fill `fill` of `shape`, whose bounding box is centred on
    /// `(x, y)`, `rx` and `ry` each way.
    fn add_fill(
        &mut self,
        fill: Fill,
        shape: Sdf,
        bbox: (f64, f64, f64, f64),
    ) -> Result<(), GfxError> {
        let (x, y, rx, ry) = bbox;
        match fill {
            Fill::None => Ok(()),
            Fill::Solid { opacity } if opacity > 0.0 => {
                let pieces = self.pieces(&[shape])?;
                self.push(f64::from(opacity), Kind::Union(pieces))
            }
            Fill::Radial { opacity, center } if opacity > 0.0 && center > 0.0 => {
                let bounds = shape.bounds().intersect(self.window);
                if bounds.is_empty() {
                    return Ok(());
                }
                let kind = Kind::Gradient {
                    shape,
                    x,
                    y,
                    rx,
                    ry,
                    center: f64::from(center),
                };
                self.push_bounded(f64::from(opacity), bounds, kind)
            }
            Fill::Solid { .. } | Fill::Radial { .. } => Ok(()),
        }
    }

    /// The pieces of `shapes` that may cover a pixel in the box.
    fn pieces(&mut self, shapes: &[Sdf]) -> Result<Vec<Piece>, GfxError> {
        let mut pieces = Vec::new();
        for &sdf in shapes {
            self.add_piece(&mut pieces, sdf)?;
        }
        Ok(pieces)
    }

    /// Adds `sdf` to `pieces` if it may cover a pixel in the box, spending
    /// a piece and the pixels it reaches from the budget.
    fn add_piece(&mut self, pieces: &mut Vec<Piece>, sdf: Sdf) -> Result<(), GfxError> {
        let bounds = sdf.bounds().intersect(self.window);
        if bounds.is_empty() {
            return Ok(());
        }
        self.spend(1, bounds)?;
        reserve(pieces, 1_usize)?;
        pieces.push(Piece { bounds, sdf });
        Ok(())
    }

    /// The pieces of `commands`' stroke, `half` across each way, with `cap`
    /// at the ends of each subpath and round joins.
    fn stroke_path(
        &mut self,
        commands: &[PathCommand],
        place: Place,
        half: f64,
        cap: Cap,
    ) -> Result<Vec<Piece>, GfxError> {
        let mut pieces = Vec::new();
        let mut points: Vec<(f64, f64)> = Vec::new();
        let mut started = false;
        for &command in commands {
            match command {
                PathCommand::Move { x, y } => {
                    self.stroke_polyline(&points, half, cap, &mut pieces)?;
                    points.clear();
                    reserve(&mut points, 1_usize)?;
                    points.push(place.point(x, y));
                    started = true;
                }
                _ if !started => {}
                PathCommand::Line { x, y } => {
                    reserve(&mut points, 1_usize)?;
                    points.push(place.point(x, y));
                }
                PathCommand::Quad { x1, y1, x, y } => {
                    let from = points.last().copied().unwrap_or_default();
                    flatten_quad(from, place.point(x1, y1), place.point(x, y), &mut points)?;
                }
                PathCommand::Cubic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
                    let from = points.last().copied().unwrap_or_default();
                    flatten_cubic(
                        from,
                        place.point(x1, y1),
                        place.point(x2, y2),
                        place.point(x, y),
                        &mut points,
                    )?;
                }
            }
        }
        self.stroke_polyline(&points, half, cap, &mut pieces)?;
        Ok(pieces)
    }

    /// Adds to `pieces` the stroke of the open polyline through `points`: a
    /// straight piece for each line, a disc at each corner (a round join),
    /// and with round caps a disc at each end. A single point draws
    /// nothing.
    fn stroke_polyline(
        &mut self,
        points: &[(f64, f64)],
        half: f64,
        cap: Cap,
        pieces: &mut Vec<Piece>,
    ) -> Result<(), GfxError> {
        if points.len() < 2 {
            return Ok(());
        }
        for pair in points.windows(2) {
            if let &[a, b] = pair
                && let Some(segment) = Sdf::segment(a, b, half)
            {
                self.add_piece(pieces, segment)?;
            }
        }
        let inner = points.get(1..points.len() - 1).unwrap_or(&[]);
        for &(x, y) in inner {
            self.add_piece(pieces, Sdf::Disc { x, y, r: half })?;
        }
        if cap == Cap::Round {
            for &(x, y) in [points.first(), points.last()].into_iter().flatten() {
                self.add_piece(pieces, Sdf::Disc { x, y, r: half })?;
            }
        }
        Ok(())
    }

    /// Adds an element of `pieces` at `alpha`, if any piece may cover a
    /// pixel.
    fn push(&mut self, alpha: f64, kind: Kind) -> Result<(), GfxError> {
        let Kind::Union(pieces) = &kind else {
            return Ok(());
        };
        let Some(bounds) = pieces.iter().map(|piece| piece.bounds).reduce(union) else {
            return Ok(());
        };
        self.push_bounded(alpha, bounds, kind)
    }

    /// Adds an element at `alpha`, times the color's alpha, spending the
    /// pixels it reaches from the budget.
    fn push_bounded(&mut self, alpha: f64, bounds: Edges, kind: Kind) -> Result<(), GfxError> {
        let alpha = alpha * self.color_alpha;
        if alpha > 0.0 {
            self.spend(0, bounds)?;
            reserve(&mut self.elements, 1_usize)?;
            self.elements.push(Element {
                bounds,
                alpha,
                kind,
            });
        }
        Ok(())
    }

    /// Spends `pieces` pieces and a coverage test for each pixel of `area`,
    /// or fails naming the shape being prepared if the budget has not that
    /// much left.
    fn spend(&mut self, pieces: usize, area: Edges) -> Result<(), GfxError> {
        let side = |length: i64| u64::try_from(length).unwrap_or(u64::MAX);
        let tests = side(area.width()).saturating_mul(side(area.height()));
        match (
            self.budget.pieces.checked_sub(pieces),
            self.budget.tests.checked_sub(tests),
        ) {
            (Some(pieces), Some(tests)) => {
                self.budget = Budget { pieces, tests };
                Ok(())
            }
            _ => Err(GfxError::InvalidStyle {
                field: self.shapes_field,
                index: Some(self.shape),
            }),
        }
    }

    /// What is left of the budget, for tests.
    #[cfg(test)]
    pub(super) fn budget_left(&self) -> (usize, u64) {
        (self.budget.pieces, self.budget.tests)
    }

    /// The mask times the opacity at column `x`: 1 within
    /// [`Drawing::mask_solid`] of the box's side, fading linearly to 0 at
    /// its other edge, at the pixel's centre.
    pub(super) fn weight(&self, x: i64) -> f64 {
        let centre = x as f64 + 0.5;
        let t = match self.side {
            Side::Left => (centre - self.left) / self.width,
            Side::Right => (self.right - centre) / self.width,
        };
        let mask = if t <= self.solid {
            1.0
        } else {
            ((1.0 - t) / (1.0 - self.solid)).clamp(0.0, 1.0)
        };
        mask * self.opacity
    }

    /// Fills `alpha` with the drawing's alpha at each pixel of `window`,
    /// before its mask and opacity, row by row, `window.width()` to a row;
    /// `scratch` is room to work in. Both must hold the window's pixels:
    /// nothing is written otherwise.
    ///
    /// Each element, in order, adds its alpha `a` where it covers a pixel
    /// by `c` as SVG composites one shape over another: `a × c + A × (1 −
    /// a × c)`.
    pub(super) fn fill_window(&self, window: Edges, alpha: &mut [f64], scratch: &mut [f64]) {
        let (Ok(width), Ok(height)) = (
            usize::try_from(window.width()),
            usize::try_from(window.height()),
        ) else {
            return;
        };
        let area = width.saturating_mul(height);
        let (Some(alpha), Some(scratch)) = (alpha.get_mut(..area), scratch.get_mut(..area)) else {
            return;
        };
        alpha.fill(0.0);
        // The index of the pixel (x, y), which is inside the window.
        let at = |x: i64, y: i64| {
            let (Ok(column), Ok(row)) = (
                usize::try_from(x - window.left),
                usize::try_from(y - window.top),
            ) else {
                return usize::MAX;
            };
            row.saturating_mul(width).saturating_add(column)
        };
        for element in &self.elements {
            let part = element.bounds.intersect(window);
            if part.is_empty() {
                continue;
            }
            for y in part.top..part.bottom {
                if let Some(row) = scratch.get_mut(at(part.left, y)..at(part.right, y)) {
                    row.fill(0.0);
                }
            }
            match &element.kind {
                Kind::Union(pieces) => {
                    for piece in pieces {
                        let area = piece.bounds.intersect(part);
                        for y in area.top..area.bottom {
                            let centre_y = y as f64 + 0.5;
                            let (left, right) = piece.span(centre_y, area.left, area.right);
                            let Some(row) = scratch.get_mut(at(left, y)..at(right, y)) else {
                                continue;
                            };
                            for (x, cell) in (left..).zip(row) {
                                let coverage = piece.sdf.coverage(x as f64 + 0.5, centre_y);
                                if coverage > *cell {
                                    *cell = coverage;
                                }
                            }
                        }
                    }
                }
                &Kind::Gradient {
                    shape,
                    x: gx,
                    y: gy,
                    rx,
                    ry,
                    center,
                } => {
                    for y in part.top..part.bottom {
                        let centre_y = y as f64 + 0.5;
                        let Some(row) = scratch.get_mut(at(part.left, y)..at(part.right, y)) else {
                            continue;
                        };
                        for (x, cell) in (part.left..).zip(row) {
                            let centre_x = x as f64 + 0.5;
                            let coverage = shape.coverage(centre_x, centre_y);
                            let t = length((centre_x - gx) / rx, (centre_y - gy) / ry);
                            let fade = center * (1.0 - t).max(0.0);
                            *cell = coverage * fade;
                        }
                    }
                }
            }
            for y in part.top..part.bottom {
                let range = at(part.left, y)..at(part.right, y);
                let (Some(sums), Some(cells)) =
                    (alpha.get_mut(range.clone()), scratch.get_mut(range))
                else {
                    continue;
                };
                for (sum, &cell) in sums.iter_mut().zip(cells.iter()) {
                    if cell > 0.0 {
                        let a = element.alpha * cell;
                        *sum = a + *sum * (1.0 - a);
                    }
                }
            }
        }
    }

    /// The drawing's alpha at the pixel (`x`, `y`), before its mask and
    /// opacity: what [`ArtPaint::fill_window`] gives there, for a window of
    /// that pixel alone.
    #[cfg(test)]
    pub(super) fn alpha_at(&self, x: i64, y: i64) -> f64 {
        let mut alpha = [0.0];
        let mut scratch = [0.0];
        let window = Edges {
            left: x,
            top: y,
            right: x + 1,
            bottom: y + 1,
        };
        self.fill_window(window, &mut alpha, &mut scratch);
        alpha[0]
    }

    /// The same, but testing every piece of every element at the pixel,
    /// without the bounds and spans that let [`ArtPaint::fill_window`] skip
    /// pixels: tests compare the two.
    #[cfg(test)]
    pub(super) fn alpha_by_brute_force(&self, x: i64, y: i64) -> f64 {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        if !(self.window.left..self.window.right).contains(&x)
            || !(self.window.top..self.window.bottom).contains(&y)
        {
            return 0.0;
        }
        let mut sum = 0.0;
        for element in &self.elements {
            let cell = match &element.kind {
                Kind::Union(pieces) => pieces
                    .iter()
                    .map(|piece| piece.sdf.coverage(px, py))
                    .fold(0.0, f64::max),
                &Kind::Gradient {
                    shape,
                    x,
                    y,
                    rx,
                    ry,
                    center,
                } => {
                    let t = length((px - x) / rx, (py - y) / ry);
                    shape.coverage(px, py) * (center * (1.0 - t).max(0.0))
                }
            };
            if cell > 0.0 {
                let a = element.alpha * cell;
                sum = a + sum * (1.0 - a);
            }
        }
        sum
    }

    /// How many elements and pieces the drawing was prepared into.
    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize) {
        let pieces = self
            .elements
            .iter()
            .map(|element| match &element.kind {
                Kind::Union(pieces) => pieces.len(),
                Kind::Gradient { .. } => 1,
            })
            .sum();
        (self.elements.len(), pieces)
    }

    /// The box's left and right edges, in device pixels.
    #[cfg(test)]
    pub(super) fn box_edges(&self) -> (f64, f64) {
        (self.left, self.right)
    }
}
