//! Exhaustive round trips of every conversion in the tiny test shape, and
//! the position oracle that `rope::tests` uses too.
//!
//! The oracle works every fact out from the definitions in `pos` with `str`
//! methods only; it shares no code with the rope.

use std::fmt::Debug;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check_sized;

use crate::rope::tests::{floor_boundary, push_text};
use crate::{LineBreak, LineCol, LineSpan, Rope, TextError, Unit};

/// Every unit, in the order the oracle indexes them.
pub(crate) const UNITS: [Unit; 3] = [Unit::Byte, Unit::Utf16, Unit::Char];

/// Whether two errors are the same variant with the same fields.
/// (`TextError` has no `PartialEq`, so that file errors can join it.)
pub(crate) fn error_eq(a: &TextError, b: &TextError) -> bool {
    use TextError as E;
    match (a, b) {
        (E::ByteOutOfBounds { byte, len }, E::ByteOutOfBounds { byte: b2, len: l2 }) => {
            byte == b2 && len == l2
        }
        (E::NotCharBoundary { byte }, E::NotCharBoundary { byte: b2 }) => byte == b2,
        (E::InvertedRange { start, end }, E::InvertedRange { start: s2, end: e2 }) => {
            start == s2 && end == e2
        }
        (
            E::OffsetOutOfBounds { offset, unit, len },
            E::OffsetOutOfBounds {
                offset: o2,
                unit: u2,
                len: l2,
            },
        ) => offset == o2 && unit == u2 && len == l2,
        (
            E::OffsetInsideChar { offset, unit },
            E::OffsetInsideChar {
                offset: o2,
                unit: u2,
            },
        ) => offset == o2 && unit == u2,
        (
            E::LineOutOfBounds { line, len_lines },
            E::LineOutOfBounds {
                line: l2,
                len_lines: n2,
            },
        ) => line == l2 && len_lines == n2,
        (
            E::ColumnOutOfBounds { pos, unit, max },
            E::ColumnOutOfBounds {
                pos: p2,
                unit: u2,
                max: m2,
            },
        ) => pos == p2 && unit == u2 && max == m2,
        (E::ColumnInsideChar { pos, unit }, E::ColumnInsideChar { pos: p2, unit: u2 }) => {
            pos == p2 && unit == u2
        }
        _ => false,
    }
}

/// `Ok` if `got` is `want`; otherwise a message naming `what`. Formats only
/// on failure.
pub(crate) fn expect<T: PartialEq + Debug>(
    what: impl FnOnce() -> String,
    got: Result<T, TextError>,
    want: Result<T, TextError>,
) -> Result<(), String> {
    let same = match (&got, &want) {
        (Ok(a), Ok(b)) => a == b,
        (Err(a), Err(b)) => error_eq(a, b),
        _ => false,
    };
    if same {
        Ok(())
    } else {
        Err(format!("{}: got {got:?}, want {want:?}", what()))
    }
}

/// The byte offsets where line breaks end, by the definition: CR LF is one
/// break, any other CR or LF is one.
pub(crate) fn break_ends(text: &str) -> impl Iterator<Item = (usize, usize, LineBreak)> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (at, c) = chars.next()?;
            let kind = match c {
                '\n' => LineBreak::Lf,
                '\r' if chars.peek().map(|&(_, next)| next) == Some('\n') => {
                    chars.next();
                    LineBreak::CrLf
                }
                '\r' => LineBreak::Cr,
                _ => continue,
            };
            // (content end, break end, kind)
            return Some((at, at + kind.as_str().len(), kind));
        }
    })
}

