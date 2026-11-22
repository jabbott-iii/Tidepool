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

//! Overlays: roots of the [`Tree`] painted over its main root, in order,
//! each anchored to a node or to the window (ADR-0052). A modal dialog
//! ([`Modal`](crate::widgets::Modal)), the command palette, a tooltip,
//! and a toast are overlays.
//!
//! New in Tidepool; not ported.
//!
//! # Layers
//!
//! A tree has one main root, which fills the window, and at most
//! [`MAX_OVERLAYS`] overlay roots. [`Tree::open_overlay`] adds one, and
//! [`Tree::close_overlay`] removes one. A root and the nodes below it make
//! a *layer*. Layers are laid out, painted, hit tested, and listed in the
//! accessibility tree in one order, from the bottom to the top
//! ([`Tree::overlays`]): the main root's first, then the overlays.
//!
//! An overlay that is not modal and is anchored to a node is *hosted* by
//! that node's layer: it goes right after its host and the overlays that
//! host already has, so it paints just above that layer, and is live or
//! inert with it. Any other overlay, one anchored to the window or a modal
//! one, goes on top. So a completion popup or a tooltip anchored to a node
//! of the window stays under a modal dialog that is open, inert with the
//! window, however it got there: opened after the dialog, given that node
//! as its anchor later, or there when a hidden dialog is shown again. A
//! tooltip on one of the dialog's own buttons, or a toast anchored to the
//! window, opened after it, goes above it and stays live. An overlay keeps
//! its place when its anchor changes but its host does not, and moves,
//! with the overlays it hosts, when its host changes. When a host closes,
//! the overlays it hosted are hosted by its own host from then on.
//!
//! An overlay root has no parent ([`Tree::parent`] is `None`). Otherwise
//! its nodes are like any others: inserted under it, changed, and removed
//! with the same calls, and at most [`MAX_DEPTH`](crate::tree::MAX_DEPTH)
//! levels below it.
//!
//! # Kinds
//!
//! An [`OverlayKind`] says how an overlay takes input:
//!
//! - **[`Modal`](OverlayKind::Modal)**, a dialog that must be answered
//!   before the rest of the window can be used again, in the spirit of
//!   `aria-modal` in WAI-ARIA (the W3C's Accessible Rich Internet
//!   Applications, which tells assistive technology what a web page's
//!   parts are). The topmost modal overlay whose root is shown is the
//!   *active modal*. It and the layers above it are *live*; every layer
//!   below it is *inert*: no node there is hovered or pressed, takes focus,
//!   or gets an event, and none is in the tab order or the accessibility
//!   snapshot. [`Tree::hit_test`] finds nothing below the active modal: a
//!   point outside every live layer hits the modal's root, so a press
//!   there goes to that root's widget. What nothing handles while a modal
//!   is active comes back [`blocked`](crate::input::Response::blocked), so
//!   the owner runs none of the window's shortcuts for it. Opening a modal
//!   overlay remembers the node that had focus and gives focus to the
//!   overlay's root, if the root can take it, so a screen reader announces
//!   it and Enter accepts nothing (ADR-0053); if not, no node has focus.
//!   The root is not a tab stop: Tab and Shift+Tab move focus among the
//!   live layers' tab stops only, wrapping, from the root to the first or
//!   the last, so focus stays within the modal and what was opened above
//!   it, such as a toast's button. The owner may focus one of its controls
//!   instead ([`Tree::set_focus`], which refuses an inert node). While a
//!   modal is active, focus never rests on no node if its *default* can
//!   take it: its root, or for the command palette its field (see
//!   [Giving focus back](self#giving-focus-back)). While no node has focus
//!   all the same, key events go to the active modal's root.
//!
//!   Whenever the active modal changes (one opens, is shown again, or is
//!   uncovered as the modal over it closes; or the active one closes or is
//!   hidden, leaving another active or none), presses wait until the
//!   pointer has moved more than [`PRESS_SLOP`](crate::input::PRESS_SLOP)
//!   device pixels (4 divided by the window's scale, in logical units)
//!   from where it rested then, as a drag threshold works (ADR-0055,
//!   ADR-0056). Meanwhile a press counts as on the active modal's root, or
//!   on nothing if none is active: it captures that root and focuses it if
//!   it can take focus, goes to no widget, and comes back
//!   [`blocked`](crate::input::Response::blocked), and so does its
//!   release. So a click aimed at the window before a consent prompt
//!   opened, the second click of a double click whose first opened it,
//!   with a pixel or two of jitter, or the second click on a dialog's OK
//!   that closed it, accepts nothing. `ui` keeps where the pointer last
//!   was, so this needs no clock; if no pointer event had placed it yet,
//!   the next one does, and the wait goes on from there. A position that
//!   is not finite is no position: it moves and places nothing.
//! - **[`Modeless`](OverlayKind::Modeless)**: it is hit where its nodes
//!   are, and its nodes take focus, after those of the layers below it in
//!   the tab order. Everything else stays live: a menu, a toast with a
//!   button.
//! - **[`PassThrough`](OverlayKind::PassThrough)**: it is never hit, so
//!   the pointer reaches what is below it, and its nodes never take focus:
//!   a hint that must leave the pointer to what is under it, such as a
//!   drag's image. It is still painted and in the accessibility snapshot
//!   while it is live. (A [`Tooltip`](crate::widgets::Tooltip) is
//!   modeless, so that the pointer can rest on it.)
//!
//! Hiding an overlay's root ([`Style::hidden`](crate::layout::Style))
//! hides its layer as it hides any subtree, and a hidden modal overlay is
//! not the active modal: the layers below it are live until it is shown
//! again. Then focus outside it goes to its default, but it keeps the node
//! it remembered when it opened. Disabling a modal overlay's root disables
//! its nodes and leaves the rest inert.
//!
//! # Giving focus back
//!
//! When the active modal closes while focus is on no node (it was in the
//! modal, which the close removed, or nowhere), focus goes back to the
//! node the modal remembered, if that node is still in the tree and can
//! take focus now. It shows if the last key or click before the close made
//! it show (see [`focus`](crate::focus)), so focus given back after Escape
//! shows and after a click does not. If the node cannot take focus (it
//! was removed, or is now disabled, hidden, not focusable, or inert), or no
//! node had focus when the modal opened, no node has focus, and
//! [`Tree::close_overlay`] returns `None`. The owner then focuses the node
//! that comes next in its workflow, as the WAI-ARIA dialog pattern asks:
//! `ui` cannot know which that is.
//!
//! Closing an overlay that is not the active modal moves no focus, unless
//! focus was inside it: then it is cleared, as removing any node clears
//! it. When a modal overlay closes, the modal overlays above it that
//! remembered a node inside it remember the node it remembered instead;
//! so does the nearest modal overlay above it if it remembered no node or
//! one since removed. So modal dialogs closed in any order give focus
//! back to where it was before the first of them opened.
//!
//! When the active modal closes while focus is on an overlay above it,
//! such as a toast's button, focus stays there, and that overlay is handed
//! the node the modal remembered: when it closes with focus in it, it
//! gives focus back to that node, as the modal would have. So a toast
//! visited from a dialog does not lose the way back to the editor. Once
//! focus moves to a node outside that overlay, the overlay forgets the
//! node: focus that comes back to it later came from elsewhere.
//!
//! While a modal is active, focus never rests on no node if the modal's
//! default can take it: whenever focus would be left on none (the node
//! that had it removed, disabled, hidden or made inert, an overlay holding
//! it closed, or [`Tree::clear_focus`]), it goes to the modal's root, or
//! to the node the modal names instead (the command palette's field),
//! showing as focus last did. Otherwise every key would go to the
//! modal's root, where Enter could run what the user did not see
//! selected.
//!
//! # Anchors and placement
//!
//! In each layout pass, an overlay root is laid out after the main root's
//! layer and its anchor's layer, at its style's size, where [`Length::Fill`](crate::layout::Length)
//! takes the space it is placed in, and never larger than that space. Its
//! [`Anchor`] places it:
//!
//! - [`Anchor::Window`]: within the window less `margin` on every side (a
//!   margin wider than half the window is cut to half of it), by `x` and
//!   `y`, as a [`Layout::Stack`](crate::layout::Layout) places a child.
//! - [`Anchor::Node`]: on `side` of the node's bounds, `gap` away, and
//!   lined up with them along that side by `align`: [`Align::Start`] puts
//!   their left (or top) edges together. If it does not fit in the window
//!   on that side but fits on the other, it flips to the other; if it fits
//!   on neither, it goes on the side with more room. Then, if need be, it
//!   is moved to lie inside the window on both axes, which may make it
//!   cover the node. The space it is placed in is the window.
//!
//! Each overlay is laid out after the layer its anchor is in, wherever
//! that layer is in the order, so an overlay is placed from where its
//! anchor is in the same pass. So no overlay may be anchored to a node of
//! its own, nor of an overlay placed from it, which would make a ring of
//! overlays each placed from the next. If the anchor node is removed, the
//! overlay stays where the node last was until its owner closes it or
//! gives it another anchor ([`Tree::set_overlay_anchor`]): the tree never
//! closes an overlay itself. A hidden overlay follows its node too, so it
//! is beside it when shown again. A hidden node's bounds are an empty rect
//! where it would be, and a node scrolled out of view keeps its bounds, so
//! the owner of a tooltip closes it when its node goes away.
//!
//! # Damage
//!
//! Damage is the part of the window a frame must draw again (see
//! [`draw`](crate::draw)). An overlay damages what it paints when it
//! opens; what it painted and what it paints when it moves (a new anchor,
//! an anchor that moved, or a new size); and what it painted when it
//! closes, as inserting, laying out, and removing any node does. An idle
//! tree with overlays open damages nothing.

