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

//! Where motions go, read within bounds (M4.2, ADR-0065): grapheme
//! clusters, words, a line's first character that is not white space, and
//! columns.
//!
//! New in Tidepool.
//!
//! # Why a long line is read in a window
//!
//! A grapheme cluster boundary depends on the text before it, and a query
//! for the next one reads up to it, so inside a cluster of 100 MB, or after
//! 100 MB of combining marks, one query reads all of it (M1.1's risk). The
//! rules never look across a line break, though, so on a line of n bytes a
//! query reads at most about n bytes.
//!
//! So on a line of at most [`WINDOW`] bytes, a cluster motion asks the rope
//! itself. On a longer line, every motion copies a *window* of the line out
//! and reads only that: from the line's start, if that is at most
//! [`WINDOW`] bytes back, or else [`WINDOW`] bytes back, up to [`WINDOW`]
//! bytes ahead or the line's end. A window that starts at the line's start
//! has the line's own boundaries. One that starts later has them too,
//! unless a cluster, or a run of marks or regional indicators, reaches past
//! its start; inside a cluster or a word longer than the window, a motion
//! stops at the window's edge, on a char boundary. Columns count at most a
//! line's first [`WINDOW`] bytes, the part the editor view shows
//! ([`MAX_LINE_BYTES`]).
//!
//! Every function takes a char boundary of the text, as a checked
//! selection holds, and gives one back.

use std::ops::Range;

use tidepool_text::pos::grapheme;
use tidepool_text::{LineSpan, Rope, TextError};

use crate::view::{MAX_LINE_BYTES, TAB_STOP_SPACES};

/// How far a motion reads either side of a position on a long line: the
/// editor view's [`MAX_LINE_BYTES`], so that the part of a line a motion
/// sees exactly is the part the view shows.
pub(crate) const WINDOW: usize = MAX_LINE_BYTES;

/// The separators that end a word, besides white space: Visual Studio
/// Code's default `editor.wordSeparators`. A run of them is a word of its
/// own to the word motions.
pub(crate) const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// What kind of character a cluster is, for the word motions, by its first
/// char.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// White space.
    Space,
    /// One of [`WORD_SEPARATORS`].
    Separator,
    /// Anything else.
    Word,
}

impl Class {
    /// The class of `cluster`.
    pub(crate) fn of(cluster: &str) -> Self {
        match cluster.chars().next() {
            Some(c) if c.is_whitespace() => Self::Space,
            Some(c) if WORD_SEPARATORS.contains(c) => Self::Separator,
            _ => Self::Word,
        }
    }
}

/// The line holding `at` and its span.
///
/// # Errors
///
/// As for [`Rope::byte_to_line`], for a position past the end or inside a
/// char.
pub(crate) fn line_of(text: &Rope, at: usize) -> Result<(usize, LineSpan), TextError> {
    let line = text.byte_to_line(at)?;
    Ok((line, text.line_span(line)?))
}

/// The part of a line's content that a motion from `at` reads, copied out:
/// see the [module documentation](self).
struct Window {
    /// Where it starts in the text.
    start: usize,
    text: String,
}

impl Window {
    /// The window of `span`'s content around `at`, which lies in it.
    fn around(text: &Rope, span: &LineSpan, at: usize) -> Self {
        let start = if at - span.start <= WINDOW {
            span.start
        } else {
            text.floor_char_boundary(at - WINDOW)
        };
        let end = if span.content_end - at <= WINDOW {
            span.content_end
        } else {
            text.ceil_char_boundary(at + WINDOW)
        };
        Self::copy(text, start..end)
    }

    /// The bytes `range` of `text`, which are char boundaries.
    fn copy(text: &Rope, range: Range<usize>) -> Self {
        let mut copied = String::with_capacity(range.len());
        if let Ok(chunks) = text.chunks_in(range.clone()) {
            for chunk in chunks {
                copied.push_str(chunk);
            }
        }
        Self {
            start: range.start,
            text: copied,
        }
    }

    /// Its clusters, as byte ranges of the text.
    fn clusters(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let mut at = self.start;
        grapheme::graphemes(&self.text).map(move |cluster| {
            let range = at..at + cluster.len();
            at = range.end;
            range
        })
    }

    /// The cluster at `range`, a range [`clusters`](Self::clusters) gave.
    fn cluster(&self, range: &Range<usize>) -> &str {
        self.text
            .get(range.start - self.start..range.end - self.start)
            .unwrap_or_default()
    }
}

