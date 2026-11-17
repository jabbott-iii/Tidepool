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

//! Drawing images: [`copy`] (replacing pixels), [`blit`] (source-over) and
//! [`blit_scaled`] (source-over, with a [`Filter`]).
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! # Scaling
//!
//! Destination pixel `i` of a rect `n` pixels wide has its centre at
//! `i + 0.5`, which maps to `(i + 0.5) × m / n` in a source `m` pixels
//! wide. That is computed exactly in integers, as
//! `(2i + 1) × m × 65536 / 2n` (16.16 fixed point, rounded down), so
//! scaling gives the same bytes everywhere.
//!
//! - [`Filter::Nearest`] takes the source pixel containing that point.
//! - [`Filter::Bilinear`] weighs the four source pixels whose centres
//!   surround it by its 16-bit fractional distances, sums in integers and
//!   rounds to nearest, halves up. Outside the source's outer pixel
//!   centres, the edge pixels repeat.
//!
//! An identity scale reproduces the source with either filter.

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect};

use crate::blend::{blend_pixel, from_bgra};
use crate::clip::Edges;
use crate::framebuffer::{BYTES_PER_PIXEL, Framebuffer, View, to_usize};

/// How [`blit_scaled`] samples the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    /// The source pixel under each destination pixel's centre: crisp, and
    /// blocky when enlarging.
    #[default]
    Nearest,
    /// The four source pixels around each destination pixel's centre,
    /// weighed by distance: smooth.
    Bilinear,
}

/// Copies `src` with its top-left pixel at `at`, replacing the pixels it
/// covers inside `clip` and the bounds: for the opaque backdrop.
pub fn copy(fb: &mut Framebuffer, clip: DeviceRect, src: &View<'_>, at: DevicePoint) {
    draw_unscaled(fb, clip, src, at, |target, source| {
        target.copy_from_slice(source)
    });
}

/// Composites `src` with its top-left pixel at `at` over the pixels it
/// covers inside `clip` and the bounds.
pub fn blit(fb: &mut Framebuffer, clip: DeviceRect, src: &View<'_>, at: DevicePoint) {
    draw_unscaled(fb, clip, src, at, |target, source| {
        let pairs = target
            .as_chunks_mut::<BYTES_PER_PIXEL>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<BYTES_PER_PIXEL>().0.iter());
        for (pixel, source) in pairs {
            blend_pixel(pixel, read(source));
        }
    });
}

/// Composites `src` scaled to fill `dst` over the pixels inside `clip` and
/// the bounds, sampled with `filter` as the
/// [module documentation](self) describes. An empty source or `dst` draws
/// nothing.
pub fn blit_scaled(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    src: &View<'_>,
    dst: DeviceRect,
    filter: Filter,
) {
    let size = src.size();
    let edges = Edges::of(dst);
    if size.width <= 0 || size.height <= 0 || edges.is_empty() {
        return;
    }
    let window = fb.window(clip, edges);
    for y in window.top..window.bottom {
        let rows = sample(
            y - edges.top,
            edges.height(),
            i64::from(size.height),
            filter,
        );
        let (top, bottom) = (src.row(rows.first), src.row(rows.second));
        let Some((first, span)) = fb.span_mut(clip, y, window.left, window.right) else {
            continue;
        };
        for (x, pixel) in (first..).zip(span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut()) {
            let columns = sample(x - edges.left, edges.width(), i64::from(size.width), filter);
            let at = |row: &[u8], column: usize| {
                row.get(column * BYTES_PER_PIXEL..(column + 1) * BYTES_PER_PIXEL)
                    .map_or(PremultipliedColor::TRANSPARENT, read)
            };
            let color = match filter {
                Filter::Nearest => at(top, columns.first),
                Filter::Bilinear => bilinear(
                    [
                        at(top, columns.first),
                        at(top, columns.second),
                        at(bottom, columns.first),
                        at(bottom, columns.second),
                    ],
                    columns.fraction,
                    rows.fraction,
                ),
            };
            blend_pixel(pixel, color);
        }
    }
}

