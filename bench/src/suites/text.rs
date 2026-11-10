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

//! The suites of `tidepool-text`: what roadmap M1.1 and M1.2 measured by
//! hand (their notes in `docs/roadmap.md`), on inputs built the same way on
//! every run.
//!
//! - `text/rope/…`: inserting one char, and removing one line's text, at
//!   1,024 places drawn from a seeded [`Pcg32`], on 400 KB and on 100 MB.
//! - `text/pos/…`: a byte offset to its line and column, and back, in each
//!   [`Unit`], at 1,024 drawn char boundaries of 100 MB.
//! - `text/history/…`: a keystroke at 10,000 carets, made by
//!   [`Transaction::change_each`] and committed through [`History`], on
//!   400 KB and on 100 MB; its undo and its redo on 400 KB; and typing one
//!   more char into an open typing group in 100 MB.
//! - `text/grapheme/…`: M1.1's crafted 100 MB lines. One cluster of 100 MB,
//!   which `next_grapheme_boundary` crosses from its start; a consonant
//!   after 100 MB of combining marks, where `is_grapheme_boundary` steps
//!   back over the marks (GB9c); and 100 MB of regional indicators, where
//!   it steps back over the run (GB12 and GB13).
//!
//! The 400 KB text is the 10,000 lines of `text`'s own test of 10,000
//! cursors (`crates/text/tests/edits.rs`), with an `é` and an emoji on each
//! line and every seventh line ending in CR LF; the 100 MB text is that
//! text 250 times. A routine that changes its text starts each iteration
//! from the same state, an O(1) clone of the rope and a clone of the
//! history, made outside the timing. Each suite checks, untimed, that its
//! routine does what it should before it is measured, or, for the slow
//! grapheme queries, checks every answer.

use std::fmt::Write as _;
use std::ops::Range;
use std::time::{Duration, Instant};

use tidepool_core::Pcg32;
use tidepool_text::{
    Cursor, Group, History, LineCol, Place, Rope, RopeBuilder, Selection, Step, TextError,
    Transaction, Unit,
};

use crate::{BenchError, Bencher, Suite};

/// The text suites, in the order a run takes them.
pub const SUITES: &[Suite] = &[
    Suite {
        name: "text/rope/insert/400kb",
        run: |b| rope_insert(b, &Text::small().rope, 1),
    },
    Suite {
        name: "text/rope/insert/100mb",
        run: |b| rope_insert(b, &Text::big().rope, 2),
    },
    Suite {
        name: "text/rope/remove_line/400kb",
        run: |b| rope_remove_line(b, &Text::small().rope, 3),
    },
    Suite {
        name: "text/rope/remove_line/100mb",
        run: |b| rope_remove_line(b, &Text::big().rope, 4),
    },
    Suite {
        name: "text/pos/byte_to_line_col/byte/100mb",
        run: |b| byte_to_line_col(b, &Text::big().rope, Unit::Byte, 5),
    },
    Suite {
        name: "text/pos/byte_to_line_col/utf16/100mb",
        run: |b| byte_to_line_col(b, &Text::big().rope, Unit::Utf16, 6),
    },
    Suite {
        name: "text/pos/byte_to_line_col/char/100mb",
        run: |b| byte_to_line_col(b, &Text::big().rope, Unit::Char, 7),
    },
    Suite {
        name: "text/pos/line_col_to_byte/byte/100mb",
        run: |b| line_col_to_byte(b, &Text::big().rope, Unit::Byte, 8),
    },
    Suite {
        name: "text/pos/line_col_to_byte/utf16/100mb",
        run: |b| line_col_to_byte(b, &Text::big().rope, Unit::Utf16, 9),
    },
    Suite {
        name: "text/pos/line_col_to_byte/char/100mb",
        run: |b| line_col_to_byte(b, &Text::big().rope, Unit::Char, 10),
    },
    Suite {
        name: "text/history/keystroke_10k_carets/400kb",
        run: |b| keystroke(b, &Text::small()),
    },
    Suite {
        name: "text/history/undo_10k_carets/400kb",
        run: |b| undo(b, &Text::small()),
    },
    Suite {
        name: "text/history/redo_10k_carets/400kb",
        run: |b| redo(b, &Text::small()),
    },
    Suite {
        name: "text/history/keystroke_10k_carets/100mb",
        run: |b| keystroke(b, &Text::big()),
    },
    Suite {
        name: "text/history/type/100mb",
        run: |b| typing(b, &Text::big()),
    },
    Suite {
        name: "text/grapheme/one_cluster/100mb",
        run: |b| one_cluster(b, BIG_MARKS),
    },
    Suite {
        name: "text/grapheme/combining_marks/100mb",
        run: |b| combining_marks(b, BIG_MARKS),
    },
    Suite {
        name: "text/grapheme/regional_indicators/100mb",
        run: |b| regional_indicators(b, BIG_INDICATORS),
    },
];