/// The error a byte-taking function must give for `byte` in `text`, if any.
pub(crate) fn byte_error(text: &str, byte: usize) -> Result<(), TextError> {
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

/// The error a range-taking function must give for `start..end`, if any.
pub(crate) fn range_error(text: &str, start: usize, end: usize) -> Result<(), TextError> {
    if start > end {
        Err(TextError::InvertedRange { start, end })
    } else if end > text.len() {
        Err(TextError::ByteOutOfBounds {
            byte: end,
            len: text.len(),
        })
    } else if !text.is_char_boundary(start) {
        Err(TextError::NotCharBoundary { byte: start })
    } else if !text.is_char_boundary(end) {
        Err(TextError::NotCharBoundary { byte: end })
    } else {
        Ok(())
    }
}

/// P1, P3 and P5 at one valid byte, worked out on the fly in one pass of
/// `chars` (no tables), so that it is cheap enough for every model step.
pub(crate) fn probe(rope: &Rope, text: &str, byte: usize) -> Result<(), String> {
    let bytes = text.as_bytes();
    // Offsets of `byte`, and of its line's start, in Byte, Utf16, Char.
    let mut offsets = [0, 0, 0];
    let mut start = [0, 0, 0];
    let mut line = 0;
    for (at, c) in text[..byte].char_indices() {
        offsets = [
            at + c.len_utf8(),
            offsets[1] + c.len_utf16(),
            offsets[2] + 1,
        ];
        // A CR followed by an LF ends its break at the LF.
        let ends_break = c == '\n' || (c == '\r' && bytes.get(at + 1) != Some(&b'\n'));
        if ends_break {
            line += 1;
            start = offsets;
        }
    }
    expect(
        || format!("byte_to_line({byte})"),
        rope.byte_to_line(byte),
        Ok(line),
    )?;
    for (k, unit) in UNITS.into_iter().enumerate() {
        let offset = offsets[k];
        expect(
            || format!("byte_to_offset({byte}, {unit:?})"),
            rope.byte_to_offset(byte, unit),
            Ok(offset),
        )?;
        expect(
            || format!("offset_to_byte({offset}, {unit:?})"),
            rope.offset_to_byte(offset, unit),
            Ok(byte),
        )?;
        let pos = LineCol::new(line, offset - start[k]);
        expect(
            || format!("byte_to_line_col({byte}, {unit:?})"),
            rope.byte_to_line_col(byte, unit),
            Ok(pos),
        )?;
        expect(
            || format!("line_col_to_byte({pos:?}, {unit:?})"),
            rope.line_col_to_byte(pos, unit),
            Ok(byte),
        )?;
    }
    Ok(())
}

/// Every position fact of one text, as tables.
pub(crate) struct Oracle<'t> {
    text: &'t str,
    /// For each unit, the offset of every byte in `0..=n` (at a byte inside
    /// a char, the offset of that char's start).
    off: [Vec<usize>; 3],
    /// For each unit, the byte at every offset in `0..=len`, if any.
    inv: [Vec<Option<usize>>; 3],
    /// Line starts.
    starts: Vec<usize>,
    /// For each line, its content end and break.
    ends: Vec<(usize, Option<LineBreak>)>,
}

impl<'t> Oracle<'t> {
    pub(crate) fn new(text: &'t str) -> Self {
        let n = text.len();
        let mut off = [vec![0; n + 1], vec![0; n + 1], vec![0; n + 1]];
        let (mut utf16, mut chars) = (0, 0);
        for (at, c) in text.char_indices() {
            let end = at + c.len_utf8();
            for (byte, slot) in off[0][at..end].iter_mut().enumerate() {
                *slot = at + byte;
            }
            off[1][at..end].fill(utf16);
            off[2][at..end].fill(chars);
            utf16 += c.len_utf16();
            chars += 1;
        }
        off[0][n] = n;
        off[1][n] = utf16;
        off[2][n] = chars;
        let lens = [n, utf16, chars];
        let inv = std::array::from_fn(|unit| {
            let mut inv = vec![None; lens[unit] + 1];
            for byte in (0..=n).filter(|&byte| text.is_char_boundary(byte)) {
                inv[off[unit][byte]] = Some(byte);
            }
            inv
        });
        let mut starts = vec![0];
        let mut ends = Vec::new();
        for (content_end, end, kind) in break_ends(text) {
            ends.push((content_end, Some(kind)));
            starts.push(end);
        }
        ends.push((n, None));
        Self {
            text,
            off,
            inv,
            starts,
            ends,
        }
    }

    fn len(&self, unit: usize) -> usize {
        self.off[unit][self.text.len()]
    }

    fn line(&self, byte: usize) -> usize {
        self.starts.partition_point(|&start| start <= byte) - 1
    }

    /// The last byte of `line`'s column space.
    fn last_byte(&self, line: usize) -> usize {
        match self.starts.get(line + 1) {
            None => self.text.len(),
            Some(&next) => (self.starts[line]..next)
                .rev()
                .find(|&byte| self.text.is_char_boundary(byte))
                .unwrap_or(self.starts[line]),
        }
    }

    fn floor(&self, byte: usize) -> usize {
        let n = self.text.len();
        if byte >= n {
            return n;
        }
        (0..=byte)
            .rev()
            .find(|&b| self.text.is_char_boundary(b))
            .unwrap_or(0)
    }

