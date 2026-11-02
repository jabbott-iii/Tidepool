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

//! Edits as change sets, and transactions (ADR-0020).
//!
//! A [`ChangeSet`] edits a whole text of `len_before` bytes into one of
//! `len_after`: sorted replacements in old-text byte offsets, each holding
//! its new text as a [`Rope`]. At least one kept byte separates any two and
//! none does nothing, so the form is canonical and `==` is structural.
//!
//! **Mapping** a position: kept text shifts; the start and end of a
//! replaced range go to the start and end of its new text; at a pure insert
//! or strictly inside a replaced range, [`Assoc::Before`] goes to the new
//! text's start and [`Assoc::After`] to its end. Mapping is monotone for
//! each `Assoc`, `Before` never maps past `After`, and char boundaries map
//! to char boundaries.
//!
//! **Laws** (property-tested): `invert` undoes and is an involution;
//! `compose` applies both, is associative, has the identity as its unit,
//! and `(a∘b)⁻¹ = b⁻¹∘a⁻¹`; `c∘c⁻¹` is the identity in effect only.
//!
//! **Not laws:** through a composed set, positions at or inside its
//! replacements may map differently than step by step; and merging
//! touching edits forgets the seam between them, so mapping through
//! [`ChangeSet::new`]'s result can differ at a seam from mapping through
//! the edits themselves ([`Transaction::change_each`] keeps seams).
//!
//! **Consumers:** LSP sends [`changes`](ChangeSet::changes) last first,
//! converting `old` on the text before; syntax re-lexes the `new` ranges.
//!
//! A [`Transaction`] is a change set and the selection after it.

use std::iter::FusedIterator;
use std::ops::Range;
use std::sync::Arc;

use crate::rope::MAX_LEAF;
use crate::selection::{Cursor, Selection};
use crate::{Rope, TextError};

mod apply;
mod compose;
mod each;

#[cfg(test)]
pub(crate) use apply::paths;
#[cfg(test)]
pub(crate) mod tests;

/// The most bytes a text can have: `isize::MAX`, the most one allocation
/// can hold. Longer would overflow the rope's sizes.
pub(crate) const MAX_LEN: usize = isize::MAX as usize;

/// Which side of text inserted exactly at a position, or replacing the
/// range it is strictly inside, the position ends up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Assoc {
    /// Before the new text.
    Before,
    /// After the new text.
    After,
}

/// An edit of a whole text of [`len_before`](ChangeSet::len_before) bytes
/// into one of [`len_after`](ChangeSet::len_after): canonical replacements
/// in old-text byte offsets (see the [module documentation](self)).
///
/// `Clone` is O(1); `Send + Sync`; `==` is structural. `Debug` shows sizes
/// and ranges only, never the text.
///
/// | Operation | Cost, for k edits or replacements with ΣL bytes of new text, on n bytes; "leaves" are the text's leaves that hold their ends |
/// | :- | :- |
/// | [`new`](ChangeSet::new) | O(k + ΣL + leaves × log n) for edits in order; out of order, plus O(k log k) to sort them and O(k log n) to check them |
/// | [`apply`](ChangeSet::apply) | O(k (log n + 4 KiB)), or O(n + ΣL) when many small edits rebuild the text in one pass; large texts are shared on the first path, copied on the second |
/// | [`invert`](ChangeSet::invert) | O(k log n + Σ min(deleted, 4 KiB)) |
/// | [`compose`](ChangeSet::compose) | O(k_a + k_b) plus rope slices and appends |
/// | [`map`](ChangeSet::map) | O(log k) |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    replacements: Arc<[Replacement]>,
    len_before: usize,
    len_after: usize,
}

/// One replacement: the old bytes `start..end` become `text`, which starts
/// at `new_start` in the text after.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Replacement {
    start: usize,
    end: usize,
    new_start: usize,
    /// `None` for a pure deletion; never empty.
    text: Option<Rope>,
}

impl Replacement {
    /// The length of the new text.
    fn text_len(&self) -> usize {
        self.text.as_ref().map_or(0, Rope::len)
    }

    fn change(&self) -> Change<'_> {
        Change {
            old: self.start..self.end,
            new: self.new_start..self.new_start + self.text_len(),
            text: self.text.as_ref(),
        }
    }
}

