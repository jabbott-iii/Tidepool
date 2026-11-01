//! Unit tests of the rope in the tiny shape (leaves of 4-16 bytes, nodes of
//! 2-4 children), so that short texts already have many leaves and levels.
//!
//! The model test runs 10,000 random edit sequences against a `String`.

use std::sync::Arc;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};

use super::node::{Child, FILL_LEAF, MAX_CHILDREN, MAX_LEAF, MIN_LEAF, Node};
use super::tree::{chunk, cut_point};
use super::{Rope, RopeBuilder, counters};
use crate::pos::tests::{byte_error, expect, probe, range_error, sweep};
use crate::{TextError, Unit};

// ---------------------------------------------------------------------------
// Generators, shared with `pos::tests`.

/// Units of generated text and their weights. An empty unit stands for a
/// random printable ASCII char. The 4-byte chars cover every lead byte,
/// F0 to F4, so that UTF-16 counts meet every supplementary plane's form:
/// U+40000 and U+80000 are unassigned but valid, U+E0100 is a variation
/// selector, U+E0067 a tag, and U+F0000 and U+10FFFF are private use.
const ALPHABET: [(&str, u32); 32] = [
    ("", 29),
    ("\n", 12),
    ("\r", 10),
    ("\r\n", 12),
    ("é", 3),
    ("€", 3),
    ("中", 3),
    ("😀", 2),
    ("𐐀", 1),
    ("🇺🇸", 1),
    ("\u{301}", 2),
    ("\u{200D}", 2),
    ("क", 1),
    ("्", 1),
    ("ᄀ", 1),
    ("ᅡ", 1),
    ("각", 1),
    ("\u{FEFF}", 1),
    ("\0", 1),
    ("\u{85}", 1),
    ("\u{2028}", 1),
    ("\u{B}", 1),
    ("\u{C}", 1),
    ("a", 4),
    (" ", 3),
    ("\u{2029}", 1),
    ("\u{40000}", 1),
    ("\u{80000}", 1),
    ("\u{E0100}", 1),
    ("\u{E0067}", 1),
    ("\u{F0000}", 1),
    ("\u{10FFFF}", 1),
];

/// The sum of the alphabet's weights.
const TOTAL_WEIGHT: u32 = {
    let mut total = 0;
    let mut i = 0;
    while i < ALPHABET.len() {
        total += ALPHABET[i].1;
        i += 1;
    }
    total
};

/// Appends one weighted unit of the alphabet to `out`.
pub(crate) fn push_unit(rng: &mut Pcg32, out: &mut String) {
    let mut pick = rng.range(0..TOTAL_WEIGHT);
    for (unit, weight) in ALPHABET {
        if pick < weight {
            if unit.is_empty() {
                out.push(char::from(rng.range(0x20..0x7Fu32) as u8));
            } else {
                out.push_str(unit);
            }
            return;
        }
        pick -= weight;
    }
}

/// Appends `units` weighted units to `out`.
pub(crate) fn push_text(rng: &mut Pcg32, out: &mut String, units: u32) {
    for _ in 0..units {
        push_unit(rng, out);
    }
}

/// A length in units: 70% 0-8, 20% 9-64, 8% 65-200, 2% 201-512.
fn units(rng: &mut Pcg32) -> u32 {
    match rng.range(0..100u32) {
        0..70 => rng.range(0..9u32),
        70..90 => rng.range(9..65u32),
        90..98 => rng.range(65..201u32),
        _ => rng.range(201..513u32),
    }
}

fn text(rng: &mut Pcg32) -> String {
    let mut out = String::new();
    let count = units(rng);
    push_text(rng, &mut out, count);
    out
}

/// The largest char boundary of `text` at or before `at` (clamped to the
/// end).
pub(crate) fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

// ---------------------------------------------------------------------------
// The model test.

/// Where an operation applies, resolved against the text when it runs.
#[derive(Clone, Copy, Debug)]
enum At {
    Start,
    End,
    /// A fraction of the length (of 2^32).
    Frac(u32),
    /// A chunk boundary of the rope, moved by `delta`.
    Seam {
        index: u32,
        delta: i8,
    },
    /// A CR or LF byte, moved by `delta` (-1..=2): includes the CR|LF offset.
    Break {
        index: u32,
        delta: i8,
    },
    /// `x % (len + 3)`, not snapped to a boundary: often invalid.
    Raw(u32),
}

/// The end of a range: another position, or a short distance past the
/// start.
#[derive(Clone, Copy, Debug)]
enum End {
    At(At),
    Len(u8),
}

#[derive(Debug)]
enum Op {
    Insert(At, String),
    Remove(At, End),
    Replace(At, End, String),
    /// `split_off`, check both parts, append back.
    SplitRejoin(At),
    /// `split_off`, then `right.append(left)`.
    SplitSwap(At),
    Append(String),
    Slice(At, End),
    Snapshot,
    /// Rebuild through `RopeBuilder` from pieces cut at these fractions.
    Rebuild(Vec<u32>),
}

