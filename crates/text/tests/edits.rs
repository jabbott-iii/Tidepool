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

//! Selections, change sets, transactions and the undo tree through the
//! public API only, against models written from ADR-0020 and its design
//! rather than from the implementation: the merge rule, the mapping table,
//! building and its error order, the change-set laws (L1-L13),
//! `change_each`, and the history at real sizes (texts up to 64 KiB,
//! inserts up to 12 KiB, up to 100 cursors). Also 10,000 cursors, a 2 MiB
//! select-all delete, runs of up to 100,000 touching edits, hundreds of
//! small edits rebuilding texts of up to 400 KB, and texts too long to
//! exist.
//!
//! The unit tests (`src/*/tests.rs`) run more cases with crate-private
//! checks. Here "unchanged" means equal content, equal accessors, and a
//! next history call that is not `HistoryMismatch`. Errors compare by their
//! `Debug` form (numbers only), since `TextError` has no `PartialEq`. Each
//! property case's input is a seed, so a failure report stays short.

use std::cell::Cell;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};
use tidepool_text::{
    Assoc, Change, ChangeSet, Changes, Checkpoint, Cursor, Group, GroupId, History, Place,
    RevisionId, Rope, Selection, Step, TextError, Transaction,
};

fn send_sync<T: Send + Sync>() {}

/// Every type M1.2 adds can be sent to and shared with workers.
fn every_new_type_is_send_sync() {
    send_sync::<Cursor>();
    send_sync::<Selection>();
    send_sync::<Assoc>();
    send_sync::<ChangeSet>();
    send_sync::<Change<'static>>();
    send_sync::<Changes<'static>>();
    send_sync::<Transaction>();
    send_sync::<Place>();
    send_sync::<History>();
    send_sync::<RevisionId>();
    send_sync::<GroupId>();
    send_sync::<Group>();
    send_sync::<Step>();
    send_sync::<Checkpoint>();
}

// Checked when the tests compile, and run by `types_are_send_sync_and_hide_text`.
const _: fn() = every_new_type_is_send_sync;

/// The largest text the real-size history cases grow to.
const MAX_TEXT: usize = 64 * 1024;
/// The coalescing window of every `Coalesce` group here.
const WINDOW: Duration = Duration::from_secs(1);
/// The most bytes a text can have.
const MAX: usize = isize::MAX as usize;

// ------------------------------------------------------------- randomness

/// A seed for a case's own generator.
fn seed(rng: &mut Pcg32) -> u64 {
    (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())
}

/// True `percent` times in 100.
fn chance(rng: &mut Pcg32, percent: u32) -> bool {
    rng.range(0..100u32) < percent
}

/// A uniform number in `0..n`, or 0 if `n` is 0.
fn below(rng: &mut Pcg32, n: usize) -> usize {
    rng.range(0..u32::try_from(n).expect("test sizes fit in a u32")) as usize
}

/// `start..end` with `start > end`, for tests of `InvertedRange`.
fn inverted(start: usize, end: usize) -> Range<usize> {
    Range { start, end }
}

fn floor(text: &str, at: usize) -> usize {
    text.floor_char_boundary(at)
}

/// `Ok` or the error's `Debug` form.
fn outcome<T>(result: &Result<T, TextError>) -> Result<(), String> {
    result.as_ref().map(|_| ()).map_err(|e| format!("{e:?}"))
}

/// Compares two results by `outcome`.
fn same<A, B>(
    what: &str,
    got: &Result<A, TextError>,
    want: &Result<B, TextError>,
) -> Result<(), String> {
    let (got, want) = (outcome(got), outcome(want));
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

/// Asserts that `result` is the error `want`.
#[track_caller]
fn fails<T>(result: Result<T, TextError>, want: TextError) {
    assert_eq!(outcome(&result), Err(format!("{want:?}")));
}

// ------------------------------------------------------------------ texts

/// Units of generated text: ASCII, every line break (CR LF twice, so pairs
/// are common), and chars of 2, 3 and 4 bytes, combining marks and ZWJ.
const UNITS: [&str; 22] = [
    "a",
    "b",
    "z",
    " ",
    "\t",
    "\n",
    "\r",
    "\r\n",
    "\r\n",
    "é",
    "€",
    "中",
    "😀",
    "🇺🇸",
    "\u{301}",
    "\u{200D}",
    "क्",
    "\u{85}",
    "\u{2028}",
    "\0",
    "\u{10FFFF}",
    "ab",
];

/// `count` units.
fn units(rng: &mut Pcg32, count: usize) -> String {
    (0..count)
        .map(|_| rng.choose(&UNITS).copied().unwrap_or("a"))
        .collect()
}

/// A small text: 0-24 units, empty one time in 10.
fn small_text(rng: &mut Pcg32) -> String {
    if chance(rng, 10) {
        return String::new();
    }
    let count = rng.range(1..25u32) as usize;
    units(rng, count)
}

/// An edit's new text: empty 30% of the time, otherwise 1-6 units.
fn new_text(rng: &mut Pcg32) -> String {
    if chance(rng, 30) {
        return String::new();
    }
    let count = rng.range(1..7u32) as usize;
    units(rng, count)
}

/// About `bytes` bytes of mostly ASCII words and line breaks.
fn big_text(rng: &mut Pcg32, bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + 8);
    while out.len() < bytes {
        match rng.range(0..10u32) {
            0..3 => out.push_str(rng.choose(&UNITS).copied().unwrap_or("a")),
            3 => out.push('\n'),
            _ => out.push(char::from(rng.range(0x20..0x7Fu32) as u8)),
        }
    }
    out
}

/// An insert length in bytes: mostly small, up to 12 KiB.
fn insert_len(rng: &mut Pcg32) -> usize {
    let n = match rng.range(0..100u32) {
        0..60 => rng.range(1..17u32),
        60..85 => rng.range(17..513u32),
        85..95 => rng.range(513..4097u32),
        _ => rng.range(4097..12_289u32),
    };
    n as usize
}

/// A position: 10% at 0, 10% at the end, 20% at or up to 2 bytes around a
/// CR or LF (the CR|LF offset included), 60% at a random boundary. First,
/// `raw` times in 100, a raw number up to 2 past the end, which may be out
/// of bounds or inside a char.
fn position(rng: &mut Pcg32, text: &str, raw: u32) -> usize {
    if chance(rng, raw) {
        return below(rng, text.len() + 3);
    }
    match rng.range(0..100u32) {
        0..10 => 0,
        10..20 => text.len(),
        20..40 => {
            let from = floor(text, below(rng, text.len() + 1));
            let found = text[from..]
                .find(['\r', '\n'])
                .map(|i| from + i)
                .or_else(|| text[..from].rfind(['\r', '\n']));
            match found {
                Some(at) => {
                    let delta = rng.range(-2..3i32) as isize;
                    floor(text, at.saturating_add_signed(delta))
                }
                None => from,
            }
        }
        _ => floor(text, below(rng, text.len() + 1)),
    }
}

// ------------------------------------------------------------- selections

/// A cursor as `(anchor, head)`.
type Pair = (usize, usize);

fn lo(c: Pair) -> usize {
    c.0.min(c.1)
}

fn hi(c: Pair) -> usize {
    c.0.max(c.1)
}

fn cursor(c: Pair) -> Cursor {
    Cursor::new(c.0, c.1)
}

fn pairs(selection: &Selection) -> Vec<Pair> {
    selection
        .cursors()
        .iter()
        .map(|c| (c.anchor, c.head))
        .collect()
}

/// `cursors[primary]` as the primary and the rest as the others, through
/// `Selection::new`.
fn selection_of(cursors: &[Pair], primary: usize) -> Selection {
    let others = cursors
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != primary)
        .map(|(_, &c)| cursor(c));
    Selection::new(cursor(cursors[primary]), others)
}

/// The merge rule as ADR-0020 states it, in O(r²): non-empty cursors that
/// share a byte merge transitively; a caret joins the group whose closed
/// span holds it, the one with the lower start where two touch at it;
/// carets that join no group and are equal become one. A group takes the
/// primary's direction if the primary is a non-empty member, else that of
/// its first non-empty member in the order (start, end, backward). Returns
/// the cursors in order and the index of the one holding the primary.
fn normalize(cursors: &[Pair], primary: usize) -> (Vec<Pair>, usize) {
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    let n = cursors.len();
    let filled = |c: Pair| lo(c) < hi(c);
    let mut root: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (cursors[i], cursors[j]);
            if filled(a) && filled(b) && lo(a) < hi(b) && lo(b) < hi(a) {
                let (ra, rb) = (find(&mut root, i), find(&mut root, j));
                root[ra] = rb;
            }
        }
    }
    // Groups as (start, end, members); first the non-empty ones.
    let mut groups: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    let mut group_of_root: Vec<Option<usize>> = vec![None; n];
    for (i, &c) in cursors.iter().enumerate() {
        if !filled(c) {
            continue;
        }
        let r = find(&mut root, i);
        let g = match group_of_root[r] {
            Some(g) => g,
            None => {
                groups.push((lo(c), hi(c), Vec::new()));
                group_of_root[r] = Some(groups.len() - 1);
                groups.len() - 1
            }
        };
        let group = &mut groups[g];
        group.0 = group.0.min(lo(c));
        group.1 = group.1.max(hi(c));
        group.2.push(i);
    }
    let ranges = groups.len();
    for (i, &c) in cursors.iter().enumerate() {
        if filled(c) {
            continue;
        }
        let at = c.0;
        let host = (0..ranges)
            .filter(|&g| groups[g].0 <= at && at <= groups[g].1)
            .min_by_key(|&g| groups[g].0);
        let g = match host.or_else(|| (ranges..groups.len()).find(|&g| groups[g].0 == at)) {
            Some(g) => g,
            None => {
                groups.push((at, at, Vec::new()));
                groups.len() - 1
            }
        };
        groups[g].2.push(i);
    }
    let mut out: Vec<(Pair, bool)> = groups
        .iter()
        .map(|(start, end, members)| {
            let holds_primary = members.contains(&primary);
            let from = if holds_primary && filled(cursors[primary]) {
                Some(primary)
            } else {
                members
                    .iter()
                    .copied()
                    .filter(|&i| filled(cursors[i]))
                    .min_by_key(|&i| {
                        let c = cursors[i];
                        (lo(c), hi(c), c.1 < c.0)
                    })
            };
            let backward = from.is_some_and(|i| cursors[i].1 < cursors[i].0);
            let pair = if backward {
                (*end, *start)
            } else {
                (*start, *end)
            };
            (pair, holds_primary)
        })
        .collect();
    out.sort_by_key(|&(c, _)| lo(c));
    let primary = out.iter().position(|&(_, p)| p).unwrap_or(usize::MAX);
    (out.into_iter().map(|(c, _)| c).collect(), primary)
}

/// `Selection::validate` as stated: each cursor's start, then its end, in
/// cursor order; the first bad position is out of bounds, else inside a
/// char.
fn validate(cursors: &[Pair], text: &str) -> Result<(), TextError> {
    for &c in cursors {
        byte_error(text, lo(c))?;
        byte_error(text, hi(c))?;
    }
    Ok(())
}

fn same_selection(what: &str, got: &Selection, want: &(Vec<Pair>, usize)) -> Result<(), String> {
    got.__check_invariants()
        .map_err(|e| format!("{what}: invariants: {e}"))?;
    let got_pairs = pairs(got);
    if got_pairs != want.0 || got.primary_index() != want.1 {
        return Err(format!(
            "{what}: got {got_pairs:?} primary {}, want {:?} primary {}",
            got.primary_index(),
            want.0,
            want.1
        ));
    }
    Ok(())
}

/// One position for a cursor: from `text`, or from `pool` when given.
fn pick(rng: &mut Pcg32, text: &str, raw: u32, pool: &[usize]) -> usize {
    if pool.is_empty() {
        position(rng, text, raw)
    } else {
        pool[below(rng, pool.len())]
    }
}

/// Cursors on `text`: 1-8 (9-`max` one time in 10 when `max` > 8; 64 on
/// three colliding positions one time in 20), 40% carets, either
/// direction, any primary.
fn random_pairs(rng: &mut Pcg32, text: &str, max: usize, raw: u32) -> (Vec<Pair>, usize) {
    let roll = rng.range(0..100u32);
    let mut pool = Vec::new();
    let count = if roll < 5 {
        pool = (0..3).map(|_| position(rng, text, raw)).collect();
        64
    } else if roll < 15 && max > 8 {
        rng.range(9..max as u32 + 1) as usize
    } else {
        rng.range(1..9u32) as usize
    };
    let cursors = (0..count)
        .map(|_| {
            let anchor = pick(rng, text, raw, &pool);
            if chance(rng, 40) {
                (anchor, anchor)
            } else {
                (anchor, pick(rng, text, raw, &pool))
            }
        })
        .collect();
    (cursors, below(rng, count))
}

// ------------------------------------------------------------ change sets

/// An edit: a byte range of the text and its new text.
type Edit = (Range<usize>, String);

/// A replacement of the canonical form.
#[derive(Clone, Debug, PartialEq)]
struct Rep {
    old: Range<usize>,
    new: Range<usize>,
    text: String,
}

/// As `Rope::insert` checks a position.
fn byte_error(text: &str, byte: usize) -> Result<(), TextError> {
    if byte > text.len() {
        Err(TextError::ByteOutOfBounds {
            byte,
            len: text.len(),
        })
    } else if !text.is_char_boundary(byte) {
        Err(TextError::NotCharBoundary { byte })
    } else {
        Ok(())
    }
}

/// As `Rope::replace` checks a range.
fn range_error(text: &str, range: &Range<usize>) -> Result<(), TextError> {
    if range.start > range.end {
        return Err(TextError::InvertedRange {
            start: range.start,
            end: range.end,
        });
    }
    if range.end > text.len() {
        return Err(TextError::ByteOutOfBounds {
            byte: range.end,
            len: text.len(),
        });
    }
    byte_error(text, range.start).and_then(|()| byte_error(text, range.end))
}

/// `TooLong` if `kept` would make a text of `len` bytes longer than
/// `isize::MAX`, with no arithmetic that can overflow.
fn too_long(len: usize, kept: &[(Range<usize>, usize)]) -> Result<(), TextError> {
    let removed: usize = kept.iter().map(|(r, _)| r.end - r.start).sum();
    let after = kept
        .iter()
        .try_fold(len - removed, |sum, &(_, added)| sum.checked_add(added));
    match after {
        Some(after) if after <= MAX => Ok(()),
        _ => Err(TextError::TooLong { max: MAX }),
    }
}

/// `ChangeSet::new` as ADR-0020 states it, on a `String`: check every
/// range in input order; drop edits that do nothing; stable-sort by (start,
/// end); the first neighbours where the next starts before the previous
/// ends overlap; then the length; then touching edits merge.
fn build(text: &str, edits: &[Edit]) -> Result<Vec<Rep>, TextError> {
    for (range, _) in edits {
        range_error(text, range)?;
    }
    let mut kept: Vec<Edit> = edits
        .iter()
        .filter(|(r, t)| r.start != r.end || !t.is_empty())
        .cloned()
        .collect();
    kept.sort_by_key(|(r, _)| (r.start, r.end));
    for pair in kept.windows(2) {
        if pair[1].0.start < pair[0].0.end {
            return Err(TextError::OverlappingEdits {
                end: pair[0].0.end,
                start: pair[1].0.start,
            });
        }
    }
    let sizes: Vec<(Range<usize>, usize)> =
        kept.iter().map(|(r, t)| (r.clone(), t.len())).collect();
    too_long(text.len(), &sizes)?;
    Ok(canonical(&kept))
}

/// Sorted edits that do not overlap, touching ones merged, with their new
/// ranges.
fn canonical(sorted: &[Edit]) -> Vec<Rep> {
    let mut merged: Vec<Edit> = Vec::new();
    for (range, text) in sorted {
        match merged.last_mut() {
            Some((last, joined)) if last.end == range.start => {
                last.end = range.end;
                joined.push_str(text);
            }
            _ => merged.push((range.clone(), text.clone())),
        }
    }
    let mut shift = 0isize;
    merged
        .into_iter()
        .map(|(old, text)| {
            let start = old
                .start
                .checked_add_signed(shift)
                .expect("new starts are offsets");
            shift += text.len() as isize - (old.end - old.start) as isize;
            Rep {
                new: start..start + text.len(),
                old,
                text,
            }
        })
        .collect()
}

/// `text` with the sorted, non-overlapping `edits` made, in one pass.
fn apply_edits(text: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for (range, new) in edits {
        out.push_str(&text[from..range.start]);
        out.push_str(new);
        from = range.end;
    }
    out.push_str(&text[from..]);
    out
}

fn apply_reps(text: &str, reps: &[Rep]) -> String {
    let edits: Vec<Edit> = reps
        .iter()
        .map(|r| (r.old.clone(), r.text.clone()))
        .collect();
    apply_edits(text, &edits)
}

/// The mapping table of ADR-0020's design, over the canonical form.
fn map_model(
    reps: &[Rep],
    lens: (usize, usize),
    p: usize,
    assoc: Assoc,
) -> Result<usize, TextError> {
    if p > lens.0 {
        return Err(TextError::ByteOutOfBounds {
            byte: p,
            len: lens.0,
        });
    }
    let Some(r) = reps.iter().find(|r| r.old.end >= p) else {
        return Ok(lens.1 - (lens.0 - p));
    };
    let (s, e, ns, l) = (r.old.start, r.old.end, r.new.start, r.text.len());
    let at = |assoc| if assoc == Assoc::Before { ns } else { ns + l };
    Ok(if p < s {
        ns - (s - p)
    } else if s == e || (s < p && p < e) {
        at(assoc)
    } else if p == s {
        ns
    } else {
        ns + l
    })
}

/// `changes()` against the model: lengths, ranges and texts.
fn same_changes(
    what: &str,
    changes: &ChangeSet,
    reps: &[Rep],
    lens: (usize, usize),
) -> Result<(), String> {
    let got_lens = (changes.len_before(), changes.len_after());
    if got_lens != lens || changes.is_identity() != reps.is_empty() {
        return Err(format!(
            "{what}: lengths {got_lens:?} (identity {}), want {lens:?} with {} replacements",
            changes.is_identity(),
            reps.len()
        ));
    }
    let list = changes.changes();
    if list.len() != reps.len() {
        return Err(format!(
            "{what}: {} replacements, want {}",
            list.len(),
            reps.len()
        ));
    }
    for (i, (got, want)) in list.zip(reps).enumerate() {
        let text_ok = match got.text {
            None => want.text.is_empty(),
            Some(text) => !want.text.is_empty() && *text == want.text,
        };
        if got.old != want.old || got.new != want.new || !text_ok {
            return Err(format!(
                "{what}: replacement {i} is {:?} -> {:?} ({} bytes), want {:?} -> {:?} ({} bytes)",
                got.old,
                got.new,
                got.text.map_or(0, Rope::len),
                want.old,
                want.new,
                want.text.len()
            ));
        }
    }
    Ok(())
}

