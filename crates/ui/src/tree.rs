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

//! The retained widget tree: [`Tree`], its nodes ([`Node`], named by
//! [`NodeId`]), the [`Widget`] trait a node's code implements, and each
//! node's interaction state ([`NodeState`]).
//!
//! New in Tidepool; not ported.
//!
//! A tree always has a root, which fills the window. Each node has a
//! [`Style`] (how it is laid out), [`Semantics`] (what assistive technology
//! is told), a focusable flag, an enabled flag, and at most one widget,
//! which measures its content, paints it and handles the events routed to
//! it (ADR-0046). The tree tracks what a widget paints from besides its own
//! data: whether its node is hovered, pressed, focused, or disabled.
//!
//! # Ids
//!
//! A [`NodeId`] is an index and a generation. Removing a node bumps its
//! slot's generation, so an id kept after its node was removed names no
//! node, even once the slot holds another: every call that takes it
//! returns [`UiError::UnknownNode`] or `None`, and
//! [`contains`](Tree::contains) `false`. A slot whose generation
//! would wrap is retired and never used again.
//!
//! An id is meaningful only in the tree that made it. It carries no mark
//! of its tree, so one from another tree is not detected: it names
//! whatever node this tree holds at that index and generation, if any.
//! Every tree's root is `0v0`, for one.
//!
//! # Overlays
//!
//! Besides its main root, a tree may hold overlay roots, painted over the
//! main root in the order they were opened and placed by an anchor: a
//! dialog, the command palette, a tooltip or a toast
//! ([`overlay`](crate::overlay), ADR-0052). An overlay root has no parent,
//! and the nodes below it are like any others.
//!
//! # Limits
//!
//! A node may be at most [`MAX_DEPTH`] levels below the root of its layer
//! (the main root or an overlay's), and the tree holds at most
//! [`MAX_NODES`] nodes, overlays included. Every operation that takes an id
//! or a value checks it; none panics.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::InputEvent;

use crate::a11y::Semantics;
use crate::draw::{Command, DrawList, PaintContext};
use crate::error::UiError;
use crate::input::{EventContext, Handled, PressGuard};
use crate::layout::{Memo, Placement, Style};
use crate::metrics::TextMetrics;
use crate::overlay::Layer;
use crate::theme::Theme;

mod arena;

/// The most levels a node may be below the root of its layer: the main
/// root or an overlay's.
pub const MAX_DEPTH: usize = 256;

/// The most nodes a tree may hold, the root included.
pub const MAX_NODES: usize = 1 << 20;

/// The most damage rects removals leave waiting for the next frame; more
/// become their bounding rect, as a frame would merge them anyway.
const MAX_PENDING_DAMAGE: usize = 4 * crate::draw::MAX_DAMAGE_RECTS;

/// The name of a node in a [`Tree`]: an index and a generation.
///
/// It shows as `3v1` (index 3, generation 1), and in `Debug` as
/// `NodeId(3v1)`. Ids are ordered by index, then generation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    /// An id that names no node in any tree: its index is past
    /// [`MAX_NODES`], which no slot reaches. For a widget that keeps ids of
    /// nodes it has not made yet.
    pub(crate) const NONE: Self = Self {
        index: u32::MAX,
        generation: u32::MAX,
    };
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({self})")
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// A node to [`insert`](Tree::insert): its style, semantics, flags and
/// widget.
///
/// ```
/// use tidepool_ui::a11y::{Role, Semantics};
/// use tidepool_ui::layout::Style;
/// use tidepool_ui::{Node, Tree};
///
/// let mut tree = Tree::new();
/// let save = Node::new(Style::stack().fixed(96.0, 30.0))
///     .with_semantics(Semantics::new(Role::Button, "Save"))
///     .focusable();
/// let id = tree.insert(tree.root(), save)?;
/// assert_eq!(tree.focusable(id), Some(true));
/// # Ok::<(), tidepool_ui::UiError>(())
/// ```
pub struct Node {
    /// How it is laid out.
    pub style: Style,
    /// What assistive technology is told about it.
    pub semantics: Semantics,
    /// Whether it can take keyboard focus. A focusable node needs a role
    /// and a name.
    pub focusable: bool,
    /// Whether it is enabled; a disabled node and its subtree get no
    /// events and cannot take focus. `true` by default.
    pub enabled: bool,
    /// The code that measures, paints and handles events for it.
    pub widget: Option<Box<dyn Widget>>,
}

impl Node {
    /// A node with `style`, no semantics or widget, not focusable, and
    /// enabled.
    pub fn new(style: Style) -> Self {
        Self {
            style,
            semantics: Semantics::default(),
            focusable: false,
            enabled: true,
            widget: None,
        }
    }

    /// The same, with `semantics`.
    pub fn with_semantics(self, semantics: Semantics) -> Self {
        Self { semantics, ..self }
    }

    /// The same, focusable.
    pub fn focusable(self) -> Self {
        Self {
            focusable: true,
            ..self
        }
    }

    /// The same, disabled.
    pub fn disabled(self) -> Self {
        Self {
            enabled: false,
            ..self
        }
    }

    /// The same, with `widget`.
    pub fn with_widget(self, widget: impl Widget) -> Self {
        Self {
            widget: Some(Box::new(widget)),
            ..self
        }
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new(Style::default())
    }
}