#[derive(Debug)]
struct Step {
    op: Op,
    /// Two positions (fractions of the length) where P1, P3 and P5 are
    /// checked after the operation.
    probes: [u32; 2],
}

#[derive(Debug)]
struct Case {
    initial: String,
    steps: Vec<Step>,
}

fn at(rng: &mut Pcg32) -> At {
    match rng.range(0..100u32) {
        0..4 => At::Start,
        4..12 => At::End,
        12..60 => At::Frac(rng.next_u32()),
        60..75 => At::Seam {
            index: rng.next_u32(),
            delta: rng.range(-2..3) as i8,
        },
        75..90 => At::Break {
            index: rng.next_u32(),
            delta: rng.range(-1..3) as i8,
        },
        _ => At::Raw(rng.next_u32()),
    }
}

fn end(rng: &mut Pcg32) -> End {
    if rng.range(0..2u32) == 0 {
        End::Len(rng.range(0..24u32) as u8)
    } else {
        End::At(at(rng))
    }
}

fn raw(rng: &mut Pcg32) -> At {
    At::Raw(rng.next_u32())
}

fn op(rng: &mut Pcg32) -> Op {
    match rng.range(0..100u32) {
        0..30 => Op::Insert(at(rng), text(rng)),
        30..50 => Op::Remove(at(rng), end(rng)),
        50..65 => Op::Replace(at(rng), end(rng), text(rng)),
        65..70 => Op::SplitRejoin(at(rng)),
        70..75 => Op::SplitSwap(at(rng)),
        75..80 => Op::Append(text(rng)),
        80..85 => Op::Slice(at(rng), end(rng)),
        85..90 => Op::Snapshot,
        90..95 => Op::Rebuild((0..rng.range(0..8u32)).map(|_| rng.next_u32()).collect()),
        // Insert and remove at raw positions.
        95..98 => Op::Insert(raw(rng), text(rng)),
        _ => Op::Remove(raw(rng), End::At(raw(rng))),
    }
}

fn case(rng: &mut Pcg32, size: u32) -> Case {
    let mut initial = String::new();
    push_text(rng, &mut initial, 4 * size);
    let steps = (0..=size)
        .map(|_| Step {
            op: op(rng),
            probes: [rng.next_u32(), rng.next_u32()],
        })
        .collect();
    Case { initial, steps }
}

fn fraction(x: u32, len: usize) -> usize {
    ((u64::from(x) * (len as u64 + 1)) >> 32) as usize
}

/// Resolves `at`; the flag says whether it is raw (unsnapped).
fn resolve(at: At, rope: &Rope, model: &str) -> (usize, bool) {
    let len = model.len();
    let shifted = |base: usize, delta: i8| {
        let moved = base.saturating_add_signed(isize::from(delta)).min(len);
        floor_boundary(model, moved)
    };
    match at {
        At::Start => (0, false),
        At::End => (len, false),
        At::Frac(x) => (floor_boundary(model, fraction(x, len)), false),
        At::Seam { index, delta } => {
            let mut seams = vec![0];
            let mut offset = 0;
            for chunk in rope.chunks() {
                offset += chunk.len();
                seams.push(offset);
            }
            let base = seams[index as usize % seams.len()];
            (shifted(base, delta), false)
        }
        At::Break { index, delta } => {
            let is_break = |byte: &u8| *byte == b'\r' || *byte == b'\n';
            let count = model
                .as_bytes()
                .iter()
                .filter(|byte| is_break(byte))
                .count();
            if count == 0 {
                return (floor_boundary(model, fraction(index, len)), false);
            }
            let wanted = index as usize % count;
            let base = model
                .as_bytes()
                .iter()
                .enumerate()
                .filter(|(_, byte)| is_break(byte))
                .nth(wanted)
                .map_or(0, |(at, _)| at);
            (shifted(base, delta), false)
        }
        At::Raw(x) => (x as usize % (len + 3), true),
    }
}

/// Resolves a range. Snapped ranges are put in order; raw ones are kept as
/// they are, so they may be inverted.
fn resolve_range(start: At, end: End, rope: &Rope, model: &str) -> (usize, usize) {
    let (a, raw_a) = resolve(start, rope, model);
    let (b, raw_b) = match end {
        End::Len(n) if raw_a => (a + usize::from(n), true),
        End::Len(n) => (floor_boundary(model, a + usize::from(n)), false),
        End::At(at) => resolve(at, rope, model),
    };
    if raw_a || raw_b {
        (a, b)
    } else {
        (a.min(b), a.max(b))
    }
}

/// After a refused operation: the rope is the very same tree as before.
fn unchanged(rope: &Rope, before: &Rope, model: &str) -> Result<(), String> {
    if !Arc::ptr_eq(&rope.root, &before.root) || rope.summary != before.summary {
        return Err("a refused operation changed the rope's tree".to_string());
    }
    if *rope != *model {
        return Err("a refused operation changed the text".to_string());
    }
    Ok(())
}

