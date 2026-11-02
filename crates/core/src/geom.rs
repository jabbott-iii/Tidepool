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

//! Geometry in logical units ([`Point`], [`Size`], [`Rect`], `f32`) and in
//! device pixels ([`DevicePoint`], [`DeviceSize`], [`DeviceRect`], `i32`),
//! with [`Scale`] converting between them.
//!
//! New in Tidepool; not ported.
//!
//! `ui` lays out and draws in logical units; `gui` rasterizes in device
//! pixels; the scale is the window's device pixels per logical unit (1.0 on
//! a standard display, 2.0 on a Retina display, 1.25 or 1.5 on many Windows
//! laptops).
//!
//! # Rounding
//!
//! - Points round to the nearest pixel.
//! - Sizes round up, so content always fits.
//! - Rects round **outward**: the left and top edges down, the right and
//!   bottom edges up, so the device rect covers every pixel the logical rect
//!   touches. A logical edge within 1/64 px of a pixel boundary counts as on
//!   it, so float error alone never grows a rect by a whole pixel.
//!
//! Converting device → logical → device gives back the same pixels for
//! coordinates within ±65,536 px. Logical → device → logical is not exact
//! (that is the point of rounding).

/// A point in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Distance to the right of the origin.
    pub x: f32,
    /// Distance below the origin.
    pub y: f32,
}

impl Point {
    /// The origin.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Creates a point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A size in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    /// The horizontal extent.
    pub width: f32,
    /// The vertical extent.
    pub height: f32,
}

impl Size {
    /// No extent.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Creates a size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in logical units: its top-left corner and size.
///
/// A negative or NaN width or height makes the rect empty.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
}

impl Rect {
    /// The empty rect at the origin.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// Creates a rect from its top-left corner and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rect from its top-left corner and size.
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    /// The top-left corner.
    pub const fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The size.
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    /// The right edge.
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// The bottom edge.
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns `true` if the rect covers no area.
    pub fn is_empty(self) -> bool {
        // Written so that NaN counts as empty.
        !(self.width > 0.0 && self.height > 0.0)
    }

    /// Returns `true` if `point` is inside the rect. The left and top edges
    /// are inside; the right and bottom edges are not.
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }
}

/// A point in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DevicePoint {
    /// Pixels to the right of the origin.
    pub x: i32,
    /// Pixels below the origin.
    pub y: i32,
}

impl DevicePoint {
    /// The origin.
    pub const ZERO: Self = Self::new(0, 0);

    /// Creates a point.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A size in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeviceSize {
    /// The width in pixels.
    pub width: i32,
    /// The height in pixels.
    pub height: i32,
}

impl DeviceSize {
    /// No extent.
    pub const ZERO: Self = Self::new(0, 0);

    /// Creates a size.
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in device pixels: its top-left pixel and size.
///
/// A width or height of zero or less makes the rect empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeviceRect {
    /// The leftmost column.
    pub x: i32,
    /// The top row.
    pub y: i32,
    /// The width in pixels.
    pub width: i32,
    /// The height in pixels.
    pub height: i32,
}

impl DeviceRect {
    /// The empty rect at the origin.
    pub const ZERO: Self = Self::new(0, 0, 0, 0);

    /// Creates a rect from its top-left pixel and size.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The column just past the right edge, saturating at `i32::MAX`.
    pub const fn right(self) -> i32 {
        self.x.saturating_add(self.width)
    }

    /// The row just past the bottom edge, saturating at `i32::MAX`.
    pub const fn bottom(self) -> i32 {
        self.y.saturating_add(self.height)
    }

    /// Returns `true` if the rect covers no pixels.
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Returns `true` if the pixel at `point` is inside the rect.
    pub const fn contains(self, point: DevicePoint) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }
}

/// Device pixels per logical unit. Always finite and greater than zero.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Scale(f32);

impl Scale {
    /// One device pixel per logical unit.
    pub const ONE: Self = Self(1.0);

