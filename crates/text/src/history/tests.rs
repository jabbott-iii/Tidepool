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

//! Unit tests of the history in the tiny rope shape.
//!
//! - **The acceptance model** keeps every revision's text as a `String` and
//!   decides joins on its own. Random sequences of commits (every grouping,
//!   long command groups, faults), undos, redos, jumps and saved markers
//!   are checked against it after every operation, with the apply path
//!   forced at random before each call that may apply. At the end every
//!   revision is reached by `jump_to` with its exact text and selection,
//!   undo returns the first text, and redo follows the last children.
//! - **Fixed cases:** grouping, saved markers, navigation, errors, and
//!   moves that do nothing.
//! - **Costs:** an Insert session stays one replacement and appends once
//!   per keystroke; a long command group composes O(K log K) replacements.
//!
//! "Unchanged" means that the rope is the same tree, that the history
//! equals its clone from before (its `PartialEq` exists in unit tests
//! only), and that the history's text is the same tree. Errors are compared
//! by their `Debug` text.

use std::ops::Range;
use std::time::{Duration, Instant};

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;

use super::{
    Checkpoint, Group, GroupId, History, Piece, Revision, RevisionId, Step, Tag, counters,
};
use crate::pos::tests::byte_error;
use crate::rope::tests::{broken_rope, floor_boundary};
use crate::transaction::appends;
use crate::transaction::paths::{self, Force};
use crate::transaction::tests::{
    Plan, Request, Tally, boundary, build, built, each_model, edit_text, edits, request, run_each,
    same_outcome, selection, style, text,
};
use crate::{Assoc, ChangeSet, Cursor, Rope, Selection, TextError, Transaction};

/// The window of the `Coalesce` groups.
const WINDOW: Duration = Duration::from_secs(1);

/// Typing, in the default keymap's sense.
const TYPING: Group = Group::Coalesce {
    kind: 1,
    window: WINDOW,
};

/// Deleting.
const DELETING: Group = Group::Coalesce {
    kind: 2,
    window: WINDOW,
};

/// An instant an hour from now, so that tests can also go back in time.
fn base() -> Instant {
    Instant::now() + Duration::from_secs(3_600)
}

// ---------------------------------------------------------------------------
// The acceptance model.

/// A selection's `(anchor, head)` pairs and its primary index.
type Shape = (Vec<(usize, usize)>, usize);

fn shape(selection: &Selection) -> Shape {
    let pairs = selection
        .cursors()
        .iter()
        .map(|c| (c.anchor, c.head))
        .collect();
    (pairs, selection.primary_index())
}

/// What `Selection::validate` must give for `shape` on `text`.
fn validate(shape: &Shape, text: &str) -> Result<(), TextError> {
    for &(anchor, head) in &shape.0 {
        byte_error(text, anchor.min(head))?;
        byte_error(text, anchor.max(head))?;
    }
    Ok(())
}

/// How the model groups a revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Root,
    New,
    Command(GroupId),
    Coalesce(u32),
}

impl Kind {
    /// The kind of a revision a commit with `group` makes.
    fn of(group: Group) -> Self {
        match group {
            Group::New => Kind::New,
            Group::Command(id) => Kind::Command(id),
            Group::Coalesce { kind, .. } => Kind::Coalesce(kind),
        }
    }

    /// The history's tag, in the model's terms.
    fn of_tag(tag: Tag) -> Self {
        match tag {
            Tag::Root => Kind::Root,
            Tag::New => Kind::New,
            Tag::Command(id) => Kind::Command(id),
            Tag::Coalesce(kind) => Kind::Coalesce(kind),
        }
    }
}

/// One revision of the model, with its whole text.
struct Node {
    parent: usize,
    last_child: Option<usize>,
    text: String,
    before: Shape,
    after: Shape,
    time: Option<Instant>,
    kind: Kind,
}

/// The history worked out on `String`s: every revision's text, and the
/// rules of ADR-0020 applied directly.
struct Model {
    nodes: Vec<Node>,
    current: usize,
    saved: Option<usize>,
    open: bool,
}

/// What a commit or a move must give: the revision then current, its text,
/// the selection to show, and the number of change sets, one per hop: the
/// revision each hop reaches is in `path`.
struct Want {
    revision: usize,
    text: String,
    selection: Shape,
    hops: usize,
    path: Vec<usize>,
}

/// A commit as the model sees it.
struct View {
    /// The length the change set is for.
    len_before: usize,
    /// Whether the change set changes nothing.
    identity: bool,
    /// Ends of its replacements, ascending, that may be inside a char.
    edges: Vec<usize>,
    /// The text after, worked out on a `String`.
    after_text: String,
    before: Shape,
    after: Shape,
    now: Instant,
    group: Group,
}

impl Model {
    fn new(text: &str) -> Self {
        let caret = (vec![(0, 0)], 0);
        Self {
            nodes: vec![Node {
                parent: 0,
                last_child: None,
                text: text.to_string(),
                before: caret.clone(),
                after: caret,
                time: None,
                kind: Kind::Root,
            }],
            current: 0,
            saved: Some(0),
            open: false,
        }
    }

    fn text(&self) -> &str {
        &self.nodes[self.current].text
    }

    /// The error for an equal copy of the text that is not the history's
    /// rope.
    fn mismatch(&self) -> TextError {
        let len = self.text().len();
        TextError::HistoryMismatch { len, expected: len }
    }

    /// What a step that reaches `revision` through `path`, one revision per
    /// hop, must give.
    fn want(&self, revision: usize, selection: Shape, path: Vec<usize>) -> Want {
        Want {
            revision,
            text: self.nodes[revision].text.clone(),
            selection,
            hops: path.len(),
            path,
        }
    }

    fn commit(&mut self, view: &View) -> Result<Option<Want>, TextError> {
        let text = self.text();
        if view.len_before != text.len() {
            return Err(TextError::LengthMismatch {
                expected: view.len_before,
                actual: text.len(),
            });
        }
        if view.identity {
            validate(&view.after, text)?;
            return Ok(Some(self.want(
                self.current,
                view.after.clone(),
                Vec::new(),
            )));
        }
        validate(&view.before, text)?;
        if let Some(&byte) = view.edges.iter().find(|&&at| !text.is_char_boundary(at)) {
            return Err(TextError::NotCharBoundary { byte });
        }
        validate(&view.after, &view.after_text)?;
        let node = &self.nodes[self.current];
        let joins = self.open
            && match view.group {
                Group::New => false,
                Group::Command(id) => node.kind == Kind::Command(id),
                Group::Coalesce { kind, window } => {
                    node.kind == Kind::Coalesce(kind)
                        && node.after == view.before
                        && node
                            .time
                            .is_some_and(|time| time <= view.now && view.now - time <= window)
                }
            };
        if joins {
            let node = &mut self.nodes[self.current];
            node.text.clone_from(&view.after_text);
            node.after = view.after.clone();
            node.time = node.time.max(Some(view.now));
        } else {
            self.nodes.push(Node {
                parent: self.current,
                last_child: None,
                text: view.after_text.clone(),
                before: view.before.clone(),
                after: view.after.clone(),
                time: Some(view.now),
                kind: Kind::of(view.group),
            });
            let child = self.nodes.len() - 1;
            self.nodes[self.current].last_child = Some(child);
            self.current = child;
        }
        self.open = true;
        Ok(Some(self.want(
            self.current,
            view.after.clone(),
            vec![self.current],
        )))
    }

    fn undo(&mut self) -> Option<Want> {
        let child = self.current;
        if child == 0 {
            return None;
        }
        let parent = self.nodes[child].parent;
        self.nodes[parent].last_child = Some(child);
        self.current = parent;
        self.open = false;
        Some(self.want(parent, self.nodes[child].before.clone(), vec![parent]))
    }

    fn redo(&mut self) -> Option<Want> {
        let child = self.nodes[self.current].last_child?;
        self.current = child;
        self.open = false;
        Some(self.want(child, self.nodes[child].after.clone(), vec![child]))
    }

    /// Whether `a` is `b` or one of its ancestors.
    fn is_ancestor(&self, a: usize, mut b: usize) -> bool {
        while b != a && b != 0 {
            b = self.nodes[b].parent;
        }
        a == b
    }

