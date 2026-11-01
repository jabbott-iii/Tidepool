//! Position round trips at the real rope shape (leaves of 1-4 KiB, nodes of
//! 8-16 children).
//!
//! - 50 texts of about 10 KiB, each of several leaves, built in different
//!   ways so that seams land next to line breaks and inside runs of 4-byte
//!   chars. Every property P1-P9 of the design is checked at every byte
//!   within 48 bytes of every chunk seam and of both ends of the text, at
//!   every 61st byte elsewhere, at the offsets around each of those bytes,
//!   at every line, and at every column of the lines near a seam and of
//!   every 5th line.
//! - Five texts of about 5 KiB in 1 KiB leaves, at every byte, offset, line
//!   and column.
//! - Three texts of 8-14 KiB, each with two lines longer than a leaf
//!   (4 KiB): one ending in LF, CR or CR LF, and the last, with no break. In
//!   leaves of 1-4 KiB, at every byte, offset, line and column.
//! - `usize::MAX` and `usize::MAX - 1` in every argument and unit.
//! - 10,000 sampled positions of 1 MiB.
//!
//! Checking every position of every 10 KiB text would take minutes in a
//! debug build: each conversion there scans a leaf of up to 4 KiB. The
//! tiny-shape unit test `pos::tests::every_conversion_round_trips` checks
//! every position of 2,000 ropes. The oracle works every fact out from the
//! definitions in `tidepool_text::pos` with `str` methods only.

use std::fmt::Debug;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::check;
use tidepool_text::{LineBreak, LineCol, LineSpan, Rope, RopeBuilder, TextError, Unit};

const UNITS: [Unit; 3] = [Unit::Byte, Unit::Utf16, Unit::Char];

/// `Ok` if `got` is `want`, comparing errors by their `Debug` form (numbers
/// only).
fn expect<T: PartialEq + Debug>(
    what: impl FnOnce() -> String,
    got: Result<T, TextError>,
    want: Result<T, TextError>,
) -> Result<(), String> {
    let same = match (&got, &want) {
        (Ok(a), Ok(b)) => a == b,
        (Err(a), Err(b)) => format!("{a:?}") == format!("{b:?}"),
        _ => false,
    };
    if same {
        Ok(())
    } else {
        Err(format!("{}: got {got:?}, want {want:?}", what()))
    }
}

fn byte_error<T>(text: &str, byte: usize) -> Result<T, TextError> {
    Err(if byte > text.len() {
        TextError::ByteOutOfBounds {
            byte,
            len: text.len(),
        }
    } else {
        TextError::NotCharBoundary { byte }
    })
}

/// Every position fact of a text. Offsets are kept for valid bytes only, so
/// that 1 MiB stays small.
struct Oracle<'t> {
    text: &'t str,
    /// The valid bytes, in order.
    valid: Vec<usize>,
    /// For each unit, the offset of each valid byte.
    offs: [Vec<usize>; 3],
    /// Line starts.
    starts: Vec<usize>,
    /// For each line, its content end and break.
    ends: Vec<(usize, Option<LineBreak>)>,
}

impl<'t> Oracle<'t> {
    fn new(text: &'t str) -> Self {
        let mut valid = Vec::with_capacity(text.len() + 1);
        let mut offs = [Vec::new(), Vec::new(), Vec::new()];
        let (mut utf16, mut chars) = (0, 0);
        for (at, c) in text.char_indices().chain([(text.len(), '\0')]) {
            valid.push(at);
            offs[0].push(at);
            offs[1].push(utf16);
            offs[2].push(chars);
            utf16 += c.len_utf16();
            chars += 1;
        }
        let mut starts = vec![0];
        let mut ends = Vec::new();
        let mut iter = text.char_indices().peekable();
        while let Some((at, c)) = iter.next() {
            let kind = match c {
                '\n' => LineBreak::Lf,
                '\r' if iter.peek().map(|&(_, next)| next) == Some('\n') => {
                    iter.next();
                    LineBreak::CrLf
                }
                '\r' => LineBreak::Cr,
                _ => continue,
            };
            ends.push((at, Some(kind)));
            starts.push(at + kind.as_str().len());
        }
        ends.push((text.len(), None));
        Self {
            text,
            valid,
            offs,
            starts,
            ends,
        }
    }

    fn len(&self, unit: usize) -> usize {
        self.offs[unit].last().copied().unwrap_or(0)
    }

    /// The offset of the valid byte `byte`.
    fn off(&self, unit: usize, byte: usize) -> usize {
        let index = self.valid.binary_search(&byte).unwrap_or(0);
        self.offs[unit][index]
    }

