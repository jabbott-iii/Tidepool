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

//! One line of the editor view laid out: its content as the view keeps it,
//! the pieces it is drawn in, and the rows it wraps to (M4.1, ADR-0063).
//!
//! New in Tidepool.
//!
//! A line is cut to [`MAX_LINE_BYTES`] at a grapheme cluster boundary, then
//! split into pieces: one for each grapheme cluster, but a cluster holding
//! a hidden character is split further, into its runs of shown characters
//! and one marker for each hidden character. Each piece has an advance,
//! measured in the context of the piece after it, as the field in `ui`
//! measures its caret stops, so that kerning between them counts; a tab's
//! advance depends on where it starts, and is set as the pieces are placed
//! on rows. A line's start is always a cluster boundary (a line break ends
//! a cluster, UAX #29 GB4 and GB5), so segmenting the line alone gives the
//! whole text's boundaries.

use tidepool_text::pos::grapheme;
use tidepool_text::{LineSpan, Rope};
use tidepool_ui::metrics::TextMetrics;
use tidepool_ui::widgets::{flag_at, is_hidden};

use super::{CUT_MARKER, EDITOR_TEXT, MARKER, MAX_LINE_BYTES, TAB_STOP_SPACES};

/// How close to a tab stop, in stops, a pen counts as on it, so that a tab
/// there advances a whole stop and never a sliver that rounding left.
const STOP_EPSILON: f32 = 1e-3;

/// What the view measures of [`EDITOR_TEXT`] once each time it lays out or
/// paints, through the frontend's [`TextMetrics`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Measures {
    /// A row's height: the line height, at least 1 unit, so that a
    /// position can always be divided by it.
    pub(super) row: f32,
    /// From the baseline up.
    pub(super) ascent: f32,
    /// From the baseline down.
    pub(super) descent: f32,
    /// A space's advance, which a selected line break shows as.
    pub(super) space: f32,
    /// A digit's advance, which the gutter is measured in.
    pub(super) digit: f32,
    /// A hidden character's marker's advance.
    pub(super) marker: f32,
    /// The cut marker's advance.
    pub(super) cut: f32,
}

impl Measures {
    /// The measures `metrics` gives.
    pub(super) fn new(metrics: &dyn TextMetrics) -> Self {
        let line = metrics.line(&EDITOR_TEXT);
        let mut marker = [0; 4];
        Self {
            row: if line.height >= 1.0 { line.height } else { 1.0 },
            ascent: line.ascent,
            descent: line.descent,
            space: metrics.width(" ", &EDITOR_TEXT),
            digit: metrics.width("0", &EDITOR_TEXT),
            marker: metrics.width(MARKER.encode_utf8(&mut marker), &EDITOR_TEXT),
            cut: metrics.width(CUT_MARKER, &EDITOR_TEXT),
        }
    }

    /// The bits of every measure, to tell when the metrics changed.
    pub(super) fn bits(&self) -> [u32; 7] {
        [
            self.row.to_bits(),
            self.ascent.to_bits(),
            self.descent.to_bits(),
            self.space.to_bits(),
            self.digit.to_bits(),
            self.marker.to_bits(),
            self.cut.to_bits(),
        ]
    }

    /// The baseline of a row whose top is at `y`: the line's ascent and
    /// descent centered in it.
    pub(super) fn baseline(&self, y: f32) -> f32 {
        y + (self.row - (self.ascent + self.descent)) / 2.0 + self.ascent
    }

    /// Where a tab that starts at `x` ends: the next stop, a stop being
    /// [`TAB_STOP_SPACES`] spaces wide; a whole stop on from a pen on one.
    fn tab_end(&self, x: f32) -> f32 {
        let stop = self.space * TAB_STOP_SPACES as f32;
        if stop > 0.0 && x.is_finite() {
            ((x / stop + STOP_EPSILON).floor() + 1.0) * stop
        } else {
            x
        }
    }
}

/// What a piece of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Shown text: a grapheme cluster, or a run of its shown characters.
    Text,
    /// A tab, which shows as space up to the next stop.
    Tab,
    /// A hidden character, shown as [`MARKER`].
    Marker,
    /// Where a line longer than [`MAX_LINE_BYTES`] is cut, shown as
    /// [`CUT_MARKER`]; it holds no byte.
    Cut,
}

/// A piece of a line: from byte `at` of the line's kept text to the next
/// piece's `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Piece {
    /// Where it starts in the line's kept text.
    pub(super) at: usize,
    /// Where it starts, from the start of its row.
    pub(super) x: f32,
    /// How far it advances the pen.
    pub(super) advance: f32,
    /// What it is.
    pub(super) kind: Kind,
    /// Whether it starts a grapheme cluster: a place a caret can be shown
    /// and a click can land, and a row can break.
    pub(super) stop: bool,
}

