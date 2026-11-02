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

//! Rope nodes: the shape constants, summaries, the node types, descents by
//! a metric, and the invariant checker.
//!
//! Unit tests (`cfg(test)`) use a tiny shape, so that short texts already
//! have many leaves and several levels; integration tests link the normal
//! library and get the real one.

use std::ops::{Add, AddAssign, Sub};
use std::sync::Arc;

/// The most bytes in a leaf.
#[cfg(not(test))]
pub(crate) const MAX_LEAF: usize = 4096;
/// The fewest bytes in a leaf that is not the root.
#[cfg(not(test))]
pub(crate) const MIN_LEAF: usize = 1024;
/// The size a bulk build cuts leaves at, leaving room to type.
#[cfg(not(test))]
pub(crate) const FILL_LEAF: usize = 3072;
/// The most children of an internal node.
#[cfg(not(test))]
pub(crate) const MAX_CHILDREN: usize = 16;

/// The most bytes in a leaf (tiny test shape).
#[cfg(test)]
pub(crate) const MAX_LEAF: usize = 16;
/// The fewest bytes in a non-root leaf (tiny test shape).
#[cfg(test)]
pub(crate) const MIN_LEAF: usize = 4;
/// The size a bulk build cuts leaves at (tiny test shape).
#[cfg(test)]
pub(crate) const FILL_LEAF: usize = 12;
/// The most children of an internal node (tiny test shape).
#[cfg(test)]
pub(crate) const MAX_CHILDREN: usize = 4;

/// The fewest children of an internal node that is not the root.
pub(crate) const MIN_CHILDREN: usize = MAX_CHILDREN / 2;

// A cut always leaves a tail of at least MIN_LEAF, and a builder's short
// tail fits into the leaf before it.
const _: () = assert!(FILL_LEAF + MIN_LEAF <= MAX_LEAF);
// A cut moves back at most 4 bytes (3 to a char boundary, or 1 off a CR LF).
const _: () = assert!(FILL_LEAF >= MIN_LEAF + 4);
// A leaf holds any char; regrouping always has at least two children.
const _: () = assert!(MIN_LEAF >= 4 && MIN_CHILDREN >= 2 && MAX_CHILDREN == 2 * MIN_CHILDREN);

/// Sizes of a piece of text in each unit, and its line breaks.
///
/// Summaries of consecutive leaves add, because no CR LF is ever split
/// between two leaves (invariant 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    /// UTF-8 bytes.
    pub(crate) bytes: usize,
    /// Chars (Unicode scalar values).
    pub(crate) chars: usize,
    /// UTF-16 code units.
    pub(crate) utf16: usize,
    /// Line breaks: LF, CR LF (one break) or a lone CR.
    pub(crate) breaks: usize,
}

impl Summary {
    /// Counts `text` in one pass of plain byte loops.
    ///
    /// - chars: the bytes that are not continuation bytes (`0b10xx_xxxx`);
    /// - UTF-16 units: chars plus one for each 4-byte char (lead byte ≥ 0xF0);
    /// - breaks: LFs + CRs − (CRs directly followed by an LF).
    pub(crate) fn of(text: &str) -> Self {
        let mut chars = 0;
        let mut astral = 0;
        let mut lf = 0;
        let mut cr = 0;
        let mut crlf = 0;
        let mut after_cr = false;
        for &byte in text.as_bytes() {
            chars += usize::from(byte & 0xC0 != 0x80);
            astral += usize::from(byte >= 0xF0);
            let is_lf = byte == b'\n';
            lf += usize::from(is_lf);
            crlf += usize::from(is_lf && after_cr);
            after_cr = byte == b'\r';
            cr += usize::from(after_cr);
        }
        Self {
            bytes: text.len(),
            chars,
            utf16: chars + astral,
            breaks: lf + cr - crlf,
        }
    }

    /// The count of `metric`.
    pub(crate) fn get(self, metric: Metric) -> usize {
        match metric {
            Metric::Bytes => self.bytes,
            Metric::Chars => self.chars,
            Metric::Utf16 => self.utf16,
            Metric::Breaks => self.breaks,
        }
    }
}

impl Add for Summary {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            bytes: self.bytes + other.bytes,
            chars: self.chars + other.chars,
            utf16: self.utf16 + other.utf16,
            breaks: self.breaks + other.breaks,
        }
    }
}

impl AddAssign for Summary {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl Sub for Summary {
    type Output = Self;

