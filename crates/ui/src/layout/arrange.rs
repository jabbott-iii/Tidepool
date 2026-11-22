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

//! The layout pass: natural sizes, then arranging each node's children,
//! as the [`layout`](super) module's documentation describes.
//!
//! New in Tidepool; not ported.

use tidepool_core::geom::{Point, Rect, Size};

use super::{Align, Axis, Insets, Layout, Length, MAX_EXTENT, Style, length, sane_size};
use crate::metrics::TextMetrics;
use crate::overlay::{Anchor, Side};
use crate::tree::{NodeId, Tree};

/// How many times [`fit`] steps a length down by one unit in the last
/// place before it gives up and makes it 0.
const FIT_STEPS: u32 = 64;

/// How many natural sizes a node remembers in a pass, by hint.
const MEMO_ENTRIES: usize = 4;

/// The natural sizes a node was measured at in this layout pass, by hint:
/// the last [`MEMO_ENTRIES`], the oldest forgotten first. A pass asks a
/// node again only with the hint it was just asked with (a scrolling
/// parent measures its children, then arranges them), so a few entries
/// keep measuring from being exponential, and memory stays linear in the
/// nodes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Memo {
    /// (hint, size), the first `len` of them in use.
    entries: [(Size, Size); MEMO_ENTRIES],
    /// How many entries are in use.
    len: usize,
    /// The entry the next size goes in.
    next: usize,
}

impl Memo {
    /// The size remembered for `hint`, if any.
    fn get(&self, hint: Size) -> Option<Size> {
        self.entries
            .iter()
            .take(self.len)
            .find(|(seen, _)| *seen == hint)
            .map(|&(_, size)| size)
    }

    /// Remembers `size` for `hint`, in place of the oldest entry if every
    /// one is in use.
    fn insert(&mut self, hint: Size, size: Size) {
        if let Some(entry) = self.entries.get_mut(self.next) {
            *entry = (hint, size);
        }
        self.next = (self.next + 1) % MEMO_ENTRIES;
        self.len = (self.len + 1).min(MEMO_ENTRIES);
    }

    /// Forgets every size.
    fn clear(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    /// How many sizes it remembers.
    #[cfg(test)]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }
}

/// What the last layout gave a node.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Placement {
    /// Its bounds.
    pub(crate) rect: Rect,
    /// Its content box: the bounds less the padding.
    pub(crate) viewport: Rect,
    /// The size of the area its children were laid out in.
    pub(crate) content_size: Size,
    /// The intersection of the window and its clipping ancestors' bounds.
    pub(crate) clip: Rect,
}

/// The layout of a row or column: its axis, gap and alignment.
#[derive(Clone, Copy)]
struct Line {
    /// Whether the main axis is horizontal (a row).
    horizontal: bool,
    /// The gap between children.
    gap: f32,
    /// The alignment on the main axis.
    main: Align,
    /// The alignment on the cross axis.
    cross: Align,
}

/// A child's length, limits and natural size on one axis.
#[derive(Clone, Copy)]
struct Extent {
    len: Length,
    min: f32,
    max: f32,
    natural: f32,
}

impl Extent {
    /// Its size where [`Length::Fill`] is `fill`, clamped by its limits.
    fn size(self, fill: f32) -> f32 {
        let size = match self.len {
            Length::Fixed(value) => value,
            Length::Content => self.natural,
            Length::Fill(_) => fill,
        };
        between(size, self.min, self.max)
    }

    /// Its fill weight, if it fills.
    fn weight(self) -> Option<f32> {
        match self.len {
            Length::Fill(weight) => Some(weight),
            _ => None,
        }
    }
}

/// A child, on both axes.
#[derive(Clone, Copy)]
struct Item {
    x: Extent,
    y: Extent,
}

impl Item {
    /// The main and cross extents of a row (`horizontal`) or column.
    const fn along(self, horizontal: bool) -> (Extent, Extent) {
        if horizontal {
            (self.x, self.y)
        } else {
            (self.y, self.x)
        }
    }
}

impl Tree {
    /// Lays out the tree in a window of `size`, which is sane: the main
    /// root's layer, then each overlay's in order.
    pub(crate) fn run_layout(&mut self, size: Size, metrics: &dyn TextMetrics) {
        for node in self.nodes_mut() {
            node.memo.clear();
        }
        // What is under the pointer, found again as the passes go, is
        // found in this window.
        self.layout_size = Some(size);
        let window = Rect::new(0.0, 0.0, size.width, size.height);
        let root = self.root();
        if self.get(root).is_some_and(|node| node.sane.hidden) {
            self.arrange_hidden_root(window);
        } else {
            self.arrange(root, window, window, metrics);
        }
        self.arrange_overlays(window, metrics);
    }

    /// Lays out each overlay, in order, but each after the layer its
    /// anchor is in, placed by its anchor as the
    /// [`overlay`](crate::overlay) module's documentation describes, its
    /// paint cut to the window alone.
    fn arrange_overlays(&mut self, window: Rect, metrics: &dyn TextMetrics) {
        let mut done = vec![false; self.overlays.len()];
        for at in 0..self.overlays.len() {
            self.arrange_overlay(at, &mut done, window, metrics);
        }
    }

