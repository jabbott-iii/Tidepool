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

//! Layout in logical units: a node's [`Style`] (its [`Layout`] of
//! children, its [`Length`]s, padding, gap, alignment, scrolling, clipping,
//! opacity and visibility), and the [`Tree`] methods that lay the tree out
//! and scroll it.
//!
//! New in Tidepool; not ported.
//!
//! # Sanitizing
//!
//! Any style is accepted. Before layout uses one, it is made sane, so
//! layout never panics and every rect it produces is finite with a size
//! that is not negative:
//!
//! - Fixed lengths, padding, the gap and the minimum size: NaN or negative
//!   is 0, and above [`MAX_EXTENT`] is [`MAX_EXTENT`]. The maximum size:
//!   NaN is [`MAX_EXTENT`], and the same rules otherwise. A minimum above
//!   the maximum wins.
//! - A [`Length::Fill`] weight: NaN or negative is 0, and above
//!   [`MAX_EXTENT`] (infinity too) is [`MAX_EXTENT`].
//! - A [`Layout::Split`] ratio: NaN is 0.5, and it is clamped to 0 to 1.
//!   The opacity: NaN is 1, and it is clamped to 0 to 1. A
//!   [`Layout::Grid`] of 0 columns has 1.
//! - The window size given to [`Tree::layout`] follows the rules of fixed
//!   lengths, and so does the size a widget measures.
//!
//! # Lengths
//!
//! [`Length::Fixed`] is that many logical units; [`Length::Content`] the
//! node's natural size; [`Length::Fill`] a share, by weight, of the free
//! space on the main axis of a [`Layout::Row`], [`Layout::Column`] or
//! [`Layout::Split`], and the whole extent on the cross axis, in a
//! [`Layout::Stack`] and in a [`Layout::Grid`]'s cell. The root always fills
//! the window, whatever its own lengths, and even when it is hidden. Every
//! size is then clamped by the node's minimum and maximum.
//!
//! # Natural size
//!
//! A node's natural size is its border box at its lengths, where
//! [`Length::Content`] and [`Length::Fill`] take its content: the larger,
//! on each axis, of what its widget measures and what its visible children
//! take at their natural sizes (a column: the widest, and the sum of the
//! heights and the gaps; a row: the transpose; a stack: the widest and the
//! tallest; a grid: the widest child times its columns, with the gaps
//! between them, and the tallest child of each row), plus its padding.
//! Natural sizes are remembered for the rest of a layout pass, for the last
//! four hints each node was measured with, so measuring takes time about
//! linear in the nodes times the depth, never exponential, and memory
//! linear in the nodes.
//!
//! # Arranging
//!
//! A node's children are laid out inside its content box: its bounds less
//! its padding, which never inverts (padding wider than the bounds is cut
//! to fit). On an axis it scrolls, the children are laid out in an area as
//! large as their natural size, or the content box if that is larger, moved
//! by the scroll offset, which is clamped to that area. Rows and columns
//! give each child its fixed or natural size on the main axis, shrink them
//! all in proportion if they do not fit, or else share the free space among
//! the fills by weight, and place them by the node's alignment. Sharing
//! follows CSS flexbox: each round gives every fill not yet frozen its
//! share, clamped by its limits; if the clamping added space in all, the
//! fills held up at their minimums are frozen there, if it took space away,
//! those held down at their maximums, and if neither, every fill keeps its
//! clamped share. What the frozen ones take leaves the space for the next
//! round. Fills whose weights are all 0 take their minimums. The fills are
//! shrunk, in proportion, only if their minimums do not fit. A gap too
//! large for the space is shrunk to fit. A split gives its first child the
//! ratio of the space and lays out the rest as a row or column. A grid has
//! columns of equal width and rows as tall as their tallest child, shrunk
//! to fit unless the node scrolls vertically.
//!
//! **Children never overflow a non-scrolling parent, exactly:** every
//! child's rect is fitted inside the area, so that `area.x <= child.x` and
//! `child.right() <= area.right()` hold in `f32` as [`Rect::right`]
//! computes them (and the same vertically), and the area is the content
//! box on every axis that does not scroll. Hidden nodes, and every node
//! below one, get an empty rect at the area's origin; a hidden root keeps
//! the window, and its children are laid out as hidden ones.
//!
//! # Scrolling into view
//!
//! [`Tree::scroll_into_view`] takes effect at the next layout: once the
//! pass has placed every node, and the collections, tab strips and
//! splitters have settled (or at once from the last pass's placements,
//! if nothing changed layout since), each scrolling ancestor of the node,
//! the nearest first, moves its offset, as that pass clamped it, the least
//! that brings the node into its content box, and if any moved, the pass
//! runs once more so that every rect shows the new offsets. So several
//! focus moves, scrolls (by the wheel or [`Tree::set_scroll_offset`]) or
//! changes to the tree between two layouts reveal the last node asked for
//! where it then is, from the offsets they left, and a request that
//! scrolls nothing costs no pass. The rows collections ask for at that
//! layout are revealed so before it, each in turn, with a pass after each
//! that moved an offset, so that the owner's request wins where they
//! share a scroller.
//!
//! # Overlays
//!
//! Each pass lays out the main root's layer, then each overlay's in the
//! order they were opened, so an overlay anchored to a node sees where
//! that node is in the same pass: an overlay root's size comes from its
//! style as a stack's child's does, and its anchor places it inside the
//! window (see [`overlay`](crate::overlay#anchors-and-placement)).
//!
//! Lengths and sizes are at most [`MAX_EXTENT`]. A position is finite but
//! may lie further from the origin, inside scrolled areas.

