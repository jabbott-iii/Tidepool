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

//! Showing pixels in a window: [`Window::present`], and [`Pixels`], the view
//! of a frame it takes (ADR-0040).
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! A frame is BGRA8 rows: each pixel is four bytes, blue, green, red and
//! alpha, premultiplied, as `gfx`'s framebuffer holds them (M2.3), and each
//! row starts a stride after the one before. The window is opaque, so the
//! alpha byte is not shown. [`Pixels::new`] checks the shape of the view,
//! so that [`Window::present`] can never read outside it, whatever the
//! caller passes.

use core::fmt;

use tidepool_core::geom::{DeviceRect, DeviceSize};

use crate::Error;
use crate::window::Window;

/// The bytes of one pixel.
pub const BYTES_PER_PIXEL: usize = 4;

/// The largest stride [`Pixels`] takes, in bytes: every system's call that
/// shows pixels takes the stride as a 32-bit signed integer.
pub const MAX_STRIDE: usize = i32::MAX as usize;

/// A borrowed view of a frame: BGRA8 premultiplied pixels in rows, its size
/// in device pixels, and the stride, the bytes from the start of one row
/// to the start of the next.
///
/// [`Pixels::new`] checks that the stride covers a row and the bytes cover
/// every row, the last included.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pixels<'a> {
    bytes: &'a [u8],
    size: DeviceSize,
    stride: usize,
}

impl<'a> Pixels<'a> {
    /// A view of `bytes` as a frame of `size` device pixels with rows
    /// `stride` bytes apart. Bytes past the last row are left out.
    ///
    /// ```
    /// use tidepool_core::geom::DeviceSize;
    /// use tidepool_platform::surface::{Pixels, PixelsError};
    ///
    /// // Two by two pixels, with four bytes of padding after each row.
    /// let bytes = [0_u8; 24];
    /// let pixels = Pixels::new(&bytes, DeviceSize::new(2, 2), 12)?;
    /// assert_eq!(pixels.stride(), 12);
    ///
    /// let short = Pixels::new(&bytes[..20], DeviceSize::new(2, 2), 12);
    /// assert_eq!(short, Err(PixelsError::TooFewBytes { len: 20, needed: 24 }));
    /// # Ok::<(), PixelsError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// - [`PixelsError::NegativeSize`] if the width or the height is
    ///   negative. Zero is allowed: such a view shows nothing.
    /// - [`PixelsError::StrideTooSmall`] if a row of `size.width` pixels
    ///   does not fit in `stride` bytes.
    /// - [`PixelsError::StrideTooLarge`] if `stride` is over
    ///   [`MAX_STRIDE`].
    /// - [`PixelsError::TooFewBytes`] if `bytes` is shorter than `stride`
    ///   times the height.
    pub fn new(bytes: &'a [u8], size: DeviceSize, stride: usize) -> Result<Self, PixelsError> {
        let (Ok(width), Ok(height)) = (usize::try_from(size.width), usize::try_from(size.height))
        else {
            return Err(PixelsError::NegativeSize { size });
        };
        // A width that fits in `i32` times 4 fits in `usize`, which is at
        // least 32 bits.
        let row = width.saturating_mul(BYTES_PER_PIXEL);
        if stride < row {
            return Err(PixelsError::StrideTooSmall { stride, min: row });
        }
        if stride > MAX_STRIDE {
            return Err(PixelsError::StrideTooLarge { stride });
        }
        let needed = stride.checked_mul(height);
        match needed.and_then(|needed| bytes.get(..needed)) {
            Some(bytes) => Ok(Self {
                bytes,
                size,
                stride,
            }),
            None => Err(PixelsError::TooFewBytes {
                len: bytes.len(),
                needed: needed.unwrap_or(usize::MAX),
            }),
        }
    }

    /// The bytes of the frame, `stride` times the height of them.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The size of the frame, in device pixels. Neither side is negative.
    pub const fn size(&self) -> DeviceSize {
        self.size
    }

    /// The bytes from the start of one row to the start of the next: at
    /// least four times the width, and at most [`MAX_STRIDE`].
    pub const fn stride(&self) -> usize {
        self.stride
    }
}

impl fmt::Debug for Pixels<'_> {
    /// The size and stride, not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pixels")
            .field("size", &self.size)
            .field("stride", &self.stride)
            .finish_non_exhaustive()
    }
}

/// Why [`Pixels::new`] refused a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelsError {
    /// The width or the height is negative.
    NegativeSize {
        /// The size given.
        size: DeviceSize,
    },
    /// A row does not fit in the stride.
    StrideTooSmall {
        /// The stride given, in bytes.
        stride: usize,
        /// The bytes of a row: four times the width.
        min: usize,
    },
    /// The stride is over [`MAX_STRIDE`].
    StrideTooLarge {
        /// The stride given, in bytes.
        stride: usize,
    },
    /// The bytes do not cover every row.
    TooFewBytes {
        /// How many bytes there are.
        len: usize,
        /// How many the rows take: the stride times the height, or
        /// `usize::MAX` if that does not fit in a `usize`.
        needed: usize,
    },
}