    /// Lays out the overlay at `at`, after the overlay its anchor is in if
    /// that is not laid out yet, unless `done` says it is.
    fn arrange_overlay(
        &mut self,
        at: usize,
        done: &mut [bool],
        window: Rect,
        metrics: &dyn TextMetrics,
    ) {
        // The chain of anchors it waits on, the first last: at most every
        // overlay, as anchors make no ring (`set_overlay_anchor` refuses
        // one), and `done` cuts it short if one did.
        let mut chain = vec![at];
        while let Some(&last) = chain.last() {
            let Some(host) = self.anchor_layer(last) else {
                break;
            };
            let Some(before) = self.overlays.iter().position(|layer| layer.root == host) else {
                break;
            };
            if done.get(before).copied().unwrap_or(true) || chain.contains(&before) {
                break;
            }
            chain.push(before);
        }
        for at in chain.into_iter().rev() {
            if let Some(flag) = done.get_mut(at) {
                if *flag {
                    continue;
                }
                *flag = true;
            }
            self.place_overlay(at, window, metrics);
        }
    }

    /// The size an overlay whose root is `root`, anchored to a window
    /// `window` with `margin` and lifted by nothing, gets from the next
    /// layout pass, and the height of the area it is placed in: the window
    /// less the margin. From the sizes this layout pass measured.
    pub(crate) fn window_overlay_fit(
        &mut self,
        root: NodeId,
        window: Rect,
        margin: f32,
        metrics: &dyn TextMetrics,
    ) -> (Size, f32) {
        let area = margined(window, length(margin));
        let item = self.item(root, area.size(), metrics);
        let rect = in_cell(area, item, Align::End, Align::End);
        (rect.size(), area.height)
    }

    /// Lays out the overlay at `at` from where its anchor is now.
    fn place_overlay(&mut self, at: usize, window: Rect, metrics: &dyn TextMetrics) {
        let Some(layer) = self.overlays.get(at) else {
            return;
        };
        let (root, anchor, last, lift) =
            (layer.root, layer.anchor, layer.anchor_bounds, layer.lift);
        // The anchor's layer was laid out first in this pass; a node
        // since removed keeps where it last was. Hidden or not, the
        // overlay follows it, so it is beside it when shown again.
        let target = match anchor {
            Anchor::Node { node, .. } => self.bounds(node).unwrap_or(last),
            Anchor::Window { .. } => last,
        };
        if let Some(layer) = self.overlays.get_mut(at) {
            layer.anchor_bounds = target;
        }
        let Some(node) = self.get(root) else {
            return;
        };
        if node.sane.hidden {
            self.arrange_hidden(root, window.origin(), window);
            return;
        }
        let rect = match anchor {
            Anchor::Window { x, y, margin } => {
                let area = margined(window, length(margin));
                let item = self.item(root, area.size(), metrics);
                let placed = fit(in_cell(area, item, x, y), area);
                // Lifted, but never above the area's top.
                let top = between(placed.y - lift, area.y, placed.y);
                fit(Rect { y: top, ..placed }, area)
            }
            Anchor::Node {
                side, align, gap, ..
            } => {
                let item = self.item(root, window.size(), metrics);
                let size = Size::new(
                    item.x.size(window.width).min(window.width),
                    item.y.size(window.height).min(window.height),
                );
                fit(
                    beside(window, size, target, side, align, length(gap)),
                    window,
                )
            }
        };
        self.arrange(root, rect, window, metrics);
    }

    /// Gives a hidden root the window, which the root always fills, and
    /// its subtree empty rects at its content box's origin, as hidden
    /// nodes get. Its scroll offset is kept, as a hidden node's is.
    fn arrange_hidden_root(&mut self, window: Rect) {
        let root = self.root();
        let Some(node) = self.get_mut(root) else {
            return;
        };
        let content = content_box(window, node.sane.padding);
        let placement = Placement {
            rect: window,
            viewport: content,
            content_size: content.size(),
            clip: window,
        };
        let changed = node.placement != placement;
        node.placement = placement;
        let children = node.children.clone();
        if changed {
            self.mark(root);
        }
        for child in children {
            self.arrange_hidden(child, content.origin(), window);
        }
    }

    /// The node's natural size given `available` (a hint; either side may
    /// be infinite), remembered for the rest of the pass.
    pub(crate) fn natural(
        &mut self,
        id: NodeId,
        available: Size,
        metrics: &dyn TextMetrics,
    ) -> Size {
        let Some(node) = self.get(id) else {
            return Size::ZERO;
        };
        if let Some(size) = node.memo.get(available) {
            return size;
        }
        let style = node.sane;
        let size =
            if let (Length::Fixed(width), Length::Fixed(height)) = (style.width, style.height) {
                // Its content cannot change its size.
                Size::new(width, height)
            } else {
                let pad = style.padding;
                let inner = Size::new(
                    (available.width - pad.left - pad.right).max(0.0),
                    (available.height - pad.top - pad.bottom).max(0.0),
                );
                let measured = node.widget.as_ref().map_or(Size::ZERO, |widget| {
                    sane_size(widget.measure(inner, metrics))
                });
                let visible = self.visible_children(id);
                let children = self.children_natural(&visible, &style, inner, metrics);
                let content = Size::new(
                    measured.width.max(children.width),
                    measured.height.max(children.height),
                );
                Size::new(
                    match style.width {
                        Length::Fixed(width) => width,
                        _ => content.width + pad.left + pad.right,
                    },
                    match style.height {
                        Length::Fixed(height) => height,
                        _ => content.height + pad.top + pad.bottom,
                    },
                )
            };
        let size = Size::new(
            between(size.width, style.min.width, style.max.width).min(MAX_EXTENT),
            between(size.height, style.min.height, style.max.height).min(MAX_EXTENT),
        );
        if let Some(node) = self.get_mut(id) {
            node.memo.insert(available, size);
        }
        size
    }

