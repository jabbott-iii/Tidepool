//! Unit tests of change sets and transactions in the tiny rope shape, and
//! the generators and models that the selection and history tests share.
//!
//! Each case's input is a [`Plan`], the seed of its own generator and its
//! size, so a failure report stays short. Errors are compared by their
//! `Debug` text (numbers only), because `TextError` has no `PartialEq`.

use std::cell::Cell;
use std::fmt::Debug;
use std::ops::Range;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};

use super::apply::{REBUILD_BYTES, REBUILD_MIN};
use super::paths::{self, Force};
use super::{Assoc, ChangeSet, MAX_LEN, Place, Replacement, Transaction, appends};
use crate::pos::tests::range_error;
use crate::rope::MAX_LEAF;
use crate::rope::tests::{broken_rope, floor_boundary, push_text, shared_leaves};
use crate::selection::{Cursor, Selection};
use crate::{Rope, TextError};

// ---------------------------------------------------------------------------
// Generators, shared with `selection::tests`.

/// One case: the seed of its own generator, and its size.
#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) seed: u64,
    pub(crate) size: u32,
}

impl Plan {
    /// A plan drawn from the harness's generator.
    pub(crate) fn draw(rng: &mut Pcg32, size: u32) -> Self {
        Self {
            seed: (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32()),
            size,
        }
    }

    /// The case's own generator.
    pub(crate) fn rng(&self) -> Pcg32 {
        Pcg32::new(self.seed, u64::from(self.size))
    }
}

/// `Ok` if both succeeded, or both failed with the same error (compared by
/// `Debug`). Formats only on failure.
pub(crate) fn same_outcome<A: Debug, B: Debug>(
    what: impl FnOnce() -> String,
    got: &Result<A, TextError>,
    want: &Result<B, TextError>,
) -> Result<(), String> {
    let same = match (got, want) {
        (Ok(_), Ok(_)) => true,
        (Err(a), Err(b)) => format!("{a:?}") == format!("{b:?}"),
        _ => false,
    };
    if same {
        Ok(())
    } else {
        Err(format!("{}: got {got:?}, want {want:?}", what()))
    }
}