impl ChangeSet {
    /// No change to a text of `len` bytes. O(1).
    pub fn identity(len: usize) -> Self {
        Self {
            replacements: Vec::new().into(),
            len_before: len,
            len_after: len,
        }
    }

    /// The change set that makes `edits`, each a byte range of `text` and
    /// its new text, given in any order.
    ///
    /// Edits that touch merge, their texts concatenated in sorted order: a
    /// text of at most 4 KiB after another is copied, a larger one shared.
    /// Pure inserts at one offset keep their input order and come before an
    /// edit that starts there, as LSP requires. Edits with an empty range
    /// and an empty text are checked, then dropped.
    ///
    /// # Errors
    ///
    /// In this order:
    /// 1. each range as [`Rope::replace`] checks it, in input order;
    /// 2. after a stable sort by (start, end),
    ///    [`OverlappingEdits`](TextError::OverlappingEdits) for the first
    ///    neighbours where the next starts before the previous ends;
    /// 3. [`TooLong`](TextError::TooLong) if the text after would be longer
    ///    than `isize::MAX` bytes.
    pub fn new<T: Into<Rope>>(
        text: &Rope,
        edits: impl IntoIterator<Item = (Range<usize>, T)>,
    ) -> Result<Self, TextError> {
        let mut edits: Vec<(Range<usize>, Rope)> = edits
            .into_iter()
            .map(|(range, new)| (range, new.into()))
            .collect();
        check_ranges(
            text,
            edits.iter().map(|(range, _)| (range.start, range.end)),
        )?;
        edits.retain(|(range, new)| range.start < range.end || !new.is_empty());
        let key = |edit: &(Range<usize>, Rope)| (edit.0.start, edit.0.end);
        if !edits.is_sorted_by_key(key) {
            edits.sort_by_key(key);
        }
        for pair in edits.windows(2) {
            if let [(prev, _), (next, _)] = pair
                && next.start < prev.end
            {
                return Err(TextError::OverlappingEdits {
                    end: prev.end,
                    start: next.start,
                });
            }
        }
        length_after(
            text.len(),
            edits
                .iter()
                .map(|(range, new)| (range.end - range.start, new.len())),
        )?;
        let mut builder = Builder::default();
        for (range, new) in edits {
            builder.edit(range.start, range.end, new);
        }
        Ok(builder.finish(text.len()))
    }

    /// The length in bytes of the text this change set edits.
    pub fn len_before(&self) -> usize {
        self.len_before
    }

    /// The length in bytes of the text after it.
    pub fn len_after(&self) -> usize {
        self.len_after
    }

    /// Whether it changes nothing (has no replacements). O(1).
    pub fn is_identity(&self) -> bool {
        self.replacements.is_empty()
    }