    /// The size `children` take at their natural sizes in `style`'s layout.
    /// May exceed [`MAX_EXTENT`].
    fn children_natural(
        &mut self,
        children: &[NodeId],
        style: &Style,
        available: Size,
        metrics: &dyn TextMetrics,
    ) -> Size {
        if children.is_empty() {
            return Size::ZERO;
        }
        let sizes: Vec<Size> = children
            .iter()
            .map(|&child| self.natural(child, available, metrics))
            .collect();
        let gap = style.gap;
        let gaps = gap * (sizes.len() as f32 - 1.0);
        let widest = sizes.iter().map(|size| size.width).fold(0.0, f32::max);
        let tallest = sizes.iter().map(|size| size.height).fold(0.0, f32::max);
        match style.layout {
            Layout::Column
            | Layout::Split {
                axis: Axis::Vertical,
                ..
            } => Size::new(
                widest,
                sizes.iter().map(|size| size.height).sum::<f32>() + gaps,
            ),
            Layout::Row
            | Layout::Split {
                axis: Axis::Horizontal,
                ..
            } => Size::new(
                sizes.iter().map(|size| size.width).sum::<f32>() + gaps,
                tallest,
            ),
            Layout::Stack => Size::new(widest, tallest),
            Layout::Grid { columns } => {
                let columns = usize::from(columns.max(1));
                let rows: Vec<f32> = sizes
                    .chunks(columns)
                    .map(|row| row.iter().map(|size| size.height).fold(0.0, f32::max))
                    .collect();
                // Every column, used or not: arranging divides the width
                // among them all.
                let across = columns as f32;
                Size::new(
                    across * widest + gap * (across - 1.0),
                    rows.iter().sum::<f32>() + gap * (rows.len() as f32 - 1.0),
                )
            }
        }
    }

    /// The node's children that are not hidden, in order.
    fn visible_children(&self, id: NodeId) -> Vec<NodeId> {
        self.get(id).map_or_else(Vec::new, |node| {
            node.children
                .iter()
                .copied()
                .filter(|&child| self.get(child).is_some_and(|child| !child.sane.hidden))
                .collect()
        })
    }

    /// The child's extents, with its natural size given `hint`.
    fn item(&mut self, id: NodeId, hint: Size, metrics: &dyn TextMetrics) -> Item {
        let style = self.get(id).map_or_else(Style::default, |node| node.sane);
        let natural = self.natural(id, hint, metrics);
        Item {
            x: Extent {
                len: style.width,
                min: style.min.width,
                max: style.max.width,
                natural: natural.width,
            },
            y: Extent {
                len: style.height,
                min: style.min.height,
                max: style.max.height,
                natural: natural.height,
            },
        }
    }

    /// Gives the node `rect` and lays out its subtree. `clip` is the
    /// intersection of the window and its clipping ancestors' bounds.
    fn arrange(&mut self, id: NodeId, rect: Rect, clip: Rect, metrics: &dyn TextMetrics) {
        let Some(node) = self.get(id) else {
            return;
        };
        let style = node.sane;
        let stored = node.scroll;
        let children = node.children.clone();
        let content = content_box(rect, style.padding);
        // Children measure against the content box, unbounded on an axis
        // that scrolls.
        let hint = Size::new(
            if style.scroll_x {
                f32::INFINITY
            } else {
                content.width
            },
            if style.scroll_y {
                f32::INFINITY
            } else {
                content.height
            },
        );
        let visible = self.visible_children(id);
        let mut extent = content.size();
        if style.scroll_x || style.scroll_y {
            let natural = self.children_natural(&visible, &style, hint, metrics);
            if style.scroll_x {
                extent.width = extent.width.max(natural.width.min(MAX_EXTENT));
            }
            if style.scroll_y {
                extent.height = extent.height.max(natural.height.min(MAX_EXTENT));
            }
        }
        let offset = Point::new(
            if style.scroll_x {
                between(stored.x, 0.0, extent.width - content.width)
            } else {
                0.0
            },
            if style.scroll_y {
                between(stored.y, 0.0, extent.height - content.height)
            } else {
                0.0
            },
        );
        let area = Rect::new(
            content.x - offset.x,
            content.y - offset.y,
            extent.width,
            extent.height,
        );
        let placement = Placement {
            rect,
            viewport: content,
            content_size: extent,
            clip,
        };
        let changed = self.get_mut(id).is_some_and(|node| {
            let changed = node.placement != placement || node.scroll != offset;
            node.placement = placement;
            node.scroll = offset;
            changed
        });
        if changed {
            self.mark(id);
        }

        let rects = self.place(&visible, &style, area, hint, metrics);
        let inner_clip = if style.clips() {
            intersect(clip, rect)
        } else {
            clip
        };
        for (&child, rect) in visible.iter().zip(rects) {
            self.arrange(child, fit(rect, area), inner_clip, metrics);
        }
        let origin = fit(Rect::from_origin_size(area.origin(), Size::ZERO), area).origin();
        for child in children {
            if self.get(child).is_some_and(|child| child.sane.hidden) {
                self.arrange_hidden(child, origin, inner_clip);
            }
        }
    }