    fn jump_to(&mut self, target: u64) -> Result<Option<Want>, TextError> {
        let count = self.nodes.len();
        let Some(goal) = usize::try_from(target).ok().filter(|&goal| goal < count) else {
            return Err(TextError::RevisionNotFound {
                revision: target,
                count,
            });
        };
        if goal == self.current {
            return Ok(None);
        }
        // The current revision's ancestors, then the first of them that is
        // also the goal's.
        let mut ancestors = vec![self.current];
        while let Some(&last) = ancestors.last()
            && last != 0
        {
            ancestors.push(self.nodes[last].parent);
        }
        let mut downs = Vec::new();
        let mut at = goal;
        while !ancestors.contains(&at) {
            downs.push(at);
            at = self.nodes[at].parent;
        }
        let ups: Vec<usize> = ancestors.iter().copied().take_while(|&a| a != at).collect();
        downs.reverse();
        for &index in ups.iter().chain(&downs) {
            let parent = self.nodes[index].parent;
            self.nodes[parent].last_child = Some(index);
        }
        let selection = match (downs.last(), ups.last()) {
            (Some(&down), _) => self.nodes[down].after.clone(),
            (None, Some(&up)) => self.nodes[up].before.clone(),
            (None, None) => unreachable!("the goal is not current"),
        };
        // Undoing an up reaches its parent; redoing a down reaches it.
        let path = ups
            .iter()
            .map(|&up| self.nodes[up].parent)
            .chain(downs.iter().copied())
            .collect();
        self.current = goal;
        self.open = false;
        Ok(Some(self.want(goal, selection, path)))
    }

    fn mark_saved(&mut self, revision: u64) -> Result<(), TextError> {
        let count = self.nodes.len();
        let Some(index) = usize::try_from(revision)
            .ok()
            .filter(|&index| index < count)
        else {
            return Err(TextError::RevisionNotFound { revision, count });
        };
        self.saved = Some(index);
        if index == self.current {
            self.open = false;
        }
        Ok(())
    }
}

/// A commit to make, and what the model needs to know of it.
struct Commit {
    transaction: Transaction,
    selection_before: Selection,
    now: Instant,
    group: Group,
    len_before: usize,
    identity: bool,
    edges: Vec<usize>,
    after_text: String,
    /// The fault put in, if any (an index into [`FAULTS`]).
    fault: Option<usize>,
}

/// The faults a commit can have.
const FAULTS: [&str; 5] = [
    "a change set for another length",
    "a change set with an end inside a char",
    "a bad selection before",
    "a bad selection after",
    "an equal rope that is not the history's",
];

/// The index of the foreign-rope fault in [`FAULTS`].
const FOREIGN: usize = 4;

/// The kinds of case the acceptance tests count. A case counts once per
/// kind it meets.
const KINDS: [&str; 15] = [
    "a commit joined by Command",
    "a commit joined by Coalesce",
    "a commit branched after an undo",
    "a commit that changed nothing",
    "a commit made by change_each",
    "refused: a change set for another length",
    "refused: a change set with an end inside a char",
    "refused: a bad selection before",
    "refused: a bad selection after",
    "refused: an equal rope that is not the history's",
    "a long Command run",
    "an undo or jump folded pending pieces",
    "a jump went up and then down",
    "a refused mark_saved or jump_to",
    "unmodified away from the root",
];

/// The number of kinds.
const KIND_COUNT: usize = KINDS.len();

/// One sequence of operations on a rope, its history and the model.
struct Driver {
    rng: Pcg32,
    /// Picks the apply path before each call, on a stream of its own.
    paths: Pcg32,
    rope: Rope,
    history: History,
    model: Model,
    initial: String,
    /// The selection the editor shows: the last step's.
    shown: Selection,
    clock: Instant,
    /// The command id in use.
    group: GroupId,
    /// Checkpoints taken, oldest first.
    tokens: Vec<Checkpoint>,
    met: [bool; KIND_COUNT],
    /// Operations checked so far.
    ops: usize,
}

impl Driver {
    fn new(plan: &Plan) -> Self {
        let mut rng = plan.rng();
        let initial = text(&mut rng);
        let rope = Rope::from(initial.as_str());
        Self {
            rng,
            paths: Pcg32::new(plan.seed, u64::from(plan.size) | (1 << 32)),
            history: History::new(&rope),
            model: Model::new(&initial),
            rope,
            initial,
            shown: Selection::caret(0),
            clock: base(),
            group: GroupId::fresh(),
            tokens: Vec::new(),
            met: [false; KIND_COUNT],
            ops: 0,
        }
    }

    /// `count` random operations, then the end-of-sequence checks.
    ///
    /// The mix: commits 50%, undo 15%, redo 10%, `jump_to` 10%, `seal` 3%,
    /// `checkpoint` 3%, `mark_saved` 5%, `clear_saved` 1%, and 3% an undo,
    /// redo or jump given an equal rope that is not the history's. One
    /// sequence in ten also has a run of 20-40 commits of one command.
    fn run(&mut self, count: u32) -> Result<(), String> {
        let long_run = (self.rng.range(0..10u32) == 0).then(|| self.rng.range(0..count));
        for op in 0..count {
            if long_run == Some(op) {
                self.long_run()?;
            }
            match self.rng.range(0..100u32) {
                0..50 => {
                    if let Some(commit) = self.propose()? {
                        self.commit(commit)?;
                    }
                }
                50..65 => {
                    self.undo(false)?;
                }
                65..75 => {
                    self.redo(false)?;
                }
                75..85 => self.jump(false)?,
                85..88 => self.seal()?,
                88..91 => self.checkpoint()?,
                91..96 => self.mark_saved()?,
                96..97 => self.clear_saved()?,
                _ => match self.rng.range(0..3u32) {
                    0 => {
                        self.undo(true)?;
                    }
                    1 => {
                        self.redo(true)?;
                    }
                    _ => self.jump(true)?,
                },
            }
        }
        self.finish()
    }

    /// The time of the next commit: the same, 10 ms, exactly the window,
    /// the window and 1 ns, or 2 s later; 3% of the time earlier.
    fn tick(&mut self) -> Instant {
        let step = match self.rng.range(0..100u32) {
            0..3 => {
                let back = Duration::from_millis(u64::from(self.rng.range(1..3_000u32)));
                self.clock = self.clock.checked_sub(back).unwrap_or(self.clock);
                return self.clock;
            }
            3..30 => Duration::ZERO,
            30..60 => Duration::from_millis(10),
            60..75 => WINDOW,
            75..90 => WINDOW + Duration::from_nanos(1),
            _ => Duration::from_secs(2),
        };
        self.clock += step;
        self.clock
    }

    /// `New` 20%, `Command` 35% (a fresh id a quarter of the time), and
    /// `Coalesce` of kind 1 or 2 45%.
    fn next_group(&mut self) -> Group {
        match self.rng.range(0..100u32) {
            0..20 => Group::New,
            20..55 => {
                if self.rng.range(0..4u32) == 0 {
                    self.group = GroupId::fresh();
                }
                Group::Command(self.group)
            }
            _ => Group::Coalesce {
                kind: self.rng.range(1..3u32),
                window: WINDOW,
            },
        }
    }

    /// The selection a commit is made from: the one shown 60% of the time,
    /// otherwise one anywhere in the text.
    fn selection_before(&mut self) -> Selection {
        if self.rng.range(0..10u32) < 6 {
            self.shown.clone()
        } else {
            let text = self.model.text().to_string();
            selection(&mut self.rng, &text)
        }
    }

    /// A commit of `edits`, valid on the current text: the selection after
    /// is the one before mapped, or one anywhere in the text after.
    fn edit_commit(
        &mut self,
        edits: &[(Range<usize>, String)],
        group: Group,
    ) -> Result<Commit, String> {
        let text = self.model.text().to_string();
        let now = self.tick();
        let selection_before = self.selection_before();
        let after_text = built(&text, edits).map_err(|e| format!("the model: {e}"))?;
        let changes = build(&self.rope, edits).map_err(|e| format!("valid edits: {e}"))?;
        let selection_after = match self.rng.range(0..4u32) {
            0 => Ok(selection(&mut self.rng, &after_text)),
            1 => selection_before.map(&changes, Assoc::Before),
            _ => selection_before.map(&changes, Assoc::After),
        }
        .map_err(|e| format!("mapping a valid selection: {e}"))?;
        Ok(Commit {
            transaction: Transaction::new(changes, selection_after),
            selection_before,
            now,
            group,
            len_before: text.len(),
            identity: edits
                .iter()
                .all(|(range, new)| range.is_empty() && new.is_empty()),
            edges: Vec::new(),
            after_text,
            fault: None,
        })
    }