    fn ceil(&self, byte: usize) -> usize {
        let n = self.text.len();
        (byte.min(n)..=n)
            .find(|&b| self.text.is_char_boundary(b))
            .unwrap_or(n)
    }
}

/// Checks P1-P9 of the design at every byte in `0..=n + 2`, every offset in
/// every unit, every line and every column up to the line's last + 2, plus
/// the boundary queries, chunks and char iterators. `text` is the model.
pub(crate) fn sweep(rope: &Rope, text: &str) -> Result<(), String> {
    rope.__check_invariants()?;
    if *rope != *text {
        return Err(format!(
            "the rope's text differs from the model ({} bytes)",
            text.len()
        ));
    }
    let o = Oracle::new(text);
    let n = text.len();
    let lines = o.starts.len();

    // P9.
    for (k, unit) in UNITS.into_iter().enumerate() {
        if rope.len_in(unit) != o.len(k) {
            return Err(format!(
                "len_in({unit:?}) is {}, want {}",
                rope.len_in(unit),
                o.len(k)
            ));
        }
    }
    if rope.len_lines() != lines {
        return Err(format!("len_lines is {}, want {lines}", rope.len_lines()));
    }

    // P1, P3, P5 at valid bytes; P8 at the others.
    for byte in 0..=n + 2 {
        if text.is_char_boundary(byte) {
            let line = o.line(byte);
            expect(
                || format!("byte_to_line({byte})"),
                rope.byte_to_line(byte),
                Ok(line),
            )?;
            for (k, unit) in UNITS.into_iter().enumerate() {
                let offset = o.off[k][byte];
                expect(
                    || format!("byte_to_offset({byte}, {unit:?})"),
                    rope.byte_to_offset(byte, unit),
                    Ok(offset),
                )?;
                expect(
                    || format!("offset_to_byte({offset}, {unit:?})"),
                    rope.offset_to_byte(offset, unit),
                    Ok(byte),
                )?;
                let pos = LineCol::new(line, offset - o.off[k][o.starts[line]]);
                expect(
                    || format!("byte_to_line_col({byte}, {unit:?})"),
                    rope.byte_to_line_col(byte, unit),
                    Ok(pos),
                )?;
                expect(
                    || format!("line_col_to_byte({pos:?}, {unit:?})"),
                    rope.line_col_to_byte(pos, unit),
                    Ok(byte),
                )?;
            }
            let bounds = (
                rope.is_char_boundary(byte),
                rope.floor_char_boundary(byte),
                rope.ceil_char_boundary(byte),
            );
            if bounds != (true, byte, byte) {
                return Err(format!(
                    "at valid byte {byte}: (is, floor, ceil) = {bounds:?}"
                ));
            }
        } else {
            invalid_byte(rope, text, byte)?;
            let bounds = (
                rope.is_char_boundary(byte),
                rope.floor_char_boundary(byte),
                rope.ceil_char_boundary(byte),
            );
            let want = (false, o.floor(byte), o.ceil(byte));
            if bounds != want {
                return Err(format!(
                    "at invalid byte {byte}: (is, floor, ceil) = {bounds:?}, want {want:?}"
                ));
            }
        }
    }

    // P2.
    for (k, unit) in UNITS.into_iter().enumerate() {
        let len = o.len(k);
        for offset in 0..=len + 2 {
            let want = match o.inv[k].get(offset) {
                Some(Some(byte)) => Ok(*byte),
                _ if unit == Unit::Byte => byte_error(text, offset).map(|()| offset),
                _ if offset > len => Err(TextError::OffsetOutOfBounds { offset, unit, len }),
                _ => Err(TextError::OffsetInsideChar { offset, unit }),
            };
            expect(
                || format!("offset_to_byte({offset}, {unit:?})"),
                rope.offset_to_byte(offset, unit),
                want,
            )?;
        }
    }

    // P4 and P7.
    for line in 0..lines + 2 {
        let out = TextError::LineOutOfBounds {
            line,
            len_lines: lines,
        };
        let Some(&start) = o.starts.get(line) else {
            expect(
                || format!("line_to_byte({line})"),
                rope.line_to_byte(line),
                Err(out),
            )?;
            let out = TextError::LineOutOfBounds {
                line,
                len_lines: lines,
            };
            expect(
                || format!("line_span({line})"),
                rope.line_span(line),
                Err(out),
            )?;
            continue;
        };
        expect(
            || format!("line_to_byte({line})"),
            rope.line_to_byte(line),
            Ok(start),
        )?;
        expect(
            || format!("byte_to_line(line_to_byte({line}))"),
            rope.byte_to_line(start),
            Ok(line),
        )?;
        let (content_end, line_break) = o.ends[line];
        let span = LineSpan {
            start,
            content_end,
            line_break,
        };
        expect(
            || format!("line_span({line})"),
            rope.line_span(line),
            Ok(span),
        )?;
        let next = o.starts.get(line + 1).copied().unwrap_or(n);
        if span.end() != next
            || span.full() != (start..next)
            || span.content() != (start..content_end)
        {
            return Err(format!("line {line}: {span:?} does not reach {next}"));
        }
    }

    // P6.
    for (k, unit) in UNITS.into_iter().enumerate() {
        for line in 0..=lines {
            let (base, max) = match o.starts.get(line) {
                Some(&start) => (
                    o.off[k][start],
                    o.off[k][o.last_byte(line)] - o.off[k][start],
                ),
                None => (0, 0),
            };
            for col in 0..=max + 2 {
                let pos = LineCol::new(line, col);
                let want = if line >= lines {
                    Err(TextError::LineOutOfBounds {
                        line,
                        len_lines: lines,
                    })
                } else if col > max {
                    Err(TextError::ColumnOutOfBounds { pos, unit, max })
                } else {
                    match o.inv[k][base + col] {
                        Some(byte) => Ok(byte),
                        None => Err(TextError::ColumnInsideChar { pos, unit }),
                    }
                };
                expect(
                    || format!("line_col_to_byte({pos:?}, {unit:?})"),
                    rope.line_col_to_byte(pos, unit),
                    want,
                )?;
            }
        }
    }

    iterators(rope, text)
}

