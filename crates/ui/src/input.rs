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

//! Input routing: [`Tree::handle`] takes one `core::input` event, routes
//! it to the widgets it concerns, and applies the tree's defaults to what
//! none of them handled; [`Tree::hit_test`] finds the node under a point.
//!
//! New in Tidepool; not ported.
//!
//! # Routing
//!
//! An event goes to a *path* of nodes, from a target up to the root, and
//! to each node on it that is enabled (it and its ancestors) and has a
//! widget, in that order, until a widget returns [`Handled::Yes`] (ADR-0046):
//!
//! - Key and text events go to the focused node's path, or, when no node
//!   has focus, to the active modal's root ([`overlay`](crate::overlay)),
//!   or the main root's when no modal is open; but Escape goes first to an
//!   open tooltip that takes input and is still anchored to its shown
//!   node, the topmost, which closes without moving focus ([`Tooltip`]).
//!   A key that is not a modifier, pressed without Ctrl, Alt or Super,
//!   first makes focus show; it moves no focus, so a tooltip that follows
//!   the pointer keeps following it.
//!   If no widget handles Tab (with at most Shift), focus moves to the next
//!   node in the tab order, or the previous with Shift, which an active
//!   modal keeps to the live layers.
//! - Every pointer event (a move, a press, a release, the wheel) records
//!   where the pointer is, and the hovered path there: the
//!   [hit](Tree::hit_test) node's. Each [layout](Tree::layout) finds that
//!   path again, so the hover follows content that scrolls or moves under
//!   a pointer at rest. A move goes to the path of the node a press
//!   captured, if any, else to the hovered path.
//! - After the active modal changed, until the pointer has moved more than
//!   [`PRESS_SLOP`] device pixels, a press counts as on the active modal's
//!   root, or on nothing if none is active
//!   ([`overlay`](crate::overlay#kinds), ADR-0055, ADR-0056): it goes to
//!   no widget and comes back blocked, as does a release no press
//!   captured. A pointer event at a position that is not finite places
//!   nothing.
//! - A primary press hides focus, captures the nearest enabled node of the
//!   hit path (setting [`NodeState::pressed`] on it and its ancestors),
//!   moves focus to the nearest node of the path that can take it, without
//!   showing it (if there is none, focus stays where it was: clicking a
//!   label in the status bar does not take focus from the editor), and
//!   goes to the hit path. Its release goes to the captured path, while
//!   `pressed` is still set, and then releases the capture. Other buttons
//!   go to the hit path, with no capture and no change of focus.
//! - A wheel event goes to the hit path. If no widget handles it, the
//!   nearest node of the path that scrolls on the delta's axis and can
//!   still move that way scrolls by it ([`SCROLL_LINE`] units a line),
//!   and what it could not take goes on to the next such node. Each starts
//!   from its offset clamped to what it can scroll in the last layout, so
//!   an offset set past the end since then moves back from the end.
//!   Positive `y` scrolls toward the end. A [`Tree::scroll_into_view`]
//!   asked for since the last layout, such as Tab's, still takes effect at
//!   the next one, moving from the offsets the wheel left.
//! - Window focus events set [`Tree::window_focused`]; losing focus
//!   releases a capture. They go to no widget.
//!
//! A path never holds a node an active modal makes inert: the hit path is
//! in a live layer, focus and a press captured are cleared and released
//! when their node becomes inert, and an event on such a path would go to
//! no widget.
//!
//! A widget's handler gets an [`EventContext`], through which it emits
//! [`Action`]s for the owner, asks for focus, a repaint or a relayout, and
//! sets its node's semantics. An action is its node's, but for the command
//! palette's list, whose actions are its palette's
//! ([`Palette`](crate::widgets::Palette)).
//! The requests are applied once the handler returns. What no widget
//! handled comes back with [`Response::handled`] `false`, for the owner
//! (`editor`, `app`) to handle; but while a modal is active, a key, text
//! or pointer event nothing handled also comes back
//! [`blocked`](Response::blocked), and the owner runs none of the
//! window's shortcuts for it, as the window is inert.
//!
//! [`NodeState::pressed`]: crate::tree::NodeState::pressed

use tidepool_core::geom::{Point, Rect};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};

use crate::a11y::Semantics;
use crate::layout::{MAX_EXTENT, intersect};
use crate::overlay::{Anchor, OverlayKind};
use crate::tree::{NodeId, NodeState, Tree};
use crate::widgets::{Tooltip, is_tooltip};

