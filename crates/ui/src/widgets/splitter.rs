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

//! The splitter: a divider between two panes of a [`Layout::Split`],
//! which the pointer drags and the arrow keys step (ADR-0052). jabbott.dev
//! has no splitter style; its looks are derived from the tokens and the
//! scrollbar's.

use std::any::Any;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, MouseButton};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Axis, Layout, Length, Style};
use crate::metrics::TextMetrics;
use crate::theme::Token;
use crate::tree::{Node, NodeId, Tree, Widget};

use super::{
    Control, FOCUS_WIDTH, Look, MAX_TAB_LABEL_BYTES, SPLITTER_STEP, SPLITTER_THICKNESS, cut_to,
    dimmed,
};

/// The width of the rule a divider shows at rest.
const RULE: f32 = 1.0;

/// The width of the bar a divider shows hovered, pressed or dragged.
const BAR: f32 = 2.0;

/// What the last binding saw of the split: where its panes may go.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SplitView {
    /// Where the split's content box starts along its axis, in window
    /// coordinates.
    start: f32,
    /// How long the split's content box is along its axis.
    extent: f32,
    /// The least ratio, which keeps the first pane at its minimum.
    low: f32,
    /// The greatest ratio, which keeps the second pane at its minimum.
    high: f32,
}

/// A splitter (ADR-0052): the divider between the two panes of a
/// [`Layout::Split`], [`SPLITTER_THICKNESS`] across, which gives the first
/// pane its `ratio` of the split's length.
///
/// [`insert`](Self::insert) builds the split: a node of
/// [`Style::split`] holding the first pane, the divider and the second
/// pane, which should fill what is left. The divider keeps the ratio it
/// was given or moved to, which the owner reads with
/// [`ratio`](Self::ratio), and it emits [`ActionKind::Change`] when that
/// changes, for the owner to keep it. The panes keep their minimum
/// lengths along the axis: at each layout, the tree's layout gives the
/// split the ratio kept between the least that gives the first pane its
/// minimum and the most that leaves the second its own, so that the panes
/// move as the divider does. The ratio itself is kept as it was: a split
/// made too small for it, then large again, has it again. Moving it
/// repaints only what moves. A divider binds only as the middle of its
/// split's three children, as `insert` places it: one put anywhere else
/// gives no split a ratio, and moves nothing.
///
/// Dragging it moves it with the pointer. While it has focus, with no
/// modifier, the arrow keys along its axis (Left and Right for a split
/// side by side, Up and Down for one above the other) move it
/// [`SPLITTER_STEP`] units from where it shows, and Home and End to its
/// least and greatest ratio. Before the split is laid out, or while it
/// has no length, neither moves it. A pointer position that is not
/// finite moves nothing.
///
/// It is a splitter in the accessibility tree, named as given, cut to
/// [`MAX_TAB_LABEL_BYTES`], its value the ratio the split shows as a whole
/// percentage, and it takes focus. At rest it shows a `line` rule down
/// its middle; hovered, a `teal-glow` bar, and pressed or dragged, while
/// its node is pressed, a `teal` one; its focus outline is drawn inside
/// its bounds, as it is as thin as an outline; disabled, it has its rest
/// look at [`DISABLED_OPACITY`](super::DISABLED_OPACITY).
///
/// ```
/// use tidepool_core::geom::Size;
/// use tidepool_core::input::{InputEvent, Key, Modifiers};
/// use tidepool_ui::input::ActionKind;
/// use tidepool_ui::layout::{Axis, Style};
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::Splitter;
/// use tidepool_ui::{Node, Tree};
///
/// let mut tree = Tree::new();
/// let (side, editor) = (Node::new(Style::stack().fill()), Node::new(Style::stack().fill()));
/// let root = tree.root();
/// let split = Splitter::new(Axis::Horizontal, "Side panel", 0.25).insert(&mut tree, root, side, editor)?;
/// let window = Size::new(408.0, 300.0);
/// tree.frame(window, &FixedMetrics::DEFAULT);
/// let divider = Splitter::divider(&tree, split).expect("a divider");
///
/// // Right moves it 10 units: from 102 of 408 to 112.
/// tree.set_focus(divider, true)?;
/// let right = InputEvent::KeyDown { key: Key::Right, modifiers: Modifiers::NONE, repeat: false };
/// assert_eq!(tree.handle(&right).actions[0].kind, ActionKind::Change);
/// tree.frame(window, &FixedMetrics::DEFAULT);
/// assert_eq!(tree.semantics(divider).and_then(|s| s.value.clone()).as_deref(), Some("27%"));
/// let first = tree.bounds(tree.children(split).expect("panes")[0]).expect("a pane");
/// assert!((first.width - 112.0).abs() < 0.01, "{first:?}");
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Splitter {
    axis: Axis,
    name: String,
    /// The ratio it was given or moved to, 0 to 1.
    ratio: f32,
    /// The ratio the split has: [`ratio`](Self::ratio) kept within what
    /// the panes' minimums left at the last layout.
    shown: f32,
    /// While it is dragged: where the pointer took it, from its start.
    grab: Option<f32>,
    view: SplitView,
}

