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

//! The [`Framebuffer`] `gfx` draws into, and [`View`], a borrowed view of
//! pixels to draw from.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! # Pixels
//!
//! Each pixel is four bytes in memory order blue, green, red, alpha
//! (BGRA8), premultiplied, as
//! [`PremultipliedColor`] describes. Rows are a stride apart. This is the
//! layout `platform::surface::Pixels` takes (ADR-0040), so `gui` passes
//! [`Framebuffer::bytes`], [`Framebuffer::size`] and
//! [`Framebuffer::stride`] straight to it.
//!
//! Pixel (x, y) covers the square [x, x + 1) × [y, y + 1) of device space,
//! and its centre is (x + 0.5, y + 0.5).
//!
//! # Bounds
//!
//! A framebuffer covers a rect of device space, its bounds, which need not
//! start at the origin: a layer can cover just part of a window. Draw
//! calls take device coordinates and draw only what falls inside both the
//! bounds and the clip. Every write in the crate goes through one helper
//! that hands out a row's bytes after clipping, so nothing can write
//! outside `clip ∩ bounds`.

use std::fmt;

use tidepool_core::color::PremultipliedColor;
use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize};

use crate::GfxError;
use crate::blend::{from_bgra, to_bgra};
use crate::clip::{Edges, SPACE};

/// The bytes of one pixel.
pub const BYTES_PER_PIXEL: usize = 4;

/// The largest width or height of a [`Framebuffer`], in device pixels: a
/// 4K display at scale 4, or two 8K displays side by side.
pub const MAX_DIMENSION: i32 = 16_384;

/// Premultiplied BGRA8 pixels covering a rect of device space, its bounds.
///
/// It owns its bytes, `stride` × height of them, with a stride of exactly
/// four times the width. A new framebuffer is fully transparent.
///
/// `Clone` allocates as `Vec` does, and so aborts if memory runs out;
/// [`Framebuffer::try_clone`] returns an error instead. Equality compares
/// the bounds and the bytes. `Debug` shows the bounds, not the bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Framebuffer {
    bytes: Vec<u8>,
    bounds: DeviceRect,
}

impl Framebuffer {
    /// A transparent framebuffer of `size` pixels with its top-left pixel
    /// at the origin.
    ///
    /// # Errors
    ///
    /// As for [`Framebuffer::new_at`].
    pub fn new(size: DeviceSize) -> Result<Self, GfxError> {
        Self::new_at(DeviceRect::new(0, 0, size.width, size.height))
    }

    /// A transparent framebuffer covering `bounds`. A side of zero is
    /// allowed: such a framebuffer has no pixels, and drawing into it does
    /// nothing.
    ///
    /// # Errors
    ///
    /// - [`GfxError::NegativeSize`] if a side is negative.
    /// - [`GfxError::TooLarge`] if a side is over [`MAX_DIMENSION`].
    /// - [`GfxError::OutsideSpace`] if the bounds reach outside
    ///   [`SPACE`].
    /// - [`GfxError::OutOfMemory`] if the bytes cannot be allocated. It
    ///   never aborts for lack of memory.
    pub fn new_at(bounds: DeviceRect) -> Result<Self, GfxError> {
        let size = DeviceSize::new(bounds.width, bounds.height);
        if bounds.width < 0 || bounds.height < 0 {
            return Err(GfxError::NegativeSize { size });
        }
        if bounds.width > MAX_DIMENSION || bounds.height > MAX_DIMENSION {
            return Err(GfxError::TooLarge { size });
        }
        let edges = Edges::of(bounds);
        let space = Edges::of(SPACE);
        if edges.left < space.left
            || edges.top < space.top
            || edges.right > space.right
            || edges.bottom > space.bottom
        {
            return Err(GfxError::OutsideSpace { bounds });
        }
        // Both sides are between 0 and 2¹⁴, so the product is at most 2³⁰.
        let len = to_usize(bounds.width)
            .saturating_mul(to_usize(bounds.height))
            .saturating_mul(BYTES_PER_PIXEL);
        Ok(Self {
            bytes: zeroed(len)?,
            bounds,
        })
    }