mod arrange;

use tidepool_core::geom::{Point, Rect, Size};

use crate::error::UiError;
use crate::metrics::TextMetrics;
use crate::tree::{NodeId, Tree};

pub(crate) use arrange::{Memo, Placement, intersect};

/// The largest length or size layout uses, in logical units; larger ones
/// are clamped to it (see the [module documentation](self)).
pub const MAX_EXTENT: f32 = 1_000_000.0;

/// How long a node is on one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// That many logical units.
    Fixed(f32),
    /// A share, by this weight, of the free space on the parent's main
    /// axis; the whole extent on a cross axis, in a stack and in a grid's
    /// cell.
    Fill(f32),
    /// The node's natural size: what its widget and children need.
    #[default]
    Content,
}

/// A direction of layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// How a node lays out its children.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Layout {
    /// One above another.
    #[default]
    Column,
    /// One beside another.
    Row,
    /// On top of each other, each placed by the node's alignment.
    Stack,
    /// In rows of this many columns of equal width.
    Grid {
        /// The number of columns; 0 counts as 1.
        columns: u16,
    },
    /// The first child takes `ratio` of the space on `axis`, and the rest
    /// are laid out as a row or column in what is left: a splitter.
    Split {
        /// The main axis.
        axis: Axis,
        /// The first child's share of the space, 0 to 1.
        ratio: f32,
    },
}

/// Where children go on an axis when they leave space free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// At the start: the left or the top.
    #[default]
    Start,
    /// In the middle.
    Center,
    /// At the end: the right or the bottom.
    End,
}

impl Align {
    /// The fraction of the free space before the children.
    const fn factor(self) -> f32 {
        match self {
            Self::Start => 0.0,
            Self::Center => 0.5,
            Self::End => 1.0,
        }
    }
}

/// Space inside a node's edges, in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    /// Above.
    pub top: f32,
    /// On the right.
    pub right: f32,
    /// Below.
    pub bottom: f32,
    /// On the left.
    pub left: f32,
}

impl Insets {
    /// None.
    pub const ZERO: Self = Self::all(0.0);

    /// The same on every side.
    pub const fn all(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    /// `horizontal` on the left and right, `vertical` above and below.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(vertical, horizontal, vertical, horizontal)
    }

