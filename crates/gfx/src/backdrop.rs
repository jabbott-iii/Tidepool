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
//! 3. The planet, `.space__planet`: a circle placed by CSS `right` and
//!    `top`, filled with its `radial-gradient(circle at 50% 100%, …)`,
//!    whose size is `farthest-corner`; under it, outside the circle, its
//!    outer glow (`box-shadow: 0 40px 140px`) and above that its 1 px ring
//!    (`0 0 0 1px`); over the fill, inside, its inner glow
//!    (`inset 0 -18px 60px`); the whole at the planet's `opacity`.
//! 4. The near and then the far star tile (`.space__stars`, and
//!    `.space__stars--far` after it): the same dots in tiles of two sizes,
//!    whose origin is offset by the layer's `inset`, each at its opacity. A
//!    dot is a tiny `radial-gradient`; one that falls outside a tile (the
//!    far tile is smaller) is cut off there, as in CSS.
//! 5. The grid, `.space__grid`: 1 px lines every 64 px, horizontal ones
//!    over vertical ones, under its elliptical `mask-image`.
//!
//! There is no drift: the stars' animation is ambient motion, which is
//! opt-in and off (`docs/subsystems/ui-and-theme.md`).
//!
//! # Approximations
//!
//! It is drawn to look like the site and to give the same bytes on every
//! CPU, not to match a browser pixel for pixel:
//!
//! - **Arithmetic.** Each pixel is computed in `f64` with `+ − × ÷`,
//!   `sqrt`, `floor`, `min`, `max` and `clamp` only (no `exp`, `erf` or
//!   other libm functions), as premultiplied color from 0 to 1, and
//!   rounded to bytes once, as `floor(v × 255 + 0.5)`, rather than after
//!   each layer as a browser does, which keeps the faint layers from
//!   banding.
//! - **Sampling.** Gradients, the planet and the grid's mask are evaluated
//!   at the pixel's centre; the grid's lines are box-filtered (below); the
//!   star dots, smaller than a pixel, at a fixed 4 × 4 grid of points in
//!   the pixel, averaged.
//! - **Edges.** The planet's circle and ring are anti-aliased with a 1 px
//!   linear ramp, `clamp(r − d + 0.5, 0, 1)` for a pixel centre `d` from
//!   the centre; the ring is the ramp of its outer edge minus the planet's.
//!   Where the planet's edge crosses a pixel, its outside layers and its
//!   inside layers are mixed by that coverage, standing in for CSS's clip
//!   of outer shadows to the outside of the box and of the background and
//!   inset shadow to the inside.
//! - **Blurred shadows.** A CSS blur radius `b` is a Gaussian of standard
//!   deviation `σ = b / 2`. The planet is so much larger than its blurs
//!   that its shadows are treated as blurred straight edges: the Gaussian's
//!   integral across the edge is replaced by a smoothstep,
//!   `3t² − 2t³` over `x` from `−2σ` to `2σ` (at least ±0.5 px), which is
//!   within 0.03 of the true curve and reaches 0 and 1 at `∓2σ`, where
//!   the true one is within 0.023 of them. The outer glow is
//!   `smoothstep(R − d′)` for `d′` the distance from the shadow's offset
//!   centre; the inner glow is `1 − smoothstep(R − d″)`.
//! - **The grid.** Each pixel's coverage is the exact length of its row or
//!   column that the lines cover (a box filter), so lines stay 1 CSS px
//!   wide at any scale; the mask is evaluated at the pixel's centre.
//! - **Colors.** The site's colors are 8-bit [`Color`]s, so an alpha such
//!   as 0.022 is stored as `round(0.022 × 255) = 6`, which is 0.0235, as
//!   [`Color::parse`] would read the CSS.
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

#[cfg(test)]
mod tests;

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

/// A CSS `box-shadow` of the planet: offset, blur radius and color.
#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    /// The horizontal offset, in CSS px.
    pub offset_x: f32,
    /// The vertical offset, in CSS px (down is positive).
    pub offset_y: f32,
    /// The blur radius, in CSS px: twice the Gaussian's standard
    /// deviation.
    pub blur: f32,
    /// The color.
    pub color: Color,
}