/// P8 at one invalid byte: every byte- or range-taking function gives the
/// exact error, and the edits leave a clone unchanged.
fn invalid_byte(rope: &Rope, text: &str, byte: usize) -> Result<(), String> {
    let n = text.len();
    let error = || byte_error(text, byte);
    expect(
        || format!("byte_to_line({byte})"),
        rope.byte_to_line(byte),
        error().map(|()| 0),
    )?;
    for unit in UNITS {
        expect(
            || format!("byte_to_offset({byte}, {unit:?})"),
            rope.byte_to_offset(byte, unit),
            error().map(|()| 0),
        )?;
        expect(
            || format!("byte_to_line_col({byte}, {unit:?})"),
            rope.byte_to_line_col(byte, unit),
            error().map(|()| LineCol::default()),
        )?;
    }
    expect(
        || format!("offset_to_byte({byte}, Byte)"),
        rope.offset_to_byte(byte, Unit::Byte),
        error().map(|()| 0),
    )?;

    let mut edited = rope.clone();
    expect(
        || format!("insert({byte})"),
        edited.insert(byte, "x\r"),
        error(),
    )?;
    expect(
        || format!("split_off({byte})"),
        edited.split_off(byte).map(|_| ()),
        error(),
    )?;
    let ranges = [
        (byte, byte),
        (0, byte),
        (byte, n.max(byte)),
        (byte, byte.saturating_sub(1)),
        (byte + 1, byte),
        (byte, n + 3),
    ];
    for (start, end) in ranges {
        let want = || range_error(text, start, end);
        expect(
            || format!("remove({start}..{end})"),
            edited.remove(start..end),
            want(),
        )?;
        expect(
            || format!("replace({start}..{end})"),
            edited.replace(start..end, "\n"),
            want(),
        )?;
        expect(
            || format!("slice({start}..{end})"),
            rope.slice(start..end).map(|_| ()),
            want(),
        )?;
        expect(
            || format!("chunks_in({start}..{end})"),
            rope.chunks_in(start..end).map(|_| ()),
            want(),
        )?;
        expect(
            || format!("chars_in({start}..{end})"),
            rope.chars_in(start..end).map(|_| ()),
            want(),
        )?;
        expect(
            || format!("char_indices_in({start}..{end})"),
            rope.char_indices_in(start..end).map(|_| ()),
            want(),
        )?;
    }
    edited.__check_invariants()?;
    if edited != *rope || edited != *text {
        return Err(format!("a refused edit at byte {byte} changed the rope"));
    }
    Ok(())
}