/// Logical units scrolled for each line of a wheel: the UI body's 13 px at
/// a line height of 1.5, rounded.
pub const SCROLL_LINE: f32 = 20.0;

/// How far, in device pixels, the pointer must move from where it rested
/// when the active modal changed before a press goes anywhere again: as
/// far as a drag must go before it is one (ADR-0056). In logical units it
/// is this divided by the window's scale
/// ([`TextMetrics::scale`](crate::metrics::TextMetrics::scale)).
pub const PRESS_SLOP: f32 = 4.0;

/// Whether a widget handled an event; if it did, the event stops there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handled {
    /// It did: the event goes no further.
    Yes,
    /// It did not: the event goes on to the ancestors.
    No,
}

/// What a widget tells its owner happened.
///
/// Non-exhaustive: later versions may add kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ActionKind {
    /// It was activated: a button was clicked or pressed.
    Activate,
    /// Its value was edited.
    Change,
    /// Enter was pressed in a field.
    Submit,
    /// It was dismissed: Escape, or a close button.
    Dismiss,
    /// A list, tree view or table moved its selection, by a key or a
    /// press: its owner reads the row it selected (ADR-0052).
    Select,
    /// A tree view asks its owner to expand its selected row, whose
    /// children the owner's model holds (ADR-0052).
    Expand,
    /// A tree view asks its owner to collapse its selected row (ADR-0052).
    Collapse,
}

/// An action a widget emitted while handling an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Action {
    /// The widget's node.
    pub node: NodeId,
    /// What happened.
    pub kind: ActionKind,
}

/// What came of an event: the actions widgets emitted, in order, whether
/// a widget or one of the tree's defaults handled it, and whether an
/// active modal, or the press guard, kept it from the window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The actions, in the order they were emitted.
    pub actions: Vec<Action>,
    /// Whether a widget, or a default such as Tab or wheel scrolling,
    /// handled the event. If not, it is the owner's, unless it is
    /// [`blocked`](Self::blocked).
    pub handled: bool,
    /// Whether a modal overlay was active ([`Tree::modal`]) and nothing
    /// handled the event, a key, text or pointer event: it would have been
    /// the window's, which the modal makes inert. A press the press guard
    /// keeps from every widget is blocked too, after the active modal
    /// changes, closing and hiding included, until the pointer moves more
    /// than [`PRESS_SLOP`] device pixels (ADR-0055, ADR-0056), even with no
    /// modal active then. The owner treats a
    /// blocked event as one it must not act on for the window: it runs
    /// none of the window's shortcuts (save, build, close a tab) for it,
    /// as no widget below the modal would have got it either. What the
    /// owner does for the modal itself, such as closing it on its
    /// [`Dismiss`](ActionKind::Dismiss), comes as actions. Window focus
    /// events are never blocked.
    pub blocked: bool,
}

/// What the tree keeps after the active modal changed, until the pointer
/// has moved more than [`PRESS_SLOP`] device pixels from where it was then
/// (ADR-0055, ADR-0056): that place, or `None` if no pointer event had
/// placed it yet, so that the next one does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PressGuard {
    /// Where the pointer was.
    pub(crate) at: Option<Point>,
}

/// What a handler asked for, applied when it returns.
#[derive(Default)]
pub(crate) struct Requests {
    actions: Vec<ActionKind>,
    focus: bool,
    paint: bool,
    layout: bool,
    semantics: Option<Semantics>,
}

/// What a widget's [`event`](crate::tree::Widget::event) handler gets: its
/// node, bounds and state, and a way to emit actions and make requests,
/// which the tree applies once the handler returns.
pub struct EventContext<'a> {
    node: NodeId,
    bounds: Rect,
    state: NodeState,
    focus_moves: u64,
    requests: &'a mut Requests,
}