use std::iter;

use tidepool_core::geom::Rect;

use crate::error::UiError;
use crate::input::PressGuard;
use crate::layout::Align;
use crate::tree::{Node, NodeId, Tree};

/// The most overlays a tree holds open at once; more are refused.
pub const MAX_OVERLAYS: usize = 64;

/// How an overlay takes input; see the
/// [module documentation](self#kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    /// While it is the active modal, the layers below it are inert, Tab
    /// stays within the live layers, and focus goes back when it closes:
    /// a dialog, the command palette.
    Modal,
    /// Hit where its nodes are, which take focus; the rest stays live: a
    /// menu, a toast with a button.
    Modeless,
    /// Never hit, and its nodes never take focus: a hint that must leave
    /// the pointer to what is under it, such as a drag's image. A
    /// [`Tooltip`](crate::widgets::Tooltip) is not one: it is modeless,
    /// so the pointer can rest on it.
    PassThrough,
}

/// Which side of its anchor node an overlay goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// Above the node.
    Above,
    /// Below the node.
    Below,
    /// Left of the node.
    Left,
    /// Right of the node.
    Right,
}

/// Where an overlay goes; see the
/// [module documentation](self#anchors-and-placement).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// In the window, less `margin` on every side.
    Window {
        /// Where it goes across.
        x: Align,
        /// Where it goes down.
        y: Align,
        /// The least space between it and the window's edges, in logical
        /// units; NaN or negative is 0, and more than half the window is
        /// half of it.
        margin: f32,
    },
    /// Beside a node's bounds.
    Node {
        /// The node, in a layer below the overlay's.
        node: NodeId,
        /// The side it goes on, unless it fits only on the other.
        side: Side,
        /// How it lines up with the node along that side.
        align: Align,
        /// The space between it and the node, in logical units; NaN or
        /// negative is 0.
        gap: f32,
    },
}