    /// The valid byte with `offset`, if any.
    fn byte_at(&self, unit: usize, offset: usize) -> Option<usize> {
        let index = self.offs[unit].binary_search(&offset).ok()?;
        Some(self.valid[index])
    }

    fn line(&self, byte: usize) -> usize {
        self.starts.partition_point(|&start| start <= byte) - 1
    }

    /// The last byte of `line`'s column space.
    fn last_byte(&self, line: usize) -> usize {
        match self.starts.get(line + 1) {
            None => self.text.len(),
            Some(&next) => {
                let index = self.valid.partition_point(|&byte| byte < next);
                self.valid[index - 1]
            }
        }
    }

    /// P1, P3 and P5 at a valid byte; P8 (conversions, boundary queries) at
    /// an invalid one.
    fn check_byte(&self, rope: &Rope, byte: usize) -> Result<(), String> {
        let text = self.text;
        if !text.is_char_boundary(byte) {
            for unit in UNITS {
                expect(
                    || format!("byte_to_offset({byte}, {unit:?})"),
                    rope.byte_to_offset(byte, unit),
                    byte_error(text, byte),
                )?;
                expect(
                    || format!("byte_to_line_col({byte}, {unit:?})"),
                    rope.byte_to_line_col(byte, unit),
                    byte_error(text, byte),
                )?;
            }
            expect(
                || format!("byte_to_line({byte})"),
                rope.byte_to_line(byte),
                byte_error(text, byte),
            )?;
            let mut edited = rope.clone();
            expect(
                || format!("insert({byte})"),
                edited.insert(byte, "\n"),
                byte_error(text, byte),
            )?;
            expect(
                || format!("remove({byte}..)"),
                edited.remove(byte..byte.max(text.len())),
                byte_error(text, byte),
            )?;
            if edited != *rope {
                return Err(format!("a refused edit at {byte} changed the rope"));
            }
            let floor = self.valid[self.valid.partition_point(|&b| b <= byte) - 1];
            let ceil = self
                .valid
                .get(self.valid.partition_point(|&b| b < byte))
                .copied()
                .unwrap_or(text.len());
            let bounds = (
                rope.is_char_boundary(byte),
                rope.floor_char_boundary(byte),
                rope.ceil_char_boundary(byte),
            );
            if bounds != (false, floor, ceil) {
                return Err(format!(
                    "at invalid byte {byte}: (is, floor, ceil) = {bounds:?}, want {:?}",
                    (false, floor, ceil)
                ));
            }
            return Ok(());
        }
        let line = self.line(byte);
        expect(
            || format!("byte_to_line({byte})"),
            rope.byte_to_line(byte),
            Ok(line),
        )?;
        for (k, unit) in UNITS.into_iter().enumerate() {
            let offset = self.off(k, byte);
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
            let pos = LineCol::new(line, offset - self.off(k, self.starts[line]));
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
        if !rope.is_char_boundary(byte)
            || rope.floor_char_boundary(byte) != byte
            || rope.ceil_char_boundary(byte) != byte
        {
            return Err(format!(
                "the boundary queries are wrong at valid byte {byte}"
            ));
        }
        Ok(())
    }

    /// P2 at one offset.
    fn check_offset(&self, rope: &Rope, k: usize, offset: usize) -> Result<(), String> {
        let unit = UNITS[k];
        let len = self.len(k);
        let want = match self.byte_at(k, offset) {
            Some(byte) => Ok(byte),
            None if unit == Unit::Byte => byte_error(self.text, offset),
            None if offset > len => Err(TextError::OffsetOutOfBounds { offset, unit, len }),
            None => Err(TextError::OffsetInsideChar { offset, unit }),
        };
        expect(
            || format!("offset_to_byte({offset}, {unit:?})"),
            rope.offset_to_byte(offset, unit),
            want,
        )
    }

    /// P4 and P7 at one line.
    fn check_line(&self, rope: &Rope, line: usize) -> Result<(), String> {
        let lines = self.starts.len();
        let Some(&start) = self.starts.get(line) else {
            let out = || {
                Err::<usize, _>(TextError::LineOutOfBounds {
                    line,
                    len_lines: lines,
                })
            };
            expect(
                || format!("line_to_byte({line})"),
                rope.line_to_byte(line),
                out(),
            )?;
            return expect(
                || format!("line_span({line})"),
                rope.line_span(line).map(|span| span.start),
                out(),
            );
        };
        expect(
            || format!("line_to_byte({line})"),
            rope.line_to_byte(line),
            Ok(start),
        )?;
        expect(
            || format!("byte_to_line({start})"),
            rope.byte_to_line(start),
            Ok(line),
        )?;
        let (content_end, line_break) = self.ends[line];
        expect(
            || format!("line_span({line})"),
            rope.line_span(line),
            Ok(LineSpan {
                start,
                content_end,
                line_break,
            }),
        )
    }

    /// P6 at one line and column.
    fn check_column(&self, rope: &Rope, k: usize, pos: LineCol) -> Result<(), String> {
        let unit = UNITS[k];
        let lines = self.starts.len();
        let want = match self.starts.get(pos.line) {
            None => Err(TextError::LineOutOfBounds {
                line: pos.line,
                len_lines: lines,
            }),
            Some(&start) => {
                let base = self.off(k, start);
                let max = self.off(k, self.last_byte(pos.line)) - base;
                if pos.col > max {
                    Err(TextError::ColumnOutOfBounds { pos, unit, max })
                } else {
                    self.byte_at(k, base + pos.col)
                        .ok_or(TextError::ColumnInsideChar { pos, unit })
                }
            }
        };
        expect(
            || format!("line_col_to_byte({pos:?}, {unit:?})"),
            rope.line_col_to_byte(pos, unit),
            want,
        )
    }

    /// The largest column of `line` in unit `k`, or 0 past the last line.
    fn max_col(&self, k: usize, line: usize) -> usize {
        match self.starts.get(line) {
            Some(&start) => self.off(k, self.last_byte(line)) - self.off(k, start),
            None => 0,
        }
    }

    /// P9, and everything else at every position.
    fn sweep(&self, rope: &Rope) -> Result<(), String> {
        let bytes: Vec<usize> = (0..=self.text.len() + 2).collect();
        let lines: Vec<usize> = (0..self.starts.len() + 2).collect();
        self.sweep_at(rope, &bytes, &lines)
    }

    /// P9; P1, P3, P5 and P8 at `bytes`; P2 at the offsets of each of those
    /// bytes and one either side; P4 and P7 at every line; and P6 at every
    /// column of `lines` (up to the last + 2), in every unit.
    fn sweep_at(&self, rope: &Rope, bytes: &[usize], lines: &[usize]) -> Result<(), String> {
        rope.__check_invariants()?;
        if *rope != *self.text {
            return Err("the rope's text differs".to_string());
        }
        for (k, unit) in UNITS.into_iter().enumerate() {
            if rope.len_in(unit) != self.len(k) {
                return Err(format!("len_in({unit:?})"));
            }
        }
        if rope.len_lines() != self.starts.len() {
            return Err("len_lines".to_string());
        }
        for &byte in bytes {
            self.check_byte(rope, byte)?;
        }
        for k in 0..3 {
            let mut offsets: Vec<usize> = bytes
                .iter()
                .map(|&byte| {
                    let index = self.valid.partition_point(|&b| b <= byte) - 1;
                    let past = byte - self.valid[index];
                    self.offs[k][index] + past
                })
                .flat_map(|offset| [offset.saturating_sub(1), offset, offset + 1])
                .collect();
            offsets.dedup();
            for offset in offsets {
                self.check_offset(rope, k, offset)?;
            }
        }
        for line in 0..self.starts.len() + 2 {
            self.check_line(rope, line)?;
        }
        for k in 0..3 {
            for &line in lines {
                for col in 0..=self.max_col(k, line) + 2 {
                    self.check_column(rope, k, LineCol::new(line, col))?;
                }
            }
        }
        Ok(())
    }

    /// The bytes and lines `sweep_at` checks for a rope with these chunks:
    /// everything within 48 bytes of a seam or an end, every 61st byte, the
    /// lines those windows touch, every 5th line, and two lines past the
    /// end.
    fn selection(&self, rope: &Rope) -> (Vec<usize>, Vec<usize>) {
        let n = self.text.len();
        let mut seams = vec![0, n + 2];
        let mut offset = 0;
        for chunk in rope.chunks() {
            offset += chunk.len();
            seams.push(offset);
        }
        let near = |byte: usize| seams.iter().any(|&seam| byte.abs_diff(seam) <= 48);
        let bytes: Vec<usize> = (0..=n + 2)
            .filter(|&byte| byte % 61 == 0 || near(byte))
            .collect();
        let lines_total = self.starts.len();
        let mut lines: Vec<usize> = (0..lines_total + 2)
            .filter(|&line| {
                line % 5 == 0
                    || line >= lines_total
                    || near(self.starts[line])
                    || self.starts.get(line + 1).is_some_and(|&next| near(next))
            })
            .collect();
        lines.dedup();
        (bytes, lines)
    }
}

/// Text from a few styles: CR LF code, LF prose, lone CRs, long lines and
/// runs of 4-byte chars (with every lead byte, F0 to F4), so that leaf seams
/// land next to breaks and inside multi-byte runs.
fn text(rng: &mut Pcg32, bytes: usize) -> String {
    const PIECES: [&str; 16] = [
        "fn x() {\r\n",
        "    let a = 1;\r\n",
        "}\r\n",
        "prose line\n",
        "\n",
        "\r",
        "\r\n",
        "😀😀😀",
        "𐐀",
        "\u{40000}\u{80000}\u{E0100}",
        "\u{E0067}\u{F0000}\u{10FFFF}",
        "中文",
        "é\u{301}",
        "\u{85}\u{2028}",
        "🇺🇸",
        "x",
    ];
    let mut out = String::with_capacity(bytes + 32);
    while out.len() < bytes {
        if rng.range(0..40u32) == 0 {
            // A long line.
            for _ in 0..rng.range(100..600u32) {
                out.push(char::from(rng.range(0x20..0x7Fu32) as u8));
            }
        } else {
            out.push_str(rng.choose(&PIECES).copied().unwrap_or("x"));
        }
    }
    out
}

/// A rope of `text` built one of several ways, so that leaf seams vary.
fn build(rng: &mut Pcg32, text: &str) -> Rope {
    let floor = |at: usize| text.floor_char_boundary(at.min(text.len()));
    match rng.range(0..4u32) {
        0 => Rope::from(text),
        1 => {
            let mut builder = RopeBuilder::new();
            let mut from = 0;
            while from < text.len() {
                let to = text.ceil_char_boundary(from + rng.range(1..1500u32) as usize);
                builder.push_str(&text[from..to]);
                from = to;
            }
            builder.finish()
        }
        2 => {
            // Appends of pieces that end in a CR, or start with an LF.
            let mut rope = Rope::new();
            let mut from = 0;
            while from < text.len() {
                let mut to = floor(from + rng.range(1..5000u32) as usize);
                if let Some(cr) = text[from..to].rfind('\r') {
                    to = from + cr + 1;
                }
                let to = to.max(text.ceil_char_boundary(from + 1));
                rope.append(Rope::from(&text[from..to]));
                from = to;
            }
            rope
        }
        _ => {
            // Inserts at random places, from an empty rope.
            let mut rope = Rope::new();
            let mut pieces = Vec::new();
            let mut from = 0;
            while from < text.len() {
                let to = text.ceil_char_boundary(from + rng.range(1..3000u32) as usize);
                pieces.push(from..to);
                from = to;
            }
            // Insert the pieces in a shuffled order, each where it belongs
            // among those already in.
            let mut order: Vec<usize> = (0..pieces.len()).collect();
            rng.shuffle(&mut order);
            let mut placed: Vec<usize> = Vec::new();
            for index in order {
                let at: usize = placed
                    .iter()
                    .filter(|&&other| other < index)
                    .map(|&other| pieces[other].len())
                    .sum();
                rope.insert(at, &text[pieces[index].clone()])
                    .expect("a valid insert");
                placed.push(index);
            }
            rope
        }
    }
}

#[test]
fn ten_kibibyte_texts_round_trip_near_every_seam() {
    check(
        "10 KiB texts round-trip near every seam",
        50,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let text = text(&mut rng, 10 * 1024);
            let rope = build(&mut rng, &text);
            let chunks = rope.chunks().count();
            if chunks < 3 {
                return Err(format!("only {chunks} chunks"));
            }
            let oracle = Oracle::new(&text);
            let (bytes, lines) = oracle.selection(&rope);
            oracle.sweep_at(&rope, &bytes, &lines)
        },
    );
}

