//! Unit tests of selections: fixed merge cases, merging against a
//! component model, and mapping against a per-position model.
//!
//! Each case's input is a [`Plan`]; the generators are shared with
//! `transaction::tests`.

use tidepool_core::testing::check;

use super::{Cursor, Selection};
use crate::transaction::tests::{Plan, Tally, applied, change_set, cursors, same_outcome, text};
use crate::{Assoc, ChangeSet, Rope, TextError};

/// The `(anchor, head)` pairs of a selection, and its primary index.
fn shape(selection: &Selection) -> (Vec<(usize, usize)>, usize) {
    let pairs = selection
        .cursors()
        .iter()
        .map(|c| (c.anchor, c.head))
        .collect();
    (pairs, selection.primary_index())
}

/// `Selection::new(primary, others)`, checked, as a shape.
fn of(primary: Cursor, others: &[Cursor]) -> (Vec<(usize, usize)>, usize) {
    let selection = Selection::new(primary, others.iter().copied());
    selection
        .__check_invariants()
        .unwrap_or_else(|e| panic!("{e}"));
    shape(&selection)
}

#[test]
fn fixed_merge_cases() {
    let (caret, range) = (Cursor::caret, Cursor::new);
    // Two carets at one point become one.
    assert_eq!(of(caret(4), &[caret(4)]), (vec![(4, 4)], 0));
    // A caret at either edge of 2..6, or inside it, joins it.
    for at in [2, 4, 6] {
        assert_eq!(of(range(2, 6), &[caret(at)]), (vec![(2, 6)], 0), "{at}");
        assert_eq!(of(caret(at), &[range(2, 6)]), (vec![(2, 6)], 0), "{at}");
    }
    // {1..3, 3, 3..5}: the caret joins the earlier range, and the touching
    // ranges stay apart.
    let pair = vec![(1, 3), (3, 5)];
    assert_eq!(of(caret(3), &[range(1, 3), range(3, 5)]), (pair.clone(), 0));
    assert_eq!(of(range(3, 5), &[range(1, 3), caret(3)]), (pair, 1));
    assert_eq!(
        of(range(2, 4), &[range(0, 2), caret(2)]),
        (vec![(0, 2), (2, 4)], 1)
    );
    // Touching ranges stay apart; ranges sharing a byte merge, transitively.
    assert_eq!(
        of(range(0, 2), &[range(2, 4), range(4, 6)]),
        (vec![(0, 2), (2, 4), (4, 6)], 0)
    );
    assert_eq!(
        of(range(0, 3), &[range(4, 7), range(2, 5), caret(9)]),
        (vec![(0, 7), (9, 9)], 0)
    );
    // The direction is the primary's if it is a non-empty part ...
    assert_eq!(of(range(5, 1), &[range(3, 8)]), (vec![(8, 1)], 0));
    assert_eq!(of(range(1, 5), &[range(8, 3)]), (vec![(1, 8)], 0));
    // ... else the first non-empty part's, forward winning among equal
    // spans.
    assert_eq!(of(caret(4), &[range(6, 2), range(2, 6)]), (vec![(2, 6)], 0));
    assert_eq!(of(caret(3), &[range(5, 2)]), (vec![(5, 2)], 0));
    assert_eq!(of(caret(3), &[range(3, 9), range(6, 2)]), (vec![(9, 2)], 0));
    // The primary survives, and follows its cursor into a merge.
    assert_eq!(
        of(caret(9), &[caret(1), range(3, 5)]),
        (vec![(1, 1), (3, 5), (9, 9)], 2)
    );
    assert_eq!(
        of(range(4, 6), &[caret(1), range(3, 5)]),
        (vec![(1, 1), (3, 6)], 1)
    );
    assert_eq!(
        of(caret(2), &[range(0, 2), range(2, 4)]),
        (vec![(0, 2), (2, 4)], 0)
    );

    // The constructors, the cursor accessors and `transform`.
    assert_eq!(shape(&Selection::caret(3)), (vec![(3, 3)], 0));
    assert_eq!(shape(&Selection::single(range(5, 2))), (vec![(5, 2)], 0));
    let cursor = range(7, 3);
    assert_eq!((cursor.start(), cursor.end(), cursor.range()), (3, 7, 3..7));
    assert!(!cursor.is_caret() && cursor.is_backward());
    assert!(caret(2).is_caret() && !caret(2).is_backward());
    let halved = Selection::new(caret(5), [caret(1), caret(9)]).transform(|c| caret(c.head / 2));
    assert_eq!(shape(&halved), (vec![(0, 0), (2, 2), (4, 4)], 1));
    let squashed = Selection::new(caret(5), [caret(1), caret(9)]).transform(|_| caret(7));
    assert_eq!(shape(&squashed), (vec![(7, 7)], 0));

    // `validate` checks each cursor's start, then its end, in cursor order.
    let text = Rope::from("a😀b");
    let validate = |primary: Cursor, others: &[Cursor]| {
        Selection::new(primary, others.iter().copied()).validate(&text)
    };
    assert!(validate(caret(0), &[range(1, 5), caret(6)]).is_ok());
    assert!(matches!(
        validate(range(9, 2), &[]),
        Err(TextError::NotCharBoundary { byte: 2 })
    ));
    assert!(matches!(
        validate(range(1, 9), &[]),
        Err(TextError::ByteOutOfBounds { byte: 9, len: 6 })
    ));
    assert!(matches!(
        validate(range(5, 9), &[caret(3)]),
        Err(TextError::NotCharBoundary { byte: 3 })
    ));

    // `abcdef`, [1..2, caret 3], 0..5 replaced by XYZ: the range vanishes,
    // and both become a caret where the `Assoc` says.
    let text = Rope::from("abcdef");
    let changes = ChangeSet::new(&text, [(0..5, "XYZ")]).expect("valid");
    let selection = Selection::new(range(1, 2), [caret(3)]);
    let mapped = |assoc| shape(&selection.map(&changes, assoc).expect("valid"));
    assert_eq!(mapped(Assoc::Before), (vec![(0, 0)], 0));
    assert_eq!(mapped(Assoc::After), (vec![(3, 3)], 0));
    // [0..2, 2..3, 3..5] with 2..3 deleted gives [0..2, 2..4].
    let text = Rope::from("abcde");
    let changes = ChangeSet::new(&text, [(2..3, "")]).expect("valid");
    let selection = Selection::new(range(0, 2), [range(2, 3), range(3, 5)]);
    let mapped = selection.map(&changes, Assoc::After).expect("valid");
    assert_eq!(shape(&mapped), (vec![(0, 2), (2, 4)], 0));
    // Text inserted at a range's edges stays outside it.
    let changes = ChangeSet::new(&text, [(1..1, "<"), (3..3, ">")]).expect("valid");
    let mapped = Selection::single(range(3, 1)).map(&changes, Assoc::After);
    assert_eq!(shape(&mapped.expect("valid")), (vec![(4, 2)], 0));
    // A position past the text before is an error.
    let far = Selection::new(caret(1), [range(4, 7)]).map(&changes, Assoc::After);
    assert!(matches!(
        far,
        Err(TextError::ByteOutOfBounds { byte: 7, len: 5 })
    ));
}