    /// Each side, in CSS's order: top, right, bottom, left.
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

/// How a node is laid out, and how it treats its children.
///
/// The default is a [`Layout::Column`] of [`Length::Content`] on both
/// axes, with no minimum, a maximum of [`MAX_EXTENT`], no padding or gap,
/// aligned at the start, not scrolling or clipping, opaque and shown. The
/// builders change one thing each:
///
/// ```
/// use tidepool_ui::layout::{Align, Insets, Length, Style};
///
/// let toolbar = Style::row()
///     .width(Length::Fill(1.0))
///     .height(Length::Fixed(30.0))
///     .padding(Insets::symmetric(8.0, 0.0))
///     .gap(4.0)
///     .align(Align::Start, Align::Center);
/// assert_eq!(toolbar.gap, 4.0);
/// ```
///
/// Any values are accepted; layout sanitizes them as the
/// [module documentation](self) describes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// How the children are laid out.
    pub layout: Layout,
    /// The width.
    pub width: Length,
    /// The height.
    pub height: Length,
    /// The smallest size.
    pub min: Size,
    /// The largest size.
    pub max: Size,
    /// Space between the bounds and the children.
    pub padding: Insets,
    /// Space between children on the main axis, and on both axes in a grid.
    pub gap: f32,
    /// Where children go horizontally when they leave space free.
    pub align_x: Align,
    /// Where children go vertically when they leave space free.
    pub align_y: Align,
    /// Whether the children scroll horizontally.
    pub scroll_x: bool,
    /// Whether the children scroll vertically.
    pub scroll_y: bool,
    /// Whether the children (not the node's own paint) are clipped to its
    /// bounds. A node that scrolls clips too.
    pub clip: bool,
    /// The opacity of the node and its subtree, 0 to 1. Below 1, the
    /// subtree is drawn as a layer.
    pub opacity: f32,
    /// Whether the node is hidden: it takes no space, and it and its
    /// subtree are not painted, hit, focused or in the accessibility tree.
    pub hidden: bool,
}

/// [`Style`]'s default, for its `const` builders.
const BASE: Style = Style {
    layout: Layout::Column,
    width: Length::Content,
    height: Length::Content,
    min: Size::ZERO,
    max: Size::new(MAX_EXTENT, MAX_EXTENT),
    padding: Insets::ZERO,
    gap: 0.0,
    align_x: Align::Start,
    align_y: Align::Start,
    scroll_x: false,
    scroll_y: false,
    clip: false,
    opacity: 1.0,
    hidden: false,
};

impl Default for Style {
    fn default() -> Self {
        BASE
    }
}

impl Style {
    /// The default style, laying its children out in a row.
    pub const fn row() -> Self {
        Self {
            layout: Layout::Row,
            ..BASE
        }
    }

    /// The default style, laying its children out in a column.
    pub const fn column() -> Self {
        BASE
    }

    /// The default style, stacking its children.
    pub const fn stack() -> Self {
        Self {
            layout: Layout::Stack,
            ..BASE
        }
    }

    /// The default style, laying its children out in a grid of `columns`.
    pub const fn grid(columns: u16) -> Self {
        Self {
            layout: Layout::Grid { columns },
            ..BASE
        }
    }

    /// The default style, splitting its space on `axis` at `ratio`.
    pub const fn split(axis: Axis, ratio: f32) -> Self {
        Self {
            layout: Layout::Split { axis, ratio },
            ..BASE
        }
    }

    /// The same, `width` wide.
    pub const fn width(self, width: Length) -> Self {
        Self { width, ..self }
    }

    /// The same, `height` high.
    pub const fn height(self, height: Length) -> Self {
        Self { height, ..self }
    }

    /// The same, filling its parent on both axes (`Fill(1.0)`).
    pub const fn fill(self) -> Self {
        Self {
            width: Length::Fill(1.0),
            height: Length::Fill(1.0),
            ..self
        }
    }

    /// The same, `width` by `height` logical units.
    pub const fn fixed(self, width: f32, height: f32) -> Self {
        Self {
            width: Length::Fixed(width),
            height: Length::Fixed(height),
            ..self
        }
    }

    /// The same, at least `min`.
    pub const fn min(self, min: Size) -> Self {
        Self { min, ..self }
    }

    /// The same, at most `max`.
    pub const fn max(self, max: Size) -> Self {
        Self { max, ..self }
    }

    /// The same, with `padding`.
    pub const fn padding(self, padding: Insets) -> Self {
        Self { padding, ..self }
    }

    /// The same, with `gap` between children.
    pub const fn gap(self, gap: f32) -> Self {
        Self { gap, ..self }
    }

    /// The same, aligning its children at `x` and `y`.
    pub const fn align(self, x: Align, y: Align) -> Self {
        Self {
            align_x: x,
            align_y: y,
            ..self
        }
    }

    /// The same, scrolling its children horizontally if `x` and vertically
    /// if `y`.
    pub const fn scroll(self, x: bool, y: bool) -> Self {
        Self {
            scroll_x: x,
            scroll_y: y,
            ..self
        }
    }

    /// The same, clipping its children to its bounds.
    pub const fn clip(self) -> Self {
        Self { clip: true, ..self }
    }