/// A row of a line: the pieces `first..end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RowSpan {
    /// Its first piece.
    pub(super) first: usize,
    /// Past its last piece.
    pub(super) end: usize,
    /// Where it starts in the line's kept text.
    pub(super) start: usize,
    /// Past its last piece: its width, the cut marker included.
    pub(super) width: f32,
    /// Past its last piece of the line's content: where a caret at its end
    /// shows, before the cut marker if it holds it.
    pub(super) content: f32,
}

/// One line laid out.
#[derive(Debug)]
pub(super) struct LineLayout {
    /// The line in the text.
    pub(super) span: LineSpan,
    /// Its content as kept: all of it, or its start up to the last grapheme
    /// cluster boundary at most [`MAX_LINE_BYTES`] in.
    pub(super) text: String,
    /// Whether its content was cut.
    pub(super) cut: bool,
    /// Its pieces, in order, the cut marker last.
    pub(super) pieces: Vec<Piece>,
    /// Its rows, at least one.
    pub(super) rows: Vec<RowSpan>,
    /// Its widest row's width.
    pub(super) width: f32,
}

impl LineLayout {
    /// Lays out the line `span` of `text`, wrapping it at `wrap` units if
    /// that is given.
    pub(super) fn new(
        text: &Rope,
        span: LineSpan,
        wrap: Option<f32>,
        measures: &Measures,
        metrics: &dyn TextMetrics,
    ) -> Self {
        let (kept, cut) = kept(text, span);
        let mut pieces = split(&kept);
        measure(&mut pieces, &kept, measures, metrics);
        if cut {
            pieces.push(Piece {
                at: kept.len(),
                x: 0.0,
                advance: measures.cut,
                kind: Kind::Cut,
                stop: false,
            });
        }
        let rows = place(&mut pieces, &kept, wrap, measures);
        let width = rows.iter().map(|row| row.width).fold(0.0, f32::max);
        Self {
            span,
            text: kept,
            cut,
            pieces,
            rows,
            width,
        }
    }

    /// Where piece `index` ends in the kept text.
    pub(super) fn piece_end(&self, index: usize) -> usize {
        self.pieces
            .get(index + 1)
            .map_or(self.text.len(), |next| next.at)
    }

    /// Where row `row` ends in the kept text: where the next starts, or the
    /// kept text's end.
    pub(super) fn row_end(&self, row: usize) -> usize {
        self.rows
            .get(row + 1)
            .map_or(self.text.len(), |next| next.start)
    }

    /// The row a caret at `at`, a byte of the line from its start, shows
    /// on: the last row starting at or before it, so that where a row
    /// wraps shows at the next row's start; past the kept text, the last.
    pub(super) fn row_of(&self, at: usize) -> usize {
        self.rows
            .partition_point(|row| row.start <= at)
            .saturating_sub(1)
    }

    /// Where a caret at `at` shows in row `row`, from the row's start: at
    /// the start of the piece it is in, at the row's end past it, and
    /// before the cut marker past the kept text.
    pub(super) fn caret_x(&self, row: usize, at: usize) -> f32 {
        let Some(span) = self.rows.get(row) else {
            return 0.0;
        };
        if at >= self.row_end(row) {
            return span.content;
        }
        let pieces = self.pieces.get(span.first..span.end).unwrap_or(&[]);
        let index = pieces.partition_point(|piece| piece.at <= at);
        match index.checked_sub(1).and_then(|index| pieces.get(index)) {
            Some(piece) => piece.x,
            None => 0.0,
        }
    }

    /// Where a selection that ends at `at` ends in row `row`: as a caret
    /// there, but past the kept text of a cut line, after the cut marker.
    pub(super) fn end_x(&self, row: usize, at: usize) -> f32 {
        if self.cut && at > self.text.len() {
            return self.rows.get(row).map_or(0.0, |span| span.width);
        }
        self.caret_x(row, at)
    }
}

/// The content of the line `span` of `text` as the view keeps it, and
/// whether it was cut: all of it if it is at most [`MAX_LINE_BYTES`] long;
/// otherwise its start up to the last grapheme cluster boundary at most
/// that far in. It reads no more of the line than that and one character.
fn kept(text: &Rope, span: LineSpan) -> (String, bool) {
    let len = span.content_end.saturating_sub(span.start);
    let cut = len > MAX_LINE_BYTES;
    // A boundary depends on the text before it and the character after it
    // (UAX #29), so a head holding one character past the bound has the
    // whole line's boundaries up to it.
    let end = if cut {
        text.ceil_char_boundary(span.start + MAX_LINE_BYTES + 1)
            .min(span.content_end)
    } else {
        span.content_end
    };
    let mut kept = String::with_capacity(end.saturating_sub(span.start));
    if let Ok(chunks) = text.chunks_in(span.start..end) {
        for chunk in chunks {
            kept.push_str(chunk);
        }
    }
    if cut {
        let mut end = 0;
        for cluster in grapheme::graphemes(&kept) {
            if end + cluster.len() > MAX_LINE_BYTES {
                break;
            }
            end += cluster.len();
        }
        kept.truncate(end);
    }
    (kept, cut)
}