    /// Field-wise difference; `other` must be part of `self`.
    fn sub(self, other: Self) -> Self {
        Self {
            bytes: self.bytes - other.bytes,
            chars: self.chars - other.chars,
            utf16: self.utf16 - other.utf16,
            breaks: self.breaks - other.breaks,
        }
    }
}

/// The line breaks of `leaf`, which has `total` of them, that end at or
/// before `off`: those of `leaf[..off]`, except a CR at `off - 1` whose LF
/// is at `off`, because that CR LF ends after `off`. Scans from the nearer
/// end of the leaf.
pub(crate) fn breaks_before(leaf: &str, total: usize, off: usize) -> usize {
    let bytes = leaf.as_bytes();
    if off > bytes.len() / 2 {
        // The breaks ending after `off` are the LFs at or after it and the
        // CRs at or after it that no LF follows (a CR LF counts at its LF).
        let tail = bytes.get(off..).unwrap_or_default();
        let mut after = 0;
        for (index, &byte) in tail.iter().enumerate() {
            let lone_cr = byte == b'\r' && tail.get(index + 1) != Some(&b'\n');
            after += usize::from(byte == b'\n' || lone_cr);
        }
        return total.saturating_sub(after);
    }
    let head = bytes.get(..off).unwrap_or(bytes);
    let mut breaks = 0;
    let mut after_cr = false;
    for &byte in head {
        // An LF ends a break unless it completes a CR LF already counted.
        breaks += usize::from(byte == b'\r' || (byte == b'\n' && !after_cr));
        after_cr = byte == b'\r';
    }
    let split_pair = after_cr && bytes.get(off) == Some(&b'\n');
    breaks - usize::from(split_pair)
}

/// A node of the tree. Every leaf is at height 0 and every child of a
/// height-h node is at height h − 1.
#[derive(Clone)]
pub(crate) enum Node {
    /// A chunk of text.
    Leaf(String),
    /// An internal node with its children and their summaries.
    Internal {
        /// At least 1.
        height: u8,
        /// `MIN_CHILDREN..=MAX_CHILDREN` children (2.. at the root).
        children: Vec<Child>,
    },
}

impl Node {
    /// 0 for a leaf.
    pub(crate) fn height(&self) -> u8 {
        match self {
            Node::Leaf(_) => 0,
            Node::Internal { height, .. } => *height,
        }
    }
}

/// A subtree and its summary, kept inline in the parent so that a descent
/// reads one contiguous array per level.
#[derive(Clone)]
pub(crate) struct Child {
    /// The summary of everything under `node`.
    pub(crate) summary: Summary,
    /// The subtree.
    pub(crate) node: Arc<Node>,
}

impl Child {
    /// A leaf holding a copy of `text`, with exact capacity.
    pub(crate) fn leaf(text: &str) -> Self {
        Self {
            summary: Summary::of(text),
            node: Arc::new(Node::Leaf(text.to_owned())),
        }
    }

    /// A leaf holding `text` itself.
    pub(crate) fn from_string(text: String) -> Self {
        Self {
            summary: Summary::of(&text),
            node: Arc::new(Node::Leaf(text)),
        }
    }

    /// An internal node of `height` over `children`.
    pub(crate) fn internal(height: u8, children: Vec<Child>) -> Self {
        let summary = children
            .iter()
            .fold(Summary::default(), |sum, child| sum + child.summary);
        Self {
            summary,
            node: Arc::new(Node::Internal { height, children }),
        }
    }
}

/// What a descent counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Metric {
    /// UTF-8 bytes.
    Bytes,
    /// Chars.
    Chars,
    /// UTF-16 code units.
    Utf16,
    /// Line breaks.
    Breaks,
}

/// Where a descent ended.
#[derive(Clone, Copy)]
pub(crate) struct Hit<'a> {
    /// The leaf's text.
    pub(crate) leaf: &'a str,
    /// The byte offset of the leaf in the whole text.
    pub(crate) start: usize,
    /// The summary of everything before the leaf.
    pub(crate) before: Summary,
    /// The summary of the leaf itself.
    pub(crate) summary: Summary,
}