    /// A random commit: 70% from an edit list, 30% from `change_each` with
    /// one style for every cursor; 8% with one fault. `None` if
    /// `change_each` refused the edits (as its model says it must).
    fn propose(&mut self) -> Result<Option<Commit>, String> {
        let group = self.next_group();
        let mut commit = if self.rng.range(0..10u32) < 7 {
            let text = self.model.text().to_string();
            let edits = edits(&mut self.rng, &text, true);
            self.edit_commit(&edits, group)?
        } else {
            let text = self.model.text().to_string();
            let now = self.tick();
            let selection_before = self.selection_before();
            let style = style(&mut self.rng);
            let requests: Vec<Request> = selection_before
                .cursors()
                .iter()
                .map(|&cursor| request(&mut self.rng, &text, cursor, &style))
                .collect();
            let want = each_model(&text, &selection_before, &requests);
            let got = run_each(&self.rope, &selection_before, &requests)?;
            same_outcome(|| "change_each".to_string(), &got, &want)?;
            let (Ok(transaction), Ok((after_text, _, kept))) = (got, want) else {
                return Ok(None);
            };
            self.met[4] = true;
            Commit {
                transaction,
                selection_before,
                now,
                group,
                len_before: text.len(),
                identity: kept.is_empty(),
                edges: Vec::new(),
                after_text,
                fault: None,
            }
        };
        if self.rng.range(0..100u32) < 8 {
            self.fault(&mut commit)?;
        }
        Ok(Some(commit))
    }

    /// A cursor with a position past the end of `text` or inside one of
    /// its chars.
    fn bad_cursor(&mut self, text: &str) -> Cursor {
        let inside: Vec<usize> = (0..text.len())
            .filter(|&at| !text.is_char_boundary(at))
            .collect();
        let bad = match (self.rng.range(0..2u32), self.rng.choose(&inside)) {
            (0, Some(&at)) => at,
            _ => text.len() + self.rng.range(1..4u32) as usize,
        };
        if self.rng.range(0..2u32) == 0 {
            Cursor::caret(bad)
        } else {
            Cursor::new(boundary(&mut self.rng, text), bad)
        }
    }

    /// Puts one fault in `commit` (see [`FAULTS`]).
    fn fault(&mut self, commit: &mut Commit) -> Result<(), String> {
        let text = self.model.text().to_string();
        let inside: Vec<usize> = (0..text.len())
            .filter(|&at| !text.is_char_boundary(at))
            .collect();
        let mut fault = self.rng.range(0..FAULTS.len() as u32) as usize;
        if fault == 1 && inside.is_empty() {
            fault = 0;
        }
        match fault {
            0 => {
                let mut other = text.clone();
                match self.rng.range(0..3u32) {
                    0 if !text.is_empty() => {
                        other.truncate(floor_boundary(&text, text.len() - 1));
                    }
                    _ => other.push_str(&"x".repeat(self.rng.range(1..4u32) as usize)),
                }
                let changes = if self.rng.range(0..3u32) == 0 {
                    ChangeSet::identity(other.len())
                } else {
                    let edits = edits(&mut self.rng, &other, true);
                    build(&Rope::from(other.as_str()), &edits).map_err(|e| e.to_string())?
                };
                commit.len_before = changes.len_before();
                commit.transaction =
                    Transaction::new(changes, commit.transaction.selection().clone());
            }
            1 => {
                // Made on an ASCII text of the same length, with one end
                // inside a char of the real text.
                let at = *self.rng.choose(&inside).ok_or("no byte inside a char")?;
                let (range, new) = match self.rng.range(0..3u32) {
                    0 => (at..at, "y"),
                    1 => (floor_boundary(&text, at)..at, ""),
                    _ => (
                        at..self.rng.range(at as u32 + 1..text.len() as u32 + 1) as usize,
                        "z",
                    ),
                };
                let ascii = Rope::from("x".repeat(text.len()).as_str());
                let changes = ChangeSet::new(&ascii, [(range.clone(), new)])
                    .map_err(|e| format!("an edit of the ASCII text: {e}"))?;
                commit.identity = false;
                commit.edges = vec![range.start, range.end];
                commit.transaction =
                    Transaction::new(changes, commit.transaction.selection().clone());
            }
            2 => {
                let bad = self.bad_cursor(&text);
                let others = commit.selection_before.cursors().to_vec();
                commit.selection_before = Selection::new(bad, others);
            }
            3 => {
                let after_text = commit.after_text.clone();
                let bad = self.bad_cursor(&after_text);
                let (changes, selection) = commit.transaction.clone().into_parts();
                let after = Selection::new(bad, selection.cursors().iter().copied());
                commit.transaction = Transaction::new(changes, after);
            }
            _ => {}
        }
        commit.fault = Some(fault);
        Ok(())
    }

    /// A run of 20-40 commits of one fresh command, each a small edit
    /// somewhere in the text.
    fn long_run(&mut self) -> Result<(), String> {
        let group = Group::Command(GroupId::fresh());
        for _ in 0..self.rng.range(20..41u32) {
            let text = self.model.text().to_string();
            let at = boundary(&mut self.rng, &text);
            let end = match self.rng.range(0..2u32) {
                0 => at,
                _ => text.ceil_char_boundary(at + self.rng.range(1..4u32) as usize),
            };
            let mut new = edit_text(&mut self.rng);
            if at == end && new.is_empty() {
                new.push('x');
            }
            let commit = self.edit_commit(&[(at..end, new)], group)?;
            self.commit(commit)?;
        }
        self.met[10] = true;
        Ok(())
    }

