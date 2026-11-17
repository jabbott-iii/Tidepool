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

//! Clipping: [`intersect`] and the [`ClipStack`].
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! Every draw function takes a clip rect and touches only the pixels inside
//! it and the framebuffer's bounds. A painter keeps a [`ClipStack`] as it
//! walks a draw list and passes [`ClipStack::current`]; clipping to a
//! damage rect uses the same parameter.
//!
//! Edges are computed in `i64`, where `x + width` never overflows, so a
//! rect at `i32::MAX` or with a huge width clips like any other.

use tidepool_core::geom::DeviceRect;

/// The device space `gfx` draws in: from −2³⁰ to 2³⁰ − 1 on each axis.
///
/// A framebuffer's bounds must lie inside it, and
/// [`Damage`](crate::damage::Damage) tracks only what lies inside it, so
/// the bounding box of any two rects in it is a [`DeviceRect`] too. Draw
/// calls take geometry anywhere: what lies outside a framebuffer is not
/// drawn.
pub const SPACE: DeviceRect = DeviceRect::new(-(1 << 30), -(1 << 30), i32::MAX, i32::MAX);

/// The intersection of `a` and `b`: the pixels inside both.
///
/// Empty rects, and rects that do not overlap, give [`DeviceRect::ZERO`].
/// Nothing overflows: the edges are computed in `i64`, and the result is
/// no wider or taller than either rect.
pub fn intersect(a: DeviceRect, b: DeviceRect) -> DeviceRect {
    Edges::of(a).intersect(Edges::of(b)).to_rect()
}

/// A stack of clip rects, each the intersection of the rect pushed and the
/// one below it. The bottom one, the base, is never popped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipStack {
    /// The base first; never empty.
    stack: Vec<DeviceRect>,
}

impl ClipStack {
    /// A stack holding only `base`, usually the framebuffer's bounds or the
    /// damage rect being painted.
    pub fn new(base: DeviceRect) -> Self {
        Self { stack: vec![base] }
    }

    /// Pushes `rect` intersected with the current clip, so a push can only
    /// narrow it.
    pub fn push(&mut self, rect: DeviceRect) {
        let narrowed = intersect(self.current(), rect);
        self.stack.push(narrowed);
    }

    /// Pops the last rect pushed and returns `true`, or returns `false` and
    /// changes nothing if only the base is left.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    /// The current clip: the intersection of the base and every rect
    /// pushed and not yet popped.
    pub fn current(&self) -> DeviceRect {
        self.stack.last().copied().unwrap_or(DeviceRect::ZERO)
    }

    /// How many rects are pushed above the base: 0 for a new stack.
    pub fn depth(&self) -> usize {
        self.stack.len().saturating_sub(1)
    }
}

/// A rect as its four edges in `i64`: the left column and top row inside
/// it, and the column and row just past it. Empty when `right <= left` or
/// `bottom <= top`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edges {
    pub(crate) left: i64,
    pub(crate) top: i64,
    pub(crate) right: i64,
    pub(crate) bottom: i64,
}

impl Edges {
    /// The edges of `rect`. A negative side gives an empty rect.
    pub(crate) fn of(rect: DeviceRect) -> Self {
        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        Self {
            left,
            top,
            right: left + i64::from(rect.width.max(0)),
            bottom: top + i64::from(rect.height.max(0)),
        }
    }

    /// The pixels inside both.
    pub(crate) fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    pub(crate) fn width(self) -> i64 {
        (self.right - self.left).max(0)
    }

    pub(crate) fn height(self) -> i64 {
        (self.bottom - self.top).max(0)
    }