    /// The same, at `opacity`.
    pub const fn opacity(self, opacity: f32) -> Self {
        Self { opacity, ..self }
    }

    /// The same, hidden.
    pub const fn hidden(self) -> Self {
        Self {
            hidden: true,
            ..self
        }
    }

    /// Whether the children are clipped to the node's bounds: it clips or
    /// scrolls.
    pub(crate) const fn clips(&self) -> bool {
        self.clip || self.scroll_x || self.scroll_y
    }

    /// The style made sane, as the [module documentation](self) describes.
    pub(crate) fn sanitized(&self) -> Self {
        let min = Size::new(length(self.min.width), length(self.min.height));
        let max = Size::new(
            upper(self.max.width).max(min.width),
            upper(self.max.height).max(min.height),
        );
        let layout = match self.layout {
            Layout::Grid { columns } => Layout::Grid {
                columns: columns.max(1),
            },
            Layout::Split { axis, ratio } => Layout::Split {
                axis,
                ratio: if ratio.is_nan() {
                    0.5
                } else {
                    ratio.clamp(0.0, 1.0)
                },
            },
            other => other,
        };
        let padding = self.padding;
        Self {
            layout,
            width: sane_length(self.width),
            height: sane_length(self.height),
            min,
            max,
            padding: Insets::new(
                length(padding.top),
                length(padding.right),
                length(padding.bottom),
                length(padding.left),
            ),
            gap: length(self.gap),
            opacity: if self.opacity.is_nan() {
                1.0
            } else {
                self.opacity.clamp(0.0, 1.0)
            },
            ..*self
        }
    }
}

/// A length made sane: NaN or negative is 0, above [`MAX_EXTENT`] is
/// [`MAX_EXTENT`].
pub(crate) fn length(value: f32) -> f32 {
    if value.is_nan() || value <= 0.0 {
        0.0
    } else {
        value.min(MAX_EXTENT)
    }
}

/// A maximum made sane: NaN is [`MAX_EXTENT`], otherwise as [`length`].
fn upper(value: f32) -> f32 {
    if value.is_nan() {
        MAX_EXTENT
    } else {
        length(value)
    }
}

/// A [`Length`] made sane.
fn sane_length(value: Length) -> Length {
    match value {
        Length::Fixed(v) => Length::Fixed(length(v)),
        // A weight follows the rules of a length; infinity is above the
        // maximum too.
        Length::Fill(weight) => Length::Fill(length(weight)),
        Length::Content => Length::Content,
    }
}

/// The most times a layout binds the bound widgets after a pass and
/// passes again, each time that changed anything: a split takes one for
/// each split it is nested in, and the frames after settle what is nested
/// deeper.
const SETTLE_PASSES: usize = 8;

/// The most times a layout puts the toasts in their order and binds again
/// for what that moved under the pointer.
const ORDER_PASSES: usize = 4;

/// A size made sane, as a [`length`] on each axis.
pub(crate) fn sane_size(size: Size) -> Size {
    Size::new(length(size.width), length(size.height))
}