/// What kind of edit lists to make.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    /// Anything, overlapping and corrupted ones included.
    Any,
    /// Sorted boundaries paired up, which never overlap; small texts.
    Valid,
    /// As `Valid`, with new texts of up to 12 KiB within `MAX_TEXT`.
    Real,
}

/// An edit list on `text`: 0-8 edits (none one time in 20, 64-100 small
/// ones one time in 20). Half (always, unless `Any`) are sorted boundaries
/// paired up, which makes touching edits, empty ranges and several inserts
/// at one point; the others are independent ranges, some overlapping.
/// Shuffled 30% of the time. With `Any`, one edit is corrupted one time in
/// 20 (inverted, past the end or inside a char), and one time in 33 an
/// empty edit goes inside another's range.
fn edit_list(rng: &mut Pcg32, text: &str, flavor: Flavor) -> Vec<Edit> {
    let count = match rng.range(0..100u32) {
        0..5 => 0,
        5..10 => rng.range(64..101u32) as usize,
        _ => rng.range(1..9u32) as usize,
    };
    let mut budget = MAX_TEXT.saturating_sub(text.len());
    let mut edits = Vec::with_capacity(count + 1);
    if flavor != Flavor::Any || count > 8 || chance(rng, 50) {
        let mut ends: Vec<usize> = Vec::with_capacity(2 * count);
        for _ in 0..2 * count {
            let at = match ends.last() {
                Some(&last) if chance(rng, 25) => last,
                _ => position(rng, text, 0),
            };
            ends.push(at);
        }
        ends.sort_unstable();
        for &[start, end] in ends.as_chunks::<2>().0 {
            let new = if count > 8 {
                let n = below(rng, 3);
                units(rng, n)
            } else if flavor == Flavor::Real && chance(rng, 15) {
                let n = insert_len(rng).min(budget);
                big_text(rng, n)
            } else {
                new_text(rng)
            };
            budget = budget.saturating_sub(new.len());
            edits.push((start..end, new));
        }
    } else {
        for _ in 0..count {
            let a = position(rng, text, 0);
            let b = if chance(rng, 50) {
                floor(text, a + below(rng, 8))
            } else {
                position(rng, text, 0)
            };
            edits.push((a.min(b)..a.max(b), new_text(rng)));
        }
    }
    if chance(rng, 30) {
        rng.shuffle(&mut edits);
    }
    if flavor != Flavor::Any {
        return edits;
    }
    if !edits.is_empty() && chance(rng, 5) {
        let i = below(rng, edits.len());
        let kind = rng.range(0..3u32);
        let mid_char = text
            .char_indices()
            .find(|(_, c)| c.len_utf8() > 1)
            .map(|(at, _)| at + 1);
        let range = &mut edits[i].0;
        match (kind, mid_char) {
            (0, _) if range.start < range.end => *range = range.end..range.start,
            (0, _) => *range = range.start + 1..range.start,
            (1, _) | (_, None) => range.end = text.len() + 1 + below(rng, 2),
            (_, Some(at)) => *range = at..at.max(range.end),
        }
    }
    if chance(rng, 3) {
        let inside = edits.iter().find_map(|(r, _)| {
            let at = floor(text, r.start + (r.end.saturating_sub(r.start)) / 2);
            (r.start < at && at < r.end).then_some(at)
        });
        if let Some(at) = inside {
            let i = below(rng, edits.len() + 1);
            edits.insert(i, (at..at, String::new()));
        }
    }
    edits
}

/// A change set on `text` that builds, checked against the model, with
/// its canonical replacements and the text after.
fn valid_change(rng: &mut Pcg32, text: &str) -> Result<(ChangeSet, Vec<Rep>, String), String> {
    let edits = edit_list(rng, text, Flavor::Valid);
    let reps = build(text, &edits).map_err(|e| format!("the model refused valid edits: {e}"))?;
    let changes = ChangeSet::new(&Rope::from(text), edits)
        .map_err(|e| format!("ChangeSet::new refused valid edits: {e}"))?;
    let after = apply_reps(text, &reps);
    same_changes(
        "a valid change set",
        &changes,
        &reps,
        (text.len(), after.len()),
    )?;
    Ok((changes, reps, after))
}

/// `rope` with `changes` applied, or why not.
fn applied(changes: &ChangeSet, rope: &Rope) -> Result<Rope, String> {
    let mut out = rope.clone();
    changes
        .apply(&mut out)
        .map_err(|e| format!("apply failed: {e}"))?;
    Ok(out)
}

// ---------------------------------------------------- selections, mapped

/// `Selection::map` as stated: the first position past `len_before` is an
/// error; a caret maps with `caret`; a range maps its start `After` and its
/// end `Before`, and becomes a caret at its start mapped with `caret` if
/// they meet or cross; then the merge rule.
fn map_selection(
    cursors: &[Pair],
    primary: usize,
    reps: &[Rep],
    lens: (usize, usize),
    caret: Assoc,
) -> Result<(Vec<Pair>, usize), TextError> {
    for &c in cursors {
        for p in [lo(c), hi(c)] {
            if p > lens.0 {
                return Err(TextError::ByteOutOfBounds {
                    byte: p,
                    len: lens.0,
                });
            }
        }
    }
    let mut mapped = Vec::with_capacity(cursors.len());
    for &c in cursors {
        let m = |p, assoc| map_model(reps, lens, p, assoc);
        mapped.push(if lo(c) == hi(c) {
            let at = m(c.0, caret)?;
            (at, at)
        } else {
            let (start, end) = (m(lo(c), Assoc::After)?, m(hi(c), Assoc::Before)?);
            if start >= end {
                let at = m(lo(c), caret)?;
                (at, at)
            } else if c.1 < c.0 {
                (end, start)
            } else {
                (start, end)
            }
        });
    }
    Ok(normalize(&mapped, primary))
}

// ------------------------------------------------------------ change_each

/// One cursor's edit for `change_each`.
#[derive(Clone, Debug)]
struct EachEdit {
    range: Range<usize>,
    text: String,
    place: Place,
}

/// What `change_each` makes: the text after, the kept edits (touching ones
/// still apart) and the selection.
struct Each {
    text: String,
    kept: Vec<Edit>,
    selection: (Vec<Pair>, usize),
}

/// `change_each` as stated: validate the selection; check every range in
/// cursor order, an edit that does nothing included; then such an edit
/// (an empty range and empty text) counts as `None`; stable-sort; sweep,
/// comparing each edit with the one kept just before it (an identical edit
/// collapses into it, overlapping deletions become their union, any other
/// overlap is an error); the length; place each cursor; merge.
fn change_each_model(
    text: &str,
    cursors: &[Pair],
    primary: usize,
    edits: &[Option<EachEdit>],
) -> Result<Each, TextError> {
    validate(cursors, text)?;
    for e in edits.iter().flatten() {
        range_error(text, &e.range)?;
    }
    let edits: Vec<Option<&EachEdit>> = edits
        .iter()
        .map(|e| {
            e.as_ref()
                .filter(|e| e.range.start != e.range.end || !e.text.is_empty())
        })
        .collect();
    let mut order: Vec<(usize, &EachEdit)> = edits
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.map(|e| (i, e)))
        .collect();
    order.sort_by_key(|(_, e)| (e.range.start, e.range.end));
    let mut kept: Vec<Edit> = Vec::new();
    let mut owner = vec![usize::MAX; cursors.len()];
    for (i, e) in order {
        if let Some(j) = kept.len().checked_sub(1) {
            let (range, new) = &mut kept[j];
            if *range == e.range && *new == e.text {
                owner[i] = j;
                continue;
            }
            if e.range.start < range.end {
                if new.is_empty() && e.text.is_empty() {
                    range.end = range.end.max(e.range.end);
                    owner[i] = j;
                    continue;
                }
                return Err(TextError::OverlappingEdits {
                    end: range.end,
                    start: e.range.start,
                });
            }
        }
        kept.push((e.range.clone(), e.text.clone()));
        owner[i] = kept.len() - 1;
    }
    let sizes: Vec<(Range<usize>, usize)> =
        kept.iter().map(|(r, t)| (r.clone(), t.len())).collect();
    too_long(text.len(), &sizes)?;
    let mut starts = Vec::with_capacity(kept.len());
    let mut shift = 0isize;
    for (range, new) in &kept {
        starts.push(
            range
                .start
                .checked_add_signed(shift)
                .expect("new starts are offsets"),
        );
        shift += new.len() as isize - (range.end - range.start) as isize;
    }
    // Mapping through the kept edits themselves, so seams stay.
    let through = |p: usize, assoc: Assoc| -> usize {
        for (j, (range, new)) in kept.iter().enumerate() {
            if range.start < p && p < range.end {
                return if assoc == Assoc::Before {
                    starts[j]
                } else {
                    starts[j] + new.len()
                };
            }
        }
        let mut before = p as isize;
        let mut inserted = 0;
        for (range, new) in &kept {
            if range.start == p && range.end == p {
                inserted += new.len();
            } else if range.end <= p {
                before += new.len() as isize - (range.end - range.start) as isize;
            }
        }
        let before = before as usize;
        if assoc == Assoc::Before {
            before
        } else {
            before + inserted
        }
    };
    let placed: Vec<Pair> = cursors
        .iter()
        .enumerate()
        .map(|(i, &c)| match edits[i].filter(|e| e.place != Place::Map) {
            Some(e) => {
                let j = owner[i];
                let (a, b) = (starts[j], starts[j] + kept[j].1.len());
                match e.place {
                    Place::Start => (a, a),
                    Place::End => (b, b),
                    // `Select`, the only other place.
                    _ => (a, b),
                }
            }
            None if lo(c) == hi(c) => {
                let at = through(c.0, Assoc::After);
                (at, at)
            }
            None => {
                let (start, end) = (through(lo(c), Assoc::After), through(hi(c), Assoc::Before));
                if start >= end {
                    (start, start)
                } else if c.1 < c.0 {
                    (end, start)
                } else {
                    (start, end)
                }
            }
        })
        .collect();
    Ok(Each {
        text: apply_edits(text, &kept),
        selection: normalize(&placed, primary),
        kept,
    })
}

/// Where the line holding `at` starts and ends (before its break).
fn line_of(text: &str, at: usize) -> Range<usize> {
    let at = floor(text, at);
    let start = text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let end = text[at..].find(['\n', '\r']).map_or(text.len(), |i| at + i);
    start..end
}

/// One cursor's edit: typing at the head, deleting the range or the char
/// before a caret, replacing the range, deleting the cursor's line or back
/// to a shared point (overlapping deletions), the shared text at the shared
/// point (identical edits), an edit that does nothing, or (if `raw`) any
/// range; `None` 15% of the time.
fn cursor_edit(
    rng: &mut Pcg32,
    text: &str,
    c: Pair,
    shared: (usize, &str),
    raw: bool,
) -> Option<EachEdit> {
    const PLACES: [Place; 4] = [Place::Map, Place::Start, Place::End, Place::Select];
    let place = rng.choose(&PLACES).copied().unwrap_or(Place::Map);
    let (start, end, head) = (floor(text, lo(c)), floor(text, hi(c)), floor(text, c.1));
    let (range, new) = match rng.range(0..100u32) {
        0..15 => return None,
        15..35 => {
            let n = rng.range(1..4u32) as usize;
            (head..head, units(rng, n))
        }
        35..50 if start < end => (start..end, String::new()),
        35..50 => (floor(text, start.saturating_sub(1))..start, String::new()),
        50..62 => (start..end, new_text(rng)),
        62..72 => (
            line_of(text, start).start..line_of(text, end).end,
            String::new(),
        ),
        72..82 => (shared.0.min(start)..end.max(shared.0), String::new()),
        82..90 => (shared.0..shared.0, shared.1.to_owned()),
        90..95 => (head..head, String::new()),
        _ if raw => {
            let (a, b) = (below(rng, text.len() + 3), below(rng, text.len() + 3));
            let n = below(rng, 3);
            (a..b, units(rng, n))
        }
        _ => (start..end, new_text(rng)),
    };
    Some(EachEdit {
        range,
        text: new,
        place,
    })
}

/// Runs `change_each` with `edits` as the closure's answers, and returns
/// the result and the cursors the closure was called with.
fn run_change_each(
    rope: &Rope,
    selection: &Selection,
    edits: &[Option<EachEdit>],
) -> (Result<Transaction, TextError>, Vec<Cursor>) {
    let mut calls = Vec::new();
    let result = Transaction::change_each(rope, selection, |c| {
        let edit = edits.get(calls.len()).cloned().flatten();
        calls.push(c);
        edit.map(|e| (e.range, e.text, e.place))
    });
    (result, calls)
}

// ----------------------------------------------------------- history model

/// How a revision was made, for joining.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Root,
    New,
    Command(GroupId),
    Coalesce(u32),
}

/// One revision of the model.
struct Rev {
    text: String,
    parent: usize,
    last_child: Option<usize>,
    before: Selection,
    after: Selection,
    time: Option<Instant>,
    tag: Tag,
    id: RevisionId,
}

/// The history as ADR-0020 states it, with every revision's text as a
/// `String`.
struct Model {
    revs: Vec<Rev>,
    current: usize,
    saved: Option<usize>,
    open: bool,
}

impl Model {
    fn new(text: &str, root: RevisionId) -> Self {
        Model {
            revs: vec![Rev {
                text: text.to_owned(),
                parent: 0,
                last_child: None,
                before: Selection::caret(0),
                after: Selection::caret(0),
                time: None,
                tag: Tag::Root,
                id: root,
            }],
            current: 0,
            saved: Some(0),
            open: false,
        }
    }

    fn text(&self) -> &str {
        &self.revs[self.current].text
    }

    /// Whether a commit with `group` from `before` at `now` joins the
    /// current revision.
    fn joins(&self, group: Group, before: &Selection, now: Instant) -> bool {
        let rev = &self.revs[self.current];
        if !self.open {
            return false;
        }
        match group {
            Group::Command(id) => rev.tag == Tag::Command(id),
            Group::Coalesce { kind, window } => {
                rev.tag == Tag::Coalesce(kind)
                    && rev
                        .time
                        .and_then(|time| now.checked_duration_since(time))
                        .is_some_and(|d| d <= window)
                    && pairs(before) == pairs(&rev.after)
                    && before.primary_index() == rev.after.primary_index()
            }
            _ => false,
        }
    }

    fn tag(group: Group) -> Tag {
        match group {
            Group::Command(id) => Tag::Command(id),
            Group::Coalesce { kind, .. } => Tag::Coalesce(kind),
            _ => Tag::New,
        }
    }

    fn undo(&mut self) -> Option<Selection> {
        if self.current == 0 {
            return None;
        }
        let cur = self.current;
        let parent = self.revs[cur].parent;
        self.revs[parent].last_child = Some(cur);
        self.current = parent;
        self.open = false;
        Some(self.revs[cur].before.clone())
    }

    fn redo(&mut self) -> Option<Selection> {
        let child = self.revs[self.current].last_child?;
        self.current = child;
        self.open = false;
        Some(self.revs[child].after.clone())
    }

    /// Moves to `target` through the lowest common ancestor; the revision
    /// each hop reaches (the parent of each one undone, then each one
    /// redone) and the selection, or `None` if it is current.
    fn jump(&mut self, target: usize) -> Option<(Vec<usize>, Selection)> {
        if target == self.current {
            return None;
        }
        let (mut a, mut b) = (self.current, target);
        let (mut ups, mut downs) = (Vec::new(), Vec::new());
        while a != b {
            if a > b {
                ups.push(a);
                a = self.revs[a].parent;
            } else {
                downs.push(b);
                b = self.revs[b].parent;
            }
        }
        downs.reverse();
        for &c in ups.iter().chain(&downs) {
            let parent = self.revs[c].parent;
            self.revs[parent].last_child = Some(c);
        }
        let selection = match (downs.last(), ups.last()) {
            (Some(&d), _) => self.revs[d].after.clone(),
            (None, Some(&u)) => self.revs[u].before.clone(),
            (None, None) => return None,
        };
        let path = ups
            .iter()
            .map(|&u| self.revs[u].parent)
            .chain(downs.iter().copied())
            .collect();
        self.current = target;
        self.open = false;
        Some((path, selection))
    }
}

/// The history's accessors, the rope and the history's text against the
/// model.
fn agree(model: &Model, history: &History, rope: &Rope, what: &str) -> Result<(), String> {
    let text = model.text();
    if *rope != *text || *history.text() != *text {
        return Err(format!(
            "{what}: the rope ({} bytes) or the history's text ({} bytes) is not the model's ({} bytes)",
            rope.len(),
            history.text().len(),
            text.len()
        ));
    }
    let got = (
        history.current().seq(),
        history.saved().map(RevisionId::seq),
        history.revision_count(),
        history.can_undo(),
        history.can_redo(),
        history.is_modified(),
        history.root().seq(),
    );
    let want = (
        model.current as u64,
        model.saved.map(|s| s as u64),
        model.revs.len(),
        model.current != 0,
        model.revs[model.current].last_child.is_some(),
        model.saved != Some(model.current),
        0,
    );
    if got != want {
        return Err(format!(
            "{what}: (current, saved, count, can_undo, can_redo, is_modified, root) = {got:?}, want {want:?}"
        ));
    }
    Ok(())
}

/// A step against what it should be: its revision, the text before, the
/// selection, and one change set per hop, which applied in order to the
/// text before give the text after each hop, `hops`.
fn check_step(
    what: &str,
    step: &Step,
    want: (RevisionId, &Selection, &[&str]),
    before: &str,
) -> Result<(), String> {
    let (revision, selection, hops) = want;
    if step.revision != revision {
        return Err(format!(
            "{what}: the step is at {:?}, want {revision:?}",
            step.revision
        ));
    }
    if step.before != *before {
        return Err(format!(
            "{what}: the step's text before ({} bytes) is not the text before ({} bytes)",
            step.before.len(),
            before.len()
        ));
    }
    if step.selection != *selection {
        return Err(format!(
            "{what}: the step's selection is {:?}, want {:?}",
            pairs(&step.selection),
            pairs(selection)
        ));
    }
    if step.changes.len() != hops.len() {
        return Err(format!(
            "{what}: {} change sets, want {}",
            step.changes.len(),
            hops.len()
        ));
    }
    let mut text = step.before.clone();
    for (i, (changes, after)) in step.changes.iter().zip(hops).enumerate() {
        changes
            .apply(&mut text)
            .map_err(|e| format!("{what}: change set {i} of the step: {e}"))?;
        if text != *after {
            return Err(format!(
                "{what}: change set {i} of the step makes {} bytes, not the text after hop {i} \
                 ({} bytes)",
                text.len(),
                after.len()
            ));
        }
    }
    Ok(())
}

/// Ids and checkpoints of another history with 200 revisions, for ids a
/// test's history does not have.
struct Foreign {
    ids: Vec<RevisionId>,
    tokens: Vec<Checkpoint>,
}