    /// A framebuffer with no pixels at the origin, which needs no memory.
    pub(crate) const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            bounds: DeviceRect::ZERO,
        }
    }

    /// A copy, or [`GfxError::OutOfMemory`] instead of aborting when the
    /// bytes cannot be allocated.
    ///
    /// # Errors
    ///
    /// [`GfxError::OutOfMemory`].
    pub fn try_clone(&self) -> Result<Self, GfxError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.bytes.len())
            .map_err(|_| GfxError::OutOfMemory {
                bytes: self.bytes.len(),
            })?;
        bytes.extend_from_slice(&self.bytes);
        Ok(Self {
            bytes,
            bounds: self.bounds,
        })
    }

    /// The rect of device space the framebuffer covers.
    pub const fn bounds(&self) -> DeviceRect {
        self.bounds
    }

    /// The width and height, in pixels.
    pub const fn size(&self) -> DeviceSize {
        DeviceSize::new(self.bounds.width, self.bounds.height)
    }

    /// The bytes from the start of one row to the start of the next:
    /// exactly four times the width.
    pub const fn stride(&self) -> usize {
        to_usize(self.bounds.width) * BYTES_PER_PIXEL
    }

    /// The pixels, row by row from the top: `stride` × height bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A view of every pixel, for drawing this framebuffer into another.
    pub fn view(&self) -> View<'_> {
        View {
            bytes: &self.bytes,
            size: self.size(),
            stride: self.stride(),
        }
    }

    /// The pixel at `at`, in device coordinates, or `None` if it is outside
    /// the bounds.
    pub fn pixel(&self, at: DevicePoint) -> Option<PremultipliedColor> {
        if !self.bounds.contains(at) {
            return None;
        }
        let row = usize::try_from(i64::from(at.y) - i64::from(self.bounds.y)).ok()?;
        let column = usize::try_from(i64::from(at.x) - i64::from(self.bounds.x)).ok()?;
        let start = row * self.stride() + column * BYTES_PER_PIXEL;
        let bytes = self.bytes.get(start..start + BYTES_PER_PIXEL)?;
        let pixel: [u8; BYTES_PER_PIXEL] = bytes.try_into().ok()?;
        Some(from_bgra(pixel))
    }

    /// Sets every pixel to `color`, ignoring any clip.
    pub fn clear(&mut self, color: PremultipliedColor) {
        let bgra = to_bgra(color);
        let edges = Edges::of(self.bounds);
        for y in edges.top..edges.bottom {
            if let Some((_, span)) = self.span_mut(self.bounds, y, edges.left, edges.right) {
                for pixel in span.as_chunks_mut::<BYTES_PER_PIXEL>().0.iter_mut() {
                    pixel.copy_from_slice(&bgra);
                }
            }
        }
    }

    /// `area` clipped to `clip` and the bounds: the pixels a draw call may
    /// touch.
    pub(crate) fn window(&self, clip: DeviceRect, area: Edges) -> Edges {
        area.intersect(Edges::of(clip))
            .intersect(Edges::of(self.bounds))
    }

    /// The bytes of row `y` from column `left` to just before `right`, in
    /// device coordinates, after clipping to `clip` and the bounds, with
    /// the column of the first pixel; or `None` if nothing is left.
    ///
    /// Every write in the crate goes through here, so that nothing can
    /// write outside `clip ∩ bounds` whatever its caller computed.
    pub(crate) fn span_mut(
        &mut self,
        clip: DeviceRect,
        y: i64,
        left: i64,
        right: i64,
    ) -> Option<(i64, &mut [u8])> {
        let area = Edges::of(clip).intersect(Edges::of(self.bounds));
        if y < area.top || y >= area.bottom {
            return None;
        }
        let left = left.max(area.left);
        let right = right.min(area.right);
        if right <= left {
            return None;
        }
        let row = usize::try_from(y - i64::from(self.bounds.y)).ok()?;
        let column = usize::try_from(left - i64::from(self.bounds.x)).ok()?;
        let count = usize::try_from(right - left).ok()?;
        let start = row * self.stride() + column * BYTES_PER_PIXEL;
        let span = self.bytes.get_mut(start..start + count * BYTES_PER_PIXEL)?;
        Some((left, span))
    }

    /// The bytes of row `y` (device coordinates) from column `left` to just
    /// before `right`, clipped to the bounds, with the first column.
    pub(crate) fn span(&self, y: i64, left: i64, right: i64) -> Option<(i64, &[u8])> {
        let area = Edges::of(self.bounds);
        if y < area.top || y >= area.bottom {
            return None;
        }
        let left = left.max(area.left);
        let right = right.min(area.right);
        if right <= left {
            return None;
        }
        let row = usize::try_from(y - i64::from(self.bounds.y)).ok()?;
        let column = usize::try_from(left - i64::from(self.bounds.x)).ok()?;
        let count = usize::try_from(right - left).ok()?;
        let start = row * self.stride() + column * BYTES_PER_PIXEL;
        let span = self.bytes.get(start..start + count * BYTES_PER_PIXEL)?;
        Some((left, span))
    }
}

