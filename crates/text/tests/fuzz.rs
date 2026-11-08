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

//! The fuzz target of `tidepool-text`: `encoding`, the decoder of
//! [`tidepool_text::encoding`], one test named after it (ADR-0027,
//! `docs/04-quality.md` → "Fuzz smoke").
//!
//! A plain `cargo test` runs its saved crashes (`fuzz/crashes/encoding/`)
//! and its seeds (`fuzz/corpus/encoding/`) unchanged. `cargo xtask
//! fuzz-smoke` runs it with mutated inputs too, in an optimized profile
//! with overflow checks.
//!
//! Decoding never fails, and the decoder takes no `Limits`, so the target
//! checks what the module docs promise instead. A mismatch panics, which
//! the fuzzer reports:
//!
//! - However the input is split into pushes, the [`Decoder`] gives what
//!   [`decode`] gives for the whole of it.
//! - The text and the detected format are what `std` decodes: the BOM by
//!   hand, then `String::from_utf8_lossy` or `char::decode_utf16` with each
//!   error replaced ([`reference`]).
//! - The runs of replaced bytes keep their promises ([`check_runs`]).
//! - [`Decoded::original_bytes`] is the input, exactly.
//! - A text decoded without a replacement, encoded in the format it was
//!   detected in, is the input again.
//!
//! [`reference`] and [`check_runs`] are copied from the unit tests in
//! `src/encoding/tests.rs`, which an integration test cannot reach.

use std::ops::Range;

use tidepool_core::testing::fuzz::{Outcome, Target};
use tidepool_text::encoding::{Decoded, Decoder, Encoding, Format, decode, encode};

/// The repository root, where `fuzz/` is.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The cap on the rise of the written peak: the target's peak rose by
/// about 0.3 MiB over a 60 s run.
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

/// U+FFFD REPLACEMENT CHARACTER, which stands for each replaced sequence.
const REPLACEMENT: char = '\u{FFFD}';

const UTF8_BOM: Format = Format::new(Encoding::Utf8, true);
const UTF16LE: Format = Format::new(Encoding::Utf16Le, true);
const UTF16BE: Format = Format::new(Encoding::Utf16Be, true);

/// `bytes` decoded by `std`: the BOM by hand, then `from_utf8_lossy` or
/// `decode_utf16` with each error replaced, and one more U+FFFD for an odd
/// last byte.
fn reference(bytes: &[u8]) -> (String, Format) {
    let utf16 = |rest: &[u8], big_endian: bool| {
        let units = rest.as_chunks::<2>().0.iter().map(|&pair| {
            if big_endian {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            }
        });
        let mut text: String = char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(REPLACEMENT))
            .collect();
        if rest.len() % 2 == 1 {
            text.push(REPLACEMENT);
        }
        text
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), UTF8_BOM)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (utf16(rest, false), UTF16LE)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (utf16(rest, true), UTF16BE)
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), Format::UTF8)
    }
}

/// The structural promises about the runs: never empty, three bytes of
/// U+FFFD per ill-formed sequence, in order, never touching, inside the
/// text, with one to three original bytes per U+FFFD.
fn check_runs(decoded: &Decoded) -> Result<(), String> {
    let text = decoded.text().to_string();
    let mut end = None;
    let mut seen = 0;
    for run in decoded.replaced() {
        seen += 1;
        let Range { start, end: stop } = run.range.clone();
        if start >= stop || (stop - start) % 3 != 0 {
            return Err(format!("run {:?} is not whole U+FFFDs", run.range));
        }
        if let Some(previous) = end
            && start <= previous
        {
            return Err(format!("run {:?} touches the one before", run.range));
        }
        end = Some(stop);
        let Some(slice) = text.get(run.range.clone()) else {
            return Err(format!("run {:?} is not on char boundaries", run.range));
        };
        if slice.chars().any(|c| c != REPLACEMENT) {
            return Err(format!("run {:?} holds more than U+FFFD", run.range));
        }
        let count = (stop - start) / 3;
        if run.bytes.len() < count || run.bytes.len() > 3 * count {
            return Err(format!(
                "run {:?} has {} bytes for {count} U+FFFDs",
                run.range,
                run.bytes.len()
            ));
        }
    }
    if seen != decoded.replaced().len() {
        return Err(format!(
            "{seen} runs, but len() says {}",
            decoded.replaced().len()
        ));
    }
    if decoded.read_only() != (seen > 0) {
        return Err("read_only does not match the runs".to_string());
    }
    Ok(())
}