impl Anchor {
    /// In the window at `x` and `y`, `margin` from its edges.
    pub const fn window(x: Align, y: Align, margin: f32) -> Self {
        Self::Window { x, y, margin }
    }

    /// On `side` of `node`, `gap` away, lined up with it by `align`.
    pub const fn node(node: NodeId, side: Side, align: Align, gap: f32) -> Self {
        Self::Node {
            node,
            side,
            align,
            gap,
        }
    }

    /// The node it is anchored to, if it is.
    const fn target(self) -> Option<NodeId> {
        match self {
            Self::Window { .. } => None,
            Self::Node { node, .. } => Some(node),
        }
    }
}

/// An open overlay, as the tree keeps it.
pub(crate) struct Layer {
    /// Its root.
    pub(crate) root: NodeId,
    /// How it takes input.
    pub(crate) kind: OverlayKind,
    /// Where it goes.
    pub(crate) anchor: Anchor,
    /// The anchor node's bounds in the last layout pass, or when it was
    /// anchored: where the overlay goes once that node is gone.
    pub(crate) anchor_bounds: Rect,
    /// For a modal overlay, the node that had focus when it opened, which
    /// gets it back when it closes.
    pub(crate) restore: Option<NodeId>,
    /// For an overlay that is not modal and is anchored to a node, the
    /// root of that node's layer, its *host*: the overlay sits right after
    /// it in the order, and is live or inert with it. `None` for one
    /// anchored to the window, and for a modal one.
    pub(crate) host: Option<NodeId>,
    /// For a modal overlay, the node that takes focus while it is active
    /// and no node has it, if not its root: a command palette's field.
    pub(crate) default: Option<NodeId>,
    /// How far above where its anchor puts it a window-anchored overlay
    /// goes, in logical units: a toast's place in the column of toasts.
    pub(crate) lift: f32,
    /// Its place in the order the overlays opened in.
    pub(crate) seq: u64,
}

