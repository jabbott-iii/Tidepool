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

//! jabbott.dev's deep-space backdrop (`.space` in the site's stylesheet),
//! rendered into a cache with a blurred copy for translucent panels:
//! [`BackdropStyle`] and [`Backdrop`].
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! # Layers
//!
//! Bottom to top, as the site paints them, in CSS px times the scale:
//!
//! 1. The background color, `bg`.
//! 2. The three `radial-gradient`s of `.space`, the last listed first, since
//!    CSS paints the first listed on top: each an ellipse whose color fades
//!    linearly, in premultiplied form, to transparent at its stop.
//! 3. The near and then the far star tile (`.space__stars`, and
//!    `.space__stars--far` after it): the same dots in tiles of two sizes,
//!    whose origin is offset by the layer's `inset`, each at its opacity. A
//!    dot is a tiny `radial-gradient`; one that falls outside a tile (the
//!    far tile is smaller) is cut off there, as in CSS.
//! 4. The grid, `.space__grid`: 1 px lines every 64 px, horizontal ones
//!    over vertical ones, under its elliptical `mask-image`.
//! 5. The two drawings (`svg.space__art`, each a [`Drawing`]): the
//!    circuits, bronze traces, pads and a chip at the left edge, and then
//!    the synapses, a purple network of nerve cells at the right edge, each
//!    in its own box under a mask that fades it toward the middle
//!    ([below](#the-drawings)).
//!
//! Nothing moves: the stars' drift and the drawings' sparks (each
//! drawing's `space__pulses`: sparks along a trace or an axon, the flash of
//! its line and a burst where it lands) are ambient motion, which is opt-in
//! and off (`docs/subsystems/ui-and-theme.md`), so the drift is not
//! applied and the sparks are not drawn (ADR-0057).
//!
//! # The drawings
//!
//! Each [`Drawing`] is an SVG drawing that the site places with CSS
//! (`.space__art` and its two variants):
//!
//! - **Its box** is as tall as the window and as wide as its view box's
//!   shape makes it ([`ViewBox`], 400 × 1000 user units for both of the
//!   site's, whose `aspect-ratio: 2 / 5` is that shape), but at most
//!   [`Drawing::max_width`] of the window's width (`height: 100%;
//!   max-width: 40vw`): for the site's drawings, 0.4 times the smaller of
//!   the window's height and width. It stands against the window's left or
//!   right edge ([`Side`]).
//! - **Its user units**, the coordinates of its shapes, are scaled to
//!   cover the box (`preserveAspectRatio="… slice"`): by the larger of the
//!   box's width and height over the view box's, which is the height's,
//!   since the box is never wider than the view box's shape, so `YMid` has
//!   nothing to centre. The view box's side is on the box's (`xMin` or
//!   `xMax`). What falls outside the box is cut off. So a drawing, its
//!   stroke widths too, scales with the window's height, and a window
//!   narrower than 2:5 of its height shows less of it.
//! - **Its shapes** are painted in order, each filled and then stroked in
//!   the drawing's color at the shape's opacities ([`Shape`]) times the
//!   color's alpha, each over the ones before it as SVG composites them.
//! - **Its mask** keeps it whole from the box's side across
//!   [`Drawing::mask_solid`] of its width, then fades it linearly to
//!   transparent at the box's other edge (`linear-gradient(90deg, #000 40%,
//!   transparent)`, `270deg` for the right side).
//! - **Its opacity** is [`BackdropStyle::narrow_opacity`] in a window at
//!   most [`BackdropStyle::narrow_width`] logical px wide (`@media
//!   (max-width: 640px) { .space__art { opacity: 0.7 } }`), and 1 in a
//!   wider one.
//!
//! # Limits
//!
//! A [`Drawing`] is the caller's data, so the work it asks for is bounded
//! twice:
//!
//! - [`BackdropStyle::validate`] refuses a drawing of more than
//!   [`MAX_SHAPES`] (1,024) shapes, a path of more than
//!   [`MAX_PATH_COMMANDS`] (256) commands or a drawing of more than
//!   [`MAX_DRAWING_COMMANDS`] (4,096) in all, and a stroke wider than the
//!   longer side of its view box. The site's drawings have 62 and 106
//!   shapes, at most 6 commands a path, 106 and 212 in all, and strokes at
//!   most 2 user units wide in a view box 1,000 tall: each limit leaves
//!   them nearly ten times the room or more (1,024 shapes for 106), the
//!   strokes 500 times. The check reads the style only, before anything is
//!   allocated.
//! - Preparing a drawing at a size, which comes before any pixel is
//!   allocated or drawn, stops with an error once it needs more than
//!   [`MAX_PIECES`] (65,536) pieces, or more coverage tests (a piece's or
//!   an element's test of a pixel it may reach) than [`MAX_TESTS_PER_PIXEL`]
//!   (64) a pixel of the drawing's box, or [`MIN_TESTS`] (2^20) in a small
//!   box. The site's drawings need at most 12,236 pieces, in a window of
//!   16,384 px a side, and from 0.9 to 2.8 tests a pixel in windows from
//!   480 × 300 to 16,384 px a side (up to 13 in a window of 40 × 100, well
//!   under [`MIN_TESTS`]). So a drawing costs at most about 64 coverage
//!   tests a pixel of its box, and its pieces at most 7.5 MiB.
//!
//! Why these numbers: the limits on the style keep `validate` and the
//! flattening of curves short (at most 4,096 commands, each at most 512
//! lines), and refuse a stroke so wide that each of its pieces reaches the
//! whole box; the budgets bound what is left, the time and memory that
//! depend on the window's size, with five times the room the site's
//! drawings need at the largest window (12,236 pieces) and over 20 times
//! at the others.
//!
//! # Approximations
//!
//! It is drawn to look like the site and to give the same bytes on every
//! CPU, not to match a browser pixel for pixel:
//!
//! - **Arithmetic.** Each pixel is computed in `f64` with `+ − × ÷`,
//!   `sqrt`, `abs`, `floor`, `ceil`, `min`, `max` and `clamp` only (no
//!   `exp`, `erf` or other libm functions, and no `mul_add`), in a fixed
//!   order, as premultiplied color from 0 to 1, and rounded to bytes once,
//!   as `floor(v × 255 + 0.5)`, after the drawings are composited over the
//!   layers below, rather than after each layer as a browser does, which
//!   keeps the faint layers from banding.
//! - **Sampling.** Gradients, the grid's mask, the drawings' masks and the
//!   gradients of their halos are evaluated at the pixel's centre; the
//!   grid's lines are box-filtered (below); the star dots, smaller than a
//!   pixel, at a fixed 4 × 4 grid of points in the pixel, averaged.
//! - **The grid.** Each pixel's coverage is the exact length of its row or
//!   column that the lines cover (a box filter), so lines stay 1 CSS px
//!   wide at any scale; the mask is evaluated at the pixel's centre.
//! - **The drawings' edges.** A shape covers a pixel by a 1 px linear ramp
//!   of its signed distance `d` (how far the pixel's centre is outside the
//!   shape's edge, negative inside), `clamp(0.5 − d, 0, 1)`, as
//!   [`raster::pill`](crate::raster::pill) does, not by its exact area.
//!   Across a long straight stroke at least 1 px wide that runs along the
//!   rows or the columns, the two leave the same ink (the sum of the
//!   coverage). Across a slanted one they leave the same ink on average,
//!   but a row's ink depends on where the stroke crosses its pixels: for a
//!   stroke at 45°, from 0.91 to 1.12 times the exact ink at 1 px wide, and
//!   from 0.975 to 1.02 times at 2 px.
//! - **Thin strokes.** A stroke narrower than 1 device px is drawn 1 px
//!   wide at its opacity times its width, where the ramp alone would
//!   overstate its ink: so it leaves the ink a 1 px stroke does, times its
//!   width, exact along the rows or the columns and right on average when
//!   slanted, with the 1 px stroke's spread.
//! - **Strokes.** A stroke is the union of a rect along each straight
//!   piece, square at its ends, a disc at each corner (a round join), and
//!   with round caps a disc at each end of a subpath. Its coverage of a
//!   pixel is the largest of its parts', so where they overlap, as at every
//!   corner, it is not counted twice: SVG strokes a path as one shape.
//!   Corners are always joined round, as the site's traces ask
//!   (`stroke-linejoin: round`); its other paths have no corners.
//! - **Curves** are flattened into lines at equal steps of their parameter,
//!   enough that no point of the curve is more than 1/32 px from them, from
//!   1 to 512 lines a curve, and stroked as above.
//! - **Rounded rects** have circular corners of radius `rx`, cut to half the
//!   shorter side (SVG cuts `rx` and `ry` each to half its own side).
//! - **Fills.** A path's fill is not drawn: the site's paths enclose
//!   nothing. A radial gradient fades from the centre of the shape's
//!   bounding box to its edge, times the shape's coverage.
//! - **The box** cuts a drawing off at pixel centres. At its inner edge the
//!   mask has faded to nearly nothing, and its outer edge is the window's.
//! - **Colors.** The site's colors are 8-bit [`Color`]s, so an alpha such
//!   as 0.022 is stored as `round(0.022 × 255) = 6`, which is 0.0235, as
//!   [`Color::parse`] would read the CSS, and `color-mix(in srgb,
//!   var(--purple) 10%, transparent)` is `--purple` at an alpha of
//!   `round(0.10 × 255) = 26`. The drawings' opacities stay numbers, as in
//!   their CSS.
//!
//! The drawings are worked out a band of rows at a time, and only near
//! each shape, but every piece of a shape is evaluated at the pixel's
//! centre alone, so a pixel's bytes are the same whichever band it falls
//! in; the tests check that against each pixel worked out on its own, and
//! against every shape tested at every pixel.
//!
//! # The blurred copy
//!
//! [`Backdrop::blurred`] is the backdrop after a blur standing in for CSS
//! `backdrop-filter: blur(10px)`, whose length is the Gaussian's standard
//! deviation, here [`BackdropStyle::blur_radius`] × the scale. It is
//! three box blurs across and then three down, with box sizes chosen so
//! their combined variance matches the Gaussian's (Kovesi's construction:
//! two of one odd width and one two wider, or the reverse). Each pass
//! keeps an integer running sum per channel, repeats the edge pixels past
//! the edges, and rounds each pixel to nearest. The standard deviation is
//! capped at 32,768 px, where the box sums still fit easily in `u64`.