fn foreign() -> &'static Foreign {
    static FOREIGN: OnceLock<Foreign> = OnceLock::new();
    FOREIGN.get_or_init(|| {
        let mut rope = Rope::new();
        let mut history = History::new(&rope);
        let mut ids = vec![history.root()];
        let mut tokens = vec![history.checkpoint()];
        for at in 0..200 {
            let changes = ChangeSet::new(&rope, [(at..at, "x")]).expect("an insert at the end");
            let tx = Transaction::new(changes, Selection::caret(at + 1));
            let step = history
                .commit(
                    &mut rope,
                    tx,
                    &Selection::caret(at),
                    Instant::now(),
                    Group::New,
                )
                .expect("a commit");
            ids.push(step.revision);
            tokens.push(history.checkpoint());
        }
        Foreign { ids, tokens }
    })
}

/// A change set, its canonical replacements, the text after and the
/// selection after.
type Made = (ChangeSet, Vec<Rep>, String, Selection);

/// What a commit should do.
struct Planned {
    changes: ChangeSet,
    /// The canonical replacements of `changes`, on the text they were
    /// built on.
    reps: Vec<Rep>,
    after_text: String,
    before: Selection,
    after: Selection,
    group: Group,
    /// Commit with an equal rope that is not the history's tree.
    foreign: bool,
}

/// What the history test counts, to show that it covers what it claims.
#[derive(Default)]
struct Seen {
    cases: Cell<u32>,
    branched: Cell<u32>,
    long_groups: Cell<u32>,
    joins: Cell<u32>,
    refused: Cell<u32>,
}

fn bump(cell: &Cell<u32>) {
    cell.set(cell.get() + 1);
}

/// One history sequence at real sizes, against the model.
struct Session<'a> {
    rng: Pcg32,
    rope: Rope,
    history: History,
    model: Model,
    initial: String,
    base: Instant,
    clock: Duration,
    shown: Selection,
    command: GroupId,
    tokens: Vec<Checkpoint>,
    seen: &'a Seen,
    branched: bool,
}

impl Session<'_> {
    fn now(&mut self) -> Instant {
        self.clock = match self.rng.range(0..100u32) {
            0..3 => self.clock.saturating_sub(Duration::from_millis(10)),
            3..30 => self.clock,
            30..60 => self.clock + Duration::from_millis(10),
            60..75 => self.clock + WINDOW,
            75..90 => self.clock + WINDOW + Duration::from_nanos(1),
            _ => self.clock + 2 * WINDOW,
        };
        self.base + self.clock
    }

    fn group(&mut self) -> Group {
        match self.rng.range(0..3u32) {
            0 => Group::New,
            1 => {
                if chance(&mut self.rng, 30) {
                    self.command = GroupId::fresh();
                }
                Group::Command(self.command)
            }
            _ => Group::Coalesce {
                kind: self.rng.range(1..3u32),
                window: WINDOW,
            },
        }
    }

    /// A commit built with `ChangeSet::new` (70%) or `change_each` (30%),
    /// 8% of them flawed: a change set for another length, one that cuts a
    /// char, a bad selection before or after, or an equal rope that is not
    /// the history's tree.
    fn commit(&mut self, what: &str) -> Result<(), String> {
        let text = self.model.text().to_owned();
        let now = self.now();
        let group = self.group();
        let mut before = if chance(&mut self.rng, 60) {
            self.shown.clone()
        } else {
            let (cursors, primary) = random_pairs(&mut self.rng, &text, 100, 0);
            selection_of(&cursors, primary)
        };
        let Some((changes, reps, after_text, mut after)) = (if chance(&mut self.rng, 70) {
            self.built(what, &text, &before)?
        } else {
            self.each(what, &text, &before)?
        }) else {
            return Ok(());
        };
        let mut planned = Planned {
            changes,
            reps,
            after_text,
            before: before.clone(),
            after: after.clone(),
            group,
            foreign: false,
        };
        if chance(&mut self.rng, 8) {
            let len = text.len();
            match self.rng.range(0..5u32) {
                0 => {
                    let other = Rope::from(format!("{text}x"));
                    planned.changes =
                        ChangeSet::new(&other, [(0..0, "y")]).expect("an insert at 0");
                }
                1 => {
                    if let Some((at, _)) = text.char_indices().find(|(_, c)| c.len_utf8() > 1) {
                        let ascii = "a".repeat(len);
                        let edits = [(at + 1..at + 1, "q".to_owned())];
                        planned.changes =
                            ChangeSet::new(&Rope::from(ascii.as_str()), edits.clone())
                                .expect("an insert in ASCII");
                        planned.reps = build(&ascii, &edits).map_err(|e| e.to_string())?;
                        planned.after_text = String::new();
                    }
                }
                2 => {
                    before = Selection::caret(len + 1 + below(&mut self.rng, 2));
                    planned.before = before.clone();
                }
                3 => {
                    let bad = match planned
                        .after_text
                        .char_indices()
                        .find(|(_, c)| c.len_utf8() > 1)
                    {
                        Some((at, _)) if chance(&mut self.rng, 50) => at + 1,
                        _ => planned.after_text.len() + 1,
                    };
                    after = Selection::new(after.primary(), [Cursor::caret(bad)]);
                    planned.after = after.clone();
                }
                _ => planned.foreign = true,
            }
        }
        self.record(what, &text, now, planned)
    }

    /// A change set from an edit list, checked against the model, with the
    /// selection after: mapped through it, or anywhere in the text after.
    fn built(
        &mut self,
        what: &str,
        text: &str,
        before: &Selection,
    ) -> Result<Option<Made>, String> {
        let flavor = if chance(&mut self.rng, 10) {
            Flavor::Any
        } else {
            Flavor::Real
        };
        let edits = edit_list(&mut self.rng, text, flavor);
        let want = build(text, &edits);
        let got = ChangeSet::new(&self.rope, edits);
        same(&format!("{what}: ChangeSet::new"), &got, &want)?;
        let (Ok(changes), Ok(reps)) = (got, want) else {
            return Ok(None);
        };
        let after_text = apply_reps(text, &reps);
        let lens = (text.len(), after_text.len());
        same_changes(&format!("{what}: ChangeSet::new"), &changes, &reps, lens)?;
        let after = if chance(&mut self.rng, 50) {
            let caret = if chance(&mut self.rng, 50) {
                Assoc::Before
            } else {
                Assoc::After
            };
            let mapped = before
                .map(&changes, caret)
                .map_err(|e| format!("{what}: map: {e}"))?;
            let want = map_selection(&pairs(before), before.primary_index(), &reps, lens, caret)
                .map_err(|e| format!("{what}: the map model: {e}"))?;
            same_selection(&format!("{what}: map"), &mapped, &want)?;
            mapped
        } else {
            let (cursors, primary) = random_pairs(&mut self.rng, &after_text, 100, 0);
            selection_of(&cursors, primary)
        };
        Ok(Some((changes, reps, after_text, after)))
    }

    /// A transaction from `change_each` on `before`, checked against the
    /// model.
    fn each(&mut self, what: &str, text: &str, before: &Selection) -> Result<Option<Made>, String> {
        let cursors = pairs(before);
        let shared_at = position(&mut self.rng, text, 0);
        let shared_text = new_text(&mut self.rng);
        let edits: Vec<Option<EachEdit>> = cursors
            .iter()
            .map(|&c| cursor_edit(&mut self.rng, text, c, (shared_at, &shared_text), false))
            .collect();
        let want = change_each_model(text, &cursors, before.primary_index(), &edits);
        let (got, calls) = run_change_each(&self.rope, before, &edits);
        same(&format!("{what}: change_each"), &got, &want)?;
        if calls != before.cursors() {
            return Err(format!(
                "{what}: change_each called the closure for {} cursors, want {}",
                calls.len(),
                cursors.len()
            ));
        }
        let (Ok(tx), Ok(want)) = (got, want) else {
            return Ok(None);
        };
        same_selection(
            &format!("{what}: change_each"),
            tx.selection(),
            &want.selection,
        )?;
        let reps = build(text, &want.kept).map_err(|e| format!("{what}: kept edits: {e}"))?;
        let (changes, after) = tx.into_parts();
        same_changes(
            &format!("{what}: change_each"),
            &changes,
            &reps,
            (text.len(), want.text.len()),
        )?;
        Ok(Some((changes, reps, want.text, after)))
    }

    /// Commits `planned` and checks the result, the step and the model.
    fn record(
        &mut self,
        what: &str,
        text: &str,
        now: Instant,
        planned: Planned,
    ) -> Result<(), String> {
        let len = text.len();
        let after_ok = |t: &str| validate(&pairs(&planned.after), t);
        let want: Result<(), TextError> = if planned.foreign {
            Err(TextError::HistoryMismatch { len, expected: len })
        } else if planned.changes.len_before() != len {
            Err(TextError::LengthMismatch {
                expected: planned.changes.len_before(),
                actual: len,
            })
        } else if planned.reps.is_empty() {
            after_ok(text)
        } else {
            validate(&pairs(&planned.before), text)
                .and_then(|()| {
                    planned
                        .reps
                        .iter()
                        .flat_map(|r| [r.old.start, r.old.end])
                        .find(|&b| !text.is_char_boundary(b))
                        .map_or(Ok(()), |byte| Err(TextError::NotCharBoundary { byte }))
                })
                .and_then(|()| after_ok(&planned.after_text))
        };
        let tx = Transaction::new(planned.changes.clone(), planned.after.clone());
        let result = if planned.foreign {
            let mut equal = Rope::from(text);
            let result = self
                .history
                .commit(&mut equal, tx, &planned.before, now, planned.group);
            if equal != *text {
                return Err(format!(
                    "{what}: a refused commit changed the rope it was given"
                ));
            }
            result
        } else {
            self.history
                .commit(&mut self.rope, tx, &planned.before, now, planned.group)
        };
        same(&format!("{what}: commit"), &result, &want)?;
        let Ok(step) = result else {
            bump(&self.seen.refused);
            return Ok(());
        };
        let current = self.model.current;
        if planned.reps.is_empty() {
            let id = self.model.revs[current].id;
            check_step(what, &step, (id, &planned.after, &[]), text)?;
            self.shown = planned.after;
            return Ok(());
        }
        if self.model.joins(planned.group, &planned.before, now) {
            bump(&self.seen.joins);
            let rev = &mut self.model.revs[current];
            rev.text.clone_from(&planned.after_text);
            rev.after = planned.after.clone();
            rev.time = rev.time.max(Some(now));
        } else {
            if self.model.revs[current].last_child.is_some() {
                self.branched = true;
            }
            let seq = self.model.revs.len();
            if step.revision.seq() != seq as u64 {
                return Err(format!(
                    "{what}: the new revision is {:?}, want sequence number {seq}",
                    step.revision
                ));
            }
            self.model.revs[current].last_child = Some(seq);
            self.model.revs.push(Rev {
                text: planned.after_text.clone(),
                parent: current,
                last_child: None,
                before: planned.before.clone(),
                after: planned.after.clone(),
                time: Some(now),
                tag: Model::tag(planned.group),
                id: step.revision,
            });
            self.model.current = seq;
        }
        self.model.open = true;
        let id = self.model.revs[self.model.current].id;
        check_step(
            what,
            &step,
            (id, &planned.after, &[planned.after_text.as_str()]),
            text,
        )?;
        if step.changes[0] != planned.changes {
            return Err(format!(
                "{what}: the step's change set is not the one committed"
            ));
        }
        self.shown = planned.after;
        Ok(())
    }

    /// 20-40 `Command` commits of one fresh id at scattered points.
    fn long_group(&mut self, what: &str) -> Result<(), String> {
        bump(&self.seen.long_groups);
        let id = GroupId::fresh();
        for k in 0..self.rng.range(20..41u32) {
            let what = format!("{what}, long group commit {k}");
            let text = self.model.text().to_owned();
            let at = position(&mut self.rng, &text, 0);
            let n = self.rng.range(1..7u32) as usize;
            let insert = units(&mut self.rng, n);
            let edits = [(at..at, insert.clone())];
            let reps = build(&text, &edits).map_err(|e| e.to_string())?;
            let changes = ChangeSet::new(&self.rope, edits).map_err(|e| e.to_string())?;
            let planned = Planned {
                changes,
                after_text: apply_reps(&text, &reps),
                reps,
                before: self.shown.clone(),
                after: Selection::caret(at + insert.len()),
                group: Group::Command(id),
                foreign: false,
            };
            let now = self.now();
            self.record(&what, &text, now, planned)?;
        }
        Ok(())
    }

    fn undo(&mut self, what: &str) -> Result<(), String> {
        let text = self.model.text().to_owned();
        let got = self
            .history
            .undo(&mut self.rope)
            .map_err(|e| format!("{what}: undo: {e}"))?;
        match (got, self.model.undo()) {
            (None, None) => Ok(()),
            (Some(step), Some(selection)) => {
                let id = self.model.revs[self.model.current].id;
                check_step(what, &step, (id, &selection, &[self.model.text()]), &text)?;
                self.shown = selection;
                Ok(())
            }
            (got, want) => Err(format!(
                "{what}: undo moved: {}, want {}",
                got.is_some(),
                want.is_some()
            )),
        }
    }

    fn redo(&mut self, what: &str) -> Result<(), String> {
        let text = self.model.text().to_owned();
        let got = self
            .history
            .redo(&mut self.rope)
            .map_err(|e| format!("{what}: redo: {e}"))?;
        match (got, self.model.redo()) {
            (None, None) => Ok(()),
            (Some(step), Some(selection)) => {
                let id = self.model.revs[self.model.current].id;
                check_step(what, &step, (id, &selection, &[self.model.text()]), &text)?;
                self.shown = selection;
                Ok(())
            }
            (got, want) => Err(format!(
                "{what}: redo moved: {}, want {}",
                got.is_some(),
                want.is_some()
            )),
        }
    }

    fn jump(&mut self, what: &str, target: usize) -> Result<(), String> {
        let text = self.model.text().to_owned();
        let id = self.model.revs[target].id;
        let got = self
            .history
            .jump_to(&mut self.rope, id)
            .map_err(|e| format!("{what}: jump_to: {e}"))?;
        match (got, self.model.jump(target)) {
            (None, None) => Ok(()),
            (Some(step), Some((path, selection))) => {
                let hops: Vec<&str> = path
                    .iter()
                    .map(|&reached| self.model.revs[reached].text.as_str())
                    .collect();
                check_step(what, &step, (id, &selection, &hops), &text)?;
                self.shown = selection;
                Ok(())
            }
            (got, want) => Err(format!(
                "{what}: jump_to moved: {}, want {}",
                got.is_some(),
                want.is_some()
            )),
        }
    }

    fn jump_somewhere(&mut self, what: &str) -> Result<(), String> {
        let count = self.model.revs.len();
        if chance(&mut self.rng, 5) {
            let id = foreign().ids[count + below(&mut self.rng, 3)];
            let got = self.history.jump_to(&mut self.rope, id);
            let want = Err::<(), _>(TextError::RevisionNotFound {
                revision: id.seq(),
                count,
            });
            return same(&format!("{what}: jump_to an unknown id"), &got, &want);
        }
        let target = below(&mut self.rng, count);
        self.jump(what, target)
    }

    fn checkpoint(&mut self, what: &str) -> Result<(), String> {
        let token = self.history.checkpoint();
        self.model.open = false;
        if token.revision() != self.model.revs[self.model.current].id
            || *token.text() != *self.model.text()
        {
            return Err(format!(
                "{what}: the checkpoint is not the current revision and its text"
            ));
        }
        self.tokens.push(token);
        Ok(())
    }

    fn mark_saved(&mut self, what: &str) -> Result<(), String> {
        let count = self.model.revs.len();
        if chance(&mut self.rng, 10) {
            let token = &foreign().tokens[count + below(&mut self.rng, 3)];
            let got = self.history.mark_saved(token);
            let want = Err::<(), _>(TextError::RevisionNotFound {
                revision: token.revision().seq(),
                count,
            });
            return same(
                &format!("{what}: mark_saved of an unknown revision"),
                &got,
                &want,
            );
        }
        if self.tokens.is_empty() {
            return Ok(());
        }
        let i = if chance(&mut self.rng, 60) {
            self.tokens.len() - 1
        } else {
            below(&mut self.rng, self.tokens.len())
        };
        let token = &self.tokens[i];
        self.history
            .mark_saved(token)
            .map_err(|e| format!("{what}: mark_saved: {e}"))?;
        let seq = token.revision().seq() as usize;
        // A saved revision's text never changes.
        if *token.text() != *self.model.revs[seq].text {
            return Err(format!(
                "{what}: revision {seq} no longer has its checkpoint's text"
            ));
        }
        self.model.saved = Some(seq);
        if seq == self.model.current {
            self.model.open = false;
        }
        Ok(())
    }

    /// An equal rope that is not the history's tree, or an edit that
    /// bypassed the history: `HistoryMismatch`, then recovery through
    /// `text()`.
    fn outside(&mut self, what: &str) -> Result<(), String> {
        let len = self.rope.len();
        if chance(&mut self.rng, 50) {
            let mut equal = Rope::from(self.model.text());
            let root = self.history.root();
            let got = match self.rng.range(0..3u32) {
                0 => self.history.undo(&mut equal),
                1 => self.history.redo(&mut equal),
                _ => self.history.jump_to(&mut equal, root),
            };
            let want = Err::<(), _>(TextError::HistoryMismatch { len, expected: len });
            same(&format!("{what}: a move with an equal rope"), &got, &want)?;
            if equal != *self.model.text() {
                return Err(format!(
                    "{what}: a refused move changed the rope it was given"
                ));
            }
        } else {
            let at = position(&mut self.rng, self.model.text(), 0);
            self.rope
                .insert(at, "!")
                .map_err(|e| format!("{what}: insert: {e}"))?;
            let got = self.history.redo(&mut self.rope);
            let want = Err::<(), _>(TextError::HistoryMismatch {
                len: len + 1,
                expected: len,
            });
            same(
                &format!("{what}: a move after an outside edit"),
                &got,
                &want,
            )?;
            self.rope = self.history.text().clone();
        }
        Ok(())
    }

    /// Full invariants; every revision (or 64 of them) reached by
    /// `jump_to` with its exact text and selection, which is "redo branches
    /// are kept"; undo to the root gives the first text; redo follows the
    /// last-visited children.
    fn finish(&mut self) -> Result<(), String> {
        self.history
            .__check_invariants()
            .map_err(|e| format!("history invariants: {e}"))?;
        self.rope
            .__check_invariants()
            .map_err(|e| format!("rope invariants: {e}"))?;
        let count = self.model.revs.len();
        let targets: Vec<usize> = if count <= 64 {
            (0..count).rev().collect()
        } else {
            (0..64).map(|_| below(&mut self.rng, count)).collect()
        };
        for target in targets {
            self.jump(&format!("the final jump to {target}"), target)?;
            agree(&self.model, &self.history, &self.rope, "after a final jump")?;
        }
        let mut undos = 0;
        while self.history.can_undo() {
            self.undo(&format!("final undo {undos}"))?;
            undos += 1;
        }
        self.undo("the undo at the root")?;
        if self.rope != *self.initial {
            return Err("undo to the root did not restore the first text".into());
        }
        let mut redos = 0;
        while self.history.can_redo() {
            self.redo(&format!("final redo {redos}"))?;
            redos += 1;
        }
        self.redo("the redo at the end")?;
        agree(&self.model, &self.history, &self.rope, "at the end")?;
        self.history
            .__check_invariants()
            .map_err(|e| format!("history invariants at the end: {e}"))
    }
}