impl fmt::Debug for Framebuffer {
    /// The bounds, not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Framebuffer")
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

/// A borrowed, read-only view of premultiplied BGRA8 pixels in rows: an
/// image to copy or blit, or a part of a framebuffer.
///
/// [`View::new`] checks that the stride covers a row and the bytes cover
/// every row, as `platform::surface::Pixels::new` does, so nothing reads
/// outside the bytes whatever the caller passes. Equality compares the
/// bytes, size and stride. `Debug` shows the size and stride, not the
/// bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct View<'a> {
    /// From the first pixel; covers every row's pixels (see
    /// [`View::bytes`]).
    bytes: &'a [u8],
    size: DeviceSize,
    stride: usize,
}

impl<'a> View<'a> {
    /// A view of `bytes` as `size` pixels in rows `stride` bytes apart.
    /// Bytes past the last row are left out.
    ///
    /// # Errors
    ///
    /// - [`GfxError::NegativeSize`] if a side is negative. Zero is
    ///   allowed: such a view has no pixels.
    /// - [`GfxError::StrideTooSmall`] if a row of `size.width` pixels does
    ///   not fit in `stride` bytes.
    /// - [`GfxError::TooFewBytes`] if `bytes` is shorter than `stride`
    ///   times the height.
    pub fn new(bytes: &'a [u8], size: DeviceSize, stride: usize) -> Result<Self, GfxError> {
        let bytes = check_shape(bytes, size, stride, BYTES_PER_PIXEL)?;
        Ok(Self {
            bytes,
            size,
            stride,
        })
    }

    /// The part of the view inside `rect`, which is relative to the view's
    /// top-left pixel. An empty `rect` inside the view gives a view with
    /// no pixels.
    ///
    /// # Errors
    ///
    /// [`GfxError::OutsideView`] if `rect` has a negative side or does not
    /// lie inside the view.
    pub fn region(&self, rect: DeviceRect) -> Result<View<'a>, GfxError> {
        let bytes = region_bytes(self.bytes, self.size, self.stride, BYTES_PER_PIXEL, rect)?;
        Ok(View {
            bytes,
            size: DeviceSize::new(rect.width, rect.height),
            stride: self.stride,
        })
    }

    /// The bytes, from the first pixel: `stride` × height of them for a
    /// view from [`View::new`]. A region's bytes end there too, or at the
    /// end of the bytes it was made from if that comes first, so its last
    /// row may end before a whole stride, though never before its last
    /// pixel.
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

    /// The pixel at `at`, relative to the view's top-left pixel, or `None`
    /// if it is outside the view.
    pub fn pixel(&self, at: DevicePoint) -> Option<PremultipliedColor> {
        let (x, y) = (usize::try_from(at.x).ok()?, usize::try_from(at.y).ok()?);
        let row = self.row(y);
        let bytes = row.get(x * BYTES_PER_PIXEL..(x + 1) * BYTES_PER_PIXEL)?;
        let pixel: [u8; BYTES_PER_PIXEL] = bytes.try_into().ok()?;
        Some(from_bgra(pixel))
    }

    /// The pixels of row `y`, four bytes each; empty past the last row.
    pub(crate) fn row(&self, y: usize) -> &'a [u8] {
        row_bytes(self.bytes, self.size, self.stride, BYTES_PER_PIXEL, y)
    }
}