/// The seed of every draw; each suite has its own stream.
const SEED: u64 = 0x7469_6465_706f_6f6c; // "tidepool"

/// The lines of the 400 KB text.
const LINES: usize = 10_000;

/// The carets of a 10,000-caret keystroke.
const CARETS: usize = 10_000;

/// The size the 100 MB texts reach or pass, in bytes.
const BIG_BYTES: usize = 100_000_000;

/// How many places a rope or position suite draws, and cycles through.
const PLACES: usize = 1_024;

/// U+0301 COMBINING ACUTE ACCENT, two bytes, Extend (and InCB Extend).
const MARK: &str = "\u{301}";

/// U+1F1E6 REGIONAL INDICATOR SYMBOL LETTER A, four bytes.
const INDICATOR: &str = "\u{1F1E6}";

/// The marks that make a grapheme line 100 MB.
const BIG_MARKS: usize = BIG_BYTES / MARK.len();

/// The regional indicators that make a grapheme line 100 MB.
const BIG_INDICATORS: usize = BIG_BYTES / INDICATOR.len();

/// The [`Group::Coalesce`] kind of typing.
const TYPING: u32 = 1;

/// The 10,000 lines, and where each one's text ends.
struct Lines {
    text: String,
    ends: Vec<usize>,
}

impl Lines {
    fn new() -> Self {
        let mut text = String::with_capacity(410_000);
        let mut ends = Vec::with_capacity(LINES);
        for i in 0..LINES {
            // Writing to a `String` cannot fail.
            let _ = write!(text, "line {i:05}: the quick brown fox é 😀");
            ends.push(text.len());
            text.push_str(if i % 7 == 3 { "\r\n" } else { "\n" });
        }
        Self { text, ends }
    }
}

/// The lines, `copies` times over, as a rope.
struct Text {
    rope: Rope,
    lines: Lines,
    copies: usize,
}

impl Text {
    /// The 400 KB text: the lines once.
    fn small() -> Self {
        Self::with_copies(1)
    }

    /// The 100 MB text: the lines as often as it takes to reach
    /// [`BIG_BYTES`], 250 times.
    fn big() -> Self {
        let lines = Lines::new();
        let copies = BIG_BYTES.div_ceil(lines.text.len().max(1));
        Self::of(lines, copies)
    }

    fn with_copies(copies: usize) -> Self {
        Self::of(Lines::new(), copies)
    }

    fn of(lines: Lines, copies: usize) -> Self {
        let mut builder = RopeBuilder::new();
        for _ in 0..copies {
            builder.push_str(&lines.text);
        }
        Self {
            rope: builder.finish(),
            lines,
            copies,
        }
    }

    /// Where the text of `line`, counted over all the copies, ends.
    fn end_of(&self, line: usize) -> usize {
        let copy = line / LINES;
        copy * self.lines.text.len() + self.lines.ends[line % LINES]
    }

    /// [`CARETS`] carets at the ends of lines spread evenly over the text:
    /// every line of the 400 KB text, every 250th of the 100 MB one.
    fn carets(&self) -> Vec<usize> {
        let step = (LINES * self.copies / CARETS).max(1);
        (0..CARETS).map(|k| self.end_of(k * step)).collect()
    }
}

