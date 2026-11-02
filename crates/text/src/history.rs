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

//! The undo tree (ADR-0020). Every edit of a buffer goes through its
//! [`History`], which applies the edit to the buffer's rope and keeps what
//! undoes it.
//!
//! - **Revisions** form a tree, numbered in creation order from the
//!   [`root`](History::root). Each keeps its change from its parent and the
//!   inverse, the selections before and after, a time and how it groups.
//!   Nothing is pruned in v1.
//! - **Every call that edits** ([`commit`](History::commit),
//!   [`undo`](History::undo), [`redo`](History::redo) and
//!   [`jump_to`](History::jump_to)) takes the buffer's rope and refuses any
//!   rope that is not the tree the history last produced
//!   ([`HistoryMismatch`](TextError::HistoryMismatch)), even one with the
//!   same text. It changes both the rope and the history, or neither, and
//!   returns a [`Step`]: the text before, the change sets applied in order
//!   (one per hop), the selection to show and the revision now current.
//! - **Grouping** ([`Group`]): a commit makes a new revision, or joins the
//!   current one if it is open and was made by the same command
//!   ([`GroupId`]), or by edits of the same kind within a time window with
//!   the cursors where they were left. Undo, redo, jumps,
//!   [`seal`](History::seal) and [`checkpoint`](History::checkpoint) close
//!   the current revision; moves that do nothing do not. A commit that
//!   changes nothing records nothing. An open revision keeps its change as a
//!   few pieces merged like a binary counter, so a group of K scattered
//!   edits costs O(K log K) composing, and an Insert session stays one
//!   replacement.
//! - **Undo** goes to the parent and restores the selection before;
//!   **redo** goes to the child created or visited last. Committing after an
//!   undo starts a branch and keeps the old one, which
//!   [`jump_to`](History::jump_to) reaches through the lowest common
//!   ancestor.
//! - **Saved state:** [`checkpoint`](History::checkpoint) when a save
//!   starts, [`mark_saved`](History::mark_saved) when it succeeds. Saves of
//!   one buffer must be serialized. [`is_modified`](History::is_modified) is
//!   false exactly at the saved revision, on any branch.
//! - **Time** is a [`std::time::Instant`] from the caller.
//!
//! Deleted text stays in the history's memory until it is dropped.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::selection::Selection;
use crate::transaction::{ChangeSet, Transaction};
use crate::{Rope, TextError};

#[cfg(test)]
mod tests;

/// The index of the root revision.
const ROOT: usize = 0;

/// A revision of a [`History`]: its sequence number there, counted from 0
/// (the root) in creation order. It means nothing in another history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RevisionId(u64);

impl RevisionId {
    /// The sequence number: 0 for the root, and one more for each revision
    /// created after it.
    pub const fn seq(self) -> u64 {
        self.0
    }

    /// The id of the revision at `index`.
    fn of(index: usize) -> Self {
        // Lossless: a usize has at most 64 bits on every target.
        Self(index as u64)
    }
}

/// Names one command, Insert session or macro replay for
/// [`Group::Command`]. Unique in the process, so one id can name one
/// command across several buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(u64);

impl GroupId {
    /// A new id, unlike any other made in this process. O(1), and it never
    /// blocks.
    pub fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// How a commit groups with the current revision. Only an open revision is
/// joined: the one the last commit made or joined, with no undo, redo,
/// jump, [`seal`](History::seal) or [`checkpoint`](History::checkpoint)
/// since. The root is never open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Group {
    /// Always a new revision.
    New,
    /// Joins the current revision if it is open and was made with the same
    /// id: one vim command, Insert session or macro replay (nested commands
    /// reuse the outer id), or one agent proposal.
    Command(GroupId),
    /// Joins the current revision if it is open, was made with the same
    /// `kind`, was last committed into at most `window` before `now` (not
    /// after it), and the selection before has the same anchor and head in
    /// every cursor, and the same primary index, as the selection the
    /// revision was left with. Typing in the default keymap.
    Coalesce {
        /// What kind of edit, such as typing or deleting, in the caller's
        /// numbering.
        kind: u32,
        /// How long after the last commit into the revision this one may
        /// come and still join it.
        window: Duration,
    },
}