impl fmt::Debug for View<'_> {
    /// The size and stride, not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("View")
            .field("size", &self.size)
            .field("stride", &self.stride)
            .finish_non_exhaustive()
    }
}

/// A side that is known not to be negative, as `usize`; a negative one is
/// 0. `usize` is at least 32 bits, so every `i32` at or above zero fits.
pub(crate) const fn to_usize(side: i32) -> usize {
    if side < 0 { 0 } else { side as usize }
}

/// `len` zero bytes, or [`GfxError::OutOfMemory`].
pub(crate) fn zeroed<T: Clone + Default>(len: usize) -> Result<Vec<T>, GfxError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| GfxError::OutOfMemory {
            bytes: len.saturating_mul(size_of::<T>()),
        })?;
    values.resize(len, T::default());
    Ok(values)
}

/// Checks the shape of rows of `bytes_per_pixel`-byte pixels, as
/// [`View::new`] documents, and returns the bytes the rows take.
pub(crate) fn check_shape(
    bytes: &[u8],
    size: DeviceSize,
    stride: usize,
    bytes_per_pixel: usize,
) -> Result<&[u8], GfxError> {
    let (Ok(width), Ok(height)) = (usize::try_from(size.width), usize::try_from(size.height))
    else {
        return Err(GfxError::NegativeSize { size });
    };
    let row = width.saturating_mul(bytes_per_pixel);
    if stride < row {
        return Err(GfxError::StrideTooSmall { stride, min: row });
    }
    let needed = stride.checked_mul(height);
    match needed.and_then(|needed| bytes.get(..needed)) {
        Some(bytes) => Ok(bytes),
        None => Err(GfxError::TooFewBytes {
            len: bytes.len(),
            needed: needed.unwrap_or(usize::MAX),
        }),
    }
}

/// The bytes of the region `rect` of rows of `bytes_per_pixel`-byte
/// pixels, from its first pixel; empty for an empty region.
///
/// The rows' invariant, which [`check_shape`] establishes and this keeps:
/// unless the size is empty, `bytes` holds at least
/// `(height − 1) × stride + width × bytes_per_pixel` bytes, so every row's
/// pixels are inside it.
pub(crate) fn region_bytes(
    bytes: &[u8],
    size: DeviceSize,
    stride: usize,
    bytes_per_pixel: usize,
    rect: DeviceRect,
) -> Result<&[u8], GfxError> {
    let outside = GfxError::OutsideView { rect, size };
    let edges = Edges::of(rect);
    if rect.width < 0
        || rect.height < 0
        || edges.left < 0
        || edges.top < 0
        || edges.right > i64::from(size.width)
        || edges.bottom > i64::from(size.height)
    {
        return Err(outside);
    }
    if rect.width == 0 || rect.height == 0 {
        return Ok(&[]);
    }
    let start = to_usize(rect.y) * stride + to_usize(rect.x) * bytes_per_pixel;
    let end = bytes
        .len()
        .min(start.saturating_add(stride.saturating_mul(to_usize(rect.height))));
    bytes.get(start..end).ok_or(outside)
}