use tidepool_core::color::Color;
use tidepool_core::geom::{DeviceSize, Scale};

use crate::GfxError;
use crate::clip::Edges;
use crate::framebuffer::{BYTES_PER_PIXEL, Framebuffer, MAX_DIMENSION, zeroed};

mod drawing;
mod jabbott_dev;

#[cfg(test)]
mod tests;

pub use drawing::{
    Cap, Drawing, Fill, Geometry, MAX_DRAWING_COMMANDS, MAX_PATH_COMMANDS, MAX_PIECES, MAX_SHAPES,
    MAX_TESTS_PER_PIXEL, MIN_TESTS, PathCommand, Shape, Side, Stroke, ViewBox,
};

use drawing::{ArtPaint, CIRCUITS, SYNAPSES};

/// A `radial-gradient` of `.space`: an ellipse of color fading linearly to
/// transparent.
#[derive(Clone, Debug, PartialEq)]
pub struct Glow {
    /// The color at the centre.
    pub color: Color,
    /// The ellipse's horizontal radius, in CSS px: 100% of the gradient.
    pub radius_x: f32,
    /// The ellipse's vertical radius, in CSS px.
    pub radius_y: f32,
    /// The centre's x as a fraction of the backdrop's width (`85%` is
    /// 0.85); it may be negative or above 1.
    pub center_x: f32,
    /// The centre's y as a fraction of the backdrop's height.
    pub center_y: f32,
    /// Where the color has faded to transparent, as a fraction of the
    /// radius (`transparent 62%` is 0.62).
    pub fade_stop: f32,
}

/// A star dot: a tiny `radial-gradient` in the star tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    /// Its centre in the tile, in CSS px.
    pub x: f32,
    /// Its centre in the tile, in CSS px.
    pub y: f32,
    /// The gradient's radius, in CSS px (`1.6px 1.6px` is 1.6).
    pub radius: f32,
    /// The color at the centre.
    pub color: Color,
    /// Where it has faded to transparent, as a fraction of the radius.
    pub fade_stop: f32,
}

/// One layer of star tiles.
#[derive(Clone, Debug, PartialEq)]
pub struct StarTile {
    /// The tile's width, in CSS px (`background-size`).
    pub width: f32,
    /// The tile's height, in CSS px.
    pub height: f32,
    /// The layer's opacity, from 0 to 1.
    pub opacity: f32,
}

/// The grid, `.space__grid`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    /// The distance between lines, in CSS px (`background-size`).
    pub cell: f32,
    /// The lines' width, in CSS px.
    pub line_width: f32,
    /// The lines' color.
    pub color: Color,
    /// The mask ellipse's horizontal radius, as a fraction of the width.
    pub mask_radius_x: f32,
    /// The mask ellipse's vertical radius, as a fraction of the height.
    pub mask_radius_y: f32,
    /// The mask's centre x, as a fraction of the width.
    pub mask_center_x: f32,
    /// The mask's centre y, as a fraction of the height.
    pub mask_center_y: f32,
    /// Where the mask has faded to nothing, as a fraction of its radii.
    pub mask_fade_stop: f32,
}