/// Counts of the kinds of case a property met, so that it can show that its
/// generators reach each kind often enough.
pub(crate) struct Tally<const N: usize> {
    names: [&'static str; N],
    counts: Cell<[u32; N]>,
}

impl<const N: usize> Tally<N> {
    /// No cases yet, of the kinds `names`.
    pub(crate) fn new(names: [&'static str; N]) -> Self {
        Self {
            names,
            counts: Cell::new([0; N]),
        }
    }

    /// One more case of kind `kind`.
    pub(crate) fn add(&self, kind: usize) {
        let mut counts = self.counts.get();
        if let Some(count) = counts.get_mut(kind) {
            *count += 1;
        }
        self.counts.set(counts);
    }

    /// Panics unless every kind has at least its `min` cases.
    #[track_caller]
    pub(crate) fn require(&self, min: [u32; N]) {
        let counts = self.counts.get();
        let all: Vec<String> = self
            .names
            .iter()
            .zip(counts)
            .map(|(name, count)| format!("{name}: {count}"))
            .collect();
        assert!(
            counts.iter().zip(min).all(|(&count, min)| count >= min),
            "too few cases of some kind (want at least {min:?}): {}",
            all.join(", ")
        );
    }
}

/// A text of 1 to `max_units` generated units, or empty 10% of the time.
pub(crate) fn text_up_to(rng: &mut Pcg32, max_units: u32) -> String {
    let mut out = String::new();
    if rng.range(0..10u32) != 0 {
        let units = rng.range(1..max_units.max(1) + 1);
        push_text(rng, &mut out, units);
    }
    out
}

/// A text of 1-24 generated units, or empty 10% of the time.
pub(crate) fn text(rng: &mut Pcg32) -> String {
    text_up_to(rng, 24)
}

/// The text of one edit: empty 30% of the time, otherwise 1-6 units.
pub(crate) fn edit_text(rng: &mut Pcg32) -> String {
    let mut out = String::new();
    if rng.range(0..10u32) >= 3 {
        let units = rng.range(1..7u32);
        push_text(rng, &mut out, units);
    }
    out
}

/// The char boundaries of `text`, ascending.
pub(crate) fn boundaries(text: &str) -> Vec<usize> {
    (0..=text.len())
        .filter(|&at| text.is_char_boundary(at))
        .collect()
}

fn random_boundary(rng: &mut Pcg32, text: &str) -> usize {
    floor_boundary(text, rng.range(0..text.len() as u32 + 1) as usize)
}

/// A valid position in `text`: about 10% at 0, 10% at the end, 20% at or
/// 1-2 bytes around a CR or LF (snapped down to a boundary, so the CR|LF
/// offset comes up), and otherwise a random boundary.
pub(crate) fn boundary(rng: &mut Pcg32, text: &str) -> usize {
    match rng.range(0..95u32) {
        0..10 => 0,
        10..20 => text.len(),
        20..40 => {
            let breaks: Vec<usize> = text
                .bytes()
                .enumerate()
                .filter(|&(_, byte)| byte == b'\r' || byte == b'\n')
                .map(|(at, _)| at)
                .collect();
            match rng.choose(&breaks) {
                Some(&at) => {
                    let near = (at + rng.range(0..5u32) as usize).saturating_sub(2);
                    floor_boundary(text, near)
                }
                None => random_boundary(rng, text),
            }
        }
        _ => random_boundary(rng, text),
    }
}

/// A position in `text`: as [`boundary`] 95% of the time, otherwise raw,
/// possibly past the end or inside a char.
pub(crate) fn position(rng: &mut Pcg32, text: &str) -> usize {
    if rng.range(0..20u32) == 0 {
        rng.range(0..text.len() as u32 + 3) as usize
    } else {
        boundary(rng, text)
    }
}

/// An edit list for `text`, as [`ChangeSet::new`] takes it.
///
/// - 5% none; 5% 64-100 small edits; otherwise 1-8.
/// - Half drawn as sorted boundaries paired up, so edits touch, ranges are
///   empty, and two or three inserts fall at one point; half as independent
///   ranges, which may overlap. Long lists and `valid` lists are always
///   paired.
/// - 3% with an empty edit strictly inside another edit's range (not an
///   overlap); 30% shuffled.
/// - Unless `valid`, 5% with one corrupted edit: inverted, past the end, or
///   inside a char.
pub(crate) fn edits(rng: &mut Pcg32, text: &str, valid: bool) -> Vec<(Range<usize>, String)> {
    let count = match rng.range(0..100u32) {
        0..5 => 0,
        5..10 => rng.range(64..101u32),
        _ => rng.range(1..9u32),
    } as usize;
    edits_of(rng, text, valid, count)
}

/// An edit list of `count` edits for `text`, drawn as [`edits`] draws one
/// of that many.
fn edits_of(rng: &mut Pcg32, text: &str, valid: bool, count: usize) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::with_capacity(count + 1);
    if valid || count > 8 || rng.range(0..2u32) == 0 {
        let mut points: Vec<usize> = (0..2 * count).map(|_| boundary(rng, text)).collect();
        if count >= 2 && rng.range(0..3u32) == 0 {
            // Three to five equal points: inserts and ends at one offset.
            let copies = rng.range(2..5u32) as usize;
            let at = points[0];
            for point in points.iter_mut().skip(1).take(copies) {
                *point = at;
            }
        }
        points.sort_unstable();
        for &[start, end] in points.as_chunks::<2>().0 {
            out.push((start..end, edit_text(rng)));
        }
    } else {
        for _ in 0..count {
            let (a, b) = (boundary(rng, text), boundary(rng, text));
            out.push((a.min(b)..a.max(b), edit_text(rng)));
        }
    }
    if rng.range(0..100u32) < 3 {
        let inside: Vec<usize> = out
            .iter()
            .flat_map(|(range, _)| range.start + 1..range.end)
            .filter(|&at| text.is_char_boundary(at))
            .collect();
        if let Some(&at) = rng.choose(&inside) {
            out.push((at..at, String::new()));
        }
    }
    if rng.range(0..100u32) < 30 {
        rng.shuffle(&mut out);
    }
    if !valid && !out.is_empty() && rng.range(0..100u32) < 5 {
        corrupt(rng, text, &mut out);
    }
    out
}

/// Makes one edit of `edits` invalid: inverted, past the end, or inside a
/// char.
fn corrupt(rng: &mut Pcg32, text: &str, edits: &mut [(Range<usize>, String)]) {
    let index = rng.range(0..edits.len() as u32) as usize;
    let inside: Vec<usize> = (0..text.len())
        .filter(|&at| !text.is_char_boundary(at))
        .collect();
    let past = text.len() + rng.range(1..4u32) as usize;
    let range = &mut edits[index].0;
    match (rng.range(0..3u32), rng.choose(&inside)) {
        (0, _) => range.start = range.end + 1,
        (1, Some(&at)) if at <= range.end => range.start = at,
        (1, Some(&at)) => range.end = at,
        _ => range.end = past,
    }
}

/// Cursors on `text` and the index of the primary: 1-8 cursors (5% of the
/// time 64, at three colliding positions), 40% carets, random directions.
pub(crate) fn cursors(rng: &mut Pcg32, text: &str) -> (Vec<Cursor>, usize) {
    let crowd = rng.range(0..20u32) == 0;
    let count = if crowd { 64 } else { rng.range(1..9u32) };
    let spots: Vec<usize> = (0..3).map(|_| boundary(rng, text)).collect();
    let at = |rng: &mut Pcg32| {
        if crowd {
            rng.choose(&spots).copied().unwrap_or(0)
        } else {
            boundary(rng, text)
        }
    };
    let cursors: Vec<Cursor> = (0..count)
        .map(|_| {
            let anchor = at(rng);
            if rng.range(0..10u32) < 4 {
                Cursor::caret(anchor)
            } else {
                Cursor::new(anchor, at(rng))
            }
        })
        .collect();
    let primary = rng.range(0..count) as usize;
    (cursors, primary)
}

/// A selection valid on `text`, from [`cursors`].
pub(crate) fn selection(rng: &mut Pcg32, text: &str) -> Selection {
    let (cursors, primary) = cursors(rng, text);
    Selection::from_cursors(cursors, primary)
}

/// What `ChangeSet::new(text, edits)` must do, worked out on a `String`:
/// check every range in input order, drop the edits that do nothing, sort
/// stably by (start, end), refuse an overlap, then replace from the last
/// edit to the first. The text after, or the first error.
pub(crate) fn built(text: &str, edits: &[(Range<usize>, String)]) -> Result<String, TextError> {
    for (range, _) in edits {
        range_error(text, range.start, range.end)?;
    }
    let mut kept: Vec<&(Range<usize>, String)> = edits
        .iter()
        .filter(|(range, new)| range.start < range.end || !new.is_empty())
        .collect();
    kept.sort_by_key(|(range, _)| (range.start, range.end));
    for pair in kept.windows(2) {
        let (prev, next) = (&pair[0].0, &pair[1].0);
        if next.start < prev.end {
            return Err(TextError::OverlappingEdits {
                end: prev.end,
                start: next.start,
            });
        }
    }
    let mut out = text.to_string();
    for (range, new) in kept.iter().rev() {
        out.replace_range(range.clone(), new);
    }
    Ok(out)
}

/// The change set of `edits` on `text`.
pub(crate) fn build(text: &Rope, edits: &[(Range<usize>, String)]) -> Result<ChangeSet, TextError> {
    ChangeSet::new(
        text,
        edits
            .iter()
            .map(|(range, new)| (range.clone(), new.as_str())),
    )
}

/// A valid change set on `text`, from [`edits`], and the text after it
/// worked out on a `String`.
pub(crate) fn change_set(rng: &mut Pcg32, text: &str) -> (ChangeSet, String) {
    let edits = edits(rng, text, true);
    let after = built(text, &edits).expect("the edits are valid");
    let changes = build(&Rope::from(text), &edits).expect("the edits are valid");
    (changes, after)
}

/// `changes` applied to a clone of `text`, checked to hold `want`.
pub(crate) fn applied(changes: &ChangeSet, text: &Rope, want: &str) -> Result<Rope, String> {
    let mut out = text.clone();
    changes.apply(&mut out).map_err(|e| format!("apply: {e}"))?;
    out.__check_invariants()
        .map_err(|e| format!("the rope after apply: {e}"))?;
    if out != *want {
        return Err(format!(
            "the text after apply differs ({} bytes, want {})",
            out.len(),
            want.len()
        ));
    }
    Ok(out)
}

/// The new text of a change, as a `String`.
fn new_text(text: Option<&Rope>) -> String {
    text.map_or_else(String::new, Rope::to_string)
}

// ---------------------------------------------------------------------------
// Building.

/// The kinds of case `building` counts.
const BUILDING: [&str; 7] = [
    "built",
    "built with merged edits",
    "built from 64 or more edits",
    "InvertedRange",
    "ByteOutOfBounds",
    "NotCharBoundary",
    "OverlappingEdits",
];

fn building(plan: &Plan, tally: &Tally<7>) -> Result<(), String> {
    let mut rng = plan.rng();
    let text = text_up_to(&mut rng, 1 + plan.size * 23 / 100);
    let rope = Rope::from(text.as_str());
    let edits = edits(&mut rng, &text, false);
    let want = built(&text, &edits);
    let got = build(&rope, &edits);
    same_outcome(|| "ChangeSet::new".to_string(), &got, &want)?;
    tally.add(match &want {
        Ok(_) => 0,
        Err(TextError::InvertedRange { .. }) => 3,
        Err(TextError::ByteOutOfBounds { .. }) => 4,
        Err(TextError::NotCharBoundary { .. }) => 5,
        Err(_) => 6,
    });
    let (Ok(changes), Ok(after)) = (got, want) else {
        return Ok(());
    };
    let effective_count = edits
        .iter()
        .filter(|(range, new)| range.start < range.end || !new.is_empty())
        .count();
    if effective_count > changes.changes().len() {
        tally.add(1);
    }
    if edits.len() >= 64 {
        tally.add(2);
    }
    // C1-C5.
    changes.__check_invariants()?;
    let effective = edits
        .iter()
        .any(|(range, new)| range.start < range.end || !new.is_empty());
    if changes.len_before() != text.len() || changes.is_identity() == effective {
        return Err(format!(
            "len_before {} for {} bytes; is_identity {} with effective edits {effective}",
            changes.len_before(),
            text.len(),
            changes.is_identity()
        ));
    }
    // L1 and L2.
    applied(&changes, &rope, &after)?;
    if changes.len_after() != after.len() {
        return Err(format!(
            "L2: len_after is {}, want {}",
            changes.len_after(),
            after.len()
        ));
    }
    // L13: LSP order, and each new range holds its text.
    let mut lsp = text.clone();
    for change in changes.changes().rev() {
        lsp.replace_range(change.old.clone(), &new_text(change.text));
    }
    if lsp != after {
        return Err("L13: applying changes() last first gives another text".to_string());
    }
    for (index, change) in changes.changes().enumerate() {
        if after.get(change.new.clone()) != Some(new_text(change.text).as_str()) {
            return Err(format!(
                "L13: change {index}'s new range {:?} does not hold its text",
                change.new
            ));
        }
    }
    let (count, forward) = (changes.changes().len(), changes.changes().count());
    let backward: Vec<_> = changes.changes().rev().collect();
    let mut chain: Vec<_> = changes.changes().collect();
    chain.reverse();
    if count != forward || backward != chain {
        return Err("changes() is not exact-size and double-ended".to_string());
    }
    Ok(())
}

/// L1, L2, L13 and C1-C5, and the exact first error, against a `String`
/// model, for 3,000 edit lists in any order: touching edits, empty ranges,
/// inserts at one point, overlaps, corrupted edits and long lists.
#[test]
fn building_matches_a_string_model() {
    let tally = Tally::new(BUILDING);
    check_sized(
        "change sets build as a String model says",
        3_000,
        Plan::draw,
        |plan| building(plan, &tally),
    );
    tally.require([800, 500, 50, 20, 20, 12, 300]);
}

#[test]
fn building_fixed_cases() {
    let rope = Rope::from("abcdef");
    // Pure inserts at one offset keep their order and come before an edit
    // starting there; an edit ending there comes before them.
    let edits = [
        (2..2, "1"),
        (2..4, "X"),
        (2..2, "2"),
        (0..2, "<"),
        (4..4, ">"),
    ];
    let changes = ChangeSet::new(&rope, edits).expect("valid");
    let mut text = rope.clone();
    changes.apply(&mut text).expect("valid");
    assert_eq!(text, "<12X>ef");
    // Touching edits merge into one replacement.
    assert_eq!(changes.changes().len(), 1);
    // An empty edit inside a range is dropped, not an overlap.
    let changes = ChangeSet::new(&rope, [(1..5, ""), (3..3, "")]).expect("valid");
    assert_eq!(changes.changes().len(), 1);
    // An insert strictly inside a range is an overlap.
    let error = ChangeSet::new(&rope, [(1..5, ""), (3..3, "x")]).expect_err("overlap");
    assert!(matches!(
        error,
        TextError::OverlappingEdits { end: 5, start: 3 }
    ));
    // Validity comes before overlap, in input order.
    let error = ChangeSet::new(&rope, [(1..5, ""), (2..4, ""), (9..9, "")]).expect_err("bad");
    assert!(matches!(
        error,
        TextError::ByteOutOfBounds { byte: 9, len: 6 }
    ));
    let text = Rope::from("a😀b");
    let error = ChangeSet::new(&text, [(0..1, ""), (2..2, ""), (7..9, "")]).expect_err("bad");
    assert!(matches!(error, TextError::NotCharBoundary { byte: 2 }));
    // Nothing to do is the identity.
    let none = ChangeSet::new(&rope, [(3..3, "")]).expect("valid");
    assert!(none.is_identity());
    assert_eq!(none, ChangeSet::identity(6));
}

/// What `f` returns, and the appends that `Builder` made while it ran.
fn counting_appends<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let start = appends::read();
    let value = f();
    (value, appends::read() - start)
}

