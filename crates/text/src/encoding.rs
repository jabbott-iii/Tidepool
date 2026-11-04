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

//! Encodings: UTF-8 and UTF-16 LE/BE, told apart by a byte-order mark only,
//! decoded losslessly from a stream and encoded chunk by chunk.
//!
//! - **Detection** looks at the first bytes only: `EF BB BF` is UTF-8 with a
//!   BOM, `FF FE` UTF-16 LE, `FE FF` UTF-16 BE, and anything else UTF-8
//!   without one. There are no heuristics: no Latin-1, no UTF-16 without a
//!   BOM (it reads as UTF-8), no UTF-32. A UTF-32 LE BOM (`FF FE 00 00`)
//!   reads as UTF-16 LE followed by U+0000.
//! - **The BOM** is not part of the text; [`Decoded::detected`] says whether
//!   there was one.
//! - **Decoding never fails.** In UTF-8, each maximal subpart of an
//!   ill-formed sequence (Unicode §3.9, as in `String::from_utf8_lossy`)
//!   becomes one U+FFFD. In UTF-16, each unpaired surrogate becomes one
//!   U+FFFD, and so does an odd last byte.
//! - **Lossless.** A [`Decoded`] keeps each run of replaced bytes beside the
//!   text ([`Decoded::replaced`]; touching runs merge), so
//!   [`Decoded::original_bytes`] gives back the input exactly, BOM
//!   included, for every input. A run costs its original bytes (at most
//!   three for each U+FFFD) and a head of about three bytes, so the runs of
//!   a binary file take at most about twice the memory its U+FFFDs take in
//!   the rope, and a long run costs one head.
//! - **Read-only.** A text with any replacement has no save
//!   [`format`](Decoded::format): writing it would change the bytes that
//!   were not text.
//! - **Streaming.** A [`Decoder`] takes the bytes in pieces of any size and
//!   builds the text with a [`RopeBuilder`], so a large file is never held
//!   as one `String`. However the input is split, the result is the same.
//! - **Encoding** ([`encode`]) writes a rope chunk by chunk, the BOM first
//!   if the format has one. For every input decoded without a replacement,
//!   encoding the text in the [`detected`](Decoded::detected) format gives
//!   back the input byte for byte. Line breaks are text (ADR-0019), so
//!   nothing here changes them.

use std::fmt;
use std::io::{self, Write};
use std::ops::Range;

use crate::{Rope, RopeBuilder};

#[cfg(test)]
mod tests;

/// U+FFFD REPLACEMENT CHARACTER, which stands for each replaced sequence.
const REPLACEMENT: char = '\u{FFFD}';

/// The length of [`REPLACEMENT`] in UTF-8.
const REPLACEMENT_LEN: usize = REPLACEMENT.len_utf8();

/// Decoded text is gathered in pieces of about this many bytes before it
/// goes to the rope builder.
const SCRATCH: usize = 4096;

/// A text encoding that Tidepool reads and writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// UTF-8.
    #[default]
    Utf8,
    /// UTF-16, little-endian.
    Utf16Le,
    /// UTF-16, big-endian.
    Utf16Be,
}

impl Encoding {
    /// The byte-order mark: `EF BB BF`, `FF FE` or `FE FF`.
    pub const fn bom(self) -> &'static [u8] {
        match self {
            Self::Utf8 => &[0xEF, 0xBB, 0xBF],
            Self::Utf16Le => &[0xFF, 0xFE],
            Self::Utf16Be => &[0xFE, 0xFF],
        }
    }

    /// The name to show: `UTF-8`, `UTF-16 LE` or `UTF-16 BE`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
}

/// How a text is stored as bytes: an encoding, and whether its byte-order
/// mark comes first.
///
/// The default, [`Format::UTF8`], is UTF-8 without a BOM: the format of new
/// files. A UTF-16 format without a BOM can be written, but it reads back
/// as UTF-8, because detection is by BOM only. Without a BOM, a text whose
/// first character is written as a BOM reads back in that BOM's format,
/// without the character: U+FEFF in any encoding, or U+FFFE in UTF-16,
/// which is the other byte order's BOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Format {
    /// The encoding.
    pub encoding: Encoding,
    /// Whether the bytes start with the encoding's byte-order mark.
    pub bom: bool,
}