/// The invariant checker refuses each broken invariant, one at a time: S1
/// (no cursors), S2 (the primary out of range), S3 (starts that do not
/// increase) and S4 (neighbours that should have merged).
#[test]
fn the_checker_finds_every_kind_of_broken_selection() {
    let good = Selection::new(Cursor::new(0, 2), [Cursor::new(4, 2), Cursor::caret(6)]);
    assert_eq!(shape(&good), (vec![(0, 2), (4, 2), (6, 6)], 0));
    good.__check_invariants().expect("a valid selection");
    let broken = |cursors: &[(usize, usize)], primary: usize, want: &str| {
        let selection = Selection {
            cursors: cursors.iter().map(|&(a, h)| Cursor::new(a, h)).collect(),
            primary,
        };
        match selection.__check_invariants() {
            Ok(()) => panic!("the checker missed: {want}"),
            Err(message) => assert!(message.contains(want), "want {want:?}, got {message:?}"),
        }
    };
    broken(&[], 0, "has no cursors");
    broken(&[(0, 2), (6, 6)], 2, "is not below the 2 cursors");
    broken(
        &[(6, 6), (2, 2)],
        0,
        "cursor 1 starts at 2, not after cursor 0's start 6",
    );
    broken(
        &[(2, 2), (2, 4)],
        0,
        "cursor 1 starts at 2, not after cursor 0's start 2",
    );
    broken(&[(0, 3), (5, 2)], 1, "should have merged");
    broken(&[(0, 2), (2, 2)], 0, "should have merged");
    broken(&[(4, 2), (4, 4), (9, 9)], 2, "should have merged");
}