    /// The replacements, in ascending order. O(1) per item.
    pub fn changes(&self) -> Changes<'_> {
        Changes {
            inner: self.replacements.iter(),
        }
    }

    /// Applies the change set to `text`, which must be the text it was made
    /// for (or one of the same length and char boundaries).
    ///
    /// O(k (log n + 4 KiB)), or O(n + ΣL) when many small edits rebuild the
    /// text in one pass; large texts are shared on the first path, copied on
    /// the second. A rebuilt text shares no nodes with the one before.
    ///
    /// # Errors
    ///
    /// [`LengthMismatch`](TextError::LengthMismatch) if `text` does not
    /// have [`len_before`](ChangeSet::len_before) bytes; then
    /// [`NotCharBoundary`](TextError::NotCharBoundary) for each
    /// replacement's start, then its end, ascending. The rope is unchanged
    /// on error.
    pub fn apply(&self, text: &mut Rope) -> Result<(), TextError> {
        apply::apply(self, text)
    }

    /// The change set that undoes this one on the text it produces from
    /// `before`. Deleted text is shared with `before` where it is large, so
    /// inverting the deletion of a whole text is O(log n).
    ///
    /// # Errors
    ///
    /// As for [`apply`](ChangeSet::apply), on `before`.
    pub fn invert(&self, before: &Rope) -> Result<ChangeSet, TextError> {
        self.check_text(before)?;
        let replacements = self
            .replacements
            .iter()
            .map(|r| Replacement {
                start: r.new_start,
                end: r.new_start + r.text_len(),
                new_start: r.start,
                text: (r.start < r.end).then(|| before.piece(r.start..r.end)),
            })
            .collect();
        Ok(Self {
            replacements,
            len_before: self.len_after,
            len_after: self.len_before,
        })
    }

    /// This change set, then `next`: one change set with the effect of both.
    ///
    /// # Errors
    ///
    /// [`LengthMismatch`](TextError::LengthMismatch) with `expected` =
    /// `next.len_before()` and `actual` = `self.len_after()` if they
    /// differ; [`NotCharBoundary`](TextError::NotCharBoundary) if `next`
    /// splits a char of a text this one inserts, with the lowest such offset
    /// in this change set's result.
    pub fn compose(&self, next: &ChangeSet) -> Result<ChangeSet, TextError> {
        compose::compose(self, next)
    }

    /// Where `pos`, a byte offset in the text before, ends up in the text
    /// after (see the [module documentation](self)). O(log k).
    ///
    /// # Errors
    ///
    /// [`ByteOutOfBounds`](TextError::ByteOutOfBounds) with `len` =
    /// [`len_before`](ChangeSet::len_before) if `pos` is past it.
    pub fn map(&self, pos: usize, assoc: Assoc) -> Result<usize, TextError> {
        if pos > self.len_before {
            return Err(TextError::ByteOutOfBounds {
                byte: pos,
                len: self.len_before,
            });
        }
        let index = self.replacements.partition_point(|r| r.end < pos);
        Ok(self.map_at(index, pos, assoc))
    }

    /// Checks the canonical form: ranges in order and in bounds, at least
    /// one kept byte between neighbours, none doing nothing, new offsets and
    /// lengths that add up, and valid texts. O(n) in the new texts. For
    /// tests; the messages hold indices, offsets and sizes only.
    ///
    /// # Errors
    ///
    /// A description of the first broken invariant.
    #[doc(hidden)]
    pub fn __check_invariants(&self) -> Result<(), String> {
        let (mut removed, mut added) = (0usize, 0usize);
        let mut previous: Option<(usize, &Replacement)> = None;
        for (index, r) in self.replacements.iter().enumerate() {
            if r.start > r.end || r.end > self.len_before {
                return Err(format!(
                    "replacement {index} covers {}..{} of a {}-byte text",
                    r.start, r.end, self.len_before
                ));
            }
            if r.text.as_ref().is_some_and(Rope::is_empty) {
                return Err(format!("replacement {index} has an empty text, not None"));
            }
            if r.start == r.end && r.text.is_none() {
                return Err(format!("replacement {index} does nothing"));
            }
            let want = match previous {
                None => r.start,
                Some((before, p)) => {
                    if p.end >= r.start {
                        return Err(format!(
                            "replacements {before} ({}..{}) and {index} ({}..{}) have no kept \
                             byte between them",
                            p.start, p.end, r.start, r.end
                        ));
                    }
                    p.new_start
                        .saturating_add(p.text_len())
                        .saturating_add(r.start - p.end)
                }
            };
            if r.new_start != want {
                return Err(format!(
                    "replacement {index} starts at {} in the text after, not {want}",
                    r.new_start
                ));
            }
            if let Some(text) = &r.text {
                text.__check_invariants()
                    .map_err(|e| format!("replacement {index}'s text: {e}"))?;
            }
            removed += r.end - r.start;
            added = added
                .checked_add(r.text_len())
                .ok_or_else(|| format!("the new texts overflow at replacement {index}"))?;
            previous = Some((index, r));
        }
        let want = self
            .len_before
            .checked_sub(removed)
            .and_then(|kept| kept.checked_add(added))
            .filter(|&len| len <= MAX_LEN);
        if want != Some(self.len_after) {
            return Err(format!(
                "len_after is {}, but {} bytes less {removed} removed plus {added} added is \
                 {want:?} (at most {MAX_LEN})",
                self.len_after, self.len_before
            ));
        }
        Ok(())
    }

    /// Checks that `text` fits: its length, then every replacement's start
    /// and end, ascending.
    fn check_text(&self, text: &Rope) -> Result<(), TextError> {
        if text.len() != self.len_before {
            return Err(TextError::LengthMismatch {
                expected: self.len_before,
                actual: text.len(),
            });
        }
        text.check_bytes(self.replacements.iter().flat_map(|r| [r.start, r.end]))
    }

    /// `map(pos, assoc)` for a valid `pos`, given the index of the first
    /// replacement that ends at or after it.
    fn map_at(&self, index: usize, pos: usize, assoc: Assoc) -> usize {
        let Some(r) = self.replacements.get(index) else {
            return self
                .len_after
                .saturating_sub(self.len_before.saturating_sub(pos));
        };
        let pick = |before, after| match assoc {
            Assoc::Before => before,
            Assoc::After => after,
        };
        let new_end = r.new_start + r.text_len();
        if pos < r.start {
            r.new_start.saturating_sub(r.start - pos)
        } else if r.start == r.end || (r.start < pos && pos < r.end) {
            // A pure insert at `pos`, or `pos` strictly inside the range.
            pick(r.new_start, new_end)
        } else if pos == r.start {
            r.new_start
        } else {
            new_end
        }
    }
}