#[test]
fn every_position_of_a_text_in_small_leaves_round_trips() {
    check(
        "every position of a text in small leaves round-trips",
        5,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let text = text(&mut rng, 5 * 1024);
            // Appends of about 1 KiB keep leaves near the minimum, so each
            // scan is short and there are several seams.
            let mut rope = Rope::new();
            let mut from = 0;
            while from < text.len() {
                let to = text.ceil_char_boundary(from + rng.range(1024..1300u32) as usize);
                rope.append(Rope::from(&text[from..to]));
                from = to;
            }
            let chunks = rope.chunks().count();
            if chunks < 4 {
                return Err(format!("only {chunks} chunks"));
            }
            Oracle::new(&text).sweep(&rope)
        },
    );
}

/// A rope of `text` made by appending pieces of 1-4 KiB, so that its leaves
/// are about that size.
fn appended(rng: &mut Pcg32, text: &str) -> Rope {
    let mut rope = Rope::new();
    let mut from = 0;
    while from < text.len() {
        let to = text.floor_char_boundary(from + rng.range(1_024..4_097u32) as usize);
        rope.append(Rope::from(&text[from..to]));
        from = to;
    }
    rope
}

/// A short first line, a line of 4,200-7,000 bytes ending in `ending`, and
/// a last line as long with no break, both longer than a leaf, of 1- to
/// 4-byte chars, in leaves of 1-4 KiB: every byte, offset, line and column
/// (up to each line's last + 2), in every unit, with the exact errors.
fn long_lines_round_trip(ending: &'static str) {
    const CHARS: [&str; 8] = [
        "x",
        " ",
        "é",
        "\u{85}",
        "中",
        "😀",
        "\u{E0100}",
        "\u{10FFFF}",
    ];
    let long_line = |rng: &mut Pcg32, text: &mut String| {
        let end = text.len() + rng.range(4_200..7_000u32) as usize;
        while text.len() < end {
            text.push_str(rng.choose(&CHARS).copied().unwrap_or("x"));
        }
    };
    check(
        &format!("lines longer than a leaf, one ending in {ending:?}, round-trip"),
        1,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let mut text = String::from("head\r\n");
            long_line(&mut rng, &mut text);
            text.push_str(ending);
            long_line(&mut rng, &mut text);
            let rope = appended(&mut rng, &text);
            let oracle = Oracle::new(&text);
            let starts = &oracle.starts;
            if starts.len() != 3
                || starts[2] - starts[1] <= 4_096
                || text.len() - starts[2] <= 4_096
            {
                return Err(format!("line starts {starts:?} of {} bytes", text.len()));
            }
            oracle.sweep(&rope)
        },
    );
}

