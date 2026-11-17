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

//! Blending: source-over on premultiplied pixels, [`scale`] by a coverage
//! or an opacity, and [`composite`], which lays a layer over a framebuffer
//! with an opacity.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! All of it is integer arithmetic that rounds to nearest, so it gives the
//! same bytes everywhere. Source-over is
//! [`PremultipliedColor::over`]: `src + round(dst × (255 − src.a) / 255)`
//! per channel. Because 255 is odd, `x × y / 255` is never exactly halfway
//! between two integers, so rounding to nearest has one answer.

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::DeviceRect;

use crate::framebuffer::{BYTES_PER_PIXEL, Framebuffer};

/// `color` as the four bytes of a pixel, in memory order: blue, green,
/// red, alpha.
pub const fn to_bgra(color: PremultipliedColor) -> [u8; 4] {
    [color.b, color.g, color.r, color.a]
}

/// The color of a pixel's four bytes, in memory order: blue, green, red,
/// alpha.
pub const fn from_bgra(pixel: [u8; 4]) -> PremultipliedColor {
    let [b, g, r, a] = pixel;
    PremultipliedColor { r, g, b, a }
}

/// `src` composited over `dst` (Porter–Duff source-over), rounding to
/// nearest: [`PremultipliedColor::over`].
pub const fn over(src: PremultipliedColor, dst: PremultipliedColor) -> PremultipliedColor {
    src.over(dst)
}

/// `color` with all four channels multiplied by `amount / 255`, rounding
/// to nearest: a coverage (from a mask or an anti-aliased edge) or an
/// opacity. 255 keeps the color; 0 makes it transparent.
pub const fn scale(color: PremultipliedColor, amount: u8) -> PremultipliedColor {
    PremultipliedColor {
        r: mul_div_255(color.r, amount),
        g: mul_div_255(color.g, amount),
        b: mul_div_255(color.b, amount),
        a: mul_div_255(color.a, amount),
    }
}

/// Composites `layer` over `dst` where it lies, its own bounds, with its
/// alpha scaled by `opacity`: each pixel inside `clip`, `dst`'s bounds and
/// `layer`'s bounds becomes `scale(layer, opacity)` over `dst`.
///
/// A layer is drawn into its own [`Framebuffer::new_at`] covering just the
/// part of the window it needs; this is CSS's `opacity` on a group.
pub fn composite(dst: &mut Framebuffer, clip: DeviceRect, layer: &Framebuffer, opacity: u8) {
    if opacity == 0 {
        return;
    }
    let window = dst.window(clip, crate::clip::Edges::of(layer.bounds()));
    for y in window.top..window.bottom {
        let Some((x, target)) = dst.span_mut(clip, y, window.left, window.right) else {
            continue;
        };
        let count = i64::try_from(target.len() / BYTES_PER_PIXEL).unwrap_or(0);
        let Some((source_x, source)) = layer.span(y, x, x + count) else {
            continue;
        };
        if source_x != x {
            continue;
        }
        let pairs = target
            .as_chunks_mut::<BYTES_PER_PIXEL>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<BYTES_PER_PIXEL>().0.iter());
        for (pixel, layer_pixel) in pairs {
            let mut color = [0; BYTES_PER_PIXEL];
            color.copy_from_slice(layer_pixel);
            blend_pixel(pixel, scale(from_bgra(color), opacity));
        }
    }
}

/// Composites `src` over the pixel `pixel`, four bytes in memory order.
pub(crate) fn blend_pixel(pixel: &mut [u8], src: PremultipliedColor) {
    if let [b, g, r, a] = pixel {
        let dst = PremultipliedColor {
            r: *r,
            g: *g,
            b: *b,
            a: *a,
        };
        let out = src.over(dst);
        (*b, *g, *r, *a) = (out.b, out.g, out.r, out.a);
    }
}

/// Composites `color` over every pixel of `span`.
pub(crate) fn fill_span(span: &mut [u8], color: PremultipliedColor) {
    if color == PremultipliedColor::TRANSPARENT {
        // `over` would give back every pixel unchanged.
        return;
    }
    if color.a == u8::MAX {
        // `over` would give back `color` exactly: it keeps none of `dst`.
        let bgra = to_bgra(color);
        for pixel in span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut() {
            pixel.copy_from_slice(&bgra);
        }
        return;
    }
    for pixel in span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut() {
        blend_pixel(pixel, color);
    }
}

/// `round(x × y / 255)`, exactly, as `tidepool_core::color` computes it.
const fn mul_div_255(x: u8, y: u8) -> u8 {
    // At most (255 × 255 + 127) / 255 = 255.
    ((x as u32 * y as u32 + 127) / 255) as u8
}

#[cfg(test)]
mod tests {
    use tidepool_core::color::Color;
    use tidepool_core::geom::{DevicePoint, DeviceSize};

    use super::*;

    fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColor {
        PremultipliedColor { r, g, b, a }
    }

    #[test]
    fn byte_order_is_blue_green_red_alpha() {
        let color = premultiplied(1, 2, 3, 4);
        assert_eq!(to_bgra(color), [3, 2, 1, 4]);
        assert_eq!(from_bgra([3, 2, 1, 4]), color);
    }

