//! Grapheme cluster conformance at the real rope shape (leaves of 1-4 KiB),
//! through the public API only.
//!
//! - Every case of `GraphemeBreakTest.txt` on `&str` and on a rope of the
//!   case, with `graphemes_in` over every range of char boundaries.
//! - All cases in one rope of several leaves, separated by U+0001 (a
//!   control, so GB4 and GB5 keep them apart), so that seams fall wherever
//!   the builder puts them.
//! - Long lines of flags, marks, conjuncts and emoji ZWJ sequences: windows
//!   and queries near every seam agree with iterating from the start.
//! - The tables' Unicode version is `std`'s (ADR-0018).
//!
//! The unit tests in `src/pos/grapheme/tests.rs` force a seam at every
//! internal char boundary of every case, in the tiny test shape.

use std::ops::Range;

use tidepool_text::Rope;
use tidepool_text::pos::grapheme::{
    UNICODE_VERSION, graphemes, is_boundary, next_boundary, prev_boundary,
};

/// Unicode's conformance test for the extended grapheme cluster rules.
const TEST_FILE: &str = include_str!("../ucd/17.0.0/GraphemeBreakTest.txt");

/// One line of [`TEST_FILE`]: its number, its text and the byte offsets
/// marked `÷` (0 and the length among them).
struct Case {
    line: usize,
    text: String,
    boundaries: Vec<usize>,
}

