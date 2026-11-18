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

//! Keyboard focus: the [`Tree`] methods that read and move it, its tab
//! order, and whether it shows.
//!
//! New in Tidepool; not ported.
//!
//! At most one node has focus. A node can take it if it is focusable, and
//! it and its ancestors are enabled and shown; the tab order is those
//! nodes in pre-order. Disabling, hiding or removing the focused node (or
//! an ancestor), or making it not focusable, clears focus.
//!
//! Whether focus *shows* follows CSS's `:focus-visible`: it shows when it
//! came from the keyboard. A key that is not a modifier, pressed without
//! Ctrl, Alt or Super, makes it show; a primary click hides it; moving
//! focus with [`Tree::focus_next`] and [`Tree::focus_previous`] shows it,
//! and [`Tree::set_focus`] says. [`NodeState::focus_visible`] is set on
//! the focused node while it shows (the spec's 2 px `teal` outline).
//!
//! A change of focus repaints the old and new focused nodes and their
//! ancestors, whose [`NodeState::focus_within`] changes.
//!
//! [`NodeState::focus_visible`]: crate::tree::NodeState::focus_visible
//! [`NodeState::focus_within`]: crate::tree::NodeState::focus_within

use crate::error::UiError;
use crate::tree::{NodeId, Tree};

impl Tree {
    /// The nodes that can take focus, in pre-order: focusable, and with no
    /// disabled or hidden ancestor-or-self.
    pub fn tab_order(&self) -> Vec<NodeId> {
        let mut order = Vec::new();
        let mut stack = vec![self.root()];
        while let Some(id) = stack.pop() {
            let Some(node) = self.get(id) else {
                continue;
            };
            if node.sane.hidden || !node.enabled {
                continue;
            }
            if node.focusable {
                order.push(id);
            }
            stack.extend(node.children.iter().rev());
        }
        order
    }

    /// The focused node, if any.
    pub const fn focused(&self) -> Option<NodeId> {
        self.focus
    }

    /// Whether a node has focus and it should show.
    pub const fn focus_visible(&self) -> bool {
        self.focus.is_some() && self.focus_visible
    }

    /// Focuses the node; focus shows if `visible`.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownNode`] if the node is not in the tree, and
    /// [`UiError::NotFocusable`] if it cannot take focus.
    pub fn set_focus(&mut self, id: NodeId, visible: bool) -> Result<(), UiError> {
        if !self.contains(id) {
            return Err(UiError::UnknownNode { id });
        }
        if !self.can_focus(id) {
            return Err(UiError::NotFocusable { id });
        }
        self.change_focus(Some(id), visible);
        Ok(())
    }

    /// Clears focus: no node has it.
    pub fn clear_focus(&mut self) {
        self.change_focus(None, self.focus_visible);
    }

    /// Moves focus to the next node in the tab order, after the focused
    /// node, or to the first if none is focused, wrapping at the end. Focus
    /// shows, and the node is scrolled into view at the next layout
    /// ([`scroll_into_view`](Tree::scroll_into_view)). Returns the node, or
    /// `None`, changing nothing, if no node can take focus.
    pub fn focus_next(&mut self) -> Option<NodeId> {
        self.step_focus(true)
    }

    /// Moves focus to the previous node in the tab order, or to the last if
    /// none is focused, wrapping at the start, as
    /// [`focus_next`](Tree::focus_next) does.
    pub fn focus_previous(&mut self) -> Option<NodeId> {
        self.step_focus(false)
    }

    /// [`focus_next`](Tree::focus_next) if `forward`, else
    /// [`focus_previous`](Tree::focus_previous).
    fn step_focus(&mut self, forward: bool) -> Option<NodeId> {
        let order = self.tab_order();
        let len = order.len();
        if len == 0 {
            return None;
        }
        let at = self
            .focus
            .and_then(|focus| order.iter().position(|&id| id == focus));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % len,
            (Some(at), false) => (at + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        };
        let id = *order.get(next)?;
        self.change_focus(Some(id), true);
        // The node is in the tree, so this cannot fail. It takes effect at
        // the next layout, which sees where the node then is.
        let _ = self.scroll_into_view(id);
        Some(id)
    }

    /// Returns `true` if the node can take focus: it is focusable, and it
    /// and its ancestors are enabled and shown.
    pub(crate) fn can_focus(&self, id: NodeId) -> bool {
        self.get(id).is_some_and(|node| node.focusable)
            && self.is_enabled(id)
            && self.is_visible(id)
    }

    /// Gives `focus` focus, showing if `visible`, and repaints what that
    /// changes.
    pub(crate) fn change_focus(&mut self, focus: Option<NodeId>, visible: bool) {
        let old = self.focus;
        if old != focus {
            self.focus_moves = self.focus_moves.wrapping_add(1);
            if let Some(old) = old {
                self.mark_path(old);
            }
            if let Some(new) = focus {
                self.mark_path(new);
            }
        } else if self.focus_visible != visible
            && let Some(focus) = focus
        {
            self.mark(focus);
        }
        self.focus = focus;
        self.focus_visible = visible;
    }

    /// Sets whether focus shows, keeping the focused node.
    pub(crate) fn set_focus_visible(&mut self, visible: bool) {
        self.change_focus(self.focus, visible);
    }
}