/// Where a motion one cluster right of `at` goes: the next grapheme
/// cluster boundary; at a line's end, past its break, which moves as one
/// (a CR LF is one cluster); at the end of the text, the end.
///
/// # Errors
///
/// As for [`line_of`].
pub(crate) fn next_cluster(text: &Rope, at: usize) -> Result<usize, TextError> {
    let len = text.len();
    if at >= len {
        return Ok(len);
    }
    let (_, span) = line_of(text, at)?;
    if at >= span.content_end {
        return Ok(span.end());
    }
    if span.content_end - span.start <= WINDOW {
        // A line break ends every cluster before it, so this reads no
        // further than the line's end.
        let next = text.next_grapheme_boundary(at).unwrap_or(len);
        return Ok(next.min(span.content_end));
    }
    let window = Window::around(text, &span, at);
    let local = at - window.start;
    let next = grapheme::next_boundary(&window.text, local).unwrap_or(window.text.len());
    Ok(window.start + next)
}

/// Where a motion one cluster left of `at` goes: the grapheme cluster
/// boundary before it; at a line's start, before the line break above,
/// which moves as one; at the start of the text, the start.
///
/// # Errors
///
/// As for [`line_of`].
pub(crate) fn prev_cluster(text: &Rope, at: usize) -> Result<usize, TextError> {
    if at == 0 {
        return Ok(0);
    }
    let (line, span) = line_of(text, at)?;
    if at > span.content_end {
        // Between a CR and its LF.
        return Ok(span.content_end);
    }
    if at == span.start {
        return Ok(text.line_span(line - 1)?.content_end);
    }
    if span.content_end - span.start <= WINDOW {
        // The break before the line ends every cluster after it.
        let prev = text.prev_grapheme_boundary(at).unwrap_or(0);
        return Ok(prev.max(span.start));
    }
    let window = Window::around(text, &span, at);
    let local = at - window.start;
    let prev = grapheme::prev_boundary(&window.text, local).unwrap_or(0);
    Ok(window.start + prev)
}

/// Where a word motion right of `at` goes: past the white space after it,
/// then past one run of word characters or of separators, staying on the
/// line; at the line's end, past its break.
///
/// # Errors
///
/// As for [`line_of`].
pub(crate) fn word_right(text: &Rope, at: usize) -> Result<usize, TextError> {
    let (_, span) = line_of(text, at)?;
    if at >= span.content_end {
        return Ok(span.end());
    }
    let window = Window::around(text, &span, at);
    let mut run: Option<Class> = None;
    let mut to = at;
    for range in window.clusters() {
        if range.end <= at {
            continue;
        }
        let class = Class::of(window.cluster(&range));
        match run {
            None if class == Class::Space => {}
            None => run = Some(class),
            Some(kind) if kind == class => {}
            Some(_) => return Ok(to),
        }
        to = range.end;
    }
    Ok(to)
}

/// Where a word motion left of `at` goes: back past the white space before
/// it, then to the start of one run of word characters or of separators,
/// staying on the line; at the line's start, before the break above.
///
/// # Errors
///
/// As for [`line_of`].
pub(crate) fn word_left(text: &Rope, at: usize) -> Result<usize, TextError> {
    if at == 0 {
        return Ok(0);
    }
    let (line, span) = line_of(text, at)?;
    if at > span.content_end {
        return Ok(span.content_end);
    }
    if at == span.start {
        return Ok(text.line_span(line - 1)?.content_end);
    }
    let window = Window::around(text, &span, at);
    // The clusters before `at`, the last cut at `at` if it reaches past.
    let before: Vec<Range<usize>> = window
        .clusters()
        .take_while(|range| range.start < at)
        .map(|range| range.start..range.end.min(at))
        .collect();
    let mut run: Option<Class> = None;
    let mut to = at;
    for range in before.iter().rev() {
        let class = Class::of(window.cluster(range));
        match run {
            None if class == Class::Space => {}
            None => run = Some(class),
            Some(kind) if kind == class => {}
            Some(_) => return Ok(to),
        }
        to = range.start;
    }
    Ok(to)
}