/// The leaf holding `value` of `metric`, in the tree under `root` whose
/// summary is `total`, and the summary of everything before it. Ties at a
/// leaf boundary go to the left leaf: at each level the descent takes the
/// first child whose running total is ≥ `value`, or the last child if there
/// is none.
pub(crate) fn find(root: &Node, total: Summary, metric: Metric, value: usize) -> Hit<'_> {
    let mut node = root;
    let mut summary = total;
    let mut before = Summary::default();
    loop {
        match node {
            Node::Leaf(leaf) => {
                return Hit {
                    leaf,
                    start: before.bytes,
                    before,
                    summary,
                };
            }
            Node::Internal { children, .. } => {
                let Some(next) = pick(children, metric, value, &mut before) else {
                    // Only a broken tree has an internal node without
                    // children; answer with an empty leaf, never a panic.
                    return Hit {
                        leaf: "",
                        start: before.bytes,
                        before,
                        summary: Summary::default(),
                    };
                };
                summary = next.summary;
                node = &next.node;
            }
        }
    }
}

/// The child a left-biased descent takes, adding the summaries of the
/// children before it to `before`.
fn pick<'a>(
    children: &'a [Child],
    metric: Metric,
    value: usize,
    before: &mut Summary,
) -> Option<&'a Child> {
    let (last, init) = children.split_last()?;
    for child in init {
        if before.get(metric) + child.summary.get(metric) >= value {
            return Some(child);
        }
        *before += child.summary;
    }
    Some(last)
}

/// Checks invariants 1-5 of the design (heights, root and node sizes, no CR
/// LF split between leaves, summaries) for a rope whose root is `root` and
/// whose stored summary is `summary`. O(n). Messages hold indices, sizes
/// and offsets, never text.
pub(crate) fn check_invariants(root: &Node, summary: Summary) -> Result<(), String> {
    let mut walk = Walk::default();
    let computed = check_node(root, true, &mut walk)?;
    if computed != summary {
        return Err(format!(
            "the rope's summary is {summary:?}, but its tree holds {computed:?}"
        ));
    }
    Ok(())
}

/// The state of an in-order walk over the leaves.
#[derive(Default)]
struct Walk {
    /// Leaves seen so far.
    leaves: usize,
    /// Bytes seen so far.
    offset: usize,
    /// Whether the previous leaf ended with a CR.
    after_cr: bool,
}

fn check_node(node: &Node, is_root: bool, walk: &mut Walk) -> Result<Summary, String> {
    match node {
        Node::Leaf(text) => {
            let len = text.len();
            let (index, offset) = (walk.leaves, walk.offset);
            let fits = if is_root {
                len <= MAX_LEAF
            } else {
                (MIN_LEAF..=MAX_LEAF).contains(&len)
            };
            if !fits {
                return Err(format!(
                    "leaf {index} at byte {offset} has {len} bytes (root: {is_root}; \
                     leaves hold {MIN_LEAF}..={MAX_LEAF})"
                ));
            }
            if walk.after_cr && text.as_bytes().first() == Some(&b'\n') {
                return Err(format!(
                    "leaves {} and {index} split a CR LF at byte {offset}",
                    index.wrapping_sub(1)
                ));
            }
            walk.after_cr = text.as_bytes().last() == Some(&b'\r');
            walk.leaves += 1;
            walk.offset += len;
            Ok(Summary::of(text))
        }
        Node::Internal { height, children } => {
            let count = children.len();
            let offset = walk.offset;
            if *height == 0 {
                return Err(format!("an internal node at byte {offset} has height 0"));
            }
            let fits = if is_root {
                (2..=MAX_CHILDREN).contains(&count)
            } else {
                (MIN_CHILDREN..=MAX_CHILDREN).contains(&count)
            };
            if !fits {
                return Err(format!(
                    "a height-{height} node at byte {offset} has {count} children (root: \
                     {is_root}; nodes hold {MIN_CHILDREN}..={MAX_CHILDREN})"
                ));
            }
            let mut total = Summary::default();
            for (index, child) in children.iter().enumerate() {
                let child_height = child.node.height();
                if u16::from(child_height) + 1 != u16::from(*height) {
                    return Err(format!(
                        "child {index} of a height-{height} node at byte {offset} has height \
                         {child_height}"
                    ));
                }
                let at = walk.offset;
                let actual = check_node(&child.node, false, walk)?;
                if actual != child.summary {
                    return Err(format!(
                        "child {index} at byte {at} stores {:?}, but its subtree holds {actual:?}",
                        child.summary
                    ));
                }
                total += actual;
            }
            Ok(total)
        }
    }
}