/// A run of touching edits keeps its texts in order and appends them once,
/// not once per edit; a later text over `MAX_LEAF` bytes stays shared and
/// costs at most two appends more. Against `String` models: 10,000 touching
/// one-byte replacements, 10,000 one-byte inserts at one offset, touching
/// edits with a large text every 1,000, 10,000 adjacent cursors typing over
/// their bytes, and a compose whose result is one run of 10,000 inserts.
#[test]
fn touching_edits_append_once() {
    const N: usize = 10_000;
    let text: String = (0..N).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let rope = Rope::from(text.as_str());
    let digit = |i: usize| char::from(b'0' + (i % 10) as u8).to_string();
    let digits: String = (0..N).map(digit).collect();
    let only = |changes: &ChangeSet| {
        changes
            .__check_invariants()
            .unwrap_or_else(|e| panic!("{e}"));
        let mut all = changes.changes();
        let first = all.next().expect("one replacement");
        assert!(
            all.next().is_none(),
            "{} replacements",
            changes.changes().len()
        );
        first.text.expect("a text").clone()
    };

    let lists: [Vec<(Range<usize>, String)>; 2] = [
        (0..N).map(|i| (i..i + 1, digit(i))).collect(),
        (0..N).map(|i| (5..5, digit(i))).collect(),
    ];
    for edits in lists {
        let (changes, made) = counting_appends(|| build(&rope, &edits).expect("valid"));
        let want = built(&text, &edits).expect("valid");
        applied(&changes, &rope, &want).unwrap_or_else(|e| panic!("{e}"));
        only(&changes);
        assert!(made <= 1, "{made} appends");
    }

    // A large text every 1,000 edits: shared, all but the leaves at its two
    // seams.
    let big = Rope::from("0123456789".repeat(MAX_LEAF).as_str());
    let mut edits: Vec<(Range<usize>, Rope)> = (0..N)
        .map(|i| (i..i + 1, Rope::from(digit(i).as_str())))
        .collect();
    for edit in edits.iter_mut().skip(500).step_by(1_000) {
        edit.1 = big.clone();
    }
    let large = N / 1_000;
    let (changes, made) = counting_appends(|| ChangeSet::new(&rope, edits.clone()).expect("valid"));
    let want: String = edits.iter().map(|(_, new)| new.to_string()).collect();
    applied(&changes, &rope, &want).unwrap_or_else(|e| panic!("{e}"));
    let (shared, leaves) = shared_leaves(&big, &only(&changes));
    assert!(made <= 1 + 2 * large as u64, "{made} appends");
    assert!(
        shared >= large * (leaves - 2),
        "{shared} leaves shared, of {large} texts of {leaves}"
    );

    // Adjacent ranges stay separate cursors; their edits make one run.
    let selection = Selection::new(Cursor::new(0, 1), (1..N).map(|i| Cursor::new(i, i + 1)));
    assert_eq!(selection.cursors().len(), N);
    let (tx, made) = counting_appends(|| {
        Transaction::change_each(&rope, &selection, |c| {
            Some((c.range(), digit(c.start()), Place::End))
        })
        .expect("valid")
    });
    applied(tx.changes(), &rope, &digits).unwrap_or_else(|e| panic!("{e}"));
    only(tx.changes());
    let carets: Vec<Cursor> = (1..=N).map(Cursor::caret).collect();
    assert_eq!(tx.selection().cursors(), carets.as_slice());
    assert!(made <= 1, "{made} appends");

    // Separate inserts, then deleting every byte between them: one run.
    let inserts: Vec<(Range<usize>, String)> = (0..N).map(|i| (i..i, digit(i))).collect();
    let a = build(&rope, &inserts).expect("valid");
    let deletes: Vec<(Range<usize>, String)> = (0..N)
        .map(|i| (2 * i + 1..2 * i + 2, String::new()))
        .collect();
    let b = build(
        &Rope::from(built(&text, &inserts).expect("valid").as_str()),
        &deletes,
    )
    .expect("valid");
    let (composed, made) = counting_appends(|| a.compose(&b).expect("they chain"));
    applied(&composed, &rope, &digits).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(only(&composed), digits.as_str());
    assert!(made <= 1, "{made} appends");
}