/// The source pixels for one destination row or column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    /// The source index, or the lower of two.
    first: usize,
    /// The upper of two; `first` for [`Filter::Nearest`].
    second: usize,
    /// How far towards `second`, in 65536ths.
    fraction: u64,
}

/// One unit in 16.16 fixed point.
const ONE: i128 = 1 << 16;

/// The source sample for destination index `index` of `dst_len` over a
/// source of `src_len` pixels. Both lengths are positive.
fn sample(index: i64, dst_len: i64, src_len: i64, filter: Filter) -> Sample {
    let last = i128::from(src_len) - 1;
    let clamp = |value: i128| to_usize(i32::try_from(value.clamp(0, last)).unwrap_or(0));
    // The centre of destination pixel `index`, in the source, in 16.16:
    // (index + 0.5) × src_len / dst_len, rounded down. At most about
    // 2³² × 2³¹ × 2¹⁶, far inside i128.
    let centre =
        (2 * i128::from(index) + 1) * i128::from(src_len) * ONE / (2 * i128::from(dst_len));
    match filter {
        Filter::Nearest => {
            let index = clamp(centre.div_euclid(ONE));
            Sample {
                first: index,
                second: index,
                fraction: 0,
            }
        }
        Filter::Bilinear => {
            // Between the centres of the pixels at and after `position`.
            let position = centre - ONE / 2;
            let below = position.div_euclid(ONE);
            Sample {
                first: clamp(below),
                second: clamp(below + 1),
                fraction: u64::try_from(position.rem_euclid(ONE)).unwrap_or(0),
            }
        }
    }
}

/// Weighs `[top_left, top_right, bottom_left, bottom_right]` by the
/// fractions `fx` and `fy` (65536ths), rounding each channel to nearest,
/// halves up.
fn bilinear(pixels: [PremultipliedColor; 4], fx: u64, fy: u64) -> PremultipliedColor {
    let one = ONE as u64;
    let weights = [
        (one - fx) * (one - fy),
        fx * (one - fy),
        (one - fx) * fy,
        fx * fy,
    ];
    // Weights sum to 2³²; a channel's sum is at most 255 × 2³².
    let channel = |get: fn(&PremultipliedColor) -> u8| {
        let sum: u64 = pixels
            .iter()
            .zip(weights)
            .map(|(pixel, weight)| u64::from(get(pixel)) * weight)
            .sum();
        u8::try_from((sum + (1 << 31)) >> 32).unwrap_or(u8::MAX)
    };
    PremultipliedColor {
        r: channel(|c| c.r),
        g: channel(|c| c.g),
        b: channel(|c| c.b),
        a: channel(|c| c.a),
    }
}

/// The color of a pixel's four bytes; transparent for a short slice.
fn read(bytes: &[u8]) -> PremultipliedColor {
    <[u8; BYTES_PER_PIXEL]>::try_from(bytes).map_or(PremultipliedColor::TRANSPARENT, from_bgra)
}