/// Every case of [`TEST_FILE`], after checking its header and its count.
fn cases() -> Vec<Case> {
    assert_eq!(
        TEST_FILE.lines().next(),
        Some("# GraphemeBreakTest-17.0.0.txt")
    );
    let mut cases = Vec::new();
    for (index, line) in TEST_FILE.lines().enumerate() {
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
                "line {}",
                index + 1
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
        assert!(!want_marker, "line {}", index + 1);
        assert_eq!(boundaries.first(), Some(&0), "line {}", index + 1);
        assert_eq!(boundaries.last(), Some(&text.len()), "line {}", index + 1);
        cases.push(Case {
            line: index + 1,
            text,
            boundaries,
        });
    }
    assert_eq!(cases.len(), 766);
    assert!(TEST_FILE.contains("\n# Lines: 766\n"));
    cases
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

/// Checks `is`, `next` and `prev` on `text` and on `rope` at every byte in
/// `bytes` against `want`, the sorted boundaries (0 and the length among
/// them).
fn check_queries(
    text: &str,
    rope: &Rope,
    want: &[usize],
    bytes: Range<usize>,
) -> Result<(), String> {
    for byte in bytes {
        let is = want.binary_search(&byte).is_ok();
        let next = want.iter().copied().find(|&b| b > byte);
        let prev = want.iter().rev().copied().find(|&b| b < byte);
        let got = (
            is_boundary(text, byte),
            next_boundary(text, byte),
            prev_boundary(text, byte),
        );
        if got != (is, next, prev) {
            return Err(format!(
                "str at {byte}: {got:?}, want {:?}",
                (is, next, prev)
            ));
        }
        let got = (
            rope.is_grapheme_boundary(byte),
            rope.next_grapheme_boundary(byte),
            rope.prev_grapheme_boundary(byte),
        );
        if got != (is, next, prev) {
            return Err(format!(
                "rope at {byte}: {got:?}, want {:?}",
                (is, next, prev)
            ));
        }
    }
    Ok(())
}

/// Checks `graphemes_in` on `rope` for `range` against `want`.
fn check_spans(rope: &Rope, want: &[usize], range: Range<usize>) -> Result<(), String> {
    let got: Vec<_> = rope
        .graphemes_in(range.clone())
        .map_err(|e| format!("graphemes_in({range:?}): {e}"))?
        .collect();
    let expected = spans_between(want, range.start, range.end);
    if got == expected {
        Ok(())
    } else {
        Err(format!(
            "graphemes_in({range:?}) is {got:?}, want {expected:?}"
        ))
    }
}

/// Every case on `&str` and on its rope: every query at every byte,
/// `graphemes()`, and `graphemes_in` over every range of char boundaries.
#[test]
fn every_case_on_str_and_rope() {
    for case in cases() {
        let text = case.text.as_str();
        let rope = Rope::from(text);
        let want = &case.boundaries;
        check_queries(text, &rope, want, 0..text.len() + 2)
            .unwrap_or_else(|e| panic!("line {}: {e}", case.line));
        let got: Vec<&str> = graphemes(text).collect();
        let expected: Vec<&str> = want.windows(2).map(|w| &text[w[0]..w[1]]).collect();
        assert_eq!(got, expected, "line {}", case.line);
        let cuts: Vec<usize> = (0..=text.len())
            .filter(|&b| text.is_char_boundary(b))
            .collect();
        for &start in &cuts {
            for &end in cuts.iter().filter(|&&end| end >= start) {
                check_spans(&rope, want, start..end)
                    .unwrap_or_else(|e| panic!("line {}: {e}", case.line));
            }
        }
    }
}

/// All cases, four times over with different paddings, in one rope of
/// several real-size leaves (about 22 KB), each case followed by U+0001.
#[test]
fn all_cases_in_one_rope() {
    let mut text = String::new();
    let mut want = vec![0];
    let mut starts = Vec::new();
    let cases = cases();
    for round in 0..4 {
        for _ in 0..round * 701 {
            text.push('\u{1}');
            want.push(text.len());
        }
        for case in &cases {
            let base = text.len();
            starts.push(base);
            text.push_str(&case.text);
            want.extend(case.boundaries.iter().skip(1).map(|b| base + b));
            text.push('\u{1}');
            want.push(text.len());
        }
    }
    let rope = Rope::from(text.as_str());
    assert!(rope.chunks().count() >= 6, "the text spans several leaves");
    check_queries(&text, &rope, &want, 0..text.len() + 2).unwrap_or_else(|e| panic!("{e}"));
    let got: Vec<&str> = graphemes(&text).collect();
    let expected: Vec<&str> = want.windows(2).map(|w| &text[w[0]..w[1]]).collect();
    assert_eq!(got, expected);
    check_spans(&rope, &want, 0..text.len()).unwrap_or_else(|e| panic!("{e}"));
    // From every case's start, and from just inside it, over the next 64
    // bytes or so.
    for &start in &starts {
        let inside = text.ceil_char_boundary(start + 1);
        for from in [start, inside] {
            let to = text.ceil_char_boundary(from + 64);
            check_spans(&rope, &want, from..to).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}

/// Long lines of the kinds that make a query step back: `graphemes_in`
/// windows of several lengths that start near every seam and near every
/// 997th byte, and the queries near every seam, agree with `graphemes()`
/// from the start of the text, which never steps back.
#[test]
fn windows_inside_long_runs_agree_with_iterating_from_the_start() {
    let texts = [
        "\u{1F1FA}\u{1F1F8}\u{301}".repeat(1_500),
        "\u{1F1FA}\u{1F1F8}\u{200D}".repeat(1_500),
        "\u{1F1E6}".repeat(4_000),
        format!("a{}", "\u{301}".repeat(8_000)),
        format!("{}\u{915}", "\u{915}\u{94D}".repeat(3_000)),
        format!("{}\u{1F600}", "\u{1F600}\u{200D}".repeat(2_000)),
        format!("{}\u{200D}\u{1F600}", "\u{1F1FA}\u{301}".repeat(2_000)),
    ];
    for text in &texts {
        let rope = Rope::from(text.as_str());
        let mut want = vec![0];
        for cluster in graphemes(text) {
            want.push(want.last().copied().unwrap_or(0) + cluster.len());
        }
        let mut seams = vec![0];
        for chunk in rope.chunks() {
            seams.push(seams.last().copied().unwrap_or(0) + chunk.len());
        }
        assert!(seams.len() > 4, "the text spans several leaves");
        // Queries inside one long cluster read to its ends, so they are
        // checked only around the seams.
        for &at in &seams {
            let from = at.saturating_sub(5);
            check_queries(text, &rope, &want, from..at + 6).unwrap_or_else(|e| panic!("{e}"));
        }
        let others = (0..text.len()).step_by(997);
        for at in seams.iter().copied().chain(others) {
            let from = text.floor_char_boundary(at.saturating_sub(9));
            let to = text.ceil_char_boundary(at + 9);
            for start in (from..=to).filter(|&b| text.is_char_boundary(b)) {
                for len in [0, 1, 7, 300] {
                    let end = text.ceil_char_boundary(start + len);
                    check_spans(&rope, &want, start..end).unwrap_or_else(|e| panic!("{e}"));
                }
            }
        }
    }
}

/// The tables and `std`'s char properties use the same Unicode version
/// (ADR-0018). A Rust upgrade that changes it must bring the new version's
/// files and regenerate the tables, as a change of its own.
#[test]
fn the_tables_follow_std_unicode_version() {
    assert_eq!(UNICODE_VERSION, char::UNICODE_VERSION);
    assert_eq!(UNICODE_VERSION, (17, 0, 0));
}