/// What a commit or a move did.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Step {
    /// The revision now current.
    pub revision: RevisionId,
    /// The text before the step, on which the first change set's old
    /// offsets lie (for LSP position conversion).
    pub before: Rope,
    /// The change sets applied, in order: one per hop, and none for a
    /// commit that changed nothing. A hop's change set may be the identity
    /// (a group that cancelled out).
    pub changes: Vec<ChangeSet>,
    /// The selection to show.
    pub selection: Selection,
}

/// A sealed revision and its exact text, made by [`History::checkpoint`]
/// when a save starts.
#[derive(Clone, Debug)]
pub struct Checkpoint {
    revision: RevisionId,
    text: Rope,
}

impl Checkpoint {
    /// The revision.
    pub fn revision(&self) -> RevisionId {
        self.revision
    }

    /// The text to write: the revision's text. O(1) to clone.
    pub fn text(&self) -> &Rope {
        &self.text
    }
}

/// The undo tree of one buffer (see the [module documentation](self)).
///
/// `Clone` copies every stored selection and shares the texts and change
/// sets: O(revisions + stored cursors + pending pieces). `Send + Sync`.
/// `Debug` shows counts only, never the text.
///
/// | Operation | Cost |
/// | :- | :- |
/// | [`commit`](History::commit) | the change set's `invert` and `apply`; a join also composes, O(log K) replacements per commit amortized over a group of K scattered edits, and one append for typing at one point |
/// | [`undo`](History::undo), [`redo`](History::redo) | one `apply`; undo first folds an open revision's pieces |
/// | [`jump_to`](History::jump_to) | one `apply` per hop |
/// | everything else | O(1) |
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct History {
    /// Every revision; the index is the sequence number.
    revisions: Vec<Revision>,
    current: usize,
    /// The later pieces of the current revision's change, oldest first.
    pending: Vec<Piece>,
    saved: Option<usize>,
    /// Whether the current revision accepts joins.
    open: bool,
    /// A clone of the text the history last produced: what the buffer's
    /// rope must be, and the text of the current revision.
    text: Rope,
}

/// One revision.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq))]
struct Revision {
    parent: usize,
    /// The child created or visited last, which redo goes to.
    last_child: Option<usize>,
    /// The change from the parent's text and its inverse; only the first
    /// piece while the revision has pending pieces.
    changes: ChangeSet,
    inverse: ChangeSet,
    selection_before: Selection,
    selection_after: Selection,
    /// The last commit into the revision; `None` for the root.
    time: Option<Instant>,
    tag: Tag,
}

/// A piece of an open revision's change.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq))]
struct Piece {
    changes: ChangeSet,
    inverse: ChangeSet,
}

/// How a revision was made, for joining.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Debug))]
enum Tag {
    Root,
    New,
    Command(GroupId),
    Coalesce(u32),
}

impl Tag {
    /// The tag of a revision made by a commit with `group`.
    fn of(group: Group) -> Self {
        match group {
            Group::New => Tag::New,
            Group::Command(id) => Tag::Command(id),
            Group::Coalesce { kind, .. } => Tag::Coalesce(kind),
        }
    }
}

/// What a commit records, worked out before anything changes.
enum Record {
    /// It joins the current revision: its first piece and the pending ones.
    Join(Piece, Vec<Piece>),
    /// It makes a new revision of `piece`, after the current revision's
    /// pieces are folded into `folded`.
    New { folded: Piece, piece: Piece },
}

