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

//! The tree's arena, inside the crate: finding a node by its id, walking
//! its ancestors and its subtree, and queueing nodes to be repainted.
//!
//! New in Tidepool; not ported.

use super::{NodeData, NodeId, Tree};

impl Tree {
    /// The node, if the id names one.
    pub(crate) fn get(&self, id: NodeId) -> Option<&NodeData> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation == id.generation {
            slot.node.as_ref()
        } else {
            None
        }
    }

    /// The node, if the id names one, to change.
    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation == id.generation {
            slot.node.as_mut()
        } else {
            None
        }
    }

    /// Every node, to change.
    pub(crate) fn nodes_mut(&mut self) -> impl Iterator<Item = &mut NodeData> {
        self.slots.iter_mut().filter_map(|slot| slot.node.as_mut())
    }

    /// The ids of every node, in slot order.
    pub(crate) fn ids(&self) -> Vec<NodeId> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.node.is_some())
            .filter_map(|(index, slot)| {
                Some(NodeId {
                    index: u32::try_from(index).ok()?,
                    generation: slot.generation,
                })
            })
            .collect()
    }

    /// The node and every node below it, in pre-order.
    pub(crate) fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        let mut stack = vec![id];
        while let Some(next) = stack.pop() {
            let Some(node) = self.get(next) else {
                continue;
            };
            nodes.push(next);
            stack.extend(node.children.iter().rev());
        }
        nodes
    }

    /// Returns `true` if `node` is `ancestor` or below it.
    pub(crate) fn is_within(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut next = Some(node);
        while let Some(id) = next {
            if id == ancestor {
                return true;
            }
            next = self.get(id).and_then(|node| node.parent);
        }
        false
    }

    /// Returns `true` if the node and its ancestors are all shown.
    pub(crate) fn is_visible(&self, id: NodeId) -> bool {
        self.all_up(id, |node| !node.sane.hidden)
    }

    /// Returns `true` if the node and its ancestors are all enabled, by
    /// their own flags and by the widgets that squeeze them.
    pub(crate) fn is_enabled(&self, id: NodeId) -> bool {
        self.all_up(id, |node| node.enabled && !node.squeezed)
    }

    /// Returns `true` if the node is in the tree and `holds` for it and
    /// each of its ancestors.
    fn all_up(&self, id: NodeId, holds: impl Fn(&NodeData) -> bool) -> bool {
        let mut next = Some(id);
        let mut seen = false;
        while let Some(id) = next {
            let Some(node) = self.get(id) else {
                return false;
            };
            if !holds(node) {
                return false;
            }
            seen = true;
            next = node.parent;
        }
        seen
    }

    /// Queues the node to be repainted at the next frame.
    pub(crate) fn mark(&mut self, id: NodeId) {
        let queue = self.get_mut(id).is_some_and(|node| {
            let first = !node.queued;
            node.queued = true;
            first
        });
        if queue {
            self.paint_queue.push(id);
        }
    }

    /// Queues the node and its ancestors to be repainted.
    pub(crate) fn mark_path(&mut self, id: NodeId) {
        let mut next = Some(id);
        while let Some(id) = next {
            self.mark(id);
            next = self.get(id).and_then(|node| node.parent);
        }
    }

    /// Releases the press a node captured, if any.
    pub(crate) fn release_capture(&mut self) {
        if let Some(capture) = self.capture.take() {
            self.mark_path(capture);
        }
    }

    /// Clears focus, the hovered path and a press inside the subtree at
    /// `id`, which is leaving the tree or being hidden.
    pub(super) fn leave(&mut self, id: NodeId) {
        if self.focus.is_some_and(|focus| self.is_within(focus, id)) {
            self.drop_focus();
        }
        if self
            .capture
            .is_some_and(|capture| self.is_within(capture, id))
        {
            self.release_capture();
        }
        if let Some(at) = self.hover.iter().position(|&hovered| hovered == id) {
            self.hover.truncate(at);
        }
    }
}