impl Format {
    /// UTF-8 without a BOM.
    pub const UTF8: Format = Format::new(Encoding::Utf8, false);

    /// The format of `encoding`, with or without its BOM.
    pub const fn new(encoding: Encoding, bom: bool) -> Self {
        Self { encoding, bom }
    }
}

impl Default for Format {
    /// [`Format::UTF8`].
    fn default() -> Self {
        Self::UTF8
    }
}

/// Decodes bytes pushed in pieces of any size: the format from the first
/// bytes, then the text, with each ill-formed sequence replaced and kept.
///
/// [`decode`] does it for bytes already in memory. There is no `Debug`,
/// because the pending text may hold secrets.
#[derive(Default)]
pub struct Decoder {
    /// What the bytes are being read as.
    mode: Mode,
    /// While `mode` is `Detecting`: the bytes so far, a proper prefix of a
    /// BOM.
    head: Small,
    /// The format the BOM named, or UTF-8 without one.
    format: Format,
    /// The decoded text.
    text: Sink,
    /// The runs of replaced bytes.
    runs: Runs,
    /// UTF-8: the start of a sequence that the last push cut off (1 to 3
    /// bytes).
    partial: Small,
    /// UTF-16: a byte waiting for the other half of its code unit.
    odd: Option<u8>,
    /// UTF-16: the two bytes of a high surrogate waiting for a low one.
    high: Option<[u8; 2]>,
}

/// What a [`Decoder`] reads its bytes as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    /// Not yet known: the bytes so far could start a BOM.
    #[default]
    Detecting,
    /// UTF-8.
    Utf8,
    /// UTF-16 in the given byte order.
    Utf16 {
        /// Big-endian, else little-endian.
        big_endian: bool,
    },
}

impl Decoder {
    /// A decoder that has seen no bytes yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `bytes`, which follow those pushed before.
    pub fn push(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while self.mode == Mode::Detecting {
            let Some((&byte, tail)) = rest.split_first() else {
                return;
            };
            rest = tail;
            self.head.push(byte);
            self.detect();
        }
        match self.mode {
            // The loop above leaves `Detecting` before it gets here.
            Mode::Detecting => {}
            Mode::Utf8 => self.utf8(rest),
            Mode::Utf16 { big_endian } => self.utf16(rest, big_endian),
        }
    }

    /// The text of every byte pushed. A sequence cut off by the end of the
    /// input is replaced: an incomplete UTF-8 sequence, a high surrogate,
    /// or an odd byte.
    pub fn finish(mut self) -> Decoded {
        if self.mode == Mode::Detecting {
            // Too short to be a BOM: the bytes are UTF-8 text.
            self.mode = Mode::Utf8;
            let head = std::mem::take(&mut self.head);
            self.utf8(head.bytes());
        }
        let partial = std::mem::take(&mut self.partial);
        if !partial.bytes().is_empty() {
            self.replace(partial.bytes());
        }
        if let Some(high) = self.high.take() {
            self.replace(&high);
        }
        if let Some(odd) = self.odd.take() {
            self.replace(&[odd]);
        }
        self.runs.close();
        Decoded {
            text: self.text.finish(),
            detected: self.format,
            runs: self.runs,
        }
    }