/// The model's chars, UTF-16 units and line breaks, in one pass of `chars`:
/// CR LF is one break, any other CR or LF is one.
fn model_sizes(text: &str) -> (usize, usize, usize) {
    let (mut chars, mut utf16, mut breaks) = (0, 0, 0);
    let mut after_cr = false;
    for c in text.chars() {
        chars += 1;
        utf16 += c.len_utf16();
        if c == '\r' || (c == '\n' && !after_cr) {
            breaks += 1;
        }
        after_cr = c == '\r';
    }
    (chars, utf16, breaks)
}

/// Checks a rope that should hold `text`: invariants, content, and its
/// lengths in bytes, chars, UTF-16 units and lines.
fn holds(rope: &Rope, text: &str, what: &str) -> Result<(), String> {
    rope.__check_invariants()
        .map_err(|e| format!("{what}: invariants: {e}"))?;
    if *rope != *text {
        return Err(format!("{what}: the text differs from the model"));
    }
    let (chars, utf16, breaks) = model_sizes(text);
    let sizes = (
        rope.len(),
        rope.len_in(Unit::Byte),
        rope.len_in(Unit::Char),
        rope.len_in(Unit::Utf16),
        rope.len_lines(),
        rope.is_empty(),
    );
    let want = (
        text.len(),
        text.len(),
        chars,
        utf16,
        breaks + 1,
        text.is_empty(),
    );
    if sizes != want {
        return Err(format!(
            "{what}: (len, bytes, chars, utf16, lines, is_empty) = {sizes:?}, want {want:?}"
        ));
    }
    Ok(())
}

fn apply(
    op: &Op,
    rope: &mut Rope,
    model: &mut String,
    snapshots: &mut Vec<(Rope, String)>,
) -> Result<(), String> {
    let before = rope.clone();
    match op {
        Op::Insert(at, text) => {
            let (p, _) = resolve(*at, rope, model);
            let want = byte_error(model, p);
            let valid = want.is_ok();
            expect(|| format!("insert({p})"), rope.insert(p, text), want)?;
            if valid {
                model.insert_str(p, text);
            } else {
                unchanged(rope, &before, model)?;
            }
        }
        Op::Remove(start, end) | Op::Replace(start, end, _) => {
            let (s, e) = resolve_range(*start, *end, rope, model);
            let want = range_error(model, s, e);
            let valid = want.is_ok();
            let text = match op {
                Op::Replace(_, _, text) => {
                    let got = rope.replace(s..e, text);
                    expect(|| format!("replace({s}..{e})"), got, want)?;
                    text.as_str()
                }
                _ => {
                    expect(|| format!("remove({s}..{e})"), rope.remove(s..e), want)?;
                    ""
                }
            };
            if valid {
                model.replace_range(s..e, text);
            } else {
                unchanged(rope, &before, model)?;
            }
        }
        Op::SplitRejoin(at) | Op::SplitSwap(at) => {
            let (p, _) = resolve(*at, rope, model);
            let want = byte_error(model, p);
            match rope.split_off(p) {
                Ok(mut right) => {
                    if want.is_err() {
                        return Err(format!("split_off({p}) succeeded, want {want:?}"));
                    }
                    holds(rope, &model[..p], "the left part")?;
                    holds(&right, &model[p..], "the right part")?;
                    if matches!(op, Op::SplitSwap(_)) {
                        right.append(std::mem::take(rope));
                        *rope = right;
                        *model = format!("{}{}", &model[p..], &model[..p]);
                    } else {
                        rope.append(right);
                    }
                }
                Err(error) => {
                    expect(|| format!("split_off({p})"), Err::<(), _>(error), want)?;
                    unchanged(rope, &before, model)?;
                }
            }
        }
        Op::Append(text) => {
            rope.append(Rope::from(text.as_str()));
            model.push_str(text);
        }
        Op::Slice(start, end) => {
            let (s, e) = resolve_range(*start, *end, rope, model);
            let want = range_error(model, s, e);
            match rope.slice(s..e) {
                Ok(slice) => {
                    if want.is_err() {
                        return Err(format!("slice({s}..{e}) succeeded, want {want:?}"));
                    }
                    holds(&slice, &model[s..e], "the slice")?;
                }
                Err(error) => expect(|| format!("slice({s}..{e})"), Err::<(), _>(error), want)?,
            }
            unchanged(rope, &before, model)?;
        }
        Op::Snapshot => snapshots.push((rope.clone(), model.clone())),
        Op::Rebuild(fractions) => {
            let mut cuts: Vec<usize> = fractions
                .iter()
                .map(|&x| floor_boundary(model, fraction(x, model.len())))
                .collect();
            // Always try a cut between a CR and its LF, when there is one.
            if let Some(crlf) = model.find("\r\n") {
                cuts.push(crlf + 1);
            }
            cuts.sort_unstable();
            let mut builder = RopeBuilder::new();
            let mut from = 0;
            for cut in cuts {
                builder.push_str(&model[from..cut]);
                from = cut;
            }
            builder.push_str(&model[from..]);
            builder.push_str("");
            *rope = builder.finish();
        }
    }
    Ok(())
}