#[test]
fn a_line_longer_than_a_leaf_ending_in_lf_round_trips() {
    long_lines_round_trip("\n");
}

#[test]
fn a_line_longer_than_a_leaf_ending_in_cr_round_trips() {
    long_lines_round_trip("\r");
}

#[test]
fn a_line_longer_than_a_leaf_ending_in_crlf_round_trips() {
    long_lines_round_trip("\r\n");
}

/// `usize::MAX` and `usize::MAX - 1`, as a byte, an offset, a line and a
/// column, in every unit, give their exact errors and never overflow.
#[test]
fn the_largest_values_give_their_exact_errors() {
    let long = format!(
        "{}\r\n{}",
        "é😀中x".repeat(700),
        "x\u{10FFFF}".repeat(1_500)
    );
    let mut rng = Pcg32::new(17, 3);
    for text in ["", "a", "\r\n", "😀", "a\rb\nc\r\n", long.as_str()] {
        let rope = appended(&mut rng, text);
        let oracle = Oracle::new(text);
        let lines = oracle.starts.len();
        let checked = || -> Result<(), String> {
            for big in [usize::MAX, usize::MAX - 1] {
                oracle.check_byte(&rope, big)?;
                oracle.check_line(&rope, big)?;
                for k in 0..3 {
                    oracle.check_offset(&rope, k, big)?;
                    for col in [0, 1, usize::MAX - 1, usize::MAX] {
                        oracle.check_column(&rope, k, LineCol::new(big, col))?;
                    }
                    for line in 0..lines {
                        oracle.check_column(&rope, k, LineCol::new(line, big))?;
                    }
                }
            }
            Ok(())
        };
        checked().unwrap_or_else(|e| panic!("a text of {} bytes: {e}", text.len()));
    }
}

#[test]
fn sampled_positions_of_one_mebibyte_round_trip() {
    check(
        "sampled positions of 1 MiB round-trip",
        1,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let text = text(&mut rng, 1 << 20);
            let rope = build(&mut rng, &text);
            rope.__check_invariants()?;
            let oracle = Oracle::new(&text);
            let lines = oracle.starts.len();
            for sample in 0..10_000u32 {
                let k = rng.range(0..3u32) as usize;
                match sample % 4 {
                    0 => {
                        let byte = rng.range(0..text.len() as u32 + 3) as usize;
                        oracle.check_byte(&rope, byte)?;
                    }
                    1 => {
                        let offset = rng.range(0..oracle.len(k) as u32 + 3) as usize;
                        oracle.check_offset(&rope, k, offset)?;
                    }
                    2 => oracle.check_line(&rope, rng.range(0..lines as u32 + 2) as usize)?,
                    _ => {
                        let line = rng.range(0..lines as u32 + 1) as usize;
                        let col = rng.range(0..oracle.max_col(k, line) as u32 + 3) as usize;
                        oracle.check_column(&rope, k, LineCol::new(line, col))?;
                    }
                }
            }
            Ok(())
        },
    );
}