impl Tree {
    /// Lays out the whole tree in a window of `size`, measuring text with
    /// `metrics`, if anything changed layout since the last time or the
    /// size is different; otherwise does nothing but a pending reveal.
    ///
    /// Every node whose bounds, clip, content size or scroll offset
    /// changed is marked to be repainted by the next
    /// [`frame`](Tree::frame), which calls this itself. Each pass lays out
    /// the overlays after the main root's layer (see
    /// [Overlays](self#overlays)). A node asked for
    /// with [`scroll_into_view`](Tree::scroll_into_view) since the last
    /// layout is scrolled into view, passing again only if an offset
    /// moved, and after any pass the hovered path is found again at the
    /// pointer's last position.
    ///
    /// Each [`Collection`](crate::widgets::Collection) binds its row
    /// nodes to the rows of its model on screen first, from the last
    /// layout's placements, and again after each pass, in the space that
    /// pass gave it and the part of it on screen; it passes once more if
    /// that added, removed or moved rows (ADR-0052). A tab strip scrolls
    /// its own track, and a splitter gives its split its ratio kept within
    /// the space the pass gave it. Binding and passing again goes on while
    /// that changes anything, at most eight times, so that splits nested
    /// that deep settle in one layout, and those nested deeper over the
    /// next; a collection with a selection to scroll into view keeps its
    /// rows as they are meanwhile. Once they settle, each such collection
    /// scrolls its selection into view, within itself, in the space it now
    /// has, and asks for its row to be scrolled into view by the scrollers
    /// around it, which this layout does after one more pass, before the
    /// node the owner asked for, if that is outside it, and in place of it
    /// otherwise; then the binding settles again. A collection's nodes
    /// take no more than it measures, so these passes change no
    /// collection's space.
    pub fn layout(&mut self, size: Size, metrics: &dyn TextMetrics) {
        let size = sane_size(size);
        let scale = metrics.scale();
        self.scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let bound = self.bind_widgets(false, false, metrics);
        let stale = bound || self.layout_dirty || self.layout_size != Some(size);
        if stale {
            self.run_layout(size, metrics);
            self.settle_bound(size, metrics);
        }
        // Without a pass, the last one's placements are fresh: nothing
        // changed layout since.
        let moved = self.reveal_pending(size, metrics);
        // A reveal moves what is on screen of the collections, and one a
        // collection asked for that moved nothing leaves its rows kept
        // where the selection is, not where the screen shows.
        if stale || moved {
            self.settle_bound(size, metrics);
        }
        if !stale && !moved {
            return;
        }
        // The toasts' places in the order follow from the placements just
        // made. Moving one changes what is under the pointer, and so
        // whether a toast is held, which places them in the column: bind
        // again while that changes anything, a few times at most.
        for pass in 1..=ORDER_PASSES {
            if !self.order_toasts() {
                break;
            }
            self.settle_layers();
            // The last time, no pass after it: the order then is the one
            // the placements made give, whatever the next layout finds.
            if pass < ORDER_PASSES && self.bind_widgets(false, false, metrics) {
                self.run_layout(size, metrics);
                self.settle_bound(size, metrics);
            }
        }
        self.layout_size = Some(size);
        self.layout_dirty = false;
        self.refresh_hover();
    }

    /// Binds the bound widgets after a pass, and passes again, while that
    /// changes anything, at most [`SETTLE_PASSES`] times, collections
    /// leaving a selection to scroll into view until then; then binds once
    /// more, the collections taking those, and passes if that changed
    /// anything. So a collection scrolls its selection in the space the
    /// settled layout gives it, and the row it asks for keeps its node
    /// until the reveal, as no binding comes between them.
    fn settle_bound(&mut self, size: Size, metrics: &dyn TextMetrics) {
        for _ in 0..SETTLE_PASSES {
            if !self.bind_widgets(true, false, metrics) {
                break;
            }
            self.run_layout(size, metrics);
        }
        if self.bind_widgets(true, true, metrics) {
            self.run_layout(size, metrics);
        }
    }

    /// The node's bounds in the last layout, in window coordinates; an
    /// empty rect at the origin before the first. `None` for an unknown
    /// node.
    pub fn bounds(&self, id: NodeId) -> Option<Rect> {
        self.get(id).map(|node| node.placement.rect)
    }

    /// The size of the area the node's children were laid out in: its
    /// content box, or larger on an axis it scrolls. `None` for an unknown
    /// node.
    pub fn content_size(&self, id: NodeId) -> Option<Size> {
        self.get(id).map(|node| node.placement.content_size)
    }

    /// How far the node's children are scrolled: 0 to the content size
    /// less the content box on each axis it scrolls, and 0 on the others,
    /// as of the last layout (or as set, or scrolled by the wheel, since).
    /// A hidden node, or one below
    /// it, keeps the offset it had, so that a panel shown again is where it
    /// was; the layout after it is shown clamps it. `None` for an unknown
    /// node.
    pub fn scroll_offset(&self, id: NodeId) -> Option<Point> {
        self.get(id).map(|node| node.scroll)
    }

    /// Scrolls the node's children to `offset`. A component that is NaN or
    /// negative is 0, and one above [`MAX_EXTENT`] is [`MAX_EXTENT`]; the
    /// next layout clamps it to what the node can scroll.
    ///
    /// A [`scroll_into_view`](Tree::scroll_into_view) of a node below it
    /// that is pending still takes effect at that layout, moving from this
    /// offset, clamped (see the [module documentation](self)).
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree.
    pub fn set_scroll_offset(&mut self, id: NodeId, offset: Point) -> Result<(), UiError> {
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        node.scroll = Point::new(length(offset.x), length(offset.y));
        self.layout_dirty = true;
        self.mark(id);
        Ok(())
    }