    /// Decides the format from `head` once it can: a BOM, or UTF-8 text.
    fn detect(&mut self) {
        let encoding = match self.head.bytes() {
            [0xEF, 0xBB, 0xBF] => Encoding::Utf8,
            [0xFF, 0xFE] => Encoding::Utf16Le,
            [0xFE, 0xFF] => Encoding::Utf16Be,
            [0xEF] | [0xEF, 0xBB] | [0xFF] | [0xFE] => return,
            _ => {
                // Not a BOM: the bytes so far are UTF-8 text.
                self.mode = Mode::Utf8;
                let head = std::mem::take(&mut self.head);
                self.utf8(head.bytes());
                return;
            }
        };
        self.format = Format::new(encoding, true);
        self.mode = match encoding {
            Encoding::Utf8 => Mode::Utf8,
            Encoding::Utf16Le => Mode::Utf16 { big_endian: false },
            Encoding::Utf16Be => Mode::Utf16 { big_endian: true },
        };
        self.head = Small::default();
    }

    /// Decodes UTF-8 `bytes`.
    fn utf8(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        // Finish the sequence the last push cut off, a byte at a time.
        while let Some(&lead) = self.partial.bytes().first() {
            let Some((&byte, tail)) = rest.split_first() else {
                return;
            };
            if !continues(lead, self.partial.len, byte) {
                // The cut-off bytes are a maximal subpart; `byte` starts
                // afresh.
                let partial = std::mem::take(&mut self.partial);
                self.replace(partial.bytes());
                break;
            }
            rest = tail;
            self.partial.push(byte);
            if self.partial.len == sequence_len(lead) {
                let partial = std::mem::take(&mut self.partial);
                match std::str::from_utf8(partial.bytes()) {
                    Ok(text) => self.text.push_str(text),
                    // Every byte was checked, so this does not happen.
                    Err(_) => self.replace(partial.bytes()),
                }
            }
        }
        let mut chunks = rest.utf8_chunks().peekable();
        while let Some(chunk) = chunks.next() {
            self.text.push_str(chunk.valid());
            let invalid = chunk.invalid();
            if invalid.is_empty() {
                continue;
            }
            if chunks.peek().is_none() && is_cut_off(invalid) {
                // The input ends inside a sequence; the next push may
                // finish it.
                for &byte in invalid {
                    self.partial.push(byte);
                }
            } else {
                self.replace(invalid);
            }
        }
    }

    /// Decodes UTF-16 `bytes` in the given byte order.
    fn utf16(&mut self, bytes: &[u8], big_endian: bool) {
        let mut rest = bytes;
        if let Some(first) = self.odd.take() {
            let Some((&second, tail)) = rest.split_first() else {
                self.odd = Some(first);
                return;
            };
            rest = tail;
            self.unit([first, second], big_endian);
        }
        let (pairs, remainder) = rest.as_chunks::<2>();
        for &pair in pairs {
            self.unit(pair, big_endian);
        }
        if let &[last] = remainder {
            self.odd = Some(last);
        }
    }

    /// Decodes one UTF-16 code unit, given as its two bytes.
    fn unit(&mut self, bytes: [u8; 2], big_endian: bool) {
        let unit = to_unit(bytes, big_endian);
        if let Some(high) = self.high.take() {
            if (0xDC00..=0xDFFF).contains(&unit)
                && let Some(Ok(c)) = char::decode_utf16([to_unit(high, big_endian), unit]).next()
            {
                self.text.push_char(c);
                return;
            }
            // Not followed by a low surrogate: unpaired.
            self.replace(&high);
        }
        match unit {
            0xD800..=0xDBFF => self.high = Some(bytes),
            0xDC00..=0xDFFF => self.replace(&bytes),
            _ => match char::from_u32(u32::from(unit)) {
                Some(c) => self.text.push_char(c),
                // Only surrogates are not chars, and they are handled above.
                None => self.replace(&bytes),
            },
        }
    }

    /// Writes one U+FFFD for the ill-formed sequence `original`.
    fn replace(&mut self, original: &[u8]) {
        self.runs.add(self.text.len, original);
        self.text.push_char(REPLACEMENT);
    }
}

/// The text of `bytes`, which are a whole file or message: [`Decoder`] on
/// one piece.
pub fn decode(bytes: &[u8]) -> Decoded {
    let mut decoder = Decoder::new();
    decoder.push(bytes);
    decoder.finish()
}