/// One case: its input is the seed of its own generator, and its size.
#[derive(Debug)]
struct Plan {
    seed: u64,
    size: u32,
}

fn history_run(plan: &Plan, seen: &Seen) -> Result<(), String> {
    let mut rng = Pcg32::new(plan.seed, u64::from(plan.size));
    let initial = big_text(&mut rng, plan.size as usize * (MAX_TEXT / 2 / 100));
    let rope = Rope::from(initial.as_str());
    let history = History::new(&rope);
    let model = Model::new(&initial, history.root());
    let ops = plan.size / 2 + 2;
    // A long group in one size in 5, and in 10% of the others.
    let long_group = plan.size % 5 == 2 || chance(&mut rng, 10);
    let long_group_at = long_group.then(|| rng.range(0..ops));
    let mut session = Session {
        rng,
        rope,
        history,
        model,
        initial,
        base: Instant::now(),
        clock: Duration::from_secs(1_000),
        shown: Selection::caret(0),
        command: GroupId::fresh(),
        tokens: Vec::new(),
        seen,
        branched: false,
    };
    bump(&seen.cases);
    agree(&session.model, &session.history, &session.rope, "the start")?;
    for op in 0..ops {
        let what = format!("op {op}");
        if long_group_at == Some(op) {
            session.long_group(&what)?;
        }
        match session.rng.range(0..100u32) {
            0..50 => session.commit(&what)?,
            50..65 => session.undo(&what)?,
            65..75 => session.redo(&what)?,
            75..85 => session.jump_somewhere(&what)?,
            85..88 => {
                session.history.seal();
                session.model.open = false;
            }
            88..91 => session.checkpoint(&what)?,
            91..96 => session.mark_saved(&what)?,
            96 => {
                session.history.clear_saved();
                session.model.saved = None;
            }
            _ => session.outside(&what)?,
        }
        agree(&session.model, &session.history, &session.rope, &what)?;
    }
    if session.branched {
        bump(&seen.branched);
    }
    session.finish()
}

// =================================================================== tests

#[test]
fn types_are_send_sync_and_hide_text() {
    every_new_type_is_send_sync();
    let mut rope = Rope::from("a secret here\nsecret");
    let mut history = History::new(&rope);
    let selection = Selection::new(Cursor::new(2, 8), [Cursor::caret(14)]);
    let changes =
        ChangeSet::new(&rope, [(2..8, "SECRET secret"), (14..20, "")]).expect("two edits");
    let tx = Transaction::change_each(&rope, &selection, |c| {
        Some((c.range(), "secret", Place::Select))
    })
    .expect("one edit per cursor");
    let mut shown = vec![
        format!("{selection:?}"),
        format!("{:?}", selection.primary()),
        format!("{changes:?}"),
        format!("{:?}", changes.changes()),
        format!("{:?}", changes.changes().next()),
        format!("{tx:?}"),
        format!("{:?}", Assoc::After),
        format!("{:?}", Place::Select),
        format!("{:?}", GroupId::fresh()),
        format!(
            "{:?}",
            Group::Coalesce {
                kind: 1,
                window: WINDOW
            }
        ),
    ];
    let step = history
        .commit(
            &mut rope,
            Transaction::new(changes.clone(), Selection::caret(0)),
            &selection,
            Instant::now(),
            Group::New,
        )
        .expect("a commit");
    shown.push(format!("{step:?}"));
    shown.push(format!("{:?}", history.checkpoint()));
    shown.push(format!("{history:?}"));
    shown.push(format!("{:?}", history.current()));
    let undo = history.undo(&mut rope).expect("an undo");
    shown.push(format!("{undo:?}"));
    shown.push(format!("{:?}", changes.invert(&rope).expect("an inverse")));
    for error in [
        TextError::LengthMismatch {
            expected: 1,
            actual: 2,
        },
        TextError::OverlappingEdits { end: 3, start: 1 },
        TextError::TooLong { max: MAX },
        TextError::RevisionNotFound {
            revision: 9,
            count: 2,
        },
        TextError::HistoryMismatch {
            len: 4,
            expected: 4,
        },
    ] {
        shown.push(format!("{error:?}"));
        shown.push(error.to_string());
    }
    for text in &shown {
        assert!(!text.is_empty());
        assert!(
            !text.to_lowercase().contains("secret"),
            "Debug or Display shows text: {text}"
        );
    }
}

#[test]
fn cursors_and_selection_constructors() {
    let back = Cursor::new(5, 2);
    assert_eq!((back.start(), back.end(), back.range()), (2, 5, 2..5));
    assert!(back.is_backward() && !back.is_caret());
    let caret = Cursor::caret(3);
    assert_eq!((caret.anchor, caret.head, caret.range()), (3, 3, 3..3));
    assert!(caret.is_caret() && !caret.is_backward());
    let forward = Cursor::new(1, 4);
    assert!(!forward.is_backward() && !forward.is_caret());

    let one = Selection::caret(4);
    assert_eq!((pairs(&one), one.primary_index()), (vec![(4, 4)], 0));
    let single = Selection::single(back);
    assert_eq!(single.primary(), back);
    assert_eq!(single.cursors(), &[back]);

    // `transform` merges afterwards, and the primary follows its cursor.
    let three = Selection::new(Cursor::caret(5), [Cursor::caret(1), Cursor::caret(9)]);
    assert_eq!(three.primary_index(), 1);
    let quartered = three.clone().transform(|c| Cursor::caret(c.head / 4));
    assert_eq!(pairs(&quartered), vec![(0, 0), (1, 1), (2, 2)]);
    assert_eq!(quartered.primary_index(), 1);
    let collapsed = three.transform(|_| Cursor::caret(7));
    assert_eq!(
        (pairs(&collapsed), collapsed.primary_index()),
        (vec![(7, 7)], 0)
    );
}

#[test]
fn the_merge_rule_examples() {
    let cases: [(&[Pair], usize, &[Pair], usize); 16] = [
        // Two carets at one point become one.
        (&[(4, 4), (4, 4)], 1, &[(4, 4)], 0),
        // A caret at either edge of 2..6, or inside it, joins it.
        (&[(2, 6), (2, 2)], 1, &[(2, 6)], 0),
        (&[(2, 6), (4, 4)], 0, &[(2, 6)], 0),
        (&[(6, 6), (2, 6)], 0, &[(2, 6)], 0),
        // Where two ranges touch at a caret, it joins the earlier one.
        (&[(1, 3), (3, 3), (3, 5)], 1, &[(1, 3), (3, 5)], 0),
        (&[(0, 2), (2, 2), (2, 4)], 1, &[(0, 2), (2, 4)], 0),
        // Touching ranges stay apart; ranges that share a byte merge,
        // transitively.
        (&[(2, 4), (0, 2)], 0, &[(0, 2), (2, 4)], 1),
        (&[(0, 3), (2, 5)], 1, &[(0, 5)], 0),
        (&[(3, 6), (0, 2), (1, 4)], 0, &[(0, 6)], 0),
        // The primary's direction wins when it is a non-empty part.
        (&[(5, 2), (1, 3)], 0, &[(5, 1)], 0),
        (&[(1, 3), (5, 2)], 0, &[(1, 5)], 0),
        // Otherwise the first non-empty part's, forward among equal spans.
        (&[(9, 9), (3, 1), (1, 3)], 0, &[(1, 3), (9, 9)], 1),
        (&[(4, 4), (6, 2), (3, 5)], 0, &[(6, 2)], 0),
        (&[(4, 1), (2, 6), (9, 9)], 2, &[(6, 1), (9, 9)], 1),
        // The primary survives, and a caret before a range at its start
        // joins it.
        (&[(0, 2), (7, 7), (9, 12)], 1, &[(0, 2), (7, 7), (9, 12)], 1),
        (&[(2, 2), (2, 5)], 0, &[(2, 5)], 0),
    ];
    for (input, primary, want, want_primary) in cases {
        let got = selection_of(input, primary);
        got.__check_invariants()
            .unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(
            (pairs(&got), got.primary_index()),
            (want.to_vec(), want_primary),
            "{input:?} with primary {primary}"
        );
        assert_eq!(
            normalize(input, primary),
            (want.to_vec(), want_primary),
            "the model on {input:?}"
        );
    }
}

fn normalizing(seed: u64) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0);
    let len = below(&mut rng, 40);
    let text = "a".repeat(len);
    let (cursors, primary) = random_pairs(&mut rng, &text, 8, 0);
    let want = normalize(&cursors, primary);
    let got = selection_of(&cursors, primary);
    same_selection("Selection::new", &got, &want)?;
    // Idempotent, and independent of the order of the others.
    let again = selection_of(&pairs(&got), got.primary_index());
    if again != got {
        return Err(format!("normalizing again changed {:?}", pairs(&got)));
    }
    let mut others: Vec<Cursor> = cursors
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != primary)
        .map(|(_, &c)| cursor(c))
        .collect();
    rng.shuffle(&mut others);
    if Selection::new(cursor(cursors[primary]), others) != got {
        return Err("the order of the others changed the result".into());
    }
    // Every input is inside an output; every output edge is an input edge.
    let out = pairs(&got);
    for &c in &cursors {
        if !out.iter().any(|&o| lo(o) <= lo(c) && hi(c) <= hi(o)) {
            return Err(format!("{c:?} is in no output cursor"));
        }
    }
    for &o in &out {
        if !cursors.iter().any(|&c| lo(c) == lo(o)) || !cursors.iter().any(|&c| hi(c) == hi(o)) {
            return Err(format!("{o:?} does not start and end at input edges"));
        }
    }
    // `transform` applies to every cursor, then merges.
    let shift = below(&mut rng, 5);
    let moved: Vec<Pair> = out.iter().map(|&(a, h)| (a / 2 + shift, h)).collect();
    let transformed = got
        .clone()
        .transform(|c| Cursor::new(c.anchor / 2 + shift, c.head));
    same_selection(
        "transform",
        &transformed,
        &normalize(&moved, got.primary_index()),
    )
}

#[test]
fn selections_normalize_by_the_merge_rule() {
    check(
        "selections normalize by the merge rule",
        2_000,
        seed,
        |&seed| normalizing(seed),
    );
}

#[test]
fn validate_checks_each_start_then_end() {
    // "aé\n": 'a' 0, 'é' 1..3, LF 3; CR LF offsets are fine elsewhere.
    let rope = Rope::from("aé\n");
    let cases: [(&[Pair], Result<(), TextError>); 8] = [
        (&[(0, 4)], Ok(())),
        (&[(2, 2)], Err(TextError::NotCharBoundary { byte: 2 })),
        (
            &[(9, 9)],
            Err(TextError::ByteOutOfBounds { byte: 9, len: 4 }),
        ),
        // The first bad position decides: a mid-char caret before an
        // out-of-bounds one.
        (
            &[(2, 2), (5, 5)],
            Err(TextError::NotCharBoundary { byte: 2 }),
        ),
        // The start before the end, for either direction.
        (&[(2, 9)], Err(TextError::NotCharBoundary { byte: 2 })),
        (&[(9, 2)], Err(TextError::NotCharBoundary { byte: 2 })),
        (
            &[(1, 9)],
            Err(TextError::ByteOutOfBounds { byte: 9, len: 4 }),
        ),
        (
            &[(0, 1), (4, 2)],
            Err(TextError::NotCharBoundary { byte: 2 }),
        ),
    ];
    for (input, want) in cases {
        let selection = selection_of(input, 0);
        let got = selection.validate(&rope);
        assert_eq!(outcome(&got), outcome(&want), "{input:?}");
        assert_eq!(
            outcome(&validate(&pairs(&selection), "aé\n")),
            outcome(&want)
        );
    }
    let crlf = Rope::from("a\r\nb");
    assert!(
        Selection::new(Cursor::new(2, 1), [Cursor::caret(4)])
            .validate(&crlf)
            .is_ok()
    );
}

fn change(text: &str, edits: &[(Range<usize>, &str)]) -> ChangeSet {
    ChangeSet::new(&Rope::from(text), edits.iter().cloned())
        .unwrap_or_else(|e| panic!("{edits:?} on a text of {} bytes: {e}", text.len()))
}

#[test]
fn selection_mapping_examples() {
    use Assoc::{After, Before};
    // 0..5 of "abcdef" becomes "XYZ": a range strictly inside it and a
    // caret inside it collapse to where the start maps with the caret Assoc.
    let c = change("abcdef", &[(0..5, "XYZ")]);
    let s = Selection::new(Cursor::new(1, 2), [Cursor::caret(3)]);
    let before = s.map(&c, Before).expect("in bounds");
    let after = s.map(&c, After).expect("in bounds");
    assert_eq!(pairs(&before), vec![(0, 0)]);
    assert_eq!(pairs(&after), vec![(3, 3)]);
    // Deleting 2..3 of [0..2, 2..3, 3..5]: the middle range vanishes into
    // the caret at 2, which joins 0..2.
    let c = change("abcde", &[(2..3, "")]);
    let s = selection_of(&[(0, 2), (2, 3), (5, 3)], 2);
    let mapped = s.map(&c, After).expect("in bounds");
    assert_eq!(
        (pairs(&mapped), mapped.primary_index()),
        (vec![(0, 2), (4, 2)], 1)
    );
    // Text inserted at a range's edges stays outside it; direction stays.
    let c = change("abcdef", &[(2..2, "X"), (4..4, "Y")]);
    let s = Selection::single(Cursor::new(4, 2));
    assert_eq!(pairs(&s.map(&c, After).expect("in bounds")), vec![(5, 3)]);
    // A caret at a pure insert goes before or after it.
    let s = Selection::caret(2);
    assert_eq!(pairs(&s.map(&c, Before).expect("in bounds")), vec![(2, 2)]);
    assert_eq!(pairs(&s.map(&c, After).expect("in bounds")), vec![(3, 3)]);
    // Mid-char positions are not checked: there is no text.
    let mapped = Selection::caret(1).map(&ChangeSet::identity(2), After);
    assert_eq!(mapped.map(|s| pairs(&s)).ok(), Some(vec![(1, 1)]));
    // The first position past len_before, start then end, in cursor order.
    let id = ChangeSet::identity(3);
    fails(
        selection_of(&[(1, 1), (9, 2)], 0).map(&id, After),
        TextError::ByteOutOfBounds { byte: 9, len: 3 },
    );
    fails(
        selection_of(&[(5, 4), (8, 8)], 1).map(&id, After),
        TextError::ByteOutOfBounds { byte: 4, len: 3 },
    );
}

fn mapping(seed: u64) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0);
    let text = small_text(&mut rng);
    let (changes, reps, after) = valid_change(&mut rng, &text)?;
    let lens = (text.len(), after.len());
    let (mut cursors, primary) = random_pairs(&mut rng, &text, 8, 0);
    // Two or more cursors strictly inside one replacement.
    if chance(&mut rng, 30)
        && let Some(r) = reps.iter().find(|r| {
            let inner = floor(&text, r.old.start + (r.old.end - r.old.start) / 2);
            r.old.start < inner && inner < r.old.end
        })
    {
        for _ in 0..rng.range(2..4u32) {
            let a = floor(
                &text,
                r.old.start + 1 + below(&mut rng, r.old.end - r.old.start - 1),
            );
            let b = floor(
                &text,
                r.old.start + 1 + below(&mut rng, r.old.end - r.old.start - 1),
            );
            if r.old.start < a.min(b) {
                cursors.push((a, b));
            }
        }
    }
    let out_of_bounds = chance(&mut rng, 10);
    if out_of_bounds {
        let at = text.len() + 1 + below(&mut rng, 2);
        cursors.push(if chance(&mut rng, 50) {
            (at, at)
        } else {
            (lo(cursors[0]), at)
        });
    }
    let selection = selection_of(&cursors, primary);
    same_selection("Selection::new", &selection, &normalize(&cursors, primary))?;
    let rope_after = Rope::from(after.as_str());
    for caret in [Assoc::Before, Assoc::After] {
        let want = map_selection(
            &pairs(&selection),
            selection.primary_index(),
            &reps,
            lens,
            caret,
        );
        let got = selection.map(&changes, caret);
        same(&format!("map with {caret:?}"), &got, &want)?;
        if let (Ok(got), Ok(want)) = (got, want) {
            same_selection(&format!("map with {caret:?}"), &got, &want)?;
            got.validate(&rope_after)
                .map_err(|e| format!("a valid selection mapped to an invalid one: {e}"))?;
        }
    }
    Ok(())
}

#[test]
fn selections_map_like_a_per_position_model() {
    check(
        "selections map like a per-position model",
        1_200,
        seed,
        |&seed| mapping(seed),
    );
}

/// Edits of "abcdef", a position, and where it maps with `Before` and
/// `After`.
type MapRow = (&'static [(Range<usize>, &'static str)], usize, usize, usize);

#[test]
fn the_mapping_table() {
    use Assoc::{After, Before};
    let table: [MapRow; 21] = [
        // A pure insert of "XY" at 2: Before and After pick a side.
        (&[(2..2, "XY")], 2, 2, 4),
        (&[(2..2, "XY")], 1, 1, 1),
        (&[(2..2, "XY")], 3, 5, 5),
        (&[(2..2, "XY")], 6, 8, 8),
        (&[(2..2, "XY")], 0, 0, 0),
        // 2..4 replaced by "XYZ": its edges stick to the kept text beside
        // them; strictly inside, Before and After pick a side.
        (&[(2..4, "XYZ")], 2, 2, 2),
        (&[(2..4, "XYZ")], 3, 2, 5),
        (&[(2..4, "XYZ")], 4, 5, 5),
        (&[(2..4, "XYZ")], 1, 1, 1),
        (&[(2..4, "XYZ")], 5, 6, 6),
        (&[(2..4, "XYZ")], 6, 7, 7),
        // A deletion of 2..4.
        (&[(2..4, "")], 3, 2, 2),
        (&[(2..4, "")], 4, 2, 2),
        (&[(2..4, "")], 5, 3, 3),
        // At the ends of the text.
        (&[(0..0, "X"), (6..6, "Y")], 0, 0, 1),
        (&[(0..0, "X"), (6..6, "Y")], 6, 7, 8),
        (&[(0..6, "")], 0, 0, 0),
        (&[(0..6, "")], 6, 0, 0),
        // Touching edits merge, so the seam between them is forgotten
        // (non-law NL3): 2 is the start of "XY" either way.
        (&[(2..2, "X"), (2..4, "Y")], 2, 2, 2),
        (&[(2..2, "X"), (2..4, "Y")], 4, 4, 4),
        // Before p < sᵢ: kept text shifts back.
        (&[(1..2, ""), (4..5, "XYZ")], 3, 2, 2),
    ];
    for (edits, p, before, after) in table {
        let c = change("abcdef", edits);
        assert_eq!(
            (c.map(p, Before).ok(), c.map(p, After).ok()),
            (Some(before), Some(after)),
            "{edits:?} at {p}"
        );
        let reps = build(
            "abcdef",
            &edits
                .iter()
                .map(|(r, t)| (r.clone(), (*t).to_owned()))
                .collect::<Vec<_>>(),
        )
        .expect("valid edits");
        let lens = (c.len_before(), c.len_after());
        assert_eq!(
            map_model(&reps, lens, p, Before).ok(),
            Some(before),
            "model {edits:?}"
        );
        assert_eq!(
            map_model(&reps, lens, p, After).ok(),
            Some(after),
            "model {edits:?}"
        );
        fails(
            c.map(7, Before),
            TextError::ByteOutOfBounds { byte: 7, len: 6 },
        );
    }
    // Non-law NL2: on "abcd", insert x at 2, then delete 3..4. Composed, 2
    // with After maps to 2; step by step it maps to 3.
    let a = change("abcd", &[(2..2, "x")]);
    let b = change("abxcd", &[(3..4, "")]);
    let ab = a.compose(&b).expect("lengths match");
    assert_eq!(ab.map(2, After).ok(), Some(2));
    let stepwise = b.map(a.map(2, After).expect("in bounds"), After).ok();
    assert_eq!(stepwise, Some(3));
    // The identity maps every offset to itself.
    let id = ChangeSet::identity(5);
    assert!(id.is_identity() && id.changes().len() == 0);
    for p in 0..=5 {
        assert_eq!(
            (id.map(p, Before).ok(), id.map(p, After).ok()),
            (Some(p), Some(p))
        );
    }
}