/// The checks after every operation.
fn check_step(rope: &Rope, model: &str, probes: [u32; 2]) -> Result<(), String> {
    holds(rope, model, "the rope")?;
    for x in probes {
        probe(rope, model, floor_boundary(model, fraction(x, model.len())))?;
    }
    Ok(())
}

fn run(case: &Case) -> Result<(), String> {
    let mut model = case.initial.clone();
    let mut rope = Rope::from(model.as_str());
    holds(&rope, &model, "the initial rope")?;
    let mut snapshots = Vec::new();
    for (index, step) in case.steps.iter().enumerate() {
        apply(&step.op, &mut rope, &mut model, &mut snapshots)
            .and_then(|()| check_step(&rope, &model, step.probes))
            .map_err(|e| format!("step {index}: {e}"))?;
    }
    for (index, (snapshot, text)) in snapshots.iter().enumerate() {
        holds(snapshot, text, &format!("snapshot {index}"))?;
    }
    if model.len() <= 128 {
        // The sweep refuses edits on purpose; keep those out of the count
        // of refusals the model steps caused.
        let before = counters::read()[2];
        let swept = sweep(&rope, &model).map_err(|e| format!("the final sweep: {e}"));
        SWEEP_REFUSALS.set(SWEEP_REFUSALS.get() + counters::read()[2] - before);
        swept?;
    }
    Ok(())
}