/// Shows whether there is a widget, not the widget.
impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("style", &self.style)
            .field("semantics", &self.semantics)
            .field("focusable", &self.focusable)
            .field("enabled", &self.enabled)
            .field("widget", &self.widget.is_some())
            .finish()
    }
}

/// The code of a node: it measures the node's content, paints it, and
/// handles the events routed to it. Every method has a default that does
/// nothing.
///
/// A widget paints from its own data and the node's [`NodeState`], which
/// the tree tracks. Its owner reaches it through
/// [`Tree::widget`] and [`Tree::widget_mut`].
pub trait Widget: Any {
    /// The size of the node's content, inside its padding, given the
    /// space `available` (a hint; either side may be infinite). Layout
    /// sanitizes the result. Zero by default.
    fn measure(&self, available: Size, metrics: &dyn TextMetrics) -> Size {
        let _ = (available, metrics);
        Size::ZERO
    }

    /// Paints the node into `list`, in window coordinates; `cx` gives its
    /// bounds and state. Paints nothing by default.
    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let _ = (cx, list);
    }

    /// Handles an event routed to the node. Returning [`Handled::Yes`]
    /// stops it bubbling to the ancestors. Ignores every event by default.
    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let _ = (cx, event);
        Handled::No
    }
}

/// A node's interaction state, which the tree tracks and its widget paints
/// from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeState {
    /// The pointer is over this node or a descendant: it is on the
    /// [hit-test](Tree::hit_test) path of the pointer's last position. That
    /// path is found again at each pointer event, after each layout and
    /// when an overlay opens or closes, so it follows content that moves
    /// under a pointer at rest. No node an active modal makes inert is
    /// hovered ([`overlay`](crate::overlay)).
    pub hovered: bool,
    /// The primary button went down on this node or a descendant and is
    /// not yet up.
    pub pressed: bool,
    /// This node has keyboard focus.
    pub focused: bool,
    /// This node or a descendant has keyboard focus: a focused panel's
    /// bracket corners.
    pub focus_within: bool,
    /// This node has focus, and focus should show: it came from the
    /// keyboard (`:focus-visible`).
    pub focus_visible: bool,
    /// This node or an ancestor is disabled, by its own flag
    /// ([`Tree::enabled`]) or by a modal dialog that squeezed it
    /// ([`Modal`](crate::widgets::Modal)).
    pub disabled: bool,
}

/// A node as the tree keeps it.
pub(crate) struct NodeData {
    /// The style as it was given.
    pub(crate) style: Style,
    /// The style made sane, which layout uses.
    pub(crate) sane: Style,
    pub(crate) semantics: Semantics,
    pub(crate) focusable: bool,
    pub(crate) enabled: bool,
    pub(crate) widget: Option<Box<dyn Widget>>,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    /// Levels below the root of its layer.
    pub(crate) depth: usize,
    /// What the last layout gave it.
    pub(crate) placement: Placement,
    /// The scroll offset of its children.
    pub(crate) scroll: Point,
    /// Natural sizes measured in this layout pass, by hint.
    pub(crate) memo: Memo,
    /// What its widget painted last.
    pub(crate) fragment: Vec<Command>,
    /// The bounds of that, clipped; `None` if it painted nothing visible.
    pub(crate) painted: Option<Rect>,
    /// Whether it is in the tree's paint queue.
    pub(crate) queued: bool,
    /// Whether its next repaint damages its painted bounds even if they
    /// did not change.
    pub(crate) forced: bool,
    /// What a tooltip on it says ([`Tree::set_tooltip`]).
    pub(crate) tooltip: Option<String>,
    /// The list whose selected item is its active option in the
    /// accessibility tree: a command palette's field's.
    pub(crate) active_in: Option<NodeId>,
    /// The node its widget's actions are reported from, if not its own: a
    /// command palette's list's are its palette's.
    pub(crate) actions_to: Option<NodeId>,
    /// Whether its owner widget disabled it for want of room, apart from
    /// its own [`enabled`](Self::enabled) flag: a modal dialog's squeezed
    /// body and row.
    pub(crate) squeezed: bool,
    /// Whether its widget keeps it out of view for want of room, apart
    /// from its style's [`hidden`](crate::layout::Style::hidden), which
    /// stays its owner's: a toast with no room in the column of toasts.
    /// It is hidden, as its sanitized style says, while this is set.
    pub(crate) parked: bool,
}

impl NodeData {
    fn new(node: Node, parent: Option<NodeId>, depth: usize) -> Self {
        let Node {
            style,
            semantics,
            focusable,
            enabled,
            widget,
        } = node;
        Self {
            sane: style.sanitized(),
            style,
            semantics,
            focusable,
            enabled,
            widget,
            parent,
            children: Vec::new(),
            depth,
            placement: Placement::default(),
            scroll: Point::ZERO,
            memo: Memo::default(),
            fragment: Vec::new(),
            painted: None,
            queued: false,
            forced: false,
            tooltip: None,
            active_in: None,
            actions_to: None,
            squeezed: false,
            parked: false,
        }
    }
}

/// A slot of the arena: a generation, and the node if one lives there.
struct Slot {
    generation: u32,
    node: Option<NodeData>,
}