    /// Gives a hidden node, and every node below it, an empty rect at
    /// `origin`.
    fn arrange_hidden(&mut self, id: NodeId, origin: Point, clip: Rect) {
        let rect = Rect::from_origin_size(origin, Size::ZERO);
        let placement = Placement {
            rect,
            viewport: rect,
            content_size: Size::ZERO,
            clip,
        };
        let Some(node) = self.get_mut(id) else {
            return;
        };
        let changed = node.placement != placement;
        node.placement = placement;
        let children = node.children.clone();
        if changed {
            self.mark(id);
        }
        for child in children {
            self.arrange_hidden(child, origin, clip);
        }
    }

    /// The rects of the visible `children` in `area`, by `style`'s layout,
    /// before they are fitted.
    fn place(
        &mut self,
        children: &[NodeId],
        style: &Style,
        area: Rect,
        hint: Size,
        metrics: &dyn TextMetrics,
    ) -> Vec<Rect> {
        let line = |horizontal: bool| Line {
            horizontal,
            gap: style.gap,
            main: if horizontal {
                style.align_x
            } else {
                style.align_y
            },
            cross: if horizontal {
                style.align_y
            } else {
                style.align_x
            },
        };
        match style.layout {
            Layout::Column => self.line(children, area, line(false), hint, metrics),
            Layout::Row => self.line(children, area, line(true), hint, metrics),
            Layout::Split { axis, ratio } => {
                let spec = line(axis == Axis::Horizontal);
                self.split(children, area, spec, ratio, hint, metrics)
            }
            Layout::Stack => children
                .iter()
                .map(|&child| {
                    let item = self.item(child, hint, metrics);
                    in_cell(area, item, style.align_x, style.align_y)
                })
                .collect(),
            Layout::Grid { columns } => self.grid(children, area, columns, style, hint, metrics),
        }
    }

    /// The rects of `children` in a row or column filling `area`.
    fn line(
        &mut self,
        children: &[NodeId],
        area: Rect,
        spec: Line,
        hint: Size,
        metrics: &dyn TextMetrics,
    ) -> Vec<Rect> {
        if children.is_empty() {
            return Vec::new();
        }
        let items: Vec<Item> = children
            .iter()
            .map(|&child| self.item(child, hint, metrics))
            .collect();
        let horizontal = spec.horizontal;
        let (main_start, cross_start, main_extent, cross_extent) = if horizontal {
            (area.x, area.y, area.width, area.height)
        } else {
            (area.y, area.x, area.height, area.width)
        };
        let gaps = (items.len() - 1) as f32;
        let gap = if gaps > 0.0 && spec.gap * gaps > main_extent {
            main_extent / gaps
        } else {
            spec.gap
        };
        let avail = (main_extent - gap * gaps).max(0.0);

        // Fixed and natural sizes first; fills start at 0.
        let mut sizes: Vec<f32> = Vec::with_capacity(items.len());
        let mut base = 0.0;
        for item in &items {
            let (main, _) = item.along(horizontal);
            if main.weight().is_some() {
                sizes.push(0.0);
            } else {
                let size = main.size(0.0);
                base += size;
                sizes.push(size);
            }
        }
        if base > avail {
            // Shrink everything in proportion; fills get nothing.
            let scale = avail / base;
            for (size, item) in sizes.iter_mut().zip(&items) {
                if item.along(horizontal).0.weight().is_none() {
                    *size *= scale;
                }
            }
        } else {
            share(&mut sizes, &items, horizontal, avail - base);
        }

        let used = sizes.iter().sum::<f32>() + gap * gaps;
        let mut position = main_start + (main_extent - used).max(0.0) * spec.main.factor();
        let mut rects = Vec::with_capacity(items.len());
        for (&size, item) in sizes.iter().zip(&items) {
            let (_, cross) = item.along(horizontal);
            let across = cross.size(cross_extent).min(cross_extent);
            let at = cross_start + (cross_extent - across).max(0.0) * spec.cross.factor();
            rects.push(if horizontal {
                Rect::new(position, at, size, across)
            } else {
                Rect::new(at, position, across, size)
            });
            position += size + gap;
        }
        rects
    }