/// What the backdrop is drawn from: every layer's geometry and colors.
///
/// [`BackdropStyle::jabbott_dev`] gives the site's. The theme
/// (`tidepool_ui::theme`, M3.2) does not carry a backdrop style, nor do the
/// controls' component tokens (M3.6, ADR-0049), so `gui` draws
/// `jabbott_dev()`, whose `bg`, and its drawings' colors, a test of `gui`
/// holds to the theme's `bg`, `bronze` and `purple` (ADR-0057). Equality
/// lets the cache see a change of style.
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropStyle {
    /// The color under everything (`var(--bg)`).
    pub bg: Color,
    /// The radial gradients, in CSS order: the first is painted on top.
    pub glows: Vec<Glow>,
    /// The dots of both star layers.
    pub stars: Vec<Star>,
    /// CSS `inset` of the star layers, in CSS px: the tiles' origin is at
    /// (`star_inset`, `star_inset`) from the backdrop's top-left corner.
    pub star_inset: f32,
    /// The near star layer, painted first.
    pub near_stars: StarTile,
    /// The far star layer, painted over the near one.
    pub far_stars: StarTile,
    /// The grid.
    pub grid: Grid,
    /// The circuits: the drawing at the left, painted over the grid.
    pub circuits: Drawing,
    /// The synapses: the drawing at the right, painted over the circuits.
    pub synapses: Drawing,
    /// The widest window, in logical px, in which both drawings are at
    /// [`BackdropStyle::narrow_opacity`] (`@media (max-width: 640px)`).
    pub narrow_width: f32,
    /// The drawings' opacity in a window at most
    /// [`BackdropStyle::narrow_width`] wide, from 0 to 1.
    pub narrow_opacity: f32,
    /// The blur of translucent panels' backdrop, in CSS px: the Gaussian's
    /// standard deviation (`backdrop-filter: blur(10px)`).
    pub blur_radius: f32,
}

impl BackdropStyle {
    /// The backdrop of jabbott.dev, from its stylesheet
    /// (`jabbott-iii/jabbott-iii.github.io`, `assets/css/style.css`, at
    /// commit 849b36e; the comments give its line numbers) and, for the
    /// drawings' geometry, its `_includes/backdrop.html` at that commit.
    /// Alphas are `round(A × 255)`, as [`Color::parse`] reads `rgba(…)`.
    pub fn jabbott_dev() -> Self {
        let white = |a| Color::rgba(255, 255, 255, a);
        let star = |x, y, radius, color| Star {
            x,
            y,
            radius,
            color,
            // `transparent 60%` on every dot (lines 115–121).
            fade_stop: 0.6,
        };
        Self {
            // `--bg: #0b0c0d` (line 5), `.space`'s last layer (line 109).
            bg: Color::rgb(0x0b, 0x0c, 0x0d),
            glows: vec![
                // Line 106: `radial-gradient(800px 900px at 104% 35%,
                // color-mix(in srgb, var(--purple) 10%, transparent),
                // transparent 62%)`: `--purple`, #9a6bff (line 27), at
                // 0.10 → 26.
                Glow {
                    color: Color::rgba(0x9a, 0x6b, 0xff, 26),
                    radius_x: 800.0,
                    radius_y: 900.0,
                    center_x: 1.04,
                    center_y: 0.35,
                    fade_stop: 0.62,
                },
                // Line 107: `radial-gradient(800px 900px at -4% 60%,
                // color-mix(in srgb, var(--bronze) 8%, transparent),
                // transparent 62%)`: `--bronze`, #cd7f32 (line 25), at
                // 0.08 → 20.
                Glow {
                    color: Color::rgba(0xcd, 0x7f, 0x32, 20),
                    radius_x: 800.0,
                    radius_y: 900.0,
                    center_x: -0.04,
                    center_y: 0.60,
                    fade_stop: 0.62,
                },
                // Line 108: `radial-gradient(1400px 900px at 50% 125%,
                // rgba(38, 70, 82, 0.4), transparent 62%)`; 0.4 → 102.
                Glow {
                    color: Color::rgba(38, 70, 82, 102),
                    radius_x: 1400.0,
                    radius_y: 900.0,
                    center_x: 0.50,
                    center_y: 1.25,
                    fade_stop: 0.62,
                },
            ],
            stars: vec![
                // Line 115: `1px 1px at 25px 35px, rgba(255,255,255,0.9)`.
                star(25.0, 35.0, 1.0, white(230)),
                // Line 116: `1px 1px at 75px 125px, …0.55`.
                star(75.0, 125.0, 1.0, white(140)),
                // Line 117: `1.6px 1.6px at 162px 62px,
                // rgba(205, 245, 255, 0.85)`.
                star(162.0, 62.0, 1.6, Color::rgba(205, 245, 255, 217)),
                // Line 118: `1px 1px at 214px 192px, …0.5`.
                star(214.0, 192.0, 1.0, white(128)),
                // Line 119: `1px 1px at 118px 232px, …0.7`.
                star(118.0, 232.0, 1.0, white(179)),
                // Line 120: `1.2px 1.2px at 272px 22px,
                // rgba(255, 236, 200, 0.7)`.
                star(272.0, 22.0, 1.2, Color::rgba(255, 236, 200, 179)),
                // Line 121: `1px 1px at 286px 148px, …0.45`.
                star(286.0, 148.0, 1.0, white(115)),
            ],
            // Line 113: `inset: -260px`.
            star_inset: -260.0,
            // Lines 122–123: `background-size: 300px 260px; opacity: 0.55`.
            near_stars: StarTile {
                width: 300.0,
                height: 260.0,
                opacity: 0.55,
            },
            // Lines 128–129: `background-size: 190px 170px; opacity: 0.28`.
            far_stars: StarTile {
                width: 190.0,
                height: 170.0,
                opacity: 0.28,
            },
            grid: Grid {
                // Line 138: `background-size: 64px 64px`.
                cell: 64.0,
                // Lines 136–137: `linear-gradient(rgba(255, 255, 255, 0.022)
                // 1px, transparent 1px)`, once down and once across;
                // 0.022 → 6.
                line_width: 1.0,
                color: white(6),
                // Line 140: `radial-gradient(ellipse 80% 60% at 50% 0%,
                // #000 0%, transparent 75%)`.
                mask_radius_x: 0.8,
                mask_radius_y: 0.6,
                mask_center_x: 0.5,
                mask_center_y: 0.0,
                mask_fade_stop: 0.75,
            },
            // Lines 150–185, and the include's two `<svg>`s.
            circuits: jabbott_dev::circuits(),
            synapses: jabbott_dev::synapses(),
            // Lines 1546 and 1549: `@media (max-width: 640px) {
            // .space__art { opacity: 0.7 } }`.
            narrow_width: 640.0,
            narrow_opacity: 0.7,
            // Lines 420 and 425: `.panel { backdrop-filter: blur(10px) }`.
            blur_radius: 10.0,
        }
    }