impl fmt::Display for PixelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NegativeSize { size } => write!(
                f,
                "a frame of {} by {} pixels: a side is negative",
                size.width, size.height
            ),
            Self::StrideTooSmall { stride, min } => write!(
                f,
                "a stride of {stride} bytes is less than a row, {min} bytes"
            ),
            Self::StrideTooLarge { stride } => {
                write!(f, "a stride of {stride} bytes is over {MAX_STRIDE}")
            }
            Self::TooFewBytes { len, needed } => {
                write!(
                    f,
                    "the frame's rows take {needed} bytes, and there are {len}"
                )
            }
        }
    }
}

impl std::error::Error for PixelsError {}

impl Window {
    /// Shows the parts of `pixels` that `damage` names, each where it is in
    /// the frame, which starts at the window's top-left corner.
    ///
    /// Each rectangle is in device pixels, and is clipped to the frame and
    /// to the window; one that is empty after that is skipped. The rest of
    /// the window keeps what it showed. The frame need not be the window's
    /// size.
    ///
    /// On Linux it queues an `XPutImage` for each rectangle and sends them
    /// to the server, without waiting for it to draw them. The bytes may
    /// change once it returns. A request the server refuses is logged by
    /// the next [`wait_events`](Window::wait_events).
    ///
    /// # Errors
    ///
    /// [`Error::CallFailed`] if Xlib cannot describe the frame, which a
    /// [`Pixels`] should never allow.
    pub fn present(&mut self, pixels: &Pixels<'_>, damage: &[DeviceRect]) -> Result<(), Error> {
        self.backend.present(pixels, damage)
    }
}

/// `rect` clipped to the frame `view` and the window `window`, both from
/// the origin, or `None` if nothing of it is left.
// Each backend's present calls it; the one for systems without a window
// has nothing to present.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn clip(rect: DeviceRect, view: DeviceSize, window: DeviceSize) -> Option<DeviceRect> {
    // In `i64`, where no edge overflows.
    let right_limit = i64::from(view.width.min(window.width));
    let bottom_limit = i64::from(view.height.min(window.height));
    let left = i64::from(rect.x).max(0);
    let top = i64::from(rect.y).max(0);
    let right = (i64::from(rect.x) + i64::from(rect.width)).min(right_limit);
    let bottom = (i64::from(rect.y) + i64::from(rect.height)).min(bottom_limit);
    if right <= left || bottom <= top {
        return None;
    }
    // Every value is between 0 and a limit that came from an `i32`.
    let to_i32 = |value: i64| i32::try_from(value).ok();
    Some(DeviceRect::new(
        to_i32(left)?,
        to_i32(top)?,
        to_i32(right - left)?,
        to_i32(bottom - top)?,
    ))
}

#[cfg(test)]
mod tests {
    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check;

    use super::*;

    fn size(width: i32, height: i32) -> DeviceSize {
        DeviceSize::new(width, height)
    }

    #[test]
    fn a_view_takes_a_tight_or_a_padded_stride() {
        let bytes = [7_u8; 64];
        let tight = Pixels::new(&bytes, size(4, 4), 16).expect("a tight view");
        assert_eq!(tight.bytes().len(), 64);
        assert_eq!((tight.size(), tight.stride()), (size(4, 4), 16));

        let padded = Pixels::new(&bytes, size(3, 3), 20).expect("a padded view");
        // Bytes past the last row are left out.
        assert_eq!(padded.bytes().len(), 60);
        assert_eq!(
            format!("{padded:?}"),
            "Pixels { size: DeviceSize { width: 3, height: 3 }, stride: 20, .. }"
        );
    }

    #[test]
    fn an_empty_view_is_allowed() {
        assert!(Pixels::new(&[], size(0, 0), 0).is_ok());
        assert!(Pixels::new(&[], size(0, 9), 0).is_ok());
        assert!(Pixels::new(&[], size(9, 0), 36).is_ok());
    }

    #[test]
    fn a_view_with_a_negative_side_is_refused() {
        for bad in [size(-1, 4), size(4, -1), size(i32::MIN, i32::MIN)] {
            assert_eq!(
                Pixels::new(&[0; 64], bad, 16),
                Err(PixelsError::NegativeSize { size: bad })
            );
        }
    }

    #[test]
    fn a_stride_shorter_than_a_row_is_refused() {
        assert_eq!(
            Pixels::new(&[0; 64], size(4, 4), 15),
            Err(PixelsError::StrideTooSmall {
                stride: 15,
                min: 16
            })
        );
        assert_eq!(
            Pixels::new(&[0; 64], size(1, 1), 0),
            Err(PixelsError::StrideTooSmall { stride: 0, min: 4 })
        );
    }