/// [`PLACES`] char boundaries of `rope`, drawn from stream `stream`.
fn places(rope: &Rope, stream: u64) -> Vec<usize> {
    let mut rng = Pcg32::new(SEED, stream);
    let bound = u32::try_from(rope.len()).unwrap_or(u32::MAX).max(1);
    (0..PLACES)
        .map(|_| rope.floor_char_boundary(rng.range(0..bound) as usize))
        .collect()
}

/// A check of what a routine did, before it is measured.
fn check(holds: bool, what: &str) -> Result<(), BenchError> {
    if holds {
        Ok(())
    } else {
        Err(BenchError::Check(format!("its check failed: {what}")))
    }
}

/// Inserting `x` at drawn places of `rope`.
fn rope_insert(b: &mut Bencher, rope: &Rope, stream: u64) -> Result<(), BenchError> {
    let at = places(rope, stream);
    for &place in &at {
        let mut text = rope.clone();
        text.insert(place, "x")?;
        check(text.len() == rope.len() + 1, "an insert added one byte")?;
    }
    let mut next = 0;
    b.iter_batched(
        || {
            let place = at[next % at.len()];
            next += 1;
            (rope.clone(), place)
        },
        |(mut text, place)| {
            let result = text.insert(place, "x");
            (text, result)
        },
    );
    Ok(())
}

/// Removing the text of drawn lines of `rope`, about 40 bytes each.
fn rope_remove_line(b: &mut Bencher, rope: &Rope, stream: u64) -> Result<(), BenchError> {
    let mut rng = Pcg32::new(SEED, stream);
    let bound = u32::try_from(rope.len_lines()).unwrap_or(u32::MAX).max(1);
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(PLACES);
    for _ in 0..PLACES {
        let range = rope.line_span(rng.range(0..bound) as usize)?.content();
        let mut text = rope.clone();
        text.remove(range.clone())?;
        check(
            text.len() + range.len() == rope.len(),
            "a removal took the line's text",
        )?;
        ranges.push(range);
    }
    let mut next = 0;
    b.iter_batched(
        || {
            let range = ranges[next % ranges.len()].clone();
            next += 1;
            (rope.clone(), range)
        },
        |(mut text, range)| {
            let result = text.remove(range);
            (text, result)
        },
    );
    Ok(())
}

/// A byte offset's line and column in `unit`, at drawn places of `rope`.
fn byte_to_line_col(
    b: &mut Bencher,
    rope: &Rope,
    unit: Unit,
    stream: u64,
) -> Result<(), BenchError> {
    let bytes = places(rope, stream);
    for &byte in &bytes {
        let pos = rope.byte_to_line_col(byte, unit)?;
        check(
            rope.line_col_to_byte(pos, unit)? == byte,
            "a position converted back to its byte",
        )?;
    }
    let mut next = 0;
    b.iter(|| {
        let byte = bytes[next];
        next = (next + 1) % bytes.len();
        rope.byte_to_line_col(byte, unit)
    });
    Ok(())
}

/// A line and column in `unit` to its byte offset, at drawn places of
/// `rope`.
fn line_col_to_byte(
    b: &mut Bencher,
    rope: &Rope,
    unit: Unit,
    stream: u64,
) -> Result<(), BenchError> {
    let bytes = places(rope, stream);
    let mut positions: Vec<LineCol> = Vec::with_capacity(bytes.len());
    for &byte in &bytes {
        let pos = rope.byte_to_line_col(byte, unit)?;
        check(
            rope.line_col_to_byte(pos, unit)? == byte,
            "a position converted back to its byte",
        )?;
        positions.push(pos);
    }
    let mut next = 0;
    b.iter(|| {
        let pos = positions[next];
        next = (next + 1) % positions.len();
        rope.line_col_to_byte(pos, unit)
    });
    Ok(())
}