thread_local! {
    /// Refused edits made by the final sweeps on this thread.
    static SWEEP_REFUSALS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Acceptance 1: 10,000 random edit sequences against a `String` model,
/// with the invariants, sizes and conversions checked after every step,
/// refused edits shown to change nothing, and both edit paths and the
/// error checks exercised often.
#[test]
fn rope_matches_string_model() {
    let [fast, general, errors] = counters::read();
    let swept = SWEEP_REFUSALS.get();
    check_sized("rope matches a String model", 10_000, case, run);
    let [fast_now, general_now, errors_now] = counters::read();
    let swept = SWEEP_REFUSALS.get() - swept;
    let counts = (
        fast_now - fast,
        general_now - general,
        errors_now - errors - swept,
    );
    assert!(
        counts.0 >= 1_000 && counts.1 >= 1_000 && counts.2 >= 500,
        "(fast-path edits, general-path edits, refused edits) = {counts:?}"
    );
}

// ---------------------------------------------------------------------------
// The tree.

fn rope_of(child: Child) -> Rope {
    Rope::from_child(child)
}

#[test]
fn the_shape_is_tiny_in_unit_tests() {
    assert_eq!(
        (MIN_LEAF, FILL_LEAF, MAX_LEAF, MAX_CHILDREN),
        (4, 12, 16, 4)
    );
}

#[test]
fn the_empty_rope() {
    let rope = Rope::new();
    assert!(rope.is_empty());
    assert_eq!((rope.len(), rope.len_lines()), (0, 1));
    assert_eq!(rope.chunks().count(), 0);
    assert_eq!(rope.chunks().next_back(), None);
    assert_eq!(rope.to_string(), "");
    assert_eq!(rope, Rope::default());
    assert_eq!(rope, Rope::from(""));
    assert_eq!(rope, RopeBuilder::new().finish());
    rope.__check_invariants().expect("valid");
    sweep(&rope, "").expect("sweep");
}

#[test]
fn debug_shows_sizes_and_never_text() {
    let rope = Rope::from("secret 😀\r\n");
    let shown = format!("{rope:?}");
    assert_eq!(shown, "Rope { bytes: 13, chars: 10, utf16: 11, lines: 2 }");
    let big = Rope::from("secret ".repeat(40).as_str());
    assert!(!format!("{big:?}").contains("secret"));
}

#[test]
fn cut_points_avoid_char_middles_and_crlf() {
    let text = "aaaaaaaaaaa\r\nbbbbbbb";
    assert_eq!(cut_point(text, 12), 11);
    assert_eq!(cut_point(text, 13), 13);
    assert_eq!(cut_point(text, 11), 11);
    let text = "aaaaaaaaaa😀bbbbbbbb";
    assert_eq!(cut_point(text, 12), 10);
    assert_eq!(cut_point(text, 14), 14);
    let text = "aaaaaaaa\r😀\nbbbbbbbb";
    assert_eq!(cut_point(text, 10), 9);
}

#[test]
fn chunks_have_the_documented_sizes() {
    let mut rng = Pcg32::new(7, 7);
    for len in 0..120u32 {
        let mut text = String::new();
        push_text(&mut rng, &mut text, len);
        let leaves = chunk(&text);
        let pieces: Vec<&str> = leaves
            .iter()
            .map(|leaf| match &*leaf.node {
                Node::Leaf(piece) => piece.as_str(),
                Node::Internal { .. } => panic!("chunk made an internal node"),
            })
            .collect();
        assert_eq!(pieces.concat(), text);
        assert_eq!(pieces.is_empty(), text.is_empty());
        if text.len() > MAX_LEAF {
            let (last, init) = pieces.split_last().expect("pieces");
            assert!(
                init.iter()
                    .all(|piece| (FILL_LEAF - 4..=FILL_LEAF).contains(&piece.len()))
            );
            assert!((MIN_LEAF + 1..=MAX_LEAF).contains(&last.len()));
        } else {
            assert!(pieces.len() <= 1);
        }
        assert!(
            pieces
                .windows(2)
                .all(|pair| !(pair[0].ends_with('\r') && pair[1].starts_with('\n')))
        );
        if text.len() <= 2 * MAX_LEAF {
            assert!(pieces.len() <= 3);
        }
    }
}

#[test]
fn the_builder_keeps_a_crlf_split_across_pushes_whole() {
    for k in 0..48 {
        let text = format!(
            "{}\r\n{}\r{}",
            "a".repeat(k),
            "b".repeat(k),
            "\n".repeat(k % 3)
        );
        for cut in (0..=text.len()).filter(|&cut| text.is_char_boundary(cut)) {
            let mut builder = RopeBuilder::new();
            builder.push_str(&text[..cut]);
            builder.push_str("");
            builder.push_str(&text[cut..]);
            let rope = builder.finish();
            holds(&rope, &text, &format!("k {k}, cut {cut}")).unwrap_or_else(|e| panic!("{e}"));
        }
        let mut builder = RopeBuilder::new();
        for c in text.chars() {
            builder.push_str(c.encode_utf8(&mut [0; 4]));
            builder.push_str("");
        }
        holds(&builder.finish(), &text, "pushed char by char").unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn the_builder_handles_long_pieces_and_short_tails() {
    let mut rng = Pcg32::new(3, 9);
    for len in [0, 1, 3, 15, 16, 17, 28, 29, 31, 32, 33, 100, 1_000, 5_000] {
        let mut text = String::new();
        while text.len() < len {
            push_unit(&mut rng, &mut text);
        }
        let mut builder = RopeBuilder::new();
        builder.push_str(&text);
        holds(&builder.finish(), &text, "one push").unwrap_or_else(|e| panic!("{e}"));
        holds(&Rope::from(text.as_str()), &text, "From<&str>").unwrap_or_else(|e| panic!("{e}"));
        holds(&Rope::from(text.clone()), &text, "From<String>").unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn append_repairs_a_crlf_seam_at_every_size_and_height() {
    for left_len in 1..=70 {
        for right_len in (1..=70).step_by(3) {
            let left_text = format!("{}\r", "x".repeat(left_len - 1));
            let right_text = format!("\n{}", "y".repeat(right_len - 1));
            let mut left = Rope::from(left_text.as_str());
            let right = Rope::from(right_text.as_str());
            assert_eq!((left.len_lines(), right.len_lines()), (2, 2));
            left.append(right);
            let text = format!("{left_text}{right_text}");
            holds(&left, &text, &format!("{left_len} + {right_len}"))
                .unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(left.len_lines(), 2, "{left_len} + {right_len}");
            assert_eq!(left.line_to_byte(1).ok(), Some(left_len + 1));
        }
    }
}

#[test]
fn appending_trees_of_different_heights_keeps_the_invariants() {
    let mut rng = Pcg32::new(11, 4);
    for _ in 0..300 {
        let (mut a, mut b) = (String::new(), String::new());
        let (a_units, b_units) = (rng.range(0..200u32), rng.range(0..200u32));
        push_text(&mut rng, &mut a, a_units);
        push_text(&mut rng, &mut b, b_units);
        if rng.range(0..2u32) == 0 {
            a.push('\r');
            b.insert(0, '\n');
        }
        let mut rope = Rope::from(a.as_str());
        rope.append(Rope::from(b.as_str()));
        holds(&rope, &format!("{a}{b}"), "a + b").unwrap_or_else(|e| panic!("{e}"));
        let mut rope = Rope::from(b.as_str());
        rope.append(Rope::from(a.as_str()));
        holds(&rope, &format!("{b}{a}"), "b + a").unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn split_off_and_slice_at_every_position() {
    let mut rng = Pcg32::new(5, 5);
    let mut text = String::new();
    push_text(&mut rng, &mut text, 120);
    text.push_str("\r\n\r\r\n");
    let rope = Rope::from(text.as_str());
    let boundaries: Vec<usize> = (0..=text.len())
        .filter(|&at| text.is_char_boundary(at))
        .collect();
    for &at in &boundaries {
        let mut left = rope.clone();
        let right = left.split_off(at).expect("valid");
        holds(&left, &text[..at], "left").unwrap_or_else(|e| panic!("at {at}: {e}"));
        holds(&right, &text[at..], "right").unwrap_or_else(|e| panic!("at {at}: {e}"));
        left.append(right);
        holds(&left, &text, "rejoined").unwrap_or_else(|e| panic!("at {at}: {e}"));
        holds(&rope, &text, "the original").unwrap_or_else(|e| panic!("at {at}: {e}"));
    }
    for (i, &start) in boundaries.iter().enumerate().step_by(5) {
        for &end in boundaries[i..].iter().step_by(7) {
            let slice = rope.slice(start..end).expect("valid");
            holds(&slice, &text[start..end], "slice")
                .unwrap_or_else(|e| panic!("{start}..{end}: {e}"));
        }
    }
}

#[test]
fn a_snapshot_is_unchanged_by_edits_and_shares_untouched_nodes() {
    let text = "0123456789".repeat(30);
    let mut rope = Rope::from(text.as_str());
    let snapshot = rope.clone();
    let [fast, ..] = counters::read();
    rope.insert(150, "x").expect("valid");
    assert_eq!(
        counters::read()[0],
        fast + 1,
        "a one-byte insert takes the fast path"
    );
    let mut edited = text.clone();
    edited.insert(150, 'x');
    holds(&rope, &edited, "edited").unwrap_or_else(|e| panic!("{e}"));
    holds(&snapshot, &text, "snapshot").unwrap_or_else(|e| panic!("{e}"));
    assert!(!Arc::ptr_eq(&rope.root, &snapshot.root));
    let (Node::Internal { children: new, .. }, Node::Internal { children: old, .. }) =
        (&*rope.root, &*snapshot.root)
    else {
        panic!("300 bytes make an internal root");
    };
    let shared = new
        .iter()
        .zip(old)
        .filter(|(a, b)| Arc::ptr_eq(&a.node, &b.node))
        .count();
    assert_eq!(shared, new.len() - 1, "only the edited path is copied");

    // A general-path edit leaves the snapshot alone too.
    rope.replace(3..290, "\r\n").expect("valid");
    holds(&snapshot, &text, "snapshot").unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn edits_that_would_split_a_crlf_take_the_general_path() {
    // The builder cuts after 12 bytes, so leaf 1 starts with the LF.
    let text = format!("{}\n{}", "a".repeat(12), "b".repeat(20));
    let mut rope = Rope::from(text.as_str());
    let [fast, general, _] = counters::read();
    rope.insert(12, "\r").expect("valid");
    assert_eq!(counters::read()[..2], [fast, general + 1]);
    let text = format!("{}\r\n{}", "a".repeat(12), "b".repeat(20));
    holds(&rope, &text, "CR before the LF").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(rope.len_lines(), 2);

    // Removing what separates a CR from an LF in the next leaf.
    let text = format!("{}\rx\n{}", "a".repeat(11), "b".repeat(20));
    let mut rope = Rope::from(text.as_str());
    let [_, general, _] = counters::read();
    rope.remove(12..13).expect("valid");
    assert_eq!(counters::read()[1], general + 1);
    let text = format!("{}\r\n{}", "a".repeat(11), "b".repeat(20));
    holds(&rope, &text, "LF after the CR").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(rope.len_lines(), 2);
}

/// `start..end` with `start > end`, on purpose.
fn inverted(start: usize, end: usize) -> std::ops::Range<usize> {
    start..end
}

#[test]
fn refused_edits_change_nothing() {
    let text = "a😀\r\nb";
    let mut rope = Rope::from(text);
    let before = rope.clone();
    assert!(matches!(
        rope.insert(99, "x"),
        Err(TextError::ByteOutOfBounds { byte: 99, len: 8 })
    ));
    assert!(matches!(
        rope.insert(2, "x"),
        Err(TextError::NotCharBoundary { byte: 2 })
    ));
    assert!(matches!(
        rope.remove(inverted(5, 1)),
        Err(TextError::InvertedRange { start: 5, end: 1 })
    ));
    assert!(matches!(
        rope.remove(9..10),
        Err(TextError::ByteOutOfBounds { byte: 10, len: 8 })
    ));
    assert!(matches!(
        rope.replace(2..3, "x"),
        Err(TextError::NotCharBoundary { byte: 2 })
    ));
    assert!(matches!(
        rope.replace(1..3, "x"),
        Err(TextError::NotCharBoundary { byte: 3 })
    ));
    assert!(matches!(
        rope.split_off(4),
        Err(TextError::NotCharBoundary { byte: 4 })
    ));
    assert!(matches!(
        rope.slice(0..9),
        Err(TextError::ByteOutOfBounds { byte: 9, len: 8 })
    ));
    assert!(rope.chunks_in(inverted(3, 1)).is_err());
    assert!(rope.chars_in(0..2).is_err());
    assert!(rope.char_indices_in(2..8).is_err());
    unchanged(&rope, &before, text).unwrap_or_else(|e| panic!("{e}"));
    // A CR|LF offset is a valid position for edits.
    rope.insert(6, "x").expect("between CR and LF");
    assert_eq!(rope, "a😀\rx\nb");
    // An empty replacement of an empty range is a no-op.
    let before = rope.clone();
    rope.replace(1..1, "").expect("valid");
    assert!(Arc::ptr_eq(&rope.root, &before.root));
}

#[test]
fn iterators_work_from_both_ends() {
    let text = "ab\r\n😀中é\u{301}\r\rxyz\n".repeat(5);
    let rope = Rope::from(text.as_str());
    let chunks: Vec<&str> = rope.chunks().collect();
    assert!(chunks.len() > 5);
    let mut back: Vec<&str> = rope.chunks().rev().collect();
    back.reverse();
    assert_eq!(back, chunks);
    let chars: String = rope.chars_in(0..text.len()).expect("valid").rev().collect();
    assert_eq!(chars, text.chars().rev().collect::<String>());
    let pairs: Vec<(usize, char)> = rope.char_indices_in(4..40).expect("valid").collect();
    let want: Vec<(usize, char)> = text[4..40]
        .char_indices()
        .map(|(at, c)| (at + 4, c))
        .collect();
    assert_eq!(pairs, want);
    let mut iter = rope.chunks_in(4..4).expect("valid");
    assert_eq!((iter.next(), iter.next_back()), (None, None));
    let clipped: String = rope.chunks_in(3..text.len() - 2).expect("valid").collect();
    assert_eq!(clipped, text[3..text.len() - 2]);
    // Clones iterate independently.
    let mut first = rope.chars_in(0..11).expect("valid");
    first.next();
    let second = first.clone();
    assert_eq!(first.collect::<String>(), second.collect::<String>());
}

/// For each step of an iteration, whether to take the next chunk from the
/// front rather than the back.
type Order = fn(usize) -> bool;

/// The orders `chunks_in_every_order` takes chunks in, by name.
const ORDERS: [(&str, Order); 5] = [
    ("forward", |_| true),
    ("backward", |_| false),
    ("front first, alternating", |step| step % 2 == 0),
    ("back first, alternating", |step| step % 2 == 1),
    ("one from the front, two from the back", |step| {
        step % 3 == 0
    }),
];

/// Takes the chunks of `start..end` in each of [`ORDERS`]: every chunk must
/// be non-empty, they must spell `text[start..end]`, and the iterator must
/// stay empty at both ends once it is done.
fn chunks_in_every_order(rope: &Rope, text: &str, start: usize, end: usize) -> Result<(), String> {
    for (order, from_front) in ORDERS {
        let mut chunks = rope
            .chunks_in(start..end)
            .map_err(|e| format!("chunks_in({start}..{end}): {e}"))?;
        let (mut head, mut tail) = (Vec::new(), Vec::new());
        let mut step = 0;
        loop {
            let front = from_front(step);
            let chunk = if front {
                chunks.next()
            } else {
                chunks.next_back()
            };
            match chunk {
                None => break,
                Some("") => {
                    return Err(format!(
                        "{order}: chunks_in({start}..{end}) gave an empty chunk at step {step}"
                    ));
                }
                Some(chunk) if front => head.push(chunk),
                Some(chunk) => tail.push(chunk),
            }
            step += 1;
        }
        if chunks.next().is_some() || chunks.next_back().is_some() {
            return Err(format!("{order}: chunks_in({start}..{end}) restarted"));
        }
        tail.reverse();
        head.extend(tail);
        if head.concat() != text[start..end] {
            return Err(format!(
                "{order}: the chunks of {start}..{end} do not spell the range"
            ));
        }
    }
    Ok(())
}

/// The chunk seams of `rope`, from 0 to its length.
fn seams(rope: &Rope) -> Vec<usize> {
    let mut seams = vec![0];
    let mut offset = 0;
    for chunk in rope.chunks() {
        offset += chunk.len();
        seams.push(offset);
    }
    seams
}

/// `chunks_in` never yields an empty chunk, from either end or in mixed
/// orders, for ranges that start and end at every chunk seam and the bytes
/// either side of it, on ropes built in three ways.
#[test]
fn chunks_in_never_yields_an_empty_chunk() {
    check(
        "chunks_in never yields an empty chunk",
        150,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let mut text = String::new();
            let units = rng.range(0..160u32);
            push_text(&mut rng, &mut text, units);
            let rope = match rng.range(0..3u32) {
                0 => Rope::from(text.as_str()),
                1 => {
                    let mut rope = Rope::new();
                    let mut from = 0;
                    while from < text.len() {
                        let to = text.ceil_char_boundary(from + rng.range(1..30u32) as usize);
                        rope.append(Rope::from(&text[from..to]));
                        from = to;
                    }
                    rope
                }
                _ => {
                    let mut builder = RopeBuilder::new();
                    let mut from = 0;
                    while from < text.len() {
                        let to = text.ceil_char_boundary(from + rng.range(0..20u32) as usize);
                        builder.push_str(&text[from..to]);
                        from = to;
                    }
                    builder.finish()
                }
            };
            holds(&rope, &text, "the rope")?;
            let mut ends: Vec<usize> = seams(&rope)
                .into_iter()
                .flat_map(|seam| [seam.saturating_sub(1), seam, seam + 1])
                .filter(|&byte| text.is_char_boundary(byte))
                .collect();
            ends.sort_unstable();
            ends.dedup();
            for (i, &start) in ends.iter().enumerate() {
                for &end in &ends[i..] {
                    chunks_in_every_order(&rope, &text, start, end)?;
                }
            }
            Ok(())
        },
    );
}

#[test]
fn equality_compares_text_whatever_the_shape() {
    let text = "the quick\r\nbrown 🦊 jumps\rover\n".repeat(4);
    let whole = Rope::from(text.as_str());
    let mut pieces = RopeBuilder::new();
    for c in text.chars() {
        pieces.push_str(c.encode_utf8(&mut [0; 4]));
    }
    let pieces = pieces.finish();
    let mut appended = Rope::new();
    for part in text.split_inclusive('\n') {
        appended.append(Rope::from(part));
    }
    assert_eq!(whole, pieces);
    assert_eq!(whole, appended);
    assert_eq!(whole, text);
    assert_eq!(whole, text.as_str());
    assert_eq!(whole, *text.as_str());
    assert_eq!(whole.to_string(), text);
    let mut different = whole.clone();
    different.replace(5..6, "Q").expect("valid");
    assert_ne!(whole, different);
    assert_ne!(different, text);
    assert_ne!(whole, Rope::from(&text[1..]));
}

#[test]
fn boundary_queries_are_total() {
    let rope = Rope::from("a😀\r\nb€");
    let len = rope.len();
    for byte in [usize::MAX, len + 1, len + 100] {
        assert!(!rope.is_char_boundary(byte));
        assert_eq!(rope.floor_char_boundary(byte), len);
        assert_eq!(rope.ceil_char_boundary(byte), len);
    }
    assert_eq!(rope.floor_char_boundary(3), 1);
    assert_eq!(rope.ceil_char_boundary(3), 5);
    assert!(rope.is_char_boundary(6));
}

#[test]
fn ropes_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Rope>();
    assert_send_sync::<RopeBuilder>();
    assert_send_sync::<super::Chunks<'_>>();
    assert_send_sync::<super::Chars<'_>>();
    assert_send_sync::<super::CharIndices<'_>>();
}

// ---------------------------------------------------------------------------
// The invariant checker catches broken trees, without showing their text.

fn leaf(text: &str) -> Child {
    Child::leaf(text)
}

fn broken(rope: &Rope, what: &str) {
    match rope.__check_invariants() {
        Ok(()) => panic!("the checker missed: {what}"),
        Err(message) => assert!(
            !message.contains("SECRET"),
            "{what}: the message shows text: {message}"
        ),
    }
}

#[test]
fn the_checker_finds_every_kind_of_broken_tree() {
    let good = Child::internal(1, vec![leaf("SECRETab"), leaf("SECRETcd")]);
    rope_of(good.clone())
        .__check_invariants()
        .expect("a valid tree");

    let mut wrong_child = good.clone();
    if let Node::Internal { children, .. } = Arc::make_mut(&mut wrong_child.node) {
        children[1].summary.utf16 += 1;
    }
    broken(&rope_of(wrong_child), "a wrong child summary");

    let mut wrong_rope = rope_of(good.clone());
    wrong_rope.summary.breaks += 1;
    broken(&wrong_rope, "a wrong rope summary");

    let seam = Child::internal(1, vec![leaf("SECRET\r"), leaf("\nSECRET")]);
    broken(&rope_of(seam), "a CR LF split between leaves");

    let short = Child::internal(1, vec![leaf("SEC"), leaf("SECRETxy")]);
    broken(&rope_of(short), "a non-root leaf under MIN_LEAF");

    broken(
        &rope_of(leaf(&"SECRET".repeat(3))),
        "a root leaf over MAX_LEAF",
    );

    let one_child = Child::internal(1, vec![leaf("SECRETab")]);
    broken(&rope_of(one_child), "an internal root with one child");

    let crowded = Child::internal(1, (0..5).map(|_| leaf("SECRET")).collect());
    broken(&rope_of(crowded), "too many children");

    let tall = Child::internal(2, vec![leaf("SECRETab"), leaf("SECRETcd")]);
    broken(&rope_of(tall), "a child at the wrong height");

    let flat = Child::internal(0, vec![leaf("SECRETab"), leaf("SECRETcd")]);
    broken(&rope_of(flat), "an internal node of height 0");

    let thin = Child::internal(
        2,
        vec![
            Child::internal(1, vec![leaf("SECRETab")]),
            Child::internal(1, vec![leaf("SECRETcd"), leaf("SECRETef")]),
        ],
    );
    broken(&rope_of(thin), "a non-root node under MIN_CHILDREN");
}
