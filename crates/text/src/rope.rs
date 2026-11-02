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

//! A persistent B-tree rope of UTF-8 chunks: O(log n) edits, splits,
//! appends and slices, and O(1) `Clone` for snapshots.
//!
//! The text lives in leaves of 1,024-4,096 bytes (built at 3,072) under
//! internal nodes of 8-16 children (the root 2-16), all leaves at one
//! depth. Each parent keeps its children's sizes in bytes, chars and UTF-16
//! units and their line breaks, so every position conversion takes at most
//! six descents, each with a scan of at most one leaf ([`pos`](crate::pos)).
//!
//! Positions are byte offsets and ranges are byte ranges. A position must be
//! a char boundary in `0..=len`; the offset between a CR and its LF is one.
//! Every function that takes a position either is total (the boundary
//! queries) or returns [`TextError`] and changes nothing.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::TextError;

mod iter;
mod node;
mod tree;

#[cfg(test)]
pub(crate) mod tests;

pub use iter::{CharIndices, Chars, Chunks};
pub(crate) use node::{Hit, MAX_LEAF, Metric, Summary, breaks_before};

use node::{Child, FILL_LEAF, MIN_LEAF, Node};

/// UTF-8 text as a persistent B-tree of chunks.
///
/// `Clone` is O(1) and shares every node; an edit copies only the nodes on
/// its path that a clone still shares, so a clone is an immutable snapshot.
/// Positions are byte offsets. `Rope` is `Send + Sync`, and nothing blocks.
///
/// Equality compares the text. `Debug` shows sizes only, never the text.
///
/// | Operation | Cost |
/// | :- | :- |
/// | `clone`, `len`, [`len_in`](Rope::len_in), [`len_lines`](Rope::len_lines) | O(1) |
/// | [`insert`](Rope::insert), [`remove`](Rope::remove), [`replace`](Rope::replace) | O(log n + text.len() + 4 KiB) |
/// | [`split_off`](Rope::split_off), [`append`](Rope::append), [`slice`](Rope::slice) | O(log n + 4 KiB) |
/// | Position conversions | O(log n + 4 KiB), whatever the line length |
/// | Building, `Display`, `==` | O(n) |
#[derive(Clone)]
pub struct Rope {
    root: Arc<Node>,
    summary: Summary,
}

/// Which way an edit went, for the unit tests' path counters.
#[derive(Clone, Copy)]
enum EditPath {
    /// Edited one leaf in place.
    Fast,
    /// Split and joined.
    General,
    /// Refused with an error.
    Error,
}

/// Counts edits by path in unit tests, so that they can show that both
/// paths and the error checks were exercised.
#[cfg(test)]
pub(crate) mod counters {
    use std::cell::Cell;

    thread_local! {
        static COUNTS: Cell<[u64; 3]> = const { Cell::new([0; 3]) };
    }

    /// The counts on this thread so far: fast-path edits, general-path
    /// edits, and refused edits.
    pub(crate) fn read() -> [u64; 3] {
        COUNTS.with(Cell::get)
    }

    pub(super) fn bump(path: super::EditPath) {
        let index = match path {
            super::EditPath::Fast => 0,
            super::EditPath::General => 1,
            super::EditPath::Error => 2,
        };
        COUNTS.with(|counts| {
            let mut all = counts.get();
            all[index] += 1;
            counts.set(all);
        });
    }
}

fn count(path: EditPath) {
    #[cfg(test)]
    counters::bump(path);
    #[cfg(not(test))]
    let _ = path;
}

/// Counts `result` as refused if it is an error, and passes it on.
fn counted<T>(result: Result<T, TextError>) -> Result<T, TextError> {
    if result.is_err() {
        count(EditPath::Error);
    }
    result
}

impl Rope {
    /// The empty text. O(1).
    pub fn new() -> Self {
        Self::from_child(Child::leaf(""))
    }

    /// Length in bytes. O(1).
    pub fn len(&self) -> usize {
        self.summary.bytes
    }

    /// Whether the text is empty. O(1).
    pub fn is_empty(&self) -> bool {
        self.summary.bytes == 0
    }

