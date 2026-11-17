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

//! Damage: the few rects of a frame that changed, which is all a frame
//! repaints and presents ([`Damage`]), and [`diff`], which finds them
//! between two frames.
//!
//! New in Tidepool; not ported (Nadir draws with OpenGL).
//!
//! # Merging
//!
//! [`Damage::add`] keeps at most [`MAX_RECTS`] rects that neither overlap
//! nor touch, and whose union covers the union of every rect added:
//!
//! 1. An empty rect is dropped, and so is one inside an existing rect.
//! 2. Existing rects inside the new one are dropped.
//! 3. While the new rect overlaps or touches an existing one (sharing at
//!    least an edge segment, not just a corner), the two become their
//!    bounding box.
//! 4. While there are more than [`MAX_RECTS`], the pair whose bounding box
//!    adds the least area becomes that box (and is merged again as in 3).
//!    A tie goes to the pair that comes first, in the order the rects are
//!    kept.
//!
//! Everything is integer arithmetic in a fixed order, so the same adds
//! give the same rects everywhere. Damage covers [`SPACE`]; a rect is
//! clipped to it as it is added, so that the bounding box of any two rects
//! is itself a [`DeviceRect`].

use std::fmt;

use tidepool_core::geom::DeviceRect;

use crate::clip::{Edges, SPACE, intersect};
use crate::framebuffer::Framebuffer;

/// The most rects a [`Damage`] keeps.
pub const MAX_RECTS: usize = 8;

/// The side of the square tiles [`diff`] compares, in pixels.
pub const TILE: i32 = 16;

/// The changed parts of a frame: at most [`MAX_RECTS`] rects, merged as the
/// [module documentation](self) describes. It needs no allocation.
///
/// Equality and `Debug` look at [`Damage::rects`] only, in their order.
#[derive(Clone, Copy, Default)]
pub struct Damage {
    /// The rects, `len` of them; one more slot than [`MAX_RECTS`] while an
    /// add merges. Slots past `len` are kept at [`DeviceRect::ZERO`].
    slots: [DeviceRect; MAX_RECTS + 1],
    len: usize,
}

impl PartialEq for Damage {
    fn eq(&self, other: &Self) -> bool {
        self.rects() == other.rects()
    }
}

impl Eq for Damage {}

impl fmt::Debug for Damage {
    /// The rects.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Damage")
            .field("rects", &self.rects())
            .finish()
    }
}

impl Damage {
    /// No damage.
    pub const fn new() -> Self {
        Self {
            slots: [DeviceRect::ZERO; MAX_RECTS + 1],
            len: 0,
        }
    }

    /// Adds `rect`, clipped to [`SPACE`], merging as the
    /// [module documentation](self) describes.
    pub fn add(&mut self, rect: DeviceRect) {
        let rect = intersect(rect, SPACE);
        if rect.is_empty() {
            return;
        }
        self.insert(rect);
        while self.len > MAX_RECTS {
            let Some((first, second)) = self.cheapest_pair() else {
                break;
            };
            let merged = bounding_box(self.slot(first), self.slot(second));
            // `second` comes after `first`, so removing it first leaves
            // `first` where it is.
            self.remove(second);
            self.remove(first);
            self.insert(merged);
        }
    }

    /// The rects, which neither overlap nor touch.
    pub fn rects(&self) -> &[DeviceRect] {
        self.slots.get(..self.len).unwrap_or(&[])
    }

    /// Returns `true` if nothing is damaged.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Forgets every rect.
    pub fn clear(&mut self) {
        self.slots = [DeviceRect::ZERO; MAX_RECTS + 1];
        self.len = 0;
    }

    /// Clips every rect to `bounds`, usually the window's, dropping those
    /// left empty.
    pub fn clip_to(&mut self, bounds: DeviceRect) {
        let mut kept = 0;
        for index in 0..self.len {
            let clipped = intersect(self.slot(index), bounds);
            if !clipped.is_empty()
                && let Some(slot) = self.slots.get_mut(kept)
            {
                *slot = clipped;
                kept += 1;
            }
        }
        for slot in self.slots.iter_mut().skip(kept) {
            *slot = DeviceRect::ZERO;
        }
        self.len = kept;
    }