impl History {
    /// A history whose root revision holds `text`, marked saved and not
    /// open. The root's selections are a caret at 0. O(1).
    pub fn new(text: &Rope) -> Self {
        let caret = Selection::caret(0);
        Self {
            revisions: vec![Revision {
                parent: ROOT,
                last_child: None,
                changes: ChangeSet::identity(text.len()),
                inverse: ChangeSet::identity(text.len()),
                selection_before: caret.clone(),
                selection_after: caret,
                time: None,
                tag: Tag::Root,
            }],
            current: ROOT,
            pending: Vec::new(),
            saved: Some(ROOT),
            open: false,
            text: text.clone(),
        }
    }

    /// The text this history last produced, which the buffer's rope must
    /// be. After an edit that bypassed the history, restore the buffer with
    /// `text().clone()`.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// Applies `transaction` to `text` and records it, as a new revision or
    /// joined to the current one (see [`Group`]).
    ///
    /// `selection_before` is the selection the edit was made from: undo
    /// restores it, and [`Group::Coalesce`] compares it with the selection
    /// the current revision was left with. `now` is when the commit
    /// happens. Committing after an undo starts a new branch.
    ///
    /// A change set that changes nothing records nothing: the step has no
    /// change sets and the current revision stays as it was, open or not
    /// (the next `Coalesce` commit still joins only if the cursors are where
    /// the revision left them).
    ///
    /// # Errors
    ///
    /// `text` and the history are unchanged on error. First, for every
    /// change set:
    /// 1. [`HistoryMismatch`](TextError::HistoryMismatch) if `text` is not
    ///    the tree the history last produced;
    /// 2. [`LengthMismatch`](TextError::LengthMismatch) if the change set
    ///    is for a text of another length.
    ///
    /// Then, for a change set that changes nothing, only the selection
    /// after, as for [`Selection::validate`] on `text`; `selection_before`
    /// is not checked, as nothing is recorded. For any other change set, in
    /// this order:
    /// 3. as for [`Selection::validate`] of `selection_before`, on `text`;
    /// 4. [`NotCharBoundary`](TextError::NotCharBoundary) for the first
    ///    replacement start or end, ascending, inside a char of `text`;
    /// 5. as for [`Selection::validate`] of the selection after, on the
    ///    text after.
    pub fn commit(
        &mut self,
        text: &mut Rope,
        transaction: Transaction,
        selection_before: &Selection,
        now: Instant,
        group: Group,
    ) -> Result<Step, TextError> {
        self.check_text(text)?;
        let (changes, selection) = transaction.into_parts();
        if changes.len_before() != text.len() {
            return Err(TextError::LengthMismatch {
                expected: changes.len_before(),
                actual: text.len(),
            });
        }
        if changes.is_identity() {
            selection.validate(text)?;
            return Ok(Step {
                revision: self.current(),
                before: self.text.clone(),
                changes: Vec::new(),
                selection,
            });
        }
        let current = self.revision(self.current);
        // Stored selections were checked against this same tree.
        let unmoved = same_positions(selection_before, &current.selection_after);
        if !unmoved {
            selection_before.validate(text)?;
        }
        // This checks every replacement's ends, so `apply` cannot fail.
        let inverse = changes.invert(text)?;
        let joins = self.open
            && match group {
                Group::New => false,
                Group::Command(id) => matches!(current.tag, Tag::Command(tag) if tag == id),
                Group::Coalesce { kind, window } => {
                    unmoved
                        && matches!(current.tag, Tag::Coalesce(tag) if tag == kind)
                        && current
                            .time
                            .and_then(|time| now.checked_duration_since(time))
                            .is_some_and(|since| since <= window)
                }
            };
        let piece = Piece {
            changes: changes.clone(),
            inverse,
        };
        let record = if joins {
            let (first, pending) = self.pushed(piece)?;
            Record::Join(first, pending)
        } else {
            Record::New {
                folded: self.folded()?,
                piece,
            }
        };
        let mut edited = text.clone();
        changes.apply(&mut edited)?;
        selection.validate(&edited)?;

        match record {
            Record::Join(first, pending) => {
                let current = self.current;
                let revision = self.revision_mut(current);
                revision.changes = first.changes;
                revision.inverse = first.inverse;
                revision.selection_after = selection.clone();
                revision.time = revision.time.max(Some(now));
                self.pending = pending;
            }
            Record::New { folded, piece } => {
                self.store(folded);
                let parent = self.current;
                self.revisions.push(Revision {
                    parent,
                    last_child: None,
                    changes: piece.changes,
                    inverse: piece.inverse,
                    selection_before: selection_before.clone(),
                    selection_after: selection.clone(),
                    time: Some(now),
                    tag: Tag::of(group),
                });
                let child = self.revisions.len() - 1;
                self.revision_mut(parent).last_child = Some(child);
                self.current = child;
            }
        }
        *text = edited;
        let before = std::mem::replace(&mut self.text, text.clone());
        self.open = true;
        Ok(Step {
            revision: self.current(),
            before,
            changes: vec![changes],
            selection,
        })
    }