    /// Checks every value: all are finite; sizes, radii, widths, blurs and
    /// stops are not negative; opacities, and the fraction of a drawing's
    /// box its mask keeps whole, are from 0 to 1. Positions, offsets and
    /// coordinates may be negative.
    ///
    /// It also checks the drawings' limits ([below](self#limits)): each has
    /// at most [`MAX_SHAPES`] shapes, each path at most
    /// [`MAX_PATH_COMMANDS`] commands, and the drawing at most
    /// [`MAX_DRAWING_COMMANDS`] in all; and no stroke is wider than the
    /// longer side of its drawing's view box. It reads the style only, so
    /// it allocates nothing and takes time in proportion to the style's
    /// size.
    ///
    /// # Errors
    ///
    /// [`GfxError::InvalidStyle`] naming the first field that is not, or
    /// that is over its limit: a drawing's `shapes` with the index
    /// [`MAX_SHAPES`] for too many shapes, and the shape's `geometry` for a
    /// path that brings too many commands.
    pub fn validate(&self) -> Result<(), GfxError> {
        use Rule::{Finite, NonNegative, Opacity};
        for (index, glow) in self.glows.iter().enumerate() {
            let index = Some(index);
            check(glow.radius_x, "glows.radius_x", index, NonNegative)?;
            check(glow.radius_y, "glows.radius_y", index, NonNegative)?;
            check(glow.center_x, "glows.center_x", index, Finite)?;
            check(glow.center_y, "glows.center_y", index, Finite)?;
            check(glow.fade_stop, "glows.fade_stop", index, NonNegative)?;
        }
        for (index, star) in self.stars.iter().enumerate() {
            let index = Some(index);
            check(star.x, "stars.x", index, Finite)?;
            check(star.y, "stars.y", index, Finite)?;
            check(star.radius, "stars.radius", index, NonNegative)?;
            check(star.fade_stop, "stars.fade_stop", index, NonNegative)?;
        }
        let grid = &self.grid;
        for (value, field, rule) in [
            (self.star_inset, "star_inset", Finite),
            (self.near_stars.width, "near_stars.width", NonNegative),
            (self.near_stars.height, "near_stars.height", NonNegative),
            (self.near_stars.opacity, "near_stars.opacity", Opacity),
            (self.far_stars.width, "far_stars.width", NonNegative),
            (self.far_stars.height, "far_stars.height", NonNegative),
            (self.far_stars.opacity, "far_stars.opacity", Opacity),
            (grid.cell, "grid.cell", NonNegative),
            (grid.line_width, "grid.line_width", NonNegative),
            (grid.mask_radius_x, "grid.mask_radius_x", NonNegative),
            (grid.mask_radius_y, "grid.mask_radius_y", NonNegative),
            (grid.mask_center_x, "grid.mask_center_x", Finite),
            (grid.mask_center_y, "grid.mask_center_y", Finite),
            (grid.mask_fade_stop, "grid.mask_fade_stop", NonNegative),
        ] {
            check(value, field, None, rule)?;
        }
        self.circuits.validate(&CIRCUITS)?;
        self.synapses.validate(&SYNAPSES)?;
        for (value, field, rule) in [
            (self.narrow_width, "narrow_width", NonNegative),
            (self.narrow_opacity, "narrow_opacity", Opacity),
            (self.blur_radius, "blur_radius", NonNegative),
        ] {
            check(value, field, None, rule)?;
        }
        Ok(())
    }
}

impl Default for BackdropStyle {
    /// [`BackdropStyle::jabbott_dev`].
    fn default() -> Self {
        Self::jabbott_dev()
    }
}

/// What [`BackdropStyle::validate`] asks of a value.
#[derive(Clone, Copy)]
enum Rule {
    Finite,
    NonNegative,
    Opacity,
}

fn check(
    value: f32,
    field: &'static str,
    index: Option<usize>,
    rule: Rule,
) -> Result<(), GfxError> {
    let valid = value.is_finite()
        && match rule {
            Rule::Finite => true,
            Rule::NonNegative => value >= 0.0,
            Rule::Opacity => (0.0..=1.0).contains(&value),
        };
    if valid {
        Ok(())
    } else {
        Err(GfxError::InvalidStyle { field, index })
    }
}

/// The backdrop cache: the backdrop at one size, scale and style, and its
/// blurred copy, rebuilt only when one of them changes (ADR-0011).
#[derive(Debug)]
pub struct Backdrop {
    key: Option<Key>,
    base: Framebuffer,
    blurred: Framebuffer,
}

/// What the cache was built for.
#[derive(Debug)]
struct Key {
    size: DeviceSize,
    scale: Scale,
    style: BackdropStyle,
}

impl Backdrop {
    /// An empty cache: both frames have no pixels until
    /// [`Backdrop::ensure`] builds them.
    pub const fn new() -> Self {
        Self {
            key: None,
            base: Framebuffer::empty(),
            blurred: Framebuffer::empty(),
        }
    }

    /// Makes sure the cache holds the backdrop for `size` device pixels at
    /// `scale` in `style`, building it if the size, the scale or the style
    /// changed. Returns `true` if it built it, `false` if it already held
    /// it.
    ///
    /// # Errors
    ///
    /// The cache is unchanged if it fails:
    ///
    /// - [`GfxError::InvalidStyle`] if `style` does not
    ///   [validate](BackdropStyle::validate), or if a drawing would take
    ///   more than its budget at this size ([`MAX_PIECES`] pieces, or more
    ///   coverage tests than [`MAX_TESTS_PER_PIXEL`] a pixel of its box and
    ///   [`MIN_TESTS`]), naming the drawing's shape at which it ran out;
    ///   that is found before any pixel is allocated or drawn.
    /// - The errors of [`Framebuffer::new`] for `size`.
    /// - [`GfxError::OutOfMemory`] for the blurred copy, the drawings'
    ///   shapes or the buffers.
    pub fn ensure(
        &mut self,
        size: DeviceSize,
        scale: Scale,
        style: &BackdropStyle,
    ) -> Result<bool, GfxError> {
        if let Some(key) = &self.key
            && key.size == size
            && key.scale == scale
            && key.style == *style
        {
            return Ok(false);
        }
        style.validate()?;
        let painter = Painter::new(size, scale, style)?;
        let mut base = Framebuffer::new(size)?;
        render(&mut base, &painter)?;
        let mut blurred = base.try_clone()?;
        let sigma = (f64::from(style.blur_radius) * f64::from(scale.factor())).min(MAX_SIGMA);
        box_blur(&mut blurred, box_radii(sigma))?;
        self.base = base;
        self.blurred = blurred;
        self.key = Some(Key {
            size,
            scale,
            style: style.clone(),
        });
        Ok(true)
    }