/// The carets of `text`, the middle one primary.
fn caret_selection(text: &Text) -> Selection {
    let carets: Vec<Cursor> = text.carets().into_iter().map(Cursor::caret).collect();
    let primary = carets
        .get(carets.len() / 2)
        .copied()
        .unwrap_or(Cursor::caret(0));
    // The primary is listed again among the others; equal carets merge.
    Selection::new(primary, carets)
}

/// Types `x` at every cursor of `selection` in `text`, committed through
/// `history` at `now` in `group`: one keystroke.
fn type_x(
    text: &mut Rope,
    history: &mut History,
    selection: &Selection,
    now: Instant,
    group: Group,
) -> Result<Step, TextError> {
    let x = Rope::from("x");
    let typed = Transaction::change_each(text, selection, |cursor| {
        Some((cursor.head..cursor.head, x.clone(), Place::End))
    })?;
    history.commit(text, typed, selection, now, group)
}

/// A keystroke at 10,000 carets of `text`, from the same start each time:
/// the text and a new history.
fn keystroke(b: &mut Bencher, text: &Text) -> Result<(), BenchError> {
    let selection = caret_selection(text);
    let rope = &text.rope;
    let history = History::new(rope);
    let now = Instant::now();
    let (mut after, mut probe) = (rope.clone(), history.clone());
    let step = type_x(&mut after, &mut probe, &selection, now, Group::New)?;
    check(
        after.len() == rope.len() + CARETS && step.changes.len() == 1,
        "a keystroke typed at every caret",
    )?;
    b.iter_batched(
        || (rope.clone(), history.clone()),
        |(mut text, mut history)| {
            let step = type_x(&mut text, &mut history, &selection, now, Group::New);
            (text, history, step)
        },
    );
    Ok(())
}

/// The text and history after one keystroke at 10,000 carets of `text`.
fn after_keystroke(text: &Text) -> Result<(Rope, History), BenchError> {
    let selection = caret_selection(text);
    let mut after = text.rope.clone();
    let mut history = History::new(&after);
    type_x(
        &mut after,
        &mut history,
        &selection,
        Instant::now(),
        Group::New,
    )?;
    Ok((after, history))
}

/// Undoing a keystroke at 10,000 carets of `text`.
fn undo(b: &mut Bencher, text: &Text) -> Result<(), BenchError> {
    let (after, history) = after_keystroke(text)?;
    let (mut undone, mut probe) = (after.clone(), history.clone());
    let step = probe.undo(&mut undone)?;
    check(
        step.is_some() && undone == text.rope,
        "the undo restored the text",
    )?;
    b.iter_batched(
        || (after.clone(), history.clone()),
        |(mut text, mut history)| {
            let step = history.undo(&mut text);
            (text, history, step)
        },
    );
    Ok(())
}

/// Redoing a keystroke at 10,000 carets of `text`.
fn redo(b: &mut Bencher, text: &Text) -> Result<(), BenchError> {
    let (after, mut history) = after_keystroke(text)?;
    let mut undone = after.clone();
    history.undo(&mut undone)?;
    let (mut redone, mut probe) = (undone.clone(), history.clone());
    let step = probe.redo(&mut redone)?;
    check(
        step.is_some() && redone == after,
        "the redo made the text again",
    )?;
    b.iter_batched(
        || (undone.clone(), history.clone()),
        |(mut text, mut history)| {
            let step = history.redo(&mut text);
            (text, history, step)
        },
    );
    Ok(())
}

