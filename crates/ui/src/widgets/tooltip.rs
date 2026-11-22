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

//! The tooltip: a few lines a node shows while the pointer rests on it or
//! keyboard focus is on it, in an overlay beside it (ADR-0052).

use std::any::Any;
use std::fmt;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, TextLength};

use crate::a11y::{Role, Semantics};
use crate::draw::{DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{ActionKind, EventContext, Handled};
use crate::layout::{Align, Insets, Style};
use crate::metrics::TextMetrics;
use crate::overlay::{Anchor, OverlayKind, Side};
use crate::theme::Token;
use crate::tree::{Node, NodeId, Tree, Widget};

use super::modal::kept_lines;
use super::{
    MAX_TOOLTIP_LINES, TOOLTIP_GAP, TOOLTIP_MAX_WIDTH, TOOLTIP_PADDING_X, TOOLTIP_PADDING_Y,
    TOOLTIP_TEXT, baseline, line_height,
};

/// What brought a node's tooltip about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TooltipCause {
    /// The pointer is on the node, or on its open tooltip.
    Pointer,
    /// Keyboard focus is on the node, and shows.
    Focus,
}

/// The node whose tooltip should show now, and why
/// ([`Tree::tooltip_target`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TooltipTarget {
    /// The node, which has a tooltip.
    pub node: NodeId,
    /// What brought it about.
    pub cause: TooltipCause,
}

/// A tooltip: the text a node was given with [`Tree::set_tooltip`], shown
/// in an overlay below the node, or above it if there is no room below
/// (ADR-0052).
///
/// # Its owner's timer
///
/// `ui` has no clock, so the delay before a tooltip shows is its owner's
/// timer, in the main loop's `next_deadline` (`docs/01-architecture.md`).
/// [`Tree::tooltip_target`] tells the owner which node's tooltip should
/// show now, if any. After each event it handles and each frame it draws,
/// the owner compares it with the target it last saw:
///
/// - when it changes, the owner closes the tooltip it has open, if any,
///   and, if there is a new target, sets its timer (a short delay for the
///   pointer, none for keyboard focus, as the owner likes);
/// - when the timer fires and the target is still the same, the owner
///   opens its tooltip with [`Tooltip::open`].
///
/// When the tooltip is dismissed, it emits [`ActionKind::Dismiss`], and
/// the target is `None` from then on until the pointer or focus moves to
/// another node: the owner closes it as the target changes, as it closes
/// any, and opens none for that node meanwhile.
///
/// # What WCAG 1.4.13 asks of it
///
/// - *Dismissible*: Escape, going down with no modifier but Shift, goes to
///   an open tooltip before anything else, wherever focus is, so it closes
///   the tooltip without moving the pointer or focus, and not the dialog
///   or field the user is in. The tooltip emits [`ActionKind::Dismiss`];
///   a repeat of a held Escape is taken and emits nothing. Then no node is
///   the target, whatever is under the pointer or focused, until the
///   pointer moves to a new position or focus to another node, and what
///   the target would be then is another node than the one dismissed.
/// - *Hoverable*: the pointer on an open tooltip counts as on its node, so
///   the target stays and the owner keeps it open, while the tooltip is
///   still anchored to that node and the node is shown and not inert. For
///   this the tooltip is a modeless overlay ([`OverlayKind::Modeless`])
///   and not a pass-through one: the pointer finds it, so it can rest on
///   it, and a press on it, which it takes and acts on no further, reaches
///   nothing it covers. It holds nothing that takes focus, so Tab never
///   reaches it. In a window too small for both, where it is moved over
///   its own node, it never keeps the pointer from that node: there, the
///   pointer and a press reach the node.
/// - *Persistent*: it stays until the target changes, when the pointer
///   leaves the node and its tooltip, or keyboard focus leaves the node,
///   or until it is dismissed.
///
/// It shows for keyboard focus as for the pointer. It follows focus once
/// focus moves to another node, by Tab, a click or its owner, and the
/// pointer once the pointer moves; a key that leaves focus where it is,
/// such as Escape, a letter, or a modifier alone, changes neither (see
/// [`Tree::tooltip_target`]).
///
/// # Its look and its place in the accessibility tree
///
/// `panel-solid`, the spec's color for menus and tooltips, with a 1-unit
/// `line` border, its lines in [`TOOLTIP_TEXT`] (`text`), as written,
/// [`TOOLTIP_PADDING_X`] and [`TOOLTIP_PADDING_Y`] in, at most
/// [`TOOLTIP_MAX_WIDTH`] wide, [`TOOLTIP_GAP`] from its node. The site
/// has no tooltip style; this is derived from its panels and the spec's
/// tokens. It is a tooltip in the accessibility tree, named by its text;
/// its node has the same text as its description whether it shows or not,
/// as `aria-describedby` gives it ([`AccessNode::description`]).
///
/// [`Tree::set_tooltip`] changes the text of the node's open tooltips too.
///
/// [`AccessNode::description`]: crate::a11y::AccessNode::description
///
/// ```
/// use tidepool_core::geom::{Point, Size};
/// use tidepool_core::input::{InputEvent, Modifiers};
/// use tidepool_ui::metrics::FixedMetrics;
/// use tidepool_ui::widgets::{Button, ButtonKind, Control, Tooltip, TooltipCause};
/// use tidepool_ui::Tree;
///
/// let mut tree = Tree::new();
/// let save = tree.insert(tree.root(), Button::new("Save", ButtonKind::Ghost).node())?;
/// tree.set_tooltip(save, "Write main.rs to disk (Ctrl+S)")?;
/// tree.layout(Size::new(400.0, 300.0), &FixedMetrics::DEFAULT);
///
/// // The pointer comes to rest on the button: the owner sets its timer.
/// let on = Point::new(10.0, 10.0);
/// tree.handle(&InputEvent::MouseMove { position: on, modifiers: Modifiers::NONE });
/// let target = tree.tooltip_target().expect("a target");
/// assert_eq!((target.node, target.cause), (save, TooltipCause::Pointer));
///
/// // The timer fires: the owner opens it, and closes it when the target
/// // changes.
/// let tip = Tooltip::open(&mut tree, save)?;
/// assert_eq!(Tooltip::anchor(&tree, tip), Some(save));
/// let away = Point::new(300.0, 200.0);
/// tree.handle(&InputEvent::MouseMove { position: away, modifiers: Modifiers::NONE });
/// assert_eq!(tree.tooltip_target(), None);
/// tree.close_overlay(tip)?;
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
#[derive(Debug)]
pub struct Tooltip;