impl Tree {
    /// Opens an overlay: `node` becomes its root, of `kind`, placed by
    /// `anchor` from the next layout. Returns the root, under which the
    /// owner inserts what it shows.
    ///
    /// It goes on top of the others, unless it is not modal and is
    /// anchored to a node: then it goes right after that node's layer and
    /// the overlays already there (see the
    /// [module documentation](self#layers)). A modal overlay remembers the
    /// node that had focus, gives focus to its root if the root can take
    /// it (and clears it otherwise), and releases a press and the hover
    /// below it (see [Kinds](self#kinds)).
    ///
    /// # Errors
    ///
    /// - [`UiError::TooManyOverlays`] if [`MAX_OVERLAYS`] are open;
    /// - [`UiError::UnknownNode`] if `anchor` names a node not in the
    ///   tree;
    /// - [`UiError::Unnamed`] if the node is focusable without a role and
    ///   a name;
    /// - [`UiError::TooManyNodes`] if the tree already holds
    ///   [`MAX_NODES`](crate::tree::MAX_NODES).
    ///
    /// Nothing changes then.
    pub fn open_overlay(
        &mut self,
        node: Node,
        kind: OverlayKind,
        anchor: Anchor,
    ) -> Result<NodeId, UiError> {
        if self.overlays.len() >= MAX_OVERLAYS {
            return Err(UiError::TooManyOverlays { max: MAX_OVERLAYS });
        }
        // Any node in the tree is laid out before the new overlay, as its
        // host's group or a layer below the top.
        let anchor_bounds = match anchor.target() {
            Some(target) => self
                .bounds(target)
                .ok_or(UiError::UnknownNode { id: target })?,
            None => Rect::ZERO,
        };
        let host = self.host_for(kind, anchor)?;
        let root = self.insert_root(node)?;
        let at = host.map_or(self.overlays.len(), |host| self.group_end(host));
        self.overlay_seq = self.overlay_seq.wrapping_add(1);
        let restore = match kind {
            OverlayKind::Modal => self.focus,
            OverlayKind::Modeless | OverlayKind::PassThrough => None,
        };
        self.overlays.insert(
            at,
            Layer {
                root,
                kind,
                anchor,
                anchor_bounds,
                restore,
                host,
                default: None,
                lift: 0.0,
                seq: self.overlay_seq,
            },
        );
        if kind == OverlayKind::Modal && self.can_focus(root) {
            self.change_focus(Some(root), self.focus_visible);
        }
        self.settle_layers();
        Ok(root)
    }

    /// Closes the overlay whose root is `id`: removes it and its subtree,
    /// as [`Tree::remove`] does, and gives focus back if it was the active
    /// modal (see the [module documentation](self#giving-focus-back)).
    /// Returns the node focus went back to, or `None` if it went back to
    /// none. The overlays it hosted are hosted by its host from then on.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree, and
    /// [`UiError::NotOverlay`] if it is not an overlay's root.
    pub fn close_overlay(&mut self, id: NodeId) -> Result<Option<NodeId>, UiError> {
        let at = self.overlay_index(id)?;
        let active = self.active_modal_index() == Some(at);
        let (restore, host, modal) = self.overlays.get(at).map_or((None, None, false), |layer| {
            (layer.restore, layer.host, layer.kind == OverlayKind::Modal)
        });
        let inside = self.focus.is_some_and(|focus| self.is_within(focus, id));
        // A toast closing under the pointer pins the column of toasts
        // until the pointer moves past the slop, so that none slides under
        // it ([`Toast`](crate::widgets::Toast)).
        if self
            .get(id)
            .and_then(|node| node.widget.as_deref())
            .is_some_and(crate::widgets::is_toast)
            && self
                .state(id)
                .is_some_and(|state| state.hovered || state.pressed)
        {
            self.toast_pin = self.pointer;
        }
        // The overlays above that would give focus back into this one, or,
        // for the nearest modal, to no node or a removed one, give it back
        // where this one would have.
        let mut nearest = modal;
        let mut inherit = Vec::new();
        for (above, layer) in self.overlays.iter().enumerate().skip(at + 1) {
            let is_modal = layer.kind == OverlayKind::Modal;
            if !is_modal && layer.restore.is_none() {
                continue;
            }
            let into = layer
                .restore
                .is_some_and(|remembered| self.is_within(remembered, id));
            let lost = layer
                .restore
                .is_none_or(|remembered| !self.contains(remembered));
            if into || (nearest && is_modal && lost) {
                inherit.push(above);
            }
            if is_modal {
                nearest = false;
            }
        }
        for above in inherit {
            if let Some(layer) = self.overlays.get_mut(above) {
                layer.restore = restore;
            }
        }
        // The active modal closing while focus is on an overlay above it,
        // such as a toast's button: focus stays there, and that overlay
        // gives it back where the modal would have when it closes.
        if active
            && !inside
            && let Some(focus) = self.focus
            && let Some(top) = self.layer_root(focus)
            && let Some(layer) = self
                .overlays
                .iter_mut()
                .skip(at + 1)
                .find(|layer| layer.root == top)
            && layer.restore.is_none()
        {
            layer.restore = restore;
        }
        for layer in &mut self.overlays {
            if layer.host == Some(id) {
                layer.host = host;
            }
        }
        self.overlays.remove(at);
        self.remove_subtree(id);
        let mut restored = None;
        // A modal remembers a node, and so does an overlay given one when
        // a modal under it closed with focus in it.
        if (active || (inside && !modal))
            && self.focus.is_none()
            && let Some(node) = restore.filter(|&node| self.can_focus(node))
        {
            self.change_focus(Some(node), self.focus_visible);
            restored = Some(node);
        }
        self.settle_layers();
        Ok(restored)
    }