/// Chunks of the whole text and chars and char indices of a few ranges,
/// from both ends and alternating.
fn iterators(rope: &Rope, text: &str) -> Result<(), String> {
    let chunks: Vec<&str> = rope.chunks().collect();
    if chunks.concat() != text || chunks.iter().any(|chunk| chunk.is_empty()) {
        return Err("the chunks do not spell the text".to_string());
    }
    if chunks
        .windows(2)
        .any(|pair| pair[0].ends_with('\r') && pair[1].starts_with('\n'))
    {
        return Err("two chunks split a CR LF".to_string());
    }
    let mut backward: Vec<&str> = rope.chunks().rev().collect();
    backward.reverse();
    if backward != chunks {
        return Err("chunks from the back differ".to_string());
    }

    let boundaries: Vec<usize> = (0..=text.len())
        .filter(|&byte| text.is_char_boundary(byte))
        .collect();
    let step = (boundaries.len() / 5).max(1);
    for (i, &start) in boundaries.iter().enumerate().step_by(step) {
        for &end in boundaries[i..].iter().step_by(step.max(3) - 1) {
            let range = start..end;
            let want: Vec<(usize, char)> = text[range.clone()]
                .char_indices()
                .map(|(at, c)| (start + at, c))
                .collect();
            let fail = |what: &str| Err(format!("{what} of {start}..{end} differ"));
            let forward: Vec<(usize, char)> = rope
                .char_indices_in(range.clone())
                .map_err(|e| e.to_string())?
                .collect();
            if forward != want {
                return fail("char indices");
            }
            let mut back: Vec<(usize, char)> = rope
                .char_indices_in(range.clone())
                .map_err(|e| e.to_string())?
                .rev()
                .collect();
            back.reverse();
            if back != want {
                return fail("char indices from the back");
            }
            let chars: String = rope
                .chars_in(range.clone())
                .map_err(|e| e.to_string())?
                .collect();
            if chars != text[range.clone()] {
                return fail("chars");
            }
            // Alternate ends: the two halves must meet without a gap or an
            // overlap, and stay empty afterwards.
            let mut both = rope
                .char_indices_in(range.clone())
                .map_err(|e| e.to_string())?;
            let (mut front, mut tail) = (Vec::new(), Vec::new());
            let mut from_front = true;
            loop {
                let item = if from_front {
                    both.next()
                } else {
                    both.next_back()
                };
                match item {
                    Some(item) if from_front => front.push(item),
                    Some(item) => tail.push(item),
                    None => break,
                }
                from_front = !from_front;
            }
            if both.next().is_some() || both.next_back().is_some() {
                return fail("a fused iterator restarted:");
            }
            tail.reverse();
            front.extend(tail);
            if front != want {
                return fail("alternating char indices");
            }
            let clipped: String = rope
                .chunks_in(range.clone())
                .map_err(|e| e.to_string())?
                .collect();
            if clipped != text[range] {
                return fail("clipped chunks");
            }
        }
    }
    Ok(())
}

/// A rope of at most `limit` bytes built by an edit history, so that tree
/// shapes vary, and its model.
fn history(rng: &mut Pcg32, size: u32, limit: usize) -> (Rope, String) {
    let mut model = String::new();
    push_text(rng, &mut model, size);
    let mut rope = Rope::from(model.as_str());
    for _ in 0..rng.range(0..12u32) {
        let at = floor_boundary(&model, rng.range(0..model.len() as u32 + 1) as usize);
        match rng.range(0..5u32) {
            0 => {
                let mut text = String::new();
                let count = rng.range(0..size / 2 + 2);
                push_text(rng, &mut text, count);
                rope.insert(at, &text).expect("a valid insert");
                model.insert_str(at, &text);
            }
            1 => {
                let end = floor_boundary(&model, at + rng.range(0..40u32) as usize);
                rope.remove(at..end).expect("a valid removal");
                model.replace_range(at..end, "");
            }
            2 => {
                let mut swapped = rope.split_off(at).expect("a valid split");
                swapped.append(rope);
                rope = swapped;
                model = format!("{}{}", &model[at..], &model[..at]);
            }
            3 => {
                let mut text = String::new();
                let count = rng.range(0..size + 1);
                push_text(rng, &mut text, count);
                rope.append(Rope::from(text.as_str()));
                model.push_str(&text);
            }
            _ => {
                // Rebuild from pieces of 0-23 bytes.
                let mut builder = crate::RopeBuilder::new();
                let mut from = 0;
                while from < model.len() {
                    let to = floor_boundary(&model, from + rng.range(0..24u32) as usize);
                    builder.push_str(&model[from..to]);
                    from = to.max(model.ceil_char_boundary(from + usize::from(to == from)));
                    builder.push_str(&model[to..from]);
                }
                rope = builder.finish();
            }
        }
    }
    let keep = floor_boundary(&model, limit);
    rope.remove(keep..model.len()).expect("a valid truncation");
    model.truncate(keep);
    (rope, model)
}