    /// The rects of `children` in a split of `area`: the first takes
    /// `ratio` of the space, and the rest are a row or column after it and
    /// one gap.
    fn split(
        &mut self,
        children: &[NodeId],
        area: Rect,
        spec: Line,
        ratio: f32,
        hint: Size,
        metrics: &dyn TextMetrics,
    ) -> Vec<Rect> {
        let Some((&first, rest)) = children.split_first() else {
            return Vec::new();
        };
        let horizontal = spec.horizontal;
        let (main_start, cross_start, main_extent, cross_extent) = if horizontal {
            (area.x, area.y, area.width, area.height)
        } else {
            (area.y, area.x, area.height, area.width)
        };
        let gap = if rest.is_empty() {
            0.0
        } else {
            spec.gap.min(main_extent)
        };
        let avail = (main_extent - gap).max(0.0);
        let (main, cross) = self.item(first, hint, metrics).along(horizontal);
        let size = between(ratio * avail, main.min, main.max).min(avail);
        let across = cross.size(cross_extent).min(cross_extent);
        let at = cross_start + (cross_extent - across).max(0.0) * spec.cross.factor();
        let mut rects = vec![if horizontal {
            Rect::new(main_start, at, size, across)
        } else {
            Rect::new(at, main_start, across, size)
        }];
        let taken = size + gap;
        let left = (main_extent - taken).max(0.0);
        let after = if horizontal {
            Rect::new(main_start + taken, area.y, left, area.height)
        } else {
            Rect::new(area.x, main_start + taken, area.width, left)
        };
        rects.extend(self.line(rest, after, spec, hint, metrics));
        rects
    }

    /// The rects of `children` in a grid of `columns` filling `area`.
    fn grid(
        &mut self,
        children: &[NodeId],
        area: Rect,
        columns: u16,
        style: &Style,
        hint: Size,
        metrics: &dyn TextMetrics,
    ) -> Vec<Rect> {
        if children.is_empty() {
            return Vec::new();
        }
        let items: Vec<Item> = children
            .iter()
            .map(|&child| self.item(child, hint, metrics))
            .collect();
        let columns = usize::from(columns.max(1));
        let column_gaps = (columns - 1) as f32;
        let gap_x = if column_gaps > 0.0 && style.gap * column_gaps > area.width {
            area.width / column_gaps
        } else {
            style.gap
        };
        let column_width = (area.width - gap_x * column_gaps).max(0.0) / columns as f32;
        let mut heights: Vec<f32> = items
            .chunks(columns)
            .map(|row| {
                row.iter()
                    .map(|item| item.y.size(item.y.natural))
                    .fold(0.0, f32::max)
            })
            .collect();
        let row_gaps = (heights.len() - 1) as f32;
        let gap_y = if row_gaps > 0.0 && style.gap * row_gaps > area.height {
            area.height / row_gaps
        } else {
            style.gap
        };
        if !style.scroll_y {
            let total: f32 = heights.iter().sum();
            if total + gap_y * row_gaps > area.height && total > 0.0 {
                let scale = (area.height - gap_y * row_gaps).max(0.0) / total;
                for height in &mut heights {
                    *height *= scale;
                }
            }
        }
        let mut rects = Vec::with_capacity(items.len());
        let mut y = area.y;
        for (row, &height) in items.chunks(columns).zip(&heights) {
            // Each column at the sum of the one before, as each row is, so
            // that a cell's right edge is its neighbour's left in `f32`.
            let mut x = area.x;
            for &item in row {
                let cell = Rect::new(x, y, column_width, height);
                rects.push(in_cell(cell, item, style.align_x, style.align_y));
                x += column_width + gap_x;
            }
            y += height + gap_y;
        }
        rects
    }
}

/// Shares `free` among the fills of a row or column by weight, as CSS
/// flexbox does, and scales the fills down if their minimums still take
/// more than `free`.
///
/// Each round gives every fill not yet frozen its share of what is left,
/// clamped by its limits. If clamping added space in all, the fills held up
/// at their minimums are frozen there; if it took space away, those held
/// down at their maximums; if neither, every fill keeps its clamped share
/// and sharing ends. Fills whose weights are all 0 take their minimum.
fn share(sizes: &mut [f32], items: &[Item], horizontal: bool, free: f32) {
    let mains: Vec<Extent> = items.iter().map(|item| item.along(horizontal).0).collect();
    let mut frozen: Vec<bool> = mains.iter().map(|main| main.weight().is_none()).collect();
    let mut portions = vec![0.0; mains.len()];
    let mut left = free;
    // Each round but the last freezes a fill, so there are at most as many
    // rounds as fills, and one more.
    for _ in 0..=items.len() {
        let total: f32 = mains
            .iter()
            .zip(&frozen)
            .filter(|&(_, &frozen)| !frozen)
            .filter_map(|(main, _)| main.weight())
            .sum();
        if total <= 0.0 {
            // No weight among the fills left: each takes its minimum.
            for ((size, main), _) in sizes
                .iter_mut()
                .zip(&mains)
                .zip(&frozen)
                .filter(|&(_, &frozen)| !frozen)
            {
                *size = between(0.0, main.min, main.max);
            }
            break;
        }
        // Every share, clamped, and how much the clamping added in all.
        let mut added = 0.0;
        for (((size, portion), main), &frozen) in
            sizes.iter_mut().zip(&mut portions).zip(&mains).zip(&frozen)
        {
            let Some(weight) = main.weight().filter(|_| !frozen) else {
                continue;
            };
            *portion = left * weight / total;
            *size = between(*portion, main.min, main.max);
            added += *size - *portion;
        }
        if added == 0.0 {
            break;
        }
        // Freeze the fills the clamping held in the direction it went.
        let mut taken = 0.0;
        for ((&size, &portion), frozen) in sizes.iter().zip(&portions).zip(frozen.iter_mut()) {
            if *frozen {
                continue;
            }
            let held = if added > 0.0 {
                size > portion
            } else {
                size < portion
            };
            if held {
                *frozen = true;
                taken += size;
            }
        }
        left -= taken;
    }
    let fills: f32 = sizes
        .iter()
        .zip(&mains)
        .filter(|(_, main)| main.weight().is_some())
        .map(|(size, _)| *size)
        .sum();
    if fills > free && fills > 0.0 {
        let scale = free / fills;
        for (size, main) in sizes.iter_mut().zip(&mains) {
            if main.weight().is_some() {
                *size *= scale;
            }
        }
    }
}