    /// The backdrop, opaque when the style's `bg` is, for drawing with
    /// [`image::copy`](crate::image::copy).
    pub const fn base(&self) -> &Framebuffer {
        &self.base
    }

    /// The backdrop blurred, to show through translucent panels.
    pub const fn blurred(&self) -> &Framebuffer {
        &self.blurred
    }
}

impl Default for Backdrop {
    fn default() -> Self {
        Self::new()
    }
}

/// The largest standard deviation of the blur, in device pixels.
const MAX_SIGMA: f64 = 2.0 * MAX_DIMENSION as f64;

/// A premultiplied color, `[r, g, b, a]` from 0 to 1.
type Rgba = [f64; 4];

const CLEAR: Rgba = [0.0; 4];

fn premultiply(color: Color) -> Rgba {
    let alpha = f64::from(color.a) / 255.0;
    [
        f64::from(color.r) / 255.0 * alpha,
        f64::from(color.g) / 255.0 * alpha,
        f64::from(color.b) / 255.0 * alpha,
        alpha,
    ]
}

/// `src` over `dst`, source-over.
fn over(src: Rgba, dst: Rgba) -> Rgba {
    let keep = 1.0 - src[3];
    [
        src[0] + dst[0] * keep,
        src[1] + dst[1] * keep,
        src[2] + dst[2] * keep,
        src[3] + dst[3] * keep,
    ]
}

fn times(color: Rgba, amount: f64) -> Rgba {
    color.map(|channel| channel * amount)
}