// ---------------------------------------------------------------------------
// The laws.

/// The bytes of `text` from `from` up to the next replacement's start (or
/// the end): the kept run after `from`.
fn kept_run(changes: &ChangeSet, text: &str, from: usize) -> Range<usize> {
    let until = changes
        .changes()
        .map(|change| change.old.start)
        .find(|&start| start > from)
        .unwrap_or(text.len());
    from..until
}

/// Whether `pos` is in a closed old range `[start, end]` of `changes`.
fn touches(changes: &ChangeSet, pos: usize) -> bool {
    changes
        .changes()
        .any(|change| change.old.start <= pos && pos <= change.old.end)
}

/// The kinds of case `laws` counts.
const LAWS: [&str; 6] = [
    "a changes something",
    "L11 checked at some position",
    "L12 checked at some position",
    "apply refused inside a char",
    "compose refused inside an inserted char",
    "compose with a cutting change set allowed",
];

fn laws(plan: &Plan, tally: &Tally<6>) -> Result<(), String> {
    let mut rng = plan.rng();
    let t = text(&mut rng);
    let rope_t = Rope::from(t.as_str());
    let (a, at) = change_set(&mut rng, &t);
    let (b, bt) = change_set(&mut rng, &at);
    let (c, ct) = change_set(&mut rng, &bt);
    let rope_at = applied(&a, &rope_t, &at)?;
    let rope_bt = applied(&b, &rope_at, &bt)?;
    applied(&c, &rope_bt, &ct)?;
    let fail = |law: &str, e: TextError| format!("{law}: {e}");

    // L3: invert undoes.
    let a_inv = a.invert(&rope_t).map_err(|e| fail("L3 invert", e))?;
    a_inv.__check_invariants()?;
    applied(&a_inv, &rope_at, &t).map_err(|e| format!("L3: {e}"))?;
    // L4: involution.
    if a_inv.invert(&rope_at).map_err(|e| fail("L4", e))? != a {
        return Err("L4: inverting twice gives another change set".to_string());
    }
    // L5: compose applies both.
    let ab = a.compose(&b).map_err(|e| fail("L5", e))?;
    ab.__check_invariants()?;
    applied(&ab, &rope_t, &bt).map_err(|e| format!("L5: {e}"))?;
    // L6: the identity is the unit.
    let left = ChangeSet::identity(a.len_before()).compose(&a);
    let right = a.compose(&ChangeSet::identity(a.len_after()));
    if left.as_ref().ok() != Some(&a) || right.as_ref().ok() != Some(&a) {
        return Err("L6: composing with the identity changes the change set".to_string());
    }
    // L7: associativity.
    let ab_c = ab.compose(&c).map_err(|e| fail("L7", e))?;
    let bc = b.compose(&c).map_err(|e| fail("L7", e))?;
    let a_bc = a.compose(&bc).map_err(|e| fail("L7", e))?;
    if ab_c != a_bc {
        return Err("L7: (a∘b)∘c differs from a∘(b∘c)".to_string());
    }
    applied(&ab_c, &rope_t, &ct).map_err(|e| format!("L7: {e}"))?;
    // L8: the inverse of a composition.
    let ab_inv = ab.invert(&rope_t).map_err(|e| fail("L8", e))?;
    let b_inv = b.invert(&rope_at).map_err(|e| fail("L8", e))?;
    if b_inv.compose(&a_inv).map_err(|e| fail("L8", e))? != ab_inv {
        return Err("L8: (a∘b)⁻¹ differs from b⁻¹∘a⁻¹".to_string());
    }
    // L9: a change and its inverse cancel, in effect.
    let cancel = a.compose(&a_inv).map_err(|e| fail("L9", e))?;
    applied(&cancel, &rope_t, &t).map_err(|e| format!("L9: {e}"))?;

    if !a.is_identity() {
        tally.add(0);
    }
    map_laws(&t, &at, &a, &a_inv, &b, &ab, tally)?;
    error_laws(&mut rng, &t, &rope_t, &at, &a, tally)?;
    Ok(())
}

/// L10-L12 at every boundary of `t`, with both `Assoc`s.
fn map_laws(
    t: &str,
    at: &str,
    a: &ChangeSet,
    a_inv: &ChangeSet,
    b: &ChangeSet,
    ab: &ChangeSet,
    tally: &Tally<6>,
) -> Result<(), String> {
    let (mut l11, mut l12) = (false, false);
    let identity = ChangeSet::identity(t.len());
    let ends = [
        a.map(0, Assoc::Before).ok(),
        a.map(t.len(), Assoc::After).ok(),
    ];
    if ends != [Some(0), Some(at.len())] {
        return Err(format!(
            "L10: the ends map to {ends:?}, want [0, {}]",
            at.len()
        ));
    }
    let mut last = [0, 0];
    for p in boundaries(t) {
        let mut mapped = [0, 0];
        for (slot, assoc) in [Assoc::Before, Assoc::After].into_iter().enumerate() {
            let q = a.map(p, assoc).map_err(|e| format!("L10: map({p}): {e}"))?;
            if identity.map(p, assoc).ok() != Some(p) {
                return Err(format!("L10: the identity moves {p}"));
            }
            if !at.is_char_boundary(q) || q < last[slot] {
                return Err(format!(
                    "L10: map({p}, {assoc:?}) = {q}: not a boundary, or below {}",
                    last[slot]
                ));
            }
            mapped[slot] = q;
            if !touches(a, p) {
                l11 = true;
                // L11: kept text maps back and is found where p maps.
                let back = a_inv.map(q, assoc).map_err(|e| format!("L11: {e}"))?;
                let run = kept_run(a, t, p);
                let found = at.get(q..q + run.len());
                if back != p || found != t.get(run) {
                    return Err(format!(
                        "L11: {p} maps to {q} and back to {back}; the kept text after it moved"
                    ));
                }
            }
            if !touches(ab, p) {
                l12 = true;
                // L12: away from its replacements a composition maps as
                // its parts do.
                let composed = ab.map(p, assoc).map_err(|e| format!("L12: {e}"))?;
                let stepped = b.map(q, assoc).map_err(|e| format!("L12: {e}"))?;
                if composed != stepped {
                    return Err(format!(
                        "L12: {p} maps to {composed} through a∘b, {stepped} through a then b"
                    ));
                }
            }
        }
        if mapped[0] > mapped[1] {
            return Err(format!("L10: {p} maps Before to {mapped:?}, past After"));
        }
        last = mapped;
    }
    if l11 {
        tally.add(1);
    }
    if l12 {
        tally.add(2);
    }
    let past = a.map(t.len() + 1, Assoc::After);
    let want: Result<usize, _> = Err(TextError::ByteOutOfBounds {
        byte: t.len() + 1,
        len: t.len(),
    });
    same_outcome(|| "map past the end".to_string(), &past, &want)
}