/// The rect of a child in a cell (or a stack's area): each axis at its
/// fixed or natural size, or the cell's for a fill, clamped by its limits
/// and the cell, placed by the alignment.
fn in_cell(cell: Rect, item: Item, align_x: Align, align_y: Align) -> Rect {
    let width = item.x.size(cell.width).min(cell.width);
    let height = item.y.size(cell.height).min(cell.height);
    Rect::new(
        cell.x + (cell.width - width).max(0.0) * align_x.factor(),
        cell.y + (cell.height - height).max(0.0) * align_y.factor(),
        width,
        height,
    )
}

/// `window` less `margin`, which is sane, on every side, the margin cut to
/// half the window on an axis it is wider than half of.
fn margined(window: Rect, margin: f32) -> Rect {
    let x = margin.min(window.width / 2.0);
    let y = margin.min(window.height / 2.0);
    fit(
        Rect::new(
            window.x + x,
            window.y + y,
            window.width - 2.0 * x,
            window.height - 2.0 * y,
        ),
        window,
    )
}

/// Where an overlay of `size`, no larger than `window`, goes on `side` of
/// `anchor`, `gap` away, lined up with it by `align`: flipped to the
/// other side if it fits only there, on the side with more room if it fits
/// on neither, then moved inside `window` on both axes. Before
/// [`fit`]; `gap` is sane.
fn beside(window: Rect, size: Size, anchor: Rect, side: Side, align: Align, gap: f32) -> Rect {
    let vertical = matches!(side, Side::Above | Side::Below);
    // Across the side (the main axis) and along it (the cross axis), as
    // (start, length) of the window, the anchor and the overlay.
    let (main_window, main_anchor, main_len) = if vertical {
        (
            (window.y, window.height),
            (anchor.y, anchor.height),
            size.height,
        )
    } else {
        (
            (window.x, window.width),
            (anchor.x, anchor.width),
            size.width,
        )
    };
    let (cross_window, cross_anchor, cross_len) = if vertical {
        (
            (window.x, window.width),
            (anchor.x, anchor.width),
            size.width,
        )
    } else {
        (
            (window.y, window.height),
            (anchor.y, anchor.height),
            size.height,
        )
    };
    let window_end = main_window.0 + main_window.1;
    let after = main_anchor.0 + main_anchor.1 + gap;
    let before = main_anchor.0 - gap - main_len;
    let room_after = window_end - after;
    let room_before = main_anchor.0 - gap - main_window.0;
    let fits_after = room_after >= main_len;
    let fits_before = room_before >= main_len;
    let wants_after = matches!(side, Side::Below | Side::Right);
    let goes_after = if wants_after {
        fits_after || (!fits_before && room_after >= room_before)
    } else {
        !fits_before && (fits_after || room_after > room_before)
    };
    let main = if goes_after { after } else { before };
    let main = between(main, main_window.0, window_end - main_len);
    let cross = cross_anchor.0 + (cross_anchor.1 - cross_len) * align.factor();
    let cross = between(
        cross,
        cross_window.0,
        cross_window.0 + cross_window.1 - cross_len,
    );
    if vertical {
        Rect::new(cross, main, size.width, size.height)
    } else {
        Rect::new(main, cross, size.width, size.height)
    }
}

/// `rect` less `padding`, which is cut to fit so that the box never
/// inverts, fitted inside `rect`.
fn content_box(rect: Rect, padding: Insets) -> Rect {
    let left = padding.left.min(rect.width);
    let right = padding.right.min(rect.width - left);
    let top = padding.top.min(rect.height);
    let bottom = padding.bottom.min(rect.height - top);
    fit(
        Rect::new(
            rect.x + left,
            rect.y + top,
            rect.width - left - right,
            rect.height - top - bottom,
        ),
        rect,
    )
}

/// `value` clamped to `low` to `high`; `low` wins if it is above `high`.
/// NaN becomes `low`.
fn between(value: f32, low: f32, high: f32) -> f32 {
    value.min(high).max(low)
}