    /// The overlays' roots, from the bottom to the top: the order they are
    /// laid out, painted and listed in (see the
    /// [module documentation](self#layers)).
    pub fn overlays(&self) -> Vec<NodeId> {
        self.overlays.iter().map(|layer| layer.root).collect()
    }

    /// The kind of the overlay whose root is `id`; `None` if it is not one.
    pub fn overlay_kind(&self, id: NodeId) -> Option<OverlayKind> {
        self.layer(id).map(|layer| layer.kind)
    }

    /// The anchor of the overlay whose root is `id`; `None` if it is not
    /// one.
    pub fn overlay_anchor(&self, id: NodeId) -> Option<Anchor> {
        self.layer(id).map(|layer| layer.anchor)
    }

    /// Places the overlay whose root is `id` by `anchor` from the next
    /// layout, which lays the tree out again.
    ///
    /// If that gives it another host (see the
    /// [module documentation](self#layers)), it moves, with the overlays it
    /// hosts, right after its new host's layer and the overlays already
    /// there, or to the top for no host, and is live or inert with them
    /// from then on.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if `id` or the anchor's node is not in the
    /// tree, [`UiError::NotOverlay`] if `id` is not an overlay's root,
    /// [`UiError::FixedAnchor`] if it is a toast's, which
    /// [`Toast`](crate::widgets::Toast) places, and [`UiError::BadAnchor`]
    /// if the anchor's node is in this overlay or in one placed beside a
    /// node of it, or beside one of those, and so on: each would be placed
    /// from where the other is. Nothing changes then.
    pub fn set_overlay_anchor(&mut self, id: NodeId, anchor: Anchor) -> Result<(), UiError> {
        let at = self.overlay_index(id)?;
        if self
            .get(id)
            .and_then(|node| node.widget.as_deref())
            .is_some_and(crate::widgets::is_toast)
        {
            return Err(UiError::FixedAnchor { id });
        }
        let kind = self
            .overlays
            .get(at)
            .map_or(OverlayKind::Modal, |layer| layer.kind);
        let block = self.block_end(at);
        let anchor_bounds = match anchor.target() {
            Some(target) => {
                let top = self
                    .layer_root(target)
                    .ok_or(UiError::UnknownNode { id: target })?;
                if self.placed_after(top, id) {
                    return Err(UiError::BadAnchor { id: target });
                }
                self.bounds(target).unwrap_or_default()
            }
            None => Rect::ZERO,
        };
        let host = self.host_for(kind, anchor)?;
        if let Some(layer) = self.overlays.get_mut(at) {
            layer.anchor = anchor;
            layer.anchor_bounds = anchor_bounds;
        }
        let moved = self
            .overlays
            .get(at)
            .is_some_and(|layer| layer.host != host);
        if moved {
            let mut taken: Vec<Layer> = self.overlays.drain(at..block).collect();
            if let Some(first) = taken.first_mut() {
                first.host = host;
            }
            let roots: Vec<NodeId> = taken.iter().map(|layer| layer.root).collect();
            let to = host.map_or(self.overlays.len(), |host| self.group_end(host));
            self.overlays.splice(to..to, taken);
            self.restacked(&roots);
            self.settle_layers();
        }
        self.layout_dirty = true;
        Ok(())
    }

    /// Forgets the node a closed modal handed to an overlay above it once
    /// focus is no longer in that overlay: it gives focus back only to
    /// focus that came to it from the modal (see
    /// [Giving focus back](self#giving-focus-back)). Focus on no node
    /// keeps it, so that the overlay closing with focus in it, which
    /// clears focus first, still gives it back.
    pub(crate) fn forget_handed(&mut self) {
        let Some(focus) = self.focus else {
            return;
        };
        let top = self.layer_root(focus);
        for layer in &mut self.overlays {
            if layer.kind != OverlayKind::Modal
                && layer.restore.is_some()
                && top != Some(layer.root)
            {
                layer.restore = None;
            }
        }
    }

    /// Makes `node` the one that takes focus while the modal overlay whose
    /// root is `id` is active and no node has it, in place of the root.
    pub(crate) fn set_modal_default(&mut self, id: NodeId, node: NodeId) {
        if let Some(layer) = self.overlays.iter_mut().find(|layer| layer.root == id) {
            layer.default = Some(node);
        }
        self.settle_focus();
    }