/// `apply` and `invert` on texts that do not fit, and `compose` with change
/// sets that do not fit: the exact error, and nothing changed.
fn error_laws(
    rng: &mut Pcg32,
    t: &str,
    rope_t: &Rope,
    at: &str,
    a: &ChangeSet,
    tally: &Tally<6>,
) -> Result<(), String> {
    // A wrong length.
    let mut longer = rope_t.clone();
    longer.append(Rope::from("x"));
    // The same length, with a's offsets now likely inside chars.
    let mut wide = "é".repeat(t.len() / 2);
    if t.len() % 2 == 1 {
        wide.push('a');
    }
    let wide_rope = Rope::from(wide.as_str());
    let mid_char = a
        .changes()
        .flat_map(|change| [change.old.start, change.old.end])
        .find(|&byte| !wide.is_char_boundary(byte));
    if mid_char.is_some() {
        tally.add(3);
    }
    let cases = [
        (
            longer,
            Err(TextError::LengthMismatch {
                expected: t.len(),
                actual: t.len() + 1,
            }),
        ),
        (
            wide_rope,
            match mid_char {
                Some(byte) => Err(TextError::NotCharBoundary { byte }),
                None => Ok(()),
            },
        ),
    ];
    for (rope, want) in cases {
        let mut edited = rope.clone();
        let got = a.apply(&mut edited);
        same_outcome(
            || "apply on a text that does not fit".to_string(),
            &got,
            &want,
        )?;
        if got.is_err() && !edited.same_tree(&rope) {
            return Err("a refused apply changed the rope".to_string());
        }
        same_outcome(
            || "invert on a text that does not fit".to_string(),
            &a.invert(&rope),
            &want,
        )?;
    }

    // Compose: a length mismatch, and a change set made on an ASCII text of
    // a(T)'s length that may cut a char `a` inserted.
    let wrong = a.compose(&ChangeSet::identity(at.len() + 1));
    let want: Result<(), _> = Err(TextError::LengthMismatch {
        expected: at.len() + 1,
        actual: at.len(),
    });
    same_outcome(|| "compose with a wrong length".to_string(), &wrong, &want)?;
    let ascii = "x".repeat(at.len());
    let cutting = build(&Rope::from(ascii.as_str()), &edits(rng, &ascii, true))
        .map_err(|e| format!("a change set on the ASCII text: {e}"))?;
    let inserted: Vec<Range<usize>> = a.changes().map(|change| change.new).collect();
    let cut = cutting
        .changes()
        .flat_map(|change| [change.old.start, change.old.end])
        .filter(|&byte| {
            !at.is_char_boundary(byte)
                && inserted
                    .iter()
                    .any(|range| range.start < byte && byte < range.end)
        })
        .min();
    let want = match cut {
        Some(byte) => Err(TextError::NotCharBoundary { byte }),
        None => Ok(()),
    };
    tally.add(if cut.is_some() { 4 } else { 5 });
    same_outcome(
        || "compose with a change set that cuts an inserted char".to_string(),
        &a.compose(&cutting),
        &want,
    )
}

/// L3-L12 on chains of valid change sets (a on T, b on a(T), c on b(a(T))),
/// at every boundary with both `Assoc`s; and the exact errors of `apply`,
/// `invert` and `compose` on texts and change sets that do not fit.
#[test]
fn change_sets_obey_the_laws() {
    let tally = Tally::new(LAWS);
    check(
        "change sets obey the laws",
        2_000,
        |rng| Plan::draw(rng, 100),
        |plan| laws(plan, &tally),
    );
    tally.require([800, 600, 450, 500, 400, 350]);
}

// ---------------------------------------------------------------------------
// The two ways to apply.

/// The kinds of case `paths_agree` counts.
const PATHS: [&str; 5] = [
    "64 or more replacements",
    "Auto replaced",
    "Auto rebuilt",
    "LengthMismatch",
    "NotCharBoundary",
];

/// `changes` applied to a clone of `target` with the path forced to
/// `force`: the outcome, the rope after, and how many times each path
/// (replace, rebuild) was taken.
fn apply_forced(
    changes: &ChangeSet,
    target: &Rope,
    force: Force,
) -> (Result<(), TextError>, Rope, [u64; 2]) {
    let before = paths::read();
    paths::force(force);
    let mut rope = target.clone();
    let result = changes.apply(&mut rope);
    paths::force(Force::Auto);
    let after = paths::read();
    (result, rope, [after[0] - before[0], after[1] - before[1]])
}

fn paths_agree(plan: &Plan, tally: &Tally<5>) -> Result<(), String> {
    let mut rng = plan.rng();
    let (t, edits) = if rng.range(0..2u32) == 0 {
        // The many-small-edits case, on texts long enough to keep 64 or
        // more replacements apart.
        let mut t = String::new();
        let units = rng.range(1_000..2_000u32);
        push_text(&mut rng, &mut t, units);
        let count = rng.range(64..101u32) as usize;
        let edits = edits_of(&mut rng, &t, true, count);
        (t, edits)
    } else {
        let t = text(&mut rng);
        let edits = edits(&mut rng, &t, true);
        (t, edits)
    };
    let rope = Rope::from(t.as_str());
    let after = built(&t, &edits).map_err(|e| format!("the model: {e}"))?;
    let changes = build(&rope, &edits).map_err(|e| format!("ChangeSet::new: {e}"))?;
    let count = changes.changes().len();
    if count >= REBUILD_MIN {
        tally.add(0);
    }
    // The rule, worked out here: at least REBUILD_MIN replacements, with at
    // most REBUILD_BYTES bytes of the text and the new texts for each.
    let added: usize = changes.changes().map(|change| change.new.len()).sum();
    let rebuilds = count >= REBUILD_MIN && t.len() + added <= count * REBUILD_BYTES;
    // The text, one a byte longer, and one of the same length with wider
    // chars, where the offsets are likely inside chars.
    let mut longer = rope.clone();
    longer.append(Rope::from("x"));
    let mut wide = "é".repeat(t.len() / 2);
    if t.len() % 2 == 1 {
        wide.push('a');
    }
    let targets = [
        (rope, Some(after)),
        (longer, None),
        (Rope::from(wide.as_str()), None),
    ];
    for (index, (target, want)) in targets.iter().enumerate() {
        let replaced = apply_forced(&changes, target, Force::Replace);
        let rebuilt = apply_forced(&changes, target, Force::Rebuild);
        let auto = apply_forced(&changes, target, Force::Auto);
        same_outcome(
            || format!("target {index}: Rebuild against Replace"),
            &rebuilt.0,
            &replaced.0,
        )?;
        same_outcome(
            || format!("target {index}: Auto against Replace"),
            &auto.0,
            &replaced.0,
        )?;
        let picked = if rebuilds { [0, 1] } else { [1, 0] };
        let runs = [
            ("Replace", &replaced, [1, 0]),
            ("Rebuild", &rebuilt, [0, 1]),
            ("Auto", &auto, picked),
        ];
        for (name, (result, edited, counts), path) in runs {
            let at = format!("target {index}, {name}");
            if result.is_err() {
                if !edited.same_tree(target) || *counts != [0, 0] {
                    return Err(format!(
                        "{at}: a refused apply changed the rope or took a path"
                    ));
                }
                continue;
            }
            edited
                .__check_invariants()
                .map_err(|e| format!("{at}: the rope after: {e}"))?;
            if *counts != path {
                return Err(format!("{at}: took the paths {counts:?}, want {path:?}"));
            }
            if *edited != replaced.1 || want.as_ref().is_some_and(|want| edited != want) {
                return Err(format!(
                    "{at}: the text after differs ({} bytes, Replace gave {})",
                    edited.len(),
                    replaced.1.len()
                ));
            }
        }
        match &replaced.0 {
            Ok(()) if index == 0 => tally.add(if rebuilds { 2 } else { 1 }),
            Ok(()) => {}
            Err(_) if index == 0 => return Err(format!("{:?} on its own text", replaced.0)),
            Err(TextError::LengthMismatch { .. }) => tally.add(3),
            Err(_) => tally.add(4),
        }
    }
    Ok(())
}