    /// Undoes the current revision: applies its inverse to `text` and moves
    /// to its parent, closing the revision. The step's selection is the one
    /// the revision was made from. `Ok(None)` at the root, with nothing
    /// changed.
    ///
    /// # Errors
    ///
    /// [`HistoryMismatch`](TextError::HistoryMismatch) if `text` is not the
    /// tree the history last produced. `text` and the history are unchanged
    /// on error.
    pub fn undo(&mut self, text: &mut Rope) -> Result<Option<Step>, TextError> {
        self.check_text(text)?;
        let child = self.current;
        if child == ROOT {
            return Ok(None);
        }
        let folded = self.folded()?;
        let inverse = folded.inverse.clone();
        let mut edited = text.clone();
        inverse.apply(&mut edited)?;
        self.store(folded);
        let parent = self.revision(child).parent;
        self.revision_mut(parent).last_child = Some(child);
        let selection = self.revision(child).selection_before.clone();
        Ok(Some(self.moved(
            text,
            edited,
            parent,
            vec![inverse],
            selection,
        )))
    }

    /// Redoes the child created or visited last: applies its change to
    /// `text` and moves to it. The step's selection is the one the child was
    /// left with. `Ok(None)` if the current revision has no child, with
    /// nothing changed.
    ///
    /// # Errors
    ///
    /// As for [`undo`](History::undo).
    pub fn redo(&mut self, text: &mut Rope) -> Result<Option<Step>, TextError> {
        self.check_text(text)?;
        let Some(child) = self.revision(self.current).last_child else {
            return Ok(None);
        };
        let changes = self.revision(child).changes.clone();
        let mut edited = text.clone();
        changes.apply(&mut edited)?;
        let selection = self.revision(child).selection_after.clone();
        Ok(Some(self.moved(
            text,
            edited,
            child,
            vec![changes],
            selection,
        )))
    }