/// A run's result line for `fuzz-smoke`, then the verdict.
#[expect(
    clippy::print_stdout,
    reason = "`cargo xtask fuzz-smoke` reads the result line from the test's output; libtest \
              shows it only with --nocapture or when the test fails"
)]
fn report(outcome: &Outcome) {
    println!("{}", outcome.summary());
    outcome.assert_clean();
}

/// Byte-order marks (and UTF-32's, which reads as UTF-16 LE), UTF-8's
/// surrogates, overlong forms, code points past U+10FFFF, lone lead and
/// continuation bytes, UTF-16 surrogates in both byte orders, valid
/// multi-byte characters, U+FEFF and U+FFFE, and line ends.
const ENCODING_TOKENS: &[&[u8]] = &[
    b"\xef\xbb\xbf",
    b"\xff\xfe",
    b"\xfe\xff",
    b"\xff\xfe\x00\x00",
    b"\xed\xa0\x80",
    b"\xed\xbf\xbf",
    b"\xc0\x80",
    b"\xc1\xbf",
    b"\xe0\x80\x80",
    b"\xe0\x9f\xbf",
    b"\xf0\x80\x80\x80",
    b"\xf0\x8f\xbf\xbf",
    b"\xf4\x90\x80\x80",
    b"\xf5\x80\x80\x80",
    b"\x80",
    b"\xbf",
    b"\xc2",
    b"\xe0",
    b"\xe2\x82",
    b"\xf0",
    b"\xf0\x9f\x98",
    b"\xf4",
    b"\xfe",
    b"\xff",
    b"\x00\xd8",
    b"\x00\xdc",
    b"\xd8\x00",
    b"\xdc\x00",
    b"\x3d\xd8\x00\xde",
    b"\xd8\x3d\xde\x00",
    b"\xc3\xa9",
    b"\xe2\x82\xac",
    b"\xf0\x9f\x98\x80",
    b"\xef\xbf\xbe",
    b"\xef\xbf\xbd",
    b"\r\n",
    b"\r",
    b"\x00",
];

/// Splits `input` into pieces at points that the input itself decides, so
/// that the same input is always split the same way: pieces of 1 to 6
/// bytes, sometimes an empty push first, and sometimes all of the rest at
/// once.
fn pieces(input: &[u8]) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    let mut at = 0;
    let mut key = input.len();
    while let Some(&byte) = input.get(at) {
        key = key.wrapping_mul(31).wrapping_add(usize::from(byte));
        let choice = key % 8;
        if choice == 0 {
            pieces.push(&input[at..at]);
        }
        let end = match choice {
            7 => input.len(),
            n => (at + n.max(1)).min(input.len()),
        };
        pieces.push(&input[at..end]);
        at = end;
    }
    pieces
}

/// `encoding`: [`Decoder`] in pieces, and [`decode`].
fn decode_text(input: &[u8]) {
    let whole = decode(input);
    let mut decoder = Decoder::new();
    for piece in pieces(input) {
        decoder.push(piece);
    }
    let split: Decoded = decoder.finish();
    assert!(
        split == whole,
        "decoding in pieces differs from decoding at once: {split:?} and {whole:?}"
    );
    let (text, format) = reference(input);
    assert!(
        whole.detected() == format,
        "detected {:?}, but the BOM says {format:?}",
        whole.detected()
    );
    assert!(
        *whole.text() == text,
        "the text is not what std decodes: {whole:?}"
    );
    if let Err(message) = check_runs(&whole) {
        panic!("{message}: {whole:?}");
    }
    assert!(
        whole.original_bytes() == input,
        "original_bytes() is not the input: {whole:?}"
    );
    if let Some(format) = whole.format() {
        let mut encoded = Vec::with_capacity(input.len());
        encode(whole.text(), format, &mut encoded).expect("a Vec takes any bytes");
        assert!(
            encoded == input,
            "a text decoded without a replacement encodes to other bytes: {whole:?}"
        );
    }
}

#[test]
fn encoding() {
    let outcome = Target::new(ROOT, "encoding", decode_text)
        .dictionary(ENCODING_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

#[test]
fn the_pieces_cover_the_input_in_order() {
    for input in [
        &b""[..],
        b"a",
        b"\xff\xfeh\x00i\x00",
        b"a longer input, so that every kind of piece turns up somewhere in it",
    ] {
        let pieces = pieces(input);
        assert_eq!(pieces.concat(), input);
        assert_eq!(pieces, self::pieces(input), "the same split every time");
    }
    let long: Vec<u8> = (0..=255).collect();
    let pieces = pieces(&long);
    assert!(pieces.iter().any(|piece| piece.is_empty()));
    assert!(pieces.iter().any(|piece| piece.len() > 6));
}
