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

//! Structural operations on rope trees: the chunker, grouping, `join`,
//! `split`, and the fast path for an edit inside one leaf.
//!
//! Every structural edit is built from `split` and `join`. A "tree" here is
//! a [`Child`] that satisfies the invariants with its root under the root
//! rules: a leaf of `0..=MAX_LEAF` bytes, or an internal node of
//! `2..=MAX_CHILDREN` children. `join_same` on two leaves is the only place
//! where a CR LF seam is repaired.

use std::sync::Arc;

use super::node::{Child, FILL_LEAF, MAX_CHILDREN, MAX_LEAF, MIN_LEAF, Node, Summary};

/// The largest `q <= target` that is a char boundary of `text` and not
/// between a CR and an LF.
///
/// Callers pass `target < text.len()`, so the byte after the cut is known.
/// The result is at most 4 bytes before `target` (at most 3 to reach a char
/// boundary, or 1 to step off a CR LF), so it is greater than 0 for
/// `target = FILL_LEAF`.
pub(crate) fn cut_point(text: &str, target: usize) -> usize {
    let mut cut = text.floor_char_boundary(target);
    let bytes = text.as_bytes();
    if cut > 0 && bytes.get(cut - 1) == Some(&b'\r') && bytes.get(cut) == Some(&b'\n') {
        cut -= 1;
    }
    cut
}

/// Cuts `text` into leaves: pieces of `FILL_LEAF - 4..=FILL_LEAF` bytes
/// while more than `MAX_LEAF` remain, then the rest. With more than
/// `MAX_LEAF` bytes the rest has more than `MIN_LEAF`; a text of up to
/// `2 * MAX_LEAF` bytes gives at most 3 leaves. An empty text gives none.
pub(crate) fn chunk(text: &str) -> Vec<Child> {
    let mut leaves = Vec::with_capacity(text.len() / FILL_LEAF + 1);
    let mut rest = text;
    while rest.len() > MAX_LEAF {
        let (piece, tail) = rest.split_at(cut_point(rest, FILL_LEAF));
        leaves.push(Child::leaf(piece));
        rest = tail;
    }
    if !rest.is_empty() {
        leaves.push(Child::leaf(rest));
    }
    leaves
}

/// Groups `children` (all of height `height - 1`) into ⌈c / MAX_CHILDREN⌉
/// nodes of `height` whose child counts differ by at most one. With more
/// than `MAX_CHILDREN` children each group has at least `MIN_CHILDREN`.
pub(crate) fn group(children: Vec<Child>, height: u8) -> Vec<Child> {
    let count = children.len();
    let groups = count.div_ceil(MAX_CHILDREN).max(1);
    let (base, extra) = (count / groups, count % groups);
    let mut rest = children.into_iter();
    (0..groups)
        .map(|index| {
            let take = base + usize::from(index < extra);
            Child::internal(height, rest.by_ref().take(take).collect())
        })
        .collect()
}

/// A tree over consecutive siblings: none, the one sibling, or a new node
/// holding them.
pub(crate) fn tree(children: &[Child]) -> Option<Child> {
    match children {
        [] => None,
        [only] => Some(only.clone()),
        [first, ..] => Some(Child::internal(first.node.height() + 1, children.to_vec())),
    }
}

/// One tree from 1-3 nodes of equal height: the node itself, or a new root
/// above them. Its summary is recomputed from the nodes.
fn root_of(mut nodes: Vec<Child>) -> Child {
    match nodes.len() {
        0 => Child::leaf(""),
        1 => nodes.swap_remove(0),
        _ => {
            let height = nodes[0].node.height() + 1;
            Child::internal(height, nodes)
        }
    }
}

/// The concatenation of two trees. The empty tree is the identity. A CR at
/// the end of `a` and an LF at the start of `b` end up in one leaf, so they
/// are one line break.
pub(crate) fn join(a: Child, b: Child) -> Child {
    if a.summary.bytes == 0 {
        return b;
    }
    if b.summary.bytes == 0 {
        return a;
    }
    let (ha, hb) = (a.node.height(), b.node.height());
    let nodes = match ha.cmp(&hb) {
        std::cmp::Ordering::Equal => join_same(&a, &b),
        std::cmp::Ordering::Greater => join_right(&a.node, &b),
        std::cmp::Ordering::Less => join_left(&a, &b.node),
    };
    root_of(nodes)
}