impl EventContext<'_> {
    /// The widget's node.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// The node's bounds in the last layout.
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The node's interaction state as the event arrived.
    pub const fn state(&self) -> NodeState {
        self.state
    }

    /// How many times, before this event, keyboard focus moved from one
    /// node to another, or the window lost focus: a node that sees the
    /// same count at two events kept focus between them, as a button
    /// that activates when Space comes up checks (ADR-0049). It wraps.
    pub const fn focus_moves(&self) -> u64 {
        self.focus_moves
    }

    /// Emits an action of this node for the owner, in
    /// [`Response::actions`].
    pub fn emit(&mut self, kind: ActionKind) {
        self.requests.actions.push(kind);
    }

    /// Asks for keyboard focus. The node gets it if it can take it; focus
    /// shows if the event was a key or text event.
    pub fn request_focus(&mut self) {
        self.requests.focus = true;
    }

    /// Asks for the node to be repainted at the next frame.
    pub fn request_paint(&mut self) {
        self.requests.paint = true;
    }

    /// Asks for the tree to be laid out again, and the node repainted, at
    /// the next frame: its size may have changed.
    pub fn request_layout(&mut self) {
        self.requests.layout = true;
    }

    /// Gives the node `semantics` once the handler returns, as
    /// [`Tree::set_semantics`] does: a field's new value, a toggle's new
    /// state (ADR-0049). Semantics that it refuses, with no role or no
    /// name for a focusable node, leave the node's as they were; the last
    /// call wins.
    pub fn set_semantics(&mut self, semantics: Semantics) {
        self.requests.semantics = Some(semantics);
    }
}

impl Tree {
    /// Routes `event` as the [module documentation](self) describes,
    /// using the bounds of the last layout, and returns what came of it.
    pub fn handle(&mut self, event: &InputEvent) -> Response {
        let moved = self.note_pointer(event);
        let response = self.route(event);
        if moved {
            self.recheck_dismissed();
        }
        response
    }

    /// Notes where a pointer event puts the pointer, before it is routed:
    /// a press guard set when the active modal changed, and the toasts'
    /// pin, end once the pointer has moved past the slop
    /// ([`PRESS_SLOP`] device pixels) from where it was (ADR-0056); and a
    /// tooltip follows the pointer again once it moves. Returns whether it
    /// moved to a new position. A position that is not finite is no
    /// position: it moves nothing, and nothing keeps it.
    fn note_pointer(&mut self, event: &InputEvent) -> bool {
        let position = match *event {
            InputEvent::MouseMove { position, .. }
            | InputEvent::MouseDown { position, .. }
            | InputEvent::MouseUp { position, .. }
            | InputEvent::Wheel { position, .. } => position,
            _ => return false,
        };
        if !finite(position) {
            return false;
        }
        let slop = PRESS_SLOP / self.scale;
        let away = |from: Point| {
            let (dx, dy) = (position.x - from.x, position.y - from.y);
            dx * dx + dy * dy > slop * slop
        };
        match self.press_guard {
            // No pointer event had placed it: this one says where it is.
            Some(PressGuard { at: None }) => {
                self.press_guard = Some(PressGuard { at: Some(position) });
            }
            Some(PressGuard { at: Some(at) }) if away(at) => self.press_guard = None,
            _ => {}
        }
        if self.toast_pin.is_some_and(away) {
            self.toast_pin = None;
        }
        let moved = self.pointer != Some(position);
        if moved {
            self.focus_last = false;
        }
        moved
    }

