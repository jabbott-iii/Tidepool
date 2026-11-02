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

//! Multi-cursor selections (ADR-0020): one or more [`Cursor`]s, sorted and
//! merged by one rule, one of them primary.
//!
//! Positions are byte offsets (ADR-0019). Nothing here snaps to graphemes;
//! commands do.
//!
//! **The merge rule**, which every constructor, [`Selection::transform`]
//! and [`Selection::map`] apply:
//!
//! - non-empty cursors merge, transitively, when they share at least one
//!   byte;
//! - a caret joins the group whose closed span `[start, end]` holds it, the
//!   earlier one where two groups touch at the caret;
//! - carets that join no group and sit at the same offset become one caret;
//! - touching non-empty ranges stay apart, so adjacent matches stay
//!   separate.
//!
//! A merged cursor spans its parts. It takes the primary's direction if the
//! primary is a non-empty part, otherwise that of its first non-empty part
//! in the order (start, end, backward), so forward wins among equal spans;
//! with no non-empty part it is a caret. The primary is the cursor holding
//! the old primary.

use std::ops::Range;

use crate::transaction::{Assoc, ChangeSet, Mapper};
use crate::{Rope, TextError};

#[cfg(test)]
mod tests;

/// A cursor: `anchor` is where the selection started and `head` is where
/// the cursor is, both byte offsets. A caret when they are equal; backward
/// when `head < anchor`.
///
/// Any two numbers form a cursor; [`Selection::validate`] checks them
/// against a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Cursor {
    /// Where the selection started.
    pub anchor: usize,
    /// Where the cursor is.
    pub head: usize,
}

impl Cursor {
    /// The cursor from `anchor` to `head`.
    pub const fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// A caret at `at`.
    pub const fn caret(at: usize) -> Self {
        Self::new(at, at)
    }

    /// The smaller end.
    pub fn start(self) -> usize {
        self.anchor.min(self.head)
    }

    /// The larger end.
    pub fn end(self) -> usize {
        self.anchor.max(self.head)
    }

    /// The bytes selected: `start..end`.
    pub fn range(self) -> Range<usize> {
        self.start()..self.end()
    }

    /// Whether `anchor == head`.
    pub fn is_caret(self) -> bool {
        self.anchor == self.head
    }

    /// Whether `head < anchor`.
    pub fn is_backward(self) -> bool {
        self.head < self.anchor
    }
}

/// The cursor over `start..end`, backward or forward.
fn span(start: usize, end: usize, backward: bool) -> Cursor {
    if backward {
        Cursor::new(end, start)
    } else {
        Cursor::new(start, end)
    }
}

/// One or more cursors, sorted and merged (see the [module
/// documentation](self)), one of them primary. Never empty.
///
/// Invariants: at least one cursor; starts strictly increase; neighbours
/// touch only when both are non-empty; the primary index is in range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    cursors: Vec<Cursor>,
    primary: usize,
}

impl Selection {
    /// One caret at `at`.
    pub fn caret(at: usize) -> Self {
        Self::single(Cursor::caret(at))
    }

    /// One cursor.
    pub fn single(cursor: Cursor) -> Self {
        Self {
            cursors: vec![cursor],
            primary: 0,
        }
    }

    /// `primary` and `others`, merged. O(r) if `others` are already in
    /// order, else O(r log r).
    pub fn new(primary: Cursor, others: impl IntoIterator<Item = Cursor>) -> Self {
        let others = others.into_iter();
        let mut cursors = Vec::with_capacity(others.size_hint().0 + 1);
        cursors.extend(others);
        let at = if cursors.is_sorted_by_key(|&cursor| order(cursor)) {
            cursors.partition_point(|&cursor| order(cursor) <= order(primary))
        } else {
            0
        };
        cursors.insert(at, primary);
        Self::from_cursors(cursors, at)
    }

    /// The cursors, in order.
    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    /// The primary cursor.
    pub fn primary(&self) -> Cursor {
        *self
            .cursors
            .get(self.primary)
            .expect("the primary index is in range (invariant)")
    }

    /// The index of the primary cursor in [`cursors`](Selection::cursors).
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Applies `f` to every cursor in place, then merges; the primary
    /// follows its cursor. No allocation.
    pub fn transform(mut self, mut f: impl FnMut(Cursor) -> Cursor) -> Self {
        for cursor in &mut self.cursors {
            *cursor = f(*cursor);
        }
        let primary = self.primary();
        self.primary = normalize(&mut self.cursors, primary);
        self
    }