impl Splitter {
    /// A splitter on `axis`, the axis its panes are laid out along, named
    /// `name`, cut to [`MAX_TAB_LABEL_BYTES`] at a grapheme cluster
    /// boundary, giving the first pane `ratio` of the split: NaN is 0.5,
    /// and it is kept from 0 to 1.
    pub fn new(axis: Axis, name: impl Into<String>, ratio: f32) -> Self {
        let name: String = name.into();
        let ratio = sane(ratio);
        Self {
            axis,
            name: cut_to(&name, MAX_TAB_LABEL_BYTES).to_string(),
            ratio,
            shown: ratio,
            grab: None,
            view: SplitView {
                low: 0.0,
                high: 1.0,
                ..SplitView::default()
            },
        }
    }

    /// The axis its panes are laid out along.
    pub const fn axis(&self) -> Axis {
        self.axis
    }

    /// The first pane's share of the split it was given or moved to, 0 to
    /// 1, which the owner keeps: the split shows it kept within what the
    /// panes' minimums leave, and the value in the accessibility tree is
    /// what it shows.
    pub const fn ratio(&self) -> f32 {
        self.ratio
    }

    /// Gives the first pane `ratio` of the split, NaN being 0.5, kept
    /// within what the panes' minimums leave at each layout.
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = sane(ratio);
    }

    /// Adds a split as the last child of `parent`, filling it: `first`,
    /// then this divider, then `second`; returns the split's node.
    ///
    /// # Errors
    ///
    /// The errors of [`Tree::insert`], after which the split is removed
    /// again, so the tree is as it was.
    pub fn insert(
        self,
        tree: &mut Tree,
        parent: NodeId,
        first: Node,
        second: Node,
    ) -> Result<NodeId, UiError> {
        let split = tree.insert(
            parent,
            Node::new(Style::split(self.axis, self.ratio).fill()),
        )?;
        let filled = tree
            .insert(split, first)
            .and_then(|_| tree.insert(split, self.node()))
            .and_then(|_| tree.insert(split, second));
        if let Err(error) = filled {
            // Just inserted, and not the root, so it can be removed.
            let _ = tree.remove(split);
            return Err(error);
        }
        Ok(split)
    }

    /// The divider of a split [`insert`](Self::insert) made, if `split`
    /// is one.
    pub fn divider(tree: &Tree, split: NodeId) -> Option<NodeId> {
        tree.children(split)?
            .iter()
            .copied()
            .find(|&child| tree.widget::<Splitter>(child).is_some())
    }

    /// `point` along the axis.
    const fn along(&self, point: Point) -> f32 {
        match self.axis {
            Axis::Horizontal => point.x,
            Axis::Vertical => point.y,
        }
    }

    /// Moves it to `ratio`, kept within the panes' minimums, and tells
    /// the owner if that changed its ratio; does nothing for NaN, or
    /// before the split has a length.
    fn move_to(&mut self, cx: &mut EventContext<'_>, ratio: f32) {
        if ratio.is_nan() || !self.has_length() {
            return;
        }
        let ratio = ratio.clamp(self.view.low, self.view.high);
        if ratio != self.ratio || ratio != self.shown {
            self.ratio = ratio;
            self.shown = ratio;
            cx.emit(ActionKind::Change);
            cx.set_semantics(self.semantics());
            cx.request_layout();
        }
    }

    /// Whether the last layout gave the split a length along its axis.
    fn has_length(&self) -> bool {
        self.view.extent > 0.0 && self.view.extent.is_finite()
    }

    /// The ratio for the first pane `length` long, if the split has a
    /// length.
    fn ratio_for(&self, length: f32) -> f32 {
        if self.has_length() {
            length / self.view.extent
        } else {
            f32::NAN
        }
    }
}