    /// Routes `event`, as [`handle`](Tree::handle) says.
    fn route(&mut self, event: &InputEvent) -> Response {
        let mut response = Response::default();
        match *event {
            InputEvent::KeyDown { key, modifiers, .. } => {
                let chord = Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER;
                if !is_modifier(key) && !modifiers.intersects(chord) {
                    self.set_focus_visible(true);
                }
                // An open tooltip takes Escape first, so that it closes
                // without moving the pointer or focus (WCAG 1.4.13); then
                // no node is the tooltip target until one of them moves to
                // another.
                if key == Key::Escape
                    && modifiers.difference(Modifiers::SHIFT).is_empty()
                    && let Some(tooltip) = self.escape_target()
                    && self.dispatch(&[tooltip], event, true, &mut response)
                {
                    if response
                        .actions
                        .iter()
                        .any(|action| action.node == tooltip && action.kind == ActionKind::Dismiss)
                    {
                        self.dismissed = Tooltip::anchor(self, tooltip);
                    }
                    response.handled = true;
                    return response;
                }
                let path = self.key_path();
                response.handled = self.dispatch(&path, event, true, &mut response);
                if !response.handled
                    && key == Key::Tab
                    && modifiers.difference(Modifiers::SHIFT).is_empty()
                {
                    let moved = if modifiers.contains(Modifiers::SHIFT) {
                        self.focus_previous()
                    } else {
                        self.focus_next()
                    };
                    response.handled = moved.is_some();
                }
            }
            InputEvent::KeyUp { .. } | InputEvent::Text(_) => {
                let path = self.key_path();
                response.handled = self.dispatch(&path, event, true, &mut response);
            }
            InputEvent::MouseMove { position, .. } => {
                self.point_at(position);
                let path = match self.capture {
                    Some(capture) => self.path_up(capture),
                    None => self.hit_path(position),
                };
                response.handled = self.dispatch(&path, event, false, &mut response);
            }
            InputEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => {
                self.point_at(position);
                self.set_focus_visible(false);
                if self.press_guard.is_some() {
                    // As a press on the active modal's root, if any, which
                    // no widget gets: it comes back blocked.
                    self.release_capture();
                    if let Some(modal) = self.modal() {
                        if self.is_enabled(modal) {
                            self.capture = Some(modal);
                            self.mark_path(modal);
                        }
                        if self.can_focus(modal) {
                            self.change_focus(Some(modal), false);
                        }
                    }
                    response.blocked = true;
                    return response;
                }
                let path = self.hit_path(position);
                self.release_capture();
                if let Some(&target) = path.iter().find(|&&id| self.is_enabled(id)) {
                    self.capture = Some(target);
                    self.mark_path(target);
                }
                if let Some(&focus) = path.iter().find(|&&id| self.can_focus(id)) {
                    self.change_focus(Some(focus), false);
                }
                response.handled = self.dispatch(&path, event, false, &mut response);
            }
            InputEvent::MouseUp {
                button: MouseButton::Left,
                position,
                ..
            } => {
                self.point_at(position);
                let path = match self.capture {
                    Some(capture) => self.path_up(capture),
                    None if self.press_guard.is_some() => {
                        response.blocked = true;
                        return response;
                    }
                    None => self.hit_path(position),
                };
                response.handled = self.dispatch(&path, event, false, &mut response);
                self.release_capture();
            }
            InputEvent::MouseDown { position, .. } | InputEvent::MouseUp { position, .. } => {
                self.point_at(position);
                if self.press_guard.is_some() {
                    response.blocked = true;
                    return response;
                }
                let path = self.hit_path(position);
                response.handled = self.dispatch(&path, event, false, &mut response);
            }
            InputEvent::Wheel {
                delta, position, ..
            } => {
                self.point_at(position);
                let path = self.hit_path(position);
                response.handled = self.dispatch(&path, event, false, &mut response);
                if !response.handled {
                    response.handled = self.scroll_by(&path, delta);
                }
            }
            InputEvent::FocusGained => {
                self.window_focused = true;
                return response;
            }
            InputEvent::FocusLost => {
                self.window_focused = false;
                self.focus_moves = self.focus_moves.wrapping_add(1);
                self.release_capture();
                return response;
            }
        }
        response.blocked = !response.handled && self.modal().is_some();
        response
    }