/// Forced to replace each range and forced to rebuild the text, `apply`
/// gives the same text or the same first error, on 1,000 change sets (half
/// of them from 64-100 small edits) and on texts that do not fit; the rule
/// picks the path it states, and the counters show which path ran. Then, at
/// the threshold's edge, the rule picks each side.
#[test]
fn both_apply_paths_agree() {
    let tally = Tally::new(PATHS);
    let start = paths::read();
    check(
        "both apply paths agree",
        1_000,
        |rng| Plan::draw(rng, 100),
        |plan| paths_agree(plan, &tally),
    );
    let end = paths::read();
    assert!(
        end[0] - start[0] >= 1_000 && end[1] - start[1] >= 1_000,
        "the paths ran {:?} times",
        [end[0] - start[0], end[1] - start[1]]
    );
    tally.require([80, 300, 80, 1_000, 300]);

    // `count` one-byte inserts into `len` bytes, applied with the path the
    // rule picks: how many times each path ran.
    let edge = |len: usize, count: usize| {
        let text = "a".repeat(len);
        let rope = Rope::from(text.as_str());
        let step = len / count;
        let edits: Vec<(Range<usize>, String)> = (0..count)
            .map(|i| (i * step + 1..i * step + 1, "x".to_string()))
            .collect();
        let changes = build(&rope, &edits).expect("valid");
        assert_eq!(changes.changes().len(), count);
        let (result, edited, counts) = apply_forced(&changes, &rope, Force::Auto);
        assert!(result.is_ok() && edited == built(&text, &edits).expect("valid"));
        counts
    };
    let edge_len = REBUILD_MIN * REBUILD_BYTES;
    // The text and the new texts come to exactly REBUILD_BYTES for each of
    // REBUILD_MIN replacements: rebuilt. One byte more: replaced.
    assert_eq!(edge(edge_len - REBUILD_MIN, REBUILD_MIN), [0, 1]);
    assert_eq!(edge(edge_len - REBUILD_MIN + 1, REBUILD_MIN), [1, 0]);
    // Fewer than REBUILD_MIN replacements, however small: replaced.
    assert_eq!(edge(4 * REBUILD_MIN, REBUILD_MIN - 1), [1, 0]);
}

#[test]
fn inverting_a_whole_deletion_shares_the_text() {
    let rope = Rope::from("0123456789".repeat(40).as_str());
    let delete = ChangeSet::new(&rope, [(0..rope.len(), "")]).expect("valid");
    let inverse = delete.invert(&rope).expect("valid");
    let restored = inverse.changes().next().and_then(|change| change.text);
    assert!(restored.is_some_and(|text| text.same_tree(&rope)));
    let mut text = rope.clone();
    delete.apply(&mut text).expect("valid");
    assert!(text.is_empty());
    inverse.apply(&mut text).expect("valid");
    assert!(text.same_tree(&rope), "undoing shares the whole tree");
}

// ---------------------------------------------------------------------------
// `change_each`.

/// One cursor's answer to the `change_each` closure.
pub(crate) type Request = Option<(Range<usize>, String, Place)>;

/// What `change_each` gives, in the model's terms: the text after, the
/// selection after, and the kept edits.
type Placed = (String, Selection, Vec<(Range<usize>, String)>);

/// How a cursor edits: what range (see [`request`]), with what text, and
/// where the cursor goes.
pub(crate) struct Style {
    kind: u32,
    /// How far a backspace or a span around the head reaches.
    reach: usize,
    new: String,
    place: Place,
}

pub(crate) fn style(rng: &mut Pcg32) -> Style {
    let place = match rng.range(0..4u32) {
        0 => Place::Map,
        1 => Place::Start,
        2 => Place::End,
        _ => Place::Select,
    };
    Style {
        kind: rng.range(0..100u32),
        reach: rng.range(1..6u32) as usize,
        new: edit_text(rng),
        place,
    }
}

/// The request of `cursor` on `text` in `style`: none, nothing, or an edit
/// of its own range, back from its head, around its head, of its line, of a
/// shared point, or of a random (possibly overlapping or invalid) range.
pub(crate) fn request(rng: &mut Pcg32, text: &str, cursor: Cursor, style: &Style) -> Request {
    let head = cursor.head;
    let range = match style.kind {
        0..20 => return None,
        20..25 => return Some((head..head, String::new(), style.place)),
        25..45 => cursor.range(),
        45..55 => floor_boundary(text, head.saturating_sub(style.reach))..head,
        55..62 => {
            let start = floor_boundary(text, head.saturating_sub(style.reach));
            start..text.ceil_char_boundary(head + style.reach)
        }
        62..75 => {
            let start = text[..head].rfind('\n').map_or(0, |at| at + 1);
            let end = text[head..].find('\n').map_or(text.len(), |at| head + at);
            start..end
        }
        75..82 => return Some((0..0, "x".to_string(), style.place)),
        _ => {
            let (a, b) = (position(rng, text), position(rng, text));
            a.min(b)..a.max(b)
        }
    };
    Some((range, style.new.clone(), style.place))
}

/// Where the model's unmerged edits put `pos`, by scanning them all.
fn seam_map(kept: &[(Range<usize>, String, usize)], pos: usize, assoc: Assoc) -> usize {
    let pick = |before, after| match assoc {
        Assoc::Before => before,
        Assoc::After => after,
    };
    if let Some((_, new, start)) = kept
        .iter()
        .find(|(range, _, _)| range.start < pos && pos < range.end)
    {
        return pick(*start, start + new.len());
    }
    let mut before = pos as isize;
    let mut inserts = 0;
    for (range, new, _) in kept {
        if range.start == pos && range.end == pos {
            inserts += new.len();
        } else if range.end <= pos {
            before += new.len() as isize - range.len() as isize;
        }
    }
    let before = before as usize;
    pick(before, before + inserts)
}

/// What `change_each` must give, worked out on `String`s, scanning every
/// edit for every position: the text after, the selection, and the kept
/// edits.
pub(crate) fn each_model(
    text: &str,
    selection: &Selection,
    requests: &[Request],
) -> Result<Placed, TextError> {
    for (range, _, _) in requests.iter().flatten() {
        range_error(text, range.start, range.end)?;
    }
    let mut edits: Vec<(usize, Range<usize>, String)> = requests
        .iter()
        .enumerate()
        .filter_map(|(cursor, request)| {
            let (range, new, _) = request.as_ref()?;
            (!range.is_empty() || !new.is_empty()).then(|| (cursor, range.clone(), new.clone()))
        })
        .collect();
    edits.sort_by_key(|(_, range, _)| (range.start, range.end));
    let mut kept: Vec<(Range<usize>, String, usize)> = Vec::new();
    let mut owner = vec![None; requests.len()];
    for (cursor, range, new) in edits {
        match kept.last_mut() {
            Some((prev, text, _)) if *prev == range && *text == new => {}
            Some((prev, text, _)) if range.start < prev.end => {
                if !(text.is_empty() && new.is_empty()) {
                    return Err(TextError::OverlappingEdits {
                        end: prev.end,
                        start: range.start,
                    });
                }
                prev.end = prev.end.max(range.end);
            }
            _ => kept.push((range, new, 0)),
        }
        owner[cursor] = Some(kept.len() - 1);
    }
    let mut shift = 0isize;
    for (range, new, start) in &mut kept {
        *start = (range.start as isize + shift) as usize;
        shift += new.len() as isize - range.len() as isize;
    }
    let placed: Vec<Cursor> = selection
        .cursors()
        .iter()
        .enumerate()
        .map(|(index, &cursor)| {
            let own = owner[index].map(|j| (kept[j].2, kept[j].2 + kept[j].1.len()));
            let place = requests[index].as_ref().map(|(_, _, place)| *place);
            match (own, place) {
                (Some((start, _)), Some(Place::Start)) => Cursor::caret(start),
                (Some((_, end)), Some(Place::End)) => Cursor::caret(end),
                (Some((start, end)), Some(Place::Select)) => Cursor::new(start, end),
                _ if cursor.is_caret() => Cursor::caret(seam_map(&kept, cursor.head, Assoc::After)),
                _ => {
                    let start = seam_map(&kept, cursor.start(), Assoc::After);
                    let end = seam_map(&kept, cursor.end(), Assoc::Before);
                    if start >= end {
                        Cursor::caret(start)
                    } else if cursor.is_backward() {
                        Cursor::new(end, start)
                    } else {
                        Cursor::new(start, end)
                    }
                }
            }
        })
        .collect();
    let primary = selection.primary_index();
    let others = placed
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != primary)
        .map(|(_, &cursor)| cursor);
    let after_selection = Selection::new(placed[primary], others);
    let mut after = text.to_string();
    for (range, new, _) in kept.iter().rev() {
        after.replace_range(range.clone(), new);
    }
    let kept = kept
        .into_iter()
        .map(|(range, new, _)| (range, new))
        .collect();
    Ok((after, after_selection, kept))
}