    #[test]
    fn scale_rounds_every_channel_to_nearest_exhaustively() {
        for amount in 0..=255u8 {
            for value in 0..=255u8 {
                // round(v × a / 255) written as floor((2va + 255) / 510).
                let expected = (2 * u32::from(value) * u32::from(amount) + 255) / 510;
                let out = scale(premultiplied(value, value, value, value), amount);
                assert_eq!(u32::from(out.r), expected, "{value} × {amount}");
                assert_eq!((out.g, out.b, out.a), (out.r, out.r, out.r));
            }
        }
        let teal = Color::rgb(0x5c, 0xcb, 0xcb).premultiply();
        assert_eq!(scale(teal, 255), teal);
        assert_eq!(scale(teal, 0), PremultipliedColor::TRANSPARENT);
        // Half coverage of teal: round(92 × 128 / 255) = round(46.18) = 46,
        // round(203 × 128 / 255) = round(101.9) = 102, and alpha 128.
        assert_eq!(scale(teal, 128), premultiplied(46, 102, 102, 128));
    }

    #[test]
    fn blending_a_pixel_is_source_over() {
        // Half-transparent blue (0, 0, 128, 128) over opaque (200, 100, 50):
        // each channel keeps round(d × 127 / 255) of the destination.
        // r = 0 + round(200 × 127 / 255) = round(99.6) = 100
        // g = 0 + round(100 × 127 / 255) = round(49.8) = 50
        // b = 128 + round(50 × 127 / 255) = 128 + round(24.9) = 153
        // a = 128 + round(255 × 127 / 255) = 255
        let mut pixel = to_bgra(premultiplied(200, 100, 50, 255));
        blend_pixel(&mut pixel, premultiplied(0, 0, 128, 128));
        assert_eq!(pixel, [153, 50, 100, 255]);
        assert_eq!(
            over(
                premultiplied(0, 0, 128, 128),
                premultiplied(200, 100, 50, 255)
            ),
            premultiplied(100, 50, 153, 255)
        );
    }

    #[test]
    fn fill_span_fast_paths_agree_with_over() {
        let colors = [
            PremultipliedColor::TRANSPARENT,
            premultiplied(10, 20, 30, 255),
            premultiplied(10, 20, 30, 40),
        ];
        let background = premultiplied(90, 80, 70, 200);
        for color in colors {
            let mut span = [to_bgra(background), to_bgra(background)].concat();
            fill_span(&mut span, color);
            let expected = to_bgra(color.over(background));
            assert_eq!(&span[..4], &expected, "{color:?}");
            assert_eq!(&span[4..], &expected, "{color:?}");
        }
    }

    #[test]
    fn composite_lays_a_layer_over_with_opacity() {
        let mut dst = Framebuffer::new(DeviceSize::new(4, 1)).expect("small");
        dst.clear(premultiplied(0, 0, 200, 255));
        // A two-pixel layer at columns 1 and 2, both opaque red; then
        // half-transparent green composited over its second column only,
        // at full opacity: (0, 128, 0, 128) over (255, 0, 0, 255) is
        // (round(255 × 127 / 255), 128, 0, 255) = (127, 128, 0, 255).
        let mut layer = Framebuffer::new_at(DeviceRect::new(1, 0, 2, 1)).expect("small");
        layer.clear(premultiplied(255, 0, 0, 255));
        let mut green = layer.try_clone().expect("small");
        green.clear(premultiplied(0, 128, 0, 128));
        composite(&mut layer, DeviceRect::new(2, 0, 1, 1), &green, 255);
        assert_eq!(
            layer.pixel(DevicePoint::new(1, 0)),
            Some(premultiplied(255, 0, 0, 255))
        );
        assert_eq!(
            layer.pixel(DevicePoint::new(2, 0)),
            Some(premultiplied(127, 128, 0, 255))
        );

        // At opacity 128: red becomes (128, 0, 0, 128), and over the blue
        // background gives (128, 0, round(200 × 127 / 255), 255)
        // = (128, 0, 100, 255); (127, 128, 0, 255) becomes
        // (round(127 × 128 / 255), round(128 × 128 / 255), 0, 128)
        // = (64, 64, 0, 128), giving (64, 64, 100, 255).
        composite(&mut dst, DeviceRect::new(0, 0, 100, 100), &layer, 128);
        let row: Vec<_> = (0..4)
            .map(|x| dst.pixel(DevicePoint::new(x, 0)).expect("inside"))
            .collect();
        assert_eq!(
            row,
            [
                premultiplied(0, 0, 200, 255),
                premultiplied(128, 0, 100, 255),
                premultiplied(64, 64, 100, 255),
                premultiplied(0, 0, 200, 255),
            ]
        );
    }

    #[test]
    fn composite_respects_the_clip_and_skips_zero_opacity() {
        let mut dst = Framebuffer::new(DeviceSize::new(3, 1)).expect("small");
        let mut layer = Framebuffer::new(DeviceSize::new(3, 1)).expect("small");
        layer.clear(premultiplied(255, 255, 255, 255));
        composite(&mut dst, DeviceRect::new(0, 0, 3, 1), &layer, 0);
        assert!(dst.bytes().iter().all(|&byte| byte == 0));
        composite(&mut dst, DeviceRect::new(1, 0, 1, 1), &layer, 255);
        assert_eq!(dst.bytes(), &[0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
    }
}