    /// The node under `point`, in the last layout: the deepest shown node
    /// whose bounds hold it, the last child on top, inside every clipping
    /// ancestor, in the topmost layer that holds it.
    ///
    /// The overlays are tried from the top down, each where its root's
    /// bounds hold the point, but for a pass-through overlay or a hidden
    /// one, and a tooltip where it covers its own node's bounds, then the
    /// main root's layer, whose root fills the window, so the main root is
    /// hit where no other node is. While a modal is
    /// active, nothing below it is: a point outside the live layers hits
    /// the modal's root ([`overlay`](crate::overlay)). `None` outside the
    /// window, before the first layout, or where only a hidden main root
    /// is.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let size = self.layout_size?;
        if !Rect::new(0.0, 0.0, size.width, size.height).contains(point) {
            return None;
        }
        let modal = self.active_modal_index();
        for (at, layer) in self.overlays.iter().enumerate().rev() {
            let holds = self
                .get(layer.root)
                .is_some_and(|root| !root.sane.hidden && root.placement.rect.contains(point));
            // A tooltip forced over its own node never keeps the pointer
            // from the part of it that shows.
            let over_anchor = match layer.anchor {
                Anchor::Node { node, .. } => self.get(node).is_some_and(|anchor| {
                    intersect(anchor.placement.rect, anchor.placement.clip).contains(point)
                }),
                Anchor::Window { .. } => false,
            } && self
                .get(layer.root)
                .and_then(|root| root.widget.as_deref())
                .is_some_and(is_tooltip);
            if holds && layer.kind != OverlayKind::PassThrough && !over_anchor {
                return self.descend(layer.root, point);
            }
            if modal == Some(at) {
                return Some(layer.root);
            }
        }
        let root = self.root();
        if self.get(root)?.sane.hidden {
            return None;
        }
        self.descend(root, point)
    }

    /// The deepest shown node holding `point` from `at` down, `at` itself
    /// if none of its children does; `at` is shown and holds it.
    fn descend(&self, mut at: NodeId, point: Point) -> Option<NodeId> {
        // Children lie inside their parent on every axis it does not
        // scroll, and a parent that scrolls clips, so a subtree whose
        // root's bounds miss the point cannot hold it.
        'down: loop {
            let node = self.get(at)?;
            for &child in node.children.iter().rev() {
                if self
                    .get(child)
                    .is_some_and(|child| !child.sane.hidden && child.placement.rect.contains(point))
                {
                    at = child;
                    continue 'down;
                }
            }
            return Some(at);
        }
    }

    /// The path for key and text events: the focused node's, or the
    /// active modal's root's, or the main root's.
    fn key_path(&self) -> Vec<NodeId> {
        let target = self.focus.or_else(|| self.modal()).unwrap_or(self.root());
        self.path_up(target)
    }

    /// The node and its ancestors, from the node up.
    fn path_up(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut next = self.contains(id).then_some(id);
        while let Some(id) = next {
            path.push(id);
            next = self.parent(id);
        }
        path
    }

    /// The hit node's path at `point`, from the node up; empty if nothing
    /// is hit.
    fn hit_path(&self, point: Point) -> Vec<NodeId> {
        self.hit_test(point)
            .map_or_else(Vec::new, |hit| self.path_up(hit))
    }

    /// Records that the pointer is at `point`, and the hovered path there;
    /// a point that is not finite is no point, and changes neither.
    fn point_at(&mut self, point: Point) {
        if !finite(point) {
            return;
        }
        self.pointer = Some(point);
        self.hover_at(point);
    }

    /// Finds the hovered path again at the pointer's last position, if a
    /// pointer event gave one, in the layout just made.
    pub(crate) fn refresh_hover(&mut self) {
        if let Some(point) = self.pointer {
            self.hover_at(point);
        }
    }

    /// Records the hovered path at `point`, repainting the nodes whose
    /// [`NodeState::hovered`] changed.
    fn hover_at(&mut self, point: Point) {
        let mut path = self.hit_path(point);
        path.reverse();
        if path == self.hover {
            return;
        }
        let old = std::mem::replace(&mut self.hover, path);
        let changed: Vec<NodeId> = old
            .iter()
            .filter(|id| !self.hover.contains(id))
            .chain(self.hover.iter().filter(|id| !old.contains(id)))
            .copied()
            .collect();
        for id in changed {
            self.mark(id);
        }
    }

    /// Sends `event` along `path`, which is in one layer, until a widget
    /// handles it; to none if that layer takes no input. `keyboard` says
    /// whether it is a key or text event. Returns whether one did.
    fn dispatch(
        &mut self,
        path: &[NodeId],
        event: &InputEvent,
        keyboard: bool,
        response: &mut Response,
    ) -> bool {
        if path.first().is_some_and(|&id| !self.takes_input(id)) {
            return false;
        }
        path.iter()
            .any(|&id| self.deliver(id, event, keyboard, response) == Handled::Yes)
    }

    /// Calls the node's widget, if it has one and is enabled, and applies
    /// what it asked for.
    fn deliver(
        &mut self,
        id: NodeId,
        event: &InputEvent,
        keyboard: bool,
        response: &mut Response,
    ) -> Handled {
        if !self.is_enabled(id) {
            return Handled::No;
        }
        let Some(state) = self.state(id) else {
            return Handled::No;
        };
        let bounds = self.bounds(id).unwrap_or_default();
        let mut requests = Requests::default();
        let focus_moves = self.focus_moves;
        let Some(widget) = self.get_mut(id).and_then(|node| node.widget.as_mut()) else {
            return Handled::No;
        };
        let mut cx = EventContext {
            node: id,
            bounds,
            state,
            focus_moves,
            requests: &mut requests,
        };
        let handled = widget.event(&mut cx, event);
        let redirect = self.get(id).and_then(|node| node.actions_to);
        let from = redirect.unwrap_or(id);
        response.actions.extend(
            requests
                .actions
                .iter()
                .map(|&kind| Action { node: from, kind }),
        );
        // The node its actions are reported from learns at once what they
        // say: a palette, the row its list's press selected, so that a key
        // in the same batch moves on from it.
        if let Some(to) = redirect
            && !requests.actions.is_empty()
        {
            self.bind_palette(to);
        }
        if requests.paint {
            self.mark(id);
        }
        if requests.layout {
            self.layout_dirty = true;
            self.mark(id);
        }
        if requests.focus && self.can_focus(id) {
            self.change_focus(Some(id), keyboard);
        }
        if let Some(semantics) = requests.semantics {
            // Refused only for a focusable node without a role or a name,
            // which keeps the semantics it had, as documented.
            let _ = self.set_semantics(id, semantics);
        }
        handled
    }

    /// Scrolls the nodes of `path` that scroll, the nearest first, by the
    /// wheel's `delta`, each taking what it can from its clamped offset and
    /// passing on the rest. Returns whether any moved.
    fn scroll_by(&mut self, path: &[NodeId], delta: ScrollDelta) -> bool {
        let (x, y) = match delta {
            ScrollDelta::Lines { x, y } => (x * SCROLL_LINE, y * SCROLL_LINE),
            ScrollDelta::Logical { x, y } => (x, y),
        };
        let (mut x, mut y) = (distance(x), distance(y));
        let mut moved = false;
        for &id in path {
            if x == 0.0 && y == 0.0 {
                break;
            }
            let Some(node) = self.get(id) else {
                continue;
            };
            let style = node.sane;
            let view = node.placement.viewport.size();
            let extent = node.placement.content_size;
            let room = Point::new(extent.width - view.width, extent.height - view.height);
            // An offset set since the last layout may be past what the node
            // can scroll in it: start from it clamped.
            let mut start = node.scroll;
            if style.scroll_x {
                start.x = start.x.min(room.x.max(0.0)).max(0.0);
            }
            if style.scroll_y {
                start.y = start.y.min(room.y.max(0.0)).max(0.0);
            }
            let mut to = start;
            if style.scroll_x {
                (to.x, x) = take(start.x, x, room.x);
            }
            if style.scroll_y {
                (to.y, y) = take(start.y, y, room.y);
            }
            if to != start {
                if let Some(node) = self.get_mut(id) {
                    node.scroll = to;
                }
                self.layout_dirty = true;
                self.mark(id);
                moved = true;
            }
        }
        moved
    }
}