/// Writes `text` in `format` to `out`: the BOM if `format.bom`, then the
/// text one chunk at a time, so the whole output is never built in memory.
/// Line breaks are written as the text has them.
///
/// For speed, give it a buffered writer; each chunk is one `write_all`.
///
/// # Errors
///
/// The first error from `out`. What was written before it stays written.
pub fn encode(text: &Rope, format: Format, mut out: impl Write) -> io::Result<()> {
    if format.bom {
        out.write_all(format.encoding.bom())?;
    }
    write_chunks(text.chunks(), format.encoding, &mut out, &mut Vec::new())
}

/// Writes each of `chunks` in `encoding`, using `buf` for UTF-16.
fn write_chunks<'a>(
    chunks: impl Iterator<Item = &'a str>,
    encoding: Encoding,
    out: &mut impl Write,
    buf: &mut Vec<u8>,
) -> io::Result<()> {
    for chunk in chunks {
        match encoding {
            Encoding::Utf8 => out.write_all(chunk.as_bytes())?,
            Encoding::Utf16Le | Encoding::Utf16Be => {
                buf.clear();
                for unit in chunk.encode_utf16() {
                    let bytes = match encoding {
                        Encoding::Utf16Be => unit.to_be_bytes(),
                        _ => unit.to_le_bytes(),
                    };
                    buf.extend_from_slice(&bytes);
                }
                out.write_all(buf)?;
            }
        }
    }
    Ok(())
}

/// A decoded text: the text, the format its bytes were read as, and the
/// bytes that were not text.
///
/// Equality compares all three. `Debug` shows sizes only, never the text or
/// the bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Decoded {
    text: Rope,
    detected: Format,
    runs: Runs,
}

impl Decoded {
    /// The text: every replaced sequence is one U+FFFD.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// The text, without the rest.
    pub fn into_text(self) -> Rope {
        self.text
    }

    /// The format the bytes were read as: the encoding the BOM named, and
    /// whether there was one; UTF-8 without a BOM otherwise.
    pub fn detected(&self) -> Format {
        self.detected
    }

    /// The format to save the text in: the [`detected`](Decoded::detected)
    /// one, or `None` if any bytes were replaced. Such a text is read-only:
    /// a caller can write it only in a format it chooses itself.
    pub fn format(&self) -> Option<Format> {
        (!self.read_only()).then_some(self.detected)
    }

    /// Whether any bytes were replaced, so that the text is not the file
    /// and must not be written back as if it were.
    pub fn read_only(&self) -> bool {
        self.runs.count > 0
    }

    /// The runs of replaced bytes, in text order. O(1) to create, O(1) per
    /// run.
    pub fn replaced(&self) -> ReplacedRuns<'_> {
        ReplacedRuns {
            heads: &self.runs.heads,
            bytes: &self.runs.bytes,
            end: 0,
            left: self.runs.count,
        }
    }

    /// Writes the bytes the text was decoded from, exactly: the BOM if
    /// there was one, the text in the detected encoding, and each replaced
    /// run's original bytes in place of its U+FFFDs.
    ///
    /// # Errors
    ///
    /// The first error from `out`.
    pub fn write_original(&self, mut out: impl Write) -> io::Result<()> {
        let encoding = self.detected.encoding;
        if self.detected.bom {
            out.write_all(encoding.bom())?;
        }
        let mut buf = Vec::new();
        let mut at = 0;
        for run in self.replaced() {
            let before = self
                .text
                .chunks_in(at..run.range.start)
                .map_err(io::Error::other)?;
            write_chunks(before, encoding, &mut out, &mut buf)?;
            out.write_all(run.bytes)?;
            at = run.range.end;
        }
        let rest = self
            .text
            .chunks_in(at..self.text.len())
            .map_err(io::Error::other)?;
        write_chunks(rest, encoding, &mut out, &mut buf)
    }

    /// The bytes the text was decoded from, exactly; see
    /// [`write_original`](Decoded::write_original). O(n + r log n) for r
    /// runs: each run costs a descent of the rope.
    pub fn original_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Writing to a `Vec` fails only if a run were not on the text's
        // char boundaries, which a `Decoder` never builds.
        let _ = self.write_original(&mut bytes);
        bytes
    }
}