/// Maps positions that do not decrease through a change set, with an
/// index into its replacements that only moves forward: O(k + r) for r
/// positions. Each position must be at most `len_before`.
pub(crate) struct Mapper<'a> {
    changes: &'a ChangeSet,
    index: usize,
}

impl<'a> Mapper<'a> {
    /// A mapper at the start of `changes`.
    pub(crate) fn new(changes: &'a ChangeSet) -> Self {
        Self { changes, index: 0 }
    }

    /// [`ChangeSet::map`] of `pos`, which is not less than the last one.
    pub(crate) fn map(&mut self, pos: usize, assoc: Assoc) -> usize {
        while self
            .changes
            .replacements
            .get(self.index)
            .is_some_and(|r| r.end < pos)
        {
            self.index += 1;
        }
        self.changes.map_at(self.index, pos, assoc)
    }
}

/// One replacement of a change set.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Change<'a> {
    /// The byte range replaced, in the text before.
    pub old: Range<usize>,
    /// The byte range of the new text, in the text after.
    pub new: Range<usize>,
    /// The new text; `None` for a pure deletion, never empty.
    pub text: Option<&'a Rope>,
}

/// The replacements of a change set, ascending, from either end. Made by
/// [`ChangeSet::changes`].
#[derive(Clone, Debug)]
pub struct Changes<'a> {
    inner: std::slice::Iter<'a, Replacement>,
}

impl<'a> Iterator for Changes<'a> {
    type Item = Change<'a>;

    fn next(&mut self) -> Option<Change<'a>> {
        self.inner.next().map(Replacement::change)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl DoubleEndedIterator for Changes<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(Replacement::change)
    }
}

impl ExactSizeIterator for Changes<'_> {}

impl FusedIterator for Changes<'_> {}

/// A change set and the selection after it, valid in the text after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    changes: ChangeSet,
    selection: Selection,
}

impl Transaction {
    /// `changes` and the selection after them. Not checked here; the
    /// history checks a transaction when it is committed.
    pub fn new(changes: ChangeSet, selection: Selection) -> Self {
        Self { changes, selection }
    }

    /// The change set.
    pub fn changes(&self) -> &ChangeSet {
        &self.changes
    }

    /// The selection after the change set.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The change set and the selection.
    pub fn into_parts(self) -> (ChangeSet, Selection) {
        (self.changes, self.selection)
    }