/// What merging must give, worked out from the rule itself in O(r²):
/// union-find over the non-empty cursors by shared bytes; each caret joins
/// the group with the lowest start whose closed span holds it; equal carets
/// left over become one; then direction and primary as the module says.
fn merged(cursors: &[Cursor], primary: usize) -> (Vec<(usize, usize)>, usize) {
    fn root(parent: &[usize], mut at: usize) -> usize {
        while parent[at] != at {
            at = parent[at];
        }
        at
    }
    let count = cursors.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for i in 0..count {
        for j in 0..i {
            let (a, b) = (cursors[i], cursors[j]);
            let share = a.start() < b.end() && b.start() < a.end();
            if !a.is_caret() && !b.is_caret() && share {
                let (ri, rj) = (root(&parent, i), root(&parent, j));
                parent[ri] = rj;
            }
        }
    }
    // The spans of the groups of non-empty cursors, by root.
    let mut spans: Vec<Option<(usize, usize)>> = vec![None; count];
    for (i, cursor) in cursors.iter().enumerate() {
        if !cursor.is_caret() {
            let slot = &mut spans[root(&parent, i)];
            *slot = Some(match *slot {
                Some((start, end)) => (start.min(cursor.start()), end.max(cursor.end())),
                None => (cursor.start(), cursor.end()),
            });
        }
    }
    let mut lone_carets: Vec<(usize, usize)> = Vec::new();
    for (i, cursor) in cursors.iter().enumerate() {
        if !cursor.is_caret() {
            continue;
        }
        let at = cursor.head;
        let holder = (0..count)
            .filter_map(|r| spans[r].map(|span| (span, r)))
            .filter(|&((start, end), _)| start <= at && at <= end)
            .min();
        parent[i] = match holder {
            Some((_, r)) => r,
            None => match lone_carets.iter().find(|&&(pos, _)| pos == at) {
                Some(&(_, r)) => r,
                None => {
                    lone_carets.push((at, i));
                    i
                }
            },
        };
    }
    // One output cursor per group.
    let primary_root = root(&parent, primary);
    let mut groups: Vec<(usize, usize, bool, usize)> = Vec::new();
    for r in (0..count).filter(|&r| root(&parent, r) == r) {
        let members: Vec<Cursor> = (0..count)
            .filter(|&i| root(&parent, i) == r)
            .map(|i| cursors[i])
            .collect();
        let (start, end) = spans[r].unwrap_or((cursors[r].head, cursors[r].head));
        let first = members
            .iter()
            .filter(|c| !c.is_caret())
            .min_by_key(|c| (c.start(), c.end(), c.is_backward()));
        let backward = if r == primary_root && !cursors[primary].is_caret() {
            cursors[primary].is_backward()
        } else {
            first.is_some_and(|c| c.is_backward())
        };
        groups.push((start, end, backward, r));
    }
    groups.sort_unstable();
    let pairs = groups
        .iter()
        .map(|&(start, end, backward, _)| if backward { (end, start) } else { (start, end) })
        .collect();
    let index = groups
        .iter()
        .position(|&(_, _, _, r)| r == primary_root)
        .unwrap_or(usize::MAX);
    (pairs, index)
}

/// The kinds of case `normalizing` counts.
const NORMALIZING: [&str; 3] = [
    "cursors merged",
    "a caret joined a range",
    "64 colliding cursors",
];

