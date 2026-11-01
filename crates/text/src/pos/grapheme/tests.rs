//! Unit tests of grapheme segmentation, in the tiny rope shape (leaves of
//! 4-16 bytes), so that clusters are split across chunk seams.
//!
//! - Every case of `GraphemeBreakTest.txt` on `&str`, and on ropes whose
//!   seams fall at every internal char boundary of every case.
//! - A literal reading of rules GB1-GB999 against random strings of every
//!   kind of char, on `&str` and on ropes.
//! - The backward recovery of the state against stepping it from the start
//!   of the text, for every sequence of up to three packed values that can
//!   be represented, and for random longer ones; and `state_at` on random
//!   strings.
//! - The ASCII fast paths, the table's shape and spot checks of the classes.
//! - Long runs, with the work they take, and every cost the module
//!   documentation states, counted in chars read.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check;

use super::class::{
    CONTROL, CR, EXT_PICT, EXTEND, GCB_MASK, INCB_CONSONANT, INCB_EXTEND, INCB_LINKER, INCB_MASK,
    L, LF, LV, LVT, PREPEND, REGIONAL_INDICATOR, SPACING_MARK, T, V, ZWJ,
};
use super::{
    Conjunct, Emoji, Pair, START, State, graphemes, is_boundary, lookup, next_boundary, pair,
    prev_boundary, props, recover, state_at, step, tables, work,
};
use crate::pos::tests::{error_eq, range_error};
use crate::{Rope, TextError};

// ---------------------------------------------------------------------------
// The conformance test file.

/// Unicode's conformance test for the extended grapheme cluster rules.
const TEST_FILE: &str = include_str!("../../../ucd/17.0.0/GraphemeBreakTest.txt");

/// The number of cases in [`TEST_FILE`].
const CASES: usize = 766;

/// One line of [`TEST_FILE`].
struct Case {
    /// The line number, from 1.
    line: usize,
    text: String,
    /// The byte offsets marked `÷`, in order; 0 and the length among them.
    boundaries: Vec<usize>,
}

/// Every case of [`TEST_FILE`], after checking its header and count.
fn cases() -> Vec<Case> {
    assert_eq!(
        TEST_FILE.lines().next(),
        Some("# GraphemeBreakTest-17.0.0.txt"),
        "the test file is the one for Unicode 17.0.0"
    );
    assert!(TEST_FILE.contains("\n# Lines: 766\n"));
    let mut cases = Vec::new();
    for (index, line) in TEST_FILE.lines().enumerate() {
        let line_number = index + 1;
        let data = line.split('#').next().unwrap_or_default().trim();
        if data.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut boundaries = Vec::new();
        let mut want_marker = true;
        for token in data.split_whitespace() {
            assert_eq!(
                want_marker,
                matches!(token, "÷" | "×"),
                "line {line_number}: markers and code points must alternate"
            );
            match token {
                "÷" => boundaries.push(text.len()),
                "×" => {}
                hex => {
                    let cp = u32::from_str_radix(hex, 16).expect("a hex code point");
                    text.push(char::from_u32(cp).expect("a scalar value"));
                }
            }
            want_marker = !want_marker;
        }
        assert!(!want_marker, "line {line_number}: ends with a marker");
        assert!(
            boundaries.first() == Some(&0) && boundaries.last() == Some(&text.len()),
            "line {line_number}: GB1 and GB2 are marked"
        );
        cases.push(Case {
            line: line_number,
            text,
            boundaries,
        });
    }
    assert_eq!(cases.len(), CASES);
    cases
}

// ---------------------------------------------------------------------------
// The oracle.

/// Checks `is`, `next` and `prev` at every byte in `0..=len + 1` against
/// `want`, the sorted boundaries of a text of `len` bytes (0 and `len`
/// among them).
fn check_queries(
    len: usize,
    want: &[usize],
    is: impl Fn(usize) -> bool,
    next: impl Fn(usize) -> Option<usize>,
    prev: impl Fn(usize) -> Option<usize>,
) -> Result<(), String> {
    for byte in 0..=len + 1 {
        let want_is = want.binary_search(&byte).is_ok();
        if is(byte) != want_is {
            return Err(format!("is_boundary({byte}) is {}", !want_is));
        }
        let want_next = want.iter().copied().find(|&b| b > byte);
        let got = next(byte);
        if got != want_next {
            return Err(format!("next({byte}) is {got:?}, want {want_next:?}"));
        }
        let want_prev = want.iter().rev().copied().find(|&b| b < byte);
        let got = prev(byte);
        if got != want_prev {
            return Err(format!("prev({byte}) is {got:?}, want {want_prev:?}"));
        }
    }
    Ok(())
}

/// The spans between consecutive boundaries in `want` strictly inside
/// `start..end`, with `start` and `end` as cuts.
fn spans_between(want: &[usize], start: usize, end: usize) -> Vec<Range<usize>> {
    if start >= end {
        return Vec::new();
    }
    let mut cuts = vec![start];
    cuts.extend(want.iter().copied().filter(|&b| start < b && b < end));
    cuts.push(end);
    cuts.windows(2).map(|w| w[0]..w[1]).collect()
}

