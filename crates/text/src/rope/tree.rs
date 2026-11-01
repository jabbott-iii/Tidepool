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
    let (old, new) = edit_path(root, end, from, to, text);
    *summary = *summary + new - old;
    true
}

/// Descends by `end` as `locate` did, making each node on the path unique,
/// edits the leaf, and fixes the summaries on the way back up. Returns the
/// leaf's old and new summaries.
fn edit_path(
    node: &mut Arc<Node>,
    end: usize,
    from: usize,
    to: usize,
    text: &str,
) -> (Summary, Summary) {
    match Arc::make_mut(node) {
        Node::Leaf(leaf) => {
            let old = Summary::of(leaf);
            let new_len = leaf.len() - (to - from) + text.len();
            if leaf.capacity() < new_len {
                leaf.reserve_exact(MAX_LEAF.max(new_len) - leaf.len());
            }
            leaf.replace_range(from..to, text);
            (old, Summary::of(leaf))
        }
        Node::Internal { children, .. } => {
            let mut start = 0;
            let index = child_at(children, end, &mut start);
            let Some(child) = children.get_mut(index) else {
                return (Summary::default(), Summary::default());
            };
            let (old, new) = edit_path(&mut child.node, end - start, from, to, text);
            child.summary = child.summary + new - old;
            (old, new)
        }
    }
}