impl Tooltip {
    /// Opens the tooltip of `anchor`, an overlay below it, and returns the
    /// overlay's root. Focus does not move.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if `anchor` is not in the tree,
    /// [`UiError::NoTooltip`] if it has no tooltip, and the errors of
    /// [`Tree::open_overlay`]. Nothing changes then.
    pub fn open(tree: &mut Tree, anchor: NodeId) -> Result<NodeId, UiError> {
        let text = tree
            .get(anchor)
            .ok_or(UiError::UnknownNode { id: anchor })?
            .tooltip
            .clone()
            .ok_or(UiError::NoTooltip { id: anchor })?;
        let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let style =
            Style::default().padding(Insets::symmetric(TOOLTIP_PADDING_X, TOOLTIP_PADDING_Y));
        let node = Node::new(style)
            .with_semantics(Semantics::new(Role::Tooltip, lines.join(" ")))
            .with_widget(TooltipView { anchor, lines });
        let place = Anchor::node(anchor, Side::Below, Align::Start, TOOLTIP_GAP);
        tree.open_overlay(node, OverlayKind::Modeless, place)
    }

    /// The node a tooltip [`open`](Self::open) opened belongs to, if
    /// `tooltip` is one's root.
    pub fn anchor(tree: &Tree, tooltip: NodeId) -> Option<NodeId> {
        tree.widget::<TooltipView>(tooltip).map(|view| view.anchor)
    }
}

/// A tooltip's root.
struct TooltipView {
    anchor: NodeId,
    lines: Vec<String>,
}