/// `rect` moved and shrunk to lie inside `area`, exactly: `area.x <= x`
/// and `x + width <= area.x + area.width` in `f32`, as [`Rect::right`]
/// computes them, and the same vertically. The size is never negative;
/// NaN becomes the area's origin or 0.
fn fit(rect: Rect, area: Rect) -> Rect {
    let (x, width) = fit_axis(rect.x, rect.width, area.x, area.right());
    let (y, height) = fit_axis(rect.y, rect.height, area.y, area.bottom());
    Rect::new(x, y, width, height)
}

/// [`fit`] on one axis: `start` and `len` inside `low` to `high`.
fn fit_axis(start: f32, len: f32, low: f32, high: f32) -> (f32, f32) {
    let start = if start.is_nan() {
        low
    } else {
        start.max(low).min(high)
    };
    let mut len = if len.is_nan() || len <= 0.0 {
        0.0
    } else {
        len.min(high - start)
    };
    // `high - start` can round up, so that `start + len` rounds past
    // `high`: step the length down until it does not.
    let mut steps = 0;
    while start + len > high && len > 0.0 && steps < FIT_STEPS {
        len = len.next_down();
        steps += 1;
    }
    if start + len > high || len.is_nan() || len < 0.0 {
        len = 0.0;
    }
    (start, len)
}