    /// The place in the order the overlays opened in of the overlay whose
    /// root is `id`.
    pub(crate) fn overlay_seq_of(&self, id: NodeId) -> Option<u64> {
        self.layer(id).map(|layer| layer.seq)
    }

    /// The lift of the overlay whose root is `id` (see [`Layer::lift`]).
    pub(crate) fn overlay_lift(&self, id: NodeId) -> Option<f32> {
        self.layer(id).map(|layer| layer.lift)
    }

    /// Lifts the window-anchored overlay whose root is `id` by `lift`
    /// units from the next layout; returns whether that changed it.
    pub(crate) fn set_overlay_lift(&mut self, id: NodeId, lift: f32) -> bool {
        let Some(layer) = self.overlays.iter_mut().find(|layer| layer.root == id) else {
            return false;
        };
        let lift = crate::layout::length(lift);
        if layer.lift == lift {
            return false;
        }
        layer.lift = lift;
        self.layout_dirty = true;
        true
    }

    /// Damages where the overlays whose roots are `roots` paint at the next
    /// frame, even if they paint the same: they moved in the order, so what
    /// they cover, or what covers them, changed.
    pub(crate) fn restacked(&mut self, roots: &[NodeId]) {
        for &root in roots {
            for inside in self.subtree(root) {
                if let Some(node) = self.get_mut(inside) {
                    node.forced = true;
                }
                self.mark(inside);
            }
        }
    }

    /// The host an overlay of `kind` placed by `anchor` would have: the
    /// root of the anchor node's layer, unless it is modal or anchored to
    /// the window.
    fn host_for(&self, kind: OverlayKind, anchor: Anchor) -> Result<Option<NodeId>, UiError> {
        match (kind, anchor.target()) {
            (OverlayKind::Modal, _) | (_, None) => Ok(None),
            (_, Some(target)) => self
                .layer_root(target)
                .map(Some)
                .ok_or(UiError::UnknownNode { id: target }),
        }
    }

    /// Returns `true` if the layer whose root is `layer` is placed from
    /// where the overlay whose root is `id` is: it is that overlay, or it
    /// is anchored to a node of it, or to a node of a layer that is, and so
    /// on. An anchor since removed places nothing.
    pub(crate) fn placed_after(&self, layer: NodeId, id: NodeId) -> bool {
        let mut next = Some(layer);
        // Each overlay has one anchor, so the chain is at most as long as
        // the list before it comes back to where it started.
        for _ in 0..=self.overlays.len() {
            let Some(at) = next else {
                return false;
            };
            if at == id {
                return true;
            }
            next = self
                .layer(at)
                .and_then(|layer| layer.anchor.target())
                .and_then(|target| self.layer_root(target));
        }
        false
    }

    /// The layer an overlay at `at` in the order is placed from: its
    /// anchor node's, while that node is in the tree.
    pub(crate) fn anchor_layer(&self, at: usize) -> Option<NodeId> {
        let target = self.overlays.get(at)?.anchor.target()?;
        self.layer_root(target)
    }

    /// Returns `true` if the overlay at `at` is hosted by the layer whose
    /// root is `host`, or by an overlay hosted by it, and so on.
    fn hosted_by(&self, at: usize, host: NodeId) -> bool {
        let mut next = self.overlays.get(at).and_then(|layer| layer.host);
        // Hosts come before the overlays they host, so the chain is at most
        // as long as the list.
        for _ in 0..=self.overlays.len() {
            match next {
                None => return false,
                Some(root) if root == host => return true,
                Some(root) => next = self.layer(root).and_then(|layer| layer.host),
            }
        }
        false
    }

    /// Where an overlay hosted by the layer whose root is `host` goes: past
    /// that layer and every overlay it hosts.
    pub(crate) fn group_end(&self, host: NodeId) -> usize {
        let mut end = if host == self.root() {
            0
        } else {
            match self.overlays.iter().position(|layer| layer.root == host) {
                Some(at) => at + 1,
                None => return self.overlays.len(),
            }
        };
        while end < self.overlays.len() && self.hosted_by(end, host) {
            end += 1;
        }
        end
    }

    /// The end of the overlay at `at` and the overlays it hosts, which
    /// follow it.
    pub(crate) fn block_end(&self, at: usize) -> usize {
        match self.overlays.get(at) {
            Some(layer) => self.group_end(layer.root),
            None => at,
        }
    }

    /// The root of the overlay that holds the node; `None` if the node is
    /// in the main root's layer or not in the tree.
    pub fn overlay_of(&self, id: NodeId) -> Option<NodeId> {
        let top = self.layer_root(id)?;
        self.layer(top).map(|layer| layer.root)
    }