/// A retained tree of nodes, with their layout, focus, input routing,
/// paint and accessibility.
///
/// The methods are spread over the modules that describe them:
/// [`layout`](crate::layout) (laying out and scrolling),
/// [`focus`](crate::focus), [`input`](crate::input) (routing events and
/// hit testing), [`draw`](crate::draw) (frames and their damage),
/// [`a11y`](crate::a11y) and [`overlay`](crate::overlay) (the layers over
/// the main root).
pub struct Tree {
    slots: Vec<Slot>,
    /// Free slots, used last in, first out.
    free: Vec<u32>,
    root: NodeId,
    /// Live nodes, the root included.
    count: usize,
    pub(crate) focus: Option<NodeId>,
    /// Whether focus shows (keyboard modality).
    pub(crate) focus_visible: bool,
    pub(crate) window_focused: bool,
    /// The hit-test path of the pointer, from the root of its layer down.
    pub(crate) hover: Vec<NodeId>,
    /// Where the last pointer event put the pointer, in window coordinates.
    pub(crate) pointer: Option<Point>,
    /// The node a primary press captured.
    pub(crate) capture: Option<NodeId>,
    /// Whether something changed layout since the last pass.
    pub(crate) layout_dirty: bool,
    /// The node the next layout scrolls into view: the latest request.
    pub(crate) reveal: Option<NodeId>,
    /// The rows collections asked to be scrolled into view at this
    /// layout, each once, in the order the layout binds the collections,
    /// which is the order they were inserted in; the layout scrolls each in
    /// turn, then [`reveal`](Self::reveal) (ADR-0052).
    pub(crate) reveal_rows: Vec<NodeId>,
    /// The window size of the last layout pass.
    pub(crate) layout_size: Option<Size>,
    /// Whether the next frame repaints every node and damages the window.
    pub(crate) repaint_all: bool,
    /// The window size of the last frame.
    pub(crate) frame_size: Option<Size>,
    /// Nodes to repaint at the next frame.
    pub(crate) paint_queue: Vec<NodeId>,
    /// Damage waiting for the next frame: removed nodes' painted bounds.
    pub(crate) damage: Vec<Rect>,
    /// What widgets paint with (ADR-0049).
    pub(crate) theme: Arc<Theme>,
    /// How many times keyboard focus moved to another node, or the window
    /// lost focus ([`EventContext::focus_moves`](crate::input::EventContext::focus_moves)).
    pub(crate) focus_moves: u64,
    /// The nodes whose widget keeps other nodes in step with it, which
    /// each layout binds: a [`Collection`](crate::widgets::Collection)'s
    /// rows, a [`TabStrip`](crate::widgets::TabStrip)'s tabs, a
    /// [`Splitter`](crate::widgets::Splitter)'s split (ADR-0052); removed
    /// ones are dropped by each layout, and by a removal once they are
    /// more than half of it.
    pub(crate) bound: Vec<NodeId>,
    /// How many of [`bound`](Self::bound) were removed since it was last
    /// cut to the nodes in the tree.
    pub(crate) bound_gone: usize,
    /// The open overlays, from the bottom to the top
    /// ([`overlay`](crate::overlay)).
    pub(crate) overlays: Vec<Layer>,
    /// Whether keyboard focus moved to another node since the pointer last
    /// moved: which of the two a tooltip follows
    /// ([`Tree::tooltip_target`]).
    pub(crate) focus_last: bool,
    /// The node whose tooltip Escape dismissed: no node is the tooltip
    /// target until the pointer or focus moves to another
    /// ([`Tree::tooltip_target`]).
    pub(crate) dismissed: Option<NodeId>,
    /// The active modal's root as the layers last settled, to see when
    /// another modal becomes active ([`overlay`](crate::overlay)).
    pub(crate) settled_modal: Option<NodeId>,
    /// Set when the active modal changes, until the pointer moves past
    /// the slop: a press meanwhile counts as on the modal's root, or on
    /// nothing if no modal is active (ADR-0055, ADR-0056).
    pub(crate) press_guard: Option<PressGuard>,
    /// Counts the overlays opened, so that each knows its place in the
    /// order they opened in, which moving one does not change.
    pub(crate) overlay_seq: u64,
    /// Where the pointer was when a toast under it closed: the column of
    /// toasts keeps still until the pointer moves past the slop from
    /// there, so that no other toast slides under it
    /// ([`Toast`](crate::widgets::Toast)).
    pub(crate) toast_pin: Option<Point>,
    /// Device pixels per logical unit, as the metrics of the last layout
    /// said ([`TextMetrics::scale`](crate::metrics::TextMetrics::scale)):
    /// what the press guard's slop is measured in.
    pub(crate) scale: f32,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    /// A tree of one node, the root, with the default style, painting
    /// with the bundled theme ([`Theme::bundled`]).
    ///
    /// The window counts as focused until a
    /// [`FocusLost`](InputEvent::FocusLost) says otherwise.
    pub fn new() -> Self {
        let root = NodeId {
            index: 0,
            generation: 0,
        };
        let mut tree = Self {
            slots: vec![Slot {
                generation: 0,
                node: Some(NodeData::new(Node::default(), None, 0)),
            }],
            free: Vec::new(),
            root,
            count: 1,
            focus: None,
            focus_visible: false,
            window_focused: true,
            hover: Vec::new(),
            pointer: None,
            capture: None,
            layout_dirty: true,
            reveal: None,
            reveal_rows: Vec::new(),
            layout_size: None,
            repaint_all: true,
            frame_size: None,
            paint_queue: Vec::new(),
            damage: Vec::new(),
            theme: Theme::bundled(),
            focus_moves: 0,
            bound: Vec::new(),
            bound_gone: 0,
            overlays: Vec::new(),
            focus_last: false,
            dismissed: None,
            settled_modal: None,
            press_guard: None,
            overlay_seq: 0,
            toast_pin: None,
            scale: 1.0,
        };
        tree.mark(root);
        tree
    }