    /// Inserts `text` at byte `at`.
    ///
    /// # Errors
    ///
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) if `at > len`, then
    /// [`NotCharBoundary`](TextError::NotCharBoundary). The rope is
    /// unchanged on error.
    pub fn insert(&mut self, at: usize, text: &str) -> Result<(), TextError> {
        counted(self.check_byte(at))?;
        self.replace_unchecked(at, at, text);
        Ok(())
    }

    /// Removes the bytes in `range`.
    ///
    /// # Errors
    ///
    /// Checked in this order: [`InvertedRange`](TextError::InvertedRange);
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) for the end;
    /// [`NotCharBoundary`](TextError::NotCharBoundary) for the start, then
    /// the end. The rope is unchanged on error.
    pub fn remove(&mut self, range: Range<usize>) -> Result<(), TextError> {
        counted(self.check_range(&range))?;
        self.replace_unchecked(range.start, range.end, "");
        Ok(())
    }

    /// Replaces the bytes in `range` with `text`.
    ///
    /// # Errors
    ///
    /// As for [`remove`](Rope::remove). The rope is unchanged on error.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), TextError> {
        counted(self.check_range(&range))?;
        self.replace_unchecked(range.start, range.end, text);
        Ok(())
    }

    /// Keeps the text before byte `at` and returns the text from `at` on.
    /// Splitting a CR LF is allowed: each part then has its own break.
    ///
    /// # Errors
    ///
    /// As for [`insert`](Rope::insert). The rope is unchanged on error.
    pub fn split_off(&mut self, at: usize) -> Result<Rope, TextError> {
        counted(self.check_byte(at))?;
        Ok(self.split_unchecked(at))
    }

    /// Appends `other`. A CR at the end of `self` and an LF at the start of
    /// `other` become one line break.
    pub fn append(&mut self, other: Rope) {
        let joined = tree::join(self.as_child(), other.into_child());
        *self = Self::from_child(joined);
    }

    /// The text in `range` as a rope, sharing all of this rope's nodes
    /// except near the two ends.
    ///
    /// # Errors
    ///
    /// As for [`remove`](Rope::remove).
    pub fn slice(&self, range: Range<usize>) -> Result<Rope, TextError> {
        counted(self.check_range(&range))?;
        Ok(self.slice_unchecked(range))
    }

    /// Whether `byte` is a char boundary: `0..=len`, not inside a
    /// character. False past the end. O(log n).
    pub fn is_char_boundary(&self, byte: usize) -> bool {
        match byte.cmp(&self.len()) {
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Less => {
                let hit = self.find(Metric::Bytes, byte);
                hit.leaf.is_char_boundary(byte - hit.start)
            }
        }
    }

    /// The largest char boundary ≤ `byte`; `len` if `byte >= len`. O(log n).
    pub fn floor_char_boundary(&self, byte: usize) -> usize {
        if byte >= self.len() {
            return self.len();
        }
        let hit = self.find(Metric::Bytes, byte);
        hit.start + hit.leaf.floor_char_boundary(byte - hit.start)
    }

    /// The smallest char boundary ≥ `byte`; `len` if `byte >= len`. O(log n).
    pub fn ceil_char_boundary(&self, byte: usize) -> usize {
        if byte >= self.len() {
            return self.len();
        }
        let hit = self.find(Metric::Bytes, byte);
        hit.start + hit.leaf.ceil_char_boundary(byte - hit.start)
    }

    /// The chunks of the whole text, from either end: non-empty,
    /// concatenating to the text, never splitting a char or a CR LF. Their
    /// sizes are not part of the API. O(log n) to create, O(1) amortized per
    /// item.
    pub fn chunks(&self) -> Chunks<'_> {
        Chunks::new(&self.root, 0, self.len())
    }

    /// The chunks of `range`, clipped to it. The first may start with an LF
    /// whose CR is before the range, and the last may end with a CR whose
    /// LF is after it.
    ///
    /// # Errors
    ///
    /// As for [`remove`](Rope::remove).
    pub fn chunks_in(&self, range: Range<usize>) -> Result<Chunks<'_>, TextError> {
        self.check_range(&range)?;
        Ok(Chunks::new(&self.root, range.start, range.end))
    }

    /// The chars in `range`, from either end.
    ///
    /// # Errors
    ///
    /// As for [`remove`](Rope::remove).
    pub fn chars_in(&self, range: Range<usize>) -> Result<Chars<'_>, TextError> {
        self.chunks_in(range).map(Chars::new)
    }

    /// `(byte offset, char)` pairs in `range`, from either end. The offsets
    /// are from the start of the rope, not of the range.
    ///
    /// # Errors
    ///
    /// As for [`remove`](Rope::remove).
    pub fn char_indices_in(&self, range: Range<usize>) -> Result<CharIndices<'_>, TextError> {
        self.chunks_in(range).map(CharIndices::new)
    }

    /// Checks the tree's structural invariants: heights, node sizes, no CR
    /// LF split between leaves, and every stored summary. O(n). For tests;
    /// the messages hold indices, sizes and offsets, never text.
    ///
    /// # Errors
    ///
    /// A description of the first broken invariant.
    #[doc(hidden)]
    pub fn __check_invariants(&self) -> Result<(), String> {
        node::check_invariants(&self.root, self.summary)
    }

    /// Checks that `byte` is a valid position.
    pub(crate) fn check_byte(&self, byte: usize) -> Result<(), TextError> {
        if byte > self.len() {
            Err(TextError::ByteOutOfBounds {
                byte,
                len: self.len(),
            })
        } else if !self.is_char_boundary(byte) {
            Err(TextError::NotCharBoundary { byte })
        } else {
            Ok(())
        }
    }

    /// Checks that `range` is a valid range, in the documented order.
    pub(crate) fn check_range(&self, range: &Range<usize>) -> Result<(), TextError> {
        let Range { start, end } = *range;
        if start > end {
            return Err(TextError::InvertedRange { start, end });
        }
        if end > self.len() {
            return Err(TextError::ByteOutOfBounds {
                byte: end,
                len: self.len(),
            });
        }
        if !self.is_char_boundary(start) {
            return Err(TextError::NotCharBoundary { byte: start });
        }
        if !self.is_char_boundary(end) {
            return Err(TextError::NotCharBoundary { byte: end });
        }
        Ok(())
    }

    /// The summary of the whole text.
    pub(crate) fn summary(&self) -> Summary {
        self.summary
    }

    /// The leaf holding `value` of `metric` (left-biased; see
    /// [`node::find`]).
    pub(crate) fn find(&self, metric: Metric, value: usize) -> Hit<'_> {
        node::find(&self.root, self.summary, metric, value)
    }

    /// Whether both ropes are the same tree (`Arc::ptr_eq` on the roots),
    /// not just the same text. O(1).
    pub(crate) fn same_tree(&self, other: &Rope) -> bool {
        Arc::ptr_eq(&self.root, &other.root)
    }

    /// Checks positions in the order given: the first past `len` is
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds), the first inside a
    /// char is [`NotCharBoundary`](TextError::NotCharBoundary).
    ///
    /// Correct for any order. When the positions do not decrease it
    /// descends once per leaf touched: O(k + leaves touched × log n).
    pub(crate) fn check_bytes(
        &self,
        positions: impl IntoIterator<Item = usize>,
    ) -> Result<(), TextError> {
        let len = self.len();
        // The last leaf a descent found, and its offset.
        let mut leaf: Option<(usize, &str)> = None;
        for byte in positions {
            if byte > len {
                return Err(TextError::ByteOutOfBounds { byte, len });
            }
            if byte == len {
                continue;
            }
            let (start, text) = match leaf {
                // A leaf's end is a boundary too: leaves hold whole chars.
                Some((start, text)) if byte >= start && byte - start <= text.len() => (start, text),
                _ => {
                    let hit = self.find(Metric::Bytes, byte);
                    leaf = Some((hit.start, hit.leaf));
                    (hit.start, hit.leaf)
                }
            };
            if !text.is_char_boundary(byte - start) {
                return Err(TextError::NotCharBoundary { byte });
            }
        }
        Ok(())
    }

    /// The text of a validated range: one fresh leaf if it has at most
    /// `MAX_LEAF` bytes, otherwise a slice that shares nodes with this rope.
    /// O(log n + 4 KiB).
    pub(crate) fn piece(&self, range: Range<usize>) -> Rope {
        let len = range.end.saturating_sub(range.start);
        if len > MAX_LEAF {
            return self.slice_unchecked(range);
        }
        let mut text = String::with_capacity(len);
        for chunk in Chunks::new(&self.root, range.start, range.end) {
            text.push_str(chunk);
        }
        Self::from_child(Child::from_string(text))
    }

    /// Replaces the validated range `start..end` with `text`, or removes it
    /// for `None`.
    ///
    /// No text, or a one-leaf text of at most `MAX_LEAF` bytes, goes the way
    /// of [`replace`](Rope::replace), with its in-leaf fast path. A larger
    /// text is shared, never copied: split, drop the middle, append the
    /// text, append the rest. O(log n + 4 KiB).
    pub(crate) fn replace_with(&mut self, start: usize, end: usize, text: Option<&Rope>) {
        let Some(text) = text else {
            self.replace_unchecked(start, end, "");
            return;
        };
        if let Node::Leaf(leaf) = &*text.root
            && leaf.len() <= MAX_LEAF
        {
            self.replace_unchecked(start, end, leaf);
            return;
        }
        count(EditPath::General);
        let right = self.split_unchecked(end);
        let _removed = self.split_unchecked(start);
        self.append(text.clone());
        self.append(right);
    }

    /// `replace` after validation.
    fn replace_unchecked(&mut self, start: usize, end: usize, text: &str) {
        if start == end && text.is_empty() {
            return;
        }
        if tree::fast_edit(&mut self.root, &mut self.summary, start, end, text) {
            count(EditPath::Fast);
            return;
        }
        count(EditPath::General);
        let right = self.split_unchecked(end);
        let _removed = self.split_unchecked(start);
        if !text.is_empty() {
            self.append(Rope::from(text));
        }
        self.append(right);
    }

    /// `slice` after validation.
    fn slice_unchecked(&self, range: Range<usize>) -> Rope {
        let (head, _) = tree::split(&self.as_child(), range.end);
        let middle = head.and_then(|head| tree::split(&head, range.start).1);
        middle.map_or_else(Rope::new, Rope::from_child)
    }

    /// `split_off` after validation.
    fn split_unchecked(&mut self, at: usize) -> Rope {
        let (left, right) = tree::split(&self.as_child(), at);
        *self = left.map_or_else(Rope::new, Rope::from_child);
        right.map_or_else(Rope::new, Rope::from_child)
    }

    fn from_child(child: Child) -> Self {
        Self {
            root: child.node,
            summary: child.summary,
        }
    }

    fn as_child(&self) -> Child {
        Child {
            summary: self.summary,
            node: Arc::clone(&self.root),
        }
    }

    fn into_child(self) -> Child {
        Child {
            summary: self.summary,
            node: self.root,
        }
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&str> for Rope {
    fn from(text: &str) -> Self {
        if text.len() <= MAX_LEAF {
            return Self::from_child(Child::leaf(text));
        }
        let mut builder = RopeBuilder::new();
        builder.push_str(text);
        builder.finish()
    }
}

impl From<String> for Rope {
    /// Keeps `text`'s buffer as the only leaf when it is small; otherwise
    /// copies it into leaves.
    fn from(text: String) -> Self {
        if text.capacity() <= MAX_LEAF {
            return Self::from_child(Child::from_string(text));
        }
        Self::from(text.as_str())
    }
}

impl fmt::Display for Rope {
    /// Writes the text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.chunks().try_for_each(|chunk| f.write_str(chunk))
    }
}