/// The word under `at`, for a double click: the run of clusters of one
/// class around it, white space included, staying on the line; an empty
/// range at a line's end.
///
/// # Errors
///
/// As for [`line_of`].
pub(crate) fn word_at(text: &Rope, at: usize) -> Result<Range<usize>, TextError> {
    let (_, span) = line_of(text, at)?;
    if at >= span.content_end {
        return Ok(at..at);
    }
    let window = Window::around(text, &span, at);
    let clusters: Vec<Range<usize>> = window.clusters().collect();
    let Some(index) = clusters.iter().position(|range| range.end > at) else {
        return Ok(at..at);
    };
    let class = |i: usize| {
        clusters
            .get(i)
            .map(|range| Class::of(window.cluster(range)))
    };
    let kind = class(index);
    let mut first = index;
    while first > 0 && class(first - 1) == kind {
        first -= 1;
    }
    let mut last = index;
    while class(last + 1) == kind {
        last += 1;
    }
    let start = clusters.get(first).map_or(at, |range| range.start);
    let end = clusters.get(last).map_or(at, |range| range.end);
    Ok(start..end)
}

/// The end of a line's indentation, its first spaces and tabs, read over
/// at most its first [`WINDOW`] bytes.
pub(crate) fn indent_end(text: &Rope, span: &LineSpan) -> usize {
    let end = span
        .content_end
        .min(text.floor_char_boundary(span.start + WINDOW));
    let mut at = span.start;
    if let Ok(chars) = text.chars_in(span.start..end) {
        for c in chars {
            if c != ' ' && c != '\t' {
                break;
            }
            at += 1;
        }
    }
    at
}

/// How many columns `cluster` takes when it starts at column `column`: a
/// tab to the next stop, [`TAB_STOP_SPACES`] apart, and any other cluster
/// one.
fn width(cluster: &str, column: usize) -> usize {
    if cluster == "\t" {
        TAB_STOP_SPACES - column % TAB_STOP_SPACES
    } else {
        1
    }
}

/// The part of `span`'s content that columns count: all of it if it holds
/// at most [`WINDOW`] bytes, otherwise its clusters up to the last boundary
/// at most that far in, as the view keeps them (one char more is read, so
/// that the boundaries are the line's).
fn counted(text: &Rope, span: &LineSpan, upto: usize) -> (Window, usize) {
    let limit = span.start + WINDOW;
    if upto <= limit {
        return (Window::copy(text, span.start..upto), upto);
    }
    let end = text.ceil_char_boundary(limit + 1).min(span.content_end);
    (Window::copy(text, span.start..end), limit)
}

/// The column of `at` on its line, whose span is `span`: the widths of the
/// clusters before it (see [`width`]). A position past the line's first
/// [`WINDOW`] bytes counts as the column where the view cuts the line.
pub(crate) fn column(text: &Rope, span: &LineSpan, at: usize) -> usize {
    let (window, limit) = counted(text, span, at.min(span.content_end));
    let mut column = 0;
    for range in window.clusters() {
        if range.end > limit {
            break;
        }
        column += width(window.cluster(&range), column);
    }
    column
}