/// The intersection of two rects; an empty rect (of size 0) where they do
/// not meet.
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check;

    fn inside(rect: Rect, area: Rect) -> bool {
        area.x <= rect.x
            && rect.right() <= area.right()
            && area.y <= rect.y
            && rect.bottom() <= area.bottom()
            && rect.width >= 0.0
            && rect.height >= 0.0
    }

    fn coordinate(rng: &mut Pcg32) -> f32 {
        match rng.range(0..8u32) {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => f32::NEG_INFINITY,
            3 => rng.range(-1e7..1e7f32),
            4 => rng.range(-1.0..1.0f32) * 1e-30,
            5 => (rng.range(-1000..1000) as f32) / 3.0,
            _ => rng.range(-2000.0..2000.0f32),
        }
    }

    #[test]
    fn fit_keeps_rects_inside_exactly() {
        check(
            "fit is exact",
            5000,
            |rng| {
                let area = Rect::new(
                    rng.range(-1e6..1e6f32),
                    rng.range(-1e6..1e6f32),
                    rng.range(0.0..1e6f32) / (1 + rng.range(0..1000u32)) as f32,
                    rng.range(0.0..1e6f32),
                );
                let rect = if rng.range(0..2u32) == 0 {
                    Rect::new(
                        coordinate(rng),
                        coordinate(rng),
                        coordinate(rng),
                        coordinate(rng),
                    )
                } else {
                    // Inside the area and too long, so the length is cut
                    // to `right - x`, which can round up.
                    let x = area.x + area.width * rng.next_f32();
                    let y = area.y + area.height * rng.next_f32();
                    Rect::new(x, y, area.width, area.height)
                };
                (area, rect)
            },
            |&(area, rect)| {
                let fitted = fit(rect, area);
                if inside(fitted, area) {
                    Ok(())
                } else {
                    Err(format!("{fitted:?} is not inside"))
                }
            },
        );
    }

    #[test]
    fn fit_leaves_a_rect_that_fits_as_it_is() {
        let area = Rect::new(10.0, 10.0, 100.0, 50.0);
        let rect = Rect::new(20.0, 15.0, 30.0, 20.0);
        assert_eq!(fit(rect, area), rect);
        assert_eq!(
            fit(Rect::new(0.0, 100.0, 500.0, 5.0), area),
            Rect::new(10.0, 60.0, 100.0, 0.0)
        );
    }

    #[test]
    fn content_boxes_never_invert() {
        let rect = Rect::new(5.0, 5.0, 10.0, 4.0);
        assert_eq!(
            content_box(rect, Insets::new(1.0, 2.0, 1.0, 3.0)),
            Rect::new(8.0, 6.0, 5.0, 2.0)
        );
        // Padding wider than the rect is cut: the left first, then the
        // right.
        assert_eq!(
            content_box(rect, Insets::new(3.0, 8.0, 3.0, 6.0)),
            Rect::new(11.0, 8.0, 0.0, 0.0)
        );
    }

    #[test]
    fn natural_sizes_are_remembered_for_a_few_hints_only() {
        use crate::metrics::FixedMetrics;
        use crate::tree::Node;

        // A chain of 200 columns, each narrower than its parent and as tall
        // as its content, over 100 leaves 5 tall: each leaf is measured
        // with a hint from every column above it.
        let metrics = FixedMetrics::DEFAULT;
        let mut tree = Tree::new();
        let mut parent = tree.root();
        let mut chain = Vec::new();
        for j in 0..200u16 {
            let column = Style::column().width(Length::Fixed(1000.0 - f32::from(j)));
            parent = tree.insert(parent, Node::new(column)).expect("insert");
            chain.push(parent);
        }
        let leaf = Style::default()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(5.0));
        let leaves: Vec<NodeId> = (0..100)
            .map(|_| tree.insert(parent, Node::new(leaf)).expect("insert"))
            .collect();
        let window = Size::new(1000.0, 1000.0);
        for pass in 0..2 {
            tree.invalidate_all();
            tree.layout(window, &metrics);
            for id in tree.ids() {
                let memo = tree.get(id).map(|node| node.memo.len());
                assert!(memo.is_some_and(|len| len <= 4), "{id}: {memo:?}");
            }
            for (j, &id) in chain.iter().enumerate() {
                let width = 1000.0 - j as f32;
                assert_eq!(
                    tree.bounds(id),
                    Some(Rect::new(0.0, 0.0, width, 500.0)),
                    "pass {pass}"
                );
            }
            for (i, &id) in leaves.iter().enumerate() {
                assert_eq!(
                    tree.bounds(id),
                    Some(Rect::new(0.0, 5.0 * i as f32, 801.0, 5.0)),
                    "pass {pass}"
                );
            }
        }
    }

    #[test]
    fn a_memo_forgets_its_oldest_hint_first() {
        let mut memo = Memo::default();
        let hint = |n: u8| Size::new(f32::from(n), 0.0);
        for n in 0..6 {
            memo.insert(hint(n), Size::new(0.0, f32::from(n)));
        }
        assert_eq!(memo.len(), MEMO_ENTRIES);
        assert_eq!(memo.get(hint(0)), None);
        assert_eq!(memo.get(hint(1)), None);
        for n in 2..6 {
            assert_eq!(memo.get(hint(n)), Some(Size::new(0.0, f32::from(n))));
        }
        memo.clear();
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.get(hint(5)), None);
    }

    #[test]
    fn an_overlay_goes_beside_its_anchor_flipped_and_clamped() {
        let window = Rect::new(0.0, 0.0, 400.0, 300.0);
        let size = Size::new(100.0, 40.0);
        let anchor = Rect::new(50.0, 100.0, 60.0, 20.0);
        let at = |anchor, side, align| beside(window, size, anchor, side, align, 4.0);
        // Below, its left edge on the anchor's, 4 away; centered; its right
        // edge on the anchor's.
        assert_eq!(
            at(anchor, Side::Below, Align::Start),
            Rect::new(50.0, 124.0, 100.0, 40.0)
        );
        assert_eq!(
            at(anchor, Side::Below, Align::Center),
            Rect::new(30.0, 124.0, 100.0, 40.0)
        );
        assert_eq!(
            at(anchor, Side::Above, Align::End),
            Rect::new(10.0, 56.0, 100.0, 40.0)
        );
        // Right and left, lined up down the side.
        assert_eq!(
            at(anchor, Side::Right, Align::Start),
            Rect::new(114.0, 100.0, 100.0, 40.0)
        );
        // Left of it there are 46 units, too few: it flips right.
        assert_eq!(
            at(anchor, Side::Left, Align::Center),
            Rect::new(114.0, 90.0, 100.0, 40.0)
        );
        // At the window's bottom it flips above; at its top, below.
        let low = Rect::new(50.0, 270.0, 60.0, 20.0);
        assert_eq!(
            at(low, Side::Below, Align::Start),
            Rect::new(50.0, 226.0, 100.0, 40.0)
        );
        let high = Rect::new(50.0, 10.0, 60.0, 20.0);
        assert_eq!(
            at(high, Side::Above, Align::Start),
            Rect::new(50.0, 34.0, 100.0, 40.0)
        );
        // Lined up past the window's edge, it is moved inside.
        let edge = Rect::new(380.0, 100.0, 20.0, 20.0);
        assert_eq!(
            at(edge, Side::Below, Align::Start),
            Rect::new(300.0, 124.0, 100.0, 40.0)
        );
        // Fitting on neither side, it goes on the side with more room,
        // whichever it asked for, and is moved inside the window, over the
        // anchor: below it there are 156 units and above it 116, then 116
        // and 156.
        let tall = Size::new(100.0, 200.0);
        for (y, placed) in [(120.0, 100.0), (160.0, 0.0)] {
            let middle = Rect::new(50.0, y, 60.0, 20.0);
            for side in [Side::Below, Side::Above] {
                assert_eq!(
                    beside(window, tall, middle, side, Align::Start, 4.0),
                    Rect::new(50.0, placed, 100.0, 200.0),
                    "{side:?} of {middle:?}"
                );
            }
        }
        // An anchor far outside the window, as a scrolled node may be.
        let far = Rect::new(-5e5, 9e5, 10.0, 10.0);
        let placed = beside(window, size, far, Side::Below, Align::Start, 4.0);
        assert_eq!(placed, Rect::new(0.0, 260.0, 100.0, 40.0));
    }

    #[test]
    fn a_margin_is_cut_to_half_the_window() {
        let window = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(margined(window, 10.0), Rect::new(10.0, 10.0, 80.0, 20.0));
        assert_eq!(margined(window, 30.0), Rect::new(30.0, 20.0, 40.0, 0.0));
        assert_eq!(margined(window, 0.0), window);
    }

    #[test]
    fn intersect_is_empty_where_rects_do_not_meet() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            intersect(a, Rect::new(5.0, 5.0, 10.0, 10.0)),
            Rect::new(5.0, 5.0, 5.0, 5.0)
        );
        assert!(intersect(a, Rect::new(20.0, 0.0, 5.0, 5.0)).is_empty());
    }
}