impl fmt::Debug for Decoded {
    /// Sizes only, never the text or the bytes (they may hold secrets).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoded")
            .field("text", &self.text)
            .field("detected", &self.detected)
            .field("replaced_runs", &self.runs.count)
            .field("replaced_bytes", &self.runs.bytes.len())
            .finish()
    }
}

/// One run of replaced bytes: where its U+FFFDs are in the text, and the
/// bytes they stand for.
///
/// `Debug` shows the range and the number of bytes, never the bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Replaced<'a> {
    /// The byte range of the run's U+FFFDs in the text: three bytes for each
    /// ill-formed sequence. Never empty.
    pub range: Range<usize>,
    /// The original bytes. Never empty.
    pub bytes: &'a [u8],
}

impl fmt::Debug for Replaced<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Replaced")
            .field("range", &self.range)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The runs of replaced bytes of a [`Decoded`], in text order, from
/// [`Decoded::replaced`]. Runs never touch: text lies between any two.
pub struct ReplacedRuns<'a> {
    /// The heads of the runs not yet returned.
    heads: &'a [u8],
    /// Their original bytes.
    bytes: &'a [u8],
    /// The end in the text of the run returned last.
    end: usize,
    /// How many runs are left.
    left: usize,
}

impl<'a> Iterator for ReplacedRuns<'a> {
    type Item = Replaced<'a>;