/// The position on the line whose span is `span` that a vertical motion
/// to column `goal` lands on: the last cluster boundary whose column is at
/// most `goal`, within the line's first [`WINDOW`] bytes.
pub(crate) fn at_column(text: &Rope, span: &LineSpan, goal: usize) -> usize {
    let (window, limit) = counted(text, span, span.content_end);
    let (mut at, mut column) = (span.start, 0);
    for range in window.clusters() {
        if range.end > limit {
            break;
        }
        let next = column + width(window.cluster(&range), column);
        if next > goal {
            break;
        }
        (at, column) = (range.end, next);
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from(text)
    }

    #[test]
    fn clusters_move_whole_and_line_breaks_too() {
        let text = rope("e\u{301}🇺🇸!\r\nab");
        let stops = [0, 3, 11, 12, 14, 15, 16];
        for pair in stops.windows(2) {
            assert_eq!(
                next_cluster(&text, pair[0]).ok(),
                Some(pair[1]),
                "right of {}",
                pair[0]
            );
            assert_eq!(
                prev_cluster(&text, pair[1]).ok(),
                Some(pair[0]),
                "left of {}",
                pair[1]
            );
        }
        assert_eq!(next_cluster(&text, 16).ok(), Some(16));
        assert_eq!(prev_cluster(&text, 0).ok(), Some(0));
        // Between the CR and the LF, a position inside their cluster.
        assert_eq!(next_cluster(&text, 13).ok(), Some(14));
        assert_eq!(prev_cluster(&text, 13).ok(), Some(12));
    }

    #[test]
    fn a_long_line_is_read_in_a_window() {
        // One cluster longer than the window: a motion stops at its edge,
        // the char boundary at or after it (the marks take two bytes).
        let line = format!("a{}", "\u{301}".repeat(3 * WINDOW));
        let text = rope(&line);
        assert_eq!(next_cluster(&text, 0).ok(), Some(WINDOW + 1));
        let end = line.len();
        assert_eq!(prev_cluster(&text, end).ok(), Some(end - WINDOW));
        // Inside it, the window starts at most WINDOW bytes back.
        let middle = 3 * WINDOW / 2 + 1;
        let next = next_cluster(&text, middle).ok();
        assert_eq!(next, Some(middle + WINDOW));
        // A word longer than the window stops at its edge too.
        let word = rope(&"x".repeat(3 * WINDOW));
        assert_eq!(word_right(&word, 0).ok(), Some(WINDOW));
        assert_eq!(word_left(&word, 3 * WINDOW).ok(), Some(2 * WINDOW));
    }

    #[test]
    fn a_long_line_keeps_its_boundaries_inside_the_window() {
        let line = format!("{}é\u{301}xy", "ab".repeat(WINDOW));
        let text = rope(&line);
        let at = 2 * WINDOW;
        assert_eq!(next_cluster(&text, at).ok(), Some(at + 4));
        assert_eq!(prev_cluster(&text, at + 4).ok(), Some(at));
        assert_eq!(next_cluster(&text, at + 4).ok(), Some(at + 5));
    }

    #[test]
    fn words_are_runs_of_one_class() {
        let text = rope("  let x = foo.bar(1);\nnext");
        let right = [
            (0, 5),
            (5, 7),
            (7, 9),
            (9, 13),
            (13, 14),
            (14, 17),
            (17, 18),
            (18, 19),
            (19, 21),
            (21, 22),
            (22, 26),
        ];
        for (from, to) in right {
            assert_eq!(word_right(&text, from).ok(), Some(to), "right of {from}");
        }
        let left = [
            (26, 22),
            (22, 21),
            (21, 19),
            (19, 18),
            (18, 17),
            (17, 14),
            (14, 13),
            (13, 10),
            (10, 8),
            (8, 6),
            (6, 2),
            (2, 0),
        ];
        for (from, to) in left {
            assert_eq!(word_left(&text, from).ok(), Some(to), "left of {from}");
        }
        assert_eq!(word_at(&text, 11).ok(), Some(10..13));
        assert_eq!(word_at(&text, 13).ok(), Some(13..14));
        assert_eq!(word_at(&text, 0).ok(), Some(0..2));
        assert_eq!(word_at(&text, 21).ok(), Some(21..21));
    }

    #[test]
    fn columns_count_tab_stops_and_clusters() {
        let text = rope("\ta\tbé\u{301}c\nxy");
        let (_, span) = line_of(&text, 0).expect("line 0");
        assert_eq!(column(&text, &span, 0), 0);
        assert_eq!(column(&text, &span, 1), 4);
        assert_eq!(column(&text, &span, 2), 5);
        assert_eq!(column(&text, &span, 3), 8);
        assert_eq!(column(&text, &span, 4), 9);
        assert_eq!(column(&text, &span, 8), 10, "after a cluster of two chars");
        assert_eq!(column(&text, &span, 9), 11);
        assert_eq!(at_column(&text, &span, 2), 0, "inside the first tab");
        assert_eq!(at_column(&text, &span, 4), 1);
        assert_eq!(at_column(&text, &span, 9), 4);
        assert_eq!(at_column(&text, &span, 10), 8);
        assert_eq!(at_column(&text, &span, 99), 9, "past the end");
    }

    #[test]
    fn columns_count_only_what_the_view_shows() {
        let line = "a".repeat(3 * WINDOW);
        let text = rope(&line);
        let (_, span) = line_of(&text, 0).expect("line 0");
        assert_eq!(column(&text, &span, 2 * WINDOW), WINDOW);
        assert_eq!(at_column(&text, &span, 2 * WINDOW), WINDOW);
    }

    #[test]
    fn indentation_is_leading_spaces_and_tabs() {
        let text = rope(" \t x\n\n    ");
        let line = |n| text.line_span(n).expect("a line");
        assert_eq!(indent_end(&text, &line(0)), 3);
        assert_eq!(indent_end(&text, &line(1)), 5);
        assert_eq!(indent_end(&text, &line(2)), 10);
    }
}