    /// One edit per cursor, for multi-cursor commands.
    ///
    /// `edit` is called once per cursor, in cursor order, and returns the
    /// cursor's edit (a byte range of `text`, its new text, and where to
    /// leave the cursor) or `None` to only move the cursor along. An edit
    /// with an empty range and an empty text is checked like any other,
    /// then counts as `None` (as [`ChangeSet::new`] checks such an edit,
    /// then drops it). To share one paste across all cursors, return a
    /// clone of one rope (O(1) each).
    ///
    /// The edits are sorted by (start, end), stably, so edits with the same
    /// range stay in cursor order. Then each is compared with the edit kept
    /// just before it: one with the same range and the same text collapses
    /// into it, a deletion that overlaps a kept deletion extends it to their
    /// union, and any other overlap is an error. So identical deletions and
    /// replacements always collapse, but among pure inserts at one offset
    /// only consecutive identical ones do: cursors inserting "a", "b", "a"
    /// at one offset insert all three. Cursors that made no edit, or chose
    /// [`Place::Map`], are mapped through the edits themselves, so a seam
    /// between touching edits stays a seam; the others are placed at their
    /// own edit's new text. Then the cursors merge, the primary's placed
    /// cursor staying primary.
    ///
    /// For r cursors and k edits with ΣL bytes of new text, O(r + k + ΣL +
    /// leaves × log n), the leaves being those that hold the cursors' and
    /// the edits' ends, when the edits come in order and so do the cursors
    /// placed; otherwise plus O(k log n) to check the edits and O(k log k +
    /// r log r) to sort. An edit with the same range as the edit kept
    /// before it is compared with it by content: O(L), or O(1) when both
    /// texts are clones of one rope.
    ///
    /// # Errors
    ///
    /// Before `edit` is called, as for [`Selection::validate`]. Then, in
    /// this order: each returned range as [`Rope::replace`] checks it, in
    /// cursor order, an edit that does nothing included;
    /// [`OverlappingEdits`](TextError::OverlappingEdits);
    /// [`TooLong`](TextError::TooLong).
    pub fn change_each<T: Into<Rope>>(
        text: &Rope,
        selection: &Selection,
        edit: impl FnMut(Cursor) -> Option<(Range<usize>, T, Place)>,
    ) -> Result<Transaction, TextError> {
        each::change_each(text, selection, edit)
    }
}

/// Where [`Transaction::change_each`] leaves a cursor that made an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Place {
    /// The old cursor, mapped through the edits: a caret with
    /// [`Assoc::After`], a range with its start `After` and its end
    /// `Before` (a range that vanishes becomes a caret where its start
    /// maps).
    Map,
    /// A caret before this cursor's own new text.
    Start,
    /// A caret after this cursor's own new text (typing, paste).
    End,
    /// This cursor's own new text, selected forward.
    Select,
}