#[test]
fn building_examples_and_error_order() {
    let text = "abcdef";
    let texts = |c: &ChangeSet| -> Vec<(Range<usize>, Range<usize>, String)> {
        c.changes()
            .map(|ch| {
                (
                    ch.old,
                    ch.new,
                    ch.text.map(Rope::to_string).unwrap_or_default(),
                )
            })
            .collect()
    };
    // Pure inserts at one offset keep their input order and come before an
    // edit starting there; touching edits merge.
    assert_eq!(
        texts(&change(text, &[(2..2, "a"), (2..2, "b")])),
        [(2..2, 2..4, "ab".into())]
    );
    assert_eq!(
        texts(&change(text, &[(2..2, "b"), (2..2, "a")])),
        [(2..2, 2..4, "ba".into())]
    );
    assert_eq!(
        texts(&change(text, &[(2..4, "X"), (2..2, "a")])),
        [(2..4, 2..4, "aX".into())]
    );
    assert_eq!(
        texts(&change(text, &[(4..4, "b"), (2..4, "X")])),
        [(2..4, 2..4, "Xb".into())]
    );
    assert_eq!(
        texts(&change(text, &[(5..6, ""), (0..1, "xy")])),
        [(0..1, 0..2, "xy".into()), (5..6, 6..6, String::new())]
    );
    // `==` is structural on the canonical form.
    assert_eq!(
        change(text, &[(0..1, "x"), (1..2, "y")]),
        change(text, &[(0..2, "xy")])
    );
    assert_ne!(change(text, &[(0..2, "xy")]), change(text, &[(0..2, "xz")]));
    // Edits that do nothing are dropped, even inside another's range.
    assert_eq!(
        change(text, &[(1..1, ""), (3..3, "")]),
        ChangeSet::identity(6)
    );
    assert_eq!(change(text, &[]), ChangeSet::identity(6));
    assert_eq!(
        change(text, &[(1..4, "X"), (2..2, "")]),
        change(text, &[(1..4, "X")])
    );
    // A deletion has no text.
    let c = change(text, &[(1..3, "")]);
    let only = c.changes().next().expect("one replacement");
    assert_eq!(
        (only.old, only.new, only.text.is_none()),
        (1..3, 1..1, true)
    );

    let rope = Rope::from(text);
    let new = |edits: &[(Range<usize>, &str)]| ChangeSet::new(&rope, edits.iter().cloned());
    fails(
        new(&[(1..4, "X"), (2..2, "y")]),
        TextError::OverlappingEdits { end: 4, start: 2 },
    );
    fails(
        new(&[(1..3, ""), (1..3, "")]),
        TextError::OverlappingEdits { end: 3, start: 1 },
    );
    fails(
        new(&[(3..5, ""), (0..4, "")]),
        TextError::OverlappingEdits { end: 4, start: 3 },
    );
    fails(
        new(&[(5..6, ""), (0..2, "x"), (1..3, "y")]),
        TextError::OverlappingEdits { end: 2, start: 1 },
    );

    // On "éé" (é is 0..2 and 2..4): every range is checked as
    // `Rope::replace` checks it, in input order, before anything else.
    let rope = Rope::from("éé");
    let new = |edits: &[(Range<usize>, &str)]| ChangeSet::new(&rope, edits.iter().cloned());
    fails(
        new(&[(2..3, ""), (1..2, "")]),
        TextError::NotCharBoundary { byte: 3 },
    );
    fails(
        new(&[(1..2, ""), (inverted(3, 1), "")]),
        TextError::NotCharBoundary { byte: 1 },
    );
    fails(
        new(&[(0..2, ""), (inverted(3, 1), "")]),
        TextError::InvertedRange { start: 3, end: 1 },
    );
    fails(
        new(&[(0..2, ""), (2..9, "")]),
        TextError::ByteOutOfBounds { byte: 9, len: 4 },
    );
    fails(
        new(&[(inverted(9, 5), "")]),
        TextError::InvertedRange { start: 9, end: 5 },
    );
    fails(
        new(&[(1..9, "")]),
        TextError::ByteOutOfBounds { byte: 9, len: 4 },
    );
    fails(new(&[(1..3, "")]), TextError::NotCharBoundary { byte: 1 });
    fails(new(&[(0..3, "")]), TextError::NotCharBoundary { byte: 3 });
    // An edit that does nothing is still checked.
    fails(
        new(&[(5..5, "")]),
        TextError::ByteOutOfBounds { byte: 5, len: 4 },
    );
    fails(new(&[(1..1, "")]), TextError::NotCharBoundary { byte: 1 });
    // Validity comes before overlap.
    fails(
        new(&[(0..4, ""), (0..2, ""), (5..5, "x")]),
        TextError::ByteOutOfBounds { byte: 5, len: 4 },
    );
    fails(
        new(&[(0..2, ""), (0..2, ""), (1..1, "x")]),
        TextError::NotCharBoundary { byte: 1 },
    );
    // Input order, not position order: edit 0's start, though edit 1's end
    // is lower.
    fails(
        new(&[(3..4, ""), (0..1, "")]),
        TextError::NotCharBoundary { byte: 3 },
    );
}

fn building(seed: u64) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0);
    let text = small_text(&mut rng);
    let rope = Rope::from(text.as_str());
    let edits = edit_list(&mut rng, &text, Flavor::Any);
    let want = build(&text, &edits);
    // New texts as `&str`, `String` and `Rope`.
    let got = match rng.range(0..3u32) {
        0 => ChangeSet::new(&rope, edits.iter().map(|(r, t)| (r.clone(), t.as_str()))),
        1 => ChangeSet::new(&rope, edits.iter().cloned()),
        _ => ChangeSet::new(
            &rope,
            edits
                .iter()
                .map(|(r, t)| (r.clone(), Rope::from(t.as_str()))),
        ),
    };
    same("ChangeSet::new", &got, &want)?;
    let (Ok(changes), Ok(reps)) = (got, want) else {
        return Ok(());
    };
    changes.__check_invariants()?;
    let after = apply_reps(&text, &reps);
    same_changes("ChangeSet::new", &changes, &reps, (text.len(), after.len()))?;
    // L1 and L2.
    let made = applied(&changes, &rope)?;
    made.__check_invariants()?;
    if made != *after || made.len() != changes.len_after() {
        return Err(format!(
            "applying made {} bytes, want {}",
            made.len(),
            after.len()
        ));
    }
    // L13: one by one, last first, by `old`; each new text is at `new`.
    let mut lsp = text.clone();
    for change in changes.changes().rev() {
        lsp.replace_range(
            change.old.clone(),
            &change.text.map(Rope::to_string).unwrap_or_default(),
        );
    }
    if lsp != after {
        return Err("applying changes() last first gave another text".into());
    }
    for change in changes.changes() {
        let new = change.text.map(Rope::to_string).unwrap_or_default();
        if after.get(change.new.clone()) != Some(new.as_str()) {
            return Err(format!(
                "the text after at {:?} is not the new text",
                change.new
            ));
        }
    }
    Ok(())
}

#[test]
fn building_matches_a_string_model() {
    check("building matches a String model", 2_000, seed, |&seed| {
        building(seed)
    });
}

fn laws(seed: u64) -> Result<(), String> {
    use Assoc::{After, Before};
    let mut rng = Pcg32::new(seed, 0);
    let t0 = small_text(&mut rng);
    let (a, ra, t1) = valid_change(&mut rng, &t0)?;
    let (b, _, t2) = valid_change(&mut rng, &t1)?;
    let (c, _, t3) = valid_change(&mut rng, &t2)?;
    let (r0, r1) = (Rope::from(t0.as_str()), Rope::from(t1.as_str()));
    let law = |name: &str, holds: bool| {
        if holds {
            Ok(())
        } else {
            Err(format!("{name} does not hold"))
        }
    };
    let e = |what: &'static str| move |e: TextError| format!("{what}: {e}");

    // L3, and the inverse's replacements.
    let ai = a.invert(&r0).map_err(e("invert"))?;
    ai.__check_invariants()?;
    let inverse: Vec<Rep> = ra
        .iter()
        .map(|r| Rep {
            old: r.new.clone(),
            new: r.old.clone(),
            text: t0[r.old.clone()].to_owned(),
        })
        .collect();
    same_changes("invert", &ai, &inverse, (t1.len(), t0.len()))?;
    law("L3 invert", applied(&ai, &r1)? == *t0)?;
    // L4.
    law(
        "L4 involution",
        ai.invert(&r1).map_err(e("invert twice"))? == a,
    )?;
    // L5.
    let ab = a.compose(&b).map_err(e("compose"))?;
    ab.__check_invariants()?;
    law("L5 compose", applied(&ab, &r0)? == *t2)?;
    // L6.
    let left = ChangeSet::identity(a.len_before())
        .compose(&a)
        .map_err(e("identity∘a"))?;
    let right = a
        .compose(&ChangeSet::identity(a.len_after()))
        .map_err(e("a∘identity"))?;
    law("L6 identity", left == a && right == a)?;
    // L7.
    let ab_c = ab.compose(&c).map_err(e("(a∘b)∘c"))?;
    let a_bc = a
        .compose(&b.compose(&c).map_err(e("b∘c"))?)
        .map_err(e("a∘(b∘c)"))?;
    law(
        "L7 associativity",
        ab_c == a_bc && applied(&ab_c, &r0)? == *t3,
    )?;
    // L8.
    let bi = b.invert(&r1).map_err(e("invert b"))?;
    let abi = ab.invert(&r0).map_err(e("invert a∘b"))?;
    law(
        "L8 inverse of compose",
        abi == bi.compose(&ai).map_err(e("b⁻¹∘a⁻¹"))?,
    )?;
    // L9.
    law(
        "L9 cancel",
        applied(&a.compose(&ai).map_err(e("a∘a⁻¹"))?, &r0)? == *t0,
    )?;

    // L10-L12 at every offset (boundaries for the boundary laws).
    let lens = (t0.len(), t1.len());
    let ab_old: Vec<Range<usize>> = ab.changes().map(|ch| ch.old).collect();
    let id = ChangeSet::identity(t0.len());
    for assoc in [Before, After] {
        let mut last = 0;
        for p in 0..=t0.len() {
            let m = a.map(p, assoc).map_err(e("map"))?;
            let want = map_model(&ra, lens, p, assoc).map_err(|e| e.to_string())?;
            if m != want {
                return Err(format!("map({p}, {assoc:?}) = {m}, want {want}"));
            }
            law("L10 monotone", m >= last)?;
            last = m;
            law(
                "L10 Before ≤ After",
                a.map(p, Before).map_err(e("map"))? <= a.map(p, After).map_err(e("map"))?,
            )?;
            law(
                "L10 identity",
                id.map(p, assoc).map_err(e("identity map"))? == p,
            )?;
            if !t0.is_char_boundary(p) {
                continue;
            }
            law("L10 boundaries", t1.is_char_boundary(m))?;
            if !ra.iter().any(|r| r.old.start <= p && p <= r.old.end) {
                law(
                    "L11 round trip",
                    ai.map(m, assoc).map_err(e("inverse map"))? == p,
                )?;
                let next = ra
                    .iter()
                    .find(|r| r.old.start > p)
                    .map_or(t0.len(), |r| r.old.start);
                law(
                    "L11 kept text",
                    t1.get(m..m + (next - p)) == Some(&t0[p..next]),
                )?;
            }
            if !ab_old.iter().any(|r| r.start <= p && p <= r.end) {
                let step = b.map(m, assoc).map_err(e("map b"))?;
                law(
                    "L12 composed map",
                    ab.map(p, assoc).map_err(e("map a∘b"))? == step,
                )?;
            }
        }
        same(
            "map past the end",
            &a.map(t0.len() + 1, assoc),
            &Err::<(), _>(TextError::ByteOutOfBounds {
                byte: t0.len() + 1,
                len: t0.len(),
            }),
        )?;
    }
    law(
        "L10 ends",
        a.map(0, Before).ok() == Some(0) && a.map(t0.len(), After).ok() == Some(a.len_after()),
    )?;

    // Errors leave the rope unchanged: another length, then a change set
    // whose ends are inside chars of this text (built on ASCII).
    let mut longer = Rope::from(format!("{t0}a"));
    let mismatch = Err::<(), _>(TextError::LengthMismatch {
        expected: t0.len(),
        actual: t0.len() + 1,
    });
    same("apply to another length", &a.apply(&mut longer), &mismatch)?;
    same("invert on another length", &a.invert(&longer), &mismatch)?;
    law(
        "a refused apply changes nothing",
        longer == *format!("{t0}a"),
    )?;
    let ascii = "a".repeat(t0.len());
    let (x, rx, _) = valid_change(&mut rng, &ascii)?;
    let first_bad = rx
        .iter()
        .flat_map(|r| [r.old.start, r.old.end])
        .find(|&byte| !t0.is_char_boundary(byte));
    let want = first_bad.map_or(Ok(()), |byte| Err(TextError::NotCharBoundary { byte }));
    let mut target = r0.clone();
    same("apply across chars", &x.apply(&mut target), &want)?;
    same("invert across chars", &x.invert(&r0), &want)?;
    if want.is_err() {
        law("a refused apply changes nothing", target == *t0)?;
    } else {
        law("apply on equal boundaries", target == *apply_reps(&t0, &rx))?;
    }
    // Compose checks lengths.
    same(
        "compose with another length",
        &a.compose(&ChangeSet::identity(a.len_after() + 1)),
        &Err::<(), _>(TextError::LengthMismatch {
            expected: a.len_after() + 1,
            actual: a.len_after(),
        }),
    )
}

#[test]
fn change_sets_obey_the_laws() {
    check("change sets obey the laws", 1_200, seed, |&seed| laws(seed));
}

/// b is built on an ASCII text as long as a(T), so its ends may fall inside
/// chars of a(T). Composing is refused exactly when one of them cuts a char
/// a inserted, at that offset of a(T); ends inside a kept char go through,
/// and the composed set is then refused when applied to T.
fn composing(seed: u64) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0);
    let t0 = small_text(&mut rng);
    let (a, ra, t1) = valid_change(&mut rng, &t0)?;
    let (b, rb, _) = valid_change(&mut rng, &"a".repeat(t1.len()))?;
    let ends: Vec<usize> = rb.iter().flat_map(|r| [r.old.start, r.old.end]).collect();
    let inserted = |q: usize| ra.iter().any(|r| r.new.start < q && q < r.new.end);
    let cuts: Vec<usize> = ends
        .iter()
        .copied()
        .filter(|&q| !t1.is_char_boundary(q) && inserted(q))
        .collect();
    let ab = match a.compose(&b) {
        Err(TextError::NotCharBoundary { byte }) if cuts.contains(&byte) => return Ok(()),
        Err(e) => return Err(format!("compose: {e:?}, want a cut at one of {cuts:?}")),
        Ok(_) if !cuts.is_empty() => {
            return Err(format!(
                "compose cut inserted chars at {cuts:?} without an error"
            ));
        }
        Ok(ab) => ab,
    };
    ab.__check_invariants()?;
    let kept_cut = ends.iter().find(|&&q| !t1.is_char_boundary(q));
    let mut text = Rope::from(t0.as_str());
    match (ab.apply(&mut text), kept_cut) {
        (Ok(()), None) if text == *apply_reps(&t1, &rb) => Ok(()),
        (Err(TextError::NotCharBoundary { .. }), Some(_)) if text == *t0 => Ok(()),
        (got, cut) => Err(format!(
            "applying a∘b gave {:?}, with a kept char cut at {cut:?}",
            outcome(&got)
        )),
    }
}

#[test]
fn compose_checks_the_chars_it_slices() {
    check("compose checks the chars it slices", 1_500, seed, |&seed| {
        composing(seed)
    });
}

#[test]
fn apply_invert_and_compose_errors() {
    // Built on ASCII, applied to "éé": the first start or end inside a
    // char, ascending.
    let rope = Rope::from("éé");
    for (edits, byte) in [
        (&[(1..1, "x"), (3..4, "")][..], 1),
        (&[(0..1, ""), (2..3, "y")][..], 1),
        (&[(2..2, "x"), (3..3, "y")][..], 3),
        (&[(0..2, "x"), (2..3, "")][..], 3),
    ] {
        let c = change("abcd", edits);
        let mut target = rope.clone();
        fails(c.apply(&mut target), TextError::NotCharBoundary { byte });
        assert!(target == "éé", "a refused apply changed the rope");
        fails(c.invert(&rope), TextError::NotCharBoundary { byte });
    }
    let c = change("abc", &[(1..2, "x")]);
    let mut longer = Rope::from("abcd");
    fails(
        c.apply(&mut longer),
        TextError::LengthMismatch {
            expected: 3,
            actual: 4,
        },
    );
    fails(
        c.invert(&longer),
        TextError::LengthMismatch {
            expected: 3,
            actual: 4,
        },
    );
    let mut empty = Rope::new();
    fails(
        ChangeSet::identity(1).apply(&mut empty),
        TextError::LengthMismatch {
            expected: 1,
            actual: 0,
        },
    );
    assert!(ChangeSet::identity(4).apply(&mut longer).is_ok() && longer == "abcd");

    // a inserts "é" at 1 of "abc" (bytes 1..3 of "aébc"). b, built on an
    // ASCII text of that length, cutting the é is an error at the offset
    // in a's result.
    let a = change("abc", &[(1..1, "é")]);
    for edits in [
        &[(2..2, "Z")][..],
        &[(1..2, "")][..],
        &[(2..3, "")][..],
        &[(2..4, "")][..],
        &[(0..2, "q")][..],
        &[(0..1, ""), (2..2, "Z")][..],
    ] {
        let b = change("xxxxx", edits);
        fails(a.compose(&b), TextError::NotCharBoundary { byte: 2 });
    }
    // Deleting all of it, or inserting next to it, is fine.
    for (edits, want) in [
        (&[(1..3, "")][..], "abc"),
        (&[(1..1, "Q")][..], "aQébc"),
        (&[(3..3, "Q"), (0..1, "")][..], "éQbc"),
        (&[(0..5, "")][..], ""),
    ] {
        let b = change("xxxxx", edits);
        let ab = a.compose(&b).unwrap_or_else(|e| panic!("{edits:?}: {e}"));
        ab.__check_invariants().expect("canonical");
        assert!(
            applied(&ab, &Rope::from("abc")).expect("applies") == want,
            "{edits:?}"
        );
    }
    fails(
        a.compose(&ChangeSet::identity(4)),
        TextError::LengthMismatch {
            expected: 4,
            actual: 5,
        },
    );
    fails(
        ChangeSet::identity(3).compose(&change("xxxxx", &[(0..1, "")])),
        TextError::LengthMismatch {
            expected: 5,
            actual: 3,
        },
    );
}

