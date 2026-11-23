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

//! The error type for `tidepool-gfx`.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! Only making a framebuffer, a view, a mask or the backdrop can fail. A
//! draw call never fails and never panics: geometry it cannot draw draws
//! nothing, or only the part of it that is valid.

use std::fmt;

use tidepool_core::geom::{DeviceRect, DeviceSize};

use crate::clip::SPACE;
use crate::framebuffer::MAX_DIMENSION;

/// Errors produced by `tidepool-gfx`.
///
/// Non-exhaustive: later versions may add variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GfxError {
    /// A width or a height is negative.
    NegativeSize {
        /// The size given.
        size: DeviceSize,
    },
    /// A side of a framebuffer is over [`MAX_DIMENSION`].
    TooLarge {
        /// The size given.
        size: DeviceSize,
    },
    /// A framebuffer's bounds reach outside [`SPACE`], the device space
    /// `gfx` draws in.
    OutsideSpace {
        /// The bounds given.
        bounds: DeviceRect,
    },
    /// A row of pixels does not fit in the stride.
    StrideTooSmall {
        /// The stride given, in bytes.
        stride: usize,
        /// The bytes of a row: the width times the bytes of a pixel.
        min: usize,
    },
    /// The bytes do not cover every row.
    TooFewBytes {
        /// How many bytes there are.
        len: usize,
        /// How many the rows take: the stride times the height, or
        /// `usize::MAX` if that does not fit in a `usize`.
        needed: usize,
    },
    /// A region asked of a view or a mask does not lie inside it, or has a
    /// negative side.
    OutsideView {
        /// The region asked for, relative to the view.
        rect: DeviceRect,
        /// The size of the view.
        size: DeviceSize,
    },
    /// The memory for pixels could not be allocated.
    OutOfMemory {
        /// How many bytes were asked for.
        bytes: usize,
    },
    /// A value of a [`BackdropStyle`](crate::backdrop::BackdropStyle) is
    /// not finite, or is negative where it cannot be, or is an opacity
    /// above 1, or is over one of the drawings' limits
    /// ([`backdrop`](crate::backdrop#limits)).
    InvalidStyle {
        /// The field, as a path from the style: `grid.cell`, or
        /// `circuits.shapes.stroke.width`.
        field: &'static str,
        /// The index in the list, for a field of an element of one
        /// (`glows`, `stars`, or a drawing's `shapes`).
        index: Option<usize>,
    },
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NegativeSize { size } => write!(
                f,
                "a size of {} by {} pixels: a side is negative",
                size.width, size.height
            ),
            Self::TooLarge { size } => write!(
                f,
                "a size of {} by {} pixels: a side is over {MAX_DIMENSION}",
                size.width, size.height
            ),
            Self::OutsideSpace { bounds } => write!(
                f,
                "bounds of {} by {} pixels at ({}, {}) reach outside the device space, \
                 from {} to {} on each axis",
                bounds.width,
                bounds.height,
                bounds.x,
                bounds.y,
                SPACE.x,
                SPACE.right(),
            ),
            Self::StrideTooSmall { stride, min } => write!(
                f,
                "a stride of {stride} bytes is less than a row, {min} bytes"
            ),
            Self::TooFewBytes { len, needed } => {
                write!(f, "the rows take {needed} bytes, and there are {len}")
            }
            Self::OutsideView { rect, size } => write!(
                f,
                "a region of {} by {} pixels at ({}, {}) is not inside a view of {} by {}",
                rect.width, rect.height, rect.x, rect.y, size.width, size.height
            ),
            Self::OutOfMemory { bytes } => {
                write!(f, "could not allocate {bytes} bytes for pixels")
            }
            Self::InvalidStyle { field, index } => {
                f.write_str("the backdrop style's ")?;
                // The index is the element's in the list the field is in:
                // a drawing's `shapes`, or else the path's first part.
                let list_end = if field.ends_with(".shapes") {
                    Some(field.len())
                } else {
                    field
                        .find(".shapes.")
                        .map(|at| at + ".shapes".len())
                        .or_else(|| field.find('.'))
                };
                match (list_end, index) {
                    (Some(end), Some(index)) => {
                        let (list, rest) = (
                            field.get(..end).unwrap_or(field),
                            field.get(end..).unwrap_or_default(),
                        );
                        write!(f, "{list}[{index}]{rest}")?;
                    }
                    _ => f.write_str(field)?,
                }
                f.write_str(
                    " is not finite, is negative, is an opacity above 1, or is over its limit",
                )
            }
        }
    }
}

impl std::error::Error for GfxError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_the_numbers_and_the_field() {
        let cases = [
            (
                GfxError::NegativeSize {
                    size: DeviceSize::new(-1, 2),
                },
                "a size of -1 by 2 pixels: a side is negative",
            ),
            (
                GfxError::TooLarge {
                    size: DeviceSize::new(20_000, 2),
                },
                "a size of 20000 by 2 pixels: a side is over 16384",
            ),
            (
                GfxError::StrideTooSmall { stride: 7, min: 8 },
                "a stride of 7 bytes is less than a row, 8 bytes",
            ),
            (
                GfxError::TooFewBytes { len: 3, needed: 8 },
                "the rows take 8 bytes, and there are 3",
            ),
            (
                GfxError::OutsideView {
                    rect: DeviceRect::new(1, 2, 3, 4),
                    size: DeviceSize::new(2, 2),
                },
                "a region of 3 by 4 pixels at (1, 2) is not inside a view of 2 by 2",
            ),
            (
                GfxError::OutOfMemory { bytes: 64 },
                "could not allocate 64 bytes for pixels",
            ),
            (
                GfxError::InvalidStyle {
                    field: "glows.radius_x",
                    index: Some(2),
                },
                "the backdrop style's glows[2].radius_x is not finite, is negative, \
                 is an opacity above 1, or is over its limit",
            ),
            (
                GfxError::InvalidStyle {
                    field: "circuits.shapes.stroke.width",
                    index: Some(4),
                },
                "the backdrop style's circuits.shapes[4].stroke.width is not finite, is \
                 negative, is an opacity above 1, or is over its limit",
            ),
            (
                GfxError::InvalidStyle {
                    field: "synapses.shapes",
                    index: Some(1024),
                },
                "the backdrop style's synapses.shapes[1024] is not finite, is negative, \
                 is an opacity above 1, or is over its limit",
            ),
            (
                GfxError::InvalidStyle {
                    field: "grid.cell",
                    index: None,
                },
                "the backdrop style's grid.cell is not finite, is negative, is an \
                 opacity above 1, or is over its limit",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
        let outside = GfxError::OutsideSpace {
            bounds: DeviceRect::new(i32::MAX - 1, 0, 4, 4),
        };
        assert!(outside.to_string().contains("-1073741824"), "{outside}");
    }
}