fn normalizing(plan: &Plan, tally: &Tally<3>) -> Result<(), String> {
    let mut rng = plan.rng();
    let room = ["x".repeat(6), "x".repeat(20), "x".repeat(200)];
    let room = rng.choose(&room).cloned().unwrap_or_default();
    let (inputs, primary) = cursors(&mut rng, &room);
    let others: Vec<Cursor> = inputs
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != primary)
        .map(|(_, &c)| c)
        .collect();
    let got = Selection::new(inputs[primary], others.iter().copied());
    got.__check_invariants()?;
    let want = merged(&inputs, primary);
    if shape(&got) != want {
        return Err(format!("merged to {:?}, want {want:?}", shape(&got)));
    }
    if got.cursors().len() < inputs.len() {
        tally.add(0);
    }
    let joined = inputs.iter().filter(|c| c.is_caret()).any(|c| {
        got.cursors()
            .iter()
            .any(|out| !out.is_caret() && out.start() <= c.head && c.head <= out.end())
    });
    if joined {
        tally.add(1);
    }
    if inputs.len() >= 64 {
        tally.add(2);
    }
    // Idempotent, and independent of the input order.
    let again_others = got
        .cursors()
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != got.primary_index())
        .map(|(_, &c)| c);
    if Selection::new(got.primary(), again_others) != got {
        return Err("merging again changed the selection".to_string());
    }
    let mut shuffled = others.clone();
    rng.shuffle(&mut shuffled);
    if Selection::new(inputs[primary], shuffled) != got {
        return Err("the input order changed the result".to_string());
    }
    // Every input is inside an output; every output's ends are input ends.
    for input in &inputs {
        if !got
            .cursors()
            .iter()
            .any(|out| out.start() <= input.start() && input.end() <= out.end())
        {
            return Err(format!("input {:?} is in no output", input.range()));
        }
    }
    for out in got.cursors() {
        let starts = inputs.iter().any(|input| input.start() == out.start());
        let ends = inputs.iter().any(|input| input.end() == out.end());
        if !starts || !ends {
            return Err(format!("output {:?} has an end no input has", out.range()));
        }
    }
    // `transform` merges as `new` does.
    let flip = |c: Cursor| Cursor::new(c.head, (c.anchor * 3) % 23);
    let transformed = got.clone().transform(flip);
    let rebuilt = Selection::new(flip(got.primary()), {
        let primary = got.primary_index();
        let cursors = got.cursors().to_vec();
        cursors
            .into_iter()
            .enumerate()
            .filter(move |&(i, _)| i != primary)
            .map(move |(_, c)| flip(c))
    });
    if transformed != rebuilt {
        return Err("transform merged differently from new".to_string());
    }
    transformed.__check_invariants()
}

/// Merging against an O(r²) model of the rule for 4,000 cursor lists:
/// S1-S4, idempotence, independence of input order, and `transform`.
#[test]
fn normalizing_matches_a_component_model() {
    let tally = Tally::new(NORMALIZING);
    check(
        "selections merge as the component model says",
        4_000,
        |rng| Plan::draw(rng, 100),
        |plan| normalizing(plan, &tally),
    );
    tally.require([1_200, 1_000, 80]);
}

/// `ChangeSet::map(p, assoc)` by its table, scanning every replacement.
fn table(changes: &ChangeSet, p: usize, assoc: Assoc) -> usize {
    let pick = |before, after| match assoc {
        Assoc::Before => before,
        Assoc::After => after,
    };
    for change in changes.changes() {
        let (s, e) = (change.old.start, change.old.end);
        let (ns, ne) = (change.new.start, change.new.end);
        if e < p {
            continue;
        }
        return if s == p && e == p {
            pick(ns, ne)
        } else if p == s {
            ns
        } else if s < p && p == e {
            ne
        } else if s < p {
            pick(ns, ne)
        } else {
            ns - (s - p)
        };
    }
    changes.len_after() - (changes.len_before() - p)
}