/// Checks every query on `text` and on `rope` (the same text) against
/// `want`, and `graphemes_in` from every char boundary in `starts` to the
/// end and from the start to every one of them.
fn check_text(text: &str, rope: &Rope, want: &[usize], starts: Range<usize>) -> Result<(), String> {
    check_queries(
        text.len(),
        want,
        |b| is_boundary(text, b),
        |b| next_boundary(text, b),
        |b| prev_boundary(text, b),
    )
    .map_err(|e| format!("str: {e}"))?;
    let got: Vec<&str> = graphemes(text).collect();
    let spans = spans_between(want, 0, text.len());
    let expected: Vec<&str> = spans.iter().map(|span| &text[span.clone()]).collect();
    if got != expected {
        return Err(format!("graphemes() is {got:?}, want {expected:?}"));
    }
    check_queries(
        rope.len(),
        want,
        |b| rope.is_grapheme_boundary(b),
        |b| rope.next_grapheme_boundary(b),
        |b| rope.prev_grapheme_boundary(b),
    )
    .map_err(|e| format!("rope: {e}"))?;
    for cut in starts.filter(|&b| text.is_char_boundary(b)) {
        for range in [cut..text.len(), 0..cut] {
            let got: Vec<_> = rope
                .graphemes_in(range.clone())
                .map_err(|e| format!("graphemes_in({range:?}): {e}"))?
                .collect();
            let expected = spans_between(want, range.start, range.end);
            if got != expected {
                return Err(format!(
                    "graphemes_in({range:?}) is {got:?}, want {expected:?}"
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Conformance.

/// Every case of `GraphemeBreakTest.txt` on `&str`: `is`, `next` and `prev`
/// at every byte, and `graphemes()`.
#[test]
fn conformance_str() {
    for case in cases() {
        let text = case.text.as_str();
        check_queries(
            text.len(),
            &case.boundaries,
            |b| is_boundary(text, b),
            |b| next_boundary(text, b),
            |b| prev_boundary(text, b),
        )
        .unwrap_or_else(|e| panic!("line {}: {e}", case.line));
        let got: Vec<&str> = graphemes(text).collect();
        let want: Vec<&str> = case
            .boundaries
            .windows(2)
            .map(|w| &text[w[0]..w[1]])
            .collect();
        assert_eq!(got, want, "line {}", case.line);
    }
}

/// Every case on ropes of `"\u{1}" × k + case + "\u{1}" × 16` for k in
/// 0..=16. U+0001 is a control, so GB4 and GB5 isolate the case. The shifts
/// move the chunk seams across the case; where the builder's cut points
/// (which depend on the text) never land on an internal char boundary, one
/// more rope is made by appending two ropes split there. Every internal
/// char boundary of every case is shown, through `chunks()`, to be a seam
/// at least once, except between CR and LF, which a seam never splits.
#[test]
fn conformance_rope_across_seams() {
    let mut forced = 0;
    for case in cases() {
        let n = case.text.len();
        let mut seams = vec![false; n + 1];
        let run = |k: usize, rope: &Rope, seams: &mut [bool]| {
            let text = rope.to_string();
            assert_eq!(text.len(), k + n + 16);
            let mut at = 0;
            for chunk in rope.chunks() {
                at += chunk.len();
                if k < at && at < k + n {
                    seams[at - k] = true;
                }
            }
            let mut want: Vec<usize> = (0..=k).collect();
            want.extend(
                case.boundaries
                    .iter()
                    .map(|b| b + k)
                    .filter(|&b| k < b && b < k + n),
            );
            want.extend(k + n..=text.len());
            check_text(&text, rope, &want, k.saturating_sub(1)..k + n + 2)
                .unwrap_or_else(|e| panic!("line {}, k = {k}: {e}", case.line));
        };
        let pad = |k: usize| "\u{1}".repeat(k);
        for k in 0..=16 {
            let rope = Rope::from(format!("{}{}{}", pad(k), case.text, pad(16)).as_str());
            run(k, &rope, &mut seams);
        }
        let bytes = case.text.as_bytes();
        let inner = |b: usize| {
            case.text.is_char_boundary(b) && !(bytes[b - 1] == b'\r' && bytes[b] == b'\n')
        };
        let missed: Vec<usize> = (1..n).filter(|&b| inner(b) && !seams[b]).collect();
        for b in missed {
            // Both leaves at the seam have at least MIN_LEAF bytes, so the
            // append keeps it.
            forced += 1;
            let mut rope = Rope::from(format!("{}{}", pad(4), &case.text[..b]).as_str());
            rope.append(Rope::from(
                format!("{}{}", &case.text[b..], pad(16)).as_str(),
            ));
            run(4, &rope, &mut seams);
        }
        for b in (1..n).filter(|&b| inner(b)) {
            assert!(
                seams[b],
                "line {}: no rope had a seam at byte {b} of the case",
                case.line
            );
        }
    }
    // Most boundaries are reached by the shifts alone.
    assert!(forced < 500, "{forced} seams had to be forced");
}

// ---------------------------------------------------------------------------
// A literal reading of the rules.

/// One char of each distinct packed value in the table, then Other (ASCII
/// letters and a space), CR, LF and U+0001.
fn samples() -> Vec<char> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for &(first, last, p) in &tables::RANGES {
        if !seen.contains(&p)
            && let Some(c) = (first..=last).find_map(char::from_u32)
        {
            seen.push(p);
            out.push(c);
        }
    }
    out.extend(['a', 'b', ' ', '\r', '\n', '\u{1}']);
    out
}

/// The samples that take part in GB9c, GB11 or GB12/13.
fn context_samples(samples: &[char]) -> Vec<char> {
    samples
        .iter()
        .copied()
        .filter(|&c| {
            let p = props(c);
            p & INCB_MASK != 0
                || p & EXT_PICT != 0
                || matches!(p & GCB_MASK, ZWJ | EXTEND)
                || p & GCB_MASK == REGIONAL_INDICATOR
        })
        .collect()
}

/// Patterns that GB9c and GB11 join, as alternatives for each place: a
/// conjunct `Consonant [Extend Linker]* Linker [Extend Linker]* Consonant`
/// and an emoji ZWJ sequence `ExtPict Extend* ZWJ ExtPict`.
const MOTIFS: [&[&[char]]; 2] = [
    &[
        &['\u{915}', '\u{995}'],
        &['\u{94D}', '\u{9CD}', '\u{300}', '\u{200D}'],
        &['\u{94D}', '\u{9CD}'],
        &['\u{94D}', '\u{300}', '\u{200D}', '\u{1F3FB}'],
        &['\u{915}', '\u{937}', '\u{995}'],
    ],
    &[
        &['\u{A9}', '\u{1F600}'],
        &['\u{300}', '\u{200C}', '\u{1F3FB}'],
        &['\u{200D}'],
        &['\u{A9}', '\u{1F600}'],
    ],
];

/// A string of up to 24 chars from `samples`, many from `context`, some
/// repeated in runs of 2-4, and some in the patterns of [`MOTIFS`] with
/// places dropped at random (so that they sometimes break).
fn sample_text(rng: &mut Pcg32, samples: &[char], context: &[char]) -> String {
    let len = rng.range(0..25u32) as usize;
    let mut text: Vec<char> = Vec::new();
    while text.len() < len {
        match rng.range(0..8u32) {
            0 => {
                let motif = MOTIFS[rng.range(0..2u32) as usize];
                for place in motif {
                    if rng.range(0..6u32) != 0 {
                        text.push(*rng.choose(place).expect("a non-empty place"));
                    }
                }
            }
            pick => {
                let pool = if pick < 4 { context } else { samples };
                let c = *rng.choose(pool).expect("a non-empty pool");
                let run = if rng.range(0..4u32) == 0 {
                    rng.range(2..5u32)
                } else {
                    1
                };
                text.extend(std::iter::repeat_n(c, run as usize));
            }
        }
    }
    text.truncate(len);
    text.into_iter().collect()
}

/// The rule of UAX #29 section 3.1.1 (revision 47) that decides the
/// position before `p[i]`, for `0 < i < p.len()`, read literally from the
/// rules in order, and whether it is a boundary. Shares no code with the
/// engine but the packed values.
fn reference(p: &[u8], i: usize) -> (&'static str, bool) {
    let gcb = |x: u8| x & GCB_MASK;
    let incb = |x: u8| x & INCB_MASK;
    let pict = |x: u8| x & EXT_PICT != 0;
    let (a, b) = (gcb(p[i - 1]), gcb(p[i]));
    let control = |g: u8| g == CONTROL || g == CR || g == LF;
    if a == CR && b == LF {
        return ("GB3", false);
    }
    if control(a) {
        return ("GB4", true);
    }
    if control(b) {
        return ("GB5", true);
    }
    if a == L && [L, V, LV, LVT].contains(&b) {
        return ("GB6", false);
    }
    if [LV, V].contains(&a) && [V, T].contains(&b) {
        return ("GB7", false);
    }
    if [LVT, T].contains(&a) && b == T {
        return ("GB8", false);
    }
    if b == EXTEND || b == ZWJ {
        return ("GB9", false);
    }
    if b == SPACING_MARK {
        return ("GB9a", false);
    }
    if a == PREPEND {
        return ("GB9b", false);
    }
    // GB9c: Consonant [Extend Linker]* Linker [Extend Linker]* × Consonant.
    let linked = (0..i).any(|k| {
        let between = &p[k + 1..i];
        incb(p[k]) == INCB_CONSONANT
            && between
                .iter()
                .all(|&x| incb(x) == INCB_EXTEND || incb(x) == INCB_LINKER)
            && between.iter().any(|&x| incb(x) == INCB_LINKER)
    });
    if incb(p[i]) == INCB_CONSONANT && linked {
        return ("GB9c", false);
    }
    // GB11: ExtPict Extend* ZWJ × ExtPict.
    let zwj_sequence = a == ZWJ
        && (0..i - 1).any(|k| pict(p[k]) && p[k + 1..i - 1].iter().all(|&x| gcb(x) == EXTEND));
    if pict(p[i]) && zwj_sequence {
        return ("GB11", false);
    }
    // GB12: sot (RI RI)* RI × RI. GB13: [^RI] (RI RI)* RI × RI.
    let ri = |x: u8| gcb(x) == REGIONAL_INDICATOR;
    let odd_run_from = |k: usize| (i - k) % 2 == 1 && p[k..i].iter().all(|&x| ri(x));
    if ri(p[i]) && odd_run_from(0) {
        return ("GB12", false);
    }
    if ri(p[i]) && (1..i).any(|k| !ri(p[k - 1]) && odd_run_from(k)) {
        return ("GB13", false);
    }
    ("GB999", true)
}

/// The engine agrees with the literal reading of the rules on 5,000 random
/// strings of every kind of char, on `&str` and on ropes, and the run
/// exercises every rule, the context rules both ways.
#[test]
fn matches_reference_rules() {
    let samples = samples();
    let context = context_samples(&samples);
    let hits = RefCell::new(BTreeMap::<&'static str, u32>::new());
    check(
        "graphemes match a literal reading of the rules",
        5_000,
        |rng| sample_text(rng, &samples, &context),
        |text| {
            let chars: Vec<(usize, char)> = text.char_indices().collect();
            let p: Vec<u8> = chars.iter().map(|&(_, c)| props(c)).collect();
            let mut want = vec![0];
            for i in 1..p.len() {
                let (rule, boundary) = reference(&p, i);
                // A pair that a context rule could join, but that breaks.
                let (a, b) = (p[i - 1], p[i]);
                let name = match pair(a, b) {
                    Pair::Context if boundary && a & GCB_MASK == REGIONAL_INDICATOR => {
                        "GB12/13 pair, GB999"
                    }
                    Pair::Context if boundary && a & GCB_MASK == ZWJ && b & EXT_PICT != 0 => {
                        "GB11 pair, GB999"
                    }
                    Pair::Context if boundary => "GB9c pair, GB999",
                    _ => rule,
                };
                *hits.borrow_mut().entry(name).or_default() += 1;
                if boundary {
                    want.push(chars[i].0);
                }
            }
            if !text.is_empty() {
                want.push(text.len());
            }
            let rope = Rope::from(text.as_str());
            check_text(text, &rope, &want, 0..text.len() + 1)
        },
    );
    let hits = hits.into_inner();
    let rules = [
        "GB3", "GB4", "GB5", "GB6", "GB7", "GB8", "GB9", "GB9a", "GB9b", "GB9c", "GB11", "GB12",
        "GB13", "GB999",
    ];
    let broken = [
        "GB9c pair, GB999",
        "GB11 pair, GB999",
        "GB12/13 pair, GB999",
    ];
    for rule in rules.into_iter().chain(broken) {
        let count = hits.get(rule).copied().unwrap_or(0);
        assert!(
            count >= 20,
            "{rule} decided only {count} positions: {hits:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The state.

/// Every state.
fn all_states() -> Vec<State> {
    let mut states = Vec::new();
    for ri_odd in [false, true] {
        for emoji in [Emoji::None, Emoji::Pict, Emoji::PictZwj] {
            for conjunct in [Conjunct::None, Conjunct::Consonant, Conjunct::Linked] {
                states.push(State {
                    ri_odd,
                    emoji,
                    conjunct,
                });
            }
        }
    }
    states
}

/// `state_at` gives the state of stepping from the start of the text, at
/// every char boundary of random strings, on `&str` and on ropes.
#[test]
fn state_at_matches_dfa() {
    let samples = samples();
    let context = context_samples(&samples);
    check(
        "state_at matches stepping from the start",
        2_000,
        |rng| sample_text(rng, &samples, &context),
        |text| {
            let rope = Rope::from(text.as_str());
            let mut state = START;
            for (at, c) in text.char_indices().chain([(text.len(), '\0')]) {
                let from_str = state_at(text.as_str(), at);
                let from_rope = state_at(&rope, at);
                if from_str != state || from_rope != state {
                    return Err(format!(
                        "at {at}: str {from_str:?}, rope {from_rope:?}, want {state:?}"
                    ));
                }
                state = step(state, props(c));
            }
            Ok(())
        },
    );
}

/// Every packed value that can be represented, not only those in today's
/// table, because the recovery must be right for any data: the 14 GCB
/// values, each with the 4 InCB values, with and without
/// Extended_Pictographic.
fn every_value() -> Vec<u8> {
    let mut values = Vec::new();
    for gcb in 0..=LVT {
        for incb in [0, INCB_LINKER, INCB_CONSONANT, INCB_EXTEND] {
            for pict in [0, EXT_PICT] {
                values.push(gcb | incb | pict);
            }
        }
    }
    values
}

/// The state after the chars of packed values `values`, stepped from the
/// start of the text.
fn stepped(values: &[u8]) -> State {
    values.iter().fold(START, |state, &p| step(state, p))
}

/// The fewest of the last values of `values` that leave the same state
/// whatever the state before them, or all of them if no tail does: a
/// backward pass cannot stop sooner, and need not read more.
fn needed(values: &[u8], states: &[State]) -> usize {
    (0..=values.len())
        .find(|&k| {
            let tail = &values[values.len() - k..];
            let after = |from: State| tail.iter().fold(from, |state, &p| step(state, p));
            let first = after(states[0]);
            states.iter().all(|&from| after(from) == first)
        })
        .unwrap_or(values.len())
}

/// `recover` on `values`, read backward: the state and how many values it
/// read.
fn recovered(values: &[u8]) -> (State, usize) {
    let mut back = values.iter().copied().rev();
    let state = recover(back.by_ref());
    (state, values.len() - back.len())
}

/// Checks that `recover` on `values` gives the state stepped from the start
/// and reads exactly as many values as it needs.
fn check_recovery(values: &[u8], states: &[State]) -> Result<(), String> {
    let (got, read) = recovered(values);
    let want = stepped(values);
    if got != want {
        return Err(format!("{values:02x?}: recovered {got:?}, want {want:?}"));
    }
    let need = needed(values, states);
    if read != need {
        return Err(format!(
            "{values:02x?}: read {read} values, but {need} are needed"
        ));
    }
    Ok(())
}

/// The backward recovery gives the state that stepping from the start of
/// the text gives, and stops as soon as the state is fixed: for every
/// sequence of up to 3 representable values, and at every prefix of random
/// sequences of up to 12, mostly of values that continue a pattern.
#[test]
fn recovery_matches_stepping_from_the_start() {
    let values = every_value();
    assert_eq!(values.len(), 14 * 4 * 2);
    let states = all_states();
    let fail = |e: String| panic!("{e}");
    check_recovery(&[], &states).unwrap_or_else(fail);
    for &a in &values {
        check_recovery(&[a], &states).unwrap_or_else(fail);
        for &b in &values {
            check_recovery(&[a, b], &states).unwrap_or_else(fail);
            for &c in &values {
                check_recovery(&[a, b, c], &states).unwrap_or_else(fail);
            }
        }
    }
    // Regional indicators, Extend, ZWJ, InCB Linker and Extend: the values
    // that continue at least one pattern.
    let continuing: Vec<u8> = values
        .iter()
        .copied()
        .filter(|&p| {
            p & EXT_PICT == 0
                && (matches!(p & GCB_MASK, EXTEND | ZWJ | REGIONAL_INDICATOR)
                    || matches!(p & INCB_MASK, INCB_LINKER | INCB_EXTEND))
        })
        .collect();
    check(
        "the recovery matches stepping from the start",
        5_000,
        |rng| {
            let len = rng.range(0..13u32);
            (0..len)
                .map(|_| {
                    let pool = if rng.range(0..4u32) == 0 {
                        &values
                    } else {
                        &continuing
                    };
                    *rng.choose(pool).expect("a non-empty pool")
                })
                .collect::<Vec<u8>>()
        },
        |sequence| (0..=sequence.len()).try_for_each(|k| check_recovery(&sequence[..k], &states)),
    );
}

/// A char after which the state is the same whatever it was before ends
/// the step back at once: the recovery reads it and nothing more. Any other
/// char makes it read on. Checked for every representable value.
#[test]
fn recovery_stops_at_a_char_that_forgets_the_state() {
    let states = all_states();
    let mut forgetting = 0;
    for p in every_value() {
        let forgets = states.iter().all(|&from| step(from, p) == step(START, p));
        // Before `p`, values that would each continue some pattern.
        let values = [
            REGIONAL_INDICATOR,
            EXTEND | INCB_LINKER,
            ZWJ | INCB_EXTEND,
            p,
        ];
        let (state, read) = recovered(&values);
        if forgets {
            forgetting += 1;
            assert_eq!(read, 1, "value {p:#04x} forgets the state");
            assert_eq!(state, step(START, p), "value {p:#04x}");
        } else {
            assert!(read > 1, "value {p:#04x} does not forget the state");
        }
    }
    // InCB None or Consonant, not a regional indicator, and either
    // Extended_Pictographic (13 GCB values) or neither Extend nor ZWJ (11).
    assert_eq!(forgetting, 13 * 2 + 11 * 2);
}

// ---------------------------------------------------------------------------
// The table and the fast paths.

/// The ASCII fast path of `props` agrees with the table; two ASCII chars
/// always break except CR LF, whatever the state; an ASCII char leaves the
/// start state, and a step back stops at it.
#[test]
fn fast_paths_match_table() {
    for a in (0..0x80u8).map(char::from) {
        assert_eq!(props(a), lookup(a), "{a:?}");
        let (state, read) = recovered(&[REGIONAL_INDICATOR, EXTEND | INCB_LINKER, props(a)]);
        assert_eq!((state, read), (START, 1), "{a:?}");
        for state in all_states() {
            assert_eq!(step(state, props(a)), START, "{a:?} from {state:?}");
        }
        for b in (0..0x80u8).map(char::from) {
            let want = if (a, b) == ('\r', '\n') {
                Pair::Join
            } else {
                Pair::Break
            };
            assert_eq!(pair(props(a), props(b)), want, "{a:?} {b:?}");
        }
    }
}

/// The generated ranges are sorted, disjoint, merged, within Unicode, and
/// hold only known values, none of them both ZWJ and Extended_Pictographic.
#[test]
fn table_is_sorted_and_merged() {
    assert!(!tables::RANGES.is_empty());
    for &(first, last, p) in &tables::RANGES {
        assert!(first <= last && last <= 0x10_FFFF, "{first:#x}..={last:#x}");
        assert!(p != 0, "{first:#x}: a range of defaults");
        assert_eq!(
            p & !(GCB_MASK | INCB_MASK | EXT_PICT),
            0,
            "{first:#x}: {p:#x}"
        );
        assert!(p & GCB_MASK <= LVT, "{first:#x}: {p:#x}");
        assert!(
            !(p & GCB_MASK == ZWJ && p & EXT_PICT != 0),
            "{first:#x}: both ZWJ and Extended_Pictographic"
        );
    }
    for two in tables::RANGES.windows(2) {
        let [(_, last, p), (first, _, q)] = [two[0], two[1]];
        assert!(last < first, "{last:#x} and {first:#x} are out of order");
        assert!(
            last + 1 < first || p != q,
            "{last:#x} and {first:#x} should be one range"
        );
    }
    // Code points between ranges are defaults.
    for c in ['a', '\u{FFFD}', '\u{10FFFF}', '\u{E1000}'] {
        assert_eq!(lookup(c), 0, "{c:?}");
    }
    assert_eq!(lookup('\u{E0000}'), CONTROL);
}

/// Spot checks of the classes against the Unicode 17.0.0 files.
#[test]
fn class_spot_checks() {
    let expected = [
        ('\u{0600}', PREPEND),
        ('\u{0903}', SPACING_MARK),
        ('\u{1100}', L),
        ('\u{1160}', V),
        ('\u{11A8}', T),
        ('\u{AC00}', LV),
        ('\u{AC01}', LVT),
        ('\u{0915}', INCB_CONSONANT),
        ('\u{094D}', EXTEND | INCB_LINKER),
        ('\u{1B44}', EXTEND | INCB_LINKER),
        ('\u{0300}', EXTEND | INCB_EXTEND),
        ('\u{200C}', EXTEND),
        ('\u{200D}', ZWJ | INCB_EXTEND),
        ('\u{1F1E6}', REGIONAL_INDICATOR),
        ('\u{1F600}', EXT_PICT),
        ('\u{00A9}', EXT_PICT),
        ('\u{1F3FB}', EXTEND | INCB_EXTEND),
        ('\r', CR),
        ('\n', LF),
        ('\u{1}', CONTROL),
        ('a', 0),
    ];
    for (c, want) in expected {
        assert_eq!(props(c), want, "U+{:04X}", u32::from(c));
    }
}

// ---------------------------------------------------------------------------
// Edge cases.

#[test]
fn the_empty_text_has_one_boundary_and_no_clusters() {
    let rope = Rope::new();
    assert!(is_boundary("", 0) && rope.is_grapheme_boundary(0));
    assert!(!is_boundary("", 1) && !rope.is_grapheme_boundary(1));
    assert_eq!(next_boundary("", 0), None);
    assert_eq!(rope.next_grapheme_boundary(0), None);
    assert_eq!(prev_boundary("", 0), None);
    assert_eq!(rope.prev_grapheme_boundary(0), None);
    assert_eq!(prev_boundary("", 7), Some(0));
    assert_eq!(rope.prev_grapheme_boundary(usize::MAX), Some(0));
    assert_eq!(graphemes("").next(), None);
    assert_eq!(rope.graphemes_in(0..0).map(Iterator::count).ok(), Some(0));
}

#[test]
fn queries_are_total() {
    let text = "a\u{301}😀\r\n";
    let rope = Rope::from(text);
    for byte in [usize::MAX, usize::MAX - 1, text.len() + 1, 2, 4] {
        assert!(!is_boundary(text, byte) && !rope.is_grapheme_boundary(byte));
    }
    assert_eq!(next_boundary(text, usize::MAX), None);
    assert_eq!(prev_boundary(text, usize::MAX), Some(text.len()));
    assert_eq!(rope.next_grapheme_boundary(usize::MAX), None);
    assert_eq!(rope.prev_grapheme_boundary(usize::MAX), Some(text.len()));
    // Inside a char.
    assert_eq!(next_boundary(text, 2), Some(3));
    assert_eq!(prev_boundary(text, 2), Some(0));
    assert_eq!(rope.next_grapheme_boundary(5), Some(7));
    assert_eq!(rope.prev_grapheme_boundary(5), Some(3));
}

/// `graphemes_in` refuses a bad range with the error `remove` gives, in the
/// same order.
#[test]
fn graphemes_in_errors_are_those_of_remove() {
    let text = "a😀b\r\n";
    let rope = Rope::from(text);
    for start in 0..=text.len() + 2 {
        for end in 0..=text.len() + 2 {
            let range = start..end;
            let got = rope.graphemes_in(range.clone()).map(|_| ());
            let mut copy = rope.clone();
            let removed = copy.remove(range);
            let want = range_error(text, start, end);
            let same = match (&got, &want, &removed) {
                (Ok(()), Ok(()), Ok(())) => true,
                (Err(a), Err(b), Err(c)) => error_eq(a, b) && error_eq(a, c),
                _ => false,
            };
            assert!(same, "{start}..{end}: {got:?}, want {want:?}");
        }
    }
    assert!(matches!(
        rope.graphemes_in(9..usize::MAX),
        Err(TextError::ByteOutOfBounds { .. })
    ));
}

#[test]
fn iterators_clone_and_fuse() {
    let text = "e\u{301}🇺🇸🇺🇸\r\nक्षa";
    let mut all = graphemes(text);
    assert_eq!(all.next(), Some("e\u{301}"));
    let copy = all.clone();
    let rest: Vec<&str> = all.by_ref().collect();
    assert_eq!(rest, ["🇺🇸", "🇺🇸", "\r\n", "क्ष", "a"]);
    assert_eq!(copy.collect::<Vec<_>>(), rest);
    assert_eq!(all.next(), None);
    assert_eq!(all.next(), None);

    let rope = Rope::from(text);
    let mut spans = rope.graphemes_in(1..text.len()).expect("a valid range");
    assert_eq!(spans.next(), Some(1..3));
    let copy = spans.clone();
    let rest: Vec<_> = spans.by_ref().collect();
    assert_eq!(copy.collect::<Vec<_>>(), rest);
    assert_eq!(rest.last(), Some(&(text.len() - 1..text.len())));
    assert_eq!(spans.next(), None);
    assert_eq!(spans.next(), None);
}

/// A cut inside a cluster ends or starts a span, and the context before the
/// range still counts: the second flag of a pair starts no new flag.
#[test]
fn graphemes_in_cuts_at_the_range_ends() {
    let flags = "🇺🇸🇺🇸🇺🇸";
    let rope = Rope::from(flags);
    let spans: Vec<_> = rope.graphemes_in(4..20).expect("a valid range").collect();
    assert_eq!(spans, [4..8, 8..16, 16..20]);
    let spans: Vec<_> = rope.graphemes_in(8..24).expect("a valid range").collect();
    assert_eq!(spans, [8..16, 16..24]);
}

// ---------------------------------------------------------------------------
// Long runs.

/// Checks queries at chosen bytes of `text` and its rope against `want`
/// (byte, is, next, prev).
fn check_points(text: &str, rope: &Rope, want: &[(usize, bool, Option<usize>, Option<usize>)]) {
    for &(byte, is, next, prev) in want {
        assert_eq!(is_boundary(text, byte), is, "str is({byte})");
        assert_eq!(rope.is_grapheme_boundary(byte), is, "rope is({byte})");
        assert_eq!(next_boundary(text, byte), next, "str next({byte})");
        assert_eq!(rope.next_grapheme_boundary(byte), next, "rope next({byte})");
        assert_eq!(prev_boundary(text, byte), prev, "str prev({byte})");
        assert_eq!(rope.prev_grapheme_boundary(byte), prev, "rope prev({byte})");
    }
}

/// The chars read by `f`.
fn work_of<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = work::read();
    let result = f();
    (result, work::read() - before)
}

/// Iterating over the clusters of `text` and of its rope (also from
/// `middle`) gives `count` clusters and reads each char at most `per_char`
/// times on average.
fn check_iteration(text: &str, rope: &Rope, middle: usize, count: usize, per_char: u64) {
    let chars = text.chars().count() as u64;
    let (n, used) = work_of(|| graphemes(text).count());
    assert_eq!(n, count);
    assert!(
        used <= per_char * chars,
        "str: {used} reads for {chars} chars"
    );
    let (n, used) = work_of(|| rope.graphemes_in(0..rope.len()).map(Iterator::count));
    assert_eq!(n.ok(), Some(count));
    assert!(
        used <= per_char * chars,
        "rope: {used} reads for {chars} chars"
    );
    let (spans, used) = work_of(|| rope.graphemes_in(middle..rope.len()).map(Iterator::count));
    assert!(spans.is_ok());
    assert!(
        used <= 2 * per_char * chars,
        "rope from {middle}: {used} reads"
    );
}

/// Long runs of regional indicators, extending marks, Indic conjuncts and
/// emoji ZWJ sequences: right at both ends and in the middle, and iterating
/// is linear.
#[test]
fn long_runs_are_linear() {
    // 64 KiB of regional indicators: 8,192 flags of 8 bytes.
    let text = "\u{1F1E6}".repeat(16_384);
    let rope = Rope::from(text.as_str());
    let (len, mid) = (text.len(), text.len() / 2);
    check_iteration(&text, &rope, mid + 4, 8_192, 2);
    assert!(graphemes(&text).all(|flag| flag.len() == 8));
    check_points(
        &text,
        &rope,
        &[
            (0, true, Some(8), None),
            (4, false, Some(8), Some(0)),
            (mid, true, Some(mid + 8), Some(mid - 8)),
            (mid + 4, false, Some(mid + 8), Some(mid)),
            (len - 4, false, Some(len), Some(len - 8)),
            (len, true, None, Some(len - 8)),
        ],
    );

    // "a" and 64 KiB of U+0301: one cluster, and O(1) per position inside.
    let text = format!("a{}", "\u{301}".repeat(32_768));
    let rope = Rope::from(text.as_str());
    let (len, mid) = (text.len(), 1 + 2 * 16_384);
    check_iteration(&text, &rope, mid, 1, 2);
    check_points(
        &text,
        &rope,
        &[
            (0, true, Some(len), None),
            (1, false, Some(len), Some(0)),
            (mid, false, Some(len), Some(0)),
            (len, true, None, Some(0)),
        ],
    );
    let (is, used) = work_of(|| rope.is_grapheme_boundary(mid));
    assert!(!is && used <= 2, "{used} reads inside a run of marks");
    let (is, used) = work_of(|| is_boundary(&text, mid));
    assert!(!is && used <= 2, "{used} reads inside a run of marks");

    // KA + virama, 10,922 times, then KA: one conjunct cluster (GB9c).
    let text = format!("{}\u{915}", "\u{915}\u{94D}".repeat(10_922));
    let rope = Rope::from(text.as_str());
    let (len, mid) = (text.len(), 6 * 5_461);
    check_iteration(&text, &rope, mid, 1, 2);
    check_points(
        &text,
        &rope,
        &[
            (3, false, Some(len), Some(0)),
            (mid, false, Some(len), Some(0)),
            (len - 3, false, Some(len), Some(0)),
            (len, true, None, Some(0)),
        ],
    );
    let (is, used) = work_of(|| rope.is_grapheme_boundary(mid));
    assert!(!is && used <= 8, "{used} reads inside a conjunct");

    // 😀 ZWJ, 8,192 times, then 😀: one emoji ZWJ sequence (GB11).
    let text = format!("{}\u{1F600}", "\u{1F600}\u{200D}".repeat(8_192));
    let rope = Rope::from(text.as_str());
    let (len, mid) = (text.len(), 7 * 4_096);
    check_iteration(&text, &rope, mid, 1, 2);
    check_points(
        &text,
        &rope,
        &[
            (mid, false, Some(len), Some(0)),
            (mid + 4, false, Some(len), Some(0)),
            (len, true, None, Some(0)),
        ],
    );
    let (is, used) = work_of(|| rope.is_grapheme_boundary(mid));
    assert!(!is && used <= 8, "{used} reads inside a ZWJ sequence");
}

// ---------------------------------------------------------------------------
// The costs of the module documentation, in chars read.

/// Runs one query on a `&str` and on a rope of the same text, and checks
/// that both give the same answer, read the same chars, and read at most
/// `limit` of them. So on a rope only finding the position costs more.
fn cheap<R: PartialEq + std::fmt::Debug>(
    limit: u64,
    what: &str,
    on_str: impl FnOnce() -> R,
    on_rope: impl FnOnce() -> R,
) -> R {
    let (got, used) = work_of(on_str);
    let (from_rope, rope_used) = work_of(on_rope);
    assert_eq!(got, from_rope, "{what}: the str and the rope differ");
    assert_eq!(
        used, rope_used,
        "{what}: the str and the rope read differently"
    );
    assert!(used <= limit, "{what} read {used} chars, more than {limit}");
    got
}

/// `is`, `next` and `prev` at `byte`, each checked by [`cheap`].
fn cheap_queries(
    text: &str,
    rope: &Rope,
    byte: usize,
    limit: u64,
) -> (bool, Option<usize>, Option<usize>) {
    let is = cheap(
        limit,
        &format!("is({byte})"),
        || is_boundary(text, byte),
        || rope.is_grapheme_boundary(byte),
    );
    let next = cheap(
        limit,
        &format!("next({byte})"),
        || next_boundary(text, byte),
        || rope.next_grapheme_boundary(byte),
    );
    let prev = cheap(
        limit,
        &format!("prev({byte})"),
        || prev_boundary(text, byte),
        || rope.prev_grapheme_boundary(byte),
    );
    (is, next, prev)
}

/// The spans of `rope.graphemes_in(range)` and the chars that took.
fn spans_and_work(rope: &Rope, range: Range<usize>) -> (Vec<Range<usize>>, u64) {
    let (spans, used) = work_of(|| rope.graphemes_in(range).map(Iterator::collect::<Vec<_>>));
    (spans.expect("a valid range"), used)
}

/// The facts about today's table that the stated costs rely on: every InCB
/// Linker or Extend char is GCB Extend or ZWJ, so a step back before a
/// consonant or a pictograph stays in the run of extending chars; and
/// regional indicators have InCB None, so a step back between two of them
/// reads only their run.
#[test]
fn the_costs_rely_on_these_facts_about_the_table() {
    for &(first, _, p) in &tables::RANGES {
        if matches!(p & INCB_MASK, INCB_LINKER | INCB_EXTEND) {
            assert!(matches!(p & GCB_MASK, EXTEND | ZWJ), "{first:#x}: {p:#x}");
        }
        if p & GCB_MASK == REGIONAL_INDICATOR {
            assert_eq!(p & INCB_MASK, 0, "{first:#x}: {p:#x}");
        }
    }
}

/// Typical text: every query at every byte reads a few chars, iterating
/// reads each char once, and a range iterator from any char boundary reads
/// little more than its range.
#[test]
fn typical_text_reads_little() {
    let line = "Grüße, 世界! e\u{301} 👩\u{200D}💻 🇺🇸 क्षि 각 👍🏽\r\n";
    let text = line.repeat(32);
    let rope = Rope::from(text.as_str());
    let want: Vec<usize> = std::iter::once(0)
        .chain(graphemes(&text).scan(0, |at, cluster| {
            *at += cluster.len();
            Some(*at)
        }))
        .collect();
    for byte in 0..=text.len() + 1 {
        let (is, next, prev) = cheap_queries(&text, &rope, byte, 12);
        assert_eq!(is, want.binary_search(&byte).is_ok(), "is({byte})");
        assert_eq!(
            next,
            want.iter().copied().find(|&b| b > byte),
            "next({byte})"
        );
        let before = want.iter().rev().copied().find(|&b| b < byte);
        assert_eq!(prev, before, "prev({byte})");
    }
    let chars = text.chars().count() as u64;
    let (count, used) = work_of(|| graphemes(&text).count());
    assert_eq!((count, used), (want.len() - 1, chars));
    // From byte 0 the state is known, so there is no step back.
    let (spans, used) = spans_and_work(&rope, 0..text.len());
    assert_eq!((spans.len(), used), (want.len() - 1, chars));
    for start in (0..line.len() * 2).filter(|&b| text.is_char_boundary(b)) {
        let (spans, used) = spans_and_work(&rope, start..text.len());
        assert_eq!(spans.last().map(|span| span.end), Some(text.len()));
        let range_chars = text[start..].chars().count() as u64;
        assert!(used <= range_chars + 4, "from {start}: {used} reads");
    }
}

/// A line of flags, each followed by a mark that GB9 joins to it (U+0301,
/// U+FE0F or a ZWJ): a query between the two indicators of a flag steps
/// back over the first one and stops at the mark before it. (Stepping back
/// to a char that resets the state and replaying used to read the whole
/// line.)
#[test]
fn flags_between_marks_step_back_over_their_own_run() {
    for mark in ['\u{301}', '\u{FE0F}', '\u{200D}'] {
        let unit = format!("\u{1F1FA}\u{1F1F8}{mark}");
        let n = unit.len();
        let text = unit.repeat(4_000);
        let rope = Rope::from(text.as_str());
        let len = text.len();
        for k in 1..=3 {
            let start = len - k * n;
            let end = start + n;
            let after = (end < len).then_some(end + n);
            let at = |byte| cheap_queries(&text, &rope, byte, 16);
            assert_eq!(at(start), (true, Some(end), Some(start - n)), "{mark:?}");
            assert_eq!(at(start + 4), (false, Some(end), Some(start)), "{mark:?}");
            assert_eq!(at(start + 8), (false, Some(end), Some(start)), "{mark:?}");
            assert_eq!(at(end), (true, after, Some(start)), "{mark:?}");
        }
    }
}

/// A flag after a long run of marks: the queries and a range iterator at
/// the flag read only around it.
#[test]
fn a_flag_after_a_long_run_of_marks_steps_back_little() {
    let text = format!("a{}\u{1F1FA}\u{1F1F8}", "\u{301}".repeat(32_768));
    let rope = Rope::from(text.as_str());
    let len = text.len();
    assert_eq!(
        cheap_queries(&text, &rope, len - 4, 16),
        (false, Some(len), Some(len - 8))
    );
    assert_eq!(
        cheap_queries(&text, &rope, len, 16),
        (true, None, Some(len - 8))
    );
    let (spans, used) = spans_and_work(&rope, len - 8..len);
    assert_eq!(spans, std::slice::from_ref(&(len - 8..len)));
    assert!(used <= 16, "{used} reads");
}

/// GB11 after a long line of flags and marks: the query reads the ZWJ, the
/// mark before it and the indicator before that, which ends the pattern.
#[test]
fn a_zwj_sequence_after_a_long_line_steps_back_little() {
    let text = format!("{}\u{200D}\u{1F600}", "\u{1F1FA}\u{301}".repeat(16_000));
    let rope = Rope::from(text.as_str());
    let len = text.len();
    // The indicator before the mark is not a pictograph, so GB11 does not
    // join.
    assert_eq!(
        cheap_queries(&text, &rope, len - 4, 16),
        (true, Some(len), Some(len - 13))
    );
    // With a pictograph before the mark, it joins.
    let text = format!(
        "{}\u{1F600}\u{301}\u{200D}\u{1F600}",
        "\u{1F1FA}\u{301}".repeat(16_000)
    );
    let rope = Rope::from(text.as_str());
    let len = text.len();
    assert_eq!(
        cheap_queries(&text, &rope, len - 4, 16),
        (false, Some(len), Some(len - 13))
    );
}

/// A `graphemes_in` window inside a long run of marks, after a letter or a
/// consonant, reads only the window: GB9 joins every mark without looking
/// back.
#[test]
fn a_window_inside_a_run_of_marks_reads_only_the_window() {
    for (base, mark) in [('a', '\u{301}'), ('\u{915}', '\u{94D}')] {
        let text = format!("{base}{}", mark.to_string().repeat(32_768));
        let rope = Rope::from(text.as_str());
        let width = mark.len_utf8();
        let mid = base.len_utf8() + width * 16_384;
        // 128 chars.
        let window = mid..mid + 128 * width;
        let (spans, used) = spans_and_work(&rope, window.clone());
        assert_eq!(spans, std::slice::from_ref(&window), "{mark:?}");
        assert!(used <= 128 + 16, "{mark:?}: {used} reads");
        let is = cheap(
            4,
            "is(mid)",
            || is_boundary(&text, mid),
            || rope.is_grapheme_boundary(mid),
        );
        assert!(!is, "{mark:?}: inside the run");
    }
}

/// GB9c and GB11: the step back reads the run of extending chars just
/// before the position, and nothing before it, however long the line. The
/// run is read even where the rule's own pattern ends sooner (ZWNJ is
/// Extend but InCB None; a ZWJ before the ZWJ ends GB11's pattern), because
/// the pass recovers all three parts of the state.
#[test]
fn conjuncts_and_zwj_sequences_step_back_over_their_marks() {
    let line = "\u{1F1FA}\u{1F1F8}\u{301}".repeat(2_000);
    for run in [0, 1, 7, 100, 1_000] {
        let marks = |mark: char| mark.to_string().repeat(run);
        // The cluster's text before its last char, that char, and whether
        // there is a boundary before it.
        let cases = [
            (
                format!("\u{915}{}\u{94D}", marks('\u{301}')),
                '\u{915}',
                false,
            ),
            (
                format!("\u{915}{}\u{94D}", marks('\u{200C}')),
                '\u{915}',
                run > 0,
            ),
            (
                format!("\u{1F600}{}\u{200D}", marks('\u{301}')),
                '\u{1F600}',
                false,
            ),
            (
                format!("\u{1F600}{}\u{200D}", marks('\u{200D}')),
                '\u{1F600}',
                run > 0,
            ),
        ];
        for (head, last, want) in cases {
            let text = format!("{line}{head}{last}");
            let rope = Rope::from(text.as_str());
            let byte = text.len() - last.len_utf8();
            let what = format!("{head:?} {last:?}");
            let run = run as u64;
            let is = cheap(
                run + 8,
                &what,
                || is_boundary(&text, byte),
                || rope.is_grapheme_boundary(byte),
            );
            assert_eq!(is, want, "{what}");
            let (_, used) = work_of(|| is_boundary(&text, byte));
            assert!(used >= run, "{what}: {used} reads");
        }
    }
}

/// GB12 and GB13: between two regional indicators, the step back reads the
/// run of indicators before the position. So a sweep of `next_boundary` or
/// `prev_boundary` calls across a run of r indicators reads O(r²) chars,
/// while iterating reads each char once, and a range iterator steps back
/// once.
#[test]
fn regional_indicators_step_back_over_their_run() {
    let r: usize = 512;
    let text = format!("x{}", "\u{1F1E6}".repeat(r));
    let rope = Rope::from(text.as_str());
    let len = text.len();
    for k in 1..r {
        // After k indicators.
        let byte = 1 + 4 * k;
        let (is, used) = work_of(|| is_boundary(&text, byte));
        assert_eq!(is, k % 2 == 0, "is({byte})");
        let k = k as u64;
        assert!(k <= used && used <= k + 4, "is({byte}): {used} reads");
    }
    let r64 = r as u64;
    let sweep = |next: &dyn Fn(usize) -> Option<usize>| {
        work_of(|| std::iter::successors(Some(0), |&b| next(b)).count())
    };
    let (count, used) = sweep(&|b| next_boundary(&text, b));
    assert_eq!(count, 2 + r / 2);
    assert!(
        r64 * r64 / 8 <= used && used <= r64 * r64,
        "next: {used} reads"
    );
    let (count, used) = sweep(&|b| prev_boundary(&text, len - b).map(|p| len - p));
    assert_eq!(count, 2 + r / 2);
    assert!(
        r64 * r64 / 8 <= used && used <= r64 * r64,
        "prev: {used} reads"
    );
    let (count, used) = work_of(|| graphemes(&text).count());
    assert_eq!((count, used), (1 + r / 2, 1 + r64));
    let (spans, used) = spans_and_work(&rope, 0..len);
    assert_eq!((spans.len(), used), (1 + r / 2, 1 + r64), "from byte 0");
    // From inside the run: one step back over the indicators before the
    // range, then one read per char.
    for k in [1, 2, 255, 256, 400] {
        let start = 1 + 4 * k;
        let (spans, used) = spans_and_work(&rope, start..len);
        assert_eq!(spans.last().map(|span| span.end), Some(len));
        let k = k as u64;
        assert!(used <= (r64 - k) + k + 4, "from {start}: {used} reads");
    }
}

/// `next_boundary` and `prev_boundary` inside one long cluster read up to
/// its end or its start, with a short step back at each context rule they
/// pass; `next_boundary` steps back at most once.
#[test]
fn queries_inside_a_long_cluster_are_linear_in_the_distance() {
    let marks = format!("a{}", "\u{301}".repeat(32_768));
    let conjunct = format!("{}\u{915}", "\u{915}\u{94D}".repeat(10_922));
    let emoji = format!("{}\u{1F600}", "\u{1F600}\u{200D}".repeat(8_192));
    for (text, per_char) in [(&marks, 1), (&conjunct, 2), (&emoji, 2)] {
        let rope = Rope::from(text.as_str());
        let len = text.len();
        let chars = text.chars().count() as u64;
        let mid = text.floor_char_boundary(len / 2);
        let back = text[..mid].chars().count() as u64;
        let what = |name: &str| format!("{name} on {} chars", chars);
        let next = cheap(
            chars - back + 8,
            &what("next(mid)"),
            || next_boundary(text, mid),
            || rope.next_grapheme_boundary(mid),
        );
        assert_eq!(next, Some(len));
        let prev = cheap(
            per_char * back + 8,
            &what("prev(mid)"),
            || prev_boundary(text, mid),
            || rope.prev_grapheme_boundary(mid),
        );
        assert_eq!(prev, Some(0));
        let prev = cheap(
            per_char * chars + 8,
            &what("prev(len)"),
            || prev_boundary(text, len),
            || rope.prev_grapheme_boundary(len),
        );
        assert_eq!(prev, Some(0));
    }
}

/// The step back stops at a line break: a long run of any kind before CR,
/// LF or CR LF is never read.
#[test]
fn the_step_back_stops_at_a_line_break() {
    let runs = ['\u{1F1E6}', '\u{301}', '\u{94D}', '\u{200D}', '\u{200C}'];
    // Pairs that a context rule decides: GB12, GB9c and GB11.
    let tails = [
        ("\u{1F1E6}\u{1F1E6}", 4),
        ("\u{94D}\u{915}", 3),
        ("\u{200D}\u{1F600}", 3),
    ];
    for c in runs {
        let run = c.to_string().repeat(4_000);
        for line_break in ["\n", "\r", "\r\n"] {
            for (tail, offset) in tails {
                let text = format!("{run}{line_break}{tail}");
                let rope = Rope::from(text.as_str());
                let byte = run.len() + line_break.len() + offset;
                cheap_queries(&text, &rope, byte, 16);
            }
        }
    }
}