/// `join` of two optional trees.
pub(crate) fn join_opt(a: Option<Child>, b: Option<Child>) -> Option<Child> {
    match (a, b) {
        (Some(a), Some(b)) => Some(join(a, b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Joins two nodes of the same height into 1-3 nodes of that height.
///
/// Leaves are kept, shared, when both have at least `MIN_LEAF` bytes and
/// the seam is not CR|LF; otherwise their text is re-chunked. Internal
/// nodes join their facing children and regroup.
fn join_same(a: &Child, b: &Child) -> Vec<Child> {
    debug_assert_eq!(a.node.height(), b.node.height());
    match (&*a.node, &*b.node) {
        (Node::Leaf(x), Node::Leaf(y)) => {
            let seam = x.ends_with('\r') && y.starts_with('\n');
            if x.len() >= MIN_LEAF && y.len() >= MIN_LEAF && !seam {
                vec![a.clone(), b.clone()]
            } else {
                let mut both = String::with_capacity(x.len() + y.len());
                both.push_str(x);
                both.push_str(y);
                chunk(&both)
            }
        }
        (
            Node::Internal {
                height,
                children: left,
            },
            Node::Internal {
                children: right, ..
            },
        ) => {
            let (Some((a_last, a_init)), Some((b_first, b_rest))) =
                (left.split_last(), right.split_first())
            else {
                return vec![a.clone(), b.clone()];
            };
            let middle = join_same(a_last, b_first);
            let mut all = Vec::with_capacity(a_init.len() + middle.len() + b_rest.len());
            all.extend_from_slice(a_init);
            all.extend(middle);
            all.extend_from_slice(b_rest);
            group(all, *height)
        }
        // Unequal heights never get here (see the assertion above); the
        // pair still holds the right text.
        _ => vec![a.clone(), b.clone()],
    }
}

/// Joins `b` onto the right spine of `a`, which is taller: 1-2 nodes of
/// `a`'s height.
fn join_right(a: &Node, b: &Child) -> Vec<Child> {
    let Node::Internal { height, children } = a else {
        debug_assert!(matches!(a, Node::Internal { .. }), "a is taller than b");
        return Vec::new();
    };
    let Some((last, init)) = children.split_last() else {
        return Vec::new();
    };
    let middle = if last.node.height() == b.node.height() {
        join_same(last, b)
    } else {
        join_right(&last.node, b)
    };
    let mut all = Vec::with_capacity(init.len() + middle.len());
    all.extend_from_slice(init);
    all.extend(middle);
    group(all, *height)
}

/// Joins `a` onto the left spine of `b`, which is taller: 1-2 nodes of
/// `b`'s height.
fn join_left(a: &Child, b: &Node) -> Vec<Child> {
    let Node::Internal { height, children } = b else {
        debug_assert!(matches!(b, Node::Internal { .. }), "b is taller than a");
        return Vec::new();
    };
    let Some((first, rest)) = children.split_first() else {
        return Vec::new();
    };
    let middle = if first.node.height() == a.node.height() {
        join_same(a, first)
    } else {
        join_left(a, &first.node)
    };
    let mut all = Vec::with_capacity(middle.len() + rest.len());
    all.extend(middle);
    all.extend_from_slice(rest);
    group(all, *height)
}

/// Splits a tree at byte `at`, a char boundary: the trees before and after
/// it (`None` for an empty side). A CR LF may be split. O(log n) node
/// visits, because the heights of the joins telescope.
pub(crate) fn split(whole: &Child, at: usize) -> (Option<Child>, Option<Child>) {
    if at == 0 {
        return (None, Some(whole.clone()));
    }
    if at >= whole.summary.bytes {
        return (Some(whole.clone()), None);
    }
    match &*whole.node {
        Node::Leaf(text) => match (text.get(..at), text.get(at..)) {
            (Some(head), Some(tail)) => (Some(Child::leaf(head)), Some(Child::leaf(tail))),
            // Callers split at char boundaries only.
            _ => (Some(whole.clone()), None),
        },
        Node::Internal { children, .. } => {
            let mut before = 0;
            for (index, child) in children.iter().enumerate() {
                let end = before + child.summary.bytes;
                if end < at {
                    before = end;
                    continue;
                }
                let (init, rest) = children.split_at(index);
                let tail = rest.get(1..).unwrap_or_default();
                if end == at {
                    return (tree(&children[..=index]), tree(tail));
                }
                let (left, right) = split(child, at - before);
                return (join_opt(tree(init), left), join_opt(right, tree(tail)));
            }
            (Some(whole.clone()), None)
        }
    }
}

/// The leaf a left-biased descent by byte `end` reaches, with its start and
/// whether some leaf comes before or after it.
struct Located<'a> {
    leaf: &'a str,
    start: usize,
    has_before: bool,
    has_after: bool,
}

fn locate(root: &Node, end: usize) -> Located<'_> {
    let mut node = root;
    let mut start = 0;
    let (mut has_before, mut has_after) = (false, false);
    loop {
        match node {
            Node::Leaf(leaf) => {
                return Located {
                    leaf,
                    start,
                    has_before,
                    has_after,
                };
            }
            Node::Internal { children, .. } => {
                let index = child_at(children, end - start, &mut start);
                let Some(child) = children.get(index) else {
                    return Located {
                        leaf: "",
                        start,
                        has_before: true,
                        has_after: true,
                    };
                };
                has_before |= index > 0;
                has_after |= index + 1 < children.len();
                node = &child.node;
            }
        }
    }
}

/// The index of the first child whose running byte total is ≥ `local`, or
/// the last child; adds the bytes before it to `start`.
fn child_at(children: &[Child], local: usize, start: &mut usize) -> usize {
    let mut before = 0;
    for (index, child) in children.iter().enumerate() {
        if before + child.summary.bytes >= local || index + 1 == children.len() {
            *start += before;
            return index;
        }
        before += child.summary.bytes;
    }
    0
}

/// Replaces bytes `start..end` (validated, not both empty) with `text` in
/// place if the edit stays inside one leaf and keeps every invariant
/// (design §3.2, step 2). Returns whether it did; if not, nothing changed.
///
/// Only the nodes on the path that a snapshot still shares are copied.
pub(crate) fn fast_edit(
    root: &mut Arc<Node>,
    summary: &mut Summary,
    start: usize,
    end: usize,
    text: &str,
) -> bool {
    let Located {
        leaf,
        start: leaf_start,
        has_before,
        has_after,
    } = locate(root, end);
    if start < leaf_start {
        return false;
    }
    let (from, to) = (start - leaf_start, end - leaf_start);
    let Some(new_len) = (leaf.len() - (to - from)).checked_add(text.len()) else {
        return false;
    };
    let is_root = matches!(**root, Node::Leaf(_));
    let fits = (MIN_LEAF..=MAX_LEAF).contains(&new_len) || (is_root && new_len <= MAX_LEAF);
    if !fits {
        return false;
    }
    let bytes = leaf.as_bytes();
    if from == 0 && has_before {
        let first = text.as_bytes().first().or_else(|| bytes.get(to));
        if first == Some(&b'\n') {
            return false;
        }
    }
    if to == leaf.len() && has_after {
        let last = text
            .as_bytes()
            .last()
            .or_else(|| bytes.get(..from).and_then(<[u8]>::last));
        if last == Some(&b'\r') {
            return false;
        }
    }
    *summary = edit_path(root, *summary, end, from, to, text);
    true
}

/// Descends by `end` as `locate` did, making each node on the path unique,
/// edits the leaf, and fixes the summaries on the way back up. `summary`
/// is the node's summary before the edit; returns its summary after.
///
/// The leaf's summary is updated, not recounted: from the summary its
/// parent holds, less the bytes the edit removes and plus those it inserts,
/// each counted with the one byte kept either side, so that a CR LF made
/// or split at the edit's ends counts right (`Summary::of_parts`). So an
/// edit costs its own bytes, and the leaf's copy when a snapshot still
/// shares it, rather than two passes over the leaf (ADR-0065). A shared
/// leaf is copied once, edited as it is copied, into room for a full leaf,
/// rather than cloned, grown and shifted.
fn edit_path(
    node: &mut Arc<Node>,
    summary: Summary,
    end: usize,
    from: usize,
    to: usize,
    text: &str,
) -> Summary {
    if let Some(Node::Leaf(leaf)) = Arc::get_mut(node) {
        let after = edited_summary(leaf, summary, from, to, text);
        let new_len = leaf.len() - (to - from) + text.len();
        if leaf.capacity() < new_len {
            leaf.reserve_exact(MAX_LEAF.max(new_len) - leaf.len());
        }
        leaf.replace_range(from..to, text);
        return after;
    }
    if let Node::Leaf(leaf) = &**node {
        let after = edited_summary(leaf, summary, from, to, text);
        let new_len = leaf.len() - (to - from) + text.len();
        let mut copy = String::with_capacity(MAX_LEAF.max(new_len));
        copy.push_str(leaf.get(..from).unwrap_or_default());
        copy.push_str(text);
        copy.push_str(leaf.get(to..).unwrap_or_default());
        *node = Arc::new(Node::Leaf(copy));
        return after;
    }
    match Arc::make_mut(node) {
        // Edited above, shared or not.
        Node::Leaf(_) => summary,
        Node::Internal { children, .. } => {
            let mut start = 0;
            let index = child_at(children, end, &mut start);
            let Some(child) = children.get_mut(index) else {
                return summary;
            };
            let old = child.summary;
            let new = edit_path(&mut child.node, old, end - start, from, to, text);
            child.summary = new;
            summary + new - old
        }
    }
}

/// One edit of a batch ([`batch_edit`]): the bytes `start..end` of the
/// whole text replaced with `text`. `Debug` shows the text's length, never
/// the text, as a [`Rope`](super::Rope)'s does.
#[derive(Clone, Copy)]
pub(crate) struct Edit<'a> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) text: &'a str,
}

impl std::fmt::Debug for Edit<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Edit")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("text_len", &self.text.len())
            .finish()
    }
}

