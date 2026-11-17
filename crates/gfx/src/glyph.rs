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

//! Drawing coverage masks in a color: [`Mask`] and [`draw`], for glyphs
//! from `font`'s atlas (M2.5) and monochrome icons.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! A mask is one byte of coverage per pixel, 0 for none and 255 for full,
//! in rows a stride apart. The atlas is one big mask, and hands out each
//! glyph as a [`Mask::region`] of it.

use std::fmt;

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize};

use crate::GfxError;
use crate::blend::{blend_pixel, scale};
use crate::clip::Edges;
use crate::framebuffer::{BYTES_PER_PIXEL, Framebuffer, check_shape, region_bytes, row_bytes};

/// A borrowed, read-only 8-bit coverage mask: one byte per pixel, in rows
/// `stride` bytes apart.
///
/// [`Mask::new`] checks its shape as [`View::new`](crate::framebuffer::View::new)
/// does. Equality compares the bytes, size and stride. `Debug` shows the
/// size and stride, not the bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Mask<'a> {
    bytes: &'a [u8],
    size: DeviceSize,
    stride: usize,
}

impl<'a> Mask<'a> {
    /// A mask of `size` pixels over `bytes`, in rows `stride` bytes apart.
    /// Bytes past the last row are left out.
    ///
    /// # Errors
    ///
    /// - [`GfxError::NegativeSize`] if a side is negative. Zero is
    ///   allowed: such a mask has no pixels.
    /// - [`GfxError::StrideTooSmall`] if a row does not fit in `stride`.
    /// - [`GfxError::TooFewBytes`] if `bytes` is shorter than `stride`
    ///   times the height.
    pub fn new(bytes: &'a [u8], size: DeviceSize, stride: usize) -> Result<Self, GfxError> {
        let bytes = check_shape(bytes, size, stride, 1)?;
        Ok(Self {
            bytes,
            size,
            stride,
        })
    }

    /// The part of the mask inside `rect`, relative to its top-left pixel.
    ///
    /// # Errors
    ///
    /// [`GfxError::OutsideView`] if `rect` has a negative side or does not
    /// lie inside the mask.
    pub fn region(&self, rect: DeviceRect) -> Result<Mask<'a>, GfxError> {
        let bytes = region_bytes(self.bytes, self.size, self.stride, 1, rect)?;
        Ok(Mask {
            bytes,
            size: DeviceSize::new(rect.width, rect.height),
            stride: self.stride,
        })
    }

    /// The bytes, from the first pixel: `stride` × height of them, or for
    /// a region up to the end of the bytes it was made from if that comes
    /// first, so its last row may end before a whole stride, though never
    /// before its last pixel.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The width and height, in pixels. Neither is negative.
    pub const fn size(&self) -> DeviceSize {
        self.size
    }

    /// The bytes from the start of one row to the start of the next.
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// The coverage at `at`, relative to the mask's top-left pixel, or
    /// `None` outside it.
    pub fn coverage(&self, at: DevicePoint) -> Option<u8> {
        let (x, y) = (usize::try_from(at.x).ok()?, usize::try_from(at.y).ok()?);
        self.row(y).get(x).copied()
    }

    fn row(&self, y: usize) -> &'a [u8] {
        row_bytes(self.bytes, self.size, self.stride, 1, y)
    }
}

impl fmt::Debug for Mask<'_> {
    /// The size and stride, not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mask")
            .field("size", &self.size)
            .field("stride", &self.stride)
            .finish_non_exhaustive()
    }
}