    /// The smallest rect holding every rect: [`DeviceRect::ZERO`] if there
    /// are none.
    pub fn bounds(&self) -> DeviceRect {
        self.rects()
            .iter()
            .copied()
            .reduce(bounding_box)
            .unwrap_or(DeviceRect::ZERO)
    }

    /// Steps 1 to 3 of the merge.
    fn insert(&mut self, mut rect: DeviceRect) {
        if self.rects().iter().any(|&kept| contains(kept, rect)) {
            return;
        }
        loop {
            let mut grew = false;
            let mut index = 0;
            while index < self.len {
                let kept = self.slot(index);
                if contains(rect, kept) {
                    self.remove(index);
                } else if touches(rect, kept) {
                    rect = bounding_box(rect, kept);
                    self.remove(index);
                    grew = true;
                } else {
                    index += 1;
                }
            }
            if !grew {
                break;
            }
        }
        if let Some(slot) = self.slots.get_mut(self.len) {
            *slot = rect;
            self.len += 1;
        }
    }

    /// The pair of rects whose bounding box adds the least area to theirs,
    /// the first such pair on a tie.
    fn cheapest_pair(&self) -> Option<(usize, usize)> {
        let mut best: Option<(i128, usize, usize)> = None;
        for first in 0..self.len {
            for second in first + 1..self.len {
                let (a, b) = (self.slot(first), self.slot(second));
                // The kept rects do not overlap, so this is the area the
                // box adds.
                let cost = area(bounding_box(a, b)) - area(a) - area(b);
                if best.is_none_or(|(least, _, _)| cost < least) {
                    best = Some((cost, first, second));
                }
            }
        }
        best.map(|(_, first, second)| (first, second))
    }

    fn slot(&self, index: usize) -> DeviceRect {
        self.slots.get(index).copied().unwrap_or(DeviceRect::ZERO)
    }

    fn remove(&mut self, index: usize) {
        if index < self.len {
            self.slots.copy_within(index + 1..self.len, index);
            self.len -= 1;
            if let Some(vacated) = self.slots.get_mut(self.len) {
                *vacated = DeviceRect::ZERO;
            }
        }
    }
}

/// The damage between two frames: rects covering every pixel that differs,
/// found by comparing [`TILE`] × [`TILE`] tiles. When the bounds differ, it
/// is all of `after`'s bounds.
pub fn diff(before: &Framebuffer, after: &Framebuffer) -> Damage {
    let mut damage = Damage::new();
    let bounds = after.bounds();
    if before.bounds() != bounds {
        damage.add(bounds);
        return damage;
    }
    let edges = Edges::of(bounds);
    let tile = i64::from(TILE);
    let mut top = edges.top;
    while top < edges.bottom {
        let bottom = (top + tile).min(edges.bottom);
        let mut left = edges.left;
        while left < edges.right {
            let right = (left + tile).min(edges.right);
            let differs = (top..bottom).any(|y| {
                let a = before.span(y, left, right).map(|(_, bytes)| bytes);
                let b = after.span(y, left, right).map(|(_, bytes)| bytes);
                a != b
            });
            if differs {
                damage.add(
                    Edges {
                        left,
                        top,
                        right,
                        bottom,
                    }
                    .to_rect(),
                );
            }
            left = right;
        }
        top = bottom;
    }
    damage
}

/// Returns `true` if every pixel of `inner` is in `outer`. An empty
/// `inner` is in any rect.
fn contains(outer: DeviceRect, inner: DeviceRect) -> bool {
    let (outer, inner) = (Edges::of(outer), Edges::of(inner));
    inner.is_empty()
        || (outer.left <= inner.left
            && outer.top <= inner.top
            && inner.right <= outer.right
            && inner.bottom <= outer.bottom)
}

/// Returns `true` if `a` and `b` overlap, or share at least a segment of
/// an edge; a shared corner alone does not count.
fn touches(a: DeviceRect, b: DeviceRect) -> bool {
    let (a, b) = (Edges::of(a), Edges::of(b));
    let x_closed = a.left <= b.right && b.left <= a.right;
    let y_closed = a.top <= b.bottom && b.top <= a.bottom;
    let x_open = a.left < b.right && b.left < a.right;
    let y_open = a.top < b.bottom && b.top < a.bottom;
    x_closed && y_closed && (x_open || y_open)
}

