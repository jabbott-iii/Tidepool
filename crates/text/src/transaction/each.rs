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

//! `Transaction::change_each`: one edit per cursor. The edits are kept as an
//! unmerged list, so that cursors map through the edits themselves and a
//! seam between touching edits stays a seam; the canonical change set is
//! built from that list at the end.

use std::ops::Range;

use super::{Assoc, Builder, Place, Transaction, check_ranges, length_after};
use crate::selection::{Cursor, Selection};
use crate::{Rope, TextError};

/// An edit kept after merging the cursors' edits.
struct Kept {
    start: usize,
    end: usize,
    text: Rope,
    /// Where `text` starts in the text after.
    new_start: usize,
}

impl Kept {
    /// The byte range of the new text, in the text after.
    fn new_range(&self) -> (usize, usize) {
        (self.new_start, self.new_start + self.text.len())
    }
}

/// See [`Transaction::change_each`].
pub(super) fn change_each<T: Into<Rope>>(
    text: &Rope,
    selection: &Selection,
    mut edit: impl FnMut(Cursor) -> Option<(Range<usize>, T, Place)>,
) -> Result<Transaction, TextError> {
    selection.validate(text)?;
    let cursors = selection.cursors();
    let asked: Vec<Option<(Range<usize>, Rope, Place)>> = cursors
        .iter()
        .map(|&cursor| edit(cursor).map(|(range, new, place)| (range, new.into(), place)))
        .collect();
    // Every range is checked, an edit that does nothing included, as
    // `ChangeSet::new` checks one before dropping it.
    check_ranges(
        text,
        asked
            .iter()
            .flatten()
            .map(|(range, _, _)| (range.start, range.end)),
    )?;

    // The edits that do something, with the cursor that made each, sorted
    // stably by (start, end). An edit that does nothing counts as `None`
    // from here on.
    let mut edits: Vec<(usize, Range<usize>, Rope)> = Vec::with_capacity(asked.len());
    let mut places: Vec<Option<Place>> = Vec::with_capacity(asked.len());
    for (cursor, request) in asked.into_iter().enumerate() {
        match request {
            Some((range, new, place)) if range.start < range.end || !new.is_empty() => {
                edits.push((cursor, range, new));
                places.push(Some(place));
            }
            _ => places.push(None),
        }
    }
    let key = |edit: &(usize, Range<usize>, Rope)| (edit.1.start, edit.1.end);
    if !edits.is_sorted_by_key(key) {
        edits.sort_by_key(key);
    }

    // Merge, comparing each edit with the one kept just before it: an edit
    // with the same range and text collapses into it, a deletion that
    // overlaps a kept deletion extends it, any other overlap is an error.
    // So of identical pure inserts at one offset, only consecutive ones
    // collapse. `owner[cursor]` is the kept edit that took the cursor's own
    // edit.
    let mut kept: Vec<Kept> = Vec::with_capacity(edits.len());
    let mut owner = vec![0; cursors.len()];
    for (cursor, range, new) in edits {
        match kept.last_mut() {
            Some(prev)
                if prev.start == range.start && prev.end == range.end && prev.text == new => {}
            Some(prev) if range.start < prev.end && prev.text.is_empty() && new.is_empty() => {
                prev.end = prev.end.max(range.end);
            }
            Some(prev) if range.start < prev.end => {
                return Err(TextError::OverlappingEdits {
                    end: prev.end,
                    start: range.start,
                });
            }
            _ => kept.push(Kept {
                start: range.start,
                end: range.end,
                text: new,
                new_start: 0,
            }),
        }
        if let Some(slot) = owner.get_mut(cursor) {
            *slot = kept.len().saturating_sub(1);
        }
    }

    length_after(
        text.len(),
        kept.iter().map(|k| (k.end - k.start, k.text.len())),
    )?;
    let (mut removed, mut added) = (0, 0);
    for k in &mut kept {
        k.new_start = k.start - removed + added;
        removed += k.end - k.start;
        added += k.text.len();
    }

    let mut seams = Seams {
        kept: &kept,
        index: 0,
        removed: 0,
        added: 0,
    };
    let placed: Vec<Cursor> = cursors
        .iter()
        .zip(&places)
        .zip(&owner)
        .map(|((&cursor, &place), &own)| {
            let own = kept.get(own).map(Kept::new_range);
            match (place, own) {
                (Some(Place::Start), Some((start, _))) => Cursor::caret(start),
                (Some(Place::End), Some((_, end))) => Cursor::caret(end),
                (Some(Place::Select), Some((start, end))) => Cursor::new(start, end),
                _ => seams.cursor(cursor),
            }
        })
        .collect();
    let selection = Selection::from_cursors(placed, selection.primary_index());

    let mut builder = Builder::default();
    for k in kept {
        builder.edit(k.start, k.end, k.text);
    }
    Ok(Transaction {
        changes: builder.finish(text.len()),
        selection,
    })
}

/// Maps positions that do not decrease through the unmerged kept edits, with
/// an index that only moves forward.
///
/// Strictly inside an edit, `Before` goes to the start of its new text and
/// `After` to its end. Elsewhere `Before` adds the growth of every edit that
/// ends at or before the position and is not a pure insert there, and
/// `After` adds the pure inserts there too; so a position between two
/// touching edits maps between their new texts.
struct Seams<'a> {
    kept: &'a [Kept],
    /// The first edit not yet passed.
    index: usize,
    /// Bytes removed by the edits passed.
    removed: usize,
    /// Bytes added by the edits passed.
    added: usize,
}

impl Seams<'_> {
    /// `cursor` mapped as [`Place::Map`] maps it.
    fn cursor(&mut self, cursor: Cursor) -> Cursor {
        if cursor.is_caret() {
            return Cursor::caret(self.map(cursor.head, Assoc::After));
        }
        let start = self.map(cursor.start(), Assoc::After);
        let end = self.map(cursor.end(), Assoc::Before);
        match (start < end, cursor.is_backward()) {
            (false, _) => Cursor::caret(start),
            (true, false) => Cursor::new(start, end),
            (true, true) => Cursor::new(end, start),
        }
    }

    fn map(&mut self, pos: usize, assoc: Assoc) -> usize {
        while let Some(k) = self.kept.get(self.index)
            && (k.end < pos || (k.end == pos && k.start < pos))
        {
            self.removed += k.end - k.start;
            self.added += k.text.len();
            self.index += 1;
        }
        let rest = self
            .kept
            .get(self.index..)
            .expect("the index stops at the end of the edits (invariant)");
        if let Some(k) = rest.first()
            && k.start < pos
        {
            // Strictly inside: the edits passed all end at or before pos.
            let (start, end) = k.new_range();
            return match assoc {
                Assoc::Before => start,
                Assoc::After => end,
            };
        }
        let before = pos.saturating_sub(self.removed) + self.added;
        match assoc {
            Assoc::Before => before,
            Assoc::After => {
                let inserts = rest
                    .iter()
                    .take_while(|k| k.start == pos && k.end == pos)
                    .map(|k| k.text.len())
                    .sum::<usize>();
                before + inserts
            }
        }
    }
}