/// Shows its lines as their lengths only (ADR-0043).
impl fmt::Debug for TooltipView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<TextLength<'_>> = self.lines.iter().map(|line| TextLength(line)).collect();
        f.debug_struct("TooltipView")
            .field("anchor", &self.anchor)
            .field("lines", &lines)
            .finish()
    }
}

impl Widget for TooltipView {
    /// As wide as its widest line, up to [`TOOLTIP_MAX_WIDTH`] with its
    /// padding; as tall as its lines.
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let widest = self
            .lines
            .iter()
            .map(|line| metrics.width(line, &TOOLTIP_TEXT))
            .fold(0.0, f32::max);
        let height = self.lines.len() as f32 * line_height(metrics, &TOOLTIP_TEXT);
        Size::new(
            widest.min(TOOLTIP_MAX_WIDTH - 2.0 * TOOLTIP_PADDING_X),
            height,
        )
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let bounds = cx.bounds();
        let theme = cx.theme();
        let metrics = cx.metrics();
        list.rect(bounds, theme.color(Token::PanelSolid));
        list.border(bounds, 1.0, theme.color(Token::Line));
        let x = bounds.x + TOOLTIP_PADDING_X;
        let width = (bounds.width - 2.0 * TOOLTIP_PADDING_X).max(0.0);
        let height = line_height(metrics, &TOOLTIP_TEXT);
        let mut y = bounds.y + TOOLTIP_PADDING_Y;
        list.push_clip(Rect::new(x, bounds.y, width, bounds.height));
        for line in &self.lines {
            if !line.is_empty() {
                let rect = Rect::new(x, y, width, height);
                list.text(
                    Point::new(x, baseline(rect, metrics, &TOOLTIP_TEXT)),
                    line.clone(),
                    TOOLTIP_TEXT,
                    theme.color(Token::Text),
                );
            }
            y += height;
        }
        list.pop_clip();
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        match *event {
            InputEvent::KeyDown {
                key: Key::Escape,
                modifiers,
                repeat,
            } if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                if !repeat {
                    cx.emit(ActionKind::Dismiss);
                }
                Handled::Yes
            }
            // The press is the tooltip's, and goes no further: nothing it
            // covers gets it.
            InputEvent::MouseDown {
                button: MouseButton::Left,
                ..
            }
            | InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => Handled::Yes,
            _ => Handled::No,
        }
    }
}

/// Returns `true` if `widget` is a tooltip's root, which takes Escape
/// before anything else.
pub(crate) fn is_tooltip(widget: &dyn Widget) -> bool {
    (widget as &dyn Any).is::<TooltipView>()
}

impl Tree {
    /// Gives the node a tooltip saying `text`, or none if `text` is empty
    /// once kept and cleaned. It keeps at most [`MAX_TOOLTIP_LINES`] lines
    /// of at most [`MAX_CELL_BYTES`](super::MAX_CELL_BYTES), read only as
    /// far as it keeps them and cleaned as a [`Modal`](super::Modal)'s are.
    /// The node's accessible description is the tooltip's text
    /// ([`AccessNode::description`](crate::a11y::AccessNode::description));
    /// changing its semantics leaves it as it is. The node's open tooltips
    /// say the new text from the next frame; if there is none, they keep
    /// what they say, as the node is no longer the
    /// [target](Tree::tooltip_target), and their owner closes them.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree.
    pub fn set_tooltip(&mut self, id: NodeId, text: impl AsRef<str>) -> Result<(), UiError> {
        let lines = kept_lines(text.as_ref(), MAX_TOOLTIP_LINES);
        let text = lines.join("\n");
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        let blank = text.trim().is_empty();
        node.tooltip = (!blank).then_some(text);
        if blank {
            // An open tooltip keeps what it says: the target is no longer
            // its node, so its owner closes it.
            return Ok(());
        }
        let open: Vec<NodeId> = self
            .overlays
            .iter()
            .map(|layer| layer.root)
            .filter(|&root| Tooltip::anchor(self, root) == Some(id))
            .collect();
        for root in open {
            if let Some(view) = self.widget_mut::<TooltipView>(root) {
                view.lines.clone_from(&lines);
            }
            if let Some(node) = self.get_mut(root) {
                node.semantics = Semantics::new(Role::Tooltip, lines.join(" "));
            }
        }
        Ok(())
    }