/// Acceptance 2: every conversion round-trips, on 2,000 ropes of up to 300
/// bytes with varied tree shapes, at every position, offset, line and
/// column, in every unit, and every invalid input gives its exact error.
#[test]
fn every_conversion_round_trips() {
    check_sized(
        "every conversion round-trips",
        2_000,
        |rng, size| history(rng, size, 300),
        |(rope, model)| sweep(rope, model),
    );
}

#[test]
fn fixed_texts_round_trip() {
    let texts = [
        "",
        "\r",
        "\n",
        "\r\n",
        "\n\r",
        "\r\r\n",
        "a\r\nb",
        "a😀b",
        "a\u{85}b\u{2028}c\u{2029}d\u{b}e\u{c}f",
        "line\n",
        "line\r\n",
        "line\r",
        "\r\n\r\n\r\r\n\n\n\r",
        "🇺🇸\r\n𐐀😀\r\n中€é\u{301}\r",
    ];
    for text in texts {
        sweep(&Rope::from(text), text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        // And split over many leaves at every position.
        for at in (0..=text.len()).filter(|&at| text.is_char_boundary(at)) {
            let mut rope = Rope::from(&text[..at]);
            rope.append(Rope::from(&text[at..]));
            sweep(&rope, text).unwrap_or_else(|e| panic!("{text:?} joined at {at}: {e}"));
        }
    }
}

#[test]
fn line_counts_follow_the_break_rules() {
    for (text, lines) in [
        ("", 1),
        ("\r", 2),
        ("\n", 2),
        ("\r\n", 2),
        ("\n\r", 3),
        ("\r\r\n", 3),
        ("a\u{85}b\u{2028}c\u{2029}d\u{b}e\u{c}f", 1),
        ("x\n", 2),
    ] {
        assert_eq!(Rope::from(text).len_lines(), lines, "{text:?}");
    }
}

#[test]
fn the_offset_between_cr_and_lf_is_one_column_past_the_content() {
    let rope = Rope::from("a\r\nb");
    for unit in UNITS {
        assert_eq!(
            rope.byte_to_line_col(2, unit).ok(),
            Some(LineCol::new(0, 2))
        );
        assert_eq!(
            rope.line_col_to_byte(LineCol::new(0, 2), unit).ok(),
            Some(2)
        );
        assert!(matches!(
            rope.line_col_to_byte(LineCol::new(0, 3), unit),
            Err(TextError::ColumnOutOfBounds { max: 2, .. })
        ));
        assert_eq!(
            rope.byte_to_line_col(3, unit).ok(),
            Some(LineCol::new(1, 0))
        );
    }
    assert_eq!(rope.byte_to_line(2).ok(), Some(0));
    let span = rope.line_span(0).ok();
    assert_eq!(
        span,
        Some(LineSpan {
            start: 0,
            content_end: 1,
            line_break: Some(LineBreak::CrLf)
        })
    );
    // An LF line ends at its content length.
    let rope = Rope::from("ab\ncd");
    assert!(matches!(
        rope.line_col_to_byte(LineCol::new(0, 3), Unit::Byte),
        Err(TextError::ColumnOutOfBounds { max: 2, .. })
    ));
}

#[test]
fn astral_chars_are_two_utf16_units() {
    let rope = Rope::from("a😀b");
    assert_eq!(rope.len_in(Unit::Byte), 6);
    assert_eq!(rope.len_in(Unit::Utf16), 4);
    assert_eq!(rope.len_in(Unit::Char), 3);
    for (byte, utf16, chars) in [(0, 0, 0), (1, 1, 1), (5, 3, 2), (6, 4, 3)] {
        assert_eq!(rope.byte_to_offset(byte, Unit::Utf16).ok(), Some(utf16));
        assert_eq!(rope.byte_to_offset(byte, Unit::Char).ok(), Some(chars));
        assert_eq!(
            rope.byte_to_line_col(byte, Unit::Utf16).ok(),
            Some(LineCol::new(0, utf16))
        );
        assert_eq!(
            rope.byte_to_line_col(byte, Unit::Char).ok(),
            Some(LineCol::new(0, chars))
        );
    }
    assert!(matches!(
        rope.offset_to_byte(2, Unit::Utf16),
        Err(TextError::OffsetInsideChar {
            offset: 2,
            unit: Unit::Utf16
        })
    ));
    assert!(matches!(
        rope.line_col_to_byte(LineCol::new(0, 2), Unit::Utf16),
        Err(TextError::ColumnInsideChar {
            pos: LineCol { line: 0, col: 2 },
            unit: Unit::Utf16
        })
    ));
    assert!(matches!(
        rope.offset_to_byte(5, Unit::Utf16),
        Err(TextError::OffsetOutOfBounds {
            offset: 5,
            unit: Unit::Utf16,
            len: 4
        })
    ));
    assert!(matches!(
        rope.byte_to_offset(2, Unit::Utf16),
        Err(TextError::NotCharBoundary { byte: 2 })
    ));
}

#[test]
fn errors_are_checked_in_the_documented_order() {
    let rope = Rope::from("é\n");
    assert!(matches!(
        rope.line_col_to_byte(LineCol::new(2, 99), Unit::Byte),
        Err(TextError::LineOutOfBounds {
            line: 2,
            len_lines: 2
        })
    ));
    assert!(matches!(
        rope.line_col_to_byte(LineCol::new(0, 3), Unit::Byte),
        Err(TextError::ColumnOutOfBounds { max: 2, .. })
    ));
    assert!(matches!(
        rope.line_col_to_byte(LineCol::new(0, 1), Unit::Byte),
        Err(TextError::ColumnInsideChar { .. })
    ));
    assert!(matches!(
        rope.byte_to_offset(9, Unit::Char),
        Err(TextError::ByteOutOfBounds { byte: 9, len: 3 })
    ));
    assert!(matches!(
        rope.offset_to_byte(9, Unit::Byte),
        Err(TextError::ByteOutOfBounds { byte: 9, len: 3 })
    ));
    assert!(matches!(
        rope.offset_to_byte(1, Unit::Byte),
        Err(TextError::NotCharBoundary { byte: 1 })
    ));
    assert!(matches!(
        rope.offset_to_byte(3, Unit::Char),
        Err(TextError::OffsetOutOfBounds {
            offset: 3,
            unit: Unit::Char,
            len: 2
        })
    ));
}

#[test]
fn line_breaks_and_spans() {
    assert_eq!(
        [LineBreak::Lf, LineBreak::CrLf, LineBreak::Cr].map(LineBreak::byte_len),
        [1, 2, 1]
    );
    assert_eq!(
        [LineBreak::Lf, LineBreak::CrLf, LineBreak::Cr].map(LineBreak::as_str),
        ["\n", "\r\n", "\r"]
    );
    let span = LineSpan {
        start: 4,
        content_end: 9,
        line_break: Some(LineBreak::CrLf),
    };
    assert_eq!((span.end(), span.content(), span.full()), (11, 4..9, 4..11));
    let last = LineSpan {
        line_break: None,
        ..span
    };
    assert_eq!((last.end(), last.full()), (9, 4..9));
    assert_eq!(LineCol::new(2, 5), LineCol { line: 2, col: 5 });
    assert!(LineCol::new(1, 9) < LineCol::new(2, 0));
}

/// UTF-16 counts a 4-byte char as two units whatever its lead byte (F0 to
/// F4): a variation selector (U+E0100), a tag (U+E0067), private use
/// (U+F0000, U+10FFFF), and the first and last char of every supplementary
/// plane.
#[test]
fn utf16_counts_chars_of_every_supplementary_plane() {
    let mut chars = vec![
        '\u{E0100}',
        '\u{E0067}',
        '\u{F0000}',
        '\u{10FFFF}',
        '\u{40000}',
    ];
    for plane in 1..=16u32 {
        for code in [plane << 16, (plane << 16) | 0xFFFF] {
            chars.push(char::from_u32(code).expect("a supplementary scalar value"));
        }
    }
    for c in chars {
        let text = format!("a{c}b\n{c}");
        let rope = Rope::from(text.as_str());
        assert_eq!(
            rope.len_in(Unit::Utf16),
            text.encode_utf16().count(),
            "{c:?}"
        );
        let b = 1 + c.len_utf8();
        assert_eq!(rope.byte_to_offset(b, Unit::Utf16).ok(), Some(3), "{c:?}");
        assert_eq!(
            rope.byte_to_line_col(b, Unit::Utf16).ok(),
            Some(LineCol::new(0, 3)),
            "{c:?}"
        );
        assert_eq!(rope.offset_to_byte(3, Unit::Utf16).ok(), Some(b), "{c:?}");
        assert_eq!(
            rope.line_col_to_byte(LineCol::new(1, 2), Unit::Utf16).ok(),
            Some(text.len()),
            "{c:?}"
        );
        sweep(&rope, &text).unwrap_or_else(|e| panic!("{c:?}: {e}"));
        // And a run of them over many leaves.
        let run = format!("{c}x").repeat(9);
        let mut rope = Rope::from(&run[..15]);
        rope.append(Rope::from(&run[15..]));
        sweep(&rope, &run).unwrap_or_else(|e| panic!("{c:?} run: {e}"));
    }
}

/// `usize::MAX` and `usize::MAX - 1`, in every argument of every conversion
/// and in every unit, give their exact errors and never overflow.
#[test]
fn the_largest_values_give_their_exact_errors() {
    let long = format!("{}\r\n{}", "é😀".repeat(8), "x中".repeat(10));
    let texts = ["", "a", "\r\n", "😀", "a\rb\nc\r\n", long.as_str()];
    for text in texts {
        let mut rope = Rope::from(&text[..text.floor_char_boundary(text.len() / 2)]);
        rope.append(Rope::from(
            &text[text.floor_char_boundary(text.len() / 2)..],
        ));
        let o = Oracle::new(text);
        let n = text.len();
        let lines = o.starts.len();
        let checked = || -> Result<(), String> {
            for big in [usize::MAX, usize::MAX - 1] {
                let byte_out = || TextError::ByteOutOfBounds { byte: big, len: n };
                let line_out = || TextError::LineOutOfBounds {
                    line: big,
                    len_lines: lines,
                };
                expect(
                    || format!("byte_to_line({big})"),
                    rope.byte_to_line(big),
                    Err(byte_out()),
                )?;
                expect(
                    || format!("line_to_byte({big})"),
                    rope.line_to_byte(big),
                    Err(line_out()),
                )?;
                expect(
                    || format!("line_span({big})"),
                    rope.line_span(big),
                    Err(line_out()),
                )?;
                for (k, unit) in UNITS.into_iter().enumerate() {
                    expect(
                        || format!("byte_to_offset({big}, {unit:?})"),
                        rope.byte_to_offset(big, unit),
                        Err(byte_out()),
                    )?;
                    expect(
                        || format!("byte_to_line_col({big}, {unit:?})"),
                        rope.byte_to_line_col(big, unit),
                        Err(byte_out()),
                    )?;
                    let offset_out = match unit {
                        Unit::Byte => byte_out(),
                        _ => TextError::OffsetOutOfBounds {
                            offset: big,
                            unit,
                            len: o.len(k),
                        },
                    };
                    expect(
                        || format!("offset_to_byte({big}, {unit:?})"),
                        rope.offset_to_byte(big, unit),
                        Err(offset_out),
                    )?;
                    for col in [0, 1, usize::MAX - 1, usize::MAX] {
                        let pos = LineCol::new(big, col);
                        expect(
                            || format!("line_col_to_byte({pos:?}, {unit:?})"),
                            rope.line_col_to_byte(pos, unit),
                            Err(line_out()),
                        )?;
                    }
                    for line in 0..lines {
                        let max = o.off[k][o.last_byte(line)] - o.off[k][o.starts[line]];
                        let pos = LineCol::new(line, big);
                        expect(
                            || format!("line_col_to_byte({pos:?}, {unit:?})"),
                            rope.line_col_to_byte(pos, unit),
                            Err(TextError::ColumnOutOfBounds { pos, unit, max }),
                        )?;
                    }
                }
            }
            Ok(())
        };
        checked().unwrap_or_else(|e| panic!("{text:?}: {e}"));
    }
}

/// A hand-built span ends at `usize::MAX` at most: `end` saturates rather
/// than overflow.
#[test]
fn a_hand_built_span_ends_at_most_at_usize_max() {
    let max = usize::MAX;
    for (content_end, line_break, end) in [
        (max, Some(LineBreak::CrLf), max),
        (max - 1, Some(LineBreak::CrLf), max),
        (max - 2, Some(LineBreak::CrLf), max),
        (max, Some(LineBreak::Lf), max),
        (max - 1, Some(LineBreak::Cr), max),
        (max, None, max),
        (max - 3, Some(LineBreak::CrLf), max - 1),
    ] {
        let span = LineSpan {
            start: 7,
            content_end,
            line_break,
        };
        assert_eq!(
            (span.end(), span.content(), span.full()),
            (end, 7..content_end, 7..end),
            "{span:?}"
        );
    }
}