/// Hands `draw` each row of the framebuffer that `src` at `at` covers,
/// clipped, with the matching bytes of `src`.
fn draw_unscaled(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    src: &View<'_>,
    at: DevicePoint,
    mut draw: impl FnMut(&mut [u8], &[u8]),
) {
    let size = src.size();
    let (x, y) = (i64::from(at.x), i64::from(at.y));
    let area = Edges {
        left: x,
        top: y,
        right: x + i64::from(size.width),
        bottom: y + i64::from(size.height),
    };
    let window = fb.window(clip, area);
    for row in window.top..window.bottom {
        let Some((first, target)) = fb.span_mut(clip, row, window.left, window.right) else {
            continue;
        };
        let (Ok(source_row), Ok(column)) = (usize::try_from(row - y), usize::try_from(first - x))
        else {
            continue;
        };
        let start = column * BYTES_PER_PIXEL;
        let Some(source) = src.row(source_row).get(start..start + target.len()) else {
            continue;
        };
        draw(target, source);
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::geom::DeviceSize;

    use super::*;
    use crate::blend::to_bgra;
    use crate::clip::SPACE;

    fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColor {
        PremultipliedColor { r, g, b, a }
    }

    /// The bytes of a 2 by 2 image: red, green / blue, half-white.
    fn two_by_two() -> Vec<u8> {
        [
            premultiplied(255, 0, 0, 255),
            premultiplied(0, 255, 0, 255),
            premultiplied(0, 0, 255, 255),
            premultiplied(128, 128, 128, 128),
        ]
        .into_iter()
        .flat_map(to_bgra)
        .collect()
    }

    fn row(fb: &Framebuffer, y: i32) -> Vec<PremultipliedColor> {
        let bounds = fb.bounds();
        (bounds.x..bounds.right())
            .map(|x| fb.pixel(DevicePoint::new(x, y)).expect("inside"))
            .collect()
    }

    #[test]
    fn copy_replaces_and_blit_blends() {
        let bytes = two_by_two();
        let src = View::new(&bytes, DeviceSize::new(2, 2), 8).expect("fits");
        let background = premultiplied(0, 0, 100, 255);
        let mut fb = Framebuffer::new(DeviceSize::new(3, 2)).expect("small");
        fb.clear(background);
        copy(&mut fb, SPACE, &src, DevicePoint::new(1, 0));
        assert_eq!(
            row(&fb, 1),
            [
                background,
                premultiplied(0, 0, 255, 255),
                premultiplied(128, 128, 128, 128)
            ]
        );

        fb.clear(background);
        blit(&mut fb, SPACE, &src, DevicePoint::new(1, 0));
        // Half-white over (0, 0, 100): r = g = 128 + 0, b = 128 +
        // round(100 × 127 / 255) = 128 + 50 = 178, a = 128 + 127 = 255.
        assert_eq!(
            row(&fb, 1),
            [
                background,
                premultiplied(0, 0, 255, 255),
                premultiplied(128, 128, 178, 255)
            ]
        );
    }

    #[test]
    fn copy_clips_and_draws_from_an_offset() {
        let bytes = two_by_two();
        let src = View::new(&bytes, DeviceSize::new(2, 2), 8).expect("fits");
        let mut fb = Framebuffer::new(DeviceSize::new(2, 2)).expect("small");
        // Placed one pixel up and left: only its bottom-right pixel lands.
        copy(&mut fb, SPACE, &src, DevicePoint::new(-1, -1));
        assert_eq!(
            row(&fb, 0),
            [
                premultiplied(128, 128, 128, 128),
                PremultipliedColor::TRANSPARENT
            ]
        );
        assert_eq!(row(&fb, 1), [PremultipliedColor::TRANSPARENT; 2]);
        // Wholly outside, or at the ends of the range: nothing.
        for at in [DevicePoint::new(5, 5), DevicePoint::new(i32::MIN, i32::MAX)] {
            let before = fb.clone();
            copy(&mut fb, SPACE, &src, at);
            blit(&mut fb, SPACE, &src, at);
            assert_eq!(fb, before);
        }
        // Clipped to one pixel.
        let mut fb = Framebuffer::new(DeviceSize::new(2, 2)).expect("small");
        copy(
            &mut fb,
            DeviceRect::new(1, 0, 1, 1),
            &src,
            DevicePoint::ZERO,
        );
        assert_eq!(
            row(&fb, 0),
            [
                PremultipliedColor::TRANSPARENT,
                premultiplied(0, 255, 0, 255)
            ]
        );
        assert_eq!(row(&fb, 1), [PremultipliedColor::TRANSPARENT; 2]);
    }

    #[test]
    fn an_identity_scale_reproduces_the_source() {
        let bytes = two_by_two();
        let src = View::new(&bytes, DeviceSize::new(2, 2), 8).expect("fits");
        for filter in [Filter::Nearest, Filter::Bilinear] {
            let mut fb = Framebuffer::new(DeviceSize::new(2, 2)).expect("small");
            let whole = fb.bounds();
            blit_scaled(&mut fb, SPACE, &src, whole, filter);
            assert_eq!(fb.bytes(), &bytes[..], "{filter:?}");
        }
    }

    #[test]
    fn nearest_doubling_duplicates_pixels() {
        let bytes = two_by_two();
        let src = View::new(&bytes, DeviceSize::new(2, 2), 8).expect("fits");
        let mut fb = Framebuffer::new(DeviceSize::new(4, 4)).expect("small");
        let whole = fb.bounds();
        blit_scaled(&mut fb, SPACE, &src, whole, Filter::Nearest);
        let pick = |x: i32, y: i32| src.pixel(DevicePoint::new(x, y)).expect("inside");
        for y in 0..4 {
            let expected: Vec<_> = (0..4).map(|x| pick(x / 2, y / 2)).collect();
            assert_eq!(row(&fb, y), expected, "row {y}");
        }
        // Halving takes the pixel under each centre: (0.5 × 2 / 1) = 1.
        let mut fb = Framebuffer::new(DeviceSize::new(1, 1)).expect("small");
        let whole = fb.bounds();
        blit_scaled(&mut fb, SPACE, &src, whole, Filter::Nearest);
        assert_eq!(row(&fb, 0), [premultiplied(128, 128, 128, 128)]);
    }

    /// The red channel of every pixel, row by row.
    fn reds(fb: &Framebuffer) -> Vec<Vec<u8>> {
        let bounds = fb.bounds();
        (bounds.y..bounds.bottom())
            .map(|y| row(fb, y).iter().map(|color| color.r).collect())
            .collect()
    }

    /// Opaque pixels with these reds, as bytes.
    fn opaque_reds(reds: &[u8]) -> Vec<u8> {
        reds.iter()
            .flat_map(|&r| to_bgra(premultiplied(r, 0, 0, 255)))
            .collect()
    }

    #[test]
    fn scaling_down_the_rows_samples_the_rows() {
        // A 1 by 2 source, red 10 above 21, stretched down. Row i's centre
        // maps to (i + 0.5) × 2 / n in the source.
        let bytes = opaque_reds(&[10, 21]);
        let src = View::new(&bytes, DeviceSize::new(1, 2), 4).expect("fits");
        let draw = |height: i32, filter: Filter| {
            let mut fb = Framebuffer::new(DeviceSize::new(1, height)).expect("small");
            let whole = fb.bounds();
            blit_scaled(&mut fb, SPACE, &src, whole, filter);
            reds(&fb)
        };
        // Three rows: 1/3, 1 and 5/3, so 0.5 below each the bilinear
        // positions −1/6 (above the first centre: 10), 1/2 (half-way:
        // (10 + 21) / 2 = 15.5, rounded up to 16) and 7/6 (past the last
        // centre: 21).
        assert_eq!(draw(3, Filter::Bilinear), [[10], [16], [21]]);
        // Four rows: positions −1/4 (10), 1/4 (10 + 11/4 = 12.75 → 13),
        // 3/4 (10 + 33/4 = 18.25 → 18) and 5/4 (21).
        assert_eq!(draw(4, Filter::Bilinear), [[10], [13], [18], [21]]);
        // Nearest takes the source row under each centre: 0.25 and 0.75
        // are in row 0, 1.25 and 1.75 in row 1.
        assert_eq!(draw(4, Filter::Nearest), [[10], [10], [21], [21]]);

        // A 2 by 1 source, 10 beside 21, stretched to 2 by 3: every row
        // samples the one source row, and the columns are at scale 1.
        let bytes = opaque_reds(&[10, 21]);
        let src = View::new(&bytes, DeviceSize::new(2, 1), 8).expect("fits");
        for filter in [Filter::Nearest, Filter::Bilinear] {
            let mut fb = Framebuffer::new(DeviceSize::new(2, 3)).expect("small");
            let whole = fb.bounds();
            blit_scaled(&mut fb, SPACE, &src, whole, filter);
            assert_eq!(reds(&fb), [[10, 21], [10, 21], [10, 21]], "{filter:?}");
            assert!(row(&fb, 2).iter().all(|color| color.a == 255), "{filter:?}");
        }
    }

    #[test]
    fn a_bilinear_midpoint_is_the_rounded_average() {
        // A 2 by 1 source of (10, 0, 0, 255) and (21, 0, 0, 255) drawn 3 px
        // wide: pixel 1's centre maps to 1.5 × 2 / 3 = 1.0, which is halfway
        // between the source centres 0.5 and 1.5, so it is the average of
        // 10 and 21, 15.5, rounded up to 16. Pixel 0 maps to 1/3, left of
        // the first centre, so it is the first pixel; pixel 2 is the last.
        let bytes: Vec<u8> = [premultiplied(10, 0, 0, 255), premultiplied(21, 0, 0, 255)]
            .into_iter()
            .flat_map(to_bgra)
            .collect();
        let src = View::new(&bytes, DeviceSize::new(2, 1), 8).expect("fits");
        let mut fb = Framebuffer::new(DeviceSize::new(3, 1)).expect("small");
        let whole = fb.bounds();
        blit_scaled(&mut fb, SPACE, &src, whole, Filter::Bilinear);
        assert_eq!(
            row(&fb, 0).iter().map(|c| c.r).collect::<Vec<_>>(),
            [10, 16, 21]
        );
        // Drawn 4 px wide: centres at 0.25, 0.75, 1.25 and 1.75 of the
        // source; 0.75 is a quarter of the way from 0.5 to 1.5:
        // 10 + 11 / 4 = 12.75 → 13, and 1.25 three quarters: 18.25 → 18.
        let mut fb = Framebuffer::new(DeviceSize::new(4, 1)).expect("small");
        let whole = fb.bounds();
        blit_scaled(&mut fb, SPACE, &src, whole, Filter::Bilinear);
        assert_eq!(
            row(&fb, 0).iter().map(|c| c.r).collect::<Vec<_>>(),
            [10, 13, 18, 21]
        );
    }

    #[test]
    fn sampling_is_exact_at_the_edges_of_the_range() {
        assert_eq!(
            sample(0, 1, 1, Filter::Bilinear),
            Sample {
                first: 0,
                second: 0,
                fraction: 0
            }
        );
        // A destination far wider than the source still maps inside it.
        let wide = sample(
            i64::from(i32::MAX) - 1,
            i64::from(i32::MAX),
            3,
            Filter::Nearest,
        );
        assert_eq!(wide.first, 2);
        let wide = sample(
            i64::from(i32::MAX) - 1,
            i64::from(i32::MAX),
            3,
            Filter::Bilinear,
        );
        assert_eq!((wide.first, wide.second), (2, 2));
    }

    #[test]
    fn scaling_clips_and_ignores_empty_inputs() {
        let bytes = two_by_two();
        let src = View::new(&bytes, DeviceSize::new(2, 2), 8).expect("fits");
        let mut fb = Framebuffer::new(DeviceSize::new(4, 4)).expect("small");
        let whole = fb.bounds();
        blit_scaled(
            &mut fb,
            DeviceRect::new(0, 0, 2, 2),
            &src,
            whole,
            Filter::Nearest,
        );
        assert_eq!(row(&fb, 1)[..2], [premultiplied(255, 0, 0, 255); 2]);
        assert_eq!(row(&fb, 1)[2..], [PremultipliedColor::TRANSPARENT; 2]);
        assert_eq!(row(&fb, 2), [PremultipliedColor::TRANSPARENT; 4]);
        let before = fb.clone();
        let empty = View::new(&[], DeviceSize::ZERO, 0).expect("empty");
        let whole = fb.bounds();
        blit_scaled(&mut fb, SPACE, &empty, whole, Filter::Bilinear);
        blit_scaled(
            &mut fb,
            SPACE,
            &src,
            DeviceRect::new(0, 0, -4, 4),
            Filter::Bilinear,
        );
        assert_eq!(fb, before);
    }
}