    /// As a [`DeviceRect`]: [`DeviceRect::ZERO`] if empty. Edges made from
    /// `DeviceRect`s by intersection always fit; others saturate.
    pub(crate) fn to_rect(self) -> DeviceRect {
        if self.is_empty() {
            return DeviceRect::ZERO;
        }
        let fit = |value: i64| {
            i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
        };
        DeviceRect::new(
            fit(self.left),
            fit(self.top),
            fit(self.width()),
            fit(self.height()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> DeviceRect {
        DeviceRect::new(x, y, width, height)
    }

    #[test]
    fn intersect_known_cases() {
        assert_eq!(
            intersect(rect(0, 0, 10, 10), rect(5, 2, 10, 3)),
            rect(5, 2, 5, 3)
        );
        // Contained.
        assert_eq!(
            intersect(rect(0, 0, 10, 10), rect(2, 2, 3, 3)),
            rect(2, 2, 3, 3)
        );
        // Touching edges share no pixel.
        assert_eq!(
            intersect(rect(0, 0, 10, 10), rect(10, 0, 5, 5)),
            DeviceRect::ZERO
        );
        // Disjoint, empty and negative-sized rects give the empty rect.
        assert_eq!(
            intersect(rect(0, 0, 4, 4), rect(100, 100, 4, 4)),
            DeviceRect::ZERO
        );
        assert_eq!(
            intersect(rect(0, 0, 4, 4), rect(1, 1, 0, 3)),
            DeviceRect::ZERO
        );
        assert_eq!(
            intersect(rect(0, 0, 4, 4), rect(1, 1, -3, 3)),
            DeviceRect::ZERO
        );
    }

    #[test]
    fn intersect_does_not_overflow_at_the_ends_of_the_range() {
        let huge = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(intersect(huge, rect(-5, -5, 3, 3)), rect(-5, -5, 3, 3));
        // `x + width` is past `i32::MAX`: the rect still holds the pixels
        // up to it.
        let far = rect(i32::MAX - 2, 0, i32::MAX, 1);
        assert_eq!(
            intersect(far, rect(i32::MAX - 10, 0, 9, 1)),
            rect(i32::MAX - 2, 0, 1, 1)
        );
        assert_eq!(intersect(far, far), far);
        assert_eq!(intersect(SPACE, SPACE), SPACE);
        assert_eq!(SPACE.right(), (1 << 30) - 1);
    }

    #[test]
    fn the_stack_narrows_and_never_pops_its_base() {
        let mut stack = ClipStack::new(rect(0, 0, 100, 100));
        assert_eq!((stack.depth(), stack.current()), (0, rect(0, 0, 100, 100)));
        assert!(!stack.pop());
        assert_eq!(stack.current(), rect(0, 0, 100, 100));

        stack.push(rect(50, -10, 100, 30));
        assert_eq!((stack.depth(), stack.current()), (1, rect(50, 0, 50, 20)));
        // A push can only narrow.
        stack.push(rect(0, 0, 1000, 1000));
        assert_eq!((stack.depth(), stack.current()), (2, rect(50, 0, 50, 20)));
        stack.push(rect(500, 500, 1, 1));
        assert_eq!((stack.depth(), stack.current()), (3, DeviceRect::ZERO));

        assert!(stack.pop());
        assert!(stack.pop());
        assert_eq!(stack.current(), rect(50, 0, 50, 20));
        assert!(stack.pop());
        assert!(!stack.pop());
        assert_eq!((stack.depth(), stack.current()), (0, rect(0, 0, 100, 100)));
    }

    #[test]
    fn edges_round_trip() {
        let edges = Edges::of(rect(-3, 4, 5, 6));
        assert_eq!(
            (edges.left, edges.top, edges.right, edges.bottom),
            (-3, 4, 2, 10)
        );
        assert_eq!((edges.width(), edges.height()), (5, 6));
        assert_eq!(edges.to_rect(), rect(-3, 4, 5, 6));
        assert!(Edges::of(rect(0, 0, -1, 5)).is_empty());
        assert_eq!(Edges::of(rect(0, 0, -1, 5)).to_rect(), DeviceRect::ZERO);
    }
}