    /// Makes `commit` on the history and the model and checks both.
    fn commit(&mut self, commit: Commit) -> Result<(), String> {
        let Commit {
            transaction,
            selection_before,
            now,
            group,
            len_before,
            identity,
            edges,
            after_text,
            fault,
        } = commit;
        let view = View {
            before: shape(&selection_before),
            after: shape(transaction.selection()),
            len_before,
            identity,
            edges,
            after_text,
            now,
            group,
        };
        let count = self.model.nodes.len();
        let branches = self.model.nodes[self.model.current].last_child.is_some();
        let committed = self.call(
            "commit",
            fault == Some(FOREIGN),
            |history, rope| {
                history
                    .commit(rope, transaction, &selection_before, now, group)
                    .map(Some)
            },
            |model| model.commit(&view),
        )?;
        match (fault, committed) {
            (Some(fault), false) => self.met[5 + fault] = true,
            (_, true) if identity => self.met[3] = true,
            (_, true) => {
                let grew = self.model.nodes.len() > count;
                if grew && branches {
                    self.met[2] = true;
                }
                match (grew, group) {
                    (false, Group::Command(_)) => self.met[0] = true,
                    (false, _) => self.met[1] = true,
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn undo(&mut self, foreign: bool) -> Result<bool, String> {
        if !self.history.pending.is_empty() && !foreign {
            self.met[11] = true;
        }
        self.call(
            "undo",
            foreign,
            |history, rope| history.undo(rope),
            |model| Ok(model.undo()),
        )
    }

    fn redo(&mut self, foreign: bool) -> Result<bool, String> {
        self.call(
            "redo",
            foreign,
            |history, rope| history.redo(rope),
            |model| Ok(model.redo()),
        )
    }

    /// A jump to a random revision, 5% of the time one this history does
    /// not have.
    fn jump(&mut self, foreign: bool) -> Result<(), String> {
        let count = self.model.nodes.len();
        let target = match self.rng.range(0..40u32) {
            0 => u64::MAX,
            1 => (count + self.rng.range(0..3u32) as usize) as u64,
            _ => u64::from(self.rng.range(0..count as u32)),
        };
        self.jump_to(RevisionId(target), foreign).map(|_| ())
    }

    fn jump_to(&mut self, target: RevisionId, foreign: bool) -> Result<bool, String> {
        let current = self.model.current;
        let known = usize::try_from(target.seq()).is_ok_and(|goal| goal < self.model.nodes.len());
        if let Ok(goal) = usize::try_from(target.seq())
            && known
            && !foreign
        {
            if !self.model.is_ancestor(goal, current) && !self.model.is_ancestor(current, goal) {
                self.met[12] = true;
            }
            if !self.model.is_ancestor(current, goal) && !self.history.pending.is_empty() {
                self.met[11] = true;
            }
        }
        let moved = self.call(
            "jump_to",
            foreign,
            |history, rope| history.jump_to(rope, target),
            |model| model.jump_to(target.seq()),
        );
        if moved.is_ok() && !foreign && !known {
            self.met[13] = true;
        }
        moved
    }

    fn seal(&mut self) -> Result<(), String> {
        self.history.seal();
        self.model.open = false;
        self.checked("seal")
    }

    fn checkpoint(&mut self) -> Result<(), String> {
        let token = self.history.checkpoint();
        self.model.open = false;
        if token.revision() != RevisionId::of(self.model.current)
            || !token.text().same_tree(self.history.text())
        {
            return Err(format!(
                "op {}: the checkpoint is not of the current revision and its text",
                self.ops
            ));
        }
        self.tokens.push(token);
        self.checked("checkpoint")
    }

    /// Marks the newest checkpoint saved, or an older one, or one of a
    /// revision this history does not have.
    fn mark_saved(&mut self) -> Result<(), String> {
        let count = self.history.revision_count();
        let token = match (self.rng.range(0..5u32), self.tokens.len()) {
            (0, _) | (_, 0) => Checkpoint {
                revision: RevisionId((count + self.rng.range(0..3u32) as usize) as u64),
                text: Rope::new(),
            },
            (1..3, newest) => self.tokens[newest - 1].clone(),
            (_, len) => self.tokens[self.rng.range(0..len as u32) as usize].clone(),
        };
        let snapshot = self.history.clone();
        let got = self.history.mark_saved(&token);
        let want = self.model.mark_saved(token.revision().seq());
        same_outcome(|| format!("op {}: mark_saved", self.ops), &got, &want)?;
        if got.is_err() {
            self.met[13] = true;
            if self.history != snapshot {
                return Err(format!(
                    "op {}: a refused mark_saved changed the history",
                    self.ops
                ));
            }
        }
        self.checked("mark_saved")
    }

    fn clear_saved(&mut self) -> Result<(), String> {
        self.history.clear_saved();
        self.model.saved = None;
        self.checked("clear_saved")
    }

    /// Runs `real` on the history and `model` on the model, or for
    /// `foreign` `real` on an equal copy of the rope (which must then be
    /// refused), and checks the outcome. Whether the call returned a step.
    /// The apply path is forced at random first (or left to the rule).
    fn call(
        &mut self,
        what: &str,
        foreign: bool,
        real: impl FnOnce(&mut History, &mut Rope) -> Result<Option<Step>, TextError>,
        model: impl FnOnce(&mut Model) -> Result<Option<Want>, TextError>,
    ) -> Result<bool, String> {
        paths::force(match self.paths.range(0..3u32) {
            0 => Force::Auto,
            1 => Force::Replace,
            _ => Force::Rebuild,
        });
        let snapshot = self.history.clone();
        let rope_before = self.rope.clone();
        let (got, want) = if foreign {
            let mut copy = Rope::from(self.model.text());
            let copy_before = copy.clone();
            let got = real(&mut self.history, &mut copy);
            if !copy.same_tree(&copy_before) {
                return Err(format!("op {}: {what} changed a rope it refused", self.ops));
            }
            (got, Err(self.model.mismatch()))
        } else {
            (
                real(&mut self.history, &mut self.rope),
                model(&mut self.model),
            )
        };
        let moved = matches!(got, Ok(Some(_)));
        let at = format!("op {} ({what})", self.ops);
        match (got, want) {
            (Err(got), Err(want)) => {
                if format!("{got:?}") != format!("{want:?}") {
                    return Err(format!("{at}: got {got:?}, want {want:?}"));
                }
                self.unchanged(&at, &snapshot, &rope_before)?;
            }
            (Ok(None), Ok(None)) => self.unchanged(&at, &snapshot, &rope_before)?,
            (Ok(Some(step)), Ok(Some(want))) => {
                self.stepped(&at, step, &want, &rope_before)?;
                if want.hops == 0 {
                    // A commit that changes nothing records nothing.
                    self.unchanged(&at, &snapshot, &rope_before)?;
                }
            }
            (got, want) => {
                let want = want.map(|want| want.map(|w| (w.revision, w.hops)));
                return Err(format!("{at}: got {got:?}, want {want:?}"));
            }
        }
        self.checked(what)?;
        Ok(moved)
    }

    fn unchanged(&self, at: &str, snapshot: &History, rope_before: &Rope) -> Result<(), String> {
        if !self.rope.same_tree(rope_before)
            || self.history != *snapshot
            || !self.history.text.same_tree(&snapshot.text)
        {
            return Err(format!(
                "{at}: a refused call, or one that did nothing, changed the rope or the history"
            ));
        }
        Ok(())
    }

    /// Checks a step against the model's: its revision and selection, its
    /// text before, and that its change sets lead from there through the
    /// text of each revision on the model's path, one per hop, to the text
    /// after.
    fn stepped(&mut self, at: &str, step: Step, want: &Want, before: &Rope) -> Result<(), String> {
        if step.revision != RevisionId::of(want.revision)
            || shape(&step.selection) != want.selection
            || step.changes.len() != want.hops
        {
            return Err(format!(
                "{at}: the step is {:?} with {:?} and {} change sets, want {} with {:?} and {}",
                step.revision,
                shape(&step.selection),
                step.changes.len(),
                want.revision,
                want.selection,
                want.hops
            ));
        }
        if !step.before.same_tree(before) {
            return Err(format!(
                "{at}: the step's text before is not the rope before"
            ));
        }
        let mut text = step.before.clone();
        for ((hop, changes), &reached) in step.changes.iter().enumerate().zip(&want.path) {
            changes
                .apply(&mut text)
                .map_err(|e| format!("{at}: change set {hop} of the step: {e}"))?;
            if text != self.model.nodes[reached].text.as_str() {
                return Err(format!(
                    "{at}: hop {hop} does not reach revision {reached}'s text"
                ));
            }
        }
        if text != want.text.as_str() {
            return Err(format!(
                "{at}: the step's change sets give another text ({} bytes, want {})",
                text.len(),
                want.text.len()
            ));
        }
        self.shown = step.selection;
        Ok(())
    }

    /// Counts the operation, then checks the accessors, the rope and every
    /// revision's links, tag, time and selections against the model, the
    /// invariants of the revisions touched (all of them every 8th
    /// operation), and the rope's invariants.
    fn checked(&mut self, what: &str) -> Result<(), String> {
        let at = format!("op {} ({what})", self.ops);
        self.ops += 1;
        let (history, model) = (&self.history, &self.model);
        let got = (
            history.current(),
            history.saved(),
            history.is_modified(),
            history.revision_count(),
            history.can_undo(),
            history.can_redo(),
            history.root(),
            history.open,
        );
        let want = (
            RevisionId::of(model.current),
            model.saved.map(RevisionId::of),
            model.saved != Some(model.current),
            model.nodes.len(),
            model.current != 0,
            model.nodes[model.current].last_child.is_some(),
            RevisionId::of(0),
            model.open,
        );
        if got != want {
            return Err(format!("{at}: the accessors are {got:?}, want {want:?}"));
        }
        if self.rope != model.text() || !self.rope.same_tree(history.text()) {
            return Err(format!(
                "{at}: the rope ({} bytes) is not the model's text ({} bytes) and the history's tree",
                self.rope.len(),
                model.text().len()
            ));
        }
        for (index, (r, node)) in history.revisions.iter().zip(&model.nodes).enumerate() {
            let got = (
                r.parent,
                r.last_child,
                r.time,
                Kind::of_tag(r.tag),
                shape(&r.selection_before),
                shape(&r.selection_after),
            );
            let want = (
                node.parent,
                node.last_child,
                node.time,
                node.kind,
                node.before.clone(),
                node.after.clone(),
            );
            if got != want {
                return Err(format!("{at}: revision {index} is {got:?}, want {want:?}"));
            }
        }
        let current = &history.revisions[history.current];
        let touched = [
            Some(history.current),
            Some(current.parent),
            current.last_child,
        ];
        for index in touched.into_iter().flatten() {
            history
                .check_revision(index)
                .map_err(|e| format!("{at}: {e}"))?;
        }
        history.check_state().map_err(|e| format!("{at}: {e}"))?;
        self.rope
            .__check_invariants()
            .map_err(|e| format!("{at}: the rope: {e}"))?;
        if self.ops.is_multiple_of(8) {
            history
                .__check_invariants()
                .map_err(|e| format!("{at}: {e}"))?;
        }
        if !history.is_modified() && history.current != 0 {
            self.met[14] = true;
        }
        Ok(())
    }

    /// Jumps to every revision (64 of them if there are more) for its exact
    /// text and selection, which is how redo branches are kept; undoes until
    /// the root for the first text; redoes along the last children.
    fn finish(&mut self) -> Result<(), String> {
        let count = self.model.nodes.len();
        let mut targets: Vec<usize> = (0..count).collect();
        self.rng.shuffle(&mut targets);
        targets.truncate(64);
        for target in targets {
            self.jump_to(RevisionId::of(target), false)?;
            if self.model.current != target || self.rope != self.model.nodes[target].text.as_str() {
                return Err(format!("jump_to({target}) did not reach its text"));
            }
        }
        for _ in 0..count {
            if !self.undo(false)? {
                break;
            }
        }
        if self.history.can_undo() || self.rope != self.initial.as_str() {
            return Err("undoing everything did not give the first text".to_string());
        }
        let mut end = 0;
        while let Some(child) = self.model.nodes[end].last_child {
            end = child;
        }
        for _ in 0..count {
            if !self.redo(false)? {
                break;
            }
        }
        if self.history.current != end || self.rope != self.model.nodes[end].text.as_str() {
            return Err(format!(
                "redoing everything reached revision {}, not {end} and its text",
                self.history.current
            ));
        }
        self.history.__check_invariants()
    }
}

/// One acceptance sequence of `size / 2 + 2` operations.
fn sequence(plan: &Plan, tally: &Tally<KIND_COUNT>) -> Result<(), String> {
    let mut driver = Driver::new(plan);
    let run = driver.run(plan.size / 2 + 2);
    paths::force(Force::Auto);
    run?;
    for (kind, &met) in driver.met.iter().enumerate() {
        if met {
            tally.add(kind);
        }
    }
    Ok(())
}

/// The acceptance property, under `name`, for 1,000 sequences.
fn acceptance(name: &str) {
    let tally = Tally::new(KINDS);
    let start = paths::read();
    check_sized(name, 1_000, Plan::draw, |plan| sequence(plan, &tally));
    let end = paths::read();
    let ran = [end[0] - start[0], end[1] - start[1]];
    assert!(
        ran.iter().all(|&count| count >= 1_000),
        "the apply paths ran {ran:?} times"
    );
    // About 40% of what the default seed meets, so another seed passes too.
    tally.require([
        160, 75, 330, 280, 350, 75, 65, 45, 60, 60, 45, 24, 310, 230, 70,
    ]);
}

/// M1.2's acceptance criterion: undo restores the text exactly after any
/// random sequence, and redo branches are kept (every revision is reached
/// with its exact text).
#[test]
fn undo_restores_every_revision_a() {
    acceptance("undo restores every revision (a)");
}

/// As [`undo_restores_every_revision_a`], on an independent stream.
#[test]
fn undo_restores_every_revision_b() {
    acceptance("undo restores every revision (b)");
}

// ---------------------------------------------------------------------------
// Fixed cases.

/// A rope and its history.
#[derive(Clone)]
struct Buffer {
    rope: Rope,
    history: History,
}

impl Buffer {
    fn new(text: &str) -> Self {
        let rope = Rope::from(text);
        Self {
            history: History::new(&rope),
            rope,
        }
    }

    /// Replaces `range` with `new`, from `before` to `after`.
    fn edit(
        &mut self,
        range: Range<usize>,
        new: &str,
        (before, after): (Selection, Selection),
        now: Instant,
        group: Group,
    ) -> Result<Step, TextError> {
        let changes = ChangeSet::new(&self.rope, [(range, new)])?;
        let transaction = Transaction::new(changes, after);
        self.history
            .commit(&mut self.rope, transaction, &before, now, group)
    }

    /// Types `new` at a caret at `at`, leaving the caret after it.
    #[track_caller]
    fn typed(&mut self, at: usize, new: &str, now: Instant, group: Group) -> Step {
        let carets = (Selection::caret(at), Selection::caret(at + new.len()));
        self.edit(at..at, new, carets, now, group)
            .expect("a valid edit")
    }

    #[track_caller]
    fn undo(&mut self) -> Option<Step> {
        self.history
            .undo(&mut self.rope)
            .expect("the history's own rope")
    }

    #[track_caller]
    fn redo(&mut self) -> Option<Step> {
        self.history
            .redo(&mut self.rope)
            .expect("the history's own rope")
    }

    #[track_caller]
    fn jump(&mut self, seq: u64) -> Option<Step> {
        self.history
            .jump_to(&mut self.rope, RevisionId(seq))
            .expect("a revision of this history")
    }

    fn count(&self) -> usize {
        self.history.revision_count()
    }

    fn seq(&self) -> u64 {
        self.history.current().seq()
    }

    /// Runs `call`, which must fail with `want` and change nothing.
    #[track_caller]
    fn refused<T>(
        &mut self,
        want: TextError,
        call: impl FnOnce(&mut History, &mut Rope) -> Result<T, TextError>,
    ) {
        let (history, rope) = (self.history.clone(), self.rope.clone());
        let got = call(&mut self.history, &mut self.rope).err();
        assert_eq!(format!("{got:?}"), format!("{:?}", Some(want)));
        assert!(
            self.rope.same_tree(&rope)
                && self.history == history
                && self.history.text.same_tree(&history.text),
            "a refused call changed something"
        );
    }
}

fn caret(at: usize) -> Selection {
    Selection::caret(at)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn grouping_rules() {
    let t0 = base();
    // The window's edge: exactly the window joins, a nanosecond more does
    // not, and the window counts from the last commit into the revision.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, TYPING);
    b.typed(1, "b", t0 + WINDOW, TYPING);
    assert_eq!(b.count(), 2);
    let late = t0 + 2 * WINDOW + Duration::from_nanos(1);
    b.typed(2, "c", late, TYPING);
    b.typed(3, "d", late + WINDOW, TYPING);
    assert_eq!(b.count(), 3);
    // A clock going backwards, a different kind, and a cursor jump each
    // start a new revision.
    b.typed(4, "e", late, TYPING);
    b.typed(5, "f", late, DELETING);
    b.edit(0..0, "g", (caret(0), caret(1)), late, DELETING)
        .expect("valid");
    assert_eq!((b.count(), b.rope.to_string()), (6, "gabcdef".to_string()));
    for want in ["abcdef", "abcde", "abcd", "ab", ""] {
        b.undo().expect("a revision to undo");
        assert_eq!(b.rope, want);
    }

    // Cursors count as unmoved only with the same primary too: typing at
    // two carets joins while the primary stays the second, and not once the
    // selection before makes the first primary.
    let mut b = Buffer::new("ab");
    let two = |first: usize, second: usize, primary: usize| {
        let carets = [Cursor::caret(first), Cursor::caret(second)];
        Selection::new(carets[primary], [carets[1 - primary]])
    };
    let type_two = |b: &mut Buffer, at: [usize; 2], before: Selection| {
        let changes =
            ChangeSet::new(&b.rope, [(at[0]..at[0], "x"), (at[1]..at[1], "x")]).expect("valid");
        let after = two(at[0] + 1, at[1] + 2, 1);
        let transaction = Transaction::new(changes, after);
        b.history
            .commit(&mut b.rope, transaction, &before, t0, TYPING)
            .expect("valid");
    };
    type_two(&mut b, [0, 2], two(0, 2, 1));
    type_two(&mut b, [1, 4], two(1, 4, 1));
    assert_eq!((b.count(), b.rope.to_string()), (2, "xxabxx".to_string()));
    type_two(&mut b, [2, 6], two(2, 6, 0));
    assert_eq!((b.count(), b.rope.to_string()), (3, "xxxabxxx".to_string()));

    // A command joins across time gaps, a clock going back, and cursor
    // jumps; `New` between two commits of one command splits it.
    let g = GroupId::fresh();
    let mut b = Buffer::new("one two");
    b.edit(0..0, ">", (caret(5), caret(1)), t0, Group::Command(g))
        .expect("valid");
    b.edit(
        8..8,
        "<",
        (caret(0), caret(9)),
        t0 + ms(3_600_000),
        Group::Command(g),
    )
    .expect("valid");
    b.edit(
        0..1,
        "",
        (caret(3), caret(0)),
        t0 - ms(1),
        Group::Command(g),
    )
    .expect("valid");
    assert_eq!((b.count(), b.rope.to_string()), (2, "one two<".to_string()));
    b.typed(0, "a", t0, Group::New);
    b.typed(0, "b", t0, Group::Command(g));
    assert_eq!(b.count(), 4);

    // Fresh ids differ, and one id used in two histories joins only within
    // each.
    let (g1, g2) = (GroupId::fresh(), GroupId::fresh());
    assert_ne!(g1, g2);
    let (mut x, mut y) = (Buffer::new("x"), Buffer::new("y"));
    for at in 1..4 {
        x.typed(at, "1", t0, Group::Command(g1));
        y.typed(at, "2", t0, Group::Command(g1));
    }
    assert_eq!((x.count(), y.count()), (2, 2));
    x.typed(0, "3", t0, Group::Command(g2));
    assert_eq!((x.rope.to_string(), x.count()), ("3x111".to_string(), 3));

    // A commit that changes nothing records nothing and keeps the revision
    // open; but if it moved the selection, `Coalesce` then does not join.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, TYPING);
    let nothing = |b: &mut Buffer, after: Selection| {
        let before = b.history.clone();
        let transaction = Transaction::new(ChangeSet::identity(b.rope.len()), after.clone());
        let step = b
            .history
            .commit(&mut b.rope, transaction, &caret(1), t0, Group::New)
            .expect("valid");
        assert!(step.changes.is_empty() && step.revision == b.history.current());
        assert_eq!(step.selection, after);
        assert!(
            b.history == before,
            "a commit that changed nothing recorded something"
        );
    };
    nothing(&mut b, caret(1));
    b.typed(1, "b", t0, TYPING);
    assert_eq!(b.count(), 2);
    nothing(&mut b, caret(0));
    assert!(b.history.open);
    b.edit(0..0, "c", (caret(0), caret(1)), t0, TYPING)
        .expect("valid");
    assert_eq!((b.count(), b.rope.to_string()), (3, "cab".to_string()));

    // A group that cancels out stays, with an identity change.
    let mut b = Buffer::new("ab");
    b.edit(1..1, "x", (caret(1), caret(2)), t0, Group::Command(g))
        .expect("valid");
    b.edit(1..2, "", (caret(2), caret(1)), t0, Group::Command(g))
        .expect("valid");
    assert_eq!((b.count(), b.rope.to_string()), (2, "ab".to_string()));
    assert!(b.history.pending.is_empty() && b.history.revisions[1].changes.is_identity());
    assert!(b.history.is_modified());
    let step = b.undo().expect("the cancelled group");
    assert!(step.changes.len() == 1 && step.changes[0].is_identity());
    assert!(!b.history.is_modified());

    // `seal` closes the revision.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, TYPING);
    b.history.seal();
    b.typed(1, "b", t0, TYPING);
    assert_eq!(b.count(), 3);

    // The root never joins, even after an undo back to it.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, Group::Command(g));
    assert_eq!(b.count(), 2);
    b.undo();
    b.typed(0, "b", t0, Group::Command(g));
    assert_eq!((b.count(), b.rope.to_string()), (3, "b".to_string()));
}

#[test]
fn saved_markers() {
    let t0 = base();
    let mut b = Buffer::new("");
    assert_eq!(b.history.saved(), Some(b.history.root()));
    assert!(!b.history.is_modified());
    b.typed(0, "a", t0, TYPING);
    assert!(b.history.is_modified());
    // A checkpoint seals its revision, so typing on starts a new one; the
    // saved text never changes.
    let old = b.history.checkpoint();
    assert_eq!(old.revision().seq(), 1);
    assert!(old.text().same_tree(b.history.text()) && *old.text() == "a");
    b.typed(1, "b", t0, TYPING);
    assert_eq!(b.count(), 3);
    b.history
        .mark_saved(&old)
        .expect("a revision of this history");
    assert_eq!(b.history.saved().map(RevisionId::seq), Some(1));
    assert!(b.history.is_modified());
    b.undo();
    assert!(!b.history.is_modified());
    // The saved revision is never joined, even when marked while current
    // and open: a checkpoint of a clone seals only the clone.
    b.typed(1, "c", t0, TYPING);
    assert_eq!((b.count(), b.seq()), (4, 3));
    let token = b.history.clone().checkpoint();
    assert!(b.history.open && token.revision().seq() == 3);
    b.history
        .mark_saved(&token)
        .expect("a revision of this history");
    assert!(!b.history.is_modified());
    b.history.__check_invariants().expect("invariants");
    // Typing on from where "c" left the caret would join an open revision.
    b.typed(2, "d", t0, TYPING);
    assert_eq!((b.count(), b.seq()), (5, 4));
    assert!(b.history.is_modified());
    assert_eq!(*token.text(), "ac");
    // A saved revision on another branch.
    b.jump(2);
    assert!(b.history.is_modified());
    b.jump(3);
    assert!(!b.history.is_modified());
    // `clear_saved`: no revision is the one on disk.
    b.history.clear_saved();
    assert_eq!(b.history.saved(), None);
    assert!(b.history.is_modified());
    b.undo();
    assert!(b.history.is_modified());
    // A checkpoint of a revision this history does not have.
    let stranger = Checkpoint {
        revision: RevisionId(10),
        text: Rope::new(),
    };
    b.refused(
        TextError::RevisionNotFound {
            revision: 10,
            count: 5,
        },
        |history, _| history.mark_saved(&stranger),
    );
}

#[test]
fn navigation() {
    let t0 = base();
    // Redo takes the newest child; after jumping to an older one and
    // undoing, redo goes back to it.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, Group::New);
    b.undo();
    b.typed(0, "b", t0, Group::New);
    b.undo();
    let step = b.redo().expect("a child");
    assert_eq!(
        (step.revision.seq(), b.rope.to_string()),
        (2, "b".to_string())
    );
    b.jump(1);
    assert_eq!(b.rope, "a");
    // The jump down made 1 the root's last child, so redo's path from the
    // root reaches the current revision.
    b.history.__check_invariants().expect("invariants");
    b.undo();
    assert_eq!(b.redo().map(|step| step.revision.seq()), Some(1));
    assert_eq!(b.rope, "a");

    // Undo shows the selection before, redo the selection after.
    let mut b = Buffer::new("abc");
    b.edit(
        1..2,
        "XY",
        (Selection::single(Cursor::new(2, 1)), caret(3)),
        t0,
        Group::New,
    )
    .expect("valid");
    let undo = b.undo().expect("a revision");
    assert_eq!(undo.selection, Selection::single(Cursor::new(2, 1)));
    let redo = b.redo().expect("a child");
    assert_eq!(redo.selection, caret(3));

    // The tree 0 → 1 → 2 and 1 → 3.
    let mut b = Buffer::new("");
    b.typed(0, "a", t0, Group::New);
    b.typed(1, "b", t0, Group::New);
    b.undo();
    b.typed(1, "c", t0, Group::New);
    assert_eq!((b.seq(), b.rope.to_string()), (3, "ac".to_string()));
    // A jump up leaves everything as undos would, and one down along the
    // last children as redos would.
    let (mut by_moves, mut by_jump) = (b.clone(), b.clone());
    by_moves.undo();
    let undone = by_moves.undo().expect("a revision");
    let step = by_jump.jump(0).expect("a jump");
    assert!(by_moves.history == by_jump.history && by_jump.rope.is_empty());
    assert_eq!((step.changes.len(), step.selection), (2, undone.selection));
    by_moves.redo();
    let redone = by_moves.redo().expect("a child");
    let step = by_jump.jump(3).expect("a jump");
    assert!(by_moves.history == by_jump.history && by_jump.rope == "ac");
    assert_eq!((step.changes.len(), step.selection), (2, redone.selection));
    // Across branches: 3 up to 1, then down to 2. Redo then follows 2.
    let step = b.jump(2).expect("a jump");
    assert_eq!(
        (step.changes.len(), b.rope.to_string()),
        (2, "ab".to_string())
    );
    assert_eq!(b.history.revisions[1].last_child, Some(2));
    b.undo();
    assert_eq!(b.redo().map(|step| step.revision.seq()), Some(2));
    // An unknown revision.
    b.refused(
        TextError::RevisionNotFound {
            revision: 4,
            count: 4,
        },
        |history, rope| history.jump_to(rope, RevisionId(4)),
    );
    b.refused(
        TextError::RevisionNotFound {
            revision: u64::MAX,
            count: 4,
        },
        |history, rope| history.jump_to(rope, RevisionId(u64::MAX)),
    );

    // 50 scattered commits of one command undo to the exact original and
    // redo to the exact final text.
    let original: String = (0..60).map(|line| format!("line {line}\n")).collect();
    let mut b = Buffer::new(&original);
    let mut lines: Vec<usize> = (0..60).collect();
    Pcg32::new(7, 0).shuffle(&mut lines);
    let group = Group::Command(GroupId::fresh());
    let mut layouts = Vec::new();
    for &line in &lines[..50] {
        let at = b.rope.line_to_byte(line).expect("a line of the text");
        b.typed(at, "x", t0, group);
        let first = std::iter::once(&b.history.revisions[1].changes);
        let pieces = first.chain(b.history.pending.iter().map(|piece| &piece.changes));
        layouts.push(
            pieces
                .map(|changes| changes.changes().len())
                .collect::<Vec<_>>(),
        );
    }
    // The binary counter: weights are replacements plus one, and the top
    // two pieces merge while the lower weighs at most twice the upper.
    let want: [&[usize]; 6] = [&[1], &[2], &[3], &[4], &[4, 1], &[6]];
    assert_eq!(layouts[..6], want);
    let edited = b.rope.to_string();
    assert_eq!((b.count(), edited.matches("xline").count()), (2, 50));
    b.undo();
    assert_eq!(b.rope, original);
    b.redo();
    assert_eq!(b.rope, edited);
    b.history.__check_invariants().expect("invariants");
}

#[test]
fn errors_change_nothing() {
    let t0 = base();
    // "xa😀b": the emoji is bytes 2..6.
    let mut b = Buffer::new("a😀b");
    b.typed(0, "x", t0, TYPING);
    let insert = |text: &Rope, at: usize| ChangeSet::new(text, [(at..at, "!")]).expect("valid");
    let commit = |changes: ChangeSet, before: Selection, after: Selection| {
        move |history: &mut History, rope: &mut Rope| {
            history.commit(rope, Transaction::new(changes, after), &before, t0, TYPING)
        }
    };
    let mismatch = || TextError::HistoryMismatch {
        len: 7,
        expected: 7,
    };

    // A rope with the same text that is not the history's, for every call
    // that edits; then one edited outside the history, and the recovery.
    let valid = insert(&b.rope, 1);
    let mut equal = Rope::from("xa😀b");
    b.refused(mismatch(), |history, _| {
        commit(valid.clone(), caret(1), caret(2))(history, &mut equal)
    });
    b.refused(mismatch(), |history, _| history.undo(&mut equal));
    b.refused(mismatch(), |history, _| history.redo(&mut equal));
    b.refused(mismatch(), |history, _| {
        history.jump_to(&mut equal, RevisionId(0))
    });
    let mut edited = b.rope.clone();
    edited.insert(0, "!").expect("valid");
    let outside = TextError::HistoryMismatch {
        len: 8,
        expected: 7,
    };
    b.refused(outside, |history, _| history.undo(&mut edited));
    let mut restored = b.history.text().clone();
    b.history
        .undo(&mut restored)
        .expect("the history's own text");
    b.rope = restored;
    b.redo();

    // Each check, and the order between them.
    let ascii = Rope::from("xxxxxxx");
    let short = Rope::from("abc");
    let cases = [
        (
            commit(insert(&short, 0), caret(1), caret(2)),
            TextError::LengthMismatch {
                expected: 3,
                actual: 7,
            },
        ),
        (
            commit(insert(&short, 0), caret(9), caret(2)),
            TextError::LengthMismatch {
                expected: 3,
                actual: 7,
            },
        ),
        (
            commit(ChangeSet::identity(7), caret(1), caret(8)),
            TextError::ByteOutOfBounds { byte: 8, len: 7 },
        ),
        (
            commit(valid.clone(), caret(9), caret(2)),
            TextError::ByteOutOfBounds { byte: 9, len: 7 },
        ),
        (
            commit(insert(&ascii, 3), caret(4), caret(2)),
            TextError::NotCharBoundary { byte: 4 },
        ),
        (
            commit(insert(&ascii, 3), caret(1), caret(99)),
            TextError::NotCharBoundary { byte: 3 },
        ),
        (
            commit(
                ChangeSet::new(&ascii, [(1..2, ""), (5..7, "")]).expect("valid"),
                caret(1),
                caret(1),
            ),
            TextError::NotCharBoundary { byte: 5 },
        ),
        (
            commit(
                ChangeSet::new(&b.rope, [(0..0, "é")]).expect("valid"),
                caret(0),
                caret(1),
            ),
            TextError::NotCharBoundary { byte: 1 },
        ),
        (
            commit(valid.clone(), caret(1), caret(9)),
            TextError::ByteOutOfBounds { byte: 9, len: 8 },
        ),
    ];
    for (call, want) in cases {
        b.refused(want, call);
    }
    // Still usable.
    b.history.__check_invariants().expect("invariants");
    commit(valid, caret(1), caret(2))(&mut b.history, &mut b.rope).expect("valid");
    assert_eq!(b.rope, "x!a😀b");
}

#[test]
fn no_op_moves_change_nothing() {
    let t0 = base();
    let mut b = Buffer::new("abc");
    // Undo at the root, redo with nothing to redo, a jump to the current
    // revision.
    let still = |b: &mut Buffer| {
        let (history, rope) = (b.history.clone(), b.rope.clone());
        assert!(b.redo().is_none());
        let current = b.seq();
        assert!(b.jump(current).is_none());
        assert!(b.history == history && b.rope.same_tree(&rope));
    };
    let (history, rope) = (b.history.clone(), b.rope.clone());
    assert!(b.undo().is_none());
    assert!(b.history == history && b.rope.same_tree(&rope));
    still(&mut b);
    // They keep the revision open: type, a redo with nothing to redo, a
    // jump to where we are, and typing on is still one revision.
    b.typed(3, "d", t0, TYPING);
    still(&mut b);
    assert!(b.history.open);
    b.typed(4, "e", t0, TYPING);
    assert_eq!((b.count(), b.rope.to_string()), (2, "abcde".to_string()));
}

/// A branched history, 0 → 1 → 2 and 1 → 3, whose current revision 3 is
/// open with a pending piece (a command group of five scattered inserts,
/// the cursors left at 0); and the texts of revisions 1 and 2.
fn branched() -> (History, Rope, Rope) {
    let t0 = base();
    let mut b = Buffer::new("SECRET0123456789abcdef");
    b.typed(0, "a", t0, Group::New);
    let first = b.rope.clone();
    b.typed(1, "b", t0, Group::New);
    let second = b.rope.clone();
    b.undo();
    let group = Group::Command(GroupId::fresh());
    for at in [2, 6, 10, 14, 18] {
        let changes = ChangeSet::new(&b.rope, [(at..at, "x")]).expect("valid");
        let transaction = Transaction::new(changes, caret(0));
        b.history
            .commit(&mut b.rope, transaction, &caret(0), t0, group)
            .expect("valid");
    }
    let h = &b.history;
    assert_eq!(
        (h.revision_count(), h.current, h.open, h.pending.len()),
        (4, 3, true, 1)
    );
    assert_eq!((first.len(), second.len(), h.text.len()), (23, 24, 28));
    h.__check_invariants().expect("a valid history");
    (b.history, first, second)
}

/// `good` changed by `corrupt`, which the checker must refuse with a
/// message containing `want` and no text.
#[track_caller]
fn refused_history(good: &History, corrupt: impl FnOnce(&mut History), want: &str) {
    let mut history = good.clone();
    corrupt(&mut history);
    match history.__check_invariants() {
        Ok(()) => panic!("the checker missed: {want}"),
        Err(message) => assert!(
            message.contains(want) && !message.contains("SECRET"),
            "want {want:?}, got {message:?}"
        ),
    }
}

/// The checker refuses each broken invariant, one at a time, H1 to H9.
#[test]
fn the_checker_finds_every_kind_of_broken_history() {
    let t0 = base();
    let (good, first, second) = branched();
    let root = "the root needs parent 0";
    let no_time = "revision 2 has the root's tag or no time";
    let not_later = "is not a later child of it";
    let open = "is open but not the newest, or is the root or the saved one";

    // H1: the root.
    refused_history(&good, |h| h.revisions[0].parent = 1, root);
    refused_history(&good, |h| h.revisions[0].tag = Tag::New, root);
    refused_history(&good, |h| h.revisions[0].time = Some(t0), root);
    refused_history(
        &good,
        |h| h.revisions[0].changes = h.revisions[1].changes.clone(),
        root,
    );
    refused_history(&History::new(&first), |h| h.open = true, open);

    // H2: parents, lengths, tags, inverses and canonical change sets.
    refused_history(
        &good,
        |h| h.revisions[2].parent = 2,
        "revision 2's parent 2 is not below it",
    );
    refused_history(&good, |h| h.revisions[2].time = None, no_time);
    refused_history(&good, |h| h.revisions[2].tag = Tag::Root, no_time);
    refused_history(
        &good,
        |h| {
            let r = &mut h.revisions[2];
            std::mem::swap(&mut r.changes, &mut r.inverse);
        },
        "revision 2's change is for 24 bytes, but its parent 1 has 23",
    );
    refused_history(
        &good,
        |h| h.revisions[3].tag = Tag::New,
        "revision 3 was made by Group::New but changes nothing or has pieces",
    );
    refused_history(
        &good,
        |h| {
            h.revisions[2].changes = ChangeSet::identity(23);
            h.revisions[2].inverse = ChangeSet::identity(23);
        },
        "revision 2 was made by Group::New but changes nothing",
    );
    refused_history(
        &good,
        |h| h.revisions[2].inverse = ChangeSet::identity(24),
        "revision 2's inverse does not swap its change's lengths",
    );
    let broken_text = ChangeSet::new(&first, [(1..1, broken_rope("b"))]).expect("valid");
    refused_history(
        &good,
        |h| h.revisions[2].changes = broken_text,
        "revision 2's change: replacement 0's text",
    );

    // H3: last children.
    refused_history(
        &good,
        |h| h.revisions[1].last_child = None,
        "revision 1 has a child, 2, but no last child",
    );
    refused_history(&good, |h| h.revisions[1].last_child = Some(0), not_later);
    refused_history(&good, |h| h.revisions[0].last_child = Some(2), not_later);

    // H4: the current and saved revisions.
    refused_history(
        &good,
        |h| h.saved = Some(9),
        "current 3 or saved Some(9) is not below the 4 revisions",
    );
    let mut plain = Buffer::new("SECRET");
    plain.typed(0, "a", t0, Group::New);
    plain.typed(1, "b", t0, Group::New);
    refused_history(&plain.history, |h| h.current = 9, "current 9 or saved");

    // H5: only the newest revision, not the root and not the saved one, is
    // open.
    refused_history(&good, |h| h.saved = Some(3), open);
    plain.undo();
    plain.history.__check_invariants().expect("a valid history");
    refused_history(&plain.history, |h| h.open = true, open);

    // H6: pending pieces only on the newest revision, chained, and lighter
    // than half the piece below.
    let fourth = ChangeSet::new(&second, [(0..0, "z")]).expect("valid");
    let fourth_inverse = fourth.invert(&second).expect("valid");
    refused_history(
        &good,
        |h| {
            h.open = false;
            h.revisions[2].last_child = Some(4);
            h.revisions.push(Revision {
                parent: 2,
                last_child: None,
                changes: fourth,
                inverse: fourth_inverse,
                selection_before: caret(0),
                selection_after: caret(0),
                time: Some(t0),
                tag: Tag::New,
            });
        },
        "revision 3 has pieces but is not the newest, or is the root",
    );
    refused_history(
        &good,
        |h| {
            let piece = &mut h.pending[0];
            std::mem::swap(&mut piece.changes, &mut piece.inverse);
        },
        "pending piece 0's lengths do not chain",
    );
    refused_history(
        &good,
        |h| {
            h.pending.push(Piece {
                changes: ChangeSet::identity(28),
                inverse: ChangeSet::identity(28),
            });
        },
        "pending piece 1 weighs 1, and the piece below it only 2",
    );

    // H7: the text's length.
    refused_history(
        &good,
        |h| h.text = Rope::from("SECRET"),
        "the text has 6 bytes, but the current revision's has 28",
    );

    // H8: stored selections inside their texts.
    refused_history(
        &good,
        |h| h.revisions[2].selection_after = caret(99),
        "revision 2's selection after reaches 99, past its 24-byte text",
    );
    refused_history(
        &good,
        |h| h.revisions[2].selection_before = caret(99),
        "revision 2's selection before reaches 99, past its 23-byte text",
    );

    // H9: redo from the root reaches the current revision.
    refused_history(
        &good,
        |h| h.revisions[1].last_child = Some(2),
        "revision 3 is on the path to the current revision 3, but its parent 1's last child \
         is Some(2)",
    );
}

#[test]
fn debug_shows_counts_and_never_text() {
    let t0 = base();
    let mut b = Buffer::new("a secret");
    let step = b.typed(0, "SECRET ", t0, TYPING);
    let checkpoint = b.history.checkpoint();
    let shown = [
        format!("{:?}", b.history),
        format!("{step:?}"),
        format!("{checkpoint:?}"),
        format!("{TYPING:?}"),
        format!("{:?}", GroupId::fresh()),
        format!("{:?}", b.history.current()),
    ];
    for text in shown {
        assert!(
            !text.contains("secret") && !text.contains("SECRET"),
            "{text}"
        );
    }
    fn send_sync<T: Send + Sync>() {}
    send_sync::<History>();
    send_sync::<Step>();
    send_sync::<Checkpoint>();
    send_sync::<Group>();
    send_sync::<GroupId>();
    send_sync::<RevisionId>();
}

// ---------------------------------------------------------------------------
// Costs.

#[test]
fn insert_session_of_10000_keystrokes_is_one_step() {
    let original = "fn main() {\n    \n}\n";
    let mut b = Buffer::new(original);
    let mut want = original.to_string();
    let group = Group::Command(GroupId::fresh());
    let t0 = base();
    let keys = ["a", "é", " ", "\n", "😀", "b", "\r\n", "中"];
    let mut at = 16;
    for (index, key) in keys.iter().cycle().take(10_000).enumerate() {
        let start = appends::read();
        b.typed(at, key, t0, group);
        let made = appends::read() - start;
        want.insert_str(at, key);
        at += key.len();
        let revision = &b.history.revisions[1];
        assert!(
            b.count() == 2
                && b.history.pending.is_empty()
                && revision.changes.changes().len() == 1
                && revision.inverse.changes().len() == 1
                && made <= 1,
            "keystroke {index}: {made} appends"
        );
    }
    assert_eq!(b.rope, want);
    b.history.__check_invariants().expect("invariants");
    let undo = b.undo().expect("the session");
    assert_eq!(undo.changes.len(), 1);
    assert_eq!(b.rope, original);
    assert!(!b.history.can_undo());
    b.redo();
    assert_eq!(b.rope, want);
}

#[test]
fn command_group_of_5000_scattered_edits_is_n_log_n() {
    const EDITS: usize = 5_000;
    let original: String = (0..EDITS).map(|line| format!("{line}\n")).collect();
    let mut b = Buffer::new(&original);
    let mut lines: Vec<usize> = (0..EDITS).collect();
    Pcg32::new(0x5EED, 0).shuffle(&mut lines);
    let group = Group::Command(GroupId::fresh());
    let t0 = base();
    let start = counters::read();
    let mut most_pieces = 0;
    for &line in &lines {
        let at = b.rope.line_to_byte(line).expect("a line of the text");
        b.typed(at, "x", t0, group);
        most_pieces = most_pieces.max(b.history.pending.len() + 1);
    }
    let committing = counters::read() - start;
    assert_eq!(b.count(), 2);
    b.undo();
    let total = counters::read() - start;
    // 8·K·⌈log₂K⌉; re-composing the whole group on every commit would read
    // about K² = 25,000,000.
    let log = u64::from(EDITS.next_power_of_two().trailing_zeros());
    let bound = 8 * EDITS as u64 * log;
    assert!(
        total <= bound,
        "composing read {committing} replacements in the commits and {total} with the \
         undo's fold, over the bound {bound}"
    );
    assert!(most_pieces as u64 <= log + 1, "{most_pieces} pieces");
    assert_eq!(b.rope, original);
    b.redo();
    let edited: String = (0..EDITS).map(|line| format!("x{line}\n")).collect();
    assert_eq!(b.rope, edited);
}