/// Draws `mask` with its top-left pixel at `at` in `color`: each pixel
/// inside `clip` and the bounds gets `scale(color, coverage)` composited
/// over it.
pub fn draw(
    fb: &mut Framebuffer,
    clip: DeviceRect,
    mask: &Mask<'_>,
    at: DevicePoint,
    color: PremultipliedColor,
) {
    let size = mask.size();
    let (x, y) = (i64::from(at.x), i64::from(at.y));
    let area = Edges {
        left: x,
        top: y,
        right: x + i64::from(size.width),
        bottom: y + i64::from(size.height),
    };
    let window = fb.window(clip, area);
    if window.is_empty() || color == PremultipliedColor::TRANSPARENT {
        return;
    }
    for row in window.top..window.bottom {
        let Some((first, target)) = fb.span_mut(clip, row, window.left, window.right) else {
            continue;
        };
        let (Ok(mask_row), Ok(column)) = (usize::try_from(row - y), usize::try_from(first - x))
        else {
            continue;
        };
        let count = target.len() / BYTES_PER_PIXEL;
        let Some(coverage) = mask.row(mask_row).get(column..column + count) else {
            continue;
        };
        for (pixel, &amount) in target
            .as_chunks_mut::<BYTES_PER_PIXEL>()
            .0
            .iter_mut()
            .zip(coverage)
        {
            match amount {
                0 => {}
                u8::MAX => blend_pixel(pixel, color),
                partial => blend_pixel(pixel, scale(color, partial)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::SPACE;

    fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColor {
        PremultipliedColor { r, g, b, a }
    }

    #[test]
    fn masks_check_their_shape() {
        let bytes = [0_u8; 12];
        let mask = Mask::new(&bytes, DeviceSize::new(3, 2), 6).expect("fits");
        assert_eq!(
            (mask.size(), mask.stride(), mask.bytes().len()),
            (DeviceSize::new(3, 2), 6, 12)
        );
        assert_eq!(
            Mask::new(&bytes, DeviceSize::new(3, 3), 6),
            Err(GfxError::TooFewBytes {
                len: 12,
                needed: 18
            })
        );
        assert_eq!(
            Mask::new(&bytes, DeviceSize::new(7, 1), 6),
            Err(GfxError::StrideTooSmall { stride: 6, min: 7 })
        );
        assert_eq!(
            Mask::new(&bytes, DeviceSize::new(-1, 1), 6),
            Err(GfxError::NegativeSize {
                size: DeviceSize::new(-1, 1)
            })
        );
        assert_eq!(
            format!("{mask:?}"),
            "Mask { size: DeviceSize { width: 3, height: 2 }, stride: 6, .. }"
        );
    }

    #[test]
    fn regions_of_an_atlas() {
        // A 4 by 3 atlas whose bytes are their own index.
        let bytes: Vec<u8> = (0..12).collect();
        let atlas = Mask::new(&bytes, DeviceSize::new(4, 3), 4).expect("fits");
        let glyph = atlas.region(DeviceRect::new(1, 1, 2, 2)).expect("inside");
        assert_eq!(glyph.coverage(DevicePoint::new(0, 0)), Some(5));
        assert_eq!(glyph.coverage(DevicePoint::new(1, 1)), Some(10));
        assert_eq!(glyph.coverage(DevicePoint::new(2, 0)), None);
        assert_eq!(glyph.coverage(DevicePoint::new(0, -1)), None);
        // From the region's first pixel to the end of the atlas, which
        // comes before the end of its second stride.
        assert_eq!(glyph.bytes(), &bytes[5..]);
        let rect = DeviceRect::new(3, 0, 2, 1);
        assert_eq!(
            atlas.region(rect),
            Err(GfxError::OutsideView {
                rect,
                size: DeviceSize::new(4, 3)
            })
        );
    }

    #[test]
    fn draw_blends_the_color_by_coverage() {
        // Coverage 0, 128 and 255 of opaque white over opaque black:
        // scale(white, 128) is (128, 128, 128, 128), over black it is
        // (128, 128, 128, 128 + 127 = 255).
        let coverage = [0, 128, 255];
        let mask = Mask::new(&coverage, DeviceSize::new(3, 1), 3).expect("fits");
        let mut fb = Framebuffer::new(DeviceSize::new(4, 1)).expect("small");
        let black = premultiplied(0, 0, 0, 255);
        fb.clear(black);
        draw(
            &mut fb,
            SPACE,
            &mask,
            DevicePoint::new(1, 0),
            premultiplied(255, 255, 255, 255),
        );
        assert_eq!(
            fb.bytes(),
            &[
                0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255
            ]
        );
        // Clipped, and partly outside on the left.
        fb.clear(black);
        draw(
            &mut fb,
            DeviceRect::new(0, 0, 1, 1),
            &mask,
            DevicePoint::new(-1, 0),
            premultiplied(255, 255, 255, 255),
        );
        assert_eq!(&fb.bytes()[..4], &[128, 128, 128, 255]);
        assert_eq!(
            &fb.bytes()[4..],
            &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
    }
}