    /// Creates a scale, or returns `None` if `factor` is not finite and
    /// greater than zero.
    pub fn new(factor: f32) -> Option<Self> {
        (factor.is_finite() && factor > 0.0).then_some(Self(factor))
    }

    /// The device pixels per logical unit.
    pub const fn factor(self) -> f32 {
        self.0
    }

    /// Converts a logical value to device pixels, rounding as described in
    /// the [module documentation](self).
    pub fn to_device<T: ToDevice>(self, value: T) -> T::Device {
        value.to_device(self)
    }

    /// Converts a device value to logical units.
    pub fn to_logical<T: ToLogical>(self, value: T) -> T::Logical {
        value.to_logical(self)
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::ONE
    }
}

/// A logical-unit value that converts to device pixels. See
/// [`Scale::to_device`].
pub trait ToDevice: Copy {
    /// The device-pixel type.
    type Device;

    /// Converts to device pixels at `scale`.
    fn to_device(self, scale: Scale) -> Self::Device;
}

/// A device-pixel value that converts to logical units. See
/// [`Scale::to_logical`].
pub trait ToLogical: Copy {
    /// The logical-unit type.
    type Logical;

    /// Converts to logical units at `scale`.
    fn to_logical(self, scale: Scale) -> Self::Logical;
}

impl ToDevice for Point {
    type Device = DevicePoint;

    fn to_device(self, scale: Scale) -> DevicePoint {
        let s = f64::from(scale.0);
        DevicePoint::new(
            to_i32((f64::from(self.x) * s).round()),
            to_i32((f64::from(self.y) * s).round()),
        )
    }
}

impl ToDevice for Size {
    type Device = DeviceSize;

    fn to_device(self, scale: Scale) -> DeviceSize {
        let s = f64::from(scale.0);
        DeviceSize::new(
            ceil_snapped(non_negative(self.width) * s),
            ceil_snapped(non_negative(self.height) * s),
        )
    }
}

impl ToDevice for Rect {
    type Device = DeviceRect;

    fn to_device(self, scale: Scale) -> DeviceRect {
        let s = f64::from(scale.0);
        let (left, top) = (f64::from(self.x), f64::from(self.y));
        let right = left + non_negative(self.width);
        let bottom = top + non_negative(self.height);
        let x = floor_snapped(left * s);
        let y = floor_snapped(top * s);
        let width = ceil_snapped(right * s).saturating_sub(x).max(0);
        let height = ceil_snapped(bottom * s).saturating_sub(y).max(0);
        DeviceRect::new(x, y, width, height)
    }
}

impl ToLogical for DevicePoint {
    type Logical = Point;

    fn to_logical(self, scale: Scale) -> Point {
        let s = f64::from(scale.0);
        Point::new(
            (f64::from(self.x) / s) as f32,
            (f64::from(self.y) / s) as f32,
        )
    }
}

impl ToLogical for DeviceSize {
    type Logical = Size;

    fn to_logical(self, scale: Scale) -> Size {
        let s = f64::from(scale.0);
        Size::new(
            (f64::from(self.width) / s) as f32,
            (f64::from(self.height) / s) as f32,
        )
    }
}

impl ToLogical for DeviceRect {
    type Logical = Rect;

    fn to_logical(self, scale: Scale) -> Rect {
        let s = f64::from(scale.0);
        Rect::new(
            (f64::from(self.x) / s) as f32,
            (f64::from(self.y) / s) as f32,
            (f64::from(self.width) / s) as f32,
            (f64::from(self.height) / s) as f32,
        )
    }
}

/// How close, in device pixels, a value must be to a whole pixel to count
/// as on it when rounding outward.
const SNAP: f64 = 1.0 / 64.0;

/// Rounds down, unless `value` is within [`SNAP`] of the integer above.
fn floor_snapped(value: f64) -> i32 {
    let nearest = value.round();
    to_i32(if (value - nearest).abs() <= SNAP {
        nearest
    } else {
        value.floor()
    })
}

/// Rounds up, unless `value` is within [`SNAP`] of the integer below.
fn ceil_snapped(value: f64) -> i32 {
    let nearest = value.round();
    to_i32(if (value - nearest).abs() <= SNAP {
        nearest
    } else {
        value.ceil()
    })
}