/// Applies `edits`, sorted, with at least one byte kept between each and
/// the next, and validated, in one walk of the tree, if each stays inside
/// one leaf and keeps every invariant, as [`fast_edit`] would edit it
/// alone. Returns whether it did; if not, nothing changed.
///
/// An edit belongs to the leaf that holds its end, as [`locate`] finds it,
/// so one at a seam is the leaf's before it. The walk goes down only into
/// the subtrees that hold edits, copying each node on the way that a
/// snapshot still shares, and rebuilds each leaf it edits once, with all
/// of its edits, its summary updated edit by edit (ADR-0065). So many
/// scattered edits cost one copy of each leaf they touch, rather than a
/// descent from the root for each.
pub(crate) fn batch_edit(root: &mut Arc<Node>, summary: &mut Summary, edits: &[Edit<'_>]) -> bool {
    let apart = edits.windows(2).all(|pair| match pair {
        [a, b] => a.end < b.start,
        _ => true,
    });
    let is_root = matches!(**root, Node::Leaf(_));
    if !apart || !batch_fits(root, 0, edits, (false, false), is_root) {
        return false;
    }
    *summary = batch_path(root, *summary, 0, edits);
    true
}

/// The edits of `edits` each child of `children`, which starts at `start`,
/// holds, as [`batch_edit`] assigns them: with the child, its start and
/// its index. An edit that starts before the child holding its end is
/// given to it anyway, for [`batch_fits`] to refuse.
fn split_edits<'e, 'a>(
    children: &[Child],
    start: usize,
    edits: &'e [Edit<'a>],
) -> Vec<(usize, usize, &'e [Edit<'a>])> {
    let mut parts = Vec::new();
    let mut rest = edits;
    let mut child_start = start;
    let last = children.len().saturating_sub(1);
    for (index, child) in children.iter().enumerate() {
        if rest.is_empty() {
            break;
        }
        let child_end = child_start + child.summary.bytes;
        let held = if index == last {
            rest.len()
        } else {
            rest.partition_point(|edit| edit.end <= child_end)
        };
        let (mine, after) = rest.split_at(held);
        if !mine.is_empty() {
            parts.push((index, child_start, mine));
        }
        rest = after;
        child_start = child_end;
    }
    parts
}