/// The pieces of `text`, a line's kept content, with no advances yet.
fn split(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let piece = |at, kind, stop| Piece {
        at,
        x: 0.0,
        advance: 0.0,
        kind,
        stop,
    };
    let mut at = 0;
    for cluster in grapheme::graphemes(text) {
        if cluster == "\t" {
            pieces.push(piece(at, Kind::Tab, true));
        } else if !cluster.chars().any(is_hidden) {
            pieces.push(piece(at, Kind::Text, true));
        } else {
            // A run of shown characters is one piece, each hidden character
            // another, but a flag of England, Scotland or Wales stays whole.
            let mut inside = 0;
            let mut last = None;
            while let Some(rest) = cluster.get(inside..)
                && let Some(c) = rest.chars().next()
            {
                let (kind, len) = match flag_at(rest) {
                    Some(flag) => (Kind::Text, flag.len()),
                    None if is_hidden(c) => (Kind::Marker, c.len_utf8()),
                    None => (Kind::Text, c.len_utf8()),
                };
                if kind == Kind::Marker || last != Some(Kind::Text) {
                    pieces.push(piece(at + inside, kind, inside == 0));
                }
                last = Some(kind);
                inside += len;
            }
        }
        at += cluster.len();
    }
    pieces
}

/// Measures each piece's advance but a tab's: a marker's and the cut
/// marker's as `measures` has them, and shown text's in the context of the
/// shown text after it, as the pair less the piece after, so that kerning
/// between them counts.
fn measure(pieces: &mut [Piece], text: &str, measures: &Measures, metrics: &dyn TextMetrics) {
    let ends: Vec<usize> = (0..pieces.len())
        .map(|index| pieces.get(index + 1).map_or(text.len(), |next| next.at))
        .collect();
    let slice = |from: usize, to: usize| text.get(from..to).unwrap_or("");
    for index in 0..pieces.len() {
        let next = pieces.get(index + 1).copied();
        let Some(piece) = pieces.get_mut(index) else {
            continue;
        };
        let end = ends.get(index).copied().unwrap_or(text.len());
        piece.advance = match piece.kind {
            Kind::Tab => 0.0,
            Kind::Marker => measures.marker,
            Kind::Cut => measures.cut,
            Kind::Text => match next {
                Some(next) if next.kind == Kind::Text => {
                    let after = ends.get(index + 1).copied().unwrap_or(text.len());
                    let pair = metrics.width(slice(piece.at, after), &EDITOR_TEXT);
                    let alone = metrics.width(slice(end, after), &EDITOR_TEXT);
                    (pair - alone).max(0.0)
                }
                _ => metrics.width(slice(piece.at, end), &EDITOR_TEXT),
            },
        };
    }
}

/// Whether the piece is a whole space or tab: a place after which a
/// wrapped line may break.
fn is_space(text: &str, pieces: &[Piece], index: usize) -> bool {
    let Some(piece) = pieces.get(index) else {
        return false;
    };
    let end = pieces.get(index + 1).map_or(text.len(), |next| next.at);
    piece.stop && matches!(text.get(piece.at..end), Some(" " | "\t"))
}

/// Places the pieces on rows, wrapping at `wrap` units if it is given: a
/// row breaks before the first cluster that would pass that, after the
/// last space or tab of the row that fits if it has one after its start,
/// else before that cluster; a row always holds at least one cluster. Sets
/// each piece's `x` from its row's start and each tab's advance.
fn place(pieces: &mut [Piece], text: &str, wrap: Option<f32>, measures: &Measures) -> Vec<RowSpan> {
    let mut rows = Vec::new();
    let mut first = 0;
    let mut x = 0.0_f32;
    // Past the last space or tab placed on this row.
    let mut space_end: Option<usize> = None;
    let mut index = 0;
    let row = |pieces: &[Piece], first: usize, end: usize, width: f32| {
        let content = match pieces.get(end.wrapping_sub(1)) {
            Some(last) if end > first && last.kind == Kind::Cut => last.x,
            _ => width,
        };
        RowSpan {
            first,
            end,
            start: pieces.get(first).map_or(text.len(), |piece| piece.at),
            width,
            content,
        }
    };
    while let Some(&piece) = pieces.get(index) {
        let advance = if piece.kind == Kind::Tab {
            measures.tab_end(x) - x
        } else {
            piece.advance
        };
        let breaks = piece.stop || piece.kind == Kind::Cut;
        if let Some(width) = wrap
            && index > first
            && breaks
            && x + advance > width
        {
            let at = match space_end {
                Some(end) if end > first => end,
                _ => index,
            };
            let width = pieces.get(at).filter(|_| at < index).map_or(x, |p| p.x);
            rows.push(row(pieces, first, at, width));
            first = at;
            x = 0.0;
            space_end = None;
            index = at;
            continue;
        }
        if let Some(piece) = pieces.get_mut(index) {
            piece.x = x;
            piece.advance = advance;
        }
        x += advance;
        if is_space(text, pieces, index) {
            space_end = Some(index + 1);
        }
        index += 1;
    }
    rows.push(row(pieces, first, pieces.len(), x));
    rows
}