    /// The node's tooltip, its lines joined by line feeds; `None` if it has
    /// none or is not in the tree.
    pub fn tooltip(&self, id: NodeId) -> Option<&str> {
        self.get(id)?.tooltip.as_deref()
    }

    /// The node whose tooltip should show now, if any, as of the last event
    /// and layout (see [`Tooltip`] for how its owner uses it):
    ///
    /// - none, after Escape dismissed a tooltip, until the pointer moves to
    ///   a new position or focus to another node, and what this would be
    ///   then is another node than the one dismissed;
    /// - if focus moved to another node since the pointer last moved, the
    ///   focused node, if it has a tooltip and focus shows
    ///   ([`TooltipCause::Focus`]);
    /// - otherwise the nearest node with a tooltip on the hovered path,
    ///   from the node under the pointer up, or the node of the tooltip the
    ///   pointer is on, while that tooltip is still anchored to it and it
    ///   is shown ([`TooltipCause::Pointer`]);
    /// - otherwise the focused node, if it has a tooltip and focus shows.
    ///
    /// No node a modal makes inert is one: neither the hovered path nor
    /// focus holds one, and the node of a tooltip the pointer is on counts
    /// only if it takes input.
    pub fn tooltip_target(&self) -> Option<TooltipTarget> {
        if self.dismissed.is_some() {
            return None;
        }
        self.tooltip_wanted()
    }

    /// What [`Tree::tooltip_target`] would be with no tooltip dismissed.
    fn tooltip_wanted(&self) -> Option<TooltipTarget> {
        let focused = self
            .focus
            .filter(|&focus| self.focus_visible && self.tooltip(focus).is_some())
            .map(|node| TooltipTarget {
                node,
                cause: TooltipCause::Focus,
            });
        if self.focus_last
            && let Some(target) = focused
        {
            return Some(target);
        }
        let over_tooltip = self.hover.first().and_then(|&root| self.live_anchor(root));
        let pointed = over_tooltip
            .or_else(|| {
                self.hover
                    .iter()
                    .rev()
                    .copied()
                    .find(|&node| self.tooltip(node).is_some())
            })
            .map(|node| TooltipTarget {
                node,
                cause: TooltipCause::Pointer,
            });
        pointed.or(focused)
    }

    /// Forgets the tooltip Escape dismissed once the pointer or focus has
    /// moved to another node: called as either moves.
    pub(crate) fn recheck_dismissed(&mut self) {
        if let Some(node) = self.dismissed
            && self.tooltip_wanted().map(|target| target.node) != Some(node)
        {
            self.dismissed = None;
        }
    }

    /// The node of the open tooltip whose root is `tooltip`, if it is still
    /// anchored to it, and the node is shown, takes input and has a
    /// tooltip: otherwise the tooltip is stale, and its owner closes it.
    fn live_anchor(&self, tooltip: NodeId) -> Option<NodeId> {
        let anchor = Tooltip::anchor(self, tooltip)?;
        let placed = matches!(
            self.overlay_anchor(tooltip),
            Some(Anchor::Node { node, .. }) if node == anchor
        );
        (placed
            && self.is_visible(anchor)
            && self.takes_input(anchor)
            && self.tooltip(anchor).is_some())
        .then_some(anchor)
    }

    /// The topmost open tooltip that takes input, is shown, and is not
    /// stale, which Escape goes to first.
    pub(crate) fn escape_target(&self) -> Option<NodeId> {
        self.overlays
            .iter()
            .rev()
            .map(|layer| layer.root)
            .find(|&root| {
                self.takes_input(root)
                    && self.is_visible(root)
                    && self
                        .get(root)
                        .and_then(|node| node.widget.as_deref())
                        .is_some_and(is_tooltip)
                    && self.live_anchor(root).is_some()
            })
    }
}