/// Typing one more char at a caret in the middle of `text`, into the
/// open typing group a first char started: the steady state of typing.
fn typing(b: &mut Bencher, text: &Text) -> Result<(), BenchError> {
    let rope = &text.rope;
    let caret = text.end_of(LINES * text.copies / 2);
    // One instant for every commit, so each joins the group whatever the
    // time between samples.
    let now = Instant::now();
    let group = Group::Coalesce {
        kind: TYPING,
        window: Duration::from_secs(1),
    };
    let mut start = rope.clone();
    let mut history = History::new(&start);
    let first = type_x(
        &mut start,
        &mut history,
        &Selection::caret(caret),
        now,
        group,
    )?;
    let selection = first.selection;
    let (mut typed, mut probe) = (start.clone(), history.clone());
    type_x(&mut typed, &mut probe, &selection, now, group)?;
    check(
        typed.len() == rope.len() + 2 && probe.revision_count() == 2,
        "the second char joined the first's revision",
    )?;
    b.iter_batched(
        || (start.clone(), history.clone()),
        |(mut text, mut history)| {
            let step = type_x(&mut text, &mut history, &selection, now, group);
            (text, history, step)
        },
    );
    Ok(())
}

/// `head`, then `unit` `count` times, then `tail`, built without a string
/// of the whole.
fn repeated(head: &str, unit: &str, count: usize, tail: &str) -> Rope {
    const CHUNK: usize = 16 * 1024;
    let chunk = unit.repeat(CHUNK);
    let mut builder = RopeBuilder::new();
    builder.push_str(head);
    let mut left = count;
    while left >= CHUNK {
        builder.push_str(&chunk);
        left -= CHUNK;
    }
    builder.push_str(&unit.repeat(left));
    builder.push_str(tail);
    builder.finish()
}

/// Measures `query` on `rope`, checking every answer against `want`: the
/// queries are slow enough that the check costs nothing.
fn grapheme_query<T: PartialEq + Copy + std::fmt::Debug>(
    b: &mut Bencher,
    rope: &Rope,
    query: impl Fn(&Rope) -> T,
    want: T,
) -> Result<(), BenchError> {
    let mut wrong = None;
    b.iter(|| {
        let got = query(rope);
        if got != want {
            wrong = Some(got);
        }
        got
    });
    match wrong {
        Some(got) => Err(BenchError::Check(format!(
            "its check failed: the query gave {got:?}, not {want:?}"
        ))),
        None => Ok(()),
    }
}

/// `a` and `marks` combining marks: one cluster, which the next boundary
/// from its start crosses whole.
fn one_cluster(b: &mut Bencher, marks: usize) -> Result<(), BenchError> {
    let rope = repeated("a", MARK, marks, "");
    let len = rope.len();
    grapheme_query(b, &rope, |rope| rope.next_grapheme_boundary(0), Some(len))
}

/// KA, `marks` combining marks, a virama and KA: one conjunct cluster
/// (GB9c). Whether there is a boundary before the last KA steps back over
/// every mark.
fn combining_marks(b: &mut Bencher, marks: usize) -> Result<(), BenchError> {
    let rope = repeated("\u{915}", MARK, marks, "\u{94D}\u{915}");
    let before_last = rope.len() - "\u{915}".len();
    grapheme_query(
        b,
        &rope,
        |rope| rope.is_grapheme_boundary(before_last),
        false,
    )
}