/// The smallest rect holding both. Both lie in [`SPACE`], so it fits.
fn bounding_box(a: DeviceRect, b: DeviceRect) -> DeviceRect {
    let (a, b) = (Edges::of(a), Edges::of(b));
    Edges {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
    .to_rect()
}

fn area(rect: DeviceRect) -> i128 {
    let edges = Edges::of(rect);
    i128::from(edges.width()) * i128::from(edges.height())
}

#[cfg(test)]
mod tests {
    use tidepool_core::color::PremultipliedColor;
    use tidepool_core::geom::{DevicePoint, DeviceSize};

    use super::*;
    use crate::raster::fill_rect;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> DeviceRect {
        DeviceRect::new(x, y, width, height)
    }

    fn damage(rects: &[DeviceRect]) -> Damage {
        let mut damage = Damage::new();
        for &rect in rects {
            damage.add(rect);
        }
        damage
    }

    #[test]
    fn empty_and_contained_rects_are_dropped() {
        let mut d = damage(&[rect(0, 0, 10, 10)]);
        d.add(rect(5, 5, 0, 3));
        d.add(rect(5, 5, -2, 3));
        d.add(rect(2, 2, 3, 3));
        assert_eq!(d.rects(), &[rect(0, 0, 10, 10)]);
        // A new rect swallows those inside it.
        let mut d = damage(&[rect(1, 1, 2, 2), rect(20, 20, 2, 2), rect(5, 5, 1, 1)]);
        assert_eq!(d.rects().len(), 3);
        d.add(rect(0, 0, 8, 8));
        assert_eq!(d.rects(), &[rect(20, 20, 2, 2), rect(0, 0, 8, 8)]);
        assert!(!d.is_empty());
        d.clear();
        assert!(d.is_empty());
        assert_eq!(d.rects(), &[]);
    }

    #[test]
    fn overlapping_and_touching_rects_merge_and_corners_do_not() {
        // Overlapping.
        assert_eq!(
            damage(&[rect(0, 0, 4, 4), rect(2, 2, 4, 4)]).rects(),
            &[rect(0, 0, 6, 6)]
        );
        // Sharing an edge.
        assert_eq!(
            damage(&[rect(0, 0, 4, 4), rect(4, 0, 4, 4)]).rects(),
            &[rect(0, 0, 8, 4)]
        );
        // Sharing a corner only.
        assert_eq!(
            damage(&[rect(0, 0, 4, 4), rect(4, 4, 4, 4)]).rects(),
            &[rect(0, 0, 4, 4), rect(4, 4, 4, 4)]
        );
        // A merge that grows into a third rect merges it too.
        assert_eq!(
            damage(&[rect(0, 0, 2, 2), rect(10, 0, 2, 2), rect(1, 0, 10, 1)]).rects(),
            &[rect(0, 0, 12, 2)]
        );
    }

    #[test]
    fn over_the_limit_the_cheapest_pair_merges() {
        // Nine 1 px rects along a row: at x = 0, 3, 6, …, 21, two columns
        // apart, and one at 23, one column after the eighth.
        let mut rects: Vec<DeviceRect> = (0..8).map(|i| rect(i * 3, 0, 1, 1)).collect();
        rects.push(rect(23, 0, 1, 1));
        let d = damage(&rects);
        assert_eq!(d.rects().len(), MAX_RECTS);
        // 21 and 23 are 1 px apart: their box (21, 0, 3, 1) adds 1 px; every
        // other pair adds at least 2.
        assert!(d.rects().contains(&rect(21, 0, 3, 1)), "{:?}", d.rects());

        // A tie goes to the first pair: nine rects 2 px apart, so every
        // neighbouring pair adds 2 px, and the first two merge.
        let rects: Vec<DeviceRect> = (0..9).map(|i| rect(i * 3, 0, 1, 1)).collect();
        let d = damage(&rects);
        assert_eq!(d.rects().len(), MAX_RECTS);
        assert!(d.rects().contains(&rect(0, 0, 4, 1)), "{:?}", d.rects());
    }

    #[test]
    fn rects_are_clipped_to_the_space_and_the_window() {
        let huge = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        let far = rect(i32::MAX - 10, i32::MAX - 10, 100, 100);
        let mut d = damage(&[huge, far]);
        // `far` lies outside the space, so only the clipped `huge` is left.
        assert_eq!(
            d.rects(),
            &[rect(SPACE.x, SPACE.y, (1 << 30) - 1, (1 << 30) - 1)]
        );
        d.add(rect(-100, 1 << 29, 10, 10));
        d.clip_to(rect(0, 0, 640, 480));
        assert_eq!(d.rects(), &[]);
        let mut d = damage(&[
            rect(-5, -5, 10, 10),
            rect(600, 400, 100, 100),
            rect(1000, 0, 5, 5),
        ]);
        d.clip_to(rect(0, 0, 640, 480));
        assert_eq!(d.rects(), &[rect(0, 0, 5, 5), rect(600, 400, 40, 80)]);
        assert_eq!(d.bounds(), rect(0, 0, 640, 480));
        assert_eq!(Damage::new().bounds(), DeviceRect::ZERO);
    }

    #[test]
    fn equality_and_debug_see_only_the_rects() {
        let mut d = damage(&[rect(3, 3, 4, 4)]);
        d.clear();
        assert_eq!(d, Damage::new());
        assert_eq!(format!("{d:?}"), "Damage { rects: [] }");

        // After a merge, which removes rects, it equals a damage built
        // from the merged rects directly.
        let merged = damage(&[rect(0, 0, 4, 4), rect(20, 0, 2, 2), rect(4, 0, 4, 4)]);
        assert_eq!(merged.rects(), &[rect(20, 0, 2, 2), rect(0, 0, 8, 4)]);
        assert_eq!(merged, damage(&[rect(20, 0, 2, 2), rect(0, 0, 8, 4)]));
        assert_eq!(
            format!("{merged:?}"),
            format!("{:?}", damage(&[rect(20, 0, 2, 2), rect(0, 0, 8, 4)]))
        );

        // After a clip that drops rects, likewise.
        let mut clipped = damage(&[rect(-10, -10, 5, 5), rect(0, 0, 3, 3), rect(50, 50, 3, 3)]);
        clipped.clip_to(rect(0, 0, 20, 20));
        assert_eq!(clipped.rects(), &[rect(0, 0, 3, 3)]);
        assert_eq!(clipped, damage(&[rect(0, 0, 3, 3)]));

        // Over the limit, the merged result equals one built fresh.
        let rects: Vec<DeviceRect> = (0..9).map(|i| rect(i * 3, 0, 1, 1)).collect();
        let full = damage(&rects);
        assert_eq!(full, damage(full.rects()));
        assert_ne!(full, Damage::new());
    }

    #[test]
    fn diff_finds_changed_tiles() {
        let mut before = Framebuffer::new(DeviceSize::new(40, 20)).expect("small");
        before.clear(PremultipliedColor {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        });
        let mut after = before.clone();
        assert!(diff(&before, &after).is_empty());
        // One pixel in the second tile of the first row of tiles.
        fill_rect(
            &mut after,
            SPACE,
            rect(17, 3, 1, 1),
            PremultipliedColor {
                r: 9,
                g: 9,
                b: 9,
                a: 255,
            },
        );
        assert_eq!(diff(&before, &after).rects(), &[rect(16, 0, 16, 16)]);
        // A pixel in the last, partial tile is clipped to the bounds.
        fill_rect(
            &mut after,
            SPACE,
            rect(39, 19, 1, 1),
            PremultipliedColor {
                r: 9,
                g: 9,
                b: 9,
                a: 255,
            },
        );
        assert_eq!(
            diff(&before, &after).rects(),
            &[rect(16, 0, 16, 16), rect(32, 16, 8, 4)]
        );
        assert_eq!(after.pixel(DevicePoint::new(39, 19)).map(|c| c.r), Some(9));
    }

    #[test]
    fn diff_of_different_bounds_is_the_whole_frame() {
        let before = Framebuffer::new(DeviceSize::new(4, 4)).expect("small");
        let after = Framebuffer::new_at(rect(2, 2, 4, 4)).expect("small");
        assert_eq!(diff(&before, &after).rects(), &[rect(2, 2, 4, 4)]);
        let empty = Framebuffer::new(DeviceSize::ZERO).expect("empty");
        assert!(diff(&before, &empty).is_empty());
    }
}