    /// Moves to `target` on any branch, through the lowest common ancestor:
    /// undoes up to it, then redoes down to `target`, one change set per
    /// hop, and leaves redo's choices as those moves would. The step's
    /// selection is the one `target` was left with if the path goes down,
    /// else the one the last revision undone was made from. `Ok(None)` if
    /// `target` is current, with nothing changed.
    ///
    /// # Errors
    ///
    /// [`HistoryMismatch`](TextError::HistoryMismatch) as for
    /// [`undo`](History::undo); then
    /// [`RevisionNotFound`](TextError::RevisionNotFound) if `target` is not
    /// in this history. `text` and the history are unchanged on error. A
    /// [`RevisionId`] from another history is not detected when its number
    /// is in range here: ids mean nothing in another history.
    pub fn jump_to(
        &mut self,
        text: &mut Rope,
        target: RevisionId,
    ) -> Result<Option<Step>, TextError> {
        self.check_text(text)?;
        let Some(goal) = self.index_of(target) else {
            return Err(self.not_found(target));
        };
        if goal == self.current {
            return Ok(None);
        }
        // A parent's index is always smaller, so moving the larger index to
        // its parent meets at the lowest common ancestor.
        let (mut ups, mut downs) = (Vec::new(), Vec::new());
        let (mut from, mut to) = (self.current, goal);
        while from != to {
            if from > to {
                ups.push(from);
                from = self.revision(from).parent;
            } else {
                downs.push(to);
                to = self.revision(to).parent;
            }
        }
        downs.reverse();
        let folded = if ups.is_empty() {
            None
        } else {
            Some(self.folded()?)
        };
        let mut edited = text.clone();
        let mut changes = Vec::with_capacity(ups.len() + downs.len());
        for &index in &ups {
            let inverse = match &folded {
                Some(folded) if index == self.current => folded.inverse.clone(),
                _ => self.revision(index).inverse.clone(),
            };
            inverse.apply(&mut edited)?;
            changes.push(inverse);
        }
        for &index in &downs {
            let change = self.revision(index).changes.clone();
            change.apply(&mut edited)?;
            changes.push(change);
        }

        if let Some(folded) = folded {
            self.store(folded);
        }
        for &index in ups.iter().chain(&downs) {
            let parent = self.revision(index).parent;
            self.revision_mut(parent).last_child = Some(index);
        }
        let selection = match (downs.last(), ups.last()) {
            (Some(&target), _) => self.revision(target).selection_after.clone(),
            (None, Some(&undone)) => self.revision(undone).selection_before.clone(),
            (None, None) => self.revision(goal).selection_after.clone(),
        };
        Ok(Some(self.moved(text, edited, goal, changes, selection)))
    }

