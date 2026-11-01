//! Iterators over a rope's chunks and chars, from either end.

use std::iter::FusedIterator;

use super::node::{Child, Node};

/// A position at one leaf: the path from the root (each internal level's
/// children and the index taken), the leaf, and the leaf's byte offset.
#[derive(Clone)]
struct Cursor<'a> {
    path: Vec<(&'a [Child], usize)>,
    leaf: &'a str,
    start: usize,
}

impl<'a> Cursor<'a> {
    /// The cursor at the leaf holding `byte`. With `right` bias that is the
    /// first leaf ending after `byte`; otherwise the first ending at or
    /// after it. Either way the last leaf if there is no such leaf.
    fn new(root: &'a Node, byte: usize, right: bool) -> Self {
        let mut path = Vec::with_capacity(usize::from(root.height()));
        let mut node = root;
        let mut start = 0;
        loop {
            match node {
                Node::Leaf(leaf) => return Self { path, leaf, start },
                Node::Internal { children, .. } => {
                    let mut index = 0;
                    for (i, child) in children.iter().enumerate() {
                        index = i;
                        let end = start + child.summary.bytes;
                        let holds = if right { end > byte } else { end >= byte };
                        if holds || i + 1 == children.len() {
                            break;
                        }
                        start = end;
                    }
                    let Some(child) = children.get(index) else {
                        // Only a broken tree has an empty internal node.
                        return Self {
                            path,
                            leaf: "",
                            start,
                        };
                    };
                    path.push((children.as_slice(), index));
                    node = &child.node;
                }
            }
        }
    }

    /// Moves to the next leaf; false (and no move) at the last one.
    fn next_leaf(&mut self) -> bool {
        let Some(depth) = self
            .path
            .iter()
            .rposition(|&(children, index)| index + 1 < children.len())
        else {
            return false;
        };
        self.path.truncate(depth + 1);
        let start = self.start + self.leaf.len();
        let Some(level) = self.path.last_mut() else {
            return false;
        };
        level.1 += 1;
        let (children, index) = *level;
        let mut node: &'a Node = match children.get(index) {
            Some(child) => &child.node,
            None => return false,
        };
        loop {
            match node {
                Node::Leaf(leaf) => {
                    self.leaf = leaf;
                    self.start = start;
                    return true;
                }
                Node::Internal { children, .. } => {
                    self.path.push((children.as_slice(), 0));
                    match children.first() {
                        Some(child) => node = &child.node,
                        None => return false,
                    }
                }
            }
        }
    }

    /// Moves to the previous leaf; false (and no move) at the first one.
    fn prev_leaf(&mut self) -> bool {
        let Some(depth) = self.path.iter().rposition(|&(_, index)| index > 0) else {
            return false;
        };
        self.path.truncate(depth + 1);
        let end = self.start;
        let Some(level) = self.path.last_mut() else {
            return false;
        };
        level.1 -= 1;
        let (children, index) = *level;
        let mut node: &'a Node = match children.get(index) {
            Some(child) => &child.node,
            None => return false,
        };
        loop {
            match node {
                Node::Leaf(leaf) => {
                    self.leaf = leaf;
                    self.start = end - leaf.len();
                    return true;
                }
                Node::Internal { children, .. } => {
                    let last = children.len().saturating_sub(1);
                    self.path.push((children.as_slice(), last));
                    match children.get(last) {
                        Some(child) => node = &child.node,
                        None => return false,
                    }
                }
            }
        }
    }

    fn end(&self) -> usize {
        self.start + self.leaf.len()
    }
}

/// The chunks of a rope, or of a byte range of it, as `&str`, from either
/// end. Made by [`Rope::chunks`](super::Rope::chunks) and
/// [`Rope::chunks_in`](super::Rope::chunks_in).
///
/// Chunks are non-empty and concatenate to the text. They never split a
/// char, and the chunks of a whole rope never split a CR LF; their sizes
/// are not part of the API. Creating the iterator is O(log n); each item is
/// O(1) amortized.
#[derive(Clone)]
pub struct Chunks<'a> {
    front: Cursor<'a>,
    back: Cursor<'a>,
    /// Where the next chunk from the front starts.
    start: usize,
    /// Where the next chunk from the back ends.
    end: usize,
}