#[test]
fn changes_iterate_from_both_ends() {
    let c = change(
        "abcdefghij",
        &[(1..2, "x"), (4..4, "yy"), (6..8, ""), (9..10, "zzz")],
    );
    let olds: Vec<Range<usize>> = c.changes().map(|ch| ch.old).collect();
    assert_eq!(olds, [1..2, 4..4, 6..8, 9..10]);
    let news: Vec<Range<usize>> = c.changes().rev().map(|ch| ch.new).collect();
    assert_eq!(news, [9..12, 8..8, 4..6, 1..2]);
    let mut it = c.changes();
    assert_eq!(it.len(), 4);
    assert_eq!(it.next().map(|ch| ch.old), Some(1..2));
    assert_eq!(it.next_back().map(|ch| ch.old), Some(9..10));
    assert_eq!(it.len(), 2);
    assert_eq!(it.size_hint(), (2, Some(2)));
    assert_eq!(it.next_back().map(|ch| ch.old), Some(6..8));
    assert_eq!(it.next().map(|ch| ch.old), Some(4..4));
    assert!(it.next().is_none() && it.next_back().is_none() && it.next().is_none());
    assert_eq!(it.len(), 0);
    let id = ChangeSet::identity(3);
    assert!(id.changes().next().is_none());
}

#[test]
fn change_each_examples() {
    let rope = Rope::from("abcdef");
    // All four places, with a cursor that makes no edit mapped.
    let s = selection_of(&[(1, 1), (2, 3), (4, 4), (6, 5)], 2);
    let edits = [
        Some(EachEdit {
            range: 1..1,
            text: "XY".into(),
            place: Place::Start,
        }),
        Some(EachEdit {
            range: 2..3,
            text: "Q".into(),
            place: Place::Select,
        }),
        Some(EachEdit {
            range: 4..4,
            text: "Z".into(),
            place: Place::End,
        }),
        None,
    ];
    let (tx, calls) = run_change_each(&rope, &s, &edits);
    let tx = tx.expect("no overlap");
    assert_eq!(calls, s.cursors());
    assert!(applied(tx.changes(), &rope).expect("applies") == "aXYbQdZef");
    assert_eq!(
        (pairs(tx.selection()), tx.selection().primary_index()),
        (vec![(1, 1), (4, 5), (7, 7), (9, 8)], 2)
    );

    // Uppercasing [0..2, 2..4] of "aaaa" with Map keeps the seam, though
    // the change set merges them.
    let rope = Rope::from("aaaa");
    let s = selection_of(&[(0, 2), (2, 4)], 0);
    let tx = Transaction::change_each(&rope, &s, |c| Some((c.range(), "AA", Place::Map)))
        .expect("touching");
    assert_eq!(pairs(tx.selection()), vec![(0, 2), (2, 4)]);
    assert_eq!(tx.changes(), &change("aaaa", &[(0..4, "AAAA")]));
    // ... while mapping through the merged change set loses it (NL3).
    assert_eq!(
        pairs(&s.map(tx.changes(), Assoc::After).expect("in bounds")),
        vec![(0, 0), (4, 4)]
    );

    // Touching edits that merge still place each cursor at its own text.
    let rope = Rope::from("ab");
    let s = selection_of(&[(0, 1), (1, 2)], 1);
    let edits = [
        Some(EachEdit {
            range: 0..1,
            text: "XX".into(),
            place: Place::Select,
        }),
        Some(EachEdit {
            range: 1..2,
            text: "YYY".into(),
            place: Place::Select,
        }),
    ];
    let tx = run_change_each(&rope, &s, &edits).0.expect("touching");
    assert_eq!(
        (pairs(tx.selection()), tx.selection().primary_index()),
        (vec![(0, 2), (2, 5)], 1)
    );
    assert_eq!(tx.changes().changes().len(), 1);

    // Carets at 2 and 4 of "abcdef" deleting back to 0: one deletion, one
    // caret, whether placed or mapped.
    let rope = Rope::from("abcdef");
    let s = selection_of(&[(2, 2), (4, 4)], 1);
    for place in [Place::Start, Place::Map, Place::End, Place::Select] {
        let tx = Transaction::change_each(&rope, &s, |c| Some((0..c.head, "", place)))
            .expect("deletions merge");
        assert_eq!(tx.changes(), &change("abcdef", &[(0..4, "")]), "{place:?}");
        assert_eq!(
            (pairs(tx.selection()), tx.selection().primary_index()),
            (vec![(0, 0)], 0)
        );
    }

    // Two cursors on one line both deleting it: one deletion. Two cursors
    // inserting the same text at one point: one insert.
    let rope = Rope::from("one\ntwo\nthree");
    let s = selection_of(&[(5, 5), (6, 7)], 0);
    let tx =
        Transaction::change_each(&rope, &s, |_| Some((4..8, "", Place::Start))).expect("identical");
    assert!(applied(tx.changes(), &rope).expect("applies") == "one\nthree");
    assert_eq!(pairs(tx.selection()), vec![(4, 4)]);
    let tx =
        Transaction::change_each(&rope, &s, |_| Some((0..0, "# ", Place::End))).expect("identical");
    assert!(applied(tx.changes(), &rope).expect("applies") == "# one\ntwo\nthree");
    assert_eq!(pairs(tx.selection()), vec![(2, 2)]);
    // Different inserts at one point keep cursor order and their own places.
    let rope = Rope::from("abcdef");
    let s = selection_of(&[(1, 1), (3, 3)], 0);
    let edits = [
        Some(EachEdit {
            range: 1..1,
            text: "x".into(),
            place: Place::End,
        }),
        Some(EachEdit {
            range: 1..1,
            text: "y".into(),
            place: Place::End,
        }),
    ];
    let tx = run_change_each(&rope, &s, &edits)
        .0
        .expect("touching inserts");
    assert!(applied(tx.changes(), &rope).expect("applies") == "axybcdef");
    assert_eq!(pairs(tx.selection()), vec![(2, 2), (3, 3)]);

    // Other overlaps are errors.
    let s = selection_of(&[(1, 1), (3, 3)], 0);
    for (first, second, end, start) in [
        ((0..3, ""), (2..4, "x"), 3, 2),
        ((0..3, "x"), (0..3, "y"), 3, 0),
        ((1..1, "x"), (0..3, ""), 3, 1),
        ((0..4, "x"), (1..2, ""), 4, 1),
    ] {
        let edits = [
            Some(EachEdit {
                range: first.0,
                text: first.1.into(),
                place: Place::End,
            }),
            Some(EachEdit {
                range: second.0,
                text: second.1.into(),
                place: Place::End,
            }),
        ];
        fails(
            run_change_each(&rope, &s, &edits).0,
            TextError::OverlappingEdits { end, start },
        );
    }

    // An edit with an empty range and empty text counts as None: mapped.
    let edits = [
        Some(EachEdit {
            range: 1..1,
            text: "XY".into(),
            place: Place::End,
        }),
        Some(EachEdit {
            range: 3..3,
            text: String::new(),
            place: Place::Start,
        }),
    ];
    let tx = run_change_each(&rope, &s, &edits).0.expect("one insert");
    assert_eq!(pairs(tx.selection()), vec![(3, 3), (5, 5)]);

    // A bad selection fails before any closure is called; every closure is
    // called before any edit is checked; ranges are checked in cursor order.
    let rope = Rope::from("aé");
    for (s, want) in [
        (
            Selection::caret(9),
            TextError::ByteOutOfBounds { byte: 9, len: 3 },
        ),
        (Selection::caret(2), TextError::NotCharBoundary { byte: 2 }),
    ] {
        let (got, calls) = run_change_each(&rope, &s, &[]);
        fails(got, want);
        assert!(calls.is_empty());
    }
    let s = selection_of(&[(0, 0), (1, 1), (3, 3)], 0);
    let edits = [
        Some(EachEdit {
            range: 0..2,
            text: String::new(),
            place: Place::End,
        }),
        Some(EachEdit {
            range: inverted(3, 1),
            text: String::new(),
            place: Place::End,
        }),
        None,
    ];
    let (got, calls) = run_change_each(&rope, &s, &edits);
    fails(got, TextError::NotCharBoundary { byte: 2 });
    assert_eq!(calls.len(), 3);

    // An edit that does nothing has its range checked before it counts as
    // None: an empty edit at byte 2 of an empty text is out of bounds, and
    // the closure is called once, for the one cursor.
    let mut calls = 0;
    let got = Transaction::change_each(&Rope::new(), &Selection::caret(0), |_| {
        calls += 1;
        Some((2..2, "", Place::Select))
    });
    fails(got, TextError::ByteOutOfBounds { byte: 2, len: 0 });
    assert_eq!(calls, 1);
}

/// After the stable sort, an edit collapses only into the edit kept just
/// before it. Identical deletions and replacements always collapse (another
/// edit of the same range is an overlap), but among pure inserts at one
/// offset only consecutive identical ones do.
#[test]
fn identical_edits_collapse_into_the_edit_kept_before_them() {
    let rope = Rope::from("0123456789");
    let s = Selection::new(Cursor::caret(2), [Cursor::caret(5), Cursor::caret(8)]);
    let edit_all = |range: Range<usize>, texts: [&'static str; 3]| {
        let mut calls = 0;
        Transaction::change_each(&rope, &s, |_| {
            calls += 1;
            Some((range.clone(), texts[calls - 1], Place::End))
        })
    };
    let cases = [
        (
            0..0,
            ["a", "b", "a"],
            "aba0123456789",
            vec![(1, 1), (2, 2), (3, 3)],
        ),
        (0..0, ["a", "a", "b"], "ab0123456789", vec![(1, 1), (2, 2)]),
        (0..1, ["", "", ""], "123456789", vec![(0, 0)]),
        (0..1, ["X", "X", "X"], "X123456789", vec![(1, 1)]),
    ];
    for (range, texts, after, carets) in cases {
        let tx = edit_all(range, texts).expect("valid edits");
        assert!(
            applied(tx.changes(), &rope).expect("applies") == after,
            "{texts:?}"
        );
        assert_eq!(pairs(tx.selection()), carets, "{texts:?}");
        assert_eq!(tx.selection().primary_index(), 0);
        assert_eq!(tx.changes().changes().len(), 1);
    }
    fails(
        edit_all(0..1, ["X", "Y", "X"]),
        TextError::OverlappingEdits { end: 1, start: 0 },
    );
}

fn each(seed: u64) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0);
    let text = small_text(&mut rng);
    let rope = Rope::from(text.as_str());
    let (cursors, primary) = random_pairs(&mut rng, &text, 8, 3);
    let selection = selection_of(&cursors, primary);
    let input = pairs(&selection);
    let shared_at = position(&mut rng, &text, 0);
    let shared_text = new_text(&mut rng);
    let edits: Vec<Option<EachEdit>> = input
        .iter()
        .map(|&c| cursor_edit(&mut rng, &text, c, (shared_at, &shared_text), true))
        .collect();
    let want = change_each_model(&text, &input, selection.primary_index(), &edits);
    let (got, calls) = run_change_each(&rope, &selection, &edits);
    same("change_each", &got, &want)?;
    let called = if validate(&input, &text).is_ok() {
        calls == selection.cursors()
    } else {
        calls.is_empty()
    };
    if !called {
        return Err(format!(
            "the closure was called for {} of {} cursors",
            calls.len(),
            input.len()
        ));
    }
    let (Ok(tx), Ok(want)) = (got, want) else {
        return Ok(());
    };
    same_selection("change_each", tx.selection(), &want.selection)?;
    let rebuilt = ChangeSet::new(&rope, want.kept.iter().cloned())
        .map_err(|e| format!("the kept edits: {e}"))?;
    if *tx.changes() != rebuilt {
        return Err("the change set is not ChangeSet::new of the kept edits".into());
    }
    let reps = canonical(&want.kept);
    same_changes(
        "change_each",
        tx.changes(),
        &reps,
        (text.len(), want.text.len()),
    )?;
    let after = applied(tx.changes(), &rope)?;
    if after != *want.text {
        return Err("change_each made another text".into());
    }
    tx.selection()
        .validate(&after)
        .map_err(|e| format!("the selection after is not valid in the text after: {e}"))
}

#[test]
fn change_each_matches_a_model() {
    check("change_each matches a model", 2_000, seed, |&seed| {
        each(seed)
    });
}