/// The pixels of row `y` of rows of `bytes_per_pixel`-byte pixels; empty
/// past the last row.
pub(crate) fn row_bytes(
    bytes: &[u8],
    size: DeviceSize,
    stride: usize,
    bytes_per_pixel: usize,
    y: usize,
) -> &[u8] {
    if y >= to_usize(size.height) {
        return &[];
    }
    let start = y * stride;
    let len = to_usize(size.width) * bytes_per_pixel;
    bytes.get(start..start + len).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColor {
        PremultipliedColor { r, g, b, a }
    }

    #[test]
    fn a_new_framebuffer_is_transparent_and_shaped_for_present() {
        let fb = Framebuffer::new(DeviceSize::new(3, 2)).expect("small");
        assert_eq!(fb.bounds(), DeviceRect::new(0, 0, 3, 2));
        assert_eq!(fb.size(), DeviceSize::new(3, 2));
        assert_eq!(fb.stride(), 12);
        assert_eq!(fb.bytes().len(), 24);
        assert!(fb.bytes().iter().all(|&byte| byte == 0));
        assert_eq!(
            fb.pixel(DevicePoint::new(2, 1)),
            Some(PremultipliedColor::TRANSPARENT)
        );
        assert_eq!(fb.pixel(DevicePoint::new(3, 1)), None);
        assert_eq!(fb.pixel(DevicePoint::new(-1, 0)), None);
        assert_eq!(
            format!("{fb:?}"),
            "Framebuffer { bounds: DeviceRect { x: 0, y: 0, width: 3, height: 2 }, .. }"
        );
    }

    #[test]
    fn bounds_may_start_anywhere_in_device_space() {
        let mut fb = Framebuffer::new_at(DeviceRect::new(-5, 10, 2, 2)).expect("small");
        fb.clear(premultiplied(1, 2, 3, 4));
        // Memory order is blue, green, red, alpha.
        assert_eq!(&fb.bytes()[..4], &[3, 2, 1, 4]);
        assert_eq!(
            fb.pixel(DevicePoint::new(-4, 11)),
            Some(premultiplied(1, 2, 3, 4))
        );
        assert_eq!(fb.pixel(DevicePoint::new(0, 0)), None);
        assert_eq!(fb.pixel(DevicePoint::new(-3, 11)), None);
    }

    #[test]
    fn creation_errors() {
        assert_eq!(
            Framebuffer::new(DeviceSize::new(-1, 4)),
            Err(GfxError::NegativeSize {
                size: DeviceSize::new(-1, 4)
            })
        );
        assert_eq!(
            Framebuffer::new(DeviceSize::new(4, MAX_DIMENSION + 1)),
            Err(GfxError::TooLarge {
                size: DeviceSize::new(4, MAX_DIMENSION + 1)
            })
        );
        let far = DeviceRect::new(i32::MAX - 2, 0, 4, 4);
        assert_eq!(
            Framebuffer::new_at(far),
            Err(GfxError::OutsideSpace { bounds: far })
        );
        let edge = DeviceRect::new(SPACE.right() - 4, SPACE.y, 4, 4);
        assert!(Framebuffer::new_at(edge).is_ok());
        let past = DeviceRect::new(SPACE.right() - 3, SPACE.y, 4, 4);
        assert!(Framebuffer::new_at(past).is_err());
        // Zero-sized framebuffers exist and hold no bytes.
        let empty = Framebuffer::new(DeviceSize::new(0, 7)).expect("empty");
        assert!(empty.bytes().is_empty());
        assert_eq!(empty.stride(), 0);
        assert_eq!(Framebuffer::empty().bounds(), DeviceRect::ZERO);
    }

    #[test]
    fn try_clone_copies() {
        let mut fb = Framebuffer::new(DeviceSize::new(2, 2)).expect("small");
        fb.clear(premultiplied(9, 9, 9, 9));
        assert_eq!(fb.try_clone(), Ok(fb.clone()));
    }

    #[test]
    fn the_write_helper_never_leaves_clip_and_bounds() {
        let mut fb = Framebuffer::new_at(DeviceRect::new(10, 10, 4, 4)).expect("small");
        let clip = DeviceRect::new(11, 0, 100, 12);
        assert!(fb.span_mut(clip, 9, 0, 100).is_none());
        assert!(fb.span_mut(clip, 12, 0, 100).is_none(), "below the clip");
        assert!(fb.span_mut(clip, 10, 0, 11).is_none(), "left of the clip");
        let (x, span) = fb.span_mut(clip, 11, i64::MIN, i64::MAX).expect("inside");
        assert_eq!((x, span.len()), (11, 12));
        span.fill(0xff);
        let changed: Vec<usize> = fb
            .bytes()
            .iter()
            .enumerate()
            .filter(|&(_, &byte)| byte != 0)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(changed, (20..32).collect::<Vec<_>>());
    }

    #[test]
    fn views_check_their_shape_like_present_does() {
        let bytes = [0_u8; 24];
        let view = View::new(&bytes, DeviceSize::new(2, 2), 12).expect("fits");
        assert_eq!((view.size(), view.stride()), (DeviceSize::new(2, 2), 12));
        assert_eq!(view.bytes().len(), 24);
        assert_eq!(
            View::new(&bytes[..20], DeviceSize::new(2, 2), 12),
            Err(GfxError::TooFewBytes {
                len: 20,
                needed: 24
            })
        );
        assert_eq!(
            View::new(&bytes, DeviceSize::new(4, 1), 12),
            Err(GfxError::StrideTooSmall {
                stride: 12,
                min: 16
            })
        );
        assert_eq!(
            View::new(&bytes, DeviceSize::new(1, -1), 4),
            Err(GfxError::NegativeSize {
                size: DeviceSize::new(1, -1)
            })
        );
        assert_eq!(
            View::new(&bytes, DeviceSize::new(1, i32::MAX), usize::MAX),
            Err(GfxError::TooFewBytes {
                len: 24,
                needed: usize::MAX
            })
        );
        // Bytes past the last row are left out.
        let long = [0_u8; 40];
        let view = View::new(&long, DeviceSize::new(2, 2), 8).expect("fits");
        assert_eq!(view.bytes().len(), 16);
        assert_eq!(
            format!("{view:?}"),
            "View { size: DeviceSize { width: 2, height: 2 }, stride: 8, .. }"
        );
    }

    #[test]
    fn regions_are_views_of_part_of_a_view() {
        // A 3 by 3 view with a padded stride, each pixel's blue byte its
        // index.
        let mut bytes = [0_u8; 3 * 16];
        for y in 0..3 {
            for x in 0..3 {
                bytes[y * 16 + x * 4] = (y * 3 + x) as u8;
            }
        }
        let view = View::new(&bytes, DeviceSize::new(3, 3), 16).expect("fits");
        let region = view.region(DeviceRect::new(1, 1, 2, 2)).expect("inside");
        assert_eq!(region.size(), DeviceSize::new(2, 2));
        let blue = |view: &View<'_>, x, y| view.pixel(DevicePoint::new(x, y)).map(|c| c.b);
        assert_eq!(blue(&region, 0, 0), Some(4));
        assert_eq!(blue(&region, 1, 1), Some(8));
        assert_eq!(blue(&region, 2, 0), None);
        // The bytes end with the view's: the region starts 20 bytes in,
        // and its two strides would end 4 bytes past the view's 48.
        assert_eq!(region.bytes().len(), 48 - 20);
        let top = view.region(DeviceRect::new(0, 0, 3, 1)).expect("inside");
        assert_eq!(top.bytes().len(), 16);
        // A region of a region.
        let inner = region.region(DeviceRect::new(1, 1, 1, 1)).expect("inside");
        assert_eq!(blue(&inner, 0, 0), Some(8));
        // Empty regions inside are allowed; anything reaching out is not.
        let empty = view.region(DeviceRect::new(3, 3, 0, 0)).expect("empty");
        assert_eq!((empty.size(), empty.bytes().len()), (DeviceSize::ZERO, 0));
        for rect in [
            DeviceRect::new(2, 2, 2, 1),
            DeviceRect::new(-1, 0, 1, 1),
            DeviceRect::new(0, 0, -1, 1),
            DeviceRect::new(i32::MAX, 0, i32::MAX, 1),
        ] {
            assert_eq!(
                view.region(rect),
                Err(GfxError::OutsideView {
                    rect,
                    size: DeviceSize::new(3, 3)
                })
            );
        }
    }

    #[test]
    fn a_framebuffer_view_is_its_pixels() {
        let mut fb = Framebuffer::new_at(DeviceRect::new(4, 4, 2, 1)).expect("small");
        fb.clear(premultiplied(5, 6, 7, 8));
        let view = fb.view();
        assert_eq!((view.size(), view.stride()), (DeviceSize::new(2, 1), 8));
        assert_eq!(
            view.pixel(DevicePoint::new(1, 0)),
            Some(premultiplied(5, 6, 7, 8))
        );
        assert_eq!(view.bytes(), fb.bytes());
    }
}