/// The planet, `.space__planet`.
#[derive(Clone, Debug, PartialEq)]
pub struct Planet {
    /// The width and height of its box, in CSS px; it is a circle
    /// (`border-radius: 50%`).
    pub diameter: f32,
    /// CSS `right`: from the backdrop's right edge in to the box's, in CSS
    /// px (negative is past it).
    pub right: f32,
    /// CSS `top`: from the backdrop's top edge down to the box's, in CSS
    /// px.
    pub top: f32,
    /// The fill gradient's centre x, as a fraction of the box (`50%`).
    pub fill_center_x: f32,
    /// The fill gradient's centre y, as a fraction of the box (`100%`).
    pub fill_center_y: f32,
    /// The fill's color at its centre (`0%`).
    pub core_color: Color,
    /// The fill's color at [`Planet::mid_stop`].
    pub mid_color: Color,
    /// Where the fill is [`Planet::mid_color`], as a fraction of its radius
    /// (the distance to the box's farthest corner).
    pub mid_stop: f32,
    /// Where the fill has faded to transparent, as a fraction of its
    /// radius.
    pub fade_stop: f32,
    /// The ring's color (`box-shadow: 0 0 0 1px …`).
    pub ring_color: Color,
    /// The ring's width (the shadow's spread), in CSS px.
    pub ring_width: f32,
    /// The outer glow, drawn outside the circle.
    pub glow: Shadow,
    /// The inner glow (an `inset` shadow), drawn inside the circle.
    pub inner_glow: Shadow,
    /// The whole planet's opacity, from 0 to 1.
    pub opacity: f32,
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
/// (`tidepool_ui::theme`, M3.2) does not carry a backdrop style: its
/// component styles come with the controls (M3.6, ADR-0047), and until then
/// `gui` draws `jabbott_dev()`, whose `bg` a test of `gui` holds to the
/// theme's. Equality lets the cache see a change of style.
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropStyle {
    /// The color under everything (`var(--bg)`).
    pub bg: Color,
    /// The radial gradients, in CSS order: the first is painted on top.
    pub glows: Vec<Glow>,
    /// The planet.
    pub planet: Planet,
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
    /// The blur of translucent panels' backdrop, in CSS px: the Gaussian's
    /// standard deviation (`backdrop-filter: blur(10px)`).
    pub blur_radius: f32,
}

impl BackdropStyle {
    /// The backdrop of jabbott.dev, from its stylesheet
    /// (`jabbott-iii/jabbott-iii.github.io`, `assets/css/style.css`; the
    /// comments give its line numbers). Alphas are `round(A × 255)`, as
    /// [`Color::parse`] reads `rgba(…)`.
    pub fn jabbott_dev() -> Self {
        // `rgba(92, 203, 203, A)`, the teal of the glows.
        let teal = |a| Color::rgba(92, 203, 203, a);
        let white = |a| Color::rgba(255, 255, 255, a);
        let star = |x, y, radius, color| Star {
            x,
            y,
            radius,
            color,
            // `transparent 60%` on every dot (lines 124–130).
            fade_stop: 0.6,
        };
        Self {
            // `--bg: #0b0c0d` (line 5), `.space`'s last layer (line 105).
            bg: Color::rgb(0x0b, 0x0c, 0x0d),
            glows: vec![
                // Line 102: `radial-gradient(1100px 620px at 85% -10%,
                // rgba(92, 203, 203, 0.12), transparent 62%)`; 0.12 → 31.
                Glow {
                    color: teal(31),
                    radius_x: 1100.0,
                    radius_y: 620.0,
                    center_x: 0.85,
                    center_y: -0.10,
                    fade_stop: 0.62,
                },
                // Line 103: `radial-gradient(900px 520px at -8% 22%,
                // rgba(242, 194, 0, 0.05), transparent 60%)`; 0.05 → 13.
                Glow {
                    color: Color::rgba(242, 194, 0, 13),
                    radius_x: 900.0,
                    radius_y: 520.0,
                    center_x: -0.08,
                    center_y: 0.22,
                    fade_stop: 0.60,
                },
                // Line 104: `radial-gradient(1400px 900px at 50% 125%,
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
            planet: Planet {
                // Line 110: `width: 1800px; height: 1800px`.
                diameter: 1800.0,
                // Line 111: `right: -620px; top: -1580px`.
                right: -620.0,
                top: -1580.0,
                // Line 113: `radial-gradient(circle at 50% 100%, #182a30 0%,
                // #0d1214 38%, transparent 62%)`.
                fill_center_x: 0.5,
                fill_center_y: 1.0,
                core_color: Color::rgb(0x18, 0x2a, 0x30),
                mid_color: Color::rgb(0x0d, 0x12, 0x14),
                mid_stop: 0.38,
                fade_stop: 0.62,
                // Line 115: `0 0 0 1px rgba(143, 229, 229, 0.18)`; 0.18 → 46.
                ring_color: Color::rgba(143, 229, 229, 46),
                ring_width: 1.0,
                // Line 116: `0 40px 140px rgba(92, 203, 203, 0.18)`.
                glow: Shadow {
                    offset_x: 0.0,
                    offset_y: 40.0,
                    blur: 140.0,
                    color: teal(46),
                },
                // Line 117: `inset 0 -18px 60px rgba(92, 203, 203, 0.12)`.
                inner_glow: Shadow {
                    offset_x: 0.0,
                    offset_y: -18.0,
                    blur: 60.0,
                    color: teal(31),
                },
                // Line 118: `opacity: 0.9`.
                opacity: 0.9,
            },
            stars: vec![
                // Line 124: `1px 1px at 25px 35px, rgba(255,255,255,0.9)`.
                star(25.0, 35.0, 1.0, white(230)),
                // Line 125: `1px 1px at 75px 125px, …0.55`.
                star(75.0, 125.0, 1.0, white(140)),
                // Line 126: `1.6px 1.6px at 162px 62px,
                // rgba(205, 245, 255, 0.85)`.
                star(162.0, 62.0, 1.6, Color::rgba(205, 245, 255, 217)),
                // Line 127: `1px 1px at 214px 192px, …0.5`.
                star(214.0, 192.0, 1.0, white(128)),
                // Line 128: `1px 1px at 118px 232px, …0.7`.
                star(118.0, 232.0, 1.0, white(179)),
                // Line 129: `1.2px 1.2px at 272px 22px,
                // rgba(255, 236, 200, 0.7)`.
                star(272.0, 22.0, 1.2, Color::rgba(255, 236, 200, 179)),
                // Line 130: `1px 1px at 286px 148px, …0.45`.
                star(286.0, 148.0, 1.0, white(115)),
            ],
            // Line 122: `inset: -260px`.
            star_inset: -260.0,
            // Lines 131–132: `background-size: 300px 260px; opacity: 0.55`.
            near_stars: StarTile {
                width: 300.0,
                height: 260.0,
                opacity: 0.55,
            },
            // Lines 137–138: `background-size: 190px 170px; opacity: 0.28`.
            far_stars: StarTile {
                width: 190.0,
                height: 170.0,
                opacity: 0.28,
            },
            grid: Grid {
                // Line 147: `background-size: 64px 64px`.
                cell: 64.0,
                // Lines 145–146: `linear-gradient(rgba(255, 255, 255, 0.022)
                // 1px, transparent 1px)`, once down and once across;
                // 0.022 → 6.
                line_width: 1.0,
                color: white(6),
                // Line 149: `radial-gradient(ellipse 80% 60% at 50% 0%,
                // #000 0%, transparent 75%)`.
                mask_radius_x: 0.8,
                mask_radius_y: 0.6,
                mask_center_x: 0.5,
                mask_center_y: 0.0,
                mask_fade_stop: 0.75,
            },
            // Lines 311–312: `.panel { backdrop-filter: blur(10px) }`.
            blur_radius: 10.0,
        }
    }

    /// Checks every value: all are finite; sizes, radii, widths, blurs and
    /// stops are not negative; opacities are from 0 to 1. Positions and
    /// offsets may be negative.
    ///
    /// # Errors
    ///
    /// [`GfxError::InvalidStyle`] naming the first field that is not.
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
        let planet = &self.planet;
        for (value, field, rule) in [
            (planet.diameter, "planet.diameter", NonNegative),
            (planet.right, "planet.right", Finite),
            (planet.top, "planet.top", Finite),
            (planet.fill_center_x, "planet.fill_center_x", Finite),
            (planet.fill_center_y, "planet.fill_center_y", Finite),
            (planet.mid_stop, "planet.mid_stop", NonNegative),
            (planet.fade_stop, "planet.fade_stop", NonNegative),
            (planet.ring_width, "planet.ring_width", NonNegative),
            (planet.glow.offset_x, "planet.glow.offset_x", Finite),
            (planet.glow.offset_y, "planet.glow.offset_y", Finite),
            (planet.glow.blur, "planet.glow.blur", NonNegative),
            (
                planet.inner_glow.offset_x,
                "planet.inner_glow.offset_x",
                Finite,
            ),
            (
                planet.inner_glow.offset_y,
                "planet.inner_glow.offset_y",
                Finite,
            ),
            (
                planet.inner_glow.blur,
                "planet.inner_glow.blur",
                NonNegative,
            ),
            (planet.opacity, "planet.opacity", Opacity),
        ] {
            check(value, field, None, rule)?;
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
    ///   [validate](BackdropStyle::validate).
    /// - The errors of [`Framebuffer::new`] for `size`.
    /// - [`GfxError::OutOfMemory`] for the blurred copy or its buffers.
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
        let mut base = Framebuffer::new(size)?;
        let painter = Painter::new(size, scale, style);
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

/// From `a` at 0 to `b` at 1.
fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    plus(times(a, 1.0 - t), times(b, t))
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

/// The blurred-edge approximation of the
/// [module documentation](self#approximations): how much of a shape
/// blurred with standard deviation `sigma` covers a point `x` inside its
/// edge (negative outside).
fn edge(x: f64, sigma: f64) -> f64 {
    let reach = (2.0 * sigma).max(0.5);
    let t = ((x + reach) / (2.0 * reach)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A 1 px anti-aliasing ramp for a point `x` inside an edge.
fn ramp(x: f64) -> f64 {
    (x + 0.5).clamp(0.0, 1.0)
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

/// The planet in device pixels.
#[derive(Debug)]
struct PlanetPaint {
    /// Beyond this distance from the centre nothing of the planet shows.
    reach: f64,
    center_x: f64,
    center_y: f64,
    radius: f64,
    ring_radius: f64,
    ring: Rgba,
    fill_x: f64,
    fill_y: f64,
    fill_radius: f64,
    core: Rgba,
    mid: Rgba,
    mid_stop: f64,
    fade_stop: f64,
    glow: Rgba,
    glow_x: f64,
    glow_y: f64,
    glow_sigma: f64,
    inner: Rgba,
    inner_x: f64,
    inner_y: f64,
    inner_sigma: f64,
    opacity: f64,
}

impl PlanetPaint {
    fn new(planet: &Planet, width: f64, scale: f64) -> Self {
        let diameter = f64::from(planet.diameter) * scale;
        let radius = diameter / 2.0;
        let left = width - (f64::from(planet.right) + f64::from(planet.diameter)) * scale;
        let top = f64::from(planet.top) * scale;
        let (center_x, center_y) = (left + radius, top + radius);
        let fill_x = left + f64::from(planet.fill_center_x) * diameter;
        let fill_y = top + f64::from(planet.fill_center_y) * diameter;
        // `farthest-corner`: the distance to the box's farthest corner.
        let far_x = (fill_x - left).abs().max((left + diameter - fill_x).abs());
        let far_y = (fill_y - top).abs().max((top + diameter - fill_y).abs());
        let fill_radius = (far_x * far_x + far_y * far_y).sqrt();
        let ring_radius = radius + f64::from(planet.ring_width) * scale;
        let shadow = |shadow: &Shadow| {
            (
                center_x + f64::from(shadow.offset_x) * scale,
                center_y + f64::from(shadow.offset_y) * scale,
                f64::from(shadow.blur) / 2.0 * scale,
            )
        };
        let (glow_x, glow_y, glow_sigma) = shadow(&planet.glow);
        let (inner_x, inner_y, inner_sigma) = shadow(&planet.inner_glow);
        // The glow shows while `R − d′ > −max(2σ, 0.5)`, and `d′` is at
        // least `d` less the offset, whose length is at most the sum of its
        // two sides; the ring and the circle show within half a pixel.
        let glow_offset = (glow_x - center_x).abs() + (glow_y - center_y).abs();
        let reach = if diameter > 0.0 {
            (radius + (2.0 * glow_sigma).max(0.5) + glow_offset).max(ring_radius + 0.5) + 1.0
        } else {
            // Nothing shows.
            0.0
        };
        Self {
            reach,
            center_x,
            center_y,
            radius,
            ring_radius,
            ring: premultiply(planet.ring_color),
            fill_x,
            fill_y,
            fill_radius,
            core: premultiply(planet.core_color),
            mid: premultiply(planet.mid_color),
            mid_stop: f64::from(planet.mid_stop),
            fade_stop: f64::from(planet.fade_stop).max(f64::from(planet.mid_stop)),
            glow: premultiply(planet.glow.color),
            glow_x,
            glow_y,
            glow_sigma,
            inner: premultiply(planet.inner_glow.color),
            inner_x,
            inner_y,
            inner_sigma,
            opacity: f64::from(planet.opacity),
        }
    }

    /// The fill gradient at `t` of its radius.
    fn fill(&self, t: f64) -> Rgba {
        if t <= 0.0 {
            self.core
        } else if t < self.mid_stop {
            lerp(self.core, self.mid, t / self.mid_stop)
        } else if t < self.fade_stop {
            lerp(
                self.mid,
                CLEAR,
                (t - self.mid_stop) / (self.fade_stop - self.mid_stop),
            )
        } else {
            CLEAR
        }
    }

    /// The planet at the point (`x`, `y`) over `below`.
    fn paint(&self, x: f64, y: f64, below: Rgba) -> Rgba {
        let distance = |cx: f64, cy: f64| {
            let (dx, dy) = (x - cx, y - cy);
            (dx * dx + dy * dy).sqrt()
        };
        let (dx, dy) = (x - self.center_x, y - self.center_y);
        let squared = dx * dx + dy * dy;
        if self.reach <= 0.0
            || self.opacity <= 0.0
            || squared.is_nan()
            || squared >= self.reach * self.reach
        {
            return below;
        }
        let d = squared.sqrt();
        let inside = ramp(self.radius - d);
        // Each side is skipped where it is weighed by exactly 0, which
        // gives the same result as computing it.
        let outside = if inside < 1.0 {
            // Outside the circle: the ring over the outer glow.
            let glow = times(
                self.glow,
                edge(
                    self.radius - distance(self.glow_x, self.glow_y),
                    self.glow_sigma,
                ),
            );
            let ring = times(self.ring, ramp(self.ring_radius - d));
            over(ring, glow)
        } else {
            CLEAR
        };
        let within = if inside > 0.0 {
            // Inside: the inner glow over the fill.
            let t = if self.fill_radius > 0.0 {
                distance(self.fill_x, self.fill_y) / self.fill_radius
            } else {
                0.0
            };
            let inner_amount = 1.0
                - edge(
                    self.radius - distance(self.inner_x, self.inner_y),
                    self.inner_sigma,
                );
            over(times(self.inner, inner_amount), self.fill(t))
        } else {
            CLEAR
        };
        let planet = plus(times(outside, 1.0 - inside), times(within, inside));
        over(times(planet, self.opacity), below)
    }
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
    planet: PlanetPaint,
    stars: Vec<StarPaint>,
    inset: f64,
    near: TilePaint,
    far: TilePaint,
    grid: GridPaint,
}

/// The fixed supersampling pattern for stars: 4 × 4 points in a pixel.
const SAMPLES: usize = 4;

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
    planet: bool,
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
    fn new(size: DeviceSize, scale: Scale, style: &BackdropStyle) -> Self {
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
        Self {
            scale: s,
            bg: premultiply(style.bg),
            glows,
            planet: PlanetPaint::new(&style.planet, width, s),
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
        }
    }

    /// The pixel at column `x` and row `y`, as bytes in memory order: what
    /// [`render`] writes there.
    #[cfg(test)]
    fn pixel(&self, x: i64, y: i64) -> [u8; 4] {
        let mut row = Row::default();
        let mut columns = Columns::default();
        self.fill_row(&mut row, y).expect("a few values");
        self.fill_columns(&mut columns, x, 1).expect("one column");
        self.shade(&row, &columns, x)
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
        let planet = &self.planet;
        row.planet = planet.reach > 0.0
            && planet.opacity > 0.0
            && (centre - planet.center_y).abs() < planet.reach;
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

    /// The pixel at column `x` of `row`, whose column terms `columns` holds.
    fn shade(&self, row: &Row, columns: &Columns, x: i64) -> [u8; 4] {
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
        if row.planet {
            color = self.planet.paint(centre_x, row.centre, color);
        }
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

/// Fills `fb` with the backdrop, row by row.
fn render(fb: &mut Framebuffer, painter: &Painter) -> Result<(), GfxError> {
    let bounds = fb.bounds();
    let edges = Edges::of(bounds);
    let mut columns = Columns::default();
    painter.fill_columns(&mut columns, 0, usize::try_from(edges.width()).unwrap_or(0))?;
    let mut row = Row::default();
    for y in edges.top..edges.bottom {
        painter.fill_row(&mut row, y - edges.top)?;
        let Some((first, span)) = fb.span_mut(bounds, y, edges.left, edges.right) else {
            continue;
        };
        for (x, pixel) in
            (first - edges.left..).zip(span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut())
        {
            pixel.copy_from_slice(&painter.shade(&row, &columns, x));
        }
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