impl<'a> Chunks<'a> {
    /// The chunks of `start..end`, a valid range of the tree under `root`.
    pub(crate) fn new(root: &'a Node, start: usize, end: usize) -> Self {
        Self {
            front: Cursor::new(root, start, true),
            back: Cursor::new(root, end, false),
            start,
            end,
        }
    }

    /// The next chunk from the front and its byte offset in the rope.
    pub(crate) fn next_with_offset(&mut self) -> Option<(usize, &'a str)> {
        if self.start >= self.end {
            return None;
        }
        let at = self.start;
        let until = self.front.end().min(self.end);
        let chunk = self
            .front
            .leaf
            .get(at - self.front.start..until - self.front.start)?;
        self.start = until;
        if self.start < self.end && !self.front.next_leaf() {
            self.start = self.end;
        }
        Some((at, chunk))
    }

    /// The next chunk from the back and its byte offset in the rope.
    pub(crate) fn next_back_with_offset(&mut self) -> Option<(usize, &'a str)> {
        if self.start >= self.end {
            return None;
        }
        let from = self.back.start.max(self.start);
        let chunk = self
            .back
            .leaf
            .get(from - self.back.start..self.end - self.back.start)?;
        self.end = from;
        if self.start < self.end && !self.back.prev_leaf() {
            self.end = self.start;
        }
        Some((from, chunk))
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        self.next_with_offset().map(|(_, chunk)| chunk)
    }
}

impl DoubleEndedIterator for Chunks<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.next_back_with_offset().map(|(_, chunk)| chunk)
    }
}

impl FusedIterator for Chunks<'_> {}

/// `(byte offset in the rope, char)` pairs of a byte range of a rope, from
/// either end. Made by [`Rope::char_indices_in`](super::Rope::char_indices_in).
#[derive(Clone)]
pub struct CharIndices<'a> {
    chunks: Chunks<'a>,
    front: Option<(usize, std::str::CharIndices<'a>)>,
    back: Option<(usize, std::str::CharIndices<'a>)>,
}

impl<'a> CharIndices<'a> {
    pub(crate) fn new(chunks: Chunks<'a>) -> Self {
        Self {
            chunks,
            front: None,
            back: None,
        }
    }
}

impl Iterator for CharIndices<'_> {
    type Item = (usize, char);

    fn next(&mut self) -> Option<(usize, char)> {
        loop {
            if let Some((base, chars)) = &mut self.front {
                if let Some((index, c)) = chars.next() {
                    return Some((*base + index, c));
                }
                self.front = None;
            }
            match self.chunks.next_with_offset() {
                Some((base, chunk)) => self.front = Some((base, chunk.char_indices())),
                None => {
                    let (base, chars) = self.back.as_mut()?;
                    return chars.next().map(|(index, c)| (*base + index, c));
                }
            }
        }
    }
}

impl DoubleEndedIterator for CharIndices<'_> {
    fn next_back(&mut self) -> Option<(usize, char)> {
        loop {
            if let Some((base, chars)) = &mut self.back {
                if let Some((index, c)) = chars.next_back() {
                    return Some((*base + index, c));
                }
                self.back = None;
            }
            match self.chunks.next_back_with_offset() {
                Some((base, chunk)) => self.back = Some((base, chunk.char_indices())),
                None => {
                    let (base, chars) = self.front.as_mut()?;
                    return chars.next_back().map(|(index, c)| (*base + index, c));
                }
            }
        }
    }
}

impl FusedIterator for CharIndices<'_> {}

/// The chars of a byte range of a rope, from either end. Made by
/// [`Rope::chars_in`](super::Rope::chars_in).
#[derive(Clone)]
pub struct Chars<'a> {
    inner: CharIndices<'a>,
}

impl<'a> Chars<'a> {
    pub(crate) fn new(chunks: Chunks<'a>) -> Self {
        Self {
            inner: CharIndices::new(chunks),
        }
    }
}

impl Iterator for Chars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        self.inner.next().map(|(_, c)| c)
    }
}

impl DoubleEndedIterator for Chars<'_> {
    fn next_back(&mut self) -> Option<char> {
        self.inner.next_back().map(|(_, c)| c)
    }
}

impl FusedIterator for Chars<'_> {}