/// `x` and `indicators` regional indicators. Whether there is a boundary
/// before the last one steps back over the whole run to count it
/// (GB12, GB13): there is one only after an even number.
fn regional_indicators(b: &mut Bencher, indicators: usize) -> Result<(), BenchError> {
    let rope = repeated("x", INDICATOR, indicators, "");
    let before_last = rope.len().saturating_sub(INDICATOR.len());
    let want = indicators < 2 || (indicators - 1).is_multiple_of(2);
    grapheme_query(
        b,
        &rope,
        |rope| rope.is_grapheme_boundary(before_last),
        want,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Settings, run_suite};

    /// Settings that finish at once, for unoptimized test builds.
    const QUICK: Settings = Settings {
        warm_up: Duration::ZERO,
        sample_target: Duration::from_micros(10),
        budget: Duration::ZERO,
        min_samples: 2,
        max_samples: 2,
    };

    /// Runs `run` as a suite with [`QUICK`].
    fn quick(run: impl FnOnce(&mut Bencher) -> Result<(), BenchError>) {
        let mut bencher = Bencher::new(QUICK);
        run(&mut bencher).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn the_inputs_are_what_the_names_say() {
        let small = Text::small();
        let len = small.rope.len();
        assert!((400_000..420_000).contains(&len), "{len} bytes");
        assert_eq!(small.rope.len_lines(), LINES + 1, "the last line is empty");
        assert_eq!(small.rope, small.lines.text.as_str());
        let carets = small.carets();
        assert_eq!(carets.len(), CARETS);
        assert_eq!(carets, small.lines.ends, "every line's end");
        // The 100 MB text is 250 copies: its size and lines are computed
        // here, not built.
        let copies = BIG_BYTES.div_ceil(len);
        assert_eq!(copies, 250);
        assert!((100_000_000..101_000_000).contains(&(copies * len)));
        assert_eq!(BIG_MARKS * MARK.len(), 100_000_000);
        assert_eq!(BIG_INDICATORS * INDICATOR.len(), 100_000_000);
        // Carets on several copies are every `copies`th line's end.
        let three = Text::with_copies(3);
        let carets = three.carets();
        assert_eq!(carets.len(), CARETS);
        assert_eq!(carets[0], three.lines.ends[0]);
        assert_eq!(carets[1], three.lines.ends[3]);
        assert_eq!(carets[3_334], len + three.lines.ends[2]);
        assert!(carets.windows(2).all(|pair| pair[0] < pair[1]));
        for &caret in &carets[..20] {
            let line = three.rope.byte_to_line(caret).unwrap();
            assert_eq!(three.rope.line_span(line).unwrap().content_end, caret);
        }
    }

    #[test]
    fn draws_are_the_same_every_run_and_char_boundaries() {
        let text = Text::small();
        let first = places(&text.rope, 1);
        assert_eq!(first, places(&text.rope, 1));
        assert_ne!(first, places(&text.rope, 2), "streams differ");
        assert_eq!(first.len(), PLACES);
        assert!(first.iter().all(|&at| text.lines.text.is_char_boundary(at)));
    }

    #[test]
    fn the_crafted_lines_are_what_m1_1_named() {
        let rope = repeated("a", MARK, 40_000, "");
        assert_eq!(rope.len(), 80_001);
        assert_eq!(rope, format!("a{}", MARK.repeat(40_000)).as_str());
        let rope = repeated("x", INDICATOR, 5, "!");
        assert_eq!(rope, format!("x{}!", INDICATOR.repeat(5)).as_str());
        assert_eq!(repeated("", MARK, 0, ""), "");
    }

    #[test]
    fn the_400kb_suites_run() {
        for suite in SUITES.iter().filter(|suite| suite.name.ends_with("/400kb")) {
            let summary = run_suite(suite, &QUICK).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(summary.samples, 2, "{}", suite.name);
        }
    }

    /// The 100 MB suites' routines, on smaller inputs: the tests never
    /// build 100 MB.
    #[test]
    fn the_100mb_routines_run_on_small_inputs() {
        let text = Text::with_copies(2);
        quick(|b| rope_insert(b, &text.rope, 2));
        quick(|b| rope_remove_line(b, &text.rope, 4));
        for unit in [Unit::Byte, Unit::Utf16, Unit::Char] {
            quick(|b| byte_to_line_col(b, &text.rope, unit, 5));
            quick(|b| line_col_to_byte(b, &text.rope, unit, 8));
        }
        quick(|b| keystroke(b, &text));
        quick(|b| typing(b, &text));
        quick(|b| one_cluster(b, 20_000));
        quick(|b| combining_marks(b, 20_000));
        for indicators in [0, 1, 2, 3, 10_000, 10_001] {
            quick(|b| regional_indicators(b, indicators));
        }
    }

    #[test]
    fn a_wrong_answer_fails_the_grapheme_check() {
        let rope = repeated("a", MARK, 100, "");
        let mut bencher = Bencher::new(QUICK);
        let error = grapheme_query(
            &mut bencher,
            &rope,
            |rope| rope.next_grapheme_boundary(0),
            Some(1),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "its check failed: the query gave Some(201), not Some(1)"
        );
    }
}