    /// Scrolls each scrolling ancestor of the node, the nearest first, the
    /// least that brings the node's bounds into that ancestor's content
    /// box. On an axis where the node is larger than the content box, its
    /// start is brought to the box's start.
    ///
    /// It takes effect at the next [`layout`](Tree::layout), from that
    /// layout's bounds and its clamped offsets, and the latest request
    /// before it wins (see the [module documentation](self)). If the node
    /// is removed or hidden by then, nothing scrolls: a hidden scroller
    /// keeps its offset. If the wheel or
    /// [`set_scroll_offset`](Tree::set_scroll_offset) scrolls one of its
    /// scrolling ancestors after this and before the layout, the reveal
    /// moves from the offset that scroll left.
    ///
    /// It does not by itself make the next layout lay the tree out again:
    /// if nothing else changed layout, that layout reveals the node from
    /// the last one's bounds, and passes again only if an offset moved.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree.
    pub fn scroll_into_view(&mut self, id: NodeId) -> Result<(), UiError> {
        if !self.contains(id) {
            return Err(UiError::UnknownNode { id });
        }
        self.reveal = Some(id);
        Ok(())
    }

    /// Scrolls the rows collections asked for into view, each in turn,
    /// then the node a [`scroll_into_view`](Tree::scroll_into_view) asked
    /// for, from the last pass's placements, which must be fresh, if each
    /// is still in the tree and shown, and forgets the requests; each that
    /// moved an offset runs the pass again, so the next one is revealed
    /// from fresh placements. Returns whether any did.
    fn reveal_pending(&mut self, size: Size, metrics: &dyn TextMetrics) -> bool {
        let mut asked = std::mem::take(&mut self.reveal_rows);
        asked.extend(self.reveal.take());
        let mut moved = false;
        for id in asked {
            if self.reveal_node(id) {
                self.run_layout(size, metrics);
                moved = true;
            }
        }
        moved
    }

    /// Scrolls `id` into view, from the last pass's placements, if it is
    /// still in the tree and shown; returns whether an offset moved.
    fn reveal_node(&mut self, id: NodeId) -> bool {
        if !self.is_visible(id) {
            return false;
        }
        let Some(node) = self.get(id) else {
            return false;
        };
        let mut target = node.placement.rect;
        let mut next = node.parent;
        let mut any = false;
        while let Some(ancestor) = next {
            let Some(node) = self.get(ancestor) else {
                break;
            };
            next = node.parent;
            let style = node.sane;
            if !style.scroll_x && !style.scroll_y {
                continue;
            }
            let view = node.placement.viewport;
            let extent = node.placement.content_size;
            // The pass stored it clamped.
            let offset = node.scroll;
            let mut moved = offset;
            if style.scroll_x {
                moved.x = reveal(
                    offset.x,
                    (target.x, target.width),
                    (view.x, view.width),
                    extent.width - view.width,
                );
            }
            if style.scroll_y {
                moved.y = reveal(
                    offset.y,
                    (target.y, target.height),
                    (view.y, view.height),
                    extent.height - view.height,
                );
            }
            if moved != offset {
                target.x -= moved.x - offset.x;
                target.y -= moved.y - offset.y;
                if let Some(node) = self.get_mut(ancestor) {
                    node.scroll = moved;
                }
                self.mark(ancestor);
                any = true;
            }
        }
        any
    }

    /// Lays out and repaints everything at the next frame, which damages
    /// the whole window: for a change of scale, font or theme, which no
    /// node's data shows.
    pub fn invalidate_all(&mut self) {
        self.layout_dirty = true;
        self.repaint_all = true;
    }
}