    /// The main root, which is always there and fills the window. Overlays
    /// have roots of their own ([`Tree::overlays`]).
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// How many nodes the tree holds, the root included.
    pub const fn node_count(&self) -> usize {
        self.count
    }

    /// Returns `true` if the id names a node in the tree.
    pub fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    /// Adds `node` as the last child of `parent`.
    ///
    /// # Errors
    ///
    /// As for [`insert_at`](Tree::insert_at).
    pub fn insert(&mut self, parent: NodeId, node: Node) -> Result<NodeId, UiError> {
        let len = self.get(parent).map_or(0, |parent| parent.children.len());
        self.insert_at(parent, len, node)
    }

    /// Adds `node` as the child of `parent` at `index`, before the child
    /// that was there.
    ///
    /// # Errors
    ///
    /// - [`UiError::UnknownNode`] if `parent` is not in the tree;
    /// - [`UiError::BadIndex`] if `index` is past its children;
    /// - [`UiError::Unnamed`] if the node is focusable without a role and a
    ///   name;
    /// - [`UiError::TooDeep`] if it would be more than [`MAX_DEPTH`] levels
    ///   below the root of its layer;
    /// - [`UiError::TooManyNodes`] if the tree already holds
    ///   [`MAX_NODES`].
    pub fn insert_at(
        &mut self,
        parent: NodeId,
        index: usize,
        node: Node,
    ) -> Result<NodeId, UiError> {
        let into = self
            .get(parent)
            .ok_or(UiError::UnknownNode { id: parent })?;
        let len = into.children.len();
        let depth = into.depth + 1;
        if index > len {
            return Err(UiError::BadIndex { index, len });
        }
        if node.focusable && !node.semantics.is_named() {
            return Err(UiError::Unnamed {
                role: node.semantics.role,
            });
        }
        if depth > MAX_DEPTH {
            return Err(UiError::TooDeep { max: MAX_DEPTH });
        }
        if self.count >= MAX_NODES {
            return Err(UiError::TooManyNodes { max: MAX_NODES });
        }
        let id = self.allocate(NodeData::new(node, Some(parent), depth))?;
        if let Some(into) = self.get_mut(parent) {
            into.children.insert(index, id);
        }
        Ok(id)
    }

    /// Adds `node` as the root of a new layer, with no parent: an
    /// overlay's, which [`open_overlay`](Tree::open_overlay) records.
    ///
    /// # Errors
    ///
    /// [`UiError::Unnamed`] if the node is focusable without a role and a
    /// name, and [`UiError::TooManyNodes`] if the tree already holds
    /// [`MAX_NODES`].
    pub(crate) fn insert_root(&mut self, node: Node) -> Result<NodeId, UiError> {
        if node.focusable && !node.semantics.is_named() {
            return Err(UiError::Unnamed {
                role: node.semantics.role,
            });
        }
        if self.count >= MAX_NODES {
            return Err(UiError::TooManyNodes { max: MAX_NODES });
        }
        self.allocate(NodeData::new(node, None, 0))
    }

    /// Puts `data` in a free slot, or a new one, and counts, lays out and
    /// repaints it; the caller links it to its parent.
    fn allocate(&mut self, data: NodeData) -> Result<NodeId, UiError> {
        let windowed = data.widget.as_deref().is_some_and(crate::widgets::is_bound);
        let id = if let Some(index) = self.free.pop() {
            let slot = self
                .slots
                .get_mut(index as usize)
                .ok_or(UiError::TooManyNodes { max: MAX_NODES })?;
            slot.node = Some(data);
            NodeId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = u32::try_from(self.slots.len())
                .map_err(|_| UiError::TooManyNodes { max: MAX_NODES })?;
            self.slots.push(Slot {
                generation: 0,
                node: Some(data),
            });
            NodeId {
                index,
                generation: 0,
            }
        };
        self.count += 1;
        self.layout_dirty = true;
        self.mark(id);
        if windowed {
            self.bound.push(id);
        }
        Ok(id)
    }

    /// Removes the node and its subtree.
    ///
    /// Focus, the hovered path and a press inside it are cleared (focus
    /// goes to the active modal's default, if a modal is active; see
    /// [`overlay`](crate::overlay#giving-focus-back)), and what the removed
    /// nodes last painted is damaged at the next frame. What
    /// waits for that frame stays bounded however many nodes come and go
    /// before it: the repaint queue is cut to the nodes still in the tree,
    /// and more than four times
    /// [`MAX_DAMAGE_RECTS`](crate::draw::MAX_DAMAGE_RECTS) damage rects
    /// become their bounding rect.
    ///
    /// Removing an overlay's root closes the overlay, as
    /// [`close_overlay`](Tree::close_overlay) does, focus given back
    /// included.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree, and
    /// [`UiError::RootNode`] for the main root.
    pub fn remove(&mut self, id: NodeId) -> Result<(), UiError> {
        if !self.contains(id) {
            return Err(UiError::UnknownNode { id });
        }
        if id == self.root {
            return Err(UiError::RootNode);
        }
        if self.is_overlay_root(id) {
            return self.close_overlay(id).map(|_| ());
        }
        self.remove_subtree(id);
        self.settle_focus();
        Ok(())
    }