/// Runs `change_each` with `requests`, checking that the closure sees every
/// cursor once, in order.
pub(crate) fn run_each(
    rope: &Rope,
    selection: &Selection,
    requests: &[Request],
) -> Result<Result<Transaction, TextError>, String> {
    let mut calls = 0;
    let mut wrong = None;
    let got = Transaction::change_each(rope, selection, |cursor| {
        if selection.cursors().get(calls) != Some(&cursor) {
            wrong = Some(calls);
        }
        let request = requests.get(calls).cloned().flatten();
        calls += 1;
        request
    });
    if let Some(call) = wrong {
        return Err(format!("call {call} saw another cursor"));
    }
    if got.is_ok() && calls != selection.cursors().len() {
        return Err(format!(
            "{calls} calls for {} cursors",
            selection.cursors().len()
        ));
    }
    Ok(got)
}

/// The kinds of case `placing` counts.
const PLACING: [&str; 5] = [
    "placed",
    "placed with edits that collapsed or united",
    "placed with touching edits",
    "OverlappingEdits",
    "a bad range",
];

fn placing(plan: &Plan, tally: &Tally<5>) -> Result<(), String> {
    let mut rng = plan.rng();
    let text = text(&mut rng);
    let rope = Rope::from(text.as_str());
    let selection = selection(&mut rng, &text);
    // A quarter of the cases edit every cursor the same way, as one
    // multi-cursor command does, and half of those delete.
    let shared = (rng.range(0..4u32) == 0).then(|| {
        let mut shared = style(&mut rng);
        if rng.range(0..2u32) == 0 {
            shared.new.clear();
        }
        shared
    });
    let requests: Vec<Request> = selection
        .cursors()
        .iter()
        .map(|&cursor| match &shared {
            Some(shared) => request(&mut rng, &text, cursor, shared),
            None => {
                let own = style(&mut rng);
                request(&mut rng, &text, cursor, &own)
            }
        })
        .collect();
    let want = each_model(&text, &selection, &requests);
    let got = run_each(&rope, &selection, &requests)?;
    same_outcome(|| "change_each".to_string(), &got, &want)?;
    tally.add(match &want {
        Ok(_) => 0,
        Err(TextError::OverlappingEdits { .. }) => 3,
        Err(_) => 4,
    });
    let (Ok(transaction), Ok((after, after_selection, kept))) = (got, want) else {
        return Ok(());
    };
    let effective = requests
        .iter()
        .flatten()
        .filter(|(range, new, _)| !range.is_empty() || !new.is_empty())
        .count();
    if kept.len() < effective {
        tally.add(1);
    }
    if kept.windows(2).any(|pair| pair[0].0.end == pair[1].0.start) {
        tally.add(2);
    }
    let (changes, placed) = transaction.into_parts();
    changes.__check_invariants()?;
    placed.__check_invariants()?;
    let after_rope = applied(&changes, &rope, &after)?;
    if placed != after_selection {
        return Err(format!(
            "the selection after is {placed:?}, want {after_selection:?}"
        ));
    }
    placed
        .validate(&after_rope)
        .map_err(|e| format!("the selection after is not valid on the text after: {e}"))?;
    let canonical = build(&rope, &kept).map_err(|e| format!("the kept edits: {e}"))?;
    if canonical != changes {
        return Err("the change set differs from ChangeSet::new on the kept edits".to_string());
    }
    Ok(())
}

/// `change_each` against a `String` model of ADR-0020's rules for 1,500
/// random selections and per-cursor edits (own ranges, backspaces, whole
/// lines, a shared point, random and invalid ranges), then fixed cases.
#[test]
fn change_each_places_cursors() {
    let tally = Tally::new(PLACING);
    check(
        "change_each places cursors as the model says",
        1_500,
        |rng| Plan::draw(rng, 100),
        |plan| placing(plan, &tally),
    );
    tally.require([550, 15, 20, 35, 6]);

    let each = |text: &str, cursors: &[Cursor], edit: &dyn Fn(Cursor) -> Request| {
        let selection = Selection::new(cursors[0], cursors[1..].iter().copied());
        Transaction::change_each(&Rope::from(text), &selection, edit)
    };
    let ranges = |transaction: &Transaction| -> Vec<(usize, usize)> {
        let cursors = transaction.selection().cursors();
        cursors.iter().map(|c| (c.anchor, c.head)).collect()
    };
    let text_after = |text: &str, transaction: &Transaction| {
        let mut rope = Rope::from(text);
        transaction.changes().apply(&mut rope).expect("valid");
        rope.to_string()
    };

    // All four places, for carets at 3 and 7 inserting "XY".
    let carets = [Cursor::caret(3), Cursor::caret(7)];
    for (place, want) in [
        (Place::Map, vec![(5, 5), (11, 11)]),
        (Place::Start, vec![(3, 3), (9, 9)]),
        (Place::End, vec![(5, 5), (11, 11)]),
        (Place::Select, vec![(3, 5), (9, 11)]),
    ] {
        let insert = |c: Cursor| Some((c.head..c.head, "XY".to_string(), place));
        let transaction = each("abc def", &carets, &insert).expect("valid");
        assert_eq!(ranges(&transaction), want, "{place:?}");
        assert_eq!(text_after("abc def", &transaction), "abcXY defXY");
    }

    // None and edits that do nothing are mapped.
    let cursors = [Cursor::new(1, 2), Cursor::caret(4), Cursor::new(6, 5)];
    let edit = |c: Cursor| match c.head {
        5 => Some((0..0, ">>".to_string(), Place::Start)),
        4 => Some((4..4, String::new(), Place::Select)),
        _ => None,
    };
    let transaction = each("abcdefg", &cursors, &edit).expect("valid");
    assert_eq!(ranges(&transaction), [(0, 0), (3, 4), (6, 6)]);

    // A seam between touching edits stays a seam with `Map`.
    let upper = |c: Cursor| Some((c.range(), "AA".to_string(), Place::Map));
    let halves = [Cursor::new(0, 2), Cursor::new(2, 4)];
    let transaction = each("aaaa", &halves, &upper).expect("valid");
    assert_eq!(ranges(&transaction), [(0, 2), (2, 4)]);
    assert_eq!(transaction.changes().changes().len(), 1);

    // Edits that merge keep each cursor's own range.
    let grow = |c: Cursor| {
        let new = if c.start() == 0 { "XX" } else { "YYY" };
        Some((c.range(), new.to_string(), Place::Select))
    };
    let transaction = each("abcd", &halves, &grow).expect("valid");
    assert_eq!(ranges(&transaction), [(0, 2), (2, 5)]);
    assert_eq!(text_after("abcd", &transaction), "XXYYY");

    // Carets at 2 and 4 both deleting back to 0 give one caret at 0.
    let back = |c: Cursor| Some((0..c.head, String::new(), Place::End));
    let transaction = each("abcdef", &[Cursor::caret(2), Cursor::caret(4)], &back).expect("valid");
    assert_eq!(ranges(&transaction), [(0, 0)]);
    assert_eq!(text_after("abcdef", &transaction), "ef");

    // Two cursors on one line deleting it, or inserting the same text at
    // one point, make one edit.
    let line = |_: Cursor| Some((0..4, String::new(), Place::End));
    let twice = [Cursor::caret(1), Cursor::caret(3)];
    let transaction = each("abc\ndef", &twice, &line).expect("valid");
    assert_eq!(text_after("abc\ndef", &transaction), "def");
    assert_eq!(ranges(&transaction), [(0, 0)]);
    let shared = |_: Cursor| Some((0..0, "x".to_string(), Place::Map));
    let transaction = each("abc\ndef", &twice, &shared).expect("valid");
    assert_eq!(text_after("abc\ndef", &transaction), "xabc\ndef");

    // Any other overlap is an error.
    let clash = |c: Cursor| Some((0..4, format!("{}", c.head), Place::End));
    let error = each("abc\ndef", &twice, &clash).expect_err("overlap");
    assert!(matches!(
        error,
        TextError::OverlappingEdits { end: 4, start: 0 }
    ));

    // A bad selection fails before the closure is called.
    let calls = std::cell::Cell::new(0);
    let counted = |_: Cursor| -> Request {
        calls.set(calls.get() + 1);
        None
    };
    let error = each("a😀", &[Cursor::caret(0), Cursor::new(1, 3)], &counted).expect_err("bad");
    assert!(matches!(error, TextError::NotCharBoundary { byte: 3 }));
    assert_eq!(calls.get(), 0);
}