    /// The active modal's root: the topmost modal overlay whose root is
    /// shown, if any.
    pub fn modal(&self) -> Option<NodeId> {
        self.active_modal_index()
            .and_then(|at| self.overlays.get(at))
            .map(|layer| layer.root)
    }

    /// Whether the node is inert: in a layer below the active modal.
    /// `None` for an unknown node.
    pub fn inert(&self, id: NodeId) -> Option<bool> {
        self.layer_state(id).map(|(live, _)| !live)
    }

    /// The overlay whose root is `id`, if it is one.
    fn layer(&self, id: NodeId) -> Option<&Layer> {
        self.overlays.iter().find(|layer| layer.root == id)
    }

    /// Where the overlay whose root is `id` is in the order.
    fn overlay_index(&self, id: NodeId) -> Result<usize, UiError> {
        if !self.contains(id) {
            return Err(UiError::UnknownNode { id });
        }
        self.overlays
            .iter()
            .position(|layer| layer.root == id)
            .ok_or(UiError::NotOverlay { id })
    }

    /// Returns `true` if `id` is an overlay's root.
    pub(crate) fn is_overlay_root(&self, id: NodeId) -> bool {
        self.layer(id).is_some()
    }

    /// The root of the node's layer: its outermost ancestor-or-self.
    pub(crate) fn layer_root(&self, id: NodeId) -> Option<NodeId> {
        let mut at = id;
        loop {
            match self.get(at)?.parent {
                Some(parent) => at = parent,
                None => return Some(at),
            }
        }
    }

    /// Where the active modal is in the order: the last modal overlay
    /// whose root is shown.
    pub(crate) fn active_modal_index(&self) -> Option<usize> {
        self.overlays.iter().rposition(|layer| {
            layer.kind == OverlayKind::Modal
                && self.get(layer.root).is_some_and(|root| !root.sane.hidden)
        })
    }

    /// Whether the node's layer is live, and its kind (`None` for the main
    /// root's); `None` for an unknown node.
    fn layer_state(&self, id: NodeId) -> Option<(bool, Option<OverlayKind>)> {
        if self.overlays.is_empty() {
            return self.contains(id).then_some((true, None));
        }
        let top = self.layer_root(id)?;
        let modal = self.active_modal_index();
        if top == self.root() {
            return Some((modal.is_none(), None));
        }
        let at = self.overlays.iter().position(|layer| layer.root == top)?;
        let live = modal.is_none_or(|modal| at >= modal);
        Some((live, self.overlays.get(at).map(|layer| layer.kind)))
    }

    /// Returns `true` if the node is live and not in a pass-through
    /// overlay, so it may be hit, pressed, focused and sent events.
    pub(crate) fn takes_input(&self, id: NodeId) -> bool {
        self.layer_state(id)
            .is_some_and(|(live, kind)| live && kind != Some(OverlayKind::PassThrough))
    }

    /// The live layers' roots, in order, with each one's kind (`None` for
    /// the main root's).
    pub(crate) fn live_layers(&self) -> Vec<(NodeId, Option<OverlayKind>)> {
        let overlays = self
            .overlays
            .iter()
            .map(|layer| (layer.root, Some(layer.kind)));
        match self.active_modal_index() {
            Some(modal) => overlays.skip(modal).collect(),
            None => iter::once((self.root(), None)).chain(overlays).collect(),
        }
    }

    /// Clears focus and releases a press that are no longer on a node that
    /// takes input, and finds the hovered path again, after the layers or
    /// the active modal changed.
    pub(crate) fn settle_layers(&mut self) {
        self.order_toasts();
        if self.focus.is_some_and(|focus| !self.takes_input(focus)) {
            self.drop_focus();
        }
        if self
            .capture
            .is_some_and(|capture| !self.takes_input(capture))
        {
            self.release_capture();
        }
        // Once another modal is active, or none, no press goes anywhere
        // until the pointer moves past the slop (ADR-0055, ADR-0056).
        let modal = self.modal();
        if modal != self.settled_modal {
            self.settled_modal = modal;
            self.press_guard = Some(PressGuard { at: self.pointer });
        }
        self.settle_focus();
        self.refresh_hover();
    }