/// The scroll offset on one axis that brings `target` (start and length)
/// into `view`, moving `offset` the least, clamped to 0 to `max`.
pub(crate) fn reveal(offset: f32, target: (f32, f32), view: (f32, f32), max: f32) -> f32 {
    let (start, len) = target;
    let (view_start, view_len) = view;
    let delta = if len > view_len || start < view_start {
        start - view_start
    } else if start + len > view_start + view_len {
        (start + len) - (view_start + view_len)
    } else {
        0.0
    };
    (offset + delta).min(max.max(0.0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_makes_every_value_sane() {
        let wild = Style {
            layout: Layout::Split {
                axis: Axis::Vertical,
                ratio: f32::NAN,
            },
            width: Length::Fixed(f32::NAN),
            height: Length::Fill(f32::INFINITY),
            min: Size::new(-3.0, 2e9),
            max: Size::new(f32::NAN, 5.0),
            padding: Insets::new(f32::NAN, -1.0, f32::INFINITY, 4.0),
            gap: f32::NEG_INFINITY,
            opacity: f32::NAN,
            ..Style::default()
        };
        let sane = wild.sanitized();
        assert_eq!(
            sane.layout,
            Layout::Split {
                axis: Axis::Vertical,
                ratio: 0.5
            }
        );
        assert_eq!(sane.width, Length::Fixed(0.0));
        assert_eq!(sane.height, Length::Fill(MAX_EXTENT));
        assert_eq!(sane.min, Size::new(0.0, MAX_EXTENT));
        // A minimum above the maximum wins.
        assert_eq!(sane.max, Size::new(MAX_EXTENT, MAX_EXTENT));
        assert_eq!(sane.padding, Insets::new(0.0, 0.0, MAX_EXTENT, 4.0));
        assert_eq!(sane.gap, 0.0);
        assert_eq!(sane.opacity, 1.0);

        let more = Style::grid(0)
            .width(Length::Fill(-2.0))
            .height(Length::Fixed(f32::INFINITY))
            .max(Size::new(-1.0, 3.0))
            .opacity(7.0)
            .sanitized();
        assert_eq!(more.layout, Layout::Grid { columns: 1 });
        assert_eq!(more.width, Length::Fill(0.0));
        assert_eq!(more.height, Length::Fixed(MAX_EXTENT));
        assert_eq!(more.max, Size::new(0.0, 3.0));
        assert_eq!(more.opacity, 1.0);
        assert_eq!(
            Style::split(Axis::Horizontal, -4.0).sanitized().layout,
            Layout::Split {
                axis: Axis::Horizontal,
                ratio: 0.0
            }
        );
        assert_eq!(Style::default().opacity(-0.5).sanitized().opacity, 0.0);
        // A sane style is its own sanitized style.
        let sane_style = Style::row().fixed(10.0, 20.0).gap(3.0);
        assert_eq!(sane_style.sanitized(), sane_style);
        assert_eq!(
            sane_size(Size::new(f32::NAN, f32::INFINITY)),
            Size::new(0.0, MAX_EXTENT)
        );
    }

    #[test]
    fn builders_set_one_field_each() {
        assert_eq!(Style::default(), Style::column());
        assert_eq!(Style::row().layout, Layout::Row);
        assert_eq!(Style::stack().layout, Layout::Stack);
        let style = Style::row()
            .fill()
            .min(Size::new(1.0, 2.0))
            .max(Size::new(3.0, 4.0))
            .padding(Insets::all(5.0))
            .gap(6.0)
            .align(Align::Center, Align::End)
            .scroll(true, false)
            .clip()
            .opacity(0.5)
            .hidden();
        assert_eq!(
            (style.width, style.height),
            (Length::Fill(1.0), Length::Fill(1.0))
        );
        assert_eq!(
            (style.min, style.max),
            (Size::new(1.0, 2.0), Size::new(3.0, 4.0))
        );
        assert_eq!(style.padding, Insets::new(5.0, 5.0, 5.0, 5.0));
        assert_eq!((style.align_x, style.align_y), (Align::Center, Align::End));
        assert!(style.scroll_x && !style.scroll_y && style.clip && style.hidden);
        assert_eq!((style.gap, style.opacity), (6.0, 0.5));
        assert_eq!(Insets::symmetric(1.0, 2.0), Insets::new(2.0, 1.0, 2.0, 1.0));
    }

    #[test]
    fn reveal_moves_the_least() {
        // A view of 0..100 over 300 of content.
        let view = (0.0, 100.0);
        assert_eq!(reveal(0.0, (20.0, 10.0), view, 200.0), 0.0);
        assert_eq!(reveal(0.0, (150.0, 10.0), view, 200.0), 60.0);
        assert_eq!(reveal(50.0, (-30.0, 10.0), view, 200.0), 20.0);
        // Larger than the view: its start goes to the view's start.
        assert_eq!(reveal(0.0, (120.0, 150.0), view, 200.0), 120.0);
        // Clamped to what can scroll.
        assert_eq!(reveal(0.0, (500.0, 10.0), view, 200.0), 200.0);
    }
}