// ---------------------------------------------------------------------------
// Limits and hygiene.

/// A rope of 2⁶² bytes made by 62 shared doublings, each O(log n) and none
/// traversing the text. Never compared, displayed or checked: that would
/// walk 2⁶² bytes.
#[cfg(target_pointer_width = "64")]
#[test]
fn too_long_is_an_error() {
    let mut big = Rope::from("a");
    for _ in 0..62 {
        let copy = big.clone();
        big.append(copy);
    }
    assert_eq!(big.len(), 1 << 62);
    let end = big.len();
    let doubled = ChangeSet::new(&big, [(end..end, big.clone())]);
    assert!(
        matches!(doubled, Err(TextError::TooLong { max }) if max == isize::MAX as usize),
        "{doubled:?}"
    );
    assert_eq!(MAX_LEN, isize::MAX as usize);
    let each = Transaction::change_each(&big, &Selection::caret(end), |c| {
        Some((c.head..c.head, big.clone(), Place::End))
    });
    assert!(
        matches!(each, Err(TextError::TooLong { max }) if max == isize::MAX as usize),
        "{each:?}"
    );
    let one = ChangeSet::new(&big, [(0..0, "x")]).expect("one more byte fits");
    assert_eq!(one.len_after(), (1 << 62) + 1);
    let mut text = big.clone();
    one.apply(&mut text).expect("valid");
    assert_eq!(text.len(), (1 << 62) + 1);
    let head: String = text.chunks_in(0..3).expect("valid").collect();
    assert_eq!(head, "xaa");
}

/// `changes` with its replacements edited by `corrupt` and its `len_after`
/// set to `len_after`, which the checker must refuse with a message
/// containing `want` and no text.
fn refused_change_set(
    changes: &ChangeSet,
    len_after: usize,
    corrupt: impl FnOnce(&mut Vec<Replacement>),
    want: &str,
) {
    let mut replacements = changes.replacements.to_vec();
    corrupt(&mut replacements);
    let broken = ChangeSet {
        replacements: replacements.into(),
        len_before: changes.len_before,
        len_after,
    };
    match broken.__check_invariants() {
        Ok(()) => panic!("the checker missed: {want}"),
        Err(message) => assert!(
            message.contains(want) && !message.contains("SECRET"),
            "want {want:?}, got {message:?}"
        ),
    }
}

/// The canonical-form checker refuses each broken invariant, one at a
/// time: C1 (a range inverted or past the end, an empty text that is not
/// `None`, a replacement that does nothing), C2 (no kept byte between
/// neighbours), C3 (a wrong new start), C4 (a wrong or impossible length
/// after) and C5 (a broken text).
#[test]
fn the_checker_finds_every_kind_of_broken_change_set() {
    let rope = Rope::from("SECRET-abcdefgh");
    let good = ChangeSet::new(&rope, [(8..9, "SECRET"), (11..13, "")]).expect("valid");
    good.__check_invariants().expect("a valid change set");
    ChangeSet::identity(3)
        .__check_invariants()
        .expect("the identity");
    let after = good.len_after;
    assert_eq!(
        (
            good.replacements.len(),
            after,
            good.replacements[1].new_start
        ),
        (2, 18, 16)
    );
    type Corrupt = fn(&mut Vec<Replacement>);
    let cases: [(Corrupt, &str); 7] = [
        (|r| r[1].start = 14, "covers 14..13"),
        (|r| r[1].end = 16, "covers 11..16 of a 15-byte text"),
        (|r| r[0].text = Some(Rope::new()), "has an empty text"),
        (|r| r[1].end = 11, "does nothing"),
        (|r| r[1].start = 9, "have no kept byte between them"),
        (
            |r| r[1].new_start = 15,
            "starts at 15 in the text after, not 16",
        ),
        (
            |r| r[0].text = Some(broken_rope("SECRET")),
            "replacement 0's text",
        ),
    ];
    for (corrupt, want) in cases {
        refused_change_set(&good, after, corrupt, want);
    }
    refused_change_set(&good, after + 1, |_| {}, "len_after is 19");
    refused_change_set(
        &ChangeSet::identity(usize::MAX),
        usize::MAX,
        |_| {},
        "(at most",
    );
}

#[test]
fn debug_shows_sizes_and_never_text() {
    let rope = Rope::from("a secret here");
    let changes = ChangeSet::new(&rope, [(2..8, "SECRET")]).expect("valid");
    let selection = Selection::new(Cursor::new(2, 8), [Cursor::caret(0)]);
    let transaction = Transaction::new(changes.clone(), selection.clone());
    let shown = [
        format!("{changes:?}"),
        format!("{:?}", changes.invert(&rope).expect("valid")),
        format!("{:?}", changes.changes()),
        format!("{:?}", changes.changes().next()),
        format!("{selection:?}"),
        format!("{transaction:?}"),
    ];
    for text in shown {
        assert!(
            !text.contains("secret") && !text.contains("SECRET"),
            "{text}"
        );
    }
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ChangeSet>();
    send_sync::<Transaction>();
    send_sync::<Selection>();
    send_sync::<super::Changes<'_>>();
}