    fn next(&mut self) -> Option<Replaced<'a>> {
        if self.left == 0 {
            return None;
        }
        let gap = take_varint(&mut self.heads)?;
        let count = take_varint(&mut self.heads)?;
        let len = take_varint(&mut self.heads)?;
        let (bytes, rest) = self.bytes.split_at_checked(len)?;
        self.bytes = rest;
        self.left -= 1;
        let start = self.end.saturating_add(gap);
        self.end = start.saturating_add(count.saturating_mul(REPLACEMENT_LEN));
        Some(Replaced {
            range: start..self.end,
            bytes,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl ExactSizeIterator for ReplacedRuns<'_> {}

/// The runs of replaced bytes, compactly: per run a head of three LEB128
/// numbers (text bytes since the previous run, U+FFFDs in the run, original
/// bytes), and all the original bytes in one buffer. A short run costs three
/// bytes and its bytes.
#[derive(Clone, Default, PartialEq, Eq)]
struct Runs {
    /// The heads of the closed runs.
    heads: Vec<u8>,
    /// The original bytes of every run, closed or open, in order.
    bytes: Vec<u8>,
    /// How many runs are closed.
    count: usize,
    /// The end in the text of the last closed run.
    end: usize,
    /// The run still growing, if any.
    open: Option<OpenRun>,
}

/// A run that the next replacement can still join.
#[derive(Clone, Copy, PartialEq, Eq)]
struct OpenRun {
    /// Its start in the text.
    start: usize,
    /// How many U+FFFDs it has.
    count: usize,
    /// Where its original bytes start in [`Runs::bytes`].
    bytes_start: usize,
}

impl Runs {
    /// Records a U+FFFD written at text offset `at` for `original`. It joins
    /// the open run if it starts where that run ends.
    fn add(&mut self, at: usize, original: &[u8]) {
        match &mut self.open {
            Some(run)
                if run
                    .start
                    .saturating_add(run.count.saturating_mul(REPLACEMENT_LEN))
                    == at =>
            {
                run.count += 1;
            }
            _ => {
                self.close();
                self.open = Some(OpenRun {
                    start: at,
                    count: 1,
                    bytes_start: self.bytes.len(),
                });
            }
        }
        self.bytes.extend_from_slice(original);
    }

    /// Closes the open run, if any, writing its head.
    fn close(&mut self) {
        let Some(run) = self.open.take() else {
            return;
        };
        put_varint(&mut self.heads, run.start.saturating_sub(self.end));
        put_varint(&mut self.heads, run.count);
        put_varint(
            &mut self.heads,
            self.bytes.len().saturating_sub(run.bytes_start),
        );
        self.end = run
            .start
            .saturating_add(run.count.saturating_mul(REPLACEMENT_LEN));
        self.count += 1;
    }
}

/// Appends `value` as unsigned LEB128: seven bits a byte, low bits first,
/// the high bit set on every byte but the last.
fn put_varint(out: &mut Vec<u8>, value: usize) {
    let mut value = value;
    loop {
        let low = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Reads one unsigned LEB128 number from the front of `input`; `None` if it
/// ends first or does not fit a `usize`.
fn take_varint(input: &mut &[u8]) -> Option<usize> {
    let mut value = 0usize;
    let mut shift = 0u32;
    while let Some((&byte, rest)) = input.split_first() {
        *input = rest;
        value |= usize::from(byte & 0x7F).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= usize::BITS {
            return None;
        }
    }
    None
}

/// Decoded text on its way to the rope builder.
#[derive(Default)]
struct Sink {
    builder: RopeBuilder,
    /// Text not yet given to the builder.
    scratch: String,
    /// The length in bytes of all the text so far.
    len: usize,
}

impl Sink {
    fn push_str(&mut self, text: &str) {
        self.len = self.len.saturating_add(text.len());
        if self.scratch.len().saturating_add(text.len()) > SCRATCH {
            self.flush();
        }
        if text.len() > SCRATCH {
            self.builder.push_str(text);
        } else {
            self.scratch.push_str(text);
        }
    }

    fn push_char(&mut self, c: char) {
        self.len = self.len.saturating_add(c.len_utf8());
        self.scratch.push(c);
        if self.scratch.len() >= SCRATCH {
            self.flush();
        }
    }

    fn flush(&mut self) {
        self.builder.push_str(&self.scratch);
        self.scratch.clear();
    }

    fn finish(mut self) -> Rope {
        self.flush();
        self.builder.finish()
    }
}

/// Up to four bytes: a BOM being detected, or a cut-off UTF-8 sequence.
#[derive(Clone, Copy, Default)]
struct Small {
    buf: [u8; 4],
    len: usize,
}

impl Small {
    fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&self.buf)
    }

    /// Appends `byte`; callers never push a fifth.
    fn push(&mut self, byte: u8) {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = byte;
            self.len += 1;
        }
    }
}

/// The length of the UTF-8 sequence that `lead` starts, or 0 if it starts
/// none (a continuation byte, `C0`, `C1` or `F5` to `FF`). Unicode §3.9,
/// table 3-7.
fn sequence_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0,
    }
}

/// Whether `byte` may be byte `index` (from 0) of a sequence that starts
/// with `lead`. The second byte's range depends on the lead, so that
/// overlong forms, surrogates and code points past U+10FFFF are ill-formed
/// (Unicode §3.9, table 3-7).
fn continues(lead: u8, index: usize, byte: u8) -> bool {
    let range = match (lead, index) {
        (0xE0, 1) => 0xA0..=0xBF,
        (0xED, 1) => 0x80..=0x9F,
        (0xF0, 1) => 0x90..=0xBF,
        (0xF4, 1) => 0x80..=0x8F,
        _ => 0x80..=0xBF,
    };
    index < sequence_len(lead) && range.contains(&byte)
}

/// Whether `invalid`, a maximal subpart at the very end of the input, is
/// the start of a well-formed sequence that more input could finish.
fn is_cut_off(invalid: &[u8]) -> bool {
    let Some((&lead, rest)) = invalid.split_first() else {
        return false;
    };
    sequence_len(lead) > invalid.len()
        && rest
            .iter()
            .enumerate()
            .all(|(i, &byte)| continues(lead, i + 1, byte))
}

/// The UTF-16 code unit of `bytes` in the given byte order.
fn to_unit(bytes: [u8; 2], big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}