/// Whether `edits`, which end in `node` (starting at `start`), each stay
/// inside one of its leaves and keep every invariant there. `around` says
/// whether a leaf comes before and after the node.
fn batch_fits(
    node: &Node,
    start: usize,
    edits: &[Edit<'_>],
    around: (bool, bool),
    is_root: bool,
) -> bool {
    match node {
        Node::Leaf(leaf) => leaf_fits(leaf, start, edits, around, is_root),
        Node::Internal { children, .. } => {
            let last = children.len().saturating_sub(1);
            split_edits(children, start, edits)
                .into_iter()
                .all(|(index, child_start, mine)| {
                    let around = (around.0 || index > 0, around.1 || index < last);
                    children.get(index).is_some_and(|child| {
                        mine.first().is_some_and(|edit| edit.start >= child_start)
                            && batch_fits(&child.node, child_start, mine, around, false)
                    })
                })
        }
    }
}

/// Whether `edits` keep `leaf`, which starts at `start`, within the leaf
/// sizes, and split no CR LF with the leaves `around` it, as
/// [`fast_edit`] checks one edit.
fn leaf_fits(
    leaf: &str,
    start: usize,
    edits: &[Edit<'_>],
    around: (bool, bool),
    is_root: bool,
) -> bool {
    let bytes = leaf.as_bytes();
    let mut new_len = leaf.len();
    for edit in edits {
        let (from, to) = (edit.start - start, edit.end - start);
        if to > leaf.len() {
            return false;
        }
        new_len = new_len - (to - from) + edit.text.len();
    }
    let fits = (MIN_LEAF..=MAX_LEAF).contains(&new_len) || (is_root && new_len <= MAX_LEAF);
    if !fits {
        return false;
    }
    if let Some(first) = edits.first()
        && first.start == start
        && around.0
    {
        let byte = first
            .text
            .as_bytes()
            .first()
            .or_else(|| bytes.get(first.end - start));
        if byte == Some(&b'\n') {
            return false;
        }
    }
    if let Some(last) = edits.last()
        && last.end - start == leaf.len()
        && around.1
    {
        let from = last.start - start;
        let byte = last
            .text
            .as_bytes()
            .last()
            .or_else(|| bytes.get(..from).and_then(<[u8]>::last));
        if byte == Some(&b'\r') {
            return false;
        }
    }
    true
}

/// Applies `edits`, which [`batch_fits`] passed, to `node`, starting at
/// `start`, whose summary is `summary`; returns its summary after.
fn batch_path(node: &mut Arc<Node>, summary: Summary, start: usize, edits: &[Edit<'_>]) -> Summary {
    if let Node::Leaf(leaf) = &**node {
        let mut after = summary;
        let mut new_len = leaf.len();
        for edit in edits {
            let (from, to) = (edit.start - start, edit.end - start);
            // Each edit's window holds only bytes no other edit changes, as
            // a byte is kept between edits.
            after = edited_summary(leaf, after, from, to, edit.text);
            new_len = new_len - (to - from) + edit.text.len();
        }
        let mut copy = String::with_capacity(MAX_LEAF.max(new_len));
        let mut kept = 0;
        for edit in edits {
            let (from, to) = (edit.start - start, edit.end - start);
            copy.push_str(leaf.get(kept..from).unwrap_or_default());
            copy.push_str(edit.text);
            kept = to;
        }
        copy.push_str(leaf.get(kept..).unwrap_or_default());
        *node = Arc::new(Node::Leaf(copy));
        return after;
    }
    let Node::Internal { children, .. } = Arc::make_mut(node) else {
        return summary;
    };
    let mut after = summary;
    for (index, child_start, mine) in split_edits(children, start, edits) {
        if let Some(child) = children.get_mut(index) {
            let old = child.summary;
            let new = batch_path(&mut child.node, old, child_start, mine);
            child.summary = new;
            after = after + new - old;
        }
    }
    after
}

/// The summary of `leaf`, whose summary is `summary`, once bytes
/// `from..to` are replaced with `text` (see [`edit_path`]).
fn edited_summary(leaf: &str, summary: Summary, from: usize, to: usize, text: &str) -> Summary {
    let bytes = leaf.as_bytes();
    let (first, last) = (from.saturating_sub(1), (to + 1).min(bytes.len()));
    let part = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or_default();
    let removed = Summary::of_parts(&[part(first..last)]);
    let added = Summary::of_parts(&[part(first..from), text.as_bytes(), part(to..last)]);
    // Added first: each count of `removed` is part of the leaf's.
    summary + added - removed
}