impl Widget for Splitter {
    /// Its look is its node's state: pressed while a press it captured
    /// lasts, wherever the pointer is.
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let look = Look::of(cx.state());
        dimmed(cx, list, |list| {
            let (width, token) = match look {
                Look::Rest => (RULE, Token::Line),
                Look::Hover => (BAR, Token::TealGlow),
                Look::Pressed => (BAR, Token::Teal),
            };
            let rect = match self.axis {
                Axis::Horizontal => Rect::new(
                    bounds.x + (bounds.width - width) / 2.0,
                    bounds.y,
                    width,
                    bounds.height,
                ),
                Axis::Vertical => Rect::new(
                    bounds.x,
                    bounds.y + (bounds.height - width) / 2.0,
                    bounds.width,
                    width,
                ),
            };
            list.rect(rect, theme.color(token));
        });
        if cx.state().focus_visible {
            list.border(bounds, FOCUS_WIDTH, theme.color(Token::Teal));
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => {
                if !finite(position) {
                    return Handled::No;
                }
                let bounds = cx.bounds();
                self.grab = Some(self.along(position) - self.along(Point::new(bounds.x, bounds.y)));
                Handled::Yes
            }
            InputEvent::MouseMove { position, .. } => match self.grab {
                Some(grab) if cx.state().pressed => {
                    if finite(position) {
                        let length = self.along(position) - grab - self.view.start;
                        let ratio = self.ratio_for(length);
                        self.move_to(cx, ratio);
                    }
                    Handled::Yes
                }
                // The press ended without its release reaching it.
                _ => {
                    self.grab = None;
                    Handled::No
                }
            },
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.grab = None;
                Handled::Yes
            }
            InputEvent::KeyDown { key, modifiers, .. }
                if cx.state().focused && modifiers.is_empty() =>
            {
                let (back, on) = match self.axis {
                    Axis::Horizontal => (Key::Left, Key::Right),
                    Axis::Vertical => (Key::Up, Key::Down),
                };
                // From where it shows; with no length, nothing moves.
                let step = self.ratio_for(SPLITTER_STEP);
                let to = if key == back {
                    self.shown - step
                } else if key == on {
                    self.shown + step
                } else if key == Key::Home {
                    self.view.low
                } else if key == Key::End {
                    self.view.high
                } else {
                    return Handled::No;
                };
                self.move_to(cx, to);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn measure(&self, _available: Size, _metrics: &dyn TextMetrics) -> Size {
        Size::new(SPLITTER_THICKNESS, SPLITTER_THICKNESS)
    }
}

impl Control for Splitter {
    fn semantics(&self) -> Semantics {
        // From 0 to 100, so the cast is exact after rounding.
        let percent = (self.shown * 100.0).round().clamp(0.0, 100.0) as u32;
        Semantics::new(Role::Splitter, self.name.clone()).with_value(format!("{percent}%"))
    }