    /// While a modal is active and no node has focus, gives focus to its
    /// default, the node it named ([`Layer::default`]) or its root, if
    /// that can take it, showing as focus last did; see
    /// [Giving focus back](self#giving-focus-back).
    pub(crate) fn settle_focus(&mut self) {
        if self.focus.is_some() {
            return;
        }
        let Some(layer) = self
            .active_modal_index()
            .and_then(|at| self.overlays.get(at))
        else {
            return;
        };
        let (default, root) = (layer.default, layer.root);
        let node = default
            .filter(|&node| self.can_focus(node))
            .or_else(|| self.can_focus(root).then_some(root));
        if let Some(node) = node {
            self.change_focus(Some(node), self.focus_visible);
        }
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::rng::Pcg32;
    use tidepool_core::testing::check_sized;

    use super::*;
    use crate::a11y::{Role, Semantics};
    use crate::layout::Style;

    fn button(name: &str) -> Node {
        Node::new(Style::default().fixed(20.0, 10.0))
            .with_semantics(Semantics::new(Role::Button, name))
            .focusable()
    }

    const KINDS: [OverlayKind; 3] = [
        OverlayKind::Modal,
        OverlayKind::Modeless,
        OverlayKind::PassThrough,
    ];

    #[test]
    fn the_overlay_list_stays_in_step_with_the_roots() {
        check_sized(
            "overlay records and roots",
            200,
            |rng: &mut Pcg32, size| (rng.next_u32(), size),
            |&(seed, size)| {
                let mut rng = Pcg32::new(u64::from(seed), 7);
                let mut tree = Tree::new();
                let mut ids = vec![tree.root()];
                for _ in 0..size {
                    let pick = *rng.choose(&ids).expect("ids");
                    match rng.range(0..5u32) {
                        0 => {
                            let kind = *rng.choose(&KINDS).expect("kinds");
                            let anchor = if rng.range(0..2u32) == 0 {
                                Anchor::window(Align::Center, Align::Center, 4.0)
                            } else {
                                Anchor::node(pick, Side::Below, Align::Start, 2.0)
                            };
                            if let Ok(id) = tree.open_overlay(Node::default(), kind, anchor) {
                                ids.push(id);
                            }
                        }
                        1 => {
                            if let Ok(id) = tree.insert(pick, button("b")) {
                                ids.push(id);
                            }
                        }
                        2 => {
                            let _ = tree.remove(pick);
                        }
                        3 => {
                            let _ = tree.close_overlay(pick);
                        }
                        _ => {
                            let hidden = rng.range(0..2u32) == 0;
                            let style = if hidden {
                                Style::default().hidden()
                            } else {
                                Style::default()
                            };
                            let _ = tree.set_style(pick, style);
                        }
                    }
                    // Every record's root is in the tree, without a parent,
                    // once; every parentless node is the main root or a
                    // record's root.
                    let roots = tree.overlays();
                    for (k, &root) in roots.iter().enumerate() {
                        if !tree.contains(root) || tree.parent(root).is_some() {
                            return Err(format!("{root} is not a root in the tree"));
                        }
                        if roots.iter().skip(k + 1).any(|&other| other == root) {
                            return Err(format!("{root} is listed twice"));
                        }
                    }
                    for id in tree.ids() {
                        if tree.parent(id).is_none() && id != tree.root() && !roots.contains(&id) {
                            return Err(format!("{id} is a root with no record"));
                        }
                    }
                    // Each hosted overlay comes after its host, and every
                    // overlay between them is hosted by that host too.
                    for (k, layer) in tree.overlays.iter().enumerate() {
                        let Some(host) = layer.host else { continue };
                        let from = if host == tree.root() {
                            0
                        } else {
                            match roots.iter().position(|&root| root == host) {
                                Some(at) if at < k => at + 1,
                                _ => return Err(format!("{} is not after its host", layer.root)),
                            }
                        };
                        if !(from..k).all(|between| tree.hosted_by(between, host)) {
                            return Err(format!("{}'s host's group is split", layer.root));
                        }
                    }
                    // Focus and the press are on nodes that take input.
                    if let Some(focus) = tree.focused()
                        && !tree.takes_input(focus)
                    {
                        return Err(format!("focus on {focus}, which takes no input"));
                    }
                    if let Some(capture) = tree.capture
                        && !tree.takes_input(capture)
                    {
                        return Err(format!("a press on {capture}, which takes no input"));
                    }
                }
                Ok(())
            },
        );
    }

    #[test]
    fn an_unknown_or_plain_node_is_not_an_overlay() {
        let mut tree = Tree::new();
        let root = tree.root();
        let plain = tree.insert(root, Node::default()).expect("a node");
        assert_eq!(
            tree.close_overlay(root),
            Err(UiError::NotOverlay { id: root })
        );
        assert_eq!(
            tree.close_overlay(plain),
            Err(UiError::NotOverlay { id: plain })
        );
        tree.remove(plain).expect("removed");
        assert_eq!(
            tree.close_overlay(plain),
            Err(UiError::UnknownNode { id: plain })
        );
        assert_eq!(tree.overlay_of(root), None);
        assert_eq!(tree.overlay_of(plain), None);
        assert_eq!(tree.inert(plain), None);
        assert_eq!(tree.inert(root), Some(false));
        assert_eq!(tree.layer_root(plain), None);
    }
}