    #[test]
    fn a_stride_over_the_limit_is_refused() {
        assert_eq!(
            Pixels::new(&[], size(0, 0), MAX_STRIDE + 1),
            Err(PixelsError::StrideTooLarge {
                stride: MAX_STRIDE + 1
            })
        );
        assert!(Pixels::new(&[], size(0, 0), MAX_STRIDE).is_ok());
    }

    #[test]
    fn bytes_that_miss_the_last_row_are_refused() {
        // The last row needs all of its stride, padding included.
        assert_eq!(
            Pixels::new(&[0; 59], size(3, 3), 20),
            Err(PixelsError::TooFewBytes {
                len: 59,
                needed: 60
            })
        );
        // Rows that would not fit in memory at all.
        assert_eq!(
            Pixels::new(&[0; 4], size(1, i32::MAX), MAX_STRIDE),
            Err(PixelsError::TooFewBytes {
                len: 4,
                needed: MAX_STRIDE * (i32::MAX as usize)
            })
        );
    }

    #[test]
    fn messages_say_what_is_wrong() {
        let cases = [
            (
                PixelsError::NegativeSize { size: size(-1, 2) },
                "a frame of -1 by 2 pixels: a side is negative",
            ),
            (
                PixelsError::StrideTooSmall { stride: 3, min: 4 },
                "a stride of 3 bytes is less than a row, 4 bytes",
            ),
            (
                PixelsError::TooFewBytes { len: 1, needed: 4 },
                "the frame's rows take 4 bytes, and there are 1",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn damage_is_clipped_to_the_view_and_the_window() {
        let view = size(100, 80);
        let window = size(90, 100);
        let cases = [
            // Inside both.
            (
                DeviceRect::new(10, 10, 20, 20),
                Some(DeviceRect::new(10, 10, 20, 20)),
            ),
            // Over the window's right edge and the view's bottom edge.
            (
                DeviceRect::new(80, 70, 50, 50),
                Some(DeviceRect::new(80, 70, 10, 10)),
            ),
            // From above and left of the origin.
            (
                DeviceRect::new(-5, -7, 10, 10),
                Some(DeviceRect::new(0, 0, 5, 3)),
            ),
            // Covering everything.
            (
                DeviceRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
                None,
            ),
            (
                DeviceRect::new(-1000, -1000, 5000, 5000),
                Some(DeviceRect::new(0, 0, 90, 80)),
            ),
            // Empty, negative, or outside.
            (DeviceRect::new(10, 10, 0, 5), None),
            (DeviceRect::new(10, 10, -5, 5), None),
            (DeviceRect::new(90, 0, 10, 10), None),
            (DeviceRect::new(0, 80, 10, 10), None),
            (DeviceRect::new(-10, 0, 10, 10), None),
            (
                DeviceRect::new(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
                None,
            ),
        ];
        for (rect, expected) in cases {
            assert_eq!(clip(rect, view, window), expected, "{rect:?}");
        }
        // A view or a window with no pixels clips everything away.
        let all = DeviceRect::new(0, 0, 10, 10);
        assert_eq!(clip(all, size(0, 10), window), None);
        assert_eq!(clip(all, view, size(10, 0)), None);
    }

    #[test]
    fn clipped_damage_is_inside_the_rect_the_view_and_the_window() {
        let generate = |rng: &mut Pcg32| {
            let side = |rng: &mut Pcg32| rng.range(-200..200);
            (
                DeviceRect::new(side(rng), side(rng), side(rng), side(rng)),
                size(rng.range(0..150), rng.range(0..150)),
                size(rng.range(0..150), rng.range(0..150)),
            )
        };
        check(
            "clipped damage stays inside",
            2000,
            generate,
            |&(rect, view, window)| {
                let Some(clipped) = clip(rect, view, window) else {
                    // Nothing left: some pixel of the rect in both would
                    // have been kept.
                    let overlaps = (0..view.width.min(window.width)).any(|x| {
                        (0..view.height.min(window.height))
                            .any(|y| rect.contains(tidepool_core::geom::DevicePoint::new(x, y)))
                    });
                    return if overlaps {
                        Err("dropped a rect that overlaps".to_string())
                    } else {
                        Ok(())
                    };
                };
                let inside = !clipped.is_empty()
                    && clipped.x >= rect.x.max(0)
                    && clipped.y >= rect.y.max(0)
                    && clipped.right() <= rect.right()
                    && clipped.bottom() <= rect.bottom()
                    && clipped.right() <= view.width.min(window.width)
                    && clipped.bottom() <= view.height.min(window.height);
                if inside {
                    Ok(())
                } else {
                    Err(format!("{clipped:?}"))
                }
            },
        );
    }
}