/// Moves `offset` by `delta` within 0 to `max`; returns the new offset and
/// what is left of the delta.
fn take(offset: f32, delta: f32, max: f32) -> (f32, f32) {
    let wanted = offset + delta;
    let to = wanted.min(max.max(0.0)).max(0.0);
    let left = if to == wanted {
        0.0
    } else {
        delta - (to - offset)
    };
    (to, left)
}

/// A wheel distance made sane: NaN is 0, and it is clamped to
/// ±[`MAX_EXTENT`].
fn distance(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-MAX_EXTENT, MAX_EXTENT)
    }
}

/// Returns `true` if both of the point's coordinates are finite.
fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// Returns `true` for Control, Shift, Alt and Super, left or right.
pub(crate) fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::LeftControl
            | Key::LeftShift
            | Key::LeftAlt
            | Key::LeftSuper
            | Key::RightControl
            | Key::RightShift
            | Key::RightAlt
            | Key::RightSuper
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_passes_on_what_it_cannot_scroll() {
        assert_eq!(take(0.0, 30.0, 100.0), (30.0, 0.0));
        assert_eq!(take(90.0, 30.0, 100.0), (100.0, 20.0));
        assert_eq!(take(10.0, -30.0, 100.0), (0.0, -20.0));
        assert_eq!(take(0.0, 5.0, -1.0), (0.0, 5.0));
        assert_eq!(distance(f32::NAN), 0.0);
        assert_eq!(distance(f32::NEG_INFINITY), -MAX_EXTENT);
    }

    #[test]
    fn modifier_keys_are_the_eight() {
        let count = Key::ALL.iter().filter(|&&key| is_modifier(key)).count();
        assert_eq!(count, 8);
        assert!(!is_modifier(Key::CapsLock));
        assert!(!is_modifier(Key::Tab));
    }
}