    /// Closes the current revision: the next commit starts a new one.
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// Seals the current revision and returns it with its text. Call it when
    /// a save starts, write [`Checkpoint::text`] on a worker, and call
    /// [`mark_saved`](History::mark_saved) if the save succeeds. O(1).
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.seal();
        Checkpoint {
            revision: self.current(),
            text: self.text.clone(),
        }
    }

    /// Records that the save of `checkpoint` succeeded: its revision is now
    /// the one on disk. Saves of one buffer must be serialized, so that the
    /// last call matches the disk.
    ///
    /// # Errors
    ///
    /// [`RevisionNotFound`](TextError::RevisionNotFound) if its revision is
    /// not in this history. Nothing changes on error. A checkpoint from
    /// another history is not detected when its revision's number is in
    /// range here: keep checkpoints per buffer.
    pub fn mark_saved(&mut self, checkpoint: &Checkpoint) -> Result<(), TextError> {
        let index = self
            .index_of(checkpoint.revision)
            .ok_or_else(|| self.not_found(checkpoint.revision))?;
        self.saved = Some(index);
        // Already sealed by `checkpoint`; a saved revision is never joined.
        if index == self.current {
            self.open = false;
        }
        Ok(())
    }

    /// Records that the file on disk is no longer any revision's text.
    pub fn clear_saved(&mut self) {
        self.saved = None;
    }

    /// The revision on disk, if any.
    pub fn saved(&self) -> Option<RevisionId> {
        self.saved.map(RevisionId::of)
    }

    /// Whether the current revision is not the one on disk.
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.current)
    }

    /// The current revision.
    pub fn current(&self) -> RevisionId {
        RevisionId::of(self.current)
    }

    /// The oldest revision, which holds the text the history was made with.
    pub fn root(&self) -> RevisionId {
        RevisionId::of(ROOT)
    }

    /// The number of revisions, the root included.
    pub fn revision_count(&self) -> usize {
        self.revisions.len()
    }

    /// Whether [`undo`](History::undo) would move: the current revision is
    /// not the root.
    pub fn can_undo(&self) -> bool {
        self.current != ROOT
    }

    /// Whether [`redo`](History::redo) would move: the current revision has
    /// a child.
    pub fn can_redo(&self) -> bool {
        self.revision(self.current).last_child.is_some()
    }

    /// Checks the invariants: the tree's links, redo's path from the root
    /// to the current revision, the lengths of every change set and the
    /// text, the open revision's pieces, and the canonical form of every
    /// stored change set and selection. O(size of everything stored). For
    /// tests; the messages hold indices and sizes only.
    ///
    /// # Errors
    ///
    /// A description of the first broken invariant.
    #[doc(hidden)]
    pub fn __check_invariants(&self) -> Result<(), String> {
        for index in 0..self.revisions.len() {
            self.check_revision(index)?;
        }
        self.check_state()?;
        self.check_redo_path()
    }

    /// Refuses a rope that is not the tree this history last produced.
    fn check_text(&self, text: &Rope) -> Result<(), TextError> {
        if text.same_tree(&self.text) {
            Ok(())
        } else {
            Err(TextError::HistoryMismatch {
                len: text.len(),
                expected: self.text.len(),
            })
        }
    }

    fn revision(&self, index: usize) -> &Revision {
        self.revisions
            .get(index)
            .expect("revision indices are in range (invariant)")
    }

    fn revision_mut(&mut self, index: usize) -> &mut Revision {
        self.revisions
            .get_mut(index)
            .expect("revision indices are in range (invariant)")
    }

    /// The index of `id`, if it is in this history.
    fn index_of(&self, id: RevisionId) -> Option<usize> {
        usize::try_from(id.0)
            .ok()
            .filter(|&index| index < self.revisions.len())
    }

    fn not_found(&self, id: RevisionId) -> TextError {
        TextError::RevisionNotFound {
            revision: id.0,
            count: self.revisions.len(),
        }
    }

    /// The length of the text of the revision at `index`.
    fn len_after(&self, index: usize) -> usize {
        match self.pending.last() {
            Some(top) if index == self.current => top.changes.len_after(),
            _ => self.revision(index).changes.len_after(),
        }
    }

    /// The pieces of the current revision, its own first, then the pending
    /// ones, oldest first.
    fn pieces(&self) -> impl DoubleEndedIterator<Item = (&ChangeSet, &ChangeSet)> {
        let current = self.revision(self.current);
        std::iter::once((&current.changes, &current.inverse)).chain(
            self.pending
                .iter()
                .map(|piece| (&piece.changes, &piece.inverse)),
        )
    }

    /// The current revision's change and inverse with its pending pieces
    /// folded in, from the top down. Not stored. O(Σ pieces), as their
    /// weights shrink geometrically.
    fn folded(&self) -> Result<Piece, TextError> {
        let mut pieces = self.pieces().rev();
        let (top_changes, top_inverse) = pieces
            .next()
            .expect("the current revision is a piece (invariant)");
        let (mut changes, mut inverse) = (top_changes.clone(), top_inverse.clone());
        for (below, below_inverse) in pieces {
            changes = then(below, &changes)?;
            inverse = then(&inverse, below_inverse)?;
        }
        Ok(Piece { changes, inverse })
    }

    /// Stores `folded` as the current revision's change, with no pending
    /// pieces.
    fn store(&mut self, folded: Piece) {
        let current = self.current;
        let revision = self.revision_mut(current);
        revision.changes = folded.changes;
        revision.inverse = folded.inverse;
        self.pending.clear();
    }

    /// The current revision's pieces with `piece` pushed on top and merged
    /// like a binary counter: while the piece below the top weighs at most
    /// twice the top, the two become one. Returns the new first piece and
    /// the pending ones. Not stored.
    fn pushed(&self, piece: Piece) -> Result<(Piece, Vec<Piece>), TextError> {
        let mut stack: Vec<Piece> = self
            .pieces()
            .map(|(changes, inverse)| Piece {
                changes: changes.clone(),
                inverse: inverse.clone(),
            })
            .collect();
        stack.push(piece);
        while let [.., below, top] = stack.as_slice()
            && weight(&below.changes) <= weight(&top.changes).saturating_mul(2)
        {
            let merged = Piece {
                changes: then(&below.changes, &top.changes)?,
                inverse: then(&top.inverse, &below.inverse)?,
            };
            stack.truncate(stack.len() - 2);
            stack.push(merged);
        }
        let mut stack = stack.into_iter();
        let first = stack
            .next()
            .expect("the current revision is a piece (invariant)");
        Ok((first, stack.collect()))
    }

    /// Finishes a move to the revision at `index`, whose text is `edited`:
    /// `text` and the history's text become it, and the revision is closed.
    fn moved(
        &mut self,
        text: &mut Rope,
        edited: Rope,
        index: usize,
        changes: Vec<ChangeSet>,
        selection: Selection,
    ) -> Step {
        self.current = index;
        self.open = false;
        *text = edited;
        let before = std::mem::replace(&mut self.text, text.clone());
        Step {
            revision: RevisionId::of(index),
            before,
            changes,
            selection,
        }
    }

    /// The links, lengths, tags and stored selections of the revision at
    /// `index`, and the canonical form of its change sets and selections.
    fn check_revision(&self, index: usize) -> Result<(), String> {
        let count = self.revisions.len();
        let Some(r) = self.revisions.get(index) else {
            return Err(format!(
                "revision {index} is not below the {count} revisions"
            ));
        };
        if index == ROOT {
            let identity = r.changes.is_identity()
                && r.inverse.is_identity()
                && r.changes.len_before() == r.inverse.len_before();
            if r.parent != ROOT || !matches!(r.tag, Tag::Root) || r.time.is_some() || !identity {
                return Err(
                    "the root needs parent 0, the root tag, no time and identity changes"
                        .to_string(),
                );
            }
        } else {
            if r.parent >= index {
                return Err(format!(
                    "revision {index}'s parent {} is not below it",
                    r.parent
                ));
            }
            if matches!(r.tag, Tag::Root) || r.time.is_none() {
                return Err(format!("revision {index} has the root's tag or no time"));
            }
            let from = self.len_after(r.parent);
            if r.changes.len_before() != from {
                return Err(format!(
                    "revision {index}'s change is for {} bytes, but its parent {} has {from}",
                    r.changes.len_before(),
                    r.parent
                ));
            }
            if self.revision(r.parent).last_child.is_none() {
                return Err(format!(
                    "revision {} has a child, {index}, but no last child",
                    r.parent
                ));
            }
            let pieces = index == self.current && !self.pending.is_empty();
            if matches!(r.tag, Tag::New) && (r.changes.is_identity() || pieces) {
                return Err(format!(
                    "revision {index} was made by Group::New but changes nothing or has pieces"
                ));
            }
        }
        if r.inverse.len_before() != r.changes.len_after()
            || r.inverse.len_after() != r.changes.len_before()
        {
            return Err(format!(
                "revision {index}'s inverse does not swap its change's lengths"
            ));
        }
        if let Some(child) = r.last_child
            && (child <= index || child >= count || self.revision(child).parent != index)
        {
            return Err(format!(
                "revision {index}'s last child {child} is not a later child of it"
            ));
        }
        for (name, changes) in [("change", &r.changes), ("inverse", &r.inverse)] {
            changes
                .__check_invariants()
                .map_err(|e| format!("revision {index}'s {name}: {e}"))?;
        }
        let ends = [
            ("before", &r.selection_before, r.changes.len_before()),
            ("after", &r.selection_after, self.len_after(index)),
        ];
        for (name, selection, len) in ends {
            selection
                .__check_invariants()
                .map_err(|e| format!("revision {index}'s selection {name}: {e}"))?;
            if let Some(end) = selection.cursors().iter().map(|c| c.end()).max()
                && end > len
            {
                return Err(format!(
                    "revision {index}'s selection {name} reaches {end}, past its {len}-byte text"
                ));
            }
        }
        Ok(())
    }

    /// The current and saved revisions, the open flag, the pending pieces
    /// and the length of the text.
    fn check_state(&self) -> Result<(), String> {
        let count = self.revisions.len();
        if count == 0 || self.current >= count || self.saved.is_some_and(|s| s >= count) {
            return Err(format!(
                "current {} or saved {:?} is not below the {count} revisions",
                self.current, self.saved
            ));
        }
        let newest = self.current == count - 1 && self.current != ROOT;
        if self.open && (!newest || self.saved == Some(self.current)) {
            return Err(format!(
                "revision {} is open but not the newest, or is the root or the saved one",
                self.current
            ));
        }
        if !self.pending.is_empty() && !newest {
            return Err(format!(
                "revision {} has pieces but is not the newest, or is the root",
                self.current
            ));
        }
        let mut pieces = self.pieces();
        if let Some((mut below, _)) = pieces.next() {
            for (index, (changes, inverse)) in pieces.enumerate() {
                if changes.len_before() != below.len_after()
                    || inverse.len_before() != changes.len_after()
                    || inverse.len_after() != changes.len_before()
                {
                    return Err(format!("pending piece {index}'s lengths do not chain"));
                }
                if weight(below) <= weight(changes).saturating_mul(2) {
                    return Err(format!(
                        "pending piece {index} weighs {}, and the piece below it only {}",
                        weight(changes),
                        weight(below)
                    ));
                }
                for (name, set) in [("change", changes), ("inverse", inverse)] {
                    set.__check_invariants()
                        .map_err(|e| format!("pending piece {index}'s {name}: {e}"))?;
                }
                below = changes;
            }
        }
        let want = self.len_after(self.current);
        if self.text.len() != want {
            return Err(format!(
                "the text has {} bytes, but the current revision's has {want}",
                self.text.len()
            ));
        }
        Ok(())
    }

    /// Every revision on the path from the root to the current one is its
    /// parent's last child, so redo from the root reaches the current
    /// revision.
    fn check_redo_path(&self) -> Result<(), String> {
        let mut child = self.current;
        while child != ROOT {
            let Some(parent) = self.revisions.get(child).map(|r| r.parent) else {
                return Err(format!("revision {child} is not in the history"));
            };
            if parent >= child {
                return Err(format!(
                    "revision {child}'s parent {parent} is not below it"
                ));
            }
            let last = self.revisions.get(parent).and_then(|r| r.last_child);
            if last != Some(child) {
                return Err(format!(
                    "revision {child} is on the path to the current revision {}, but its \
                     parent {parent}'s last child is {last:?}",
                    self.current
                ));
            }
            child = parent;
        }
        Ok(())
    }
}