#[test]
fn extreme_offsets_are_errors_not_panics() {
    let rope = Rope::from("aé");
    let max = usize::MAX;
    let oob = |byte| TextError::ByteOutOfBounds { byte, len: 3 };
    fails(
        Selection::single(Cursor::new(0, max)).validate(&rope),
        oob(max),
    );
    fails(
        Selection::caret(max).map(&ChangeSet::identity(3), Assoc::After),
        oob(max),
    );
    fails(ChangeSet::identity(3).map(max, Assoc::Before), oob(max));
    fails(ChangeSet::new(&rope, [(max..max, "")]), oob(max));
    fails(ChangeSet::new(&rope, [(0..max, "x")]), oob(max));
    fails(
        ChangeSet::new(&rope, [(inverted(max, 0), "x")]),
        TextError::InvertedRange { start: max, end: 0 },
    );
    fails(
        Transaction::change_each(&rope, &Selection::caret(0), |_| {
            Some((1..max, "x", Place::End))
        }),
        oob(max),
    );
    let both = Selection::new(
        Cursor::new(max, 0),
        [Cursor::caret(max), Cursor::new(1, max)],
    );
    assert_eq!(pairs(&both), vec![(max, 0)]);
    let mut b = Buffer::new("aé");
    let tx = Transaction::new(
        ChangeSet::new(&b.rope, [(0..1, "")]).expect("a deletion"),
        Selection::caret(0),
    );
    fails(
        b.commit(tx, Some(&Selection::caret(max)), ms(0), Group::New),
        oob(max),
    );
    let tx = Transaction::new(
        ChangeSet::new(&b.rope, [(0..1, "")]).expect("a deletion"),
        Selection::caret(max),
    );
    fails(
        b.commit(tx, None, ms(0), Group::New),
        TextError::ByteOutOfBounds { byte: max, len: 2 },
    );
    assert_eq!((b.count(), b.text()), (1, "aé".to_owned()));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn too_long_is_an_error_and_never_overflows() {
    // 2⁶² bytes by 62 shared doublings, and 2⁶² - 1 as the sum of the
    // doublings; neither is ever traversed (no `==`, `Display` or
    // invariant check on them).
    let mut doublings = vec![Rope::from("a")];
    for _ in 0..62 {
        let mut next = doublings[doublings.len() - 1].clone();
        next.append(next.clone());
        doublings.push(next);
    }
    let huge = doublings[62].clone();
    assert_eq!(huge.len(), 1 << 62);
    let mut almost = Rope::new();
    for rope in &doublings[..62] {
        almost.append(rope.clone());
    }
    assert_eq!(almost.len(), (1 << 62) - 1);

    // One more doubling is too long; so are sums that overflow a usize.
    fails(
        ChangeSet::new(&huge, [(0..0, huge.clone())]),
        TextError::TooLong { max: MAX },
    );
    fails(
        ChangeSet::new(&huge, (0..4).map(|_| (0..0, huge.clone()))),
        TextError::TooLong { max: MAX },
    );
    fails(
        ChangeSet::new(&huge, (0..9).map(|_| (0..0, huge.clone()))),
        TextError::TooLong { max: MAX },
    );
    // Exactly isize::MAX is not too long, even when the inserts alone
    // would be; one byte more is.
    let at_max = ChangeSet::new(
        &huge,
        [
            (0..0, almost.clone()),
            (1..1, Rope::from("x")),
            (2..3, Rope::new()),
        ],
    )
    .expect("exactly isize::MAX bytes");
    assert_eq!(at_max.len_after(), MAX);
    fails(
        ChangeSet::new(&huge, [(0..0, almost.clone()), (1..1, Rope::from("x"))]),
        TextError::TooLong { max: MAX },
    );
    // Validity and overlap come first.
    fails(
        ChangeSet::new(
            &huge,
            [
                (0..0, huge.clone()),
                (huge.len() + 1..huge.len() + 1, Rope::new()),
            ],
        ),
        TextError::ByteOutOfBounds {
            byte: huge.len() + 1,
            len: huge.len(),
        },
    );
    fails(
        ChangeSet::new(&huge, [(0..2, huge.clone()), (1..3, Rope::new())]),
        TextError::OverlappingEdits { end: 2, start: 1 },
    );
    // `change_each` checks the same way.
    let both_ends = Selection::new(Cursor::caret(0), [Cursor::caret(huge.len())]);
    fails(
        Transaction::change_each(&huge, &Selection::caret(0), |c| {
            Some((c.head..c.head, huge.clone(), Place::End))
        }),
        TextError::TooLong { max: MAX },
    );
    let (got, calls) = {
        let mut calls = 0;
        let got = Transaction::change_each(&huge, &both_ends, |c| {
            calls += 1;
            Some((c.head..c.head, huge.clone(), Place::End))
        });
        (got, calls)
    };
    fails(got, TextError::TooLong { max: MAX });
    assert_eq!(calls, 2);
    // Identical inserts from two cursors collapse before the length is
    // checked: exactly isize::MAX.
    let two = Selection::new(Cursor::caret(0), [Cursor::caret(1)]);
    let tx = Transaction::change_each(&huge, &two, |_| Some((0..0, almost.clone(), Place::End)))
        .expect("one insert of 2⁶² - 1 bytes");
    assert_eq!(
        (tx.changes().len_after(), tx.changes().changes().len()),
        (MAX, 1)
    );
    // A one-byte insert works, applies, and goes through a history.
    let one = ChangeSet::new(&huge, [(0..0, "x")]).expect("one more byte");
    assert_eq!(one.len_after(), (1 << 62) + 1);
    let mut buffer = huge.clone();
    let mut history = History::new(&buffer);
    let step = history
        .commit(
            &mut buffer,
            Transaction::new(one, Selection::caret(1)),
            &Selection::caret(0),
            Instant::now(),
            Group::New,
        )
        .expect("a commit");
    assert_eq!((buffer.len(), step.changes.len()), ((1 << 62) + 1, 1));
    history.undo(&mut buffer).expect("an undo").expect("a step");
    assert_eq!(buffer.len(), 1 << 62);
    history.redo(&mut buffer).expect("a redo").expect("a step");
    assert_eq!(buffer.len(), (1 << 62) + 1);
}

// --------------------------------------------------------- history, fixed

/// A buffer for the fixed history cases: its rope, its history, the
/// selection shown, and a base instant for times.
struct Buffer {
    rope: Rope,
    history: History,
    shown: Selection,
    base: Instant,
}

impl Buffer {
    fn new(text: &str) -> Self {
        let rope = Rope::from(text);
        Buffer {
            history: History::new(&rope),
            rope,
            shown: Selection::caret(0),
            base: Instant::now(),
        }
    }

    fn at(&self, time: Duration) -> Instant {
        self.base + Duration::from_secs(100) + time
    }

    /// Replaces `range` with `text` from the shown selection, leaving a
    /// caret after it.
    fn edit(
        &mut self,
        range: Range<usize>,
        text: &str,
        time: Duration,
        group: Group,
    ) -> Result<Step, TextError> {
        let changes = ChangeSet::new(&self.rope, [(range.clone(), text)])?;
        let after = Selection::caret(range.start + text.len());
        let now = self.at(time);
        let step = self.history.commit(
            &mut self.rope,
            Transaction::new(changes, after),
            &self.shown,
            now,
            group,
        )?;
        self.shown = step.selection.clone();
        Ok(step)
    }

    /// Commits `tx` from `before` (the shown selection if `None`) at `time`,
    /// leaving the shown selection as it is.
    fn commit(
        &mut self,
        tx: Transaction,
        before: Option<&Selection>,
        time: Duration,
        group: Group,
    ) -> Result<Step, TextError> {
        let now = self.at(time);
        let before = before.unwrap_or(&self.shown).clone();
        self.history.commit(&mut self.rope, tx, &before, now, group)
    }

    /// Types `text` at the primary head.
    fn typed(&mut self, text: &str, time: Duration, group: Group) -> Step {
        let at = self.shown.primary().head;
        self.edit(at..at, text, time, group).expect("typing")
    }

    fn undo(&mut self) -> Option<Step> {
        let step = self.history.undo(&mut self.rope).expect("undo");
        if let Some(step) = &step {
            self.shown = step.selection.clone();
        }
        step
    }

    fn redo(&mut self) -> Option<Step> {
        let step = self.history.redo(&mut self.rope).expect("redo");
        if let Some(step) = &step {
            self.shown = step.selection.clone();
        }
        step
    }

    fn jump(&mut self, target: RevisionId) -> Option<Step> {
        let step = self
            .history
            .jump_to(&mut self.rope, target)
            .expect("jump_to");
        if let Some(step) = &step {
            self.shown = step.selection.clone();
        }
        step
    }

    fn text(&self) -> String {
        self.rope.to_string()
    }

    fn count(&self) -> usize {
        self.history.revision_count()
    }

    /// The accessors, to compare before and after a refused call.
    fn state(&self) -> (String, u64, Option<u64>, usize, bool, bool, bool, String) {
        (
            self.text(),
            self.history.current().seq(),
            self.history.saved().map(RevisionId::seq),
            self.count(),
            self.history.can_undo(),
            self.history.can_redo(),
            self.history.is_modified(),
            self.history.text().to_string(),
        )
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn typing(kind: u32) -> Group {
    Group::Coalesce {
        kind,
        window: WINDOW,
    }
}

#[test]
fn coalescing_needs_the_kind_the_window_and_unmoved_cursors() {
    let mut b = Buffer::new("");
    b.typed("a", ms(0), typing(1));
    b.typed("b", WINDOW, typing(1)); // exactly the window: joins
    assert_eq!(b.count(), 2);
    b.typed("c", 2 * WINDOW + Duration::from_nanos(1), typing(1)); // 1 ns past it
    assert_eq!(b.count(), 3);
    b.typed("d", 2 * WINDOW, typing(1)); // the clock went back
    assert_eq!(b.count(), 4);
    b.typed("e", 2 * WINDOW + ms(10), typing(2)); // another kind
    assert_eq!(b.count(), 5);
    b.typed("f", 2 * WINDOW + ms(20), typing(2));
    assert_eq!(b.count(), 5);
    b.shown = Selection::caret(0); // the cursor moved
    b.typed("g", 2 * WINDOW + ms(30), typing(2));
    assert_eq!(b.count(), 6);
    b.typed("h", 2 * WINDOW + ms(40), Group::Command(GroupId::fresh())); // not typing
    b.typed("i", 2 * WINDOW + ms(50), typing(2));
    assert_eq!(b.count(), 8);
    assert_eq!(b.text(), "ghiabcdef");
    // Each undo restores one revision's text exactly.
    for want in ["ghabcdef", "gabcdef", "abcdef", "abcd", "abc", "ab", ""] {
        b.undo().expect("a step");
        assert_eq!(b.text(), want);
    }
    assert!(b.undo().is_none());

    // A zero window joins at the same instant only.
    let mut b = Buffer::new("");
    let zero = Group::Coalesce {
        kind: 3,
        window: Duration::ZERO,
    };
    b.typed("a", ms(5), zero);
    b.typed("b", ms(5), zero);
    b.typed("c", ms(6), zero);
    assert_eq!(b.count(), 3);

    // The same cursors with another primary index do not join.
    let rope = Rope::from("ab");
    let mut history = History::new(&rope);
    let mut buffer = rope.clone();
    let start = Instant::now();
    let mut commit = |before: &Selection, primary_moved: bool| {
        let tx =
            Transaction::change_each(&buffer, before, |c| Some((c.head..c.head, "x", Place::End)))
                .expect("inserts");
        let after = tx.selection().clone();
        history
            .commit(&mut buffer, tx, before, start, typing(1))
            .expect("a commit");
        if primary_moved {
            Selection::new(
                after.cursors()[1 - after.primary_index()],
                after.cursors().iter().copied(),
            )
        } else {
            after
        }
    };
    let first = Selection::new(Cursor::caret(0), [Cursor::caret(2)]);
    let next = commit(&first, false);
    let moved = commit(&next, true);
    assert_eq!(pairs(&moved), vec![(2, 2), (6, 6)]);
    commit(&moved, false);
    drop(commit);
    assert_eq!(history.revision_count(), 3);
    assert!(buffer == "xxxabxxx");
}

#[test]
fn command_groups_join_across_time_and_cursor_jumps() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("abc");
    b.typed("1", ms(0), Group::Command(g));
    b.edit(1..2, "", ms(60_000), Group::Command(g))
        .expect("a deletion"); // a minute later, elsewhere
    b.shown = Selection::single(Cursor::new(2, 0));
    b.edit(3..3, "2", ms(0), Group::Command(g))
        .expect("an insert"); // the clock went back
    assert_eq!((b.count(), b.text()), (2, "1bc2".to_owned()));
    b.typed("n", ms(10), Group::New);
    b.typed("3", ms(20), Group::Command(g));
    assert_eq!((b.count(), b.text()), (4, "1bc2n3".to_owned()));
    b.undo();
    b.undo();
    assert_eq!(b.text(), "1bc2");
    let step = b.undo().expect("a step");
    assert_eq!(b.text(), "abc");
    // Undo restores the selection the group started from.
    assert_eq!(step.selection, Selection::caret(0));
    assert_eq!(step.changes.len(), 1);

    // New never joins, not even itself.
    let mut b = Buffer::new("");
    for k in 0..3 {
        b.typed("x", ms(k), Group::New);
    }
    assert_eq!(b.count(), 4);
}

#[test]
fn group_ids_are_unique_and_scoped_to_one_history() {
    let threads: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(|| (0..250).map(|_| GroupId::fresh()).collect::<Vec<_>>()))
        .collect();
    let mut ids = HashSet::new();
    for thread in threads {
        for id in thread.join().expect("no panic") {
            assert!(ids.insert(id), "an id was made twice");
        }
    }
    assert_eq!(ids.len(), 1_000);
    assert_ne!(GroupId::fresh(), GroupId::fresh());

    let g = GroupId::fresh();
    let (mut a, mut b) = (Buffer::new(""), Buffer::new(""));
    a.typed("a", ms(0), Group::Command(g));
    b.typed("x", ms(0), Group::Command(g));
    a.typed("b", ms(1), Group::Command(g));
    b.typed("y", ms(1), Group::Command(g));
    assert_eq!((a.count(), b.count()), (2, 2));
    assert_eq!((a.text(), b.text()), ("ab".to_owned(), "xy".to_owned()));
}

#[test]
fn identity_commits_record_nothing() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("abc");
    // At the root: no revision, and the root does not open.
    let id = |len, at| Transaction::new(ChangeSet::identity(len), Selection::caret(at));
    let step = b
        .commit(id(3, 1), None, ms(0), Group::Command(g))
        .expect("an identity commit");
    assert_eq!(
        (step.revision, step.changes.len(), b.count()),
        (b.history.root(), 0, 1)
    );
    assert_eq!(
        (step.selection.clone(), step.before.to_string()),
        (Selection::caret(1), "abc".to_owned())
    );
    b.shown = Selection::caret(3);
    b.typed("d", ms(0), Group::Command(g));
    assert_eq!(b.count(), 2);
    // An open revision stays open and keeps its tag.
    let step = b
        .commit(id(4, 4), None, ms(1), Group::New)
        .expect("an identity commit");
    assert!(
        step.changes.is_empty() && step.before == "abcd" && step.revision == b.history.current()
    );
    b.edit(0..0, "z", ms(2), Group::Command(g))
        .expect("an insert");
    assert_eq!((b.count(), b.text()), (2, "zabcd".to_owned()));

    // With typing: an identity commit that leaves the cursors where they
    // were keeps coalescing; one that moves them stops it.
    let mut b = Buffer::new("");
    b.typed("a", ms(0), typing(1));
    b.commit(id(1, 1), None, ms(1), typing(1))
        .expect("an identity commit");
    b.typed("b", ms(2), typing(1));
    assert_eq!(b.count(), 2);
    b.commit(id(2, 0), None, ms(3), typing(1))
        .expect("an identity commit");
    b.shown = Selection::caret(0);
    b.typed("c", ms(4), typing(1));
    assert_eq!((b.count(), b.text()), (3, "cab".to_owned()));

    // Only the selection after is checked; the selection before is not.
    let state = b.state();
    let bad = Selection::caret(99);
    fails(
        b.commit(id(3, 99), None, ms(5), Group::New),
        TextError::ByteOutOfBounds { byte: 99, len: 3 },
    );
    assert_eq!(b.state(), state);
    b.commit(id(3, 0), Some(&bad), ms(5), Group::New)
        .expect("the selection before is not checked");
    assert_eq!(b.state(), state);
    fails(
        b.commit(id(4, 0), None, ms(5), Group::New),
        TextError::LengthMismatch {
            expected: 4,
            actual: 3,
        },
    );
    assert_eq!(b.state(), state);
}

#[test]
fn a_group_that_cancels_out_stays() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("abc");
    b.edit(1..1, "x", ms(0), Group::Command(g))
        .expect("an insert");
    b.edit(1..2, "", ms(1), Group::Command(g))
        .expect("a deletion");
    assert_eq!(
        (b.count(), b.text(), b.history.can_undo()),
        (2, "abc".to_owned(), true)
    );
    let undo = b.undo().expect("a step");
    assert_eq!((undo.changes.len(), undo.revision), (1, b.history.root()));
    assert!(undo.changes[0].is_identity());
    let redo = b.redo().expect("a step");
    assert!(redo.changes.len() == 1 && redo.changes[0].is_identity());
    assert!(b.history.is_modified());
    b.history.__check_invariants().expect("invariants");
}

#[test]
fn seal_and_checkpoint_close_the_revision() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("");
    b.typed("a", ms(0), Group::Command(g));
    b.history.seal();
    b.typed("b", ms(0), Group::Command(g));
    assert_eq!(b.count(), 3);
    let cp = b.history.checkpoint();
    b.typed("c", ms(0), Group::Command(g));
    assert_eq!(b.count(), 4);
    assert_eq!(
        (cp.revision().seq(), cp.text().to_string()),
        (2, "ab".to_owned())
    );
    b.typed("d", ms(0), typing(1));
    b.history.seal();
    b.typed("e", ms(1), typing(1));
    assert_eq!((b.count(), b.text()), (6, "abcde".to_owned()));
}

#[test]
fn saved_markers_follow_checkpoints() {
    let mut b = Buffer::new("");
    assert_eq!(b.history.saved(), Some(b.history.root()));
    assert!(!b.history.is_modified());
    let r1 = b.typed("a", ms(0), typing(1)).revision;
    assert!(b.history.is_modified());
    let cp1 = b.history.checkpoint();
    // Typing after a checkpoint starts a new revision, so a saved
    // revision's text never changes.
    let r2 = b.typed("b", ms(1), typing(1)).revision;
    assert_ne!(r1, r2);
    b.history.mark_saved(&cp1).expect("in range");
    assert_eq!(b.history.saved(), Some(r1));
    assert!(b.history.is_modified());
    assert_eq!(cp1.text().to_string(), "a");
    b.undo();
    assert!(!b.history.is_modified());
    b.redo();
    assert!(b.history.is_modified());
    // The saved revision is never joined: checkpoint, save, type.
    let cp2 = b.history.checkpoint();
    b.history.mark_saved(&cp2).expect("in range");
    assert!(!b.history.is_modified());
    let r3 = b.typed("c", ms(2), typing(1)).revision;
    assert!(r3 != r2 && b.history.is_modified());
    b.undo();
    assert_eq!(
        (b.text(), b.history.is_modified()),
        ("ab".to_owned(), false)
    );
    // The saved revision on another branch.
    b.undo();
    b.undo();
    let r4 = b.typed("z", ms(3), Group::New).revision;
    assert!(b.history.is_modified());
    b.jump(r2).expect("a step");
    assert_eq!(
        (b.text(), b.history.is_modified()),
        ("ab".to_owned(), false)
    );
    b.jump(r4).expect("a step");
    assert!(b.history.is_modified());
    // No revision is on disk after `clear_saved`.
    b.history.clear_saved();
    assert_eq!(b.history.saved(), None);
    for target in [b.history.root(), r1, r2, r3, r4] {
        b.jump(target);
        assert!(b.history.is_modified(), "{target:?}");
    }
    // A checkpoint of a revision this history does not have.
    let token = &foreign().tokens[20];
    fails(
        b.history.mark_saved(token),
        TextError::RevisionNotFound {
            revision: 20,
            count: 5,
        },
    );
    assert_eq!(b.history.saved(), None);
    b.history.__check_invariants().expect("invariants");
}

#[test]
fn redo_takes_the_child_made_or_visited_last() {
    let mut b = Buffer::new("");
    let a = b.typed("a", ms(0), Group::New).revision;
    b.undo();
    let bb = b.typed("b", ms(0), Group::New).revision;
    b.undo();
    assert_eq!(b.redo().map(|s| s.revision), Some(bb));
    b.jump(a);
    b.undo();
    assert_eq!(b.redo().map(|s| s.revision), Some(a));
    assert_eq!(b.text(), "a");
    // A deeper tree: root - a - a1, a - a2. Jumping sets the children as
    // the moves one by one would.
    let a1 = b.typed("1", ms(0), Group::New).revision;
    b.undo();
    let a2 = b.typed("2", ms(0), Group::New).revision;
    let step = b.jump(a1).expect("two hops");
    assert_eq!((step.changes.len(), b.text()), (2, "a1".to_owned()));
    assert_eq!(step.before, "a2");
    b.undo();
    assert_eq!(b.redo().map(|s| s.revision), Some(a1));
    let step = b.jump(b.history.root()).expect("two hops");
    assert_eq!(step.changes.len(), 2);
    // Up only: the selection the last revision undone was made from.
    assert_eq!(step.selection, Selection::caret(0));
    assert_eq!(b.redo().map(|s| s.revision), Some(a));
    assert_eq!(b.redo().map(|s| s.revision), Some(a1));
    let step = b.jump(a2).expect("two hops");
    assert_eq!(
        (step.selection.clone(), b.text()),
        (Selection::caret(2), "a2".to_owned())
    );
    let step = b.jump(bb).expect("three hops");
    assert_eq!((step.changes.len(), b.text()), (3, "b".to_owned()));
    b.undo();
    assert_eq!(b.redo().map(|s| s.revision), Some(bb));
    // jump_to the current revision does nothing.
    assert!(b.jump(bb).is_none());
    // A jump leaves the redo choice of revisions off its path alone: from
    // `a`, redo goes to the child visited last (a2, then the older a1), not
    // to the newest.
    b.jump(a);
    assert_eq!(b.redo().map(|s| s.revision), Some(a2));
    b.jump(a1);
    b.jump(bb);
    b.jump(a);
    assert_eq!(b.redo().map(|s| s.revision), Some(a1));
    assert_eq!(b.text(), "a1");
}

#[test]
fn moves_that_do_nothing_keep_the_revision_open() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("");
    b.typed("a", ms(0), Group::Command(g));
    assert!(b.redo().is_none());
    b.typed("b", ms(0), Group::Command(g));
    let current = b.history.current();
    assert!(b.jump(current).is_none());
    b.typed("c", ms(0), Group::Command(g));
    assert_eq!((b.count(), b.text()), (2, "abc".to_owned()));
    b.typed("d", ms(10), typing(1));
    assert!(b.redo().is_none());
    b.typed("e", ms(20), typing(1));
    assert_eq!(b.count(), 3);
}

#[test]
fn the_root_never_joins() {
    let g = GroupId::fresh();
    let mut b = Buffer::new("x");
    b.typed("a", ms(0), Group::Command(g));
    b.undo();
    b.typed("b", ms(0), Group::Command(g));
    assert_eq!((b.count(), b.text()), (3, "bx".to_owned()));
    b.undo();
    assert!(b.undo().is_none());
    b.typed("c", ms(0), typing(1));
    assert_eq!(b.count(), 4);
}