impl fmt::Debug for Rope {
    /// Sizes only, never the text (it may hold secrets).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rope")
            .field("bytes", &self.summary.bytes)
            .field("chars", &self.summary.chars)
            .field("utf16", &self.summary.utf16)
            .field("lines", &(self.summary.breaks + 1))
            .finish()
    }
}

impl PartialEq for Rope {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.root, &other.root) {
            return true;
        }
        self.len() == other.len() && chunks_eq(self.chunks(), other.chunks())
    }
}

impl Eq for Rope {}

impl PartialEq<str> for Rope {
    fn eq(&self, other: &str) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let mut rest = other.as_bytes();
        for chunk in self.chunks() {
            match rest.strip_prefix(chunk.as_bytes()) {
                Some(tail) => rest = tail,
                None => return false,
            }
        }
        rest.is_empty()
    }
}

impl PartialEq<&str> for Rope {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl PartialEq<String> for Rope {
    fn eq(&self, other: &String) -> bool {
        *self == **other
    }
}

/// Whether two sequences of chunks spell the same bytes. Both iterators must
/// be fused.
fn chunks_eq<'a, 'b>(
    mut a: impl Iterator<Item = &'a str>,
    mut b: impl Iterator<Item = &'b str>,
) -> bool {
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    loop {
        if x.is_empty()
            && let Some(chunk) = a.next()
        {
            x = chunk.as_bytes();
            continue;
        }
        if y.is_empty()
            && let Some(chunk) = b.next()
        {
            y = chunk.as_bytes();
            continue;
        }
        if x.is_empty() || y.is_empty() {
            return x.is_empty() && y.is_empty();
        }
        let n = x.len().min(y.len());
        let ((xa, xb), (ya, yb)) = (x.split_at(n), y.split_at(n));
        if xa != ya {
            return false;
        }
        (x, y) = (xb, yb);
    }
}

/// Builds a rope from pieces in time linear in their total length.
///
/// Pieces may be empty and may split a CR LF or nothing else: a CR at the
/// end of one piece and an LF at the start of the next stay one break.
/// There is no `Debug`, because the pending text may hold secrets.
#[derive(Default)]
pub struct RopeBuilder {
    /// Finished leaves, in order.
    leaves: Vec<Child>,
    /// Text not yet in a leaf; never more than `2 * MAX_LEAF` bytes.
    pending: String,
}

impl RopeBuilder {
    /// A builder with no text yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `text`.
    pub fn push_str(&mut self, text: &str) {
        let mut rest = text;
        while !rest.is_empty() {
            let take = if rest.len() <= MAX_LEAF {
                rest.len()
            } else {
                rest.floor_char_boundary(MAX_LEAF)
            };
            let (piece, tail) = rest.split_at(take);
            self.pending.push_str(piece);
            rest = tail;
            // Cut only with more than MAX_LEAF bytes buffered, so the byte
            // after the cut is known and a CR LF is never split.
            while self.pending.len() > MAX_LEAF {
                let cut = tree::cut_point(&self.pending, FILL_LEAF);
                let Some(head) = self.pending.get(..cut) else {
                    break;
                };
                self.leaves.push(Child::leaf(head));
                self.pending.replace_range(..cut, "");
            }
        }
    }

    /// The rope of everything pushed.
    pub fn finish(self) -> Rope {
        let Self {
            mut leaves,
            pending,
        } = self;
        if !pending.is_empty() {
            // A tail shorter than MIN_LEAF joins the leaf before it, which
            // has at most FILL_LEAF bytes; FILL_LEAF + MIN_LEAF <= MAX_LEAF.
            let merged = match leaves.last().map(|last| &*last.node) {
                Some(Node::Leaf(text)) if pending.len() < MIN_LEAF => {
                    let mut merged = String::with_capacity(text.len() + pending.len());
                    merged.push_str(text);
                    merged.push_str(&pending);
                    Some(merged)
                }
                _ => None,
            };
            match merged {
                Some(merged) => {
                    leaves.pop();
                    leaves.push(Child::from_string(merged));
                }
                None => leaves.push(Child::leaf(&pending)),
            }
        }
        let mut height = 1;
        while leaves.len() > 1 {
            leaves = tree::group(leaves, height);
            height += 1;
        }
        leaves.pop().map_or_else(Rope::new, Rope::from_child)
    }
}