impl fmt::Debug for History {
    /// Counts and indices only, never the text (it may hold secrets).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("revisions", &self.revisions.len())
            .field("current", &self.current)
            .field("saved", &self.saved)
            .field("open", &self.open)
            .field("pending", &self.pending.len())
            .finish()
    }
}

/// Whether two selections have the same anchor and head in every cursor and
/// the same primary index; other cursor fields do not count.
fn same_positions(a: &Selection, b: &Selection) -> bool {
    a.primary_index() == b.primary_index()
        && a.cursors().len() == b.cursors().len()
        && a.cursors()
            .iter()
            .zip(b.cursors())
            .all(|(x, y)| x.anchor == y.anchor && x.head == y.head)
}

/// A piece's weight in the binary counter: its replacements, plus one.
fn weight(changes: &ChangeSet) -> usize {
    changes.changes().len() + 1
}

/// `a`, then `b`; counted in unit tests.
fn then(a: &ChangeSet, b: &ChangeSet) -> Result<ChangeSet, TextError> {
    #[cfg(test)]
    counters::add(a.changes().len() + b.changes().len());
    a.compose(b)
}

/// Counts the replacements the history's composes read, in unit tests, so
/// that they can bound the work of long groups.
#[cfg(test)]
mod counters {
    use std::cell::Cell;

    thread_local! {
        static READ: Cell<u64> = const { Cell::new(0) };
    }

    /// The replacements read on this thread so far.
    pub(super) fn read() -> u64 {
        READ.with(Cell::get)
    }

    pub(super) fn add(count: usize) {
        READ.with(|read| read.set(read.get() + count as u64));
    }
}