fn plus(a: Rgba, b: Rgba) -> Rgba {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

/// As pixel bytes in memory order, `floor(v × 255 + 0.5)` each, with no
/// color channel above alpha.
fn to_bgra(color: Rgba) -> [u8; 4] {
    let byte = |value: f64| (value.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8;
    let alpha = byte(color[3]);
    let channel = |value: f64| byte(value).min(alpha);
    [
        channel(color[2]),
        channel(color[1]),
        channel(color[0]),
        alpha,
    ]
}

/// How much of an elliptical gradient's color is left at (`dx`, `dy`) from
/// its centre: 1 at the centre, fading linearly to 0 at `stop` of the
/// radii. A zero radius or stop leaves nothing.
fn ellipse_fade(dx: f64, dy: f64, radius_x: f64, radius_y: f64, stop: f64) -> f64 {
    if !(radius_x > 0.0 && radius_y > 0.0 && stop > 0.0) {
        return 0.0;
    }
    let (u, v) = (dx / radius_x, dy / radius_y);
    (1.0 - (u * u + v * v).sqrt() / stop).max(0.0)
}

/// `value` wrapped into a tile `size` long, from 0 up to `size`.
fn wrap(value: f64, size: f64) -> f64 {
    let wrapped = value - (value / size).floor() * size;
    if (0.0..size).contains(&wrapped) {
        wrapped
    } else {
        0.0
    }
}

/// The length of [`left`, `left` + 1) covered by lines `line` long every
/// `period`, starting at 0.
fn line_coverage(left: f64, period: f64, line: f64) -> f64 {
    // How much of [0, v) the lines cover.
    let covered = |v: f64| {
        let lines = (v / period).floor();
        lines * line + (v - lines * period).clamp(0.0, line)
    };
    (covered(left + 1.0) - covered(left)).clamp(0.0, 1.0)
}

/// A glow in device pixels.
#[derive(Debug)]
struct GlowPaint {
    color: Rgba,
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
    stop: f64,
}

/// A star in CSS px, with the distance past which it is clear.
#[derive(Debug)]
struct StarPaint {
    x: f64,
    y: f64,
    radius: f64,
    stop: f64,
    color: Rgba,
    reach: f64,
}

/// A star layer in CSS px.
#[derive(Debug)]
struct TilePaint {
    width: f64,
    height: f64,
    opacity: f64,
}

/// The grid in device pixels.
#[derive(Debug)]
struct GridPaint {
    period: f64,
    line: f64,
    color: Rgba,
    mask_x: f64,
    mask_y: f64,
    mask_radius_x: f64,
    mask_radius_y: f64,
    mask_stop: f64,
}

/// A style at one size and scale, ready to evaluate pixel by pixel.
#[derive(Debug)]
struct Painter {
    scale: f64,
    bg: Rgba,
    /// In painting order: the last listed first.
    glows: Vec<GlowPaint>,
    stars: Vec<StarPaint>,
    inset: f64,
    near: TilePaint,
    far: TilePaint,
    grid: GridPaint,
    /// The circuits and then the synapses, in painting order.
    drawings: [ArtPaint; 2],
}

/// The fixed supersampling pattern for stars: 4 × 4 points in a pixel.
const SAMPLES: usize = 4;

/// How many rows [`render`] works the drawings out for at a time: at most
/// 32 × 16,384 `f64`s, 4 MiB, a buffer.
const BAND: i64 = 32;

/// What a row of pixels shares: which layers can reach it, and the terms
/// of its gradients that depend only on y.
#[derive(Debug, Default)]
struct Row {
    y: i64,
    /// The y of the row's pixel centres.
    centre: f64,
    /// Each glow's vertical term, `(y − cy) / ry`, or `None` if the glow
    /// cannot reach the row.
    glows: Vec<Option<f64>>,
    near: bool,
    far: bool,
    /// How much of the row the grid's horizontal lines cover.
    down: f64,
}

/// What a run of columns shares: the terms that depend only on x.
#[derive(Debug, Default)]
struct Columns {
    first: i64,
    /// Each glow's horizontal term, `(x − cx) / rx`, column by column.
    glows: Vec<f64>,
    /// How much of each column the grid's vertical lines cover.
    grid: Vec<f64>,
}

impl Painter {
    fn new(size: DeviceSize, scale: Scale, style: &BackdropStyle) -> Result<Self, GfxError> {
        let s = f64::from(scale.factor());
        let (width, height) = (f64::from(size.width.max(0)), f64::from(size.height.max(0)));
        let glows = style
            .glows
            .iter()
            .rev()
            .map(|glow| GlowPaint {
                color: premultiply(glow.color),
                center_x: f64::from(glow.center_x) * width,
                center_y: f64::from(glow.center_y) * height,
                radius_x: f64::from(glow.radius_x) * s,
                radius_y: f64::from(glow.radius_y) * s,
                stop: f64::from(glow.fade_stop),
            })
            .collect();
        let stars = style
            .stars
            .iter()
            .map(|star| {
                let (radius, stop) = (f64::from(star.radius), f64::from(star.fade_stop));
                StarPaint {
                    x: f64::from(star.x),
                    y: f64::from(star.y),
                    radius,
                    stop,
                    color: premultiply(star.color),
                    reach: radius * stop,
                }
            })
            .collect();
        let tile = |tile: &StarTile| TilePaint {
            width: f64::from(tile.width),
            height: f64::from(tile.height),
            opacity: f64::from(tile.opacity),
        };
        let grid = &style.grid;
        let period = f64::from(grid.cell) * s;
        // `max-width: 640px` holds for a window at most that wide, in
        // logical px as the rest of Tidepool converts them.
        let opacity = if scale.to_logical(size).width <= style.narrow_width {
            f64::from(style.narrow_opacity)
        } else {
            1.0
        };
        Ok(Self {
            scale: s,
            bg: premultiply(style.bg),
            glows,
            stars,
            inset: f64::from(style.star_inset),
            near: tile(&style.near_stars),
            far: tile(&style.far_stars),
            grid: GridPaint {
                period,
                line: (f64::from(grid.line_width) * s).min(period),
                color: premultiply(grid.color),
                mask_x: f64::from(grid.mask_center_x) * width,
                mask_y: f64::from(grid.mask_center_y) * height,
                mask_radius_x: f64::from(grid.mask_radius_x) * width,
                mask_radius_y: f64::from(grid.mask_radius_y) * height,
                mask_stop: f64::from(grid.mask_fade_stop),
            },
            drawings: [
                ArtPaint::new(&style.circuits, &CIRCUITS, size, opacity)?,
                ArtPaint::new(&style.synapses, &SYNAPSES, size, opacity)?,
            ],
        })
    }

    /// The pixel at column `x` and row `y`, as bytes in memory order: what
    /// [`render`] writes there.
    #[cfg(test)]
    fn pixel(&self, x: i64, y: i64) -> [u8; 4] {
        let mut row = Row::default();
        let mut columns = Columns::default();
        self.fill_row(&mut row, y).expect("a few values");
        self.fill_columns(&mut columns, x, 1).expect("one column");
        let alphas = self
            .drawings
            .each_ref()
            .map(|drawing| drawing.alpha_at(x, y));
        self.finish(x, self.shade(&row, &columns, x), alphas)
    }

    /// Sets `row` up for row `y`.
    fn fill_row(&self, row: &mut Row, y: i64) -> Result<(), GfxError> {
        let centre = y as f64 + 0.5;
        row.y = y;
        row.centre = centre;
        row.glows.clear();
        row.glows
            .try_reserve(self.glows.len())
            .map_err(|_| GfxError::OutOfMemory {
                bytes: self.glows.len().saturating_mul(size_of::<Option<f64>>()),
            })?;
        row.glows.extend(self.glows.iter().map(|glow| {
            let v = (centre - glow.center_y) / glow.radius_y;
            let visible = glow.radius_x > 0.0 && glow.radius_y > 0.0 && glow.stop > 0.0;
            (visible && v.abs() < glow.stop).then_some(v)
        }));
        row.near = self.stars_may_reach(y, &self.near);
        row.far = self.stars_may_reach(y, &self.far);
        row.down = self.grid.coverage(y);
        Ok(())
    }

    /// Sets `columns` up for `count` columns from `first`.
    fn fill_columns(
        &self,
        columns: &mut Columns,
        first: i64,
        count: usize,
    ) -> Result<(), GfxError> {
        let glows = self.glows.len();
        let cells = count.saturating_mul(glows);
        columns.first = first;
        columns.glows.clear();
        columns.grid.clear();
        let out_of_memory = |_| GfxError::OutOfMemory {
            bytes: cells.saturating_add(count).saturating_mul(size_of::<f64>()),
        };
        columns
            .glows
            .try_reserve_exact(cells)
            .map_err(out_of_memory)?;
        columns
            .grid
            .try_reserve_exact(count)
            .map_err(out_of_memory)?;
        for x in (first..).take(count) {
            let centre = x as f64 + 0.5;
            columns.glows.extend(
                self.glows
                    .iter()
                    .map(|glow| (centre - glow.center_x) / glow.radius_x),
            );
            columns.grid.push(self.grid.coverage(x));
        }
        Ok(())
    }

    /// The layers under the drawings at column `x` of `row`, whose column
    /// terms `columns` holds.
    fn shade(&self, row: &Row, columns: &Columns, x: i64) -> Rgba {
        let index = usize::try_from(x - columns.first).unwrap_or(usize::MAX);
        let mut color = self.bg;
        for (number, (glow, v)) in self.glows.iter().zip(&row.glows).enumerate() {
            let Some(v) = *v else { continue };
            let u = columns
                .glows
                .get(
                    index
                        .saturating_mul(self.glows.len())
                        .saturating_add(number),
                )
                .copied()
                .unwrap_or(f64::INFINITY);
            let amount = 1.0 - (u * u + v * v).sqrt() / glow.stop;
            if amount > 0.0 {
                color = over(times(glow.color, amount), color);
            }
        }
        let centre_x = x as f64 + 0.5;
        if row.near {
            color = self.stars(x, row.y, &self.near, color);
        }
        if row.far {
            color = self.stars(x, row.y, &self.far, color);
        }
        let across = columns.grid.get(index).copied().unwrap_or(0.0);
        if across > 0.0 || row.down > 0.0 {
            color = self.grid(centre_x, row.centre, across, row.down, color);
        }
        color
    }

    /// The pixel at column `x`, as bytes in memory order: the drawings,
    /// whose alphas there are `alphas` (before their masks and opacity),
    /// over `below`.
    fn finish(&self, x: i64, below: Rgba, alphas: [f64; 2]) -> [u8; 4] {
        let mut color = below;
        for (drawing, alpha) in self.drawings.iter().zip(alphas) {
            if alpha > 0.0 {
                color = over(times(drawing.color, alpha * drawing.weight(x)), color);
            }
        }
        to_bgra(color)
    }

    /// Returns `false` if no star of the layer `tile` can show in row `y`.
    fn stars_may_reach(&self, y: i64, tile: &TilePaint) -> bool {
        if !(tile.width > 0.0 && tile.height > 0.0 && tile.opacity > 0.0) || self.stars.is_empty() {
            return false;
        }
        let margin = 0.75 / self.scale;
        if tile.height <= 2.0 * margin {
            return true;
        }
        let local = wrap((y as f64 + 0.5) / self.scale - self.inset, tile.height);
        self.stars
            .iter()
            .any(|star| near_in_tile(local, star.y, tile.height, star.reach + margin))
    }

    /// The star layer `tile` at pixel (`x`, `y`) over `below`.
    fn stars(&self, x: i64, y: i64, tile: &TilePaint, below: Rgba) -> Rgba {
        if !(tile.width > 0.0 && tile.height > 0.0 && tile.opacity > 0.0) || self.stars.is_empty() {
            return below;
        }
        // Skip pixels with no star near them. A sample is at most 0.53
        // device px from the centre, and 0.75 is more; a tile smaller than
        // that margin could put any star under a sample, so it is never
        // skipped.
        let margin = 0.75 / self.scale;
        if tile.width > 2.0 * margin && tile.height > 2.0 * margin {
            let local_x = wrap((x as f64 + 0.5) / self.scale - self.inset, tile.width);
            let local_y = wrap((y as f64 + 0.5) / self.scale - self.inset, tile.height);
            let any = self.stars.iter().any(|star| {
                near_in_tile(local_x, star.x, tile.width, star.reach + margin)
                    && near_in_tile(local_y, star.y, tile.height, star.reach + margin)
            });
            if !any {
                return below;
            }
        }
        let mut sum = CLEAR;
        for j in 0..SAMPLES {
            for i in 0..SAMPLES {
                let sx = x as f64 + (i as f64 + 0.5) / SAMPLES as f64;
                let sy = y as f64 + (j as f64 + 0.5) / SAMPLES as f64;
                let local_x = wrap(sx / self.scale - self.inset, tile.width);
                let local_y = wrap(sy / self.scale - self.inset, tile.height);
                let mut layer = CLEAR;
                // The first listed is painted on top.
                for star in self.stars.iter().rev() {
                    let amount = ellipse_fade(
                        local_x - star.x,
                        local_y - star.y,
                        star.radius,
                        star.radius,
                        star.stop,
                    );
                    if amount > 0.0 {
                        layer = over(times(star.color, amount), layer);
                    }
                }
                sum = plus(sum, layer);
            }
        }
        let average = times(sum, 1.0 / (SAMPLES * SAMPLES) as f64);
        over(times(average, tile.opacity), below)
    }

    /// The grid's lines, covering `across` of the pixel's column and `down`
    /// of its row, at the pixel centred on (`x`, `y`) over `below`.
    fn grid(&self, x: f64, y: f64, across: f64, down: f64, below: Rgba) -> Rgba {
        let grid = &self.grid;
        let mask = ellipse_fade(
            x - grid.mask_x,
            y - grid.mask_y,
            grid.mask_radius_x,
            grid.mask_radius_y,
            grid.mask_stop,
        );
        if mask <= 0.0 {
            return below;
        }
        // The horizontal lines' image is listed first, so it is on top.
        let lines = over(times(grid.color, down), times(grid.color, across));
        over(times(lines, mask), below)
    }
}

impl GridPaint {
    /// How much of pixel row or column `index` the lines cover: 0 when the
    /// grid shows nothing.
    fn coverage(&self, index: i64) -> f64 {
        if self.period > 0.0 && self.line > 0.0 && self.color[3] > 0.0 {
            line_coverage(index as f64, self.period, self.line)
        } else {
            0.0
        }
    }
}

/// Returns `true` if a sample at `local` in a tile `size` long, or in the
/// tile before or after it, can be within `reach` of a star at `star`.
fn near_in_tile(local: f64, star: f64, size: f64, reach: f64) -> bool {
    let gap = (local - star)
        .abs()
        .min((local + size - star).abs())
        .min((local - size - star).abs());
    gap < reach
}

/// Fills `fb` with the backdrop, a band of [`BAND`] rows at a time: the
/// drawings' alphas for the band, then each row.
fn render(fb: &mut Framebuffer, painter: &Painter) -> Result<(), GfxError> {
    let bounds = fb.bounds();
    let edges = Edges::of(bounds);
    let mut columns = Columns::default();
    painter.fill_columns(&mut columns, 0, usize::try_from(edges.width()).unwrap_or(0))?;
    let mut row = Row::default();
    // The drawings' alphas for a band, each across its own box's columns,
    // and room for them to work in.
    let widths = painter
        .drawings
        .each_ref()
        .map(|drawing| usize::try_from(drawing.window.width()).unwrap_or(0));
    let band_len = |width: usize| width.saturating_mul(BAND as usize);
    let mut alphas = [
        zeroed::<f64>(band_len(widths[0]))?,
        zeroed::<f64>(band_len(widths[1]))?,
    ];
    let mut scratch = zeroed::<f64>(band_len(widths[0].max(widths[1])))?;
    let height = edges.height();
    let mut band_top = 0;
    while band_top < height {
        let band_bottom = (band_top + BAND).min(height);
        for (drawing, alpha) in painter.drawings.iter().zip(&mut alphas) {
            let window = Edges {
                top: band_top,
                bottom: band_bottom,
                ..drawing.window
            };
            drawing.fill_window(window, alpha, &mut scratch);
        }
        for y in band_top..band_bottom {
            painter.fill_row(&mut row, y)?;
            let Some((first, span)) = fb.span_mut(bounds, edges.top + y, edges.left, edges.right)
            else {
                continue;
            };
            // Each drawing's alphas for this row.
            let offset = usize::try_from(y - band_top).unwrap_or(0);
            let rows = [0, 1].map(|index| {
                let start = offset.saturating_mul(widths[index]);
                alphas[index]
                    .get(start..start.saturating_add(widths[index]))
                    .unwrap_or(&[])
            });
            for (x, pixel) in
                (first - edges.left..).zip(span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut())
            {
                let drawn = [0, 1].map(|index| {
                    let window = painter.drawings[index].window;
                    if x < window.left || x >= window.right {
                        return 0.0;
                    }
                    let column = usize::try_from(x - window.left).unwrap_or(usize::MAX);
                    rows[index].get(column).copied().unwrap_or(0.0)
                });
                let below = painter.shade(&row, &columns, x);
                pixel.copy_from_slice(&painter.finish(x, below, drawn));
            }
        }
        band_top = band_bottom;
    }
    Ok(())
}

/// The radii of three box blurs whose combined variance best matches a
/// Gaussian of standard deviation `sigma` (Kovesi): `m` boxes of the odd
/// width `w` just under `sqrt(12σ²/3 + 1)` and the rest `w + 2` wide.
pub(crate) fn box_radii(sigma: f64) -> [usize; 3] {
    if sigma.is_nan() || sigma <= 0.0 {
        return [0; 3];
    }
    let sigma = sigma.min(MAX_SIGMA);
    let passes = 3.0;
    let variance = 12.0 * sigma * sigma;
    let ideal = (variance / passes + 1.0).sqrt();
    // The odd width at or just under the ideal one, at least 1. The cast
    // saturates, and `sigma` is capped, so it is far inside `i64`.
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1) as f64;
    let upper = lower + 2.0;
    let count = (variance - passes * lower * lower - 4.0 * passes * lower - 3.0 * passes)
        / (-4.0 * lower - 4.0);
    let count = (count + 0.5).floor().clamp(0.0, passes);
    let radius = |width: f64| ((width - 1.0) / 2.0) as usize;
    let mut radii = [0; 3];
    for (pass, slot) in radii.iter_mut().enumerate() {
        *slot = if (pass as f64) < count {
            radius(lower)
        } else {
            radius(upper)
        };
    }
    radii
}

/// How many columns [`box_blur`] blurs at a time: their pixels are copied
/// out row by row, which reads memory in order.
const STRIP: usize = 16;

/// Blurs `fb` with a box of each of `radii` across every row and then down
/// every column, as the [module documentation](self#the-blurred-copy)
/// describes.
pub(crate) fn box_blur(fb: &mut Framebuffer, radii: [usize; 3]) -> Result<(), GfxError> {
    let bounds = fb.bounds();
    let edges = Edges::of(bounds);
    let (Ok(width), Ok(height)) = (
        usize::try_from(edges.width()),
        usize::try_from(edges.height()),
    ) else {
        return Ok(());
    };
    if width == 0 || height == 0 || radii.iter().all(|&radius| radius == 0) {
        return Ok(());
    }
    let longest = width.max(height) * BYTES_PER_PIXEL;
    let mut line = zeroed::<u8>(longest)?;
    let mut spare = zeroed::<u8>(longest)?;
    // Blurs the first `len` bytes of `line` with every box, leaving the
    // result in `line`.
    let passes = |line: &mut Vec<u8>, spare: &mut Vec<u8>, len: usize| {
        for &radius in &radii {
            if radius > 0 {
                blur_line(
                    line.get(..len).unwrap_or(&[]),
                    spare.get_mut(..len).unwrap_or(&mut []),
                    radius,
                );
                std::mem::swap(line, spare);
            }
        }
    };

    let row_len = width * BYTES_PER_PIXEL;
    for y in edges.top..edges.bottom {
        let Some((_, span)) = fb.span(y, edges.left, edges.right) else {
            continue;
        };
        if let Some(target) = line.get_mut(..row_len) {
            target.copy_from_slice(span);
        }
        passes(&mut line, &mut spare, row_len);
        if let Some((_, span)) = fb.span_mut(bounds, y, edges.left, edges.right)
            && let Some(source) = line.get(..row_len)
        {
            span.copy_from_slice(source);
        }
    }

    // Columns, a strip at a time: `strip` holds each column's pixels in
    // order, one column after another.
    let column_len = height * BYTES_PER_PIXEL;
    let mut strip = zeroed::<u8>(STRIP * column_len)?;
    let mut left = edges.left;
    while left < edges.right {
        let right = (left + STRIP as i64).min(edges.right);
        for (row, y) in (edges.top..edges.bottom).enumerate() {
            let Some((_, span)) = fb.span(y, left, right) else {
                continue;
            };
            for (column, pixel) in span.as_chunks::<BYTES_PER_PIXEL>().0.iter().enumerate() {
                let start = column * column_len + row * BYTES_PER_PIXEL;
                if let Some(target) = strip.get_mut(start..start + BYTES_PER_PIXEL) {
                    target.copy_from_slice(pixel);
                }
            }
        }
        for column in strip
            .chunks_exact_mut(column_len)
            .take(usize::try_from(right - left).unwrap_or(0))
        {
            if let Some(target) = line.get_mut(..column_len) {
                target.copy_from_slice(column);
            }
            passes(&mut line, &mut spare, column_len);
            if let Some(source) = line.get(..column_len) {
                column.copy_from_slice(source);
            }
        }
        for (row, y) in (edges.top..edges.bottom).enumerate() {
            let Some((_, span)) = fb.span_mut(bounds, y, left, right) else {
                continue;
            };
            for (column, pixel) in span
                .as_chunks_mut::<BYTES_PER_PIXEL>()
                .0
                .iter_mut()
                .enumerate()
            {
                let start = column * column_len + row * BYTES_PER_PIXEL;
                if let Some(source) = strip.get(start..start + BYTES_PER_PIXEL) {
                    pixel.copy_from_slice(source);
                }
            }
        }
        left = right;
    }
    Ok(())
}

/// Box-blurs a line of BGRA8 pixels `src` into `dst`, the same length,
/// with a box `2 × radius + 1` pixels wide: each output channel is the
/// window's sum divided by its width, rounded to nearest, with the edge
/// pixels repeated past the ends.
pub(crate) fn blur_line(src: &[u8], dst: &mut [u8], radius: usize) {
    // Far beyond any box `box_radii` gives, and small enough that no sum
    // overflows.
    let radius = radius.min(1 << 24);
    let count = src.len().min(dst.len()) / BYTES_PER_PIXEL;
    if count == 0 {
        return;
    }
    let last = count - 1;
    let pixel = |index: usize| -> [u64; 4] {
        let start = index.min(last) * BYTES_PER_PIXEL;
        match src.get(start..start + BYTES_PER_PIXEL) {
            Some(&[b, g, r, a]) => [b, g, r, a].map(u64::from),
            _ => [0; 4],
        }
    };
    let width = (radius as u64).saturating_mul(2).saturating_add(1);
    let half = radius as u64;
    // The window at pixel 0: the first pixel `radius + 1` times, then
    // pixels 1 to `radius`, the last repeated past the end.
    let mut sum = pixel(0).map(|value| value * (half + 1));
    let inside = radius.min(last);
    for index in 1..=inside {
        let value = pixel(index);
        for (total, value) in sum.iter_mut().zip(value) {
            *total += value;
        }
    }
    let repeats = (radius - inside) as u64;
    for (total, value) in sum.iter_mut().zip(pixel(last)) {
        *total += value * repeats;
    }
    for (index, out) in dst
        .as_chunks_mut::<BYTES_PER_PIXEL>()
        .0
        .iter_mut()
        .take(count)
        .enumerate()
    {
        for (byte, total) in out.iter_mut().zip(sum) {
            // `width` is odd, so the quotient is never exactly a half, and
            // adding `half` (half the width, rounded down) rounds it to
            // nearest. At most 255, since the sum is at most 255 × width.
            *byte = u8::try_from((total + half) / width).unwrap_or(u8::MAX);
        }
        let incoming = pixel(index.saturating_add(radius).saturating_add(1));
        let outgoing = pixel(index.saturating_sub(radius));
        for ((total, add), remove) in sum.iter_mut().zip(incoming).zip(outgoing) {
            *total = *total + add - remove;
        }
    }
}