/// What mapping must give: each cursor mapped by the table, a vanished
/// range made a caret, then a full merge (with a sort).
fn mapped_by_model(selection: &Selection, changes: &ChangeSet, caret: Assoc) -> Selection {
    let mapped: Vec<Cursor> = selection
        .cursors()
        .iter()
        .map(|&c| {
            if c.is_caret() {
                return Cursor::caret(table(changes, c.head, caret));
            }
            let start = table(changes, c.start(), Assoc::After);
            let end = table(changes, c.end(), Assoc::Before);
            if start >= end {
                Cursor::caret(table(changes, c.start(), caret))
            } else if c.is_backward() {
                Cursor::new(end, start)
            } else {
                Cursor::new(start, end)
            }
        })
        .collect();
    let primary = selection.primary_index();
    let others = mapped
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != primary)
        .map(|(_, &c)| c);
    Selection::new(mapped[primary], others)
}

/// The kinds of case `mapping` counts.
const MAPPING: [&str; 3] = [
    "the change set changes something",
    "two or more cursors strictly inside one replacement",
    "fewer cursors after mapping",
];

fn mapping(plan: &Plan, tally: &Tally<3>) -> Result<(), String> {
    let mut rng = plan.rng();
    let t = text(&mut rng);
    let rope = Rope::from(t.as_str());
    let (changes, after) = change_set(&mut rng, &t);
    let (mut inputs, primary) = cursors(&mut rng, &t);
    if rng.range(0..10u32) < 3 {
        // Two or more cursors strictly inside one replacement.
        let insides: Vec<Vec<usize>> = changes
            .changes()
            .map(|change| {
                (change.old.start + 1..change.old.end)
                    .filter(|&at| t.is_char_boundary(at))
                    .collect::<Vec<usize>>()
            })
            .filter(|inside| !inside.is_empty())
            .collect();
        if let Some(inside) = rng.choose(&insides).cloned() {
            tally.add(1);
            for _ in 0..rng.range(2..4u32) {
                let a = rng.choose(&inside).copied().unwrap_or(0);
                let b = rng.choose(&inside).copied().unwrap_or(0);
                inputs.push(Cursor::new(a, if rng.range(0..2u32) == 0 { a } else { b }));
            }
        }
    }
    let selection = Selection::from_cursors(inputs, primary);
    let after_rope = applied(&changes, &rope, &after)?;
    if !changes.is_identity() {
        tally.add(0);
    }
    for caret in [Assoc::Before, Assoc::After] {
        let got = selection
            .map(&changes, caret)
            .map_err(|e| format!("map with {caret:?}: {e}"))?;
        got.__check_invariants()?;
        let want = mapped_by_model(&selection, &changes, caret);
        if got != want {
            return Err(format!(
                "{selection:?} maps with {caret:?} to {got:?}, want {want:?}"
            ));
        }
        got.validate(&after_rope)
            .map_err(|e| format!("the mapped selection is not valid after: {e}"))?;
        if caret == Assoc::After && got.cursors().len() < selection.cursors().len() {
            tally.add(2);
        }
    }
    let len = t.len();
    let far = Selection::new(
        selection.primary(),
        selection
            .cursors()
            .iter()
            .copied()
            .chain([Cursor::new(len + 2, len + 1)]),
    );
    let want: Result<Selection, _> = Err(TextError::ByteOutOfBounds { byte: len + 1, len });
    same_outcome(
        || "mapping a selection past the end".to_string(),
        &far.map(&changes, Assoc::After),
        &want,
    )
}

/// Mapping against the per-position table, a vanish rule and a full merge
/// for 2,000 selections and change sets, with both `Assoc`s; 30% of cases
/// put two or more cursors strictly inside one replacement. Equality with
/// the sorted model shows that the mapped cursors need no sort.
#[test]
fn mapping_matches_a_per_position_model() {
    let tally = Tally::new(MAPPING);
    check(
        "selections map as the per-position model says",
        2_000,
        |rng| Plan::draw(rng, 100),
        |plan| mapping(plan, &tally),
    );
    tally.require([750, 140, 60]);
}