    /// [`SPLITTER_THICKNESS`] along its axis, and filling the split
    /// across it.
    fn style(&self) -> Style {
        match self.axis {
            Axis::Horizontal => Style::stack()
                .width(Length::Fixed(SPLITTER_THICKNESS))
                .height(Length::Fill(1.0)),
            Axis::Vertical => Style::stack()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(SPLITTER_THICKNESS)),
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Tree {
    /// Gives a splitter's split its ratio, kept within its panes'
    /// minimums in the space the last pass gave the split, and keeps where
    /// the split is for a drag to measure from; returns whether the split's
    /// ratio changed, so that layout must pass again. The splitter's own
    /// ratio is left as it is, for a later layout with more room.
    pub(crate) fn bind_splitter(&mut self, id: NodeId) -> bool {
        let Some(split) = self.parent(id) else {
            return false;
        };
        let Some(&style) = self.style(split) else {
            return false;
        };
        let Layout::Split { axis, ratio } = style.layout else {
            return false;
        };
        let Some(children) = self.children(split).map(<[_]>::to_vec) else {
            return false;
        };
        // Only the divider between the two panes, as `insert` builds it:
        // another splitter in the split would give it a ratio of its own,
        // and one elsewhere read the minimums of what are not its panes.
        if children.get(1) != Some(&id) {
            return false;
        }
        let along = |size: Size| match axis {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        };
        let minimum = |tree: &Self, node: Option<&NodeId>| {
            node.and_then(|&node| tree.style(node))
                .map_or(0.0, |style| along(style.min).max(0.0))
        };
        let (first, second) = (
            minimum(self, children.first()),
            minimum(self, children.get(2)),
        );
        let Some(placement) = self.get(split).map(|node| node.placement) else {
            return false;
        };
        let content = placement.viewport;
        let (start, extent) = match axis {
            Axis::Horizontal => (content.x, content.width),
            Axis::Vertical => (content.y, content.height),
        };
        let (low, high) = limits(extent, first, second);
        let Some(divider) = self.splitter_mut(id) else {
            return false;
        };
        divider.view = SplitView {
            start,
            extent,
            low,
            high,
        };
        divider.shown = sane(divider.ratio).clamp(low, high);
        let wanted = divider.shown;
        let semantics = divider.semantics();
        if self.semantics(id) != Some(&semantics) {
            // Named at the insert, so not refused.
            let _ = self.set_semantics(id, semantics);
        }
        if wanted == ratio {
            return false;
        }
        let style = Style {
            layout: Layout::Split {
                axis,
                ratio: wanted,
            },
            ..style
        };
        self.set_split_style(split, style);
        true
    }

    /// Gives `split` `style`, its own but for its split's ratio: layout
    /// runs again, and its pass marks only the nodes it moves, where
    /// [`Tree::set_style`] repaints all that the split holds.
    fn set_split_style(&mut self, split: NodeId, style: Style) {
        if let Some(node) = self.get_mut(split) {
            node.style = style;
            node.sane = style.sanitized();
            self.layout_dirty = true;
        }
    }

    /// The splitter's widget, to change, without marking its node.
    fn splitter_mut(&mut self, id: NodeId) -> Option<&mut Splitter> {
        let widget: &mut dyn Widget = self.get_mut(id)?.widget.as_deref_mut()?;
        (widget as &mut dyn Any).downcast_mut()
    }
}

/// The least and greatest ratio of a split `extent` long whose first pane
/// is at least `first` long and second at least `second`: from 0 to 1, the
/// least no greater than the greatest, and 0 and 1 for a split with no
/// length.
fn limits(extent: f32, first: f32, second: f32) -> (f32, f32) {
    if !(extent > 0.0 && extent.is_finite()) {
        return (0.0, 1.0);
    }
    let low = sane(first / extent);
    let high = sane((extent - SPLITTER_THICKNESS - second) / extent);
    (low, high.max(low))
}

/// Whether `point` is finite on both axes.
const fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// `ratio` made sane: NaN is 0.5, and it is kept from 0 to 1.
fn sane(ratio: f32) -> f32 {
    if ratio.is_nan() {
        0.5
    } else {
        ratio.clamp(0.0, 1.0)
    }
}