/// Checks ranges as [`Rope::replace`] checks each one, in the order given,
/// with one walk of the text for all their char boundaries: the O(1) checks
/// first, up to the first that fails, then the boundaries of the ranges
/// before it.
fn check_ranges(
    text: &Rope,
    ranges: impl Iterator<Item = (usize, usize)> + Clone,
) -> Result<(), TextError> {
    let len = text.len();
    let mut first_bad = None;
    for (index, (start, end)) in ranges.clone().enumerate() {
        if start > end {
            first_bad = Some((index, TextError::InvertedRange { start, end }));
            break;
        }
        if end > len {
            first_bad = Some((index, TextError::ByteOutOfBounds { byte: end, len }));
            break;
        }
    }
    let checked = first_bad.as_ref().map_or(usize::MAX, |(index, _)| *index);
    text.check_bytes(ranges.take(checked).flat_map(|(start, end)| [start, end]))?;
    match first_bad {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}

/// The length after edits of `(removed, added)` bytes to a text of `len`
/// bytes, which must not overlap.
///
/// # Errors
///
/// [`TooLong`](TextError::TooLong) if it would be over [`MAX_LEN`].
fn length_after(
    len: usize,
    sizes: impl Iterator<Item = (usize, usize)>,
) -> Result<usize, TextError> {
    let mut removed = 0usize;
    let mut added = Some(0usize);
    for (gone, new) in sizes {
        removed = removed.saturating_add(gone);
        added = added.and_then(|sum| sum.checked_add(new));
    }
    len.checked_sub(removed)
        .zip(added)
        .and_then(|(kept, added)| kept.checked_add(added))
        .filter(|&after| after <= MAX_LEN)
        .ok_or(TextError::TooLong { max: MAX_LEN })
}

/// Builds a canonical change set left to right from kept runs, deletions
/// and insertions; edits with no kept byte between them merge.
///
/// A run's first text is shared, whatever its size. After it, a text of at
/// most `MAX_LEAF` bytes is copied onto the run's tail, and a larger one is
/// shared: the tail is appended first, then the text. So a run makes at
/// most one append, plus two per later text of over `MAX_LEAF` bytes,
/// rather than one append (up to 4 KiB of copying) per text after the
/// first; typing at one point still makes one.
#[derive(Default)]
struct Builder {
    replacements: Vec<Replacement>,
    /// The old offset reached.
    old: usize,
    /// The new offset reached.
    new: usize,
    /// The replacement being built, if it has deleted or inserted anything.
    run: Option<Run>,
    /// Small texts inserted into the run after its text, in order, not yet
    /// appended to it. Empty while the run has no text.
    tail: String,
}

/// A replacement being built.
struct Run {
    start: usize,
    new_start: usize,
    deleted: usize,
    text: Option<Rope>,
}

/// The run in `slot`, started at the offsets `old` and `new` if there is
/// none.
fn run_at(slot: &mut Option<Run>, old: usize, new: usize) -> &mut Run {
    slot.get_or_insert_with(|| Run {
        start: old,
        new_start: new,
        deleted: 0,
        text: None,
    })
}

// `settle` and `add` stay out of line so that `insert` and `flush` stay
// small for the common run of one text: inlined, they cost separate edits
// about 10% (measured).

/// Appends `tail`, emptied, to a run's `text`, with one append if it holds
/// anything.
#[inline(never)]
fn settle(text: &mut Option<Rope>, tail: &mut String) {
    if tail.is_empty() {
        return;
    }
    let more = Rope::from(std::mem::take(tail));
    match text {
        Some(text) => append(text, more),
        None => *text = Some(more),
    }
}

/// Adds `new` to a run's `text` and `tail` after what they hold: copied
/// onto the tail if it has at most `MAX_LEAF` bytes, otherwise shared,
/// after the tail.
#[inline(never)]
fn add(text: &mut Option<Rope>, tail: &mut String, new: Rope) {
    if new.len() <= MAX_LEAF {
        tail.extend(new.chunks());
        return;
    }
    settle(text, tail);
    match text {
        Some(text) => append(text, new),
        None => *text = Some(new),
    }
}

/// `text.append(more)`, counted in unit tests.
fn append(text: &mut Rope, more: Rope) {
    #[cfg(test)]
    appends::bump();
    text.append(more);
}

impl Builder {
    /// Keeps `n` bytes, ending the current replacement if `n > 0`.
    fn retain(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.flush();
        self.old += n;
        self.new += n;
    }

    /// Deletes `n` old bytes.
    fn delete(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.run().deleted += n;
        self.old += n;
    }

    /// Inserts `text`: shared if it is the run's first text or has more than
    /// `MAX_LEAF` bytes, otherwise copied onto the run's tail.
    fn insert(&mut self, text: Rope) {
        if text.is_empty() {
            return;
        }
        let len = text.len();
        let run = run_at(&mut self.run, self.old, self.new);
        // The tail is empty while the run has no text.
        if run.text.is_none() {
            run.text = Some(text);
        } else {
            add(&mut run.text, &mut self.tail, text);
        }
        self.new += len;
    }

    /// Replaces the old bytes `start..end`, which start at or after the end
    /// of the last edit, with `text`.
    fn edit(&mut self, start: usize, end: usize, text: Rope) {
        self.retain(start.saturating_sub(self.old));
        self.delete(end.saturating_sub(start));
        self.insert(text);
    }

    /// The current replacement, started here if there is none.
    fn run(&mut self) -> &mut Run {
        run_at(&mut self.run, self.old, self.new)
    }

    /// Ends the current replacement, if any, settling its tail.
    fn flush(&mut self) {
        if let Some(mut run) = self.run.take() {
            if !self.tail.is_empty() {
                settle(&mut run.text, &mut self.tail);
            }
            self.replacements.push(Replacement {
                start: run.start,
                end: run.start + run.deleted,
                new_start: run.new_start,
                text: run.text,
            });
        }
    }

    /// The change set, keeping the rest of a text of `len_before` bytes.
    fn finish(mut self, len_before: usize) -> ChangeSet {
        self.flush();
        let len_after = self.new + len_before.saturating_sub(self.old);
        ChangeSet {
            replacements: self.replacements.into(),
            len_before,
            len_after,
        }
    }
}

/// Counts the [`Rope::append`] calls that [`Builder`] makes, on this thread,
/// in unit tests, so that they can show that a run of touching edits
/// appends once.
#[cfg(test)]
pub(crate) mod appends {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }

    /// The appends on this thread so far.
    pub(crate) fn read() -> u64 {
        COUNT.with(Cell::get)
    }

    pub(super) fn bump() {
        COUNT.with(|count| count.set(count.get() + 1));
    }
}
