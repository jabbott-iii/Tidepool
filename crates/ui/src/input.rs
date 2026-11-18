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
//! - Key and text events go to the focused node's path, or the root's when
//!   no node has focus. A key that is not a modifier, pressed without Ctrl,
//!   Alt or Super, first makes focus show. If no widget handles Tab (with
//!   at most Shift), focus moves to the next node in the tab order, or the
//!   previous with Shift.
//! - Every pointer event (a move, a press, a release, the wheel) records
//!   where the pointer is, and the hovered path there: the
//!   [hit](Tree::hit_test) node's. Each [layout](Tree::layout) finds that
//!   path again, so the hover follows content that scrolls or moves under
//!   a pointer at rest. A move goes to the path of the node a press
//!   captured, if any, else to the hovered path.
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
//! A widget's handler gets an [`EventContext`], through which it emits
//! [`Action`]s for the owner and asks for focus, a repaint or a relayout.
//! The requests are applied once the handler returns. What no widget
//! handled comes back with [`Response::handled`] `false`, for the owner
//! (`editor`, `app`) to handle.
//!
//! [`NodeState::pressed`]: crate::tree::NodeState::pressed

use tidepool_core::geom::{Point, Rect};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};

use crate::layout::MAX_EXTENT;
use crate::tree::{NodeId, NodeState, Tree};

/// Logical units scrolled for each line of a wheel: the UI body's 13 px at
/// a line height of 1.5, rounded.
pub const SCROLL_LINE: f32 = 20.0;

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
}

/// An action a widget emitted while handling an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Action {
    /// The widget's node.
    pub node: NodeId,
    /// What happened.
    pub kind: ActionKind,
}

/// What came of an event: the actions widgets emitted, in order, and
/// whether a widget or one of the tree's defaults handled it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The actions, in the order they were emitted.
    pub actions: Vec<Action>,
    /// Whether a widget, or a default such as Tab or wheel scrolling,
    /// handled the event. If not, it is the owner's.
    pub handled: bool,
}

/// What a handler asked for, applied when it returns.
#[derive(Default)]
pub(crate) struct Requests {
    actions: Vec<ActionKind>,
    focus: bool,
    paint: bool,
    layout: bool,
}

/// What a widget's [`event`](crate::tree::Widget::event) handler gets: its
/// node, bounds and state, and a way to emit actions and make requests,
/// which the tree applies once the handler returns.
pub struct EventContext<'a> {
    node: NodeId,
    bounds: Rect,
    state: NodeState,
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
}

impl Tree {
    /// Routes `event` as the [module documentation](self) describes,
    /// using the bounds of the last layout, and returns what came of it.
    pub fn handle(&mut self, event: &InputEvent) -> Response {
        let mut response = Response::default();
        match *event {
            InputEvent::KeyDown { key, modifiers, .. } => {
                let chord = Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER;
                if !is_modifier(key) && !modifiers.intersects(chord) {
                    self.set_focus_visible(true);
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
                    None => self.hit_path(position),
                };
                response.handled = self.dispatch(&path, event, false, &mut response);
                self.release_capture();
            }
            InputEvent::MouseDown { position, .. } | InputEvent::MouseUp { position, .. } => {
                self.point_at(position);
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
            InputEvent::FocusGained => self.window_focused = true,
            InputEvent::FocusLost => {
                self.window_focused = false;
                self.release_capture();
            }
        }
        response
    }

    /// The node under `point`, in the last layout: the deepest shown node
    /// whose bounds hold it, the last child on top, inside every clipping
    /// ancestor; the root if no other node does. `None` outside the window,
    /// before the first layout, or while the root is hidden.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let size = self.layout_size?;
        if !Rect::new(0.0, 0.0, size.width, size.height).contains(point) {
            return None;
        }
        let mut at = self.root();
        if self.get(at)?.sane.hidden {
            return None;
        }
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
    /// root's.
    fn key_path(&self) -> Vec<NodeId> {
        self.path_up(self.focus.unwrap_or(self.root()))
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

    /// Records that the pointer is at `point`, and the hovered path there.
    fn point_at(&mut self, point: Point) {
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

    /// Sends `event` along `path` until a widget handles it. `keyboard`
    /// says whether it is a key or text event. Returns whether one did.
    fn dispatch(
        &mut self,
        path: &[NodeId],
        event: &InputEvent,
        keyboard: bool,
        response: &mut Response,
    ) -> bool {
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
        let Some(widget) = self.get_mut(id).and_then(|node| node.widget.as_mut()) else {
            return Handled::No;
        };
        let mut cx = EventContext {
            node: id,
            bounds,
            state,
            requests: &mut requests,
        };
        let handled = widget.event(&mut cx, event);
        response.actions.extend(
            requests
                .actions
                .iter()
                .map(|&kind| Action { node: id, kind }),
        );
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

/// Returns `true` for Control, Shift, Alt and Super, left or right.
fn is_modifier(key: Key) -> bool {
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