    /// Removes the node, which is in the tree and not the main root, and
    /// its subtree, as [`remove`](Tree::remove) describes; an overlay's
    /// record is the caller's.
    pub(crate) fn remove_subtree(&mut self, id: NodeId) {
        let Some(node) = self.get(id) else {
            return;
        };
        let parent = node.parent;
        self.leave(id);
        if let Some(parent) = parent.and_then(|parent| self.get_mut(parent)) {
            parent.children.retain(|&child| child != id);
        }
        let mut bound_gone = 0;
        for gone in self.subtree(id) {
            let Some(slot) = self.slots.get_mut(gone.index as usize) else {
                continue;
            };
            if let Some(node) = slot.node.take() {
                if node.widget.as_deref().is_some_and(crate::widgets::is_bound) {
                    bound_gone += 1;
                }
                self.damage.extend(node.painted);
                self.count -= 1;
            }
            // A slot whose generation would wrap is retired.
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(gone.index);
            }
        }
        self.layout_dirty = true;
        self.bound_pending();
        // Only a bound widget's removal leaves the list a node to drop, so
        // removing other nodes never reads it; and it is cut only once more
        // than half of it is gone, so the cost is spread over the removals.
        self.bound_gone += bound_gone;
        if bound_gone > 0 && self.bound_gone > self.bound.len() / 2 {
            let mut bound = std::mem::take(&mut self.bound);
            bound.retain(|&node| self.contains(node));
            self.bound = bound;
            self.bound_gone = 0;
        }
    }

    /// Keeps what waits for the next frame bounded: the repaint queue (an
    /// insert queues its node, and a removal leaves its id behind) and the
    /// removed nodes' damage.
    fn bound_pending(&mut self) {
        // Cut only when it is well past the live nodes, so the cost is
        // spread over the inserts that grew it.
        if self.paint_queue.len() > 2 * self.count + 64 {
            let mut queue = std::mem::take(&mut self.paint_queue);
            queue.retain(|&id| self.contains(id));
            self.paint_queue = queue;
        }
        if self.damage.len() > MAX_PENDING_DAMAGE {
            let bounding = crate::draw::bounding(&self.damage);
            self.damage.clear();
            self.damage.extend(bounding);
        }
    }

    /// The node's parent; `None` for the root and an unknown node.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.get(id).and_then(|node| node.parent)
    }

    /// The node's children, in order.
    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.get(id).map(|node| node.children.as_slice())
    }

    /// The node's style, as it was given.
    pub fn style(&self, id: NodeId) -> Option<&Style> {
        self.get(id).map(|node| &node.style)
    }

    /// Gives the node a new style.
    ///
    /// The tree is laid out again, and the subtree's old and new painted
    /// bounds are damaged at the next frame, since a change of clip,
    /// opacity or visibility changes pixels its bounds may not show.
    /// Hiding the node clears focus, the hovered path and a press inside
    /// it; while a modal is active, focus goes to its default. Hiding or
    /// showing a modal overlay's root changes which layers are inert
    /// ([`overlay`](crate::overlay#kinds)).
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree.
    pub fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), UiError> {
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        node.style = style;
        node.sane = style.sanitized();
        node.sane.hidden |= node.parked;
        let hidden = node.sane.hidden;
        self.layout_dirty = true;
        for inside in self.subtree(id) {
            if let Some(node) = self.get_mut(inside) {
                node.forced = true;
            }
            self.mark(inside);
        }
        if hidden {
            self.leave(id);
        }
        if self.is_overlay_root(id) {
            self.settle_layers();
        }
        self.settle_focus();
        Ok(())
    }

    /// Keeps the node out of view, or lets it back, for want of room, as
    /// its widget decides, leaving its style, its owner's, as it is; with
    /// what hiding it with [`set_style`](Tree::set_style) does besides.
    /// Returns whether that changed it.
    pub(crate) fn park(&mut self, id: NodeId, parked: bool) -> bool {
        let Some(node) = self.get_mut(id) else {
            return false;
        };
        if node.parked == parked {
            return false;
        }
        node.parked = parked;
        let style = node.style;
        // In the tree: found just now.
        let _ = self.set_style(id, style);
        true
    }

    /// The node's semantics.
    pub fn semantics(&self, id: NodeId) -> Option<&Semantics> {
        self.get(id).map(|node| &node.semantics)
    }

    /// Gives the node new semantics.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree, and
    /// [`UiError::Unnamed`] if it is focusable and `semantics` has no role
    /// or no name; nothing changes then.
    pub fn set_semantics(&mut self, id: NodeId, semantics: Semantics) -> Result<(), UiError> {
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        if node.focusable && !semantics.is_named() {
            return Err(UiError::Unnamed {
                role: semantics.role,
            });
        }
        node.semantics = semantics;
        Ok(())
    }

    /// Whether the node is focusable (its own flag).
    pub fn focusable(&self, id: NodeId) -> Option<bool> {
        self.get(id).map(|node| node.focusable)
    }

    /// Makes the node focusable or not. Making the focused node not
    /// focusable clears focus, or gives it to the active modal's default
    /// ([`overlay`](crate::overlay#giving-focus-back)).
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree, and
    /// [`UiError::Unnamed`] if `focusable` and its semantics have no role
    /// or no name; nothing changes then.
    pub fn set_focusable(&mut self, id: NodeId, focusable: bool) -> Result<(), UiError> {
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        if focusable && !node.semantics.is_named() {
            return Err(UiError::Unnamed {
                role: node.semantics.role,
            });
        }
        node.focusable = focusable;
        if !focusable && self.focus == Some(id) {
            self.drop_focus();
        }
        // A modal's default made focusable again takes the focus no node
        // has.
        self.settle_focus();
        Ok(())
    }

    /// Whether the node is enabled (its own flag; an ancestor may still be
    /// disabled).
    pub fn enabled(&self, id: NodeId) -> Option<bool> {
        self.get(id).map(|node| node.enabled)
    }

    /// Enables or disables the node, and so its subtree, which is
    /// repainted. Disabling clears focus and releases a press inside it;
    /// while a modal is active, focus goes to its default
    /// ([`overlay`](crate::overlay#giving-focus-back)).
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree.
    pub fn set_enabled(&mut self, id: NodeId, enabled: bool) -> Result<(), UiError> {
        let node = self.get_mut(id).ok_or(UiError::UnknownNode { id })?;
        if node.enabled == enabled {
            return Ok(());
        }
        node.enabled = enabled;
        for inside in self.subtree(id) {
            self.mark(inside);
        }
        if enabled {
            // A modal's default enabled again takes the focus no node has.
            self.settle_focus();
        } else {
            self.leave_disabled(id);
        }
        Ok(())
    }

    /// Clears focus and releases a press inside the subtree at `id`, which
    /// was just disabled, giving focus to the active modal's default
    /// ([`overlay`](crate::overlay#giving-focus-back)).
    pub(crate) fn leave_disabled(&mut self, id: NodeId) {
        if self.focus.is_some_and(|focus| self.is_within(focus, id)) {
            self.drop_focus();
        }
        if self
            .capture
            .is_some_and(|capture| self.is_within(capture, id))
        {
            self.release_capture();
        }
        self.settle_focus();
    }

    /// The node's widget, if it has one of type `T`.
    pub fn widget<T: Widget>(&self, id: NodeId) -> Option<&T> {
        let widget: &dyn Widget = self.get(id)?.widget.as_deref()?;
        (widget as &dyn Any).downcast_ref()
    }

    /// The node's widget, if it has one of type `T`, to change.
    ///
    /// Since its size may change, the tree is laid out again and the node
    /// repainted at the next frame.
    pub fn widget_mut<T: Widget>(&mut self, id: NodeId) -> Option<&mut T> {
        self.widget::<T>(id)?;
        self.layout_dirty = true;
        self.mark(id);
        let widget: &mut dyn Widget = self.get_mut(id)?.widget.as_deref_mut()?;
        (widget as &mut dyn Any).downcast_mut()
    }

    /// The node's interaction state.
    pub fn state(&self, id: NodeId) -> Option<NodeState> {
        self.get(id)?;
        let focused = self.focus == Some(id);
        Some(NodeState {
            hovered: self.hover.contains(&id),
            pressed: self
                .capture
                .is_some_and(|capture| self.is_within(capture, id)),
            focused,
            focus_within: self.focus.is_some_and(|focus| self.is_within(focus, id)),
            focus_visible: focused && self.focus_visible,
            disabled: !self.is_enabled(id),
        })
    }

    /// Whether the window has keyboard focus, as the last
    /// [`FocusGained`](InputEvent::FocusGained) or
    /// [`FocusLost`](InputEvent::FocusLost) said.
    pub const fn window_focused(&self) -> bool {
        self.window_focused
    }

    /// The theme widgets paint with, through
    /// [`PaintContext::theme`] (ADR-0049).
    pub fn theme(&self) -> &Arc<Theme> {
        &self.theme
    }

    /// Paints with `theme` from the next frame: the tree is laid out again
    /// and every node repainted, which damages the whole window
    /// ([`Tree::invalidate_all`]). A theme equal to the one held changes
    /// nothing.
    pub fn set_theme(&mut self, theme: Arc<Theme>) {
        if Arc::ptr_eq(&theme, &self.theme) || *theme == *self.theme {
            return;
        }
        self.theme = theme;
        self.invalidate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::Role;

    fn button(name: &str) -> Node {
        Node::new(Style::default())
            .with_semantics(Semantics::new(Role::Button, name))
            .focusable()
    }

    #[test]
    fn ids_show_index_and_generation() {
        let id = NodeId {
            index: 3,
            generation: 1,
        };
        assert_eq!(id.to_string(), "3v1");
        assert_eq!(format!("{id:?}"), "NodeId(3v1)");
    }

    #[test]
    fn a_freed_slot_comes_back_with_a_new_generation() {
        let mut tree = Tree::new();
        let root = tree.root();
        let first = tree.insert(root, Node::default()).expect("insert");
        assert_eq!(first.to_string(), "1v0");
        tree.remove(first).expect("remove");
        assert!(!tree.contains(first));
        let second = tree.insert(root, Node::default()).expect("insert");
        assert_eq!(second.to_string(), "1v1");
        assert!(tree.contains(second));
        // The stale id names nothing, though its slot is in use.
        assert_eq!(tree.remove(first), Err(UiError::UnknownNode { id: first }));
        assert_eq!(
            tree.set_style(first, Style::row()),
            Err(UiError::UnknownNode { id: first })
        );
        assert_eq!(tree.parent(first), None);
        assert_eq!(tree.state(first), None);
        assert_eq!(tree.node_count(), 2);
    }

    #[test]
    fn a_slot_whose_generation_would_wrap_is_retired() {
        let mut tree = Tree::new();
        let root = tree.root();
        let old = tree.insert(root, Node::default()).expect("insert");
        tree.slots[1].generation = u32::MAX;
        let last = NodeId {
            index: 1,
            generation: u32::MAX,
        };
        assert_ne!(old, last);
        tree.remove(last).expect("remove");
        let next = tree.insert(root, Node::default()).expect("insert");
        assert_eq!(next.to_string(), "2v0");
        assert!(!tree.contains(last));
        assert_eq!(tree.node_count(), 2);
    }

    #[test]
    fn nodes_may_be_max_depth_below_the_root() {
        let mut tree = Tree::new();
        let mut parent = tree.root();
        for _ in 0..MAX_DEPTH {
            parent = tree
                .insert(parent, Node::default())
                .expect("within the limit");
        }
        assert_eq!(
            tree.insert(parent, Node::default()),
            Err(UiError::TooDeep { max: MAX_DEPTH })
        );
        assert_eq!(tree.node_count(), MAX_DEPTH + 1);
        // The deepest chain lays out, paints and takes a snapshot.
        let metrics = crate::metrics::FixedMetrics::DEFAULT;
        tree.layout(Size::new(100.0, 100.0), &metrics);
        assert!(tree.frame(Size::new(100.0, 100.0), &metrics).is_some());
        assert!(tree.accessibility().nodes().is_empty());
    }

    #[test]
    fn the_tree_refuses_a_node_past_the_limit() {
        let mut tree = Tree::new();
        tree.count = MAX_NODES;
        assert_eq!(
            tree.insert(tree.root(), Node::default()),
            Err(UiError::TooManyNodes { max: MAX_NODES })
        );
    }

    #[test]
    fn a_modal_dialog_needs_room_for_its_three_nodes() {
        use crate::widgets::Modal;
        let mut tree = Tree::new();
        let root = tree.root();
        let kept = tree.insert(root, button("Save")).expect("a button");
        tree.set_focus(kept, true).expect("focused");
        // A press held on the button, which opening a modal would release.
        tree.capture = Some(kept);
        let moves = tree.focus_moves;
        tree.count = MAX_NODES - 2;
        assert_eq!(
            Modal::new("Quit?").open(&mut tree),
            Err(UiError::TooManyNodes { max: MAX_NODES })
        );
        // Refused before anything changed: focus, the press, the overlays,
        // the count.
        assert_eq!(tree.focused(), Some(kept));
        assert!(tree.focus_visible());
        assert_eq!(tree.capture, Some(kept));
        assert_eq!(tree.focus_moves, moves);
        assert!(tree.overlays.is_empty());
        assert_eq!(tree.count, MAX_NODES - 2);
        tree.count = MAX_NODES - 3;
        let dialog = Modal::new("Quit?").open(&mut tree).expect("room for three");
        assert_eq!(tree.count, MAX_NODES);
        assert_eq!(tree.focused(), Some(dialog));
    }

    #[test]
    fn insert_checks_the_parent_index_and_name() {
        let mut tree = Tree::new();
        let root = tree.root();
        let a = tree.insert(root, Node::default()).expect("insert");
        let b = tree.insert_at(root, 0, Node::default()).expect("insert");
        assert_eq!(tree.children(root), Some(&[b, a][..]));
        assert_eq!(
            tree.insert_at(root, 3, Node::default()),
            Err(UiError::BadIndex { index: 3, len: 2 })
        );
        assert_eq!(
            tree.insert(root, Node::default().focusable()),
            Err(UiError::Unnamed { role: Role::None })
        );
        assert_eq!(
            tree.insert(root, button("  ")),
            Err(UiError::Unnamed { role: Role::Button })
        );
        assert!(tree.insert(a, button("OK")).is_ok());
        assert_eq!(tree.remove(root), Err(UiError::RootNode));
        assert_eq!(tree.node_count(), 4);
        tree.remove(a).expect("remove");
        assert_eq!(tree.node_count(), 2);
        assert_eq!(tree.children(root), Some(&[b][..]));
    }

    #[test]
    fn widgets_downcast_to_their_own_type() {
        struct Counter(u32);
        impl Widget for Counter {}
        struct Other;
        impl Widget for Other {}

        let mut tree = Tree::new();
        let id = tree
            .insert(tree.root(), Node::default().with_widget(Counter(1)))
            .expect("insert");
        assert_eq!(tree.widget::<Counter>(id).map(|c| c.0), Some(1));
        assert!(tree.widget::<Other>(id).is_none());
        if let Some(counter) = tree.widget_mut::<Counter>(id) {
            counter.0 = 2;
        }
        assert_eq!(tree.widget::<Counter>(id).map(|c| c.0), Some(2));
        assert!(tree.widget_mut::<Other>(id).is_none());
        assert!(tree.widget::<Counter>(tree.root()).is_none());
    }

    #[test]
    fn the_paint_queue_and_pending_damage_stay_bounded_between_frames() {
        use tidepool_core::color::Color;

        /// Fills its bounds with its color.
        struct Fill(Color);
        impl Widget for Fill {
            fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
                list.rect(cx.bounds(), self.0);
            }
        }

        let metrics = crate::metrics::FixedMetrics::DEFAULT;
        let window = Size::new(200.0, 200.0);
        let cell = Style::default().fixed(10.0, 10.0);
        let mut tree = Tree::new();
        let root = tree.root();
        tree.set_style(root, Style::grid(20)).expect("the root");
        let kept = tree
            .insert(root, Node::new(cell).with_widget(Fill(Color::WHITE)))
            .expect("insert");
        assert!(tree.frame(window, &metrics).is_some());

        // A change made before the churn stays queued through every cut of
        // the queue.
        if let Some(fill) = tree.widget_mut::<Fill>(kept) {
            fill.0 = Color::BLACK;
        }

        // Nodes that come and go between two frames leave the queue
        // bounded by the live nodes.
        for _ in 0..10_000 {
            let id = tree
                .insert(root, Node::new(cell).with_widget(Fill(Color::BLACK)))
                .expect("insert");
            tree.remove(id).expect("remove");
            assert!(
                tree.paint_queue.len() <= 2 * tree.node_count() + 64,
                "{} queued for {} nodes",
                tree.paint_queue.len(),
                tree.node_count()
            );
        }
        // None of the churned nodes was painted, and the cuts kept the live
        // node's repaint: the next frame damages just it, and the one after
        // nothing.
        let frame = tree
            .frame(window, &metrics)
            .expect("the change made before the churn");
        assert_eq!(frame.damage, vec![Rect::new(0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(tree.frame(window, &metrics), None);

        // Removing many painted nodes between frames keeps the pending
        // damage to a few rects, which the frame merges.
        let cells: Vec<NodeId> = (0..199)
            .map(|_| {
                tree.insert(root, Node::new(cell).with_widget(Fill(Color::WHITE)))
                    .expect("insert")
            })
            .collect();
        assert!(tree.frame(window, &metrics).is_some());
        for id in cells {
            tree.remove(id).expect("remove");
            assert!(
                tree.damage.len() <= MAX_PENDING_DAMAGE,
                "{}",
                tree.damage.len()
            );
        }
        let frame = tree.frame(window, &metrics).expect("the removals");
        assert_eq!(frame.damage, vec![Rect::new(0.0, 0.0, 200.0, 100.0)]);
        assert_eq!(tree.node_count(), 2);
    }

    #[test]
    fn node_debug_shows_whether_there_is_a_widget() {
        struct Blank;
        impl Widget for Blank {}
        let shown = format!("{:?}", Node::default().with_widget(Blank));
        assert!(shown.contains("widget: true"), "{shown}");
        assert!(shown.contains("enabled: true"), "{shown}");
    }

    #[test]
    fn removing_nodes_reads_the_bound_list_only_for_bound_widgets_and_seldom() {
        use crate::widgets::{Control, Tab, TabKind, TabStrip};
        let strip = || TabStrip::new("Tabs", TabKind::Content, [Tab::new("a")]).node();
        let mut tree = Tree::new();
        let root = tree.root();
        let strips: Vec<NodeId> = (0..4)
            .map(|_| tree.insert(root, strip()).expect("a strip"))
            .collect();
        let plain = tree.insert(root, Node::default()).expect("a node");
        let inner = tree.insert(plain, Node::default()).expect("a node");
        // A stale id the list would drop if it were read: removing plain
        // nodes leaves it there.
        let stale = NodeId {
            index: 9_999,
            generation: 0,
        };
        tree.bound.push(stale);
        tree.remove(inner).expect("removed");
        tree.remove(plain).expect("removed");
        assert_eq!(tree.bound.len(), 5);
        assert_eq!(tree.bound.last(), Some(&stale));
        // Removing bound widgets reads it only once more than half of it
        // is gone: then it holds only the strips left.
        tree.remove(strips[0]).expect("removed");
        tree.remove(strips[1]).expect("removed");
        assert_eq!(tree.bound.len(), 5);
        tree.remove(strips[2]).expect("removed");
        assert_eq!(tree.bound, [strips[3]]);
        // A layout drops what is gone too.
        tree.bound.push(stale);
        tree.layout(
            Size::new(100.0, 100.0),
            &crate::metrics::FixedMetrics::DEFAULT,
        );
        assert_eq!(tree.bound, [strips[3]]);
        // So removing many, one at a time, stays linear: the list is never
        // more than twice the live ones and one.
        let mut tree = Tree::new();
        let root = tree.root();
        let many: Vec<NodeId> = (0..2_000)
            .map(|_| tree.insert(root, strip()).expect("a strip"))
            .collect();
        for (k, &id) in many.iter().enumerate() {
            tree.remove(id).expect("removed");
            let live = many.len() - k - 1;
            assert!(
                tree.bound.len() <= 2 * live + 1,
                "{} for {live}",
                tree.bound.len()
            );
        }
    }
}