    /// Checks every cursor against `text`: each cursor's start, then its
    /// end, in cursor order. One walk of the text: O(r + leaves × log n).
    ///
    /// # Errors
    ///
    /// For the first bad position,
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) if it is past the end,
    /// else [`NotCharBoundary`](TextError::NotCharBoundary) (as for
    /// [`Rope::insert`]).
    pub fn validate(&self, text: &Rope) -> Result<(), TextError> {
        text.check_bytes(self.cursors.iter().flat_map(|&c| [c.start(), c.end()]))
    }

    /// The selection after `changes`.
    ///
    /// A caret maps with `caret`. A range maps its start with
    /// [`Assoc::After`] and its end with [`Assoc::Before`], so text inserted
    /// at its edges stays outside it; a range whose ends meet or cross
    /// becomes a caret where its start maps with `caret`. Then cursors
    /// merge. The mapped cursors are already in order, so this is O(k + r).
    ///
    /// Mid-char positions are not checked (there is no text here), but a
    /// selection valid in a text maps to one valid in the text after.
    ///
    /// # Errors
    ///
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) with `len` =
    /// [`len_before`](ChangeSet::len_before) for the first position past
    /// it, each cursor's start then its end, in cursor order.
    pub fn map(&self, changes: &ChangeSet, caret: Assoc) -> Result<Selection, TextError> {
        let len = changes.len_before();
        let mut positions = self.cursors.iter().flat_map(|&c| [c.start(), c.end()]);
        if let Some(byte) = positions.find(|&byte| byte > len) {
            return Err(TextError::ByteOutOfBounds { byte, len });
        }
        let mut mapper = Mapper::new(changes);
        let mut cursors: Vec<Cursor> = self
            .cursors
            .iter()
            .map(|&cursor| {
                if cursor.is_caret() {
                    return Cursor::caret(mapper.map(cursor.head, caret));
                }
                let before = mapper.map(cursor.start(), Assoc::Before);
                let after = mapper.map(cursor.start(), Assoc::After);
                let end = mapper.map(cursor.end(), Assoc::Before);
                if after >= end {
                    Cursor::caret(match caret {
                        Assoc::Before => before,
                        Assoc::After => after,
                    })
                } else {
                    span(after, end, cursor.is_backward())
                }
            })
            .collect();
        let primary = cursors
            .get(self.primary)
            .copied()
            .expect("the primary index is in range (invariant)");
        // Mapping keeps the order (ADR-0020), so only the merge pass runs.
        let primary = merge(&mut cursors, primary);
        Ok(Selection { cursors, primary })
    }

    /// Checks the invariants. O(r). For tests; the messages hold indices
    /// and offsets only.
    ///
    /// # Errors
    ///
    /// A description of the first broken invariant.
    #[doc(hidden)]
    pub fn __check_invariants(&self) -> Result<(), String> {
        let count = self.cursors.len();
        if count == 0 {
            return Err("the selection has no cursors".to_string());
        }
        if self.primary >= count {
            return Err(format!(
                "the primary index {} is not below the {count} cursors",
                self.primary
            ));
        }
        for (index, pair) in self.cursors.windows(2).enumerate() {
            let [a, b] = [pair[0], pair[1]];
            if a.start() >= b.start() {
                return Err(format!(
                    "cursor {} starts at {}, not after cursor {index}'s start {}",
                    index + 1,
                    b.start(),
                    a.start()
                ));
            }
            let apart =
                a.end() < b.start() || (a.end() == b.start() && !a.is_caret() && !b.is_caret());
            if !apart {
                return Err(format!(
                    "cursors {index} ({:?}) and {} ({:?}) should have merged",
                    a.range(),
                    index + 1,
                    b.range()
                ));
            }
        }
        Ok(())
    }

    /// `cursors` (at least one), merged, with `cursors[primary]` as the
    /// primary.
    pub(crate) fn from_cursors(mut cursors: Vec<Cursor>, primary: usize) -> Self {
        let Some(&chosen) = cursors.get(primary).or(cursors.first()) else {
            return Self::caret(0);
        };
        let primary = normalize(&mut cursors, chosen);
        Self { cursors, primary }
    }
}

/// The sort key: start, then end, then forward before backward.
fn order(cursor: Cursor) -> (usize, usize, bool) {
    (cursor.start(), cursor.end(), cursor.is_backward())
}

/// Sorts `cursors` if they are out of order, then merges them; returns the
/// index of the cursor holding `primary`. Sorting is unstable, which is
/// safe: cursors with equal keys are equal. No allocation.
fn normalize(cursors: &mut Vec<Cursor>, primary: Cursor) -> usize {
    if !cursors.is_sorted_by_key(|&cursor| order(cursor)) {
        cursors.sort_unstable_by_key(|&cursor| order(cursor));
    }
    merge(cursors, primary)
}

/// Merges sorted `cursors` in place by the module's rule, in one sweep, and
/// returns the index of the cursor holding `primary`.
///
/// A running group takes the next cursor when it starts before the group's
/// end, or exactly at it if the group or the cursor is a caret. Its span is
/// the group's; its direction comes from its first non-empty member, and is
/// fixed up for the primary's group at the end.
fn merge(cursors: &mut Vec<Cursor>, primary: Cursor) -> usize {
    cursors.dedup_by(|next, group| {
        let joins = next.start() < group.end()
            || (next.start() == group.end() && (group.is_caret() || next.is_caret()));
        if joins {
            let backward = if group.is_caret() {
                next.is_backward()
            } else {
                group.is_backward()
            };
            *group = span(group.start(), group.end().max(next.end()), backward);
        }
        joins
    });
    // The group holding the primary is the first that ends at or after it:
    // a group before it ends at or before its start, and only touches it
    // there if the primary is a caret, which then joined that earlier group.
    let index = cursors
        .partition_point(|cursor| cursor.end() < primary.end())
        .min(cursors.len().saturating_sub(1));
    if !primary.is_caret()
        && let Some(group) = cursors.get_mut(index)
    {
        *group = span(group.start(), group.end(), primary.is_backward());
    }
    index
}