#[test]
fn errors_change_nothing_and_come_in_order() {
    // "xaé\n": x 0, a 1, é 2..4, LF 4.
    let mut b = Buffer::new("aé\n");
    b.typed("x", ms(0), Group::New);
    let state = b.state();
    let ascii = Rope::from("aaaaa");
    let mid = ChangeSet::new(&ascii, [(3..3, "q")]).expect("an insert in ASCII");
    let fine = ChangeSet::new(&b.rope, [(0..1, "")]).expect("a deletion");
    let wrong = ChangeSet::identity(9);
    let now = b.at(ms(1));
    let cases = [
        // A foreign rope first, even with a wrong length.
        (
            true,
            wrong.clone(),
            Selection::caret(0),
            Selection::caret(0),
            TextError::HistoryMismatch {
                len: 5,
                expected: 5,
            },
        ),
        // The length before a bad selection.
        (
            false,
            wrong,
            Selection::caret(99),
            Selection::caret(99),
            TextError::LengthMismatch {
                expected: 9,
                actual: 5,
            },
        ),
        // The selection before before a change that cuts a char.
        (
            false,
            mid.clone(),
            Selection::caret(99),
            Selection::caret(0),
            TextError::ByteOutOfBounds { byte: 99, len: 5 },
        ),
        // The change before the selection after.
        (
            false,
            mid,
            Selection::caret(1),
            Selection::caret(99),
            TextError::NotCharBoundary { byte: 3 },
        ),
        // The selection after, on the text after.
        (
            false,
            fine.clone(),
            Selection::caret(1),
            Selection::caret(2),
            TextError::NotCharBoundary { byte: 2 },
        ),
        (
            false,
            fine,
            Selection::caret(1),
            Selection::caret(5),
            TextError::ByteOutOfBounds { byte: 5, len: 4 },
        ),
    ];
    for (foreign_rope, changes, before, after, want) in cases {
        let tx = Transaction::new(changes, after);
        let got = if foreign_rope {
            let mut equal = Rope::from("xaé\n");
            let got = b.history.commit(&mut equal, tx, &before, now, Group::New);
            assert!(equal == "xaé\n");
            got
        } else {
            b.history.commit(&mut b.rope, tx, &before, now, Group::New)
        };
        fails(got, want);
        assert_eq!(b.state(), state);
    }
    let mut other = Rope::from("abc");
    let tx = Transaction::new(ChangeSet::identity(3), Selection::caret(0));
    fails(
        b.history
            .commit(&mut other, tx, &Selection::caret(0), now, Group::New),
        TextError::HistoryMismatch {
            len: 3,
            expected: 5,
        },
    );
    fails(
        b.history.undo(&mut other),
        TextError::HistoryMismatch {
            len: 3,
            expected: 5,
        },
    );
    assert!(other == "abc");
    assert_eq!(b.state(), state);
    // The rope is still the history's tree: moves work.
    let mut equal = Rope::from("xaé\n");
    fails(
        b.history.undo(&mut equal),
        TextError::HistoryMismatch {
            len: 5,
            expected: 5,
        },
    );
    fails(
        b.history.redo(&mut equal),
        TextError::HistoryMismatch {
            len: 5,
            expected: 5,
        },
    );
    let root = b.history.root();
    fails(
        b.history.jump_to(&mut equal, root),
        TextError::HistoryMismatch {
            len: 5,
            expected: 5,
        },
    );
    let unknown = foreign().ids[7];
    fails(
        b.history.jump_to(&mut equal, unknown),
        TextError::HistoryMismatch {
            len: 5,
            expected: 5,
        },
    );
    fails(
        b.history.jump_to(&mut b.rope, unknown),
        TextError::RevisionNotFound {
            revision: 7,
            count: 2,
        },
    );
    assert_eq!(b.state(), state);
    assert!(b.undo().is_some());
    assert_eq!(b.text(), "aé\n");
}

#[test]
fn an_equal_rope_that_is_another_tree_is_refused() {
    for text in [String::new(), "abc".to_owned(), "é\r\n".repeat(4_000)] {
        let mut b = Buffer::new(&text);
        let len = text.len();
        let mut equal = Rope::from(text.as_str());
        assert!(equal == b.rope);
        let mismatch = || TextError::HistoryMismatch { len, expected: len };
        let tx = Transaction::new(
            ChangeSet::new(&equal, [(0..0, "x")]).expect("an insert"),
            Selection::caret(1),
        );
        fails(
            b.history.commit(
                &mut equal,
                tx,
                &Selection::caret(0),
                b.at(ms(0)),
                Group::New,
            ),
            mismatch(),
        );
        fails(b.history.undo(&mut equal), mismatch());
        fails(b.history.redo(&mut equal), mismatch());
        let root = b.history.root();
        fails(b.history.jump_to(&mut equal, root), mismatch());
        assert!(equal == *text && b.count() == 1);
        // A clone is the same tree; the rope it replaced is then stale
        // until the buffer restores the history's text.
        let mut clone = b.rope.clone();
        let tx = Transaction::new(
            ChangeSet::new(&clone, [(0..0, "x")]).expect("an insert"),
            Selection::caret(1),
        );
        b.history
            .commit(
                &mut clone,
                tx,
                &Selection::caret(0),
                b.at(ms(0)),
                Group::New,
            )
            .expect("a clone is accepted");
        fails(
            b.history.undo(&mut b.rope),
            TextError::HistoryMismatch {
                len,
                expected: len + 1,
            },
        );
        b.rope = b.history.text().clone();
        assert!(b.undo().is_some() && b.rope == *text);
    }
}

#[test]
fn an_insert_session_stays_one_replacement() {
    let mut rng = Pcg32::new(7, 1);
    let original = big_text(&mut rng, MAX_TEXT);
    let mut model = original.clone();
    let mut b = Buffer::new(&original);
    let start = floor(&original, original.len() / 2);
    b.shown = Selection::caret(start);
    let first = b.shown.clone();
    for k in 0..2_000u64 {
        let at = b.shown.primary().head;
        if k % 10 == 9 && at > start {
            let prev = floor(&model, at - 1);
            b.edit(prev..at, "", ms(10 * k), typing(1))
                .expect("a backspace");
            model.replace_range(prev..at, "");
        } else {
            let key = if k % 3 == 0 { "é" } else { "x" };
            b.typed(key, ms(10 * k), typing(1));
            model.insert_str(at, key);
        }
    }
    assert_eq!(b.count(), 2);
    assert!(b.rope == model);
    let undo = b.undo().expect("a step");
    assert!(b.rope == original);
    assert_eq!(undo.selection, first);
    assert_eq!(
        (undo.changes.len(), undo.changes[0].changes().len()),
        (1, 1)
    );
    let redo = b.redo().expect("a step");
    assert!(b.rope == model);
    assert_eq!(redo.changes[0].changes().len(), 1);
    b.history.__check_invariants().expect("invariants");
}

#[test]
fn a_command_group_of_scattered_edits_undoes_exactly() {
    let mut rng = Pcg32::new(11, 2);
    let lines = 400;
    let original: String = (0..lines).map(|i| format!("line {i} é\n")).collect();
    let mut starts: Vec<usize> = Vec::with_capacity(lines);
    let mut at = 0;
    for line in original.split_inclusive('\n') {
        starts.push(at);
        at += line.len();
    }
    let mut order: Vec<usize> = (0..lines).collect();
    rng.shuffle(&mut order);
    let mut model = original.clone();
    let mut b = Buffer::new(&original);
    let g = GroupId::fresh();
    for (k, &line) in order.iter().take(300).enumerate() {
        let at = starts[line];
        b.edit(at..at, "x", ms(k as u64), Group::Command(g))
            .expect("an insert");
        model.insert(at, 'x');
        for start in &mut starts[line + 1..] {
            *start += 1;
        }
    }
    assert_eq!(b.count(), 2);
    assert!(b.rope == model);
    let undo = b.undo().expect("a step");
    assert!(b.rope == original);
    assert_eq!(undo.changes[0].changes().len(), 300);
    let redo = b.redo().expect("a step");
    assert!(b.rope == model);
    assert_eq!(redo.changes[0].changes().len(), 300);
    b.history.__check_invariants().expect("invariants");
}

#[test]
fn history_matches_the_model_at_real_sizes() {
    let seen = Seen::default();
    check_sized(
        "history matches the model at real sizes",
        40,
        |rng, size| Plan {
            seed: seed(rng),
            size,
        },
        |plan| history_run(plan, &seen),
    );
    // The generator covers what the test claims.
    let (cases, branched) = (seen.cases.get(), seen.branched.get());
    assert!(
        branched * 5 >= cases,
        "{branched} of {cases} sequences branched"
    );
    let (long_groups, joins, refused) =
        (seen.long_groups.get(), seen.joins.get(), seen.refused.get());
    assert!(
        long_groups >= 4 && joins >= 20 && refused >= 5,
        "{long_groups} long groups, {joins} joins, {refused} refused commits"
    );
}

#[test]
fn ten_thousand_cursors_type_and_undo() {
    let mut text = String::with_capacity(450_000);
    let mut ends = Vec::with_capacity(10_000);
    for i in 0..10_000 {
        text.push_str(&format!("line {i:05}: the quick brown fox é 😀"));
        ends.push(text.len());
        text.push_str(if i % 7 == 3 { "\r\n" } else { "\n" });
    }
    let rope = Rope::from(text.as_str());
    let carets: Vec<Cursor> = ends.iter().map(|&at| Cursor::caret(at)).collect();
    // The primary is listed again among the others; equal carets merge.
    let selection = Selection::new(carets[5_000], carets.iter().copied());
    assert_eq!(
        (selection.cursors().len(), selection.primary_index()),
        (10_000, 5_000)
    );
    let x = Rope::from("x");
    let mut calls = 0;
    let tx = Transaction::change_each(&rope, &selection, |c| {
        calls += 1;
        Some((c.head..c.head, x.clone(), Place::End))
    })
    .expect("10,000 inserts");
    assert_eq!(calls, 10_000);
    let mut want = String::with_capacity(text.len() + 10_000);
    let mut from = 0;
    for &at in &ends {
        want.push_str(&text[from..at]);
        want.push('x');
        from = at;
    }
    want.push_str(&text[from..]);
    let after: Vec<Pair> = ends
        .iter()
        .enumerate()
        .map(|(i, &at)| (at + i + 1, at + i + 1))
        .collect();
    assert_eq!(pairs(tx.selection()), after);
    assert_eq!(tx.selection().primary_index(), 5_000);
    assert_eq!(tx.changes().changes().len(), 10_000);
    let shown_after = tx.selection().clone();
    let mut buffer = rope.clone();
    let mut history = History::new(&buffer);
    let step = history
        .commit(&mut buffer, tx, &selection, Instant::now(), typing(1))
        .expect("a commit");
    assert!(buffer == want, "the text after differs");
    assert_eq!(step.selection, shown_after);
    let undo = history.undo(&mut buffer).expect("an undo").expect("a step");
    assert!(buffer == text, "undo did not restore the text");
    assert_eq!(undo.selection, selection);
    let redo = history.redo(&mut buffer).expect("a redo").expect("a step");
    assert!(buffer == want, "redo did not make the text after");
    assert_eq!(redo.selection, shown_after);
    buffer.__check_invariants().expect("rope invariants");
    history.__check_invariants().expect("history invariants");
}

#[test]
fn select_all_delete_of_two_mebibytes() {
    let pattern = "fn main() {\r\n  \"😀 中 é\"\r}\n\r\n";
    let mut text = String::with_capacity((2 << 20) + pattern.len());
    while text.len() < 2 << 20 {
        text.push_str(pattern);
    }
    let rope = Rope::from(text.as_str());
    let all = Selection::single(Cursor::new(0, text.len()));
    let tx = Transaction::change_each(&rope, &all, |c| Some((c.range(), "", Place::Start)))
        .expect("one deletion");
    assert_eq!(
        tx.changes(),
        &ChangeSet::new(&rope, [(0..text.len(), "")]).expect("one deletion")
    );
    let only = tx.changes().changes().next().expect("one replacement");
    assert_eq!(
        (only.old, only.new, only.text.is_none()),
        (0..text.len(), 0..0, true)
    );
    let mut buffer = rope.clone();
    let mut history = History::new(&buffer);
    let step = history
        .commit(&mut buffer, tx, &all, Instant::now(), Group::New)
        .expect("a commit");
    assert!(buffer.is_empty() && step.changes.len() == 1);
    assert_eq!(step.selection, Selection::caret(0));
    let undo = history.undo(&mut buffer).expect("an undo").expect("a step");
    assert!(buffer == text, "undo did not restore the text");
    assert_eq!(
        (undo.changes.len(), undo.changes[0].changes().len()),
        (1, 1)
    );
    let back = undo.changes[0].changes().next().expect("one replacement");
    assert_eq!((back.old, back.new), (0..0, 0..text.len()));
    assert!(back.text.is_some_and(|t| *t == text));
    assert_eq!(undo.selection, all);
    history.redo(&mut buffer).expect("a redo").expect("a step");
    assert!(buffer.is_empty());
    history.undo(&mut buffer).expect("an undo").expect("a step");
    buffer.__check_invariants().expect("rope invariants");
    assert!(buffer == text);
}

/// Line breaks + 1, a CR LF counting once.
fn lines_in(text: &str) -> usize {
    text.matches('\n').count() + text.matches('\r').count() - text.matches("\r\n").count() + 1
}

/// Touching edits merge into one replacement whose text keeps their order:
/// 10,000 whole-line replacements of a 10,000-line text (with CR LF seams
/// between the new lines), and 100,000 one-byte inserts at one offset,
/// against `String` models. Each undoes exactly.
#[test]
fn many_touching_edits_merge_in_order() {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(10_001);
    for i in 0..10_000 {
        starts.push(text.len());
        text.push_str(&format!("line {i:05} é 😀"));
        text.push_str(if i % 7 == 3 { "\r\n" } else { "\n" });
    }
    starts.push(text.len());
    let lines: Vec<Edit> = starts
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            // A new line ending in a CR is followed by one starting with LF.
            let head = if i % 3 == 1 { "\n" } else { "" };
            let tail = if i % 3 == 0 { "\r" } else { "\n" };
            (w[0]..w[1], format!("{head}LINE {i} 中{tail}"))
        })
        .collect();
    // Between the CR and the LF of "01\r\n456789".
    let keys = ["a", "\r", "\n", "b", "\r"];
    let inserts: Vec<Edit> = (0..100_000)
        .map(|i| (3..3, keys[i % keys.len()].to_owned()))
        .collect();
    for (text, edits) in [(text.as_str(), lines), ("01\r\n456789", inserts)] {
        let rope = Rope::from(text);
        let reps = build(text, &edits).expect("valid edits");
        assert_eq!(reps.len(), 1);
        let after = apply_reps(text, &reps);
        let changes = ChangeSet::new(&rope, edits).expect("valid edits");
        same_changes("touching edits", &changes, &reps, (text.len(), after.len()))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut edited = applied(&changes, &rope).expect("applies");
        assert!(edited == after, "the text after differs");
        assert_eq!(edited.len_lines(), lines_in(&after));
        edited.__check_invariants().expect("rope invariants");
        let inverse = changes.invert(&rope).expect("the text it was made for");
        inverse.apply(&mut edited).expect("the text after");
        assert!(edited == text, "undo did not restore the text");
    }
}

/// `count` small edits at random points of `text`, sorted, or shuffled one
/// time in 4: each removes 0-3 bytes, never past the next edit's start,
/// and inserts 0-2 units.
fn scattered(rng: &mut Pcg32, text: &str, count: usize) -> Vec<Edit> {
    let mut starts: Vec<usize> = (0..count)
        .map(|_| floor(text, below(rng, text.len() + 1)))
        .collect();
    starts.sort_unstable();
    let mut edits: Vec<Edit> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let next = starts.get(i + 1).copied().unwrap_or(text.len());
            let end = floor(text, start + below(rng, 4)).min(next);
            let n = below(rng, 3);
            (start..end, units(rng, n))
        })
        .collect();
    if chance(rng, 25) {
        rng.shuffle(&mut edits);
    }
    edits
}

/// One sequence at real leaf sizes: a text of 50-400 KB (mixed widths and
/// CR LF) and one or two change sets of 80-600 small edits, enough that
/// the size rule rebuilds the text in one pass; each against the `String`
/// model through `ChangeSet::new` and `apply`, then committed, and all of
/// them undone and redone through a `History`. Counts the change sets for
/// which the rule (at least 64 replacements, and at most 4,096 bytes of the
/// text before and the new texts for each) picks the rebuild.
fn rebuilding(seed: u64, sets: &Cell<u32>, rebuilt: &Cell<u32>) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 9);
    let bytes = rng.range(50_000..400_001u32) as usize;
    let initial = big_text(&mut rng, bytes);
    let mut rope = Rope::from(initial.as_str());
    let mut history = History::new(&rope);
    let mut texts = vec![initial];
    for round in 0..rng.range(1..3u32) {
        let what = format!("change set {round}");
        let text = texts.last().expect("the first text").clone();
        let fewest = (text.len() / 3_000).max(80);
        let count = rng.range(fewest as u32..601) as usize;
        let edits = scattered(&mut rng, &text, count);
        let reps = build(&text, &edits).map_err(|e| format!("{what}: the model: {e}"))?;
        let after = apply_reps(&text, &reps);
        let changes =
            ChangeSet::new(&rope, edits).map_err(|e| format!("{what}: ChangeSet::new: {e}"))?;
        same_changes(&what, &changes, &reps, (text.len(), after.len()))?;
        bump(sets);
        let added: usize = reps.iter().map(|r| r.text.len()).sum();
        if reps.len() >= 64 && text.len() + added <= reps.len() * 4_096 {
            bump(rebuilt);
        }
        let edited = applied(&changes, &rope)?;
        if edited != *after {
            return Err(format!("{what}: apply made another text"));
        }
        edited
            .__check_invariants()
            .map_err(|e| format!("{what}: the rope after apply: {e}"))?;
        let tx = Transaction::new(changes.clone(), Selection::caret(0));
        let step = history
            .commit(
                &mut rope,
                tx,
                &Selection::caret(0),
                Instant::now(),
                Group::New,
            )
            .map_err(|e| format!("{what}: commit: {e}"))?;
        if rope != *after || step.before != *text || step.changes != [changes] {
            return Err(format!("{what}: the commit made another text or step"));
        }
        texts.push(after);
    }
    for (i, want) in texts.iter().enumerate().rev().skip(1) {
        history
            .undo(&mut rope)
            .map_err(|e| format!("undo to {i}: {e}"))?
            .ok_or_else(|| format!("undo to {i} did not move"))?;
        if rope != **want {
            return Err(format!("undo to {i} did not restore its text"));
        }
    }
    for (i, want) in texts.iter().enumerate().skip(1) {
        history
            .redo(&mut rope)
            .map_err(|e| format!("redo to {i}: {e}"))?
            .ok_or_else(|| format!("redo to {i} did not move"))?;
        if rope != **want {
            return Err(format!("redo to {i} did not make its text"));
        }
    }
    rope.__check_invariants()
        .map_err(|e| format!("the rope at the end: {e}"))?;
    history
        .__check_invariants()
        .map_err(|e| format!("history invariants: {e}"))
}

/// The rebuild path at real leaf sizes, against the `String` model; the
/// generator makes the size rule pick it for nearly every change set.
#[test]
fn many_small_edits_rebuild_large_texts_exactly() {
    let (sets, rebuilt) = (Cell::new(0), Cell::new(0));
    check(
        "many small edits rebuild large texts exactly",
        24,
        seed,
        |&seed| rebuilding(seed, &sets, &rebuilt),
    );
    let (sets, rebuilt) = (sets.get(), rebuilt.get());
    assert!(
        rebuilt * 10 >= sets * 9 && sets >= 24,
        "the rule rebuilds {rebuilt} of {sets} change sets"
    );
}