/// A width or height as `f64`, with negative values and NaN as zero.
fn non_negative(extent: f32) -> f64 {
    f64::from(extent).max(0.0)
}

/// Converts a whole number to `i32`, saturating at the ends of the range;
/// NaN becomes 0. (Float-to-int `as` casts saturate.)
fn to_i32(value: f64) -> i32 {
    value as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use crate::testing::check;

    /// Device coordinates for which round trips are documented as exact.
    const RANGE: i32 = 65_536;

    fn scale(factor: f32) -> Scale {
        Scale::new(factor).expect("a valid scale")
    }

    fn device_rect(rng: &mut Pcg32) -> DeviceRect {
        DeviceRect::new(
            rng.range(-RANGE..RANGE),
            rng.range(-RANGE..RANGE),
            rng.range(0..RANGE),
            rng.range(0..RANGE),
        )
    }

    /// The largest difference between corresponding edges.
    fn edge_error(a: DeviceRect, b: DeviceRect) -> i32 {
        [
            a.x.abs_diff(b.x),
            a.y.abs_diff(b.y),
            a.right().abs_diff(b.right()),
            a.bottom().abs_diff(b.bottom()),
        ]
        .into_iter()
        .max()
        .map_or(0, |error| i32::try_from(error).unwrap_or(i32::MAX))
    }

    #[test]
    fn scale_must_be_finite_and_positive() {
        assert_eq!(Scale::new(2.0).map(Scale::factor), Some(2.0));
        assert_eq!(Scale::default(), Scale::ONE);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(Scale::new(bad), None, "{bad}");
        }
    }

    #[test]
    fn known_conversions() {
        let two = scale(2.0);
        assert_eq!(
            two.to_device(Point::new(1.5, -2.25)),
            DevicePoint::new(3, -5)
        );
        assert_eq!(two.to_device(Size::new(10.25, 3.0)), DeviceSize::new(21, 6));
        assert_eq!(
            two.to_logical(DevicePoint::new(3, -5)),
            Point::new(1.5, -2.5)
        );
        assert_eq!(two.to_logical(DeviceSize::new(21, 6)), Size::new(10.5, 3.0));

        // Rects round outward at a fractional scale: 0.3 * 1.5 = 0.45 → 0,
        // and (0.3 + 1.0) * 1.5 = 1.95 → 2.
        let one_and_a_half = scale(1.5);
        assert_eq!(
            one_and_a_half.to_device(Rect::new(0.3, 0.3, 1.0, 1.0)),
            DeviceRect::new(0, 0, 2, 2)
        );
        assert_eq!(
            one_and_a_half.to_logical(DeviceRect::new(3, 6, 3, 3)),
            Rect::new(2.0, 4.0, 2.0, 2.0)
        );
        // Float error alone does not grow a rect: 0.1 * 10 is a hair over 1.
        assert_eq!(
            scale(10.0).to_device(Rect::new(0.1, 0.1, 0.2, 0.2)),
            DeviceRect::new(1, 1, 2, 2)
        );
    }

    #[test]
    fn degenerate_logical_values_convert_without_panicking() {
        let s = scale(2.0);
        assert_eq!(
            s.to_device(Rect::new(1.0, 1.0, -5.0, f32::NAN)),
            DeviceRect::new(2, 2, 0, 0)
        );
        assert_eq!(s.to_device(Size::new(-1.0, f32::NAN)), DeviceSize::ZERO);
        assert_eq!(
            s.to_device(Point::new(f32::NAN, f32::INFINITY)),
            DevicePoint::new(0, i32::MAX)
        );
        let huge = s.to_device(Rect::new(-3e38, -3e38, f32::MAX, f32::MAX));
        assert_eq!((huge.x, huge.y), (i32::MIN, i32::MIN));
        assert!(huge.width >= 0 && huge.height >= 0, "{huge:?}");
    }

    #[test]
    fn rect_queries() {
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!((rect.right(), rect.bottom()), (4.0, 6.0));
        assert_eq!(rect.origin(), Point::new(1.0, 2.0));
        assert_eq!(rect.size(), Size::new(3.0, 4.0));
        assert_eq!(Rect::from_origin_size(rect.origin(), rect.size()), rect);
        assert!(rect.contains(Point::new(1.0, 2.0)));
        assert!(!rect.contains(Point::new(4.0, 2.0)));
        assert!(!rect.is_empty());
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::new(0.0, 0.0, f32::NAN, 1.0).is_empty());

        let device = DeviceRect::new(1, 2, 3, 4);
        assert_eq!((device.right(), device.bottom()), (4, 6));
        assert!(device.contains(DevicePoint::new(3, 5)));
        assert!(!device.contains(DevicePoint::new(4, 5)));
        assert!(DeviceRect::new(0, 0, 0, 5).is_empty());
        assert_eq!(DeviceRect::new(i32::MAX, 0, 10, 1).right(), i32::MAX);
    }

    #[test]
    fn device_to_logical_to_device_is_exact_at_integer_scales() {
        check(
            "geom device round trip at integer scales",
            2000,
            |rng| (rng.range(1..5u32), device_rect(rng)),
            |&(factor, rect)| {
                let s = scale(factor as f32);
                let back = s.to_device(s.to_logical(rect));
                if back != rect {
                    return Err(format!("came back as {back:?}"));
                }
                let point = DevicePoint::new(rect.x, rect.y);
                let back = s.to_device(s.to_logical(point));
                if back != point {
                    return Err(format!("point came back as {back:?}"));
                }
                let size = DeviceSize::new(rect.width, rect.height);
                let back = s.to_device(s.to_logical(size));
                if back != size {
                    return Err(format!("size came back as {back:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn device_to_logical_to_device_is_within_a_pixel_at_fractional_scales() {
        const COMMON: [f32; 6] = [1.25, 1.5, 1.75, 2.25, 2.5, 3.5];
        check(
            "geom device round trip at fractional scales",
            2000,
            |rng| {
                let factor = if rng.range(0..2u32) == 0 {
                    *rng.choose(&COMMON).expect("not empty")
                } else {
                    rng.range(0.5..4.0f32)
                };
                (factor, device_rect(rng))
            },
            |&(factor, rect)| {
                let s = scale(factor);
                let back = s.to_device(s.to_logical(rect));
                let error = edge_error(rect, back);
                if error > 1 {
                    return Err(format!("came back as {back:?}, {error} px off"));
                }
                let point = DevicePoint::new(rect.x, rect.y);
                let back = s.to_device(s.to_logical(point));
                if back.x.abs_diff(point.x) > 1 || back.y.abs_diff(point.y) > 1 {
                    return Err(format!("point came back as {back:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn logical_rects_round_outward_to_cover_themselves() {
        check(
            "geom outward rounding covers the rect",
            2000,
            |rng| {
                let rect = Rect::new(
                    rng.range(-1000.0..1000.0f32),
                    rng.range(-1000.0..1000.0f32),
                    rng.range(0.0..500.0f32),
                    rng.range(0.0..500.0f32),
                );
                (rng.range(0.5..4.0f32), rect)
            },
            |&(factor, rect)| {
                let s = scale(factor);
                let device = s.to_device(rect);
                let covered = s.to_logical(device);
                // Within the snap tolerance, in logical units.
                let slack = (SNAP as f32) / factor + 1e-3;
                if covered.x > rect.x + slack
                    || covered.y > rect.y + slack
                    || covered.right() < rect.right() - slack
                    || covered.bottom() < rect.bottom() - slack
                {
                    return Err(format!("{device:?} ({covered:?}) does not cover it"));
                }
                // And it grows by less than one pixel on each side.
                let grow = 1.0 / factor + slack;
                if covered.x < rect.x - grow || covered.right() > rect.right() + grow {
                    return Err(format!("{device:?} ({covered:?}) is too wide"));
                }
                Ok(())
            },
        );
    }
}