#[cfg(test)]
mod tests {
    use tidepool_ui::metrics::FixedMetrics;

    use super::*;

    /// 7 units an advance, rows 21 high: the numbers stay exact.
    const METRICS: FixedMetrics = FixedMetrics {
        advance: 0.5,
        ascent: 0.75,
        descent: 0.25,
    };

    fn laid_out(text: &str, wrap: Option<f32>) -> LineLayout {
        let rope = Rope::from(text);
        let span = rope.line_span(0).expect("line 0");
        LineLayout::new(&rope, span, wrap, &Measures::new(&METRICS), &METRICS)
    }

    fn kinds(layout: &LineLayout) -> Vec<(usize, Kind, bool)> {
        layout
            .pieces
            .iter()
            .map(|piece| (piece.at, piece.kind, piece.stop))
            .collect()
    }

    #[test]
    fn measures_come_from_the_metrics() {
        let measures = Measures::new(&METRICS);
        assert_eq!(measures.row, 21.0);
        assert_eq!((measures.ascent, measures.descent), (10.5, 3.5));
        assert_eq!((measures.space, measures.digit), (7.0, 7.0));
        assert_eq!(measures.baseline(0.0), 3.5 + 10.5);
        // Stops every 28: from 0 and from a stop, a whole stop on.
        assert_eq!(measures.tab_end(0.0), 28.0);
        assert_eq!(measures.tab_end(27.0), 28.0);
        assert_eq!(measures.tab_end(28.0), 56.0);
        assert_eq!(measures.tab_end(27.99999), 56.0);
    }

    #[test]
    fn a_cluster_with_hidden_characters_splits_into_runs_and_markers() {
        // "a" with a tag character after it is one cluster: the tag is
        // a marker inside it, not a stop.
        let layout = laid_out("a\u{E0041}b", None);
        assert_eq!(
            kinds(&layout),
            [
                (0, Kind::Text, true),
                (1, Kind::Marker, false),
                (5, Kind::Text, true)
            ]
        );
        let wales = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0077}\u{E006C}\u{E0073}\u{E007F}";
        let layout = laid_out(&format!("{wales}\u{E0041}"), None);
        // The flag and the stray tag are one cluster: the flag whole, then
        // a marker.
        assert_eq!(
            kinds(&layout),
            [(0, Kind::Text, true), (wales.len(), Kind::Marker, false)]
        );
    }

    #[test]
    fn rows_break_after_the_last_space_that_fits_or_inside_a_word() {
        // 10 columns of 7: "aaa bbb ccc" breaks after "bbb ".
        let layout = laid_out("aaa bbb ccc", Some(70.0));
        let starts: Vec<usize> = layout.rows.iter().map(|row| row.start).collect();
        assert_eq!(starts, [0, 8]);
        assert_eq!(layout.rows[0].width, 56.0);
        // No space: a break at a cluster boundary.
        let layout = laid_out("abcdefghijklmnop", Some(70.0));
        let starts: Vec<usize> = layout.rows.iter().map(|row| row.start).collect();
        assert_eq!(starts, [0, 10]);
        // A row holds a cluster even if it is wider than the row.
        let layout = laid_out("ab", Some(1.0));
        assert_eq!(layout.rows.len(), 2);
    }

    #[test]
    fn carets_show_at_piece_starts_and_row_ends() {
        let layout = laid_out("ab\tc", None);
        assert_eq!(layout.caret_x(0, 0), 0.0);
        assert_eq!(layout.caret_x(0, 2), 14.0);
        assert_eq!(layout.caret_x(0, 3), 28.0);
        assert_eq!(layout.caret_x(0, 4), 35.0);
        assert_eq!(layout.row_of(4), 0);
    }
}
