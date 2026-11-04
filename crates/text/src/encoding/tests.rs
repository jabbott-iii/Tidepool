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

//! Unit and property tests of `encoding`: hand-made vectors, then random
//! bytes biased toward near-valid UTF-8 and UTF-16 (cut-off sequences,
//! overlong forms, surrogates, stray continuation bytes, BOMs) checked
//! against `String::from_utf8_lossy` and `char::decode_utf16`, for lossless
//! round trips and for the same result however the input is split.

use std::io::{self, Write};
use std::ops::Range;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};

use super::{Decoded, Decoder, Encoding, Format, REPLACEMENT, decode, encode};
use crate::Rope;

const UTF8_BOM: Format = Format::new(Encoding::Utf8, true);
const UTF16LE: Format = Format::new(Encoding::Utf16Le, true);
const UTF16BE: Format = Format::new(Encoding::Utf16Be, true);

// `Decoder` and `Decoded` can be sent to and shared with workers.
const _: fn() = || {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Decoder>();
    send_sync::<Decoded>();
};

/// The runs of `decoded` as plain values.
fn runs(decoded: &Decoded) -> Vec<(Range<usize>, Vec<u8>)> {
    decoded
        .replaced()
        .map(|run| (run.range, run.bytes.to_vec()))
        .collect()
}

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

/// `bytes` pushed in the pieces between `cuts` (sorted offsets).
fn decode_in_pieces(bytes: &[u8], cuts: &[usize]) -> Decoded {
    let mut decoder = Decoder::new();
    let mut at = 0;
    for &cut in cuts {
        let cut = cut.clamp(at, bytes.len());
        decoder.push(&bytes[at..cut]);
        at = cut;
    }
    decoder.push(&bytes[at..]);
    decoder.finish()
}

/// The structural promises about the runs: never empty, three bytes of
/// U+FFFD per ill-formed sequence, in order, never touching, inside the
/// text, with one to four original bytes per U+FFFD.
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

/// Every promise for one input, split at `cuts` for the streaming check.
fn verify(bytes: &[u8], cuts: &[usize]) -> Result<(), String> {
    let whole = decode(bytes);
    if whole.original_bytes() != bytes {
        return Err(format!(
            "original_bytes differ: {:02x?} (runs {:?})",
            whole.original_bytes(),
            runs(&whole)
        ));
    }
    check_runs(&whole)?;
    let (text, format) = reference(bytes);
    if whole.detected() != format {
        return Err(format!(
            "detected {:?}, std says {format:?}",
            whole.detected()
        ));
    }
    if *whole.text() != text {
        return Err(format!(
            "text {:?}, std says {text:?}",
            whole.text().to_string()
        ));
    }
    if whole.read_only() {
        if whole.format().is_some() {
            return Err("a text with replacements has a save format".to_string());
        }
    } else {
        if whole.format() != Some(format) {
            return Err(format!("save format {:?}", whole.format()));
        }
        let mut again = Vec::new();
        encode(whole.text(), format, &mut again).map_err(|error| error.to_string())?;
        if again != bytes {
            return Err(format!("re-encoded as {again:02x?}"));
        }
    }
    let pieces = decode_in_pieces(bytes, cuts);
    if pieces != whole {
        return Err(format!(
            "split at {cuts:?}: {pieces:?} runs {:?}, whole: {whole:?} runs {:?}",
            runs(&pieces),
            runs(&whole)
        ));
    }
    if bytes.len() <= 512 {
        let every: Vec<usize> = (0..=bytes.len()).collect();
        let single = decode_in_pieces(bytes, &every);
        if single != whole {
            return Err(format!("byte by byte: {:?}", runs(&single)));
        }
    }
    Ok(())
}

#[test]
fn hand_vectors() {
    type Case = (
        &'static [u8],
        &'static str,
        Format,
        &'static [(Range<usize>, &'static [u8])],
    );
    let cases: [Case; 24] = [
        (b"", "", Format::UTF8, &[]),
        (b"a\r\nb\rc\n", "a\r\nb\rc\n", Format::UTF8, &[]),
        (b"\xC3\xA9", "é", Format::UTF8, &[]),
        (b"\xEF\xBB\xBF", "", UTF8_BOM, &[]),
        (b"\xEF\xBB\xBFhi", "hi", UTF8_BOM, &[]),
        (b"\xEF\xBB\xBF\xEF\xBB\xBF", "\u{FEFF}", UTF8_BOM, &[]),
        (b"\xFF\xFE", "", UTF16LE, &[]),
        (b"\xFE\xFF", "", UTF16BE, &[]),
        (b"\xFF\xFEh\0i\0", "hi", UTF16LE, &[]),
        (b"\xFE\xFF\0h\0i", "hi", UTF16BE, &[]),
        // A UTF-32 LE BOM is a UTF-16 LE BOM and U+0000.
        (b"\xFF\xFE\0\0", "\0", UTF16LE, &[]),
        // A UTF-32 BE BOM is not a BOM at all.
        (
            b"\0\0\xFE\xFF",
            "\0\0\u{FFFD}\u{FFFD}",
            Format::UTF8,
            &[(2..8, b"\xFE\xFF")],
        ),
        // A BOM cut short is UTF-8 without a BOM, and ill-formed.
        (
            b"\xEF\xBB",
            "\u{FFFD}",
            Format::UTF8,
            &[(0..3, b"\xEF\xBB")],
        ),
        (b"\xFF", "\u{FFFD}", Format::UTF8, &[(0..3, b"\xFF")]),
        (b"\xFEa", "\u{FFFD}a", Format::UTF8, &[(0..3, b"\xFE")]),
        // An odd last byte, and a high surrogate the input ends inside.
        (b"\xFF\xFEA", "\u{FFFD}", UTF16LE, &[(0..3, b"A")]),
        (b"\xFF\xFE=\xD8", "\u{FFFD}", UTF16LE, &[(0..3, b"=\xD8")]),
        (
            b"\xFF\xFE=\xD8=\xD8\0\xDE",
            "\u{FFFD}😀",
            UTF16LE,
            &[(0..3, b"=\xD8")],
        ),
        (
            b"\xFE\xFF\xDC\0\0A",
            "\u{FFFD}A",
            UTF16BE,
            &[(0..3, b"\xDC\0")],
        ),
        // A lone low, a high without its low, and an odd byte: one run.
        (
            b"\xFF\xFE\0\xDC=\xD8A",
            "\u{FFFD}\u{FFFD}\u{FFFD}",
            UTF16LE,
            &[(0..9, b"\0\xDC=\xD8A")],
        ),
        (b"a\xC3", "a\u{FFFD}", Format::UTF8, &[(1..4, b"\xC3")]),
        // A surrogate in UTF-8, and a code point past U+10FFFF: each byte
        // is a maximal subpart.
        (
            b"\xED\xA0\x80",
            "\u{FFFD}\u{FFFD}\u{FFFD}",
            Format::UTF8,
            &[(0..9, b"\xED\xA0\x80")],
        ),
        (
            b"\xF4\x90\x80\x80!",
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}!",
            Format::UTF8,
            &[(0..12, b"\xF4\x90\x80\x80")],
        ),
        // Unicode §3.9, table 3-8: maximal subparts.
        (
            b"\x61\xF1\x80\x80\xE1\x80\xC2\x62\x80\x63\x80\xBF\x64",
            "a\u{FFFD}\u{FFFD}\u{FFFD}b\u{FFFD}c\u{FFFD}\u{FFFD}d",
            Format::UTF8,
            &[
                (1..10, b"\xF1\x80\x80\xE1\x80\xC2"),
                (11..14, b"\x80"),
                (15..21, b"\x80\xBF"),
            ],
        ),
    ];
    for (bytes, text, format, want) in cases {
        let decoded = decode(bytes);
        assert_eq!(*decoded.text(), text, "{bytes:02x?}");
        assert_eq!(decoded.detected(), format, "{bytes:02x?}");
        let want: Vec<(Range<usize>, Vec<u8>)> = want
            .iter()
            .map(|(range, bytes)| (range.clone(), bytes.to_vec()))
            .collect();
        assert_eq!(runs(&decoded), want, "{bytes:02x?}");
        assert_eq!(decoded.read_only(), !want.is_empty(), "{bytes:02x?}");
        assert_eq!(decoded.original_bytes(), bytes, "{bytes:02x?}");
        let cuts: Vec<usize> = (0..=bytes.len()).collect();
        assert_eq!(decode_in_pieces(bytes, &cuts), decoded, "{bytes:02x?}");
        assert_eq!(verify(bytes, &[1, 2, 3]), Ok(()), "{bytes:02x?}");
    }
}

#[test]
fn utf16_without_a_bom_reads_as_utf8() {
    let mut bytes = Vec::new();
    let format = Format::new(Encoding::Utf16Le, false);
    encode(&Rope::from("hi"), format, &mut bytes).expect("write to a Vec");
    assert_eq!(bytes, b"h\0i\0");
    let decoded = decode(&bytes);
    assert_eq!(decoded.detected(), Format::UTF8);
    assert_eq!(*decoded.text(), "h\0i\0");
}

#[test]
fn the_defaults_are_utf8_without_a_bom() {
    assert_eq!(Format::default(), Format::UTF8);
    assert_eq!(Format::UTF8, Format::new(Encoding::Utf8, false));
    assert_eq!(Encoding::default(), Encoding::Utf8);
    assert_eq!(Decoder::new().finish(), decode(b""));
    let names = [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be].map(Encoding::name);
    assert_eq!(names, ["UTF-8", "UTF-16 LE", "UTF-16 BE"]);
}

#[test]
fn debug_shows_sizes_only() {
    let decoded = decode(b"secret \xFF\xFE value");
    let shown = format!("{decoded:?}");
    assert!(
        !shown.contains("secret") && !shown.contains("255"),
        "{shown}"
    );
    assert!(shown.contains("replaced_runs: 1"), "{shown}");
    assert!(shown.contains("replaced_bytes: 2"), "{shown}");
    let run = decoded.replaced().next().expect("one run");
    assert_eq!(format!("{run:?}"), "Replaced { range: 7..13, bytes: 2 }");
}

/// A short run costs three bytes of head and its original bytes, here four
/// bytes for a three-byte U+FFFD in the rope; a long run costs one head.
#[test]
fn short_runs_cost_three_bytes_and_their_bytes() {
    let bytes = b"\x80a".repeat(10_000);
    let decoded = decode(&bytes);
    assert_eq!(decoded.replaced().len(), 10_000);
    assert_eq!(decoded.runs.heads.len(), 30_000);
    assert_eq!(decoded.runs.bytes.len(), 10_000);
    assert_eq!(decoded.text().len(), 40_000);
    assert_eq!(decoded.original_bytes(), bytes);

    // One long run is one head, whatever its length.
    let bytes = vec![0xFF; 100_000];
    let decoded = decode(&bytes);
    assert_eq!(runs(&decoded), [(0..300_000, bytes.clone())]);
    assert!(
        decoded.runs.heads.len() <= 1 + 3 + 3,
        "{}",
        decoded.runs.heads.len()
    );
}

#[test]
fn varints_round_trip() {
    let values = [
        0,
        1,
        127,
        128,
        255,
        16_383,
        16_384,
        usize::MAX >> 1,
        usize::MAX,
    ];
    let mut buf = Vec::new();
    for value in values {
        super::put_varint(&mut buf, value);
    }
    let mut input = buf.as_slice();
    for value in values {
        assert_eq!(super::take_varint(&mut input), Some(value));
    }
    assert!(input.is_empty());
    assert_eq!(super::take_varint(&mut input), None);
    // Cut off, or too long for a usize.
    assert_eq!(super::take_varint(&mut &[0x80][..]), None);
    assert_eq!(super::take_varint(&mut &[0xFF; 11][..]), None);
}

/// Records the size of each write.
#[derive(Default)]
struct Writes(Vec<usize>, Vec<u8>);

impl Write for Writes {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.push(buf.len());
        self.1.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn encode_writes_one_chunk_at_a_time() {
    let text = "line one\r\nzwei: é€😀\n".repeat(50_000);
    let rope = Rope::from(text.as_str());
    let chunks = rope.chunks().count();
    for format in [Format::UTF8, UTF8_BOM, UTF16LE, UTF16BE] {
        let mut out = Writes::default();
        encode(&rope, format, &mut out).expect("write");
        assert_eq!(out.0.len(), chunks + usize::from(format.bom), "{format:?}");
        assert!(out.0.iter().all(|&len| len <= 2 * 4096), "{format:?}");
        let decoded = decode(&out.1);
        assert_eq!(decoded.detected(), format);
        assert!(*decoded.text() == text, "{format:?}");
    }
}

// ---------------------------------------------------------------------------
// Generators.

/// Chars of every UTF-8 length and at the edges of each, the BOM's char,
/// U+FFFD itself, and line breaks.
const CHARS: [char; 18] = [
    'a',
    ' ',
    '\n',
    '\r',
    '\0',
    '\u{7F}',
    '\u{80}',
    'é',
    '\u{7FF}',
    '\u{800}',
    '€',
    '\u{D7FF}',
    '\u{E000}',
    '\u{FEFF}',
    '\u{FFFD}',
    '\u{FFFF}',
    '😀',
    '\u{10FFFF}',
];

/// Ill-formed UTF-8 that a random byte rarely makes: overlong forms,
/// surrogates, code points past U+10FFFF, and BOMs and their prefixes.
const ILL_FORMED: [&[u8]; 17] = [
    b"\xC0\x80",
    b"\xC1\xBF",
    b"\xE0\x80\x80",
    b"\xE0\x9F\xBF",
    b"\xF0\x80\x80\x80",
    b"\xF0\x8F\xBF\xBF",
    b"\xED\xA0\x80",
    b"\xED\xBF\xBF",
    b"\xED\xA0\xBD\xED\xB8\x80",
    b"\xF4\x90\x80\x80",
    b"\xF5\x80\x80\x80",
    b"\xFF",
    b"\xEF\xBB\xBF",
    b"\xFF\xFE",
    b"\xFE\xFF",
    b"\xEF\xBB",
    b"\xFE",
];

fn byte(rng: &mut Pcg32, range: Range<u32>) -> u8 {
    rng.range(range) as u8
}

fn char_of(rng: &mut Pcg32) -> char {
    *rng.choose(&CHARS).unwrap_or(&'a')
}

/// Appends one near-valid piece of UTF-8.
fn utf8_atom(rng: &mut Pcg32, out: &mut Vec<u8>) {
    match rng.range(0..12u32) {
        0..4 => out.push(byte(rng, 0x20..0x7F)),
        4..6 => {
            let mut buf = [0; 4];
            out.extend_from_slice(char_of(rng).encode_utf8(&mut buf).as_bytes());
        }
        6 => {
            // A multi-byte char cut short.
            let mut buf = [0; 4];
            let c = ['é', '€', '😀', '\u{10FFFF}', '\u{800}'][rng.range(0..5u32) as usize];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            let keep = rng.range(1..bytes.len() as u32) as usize;
            out.extend_from_slice(&bytes[..keep]);
        }
        7 => out.push(byte(rng, 0x80..0xC0)),
        8 => out.extend_from_slice(rng.choose(&ILL_FORMED).copied().unwrap_or(b"\xFF")),
        9 => out.push(byte(rng, 0..256)),
        10 => {
            // A lead byte and whatever follows.
            out.push(byte(rng, 0xC2..0xF5));
            out.push(byte(rng, 0..256));
        }
        _ => out.extend_from_slice(b"\r\n"),
    }
}

/// Appends one near-valid piece of UTF-16.
fn utf16_atom(rng: &mut Pcg32, big_endian: bool, out: &mut Vec<u8>) {
    let mut unit = |unit: u16| {
        let bytes = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        out.extend_from_slice(&bytes);
    };
    match rng.range(0..10u32) {
        0..4 => {
            let c = char_of(rng);
            let mut buf = [0; 2];
            for &u in c.encode_utf16(&mut buf).iter() {
                unit(u);
            }
        }
        4..6 => {
            unit(rng.range(0xD800..0xDC00u32) as u16);
            unit(rng.range(0xDC00..0xE000u32) as u16);
        }
        6 => unit(rng.range(0xD800..0xDC00u32) as u16),
        7 => unit(rng.range(0xDC00..0xE000u32) as u16),
        8 => out.push(byte(rng, 0..256)),
        _ => unit(rng.range(0..0x1_0000u32) as u16),
    }
}

/// Bytes biased toward near-valid text: a BOM (or not), then `atoms` pieces
/// in its encoding, with now and then a piece of the other.
fn near_valid(rng: &mut Pcg32, atoms: u32) -> Vec<u8> {
    const STARTS: [&[u8]; 8] = [
        b"",
        b"",
        b"",
        b"\xEF\xBB\xBF",
        b"\xFF\xFE",
        b"\xFF\xFE",
        b"\xFE\xFF",
        b"\xFE\xFF",
    ];
    let start = rng.choose(&STARTS).copied().unwrap_or(b"");
    let mut out = start.to_vec();
    let utf16 = match start {
        b"\xFF\xFE" => Some(false),
        b"\xFE\xFF" => Some(true),
        _ => None,
    };
    for _ in 0..atoms {
        match utf16 {
            Some(big_endian) if rng.range(0..10u32) != 0 => utf16_atom(rng, big_endian, &mut out),
            _ => utf8_atom(rng, &mut out),
        }
    }
    out
}

/// Up to `count` sorted cut points in `0..=len`.
fn cuts(rng: &mut Pcg32, len: usize, count: u32) -> Vec<usize> {
    let mut cuts: Vec<usize> = (0..rng.range(0..count + 1))
        .map(|_| rng.range(0..len as u32 + 1) as usize)
        .collect();
    cuts.sort_unstable();
    cuts
}

#[test]
fn near_valid_bytes_round_trip_and_split_anywhere() {
    check_sized(
        "near-valid bytes round-trip and split anywhere",
        6_000,
        |rng, size| {
            let atoms = rng.range(0..2 * size + 1);
            let bytes = near_valid(rng, atoms);
            let cuts = cuts(rng, bytes.len(), 8);
            (bytes, cuts)
        },
        |(bytes, cuts)| verify(bytes, cuts),
    );
}

#[test]
fn random_bytes_round_trip() {
    check_sized(
        "random bytes round-trip",
        2_000,
        |rng, size| {
            let bytes: Vec<u8> = (0..rng.range(0..4 * size + 1))
                .map(|_| byte(rng, 0..256))
                .collect();
            let cuts = cuts(rng, bytes.len(), 6);
            (bytes, cuts)
        },
        |(bytes, cuts)| verify(bytes, cuts),
    );
}

/// Inputs of many leaves, so that pieces and CR LFs meet leaf seams.
#[test]
fn large_inputs_split_anywhere_agree() {
    check(
        "large inputs split anywhere agree",
        150,
        |rng| {
            let target = rng.range(0..24 * 1024u32) as usize;
            let mut bytes = near_valid(rng, 0);
            let utf16 = bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF");
            let big_endian = bytes.starts_with(b"\xFE\xFF");
            while bytes.len() < target {
                if utf16 && rng.range(0..50u32) != 0 {
                    utf16_atom(rng, big_endian, &mut bytes);
                } else {
                    utf8_atom(rng, &mut bytes);
                }
            }
            let cuts = cuts(rng, bytes.len(), 12);
            (bytes, cuts)
        },
        |(bytes, cuts)| {
            verify(bytes, cuts)?;
            decode(bytes).text().__check_invariants()
        },
    );
}

#[test]
fn valid_text_encodes_and_decodes_back() {
    const FORMATS: [Format; 4] = [Format::UTF8, UTF8_BOM, UTF16LE, UTF16BE];
    check(
        "valid text encodes and decodes back",
        3_000,
        |rng| {
            let len = rng.range(0..200u32);
            let text: String = (0..len).map(|_| char_of(rng)).collect();
            (text, rng.range(0..4u32) as usize)
        },
        |(text, format)| {
            let format = FORMATS[*format];
            let mut bytes = Vec::new();
            encode(&Rope::from(text.as_str()), format, &mut bytes).map_err(|e| e.to_string())?;
            let decoded = decode(&bytes);
            if decoded.read_only() {
                return Err(format!("replaced {:?}", runs(&decoded)));
            }
            let mut again = Vec::new();
            encode(decoded.text(), decoded.detected(), &mut again).map_err(|e| e.to_string())?;
            if again != bytes {
                return Err(format!("re-encoded as {again:02x?}"));
            }
            // UTF-8 without a BOM whose text starts with U+FEFF reads back as
            // UTF-8 with a BOM: the same bytes, another split.
            if (format.bom || !text.starts_with('\u{FEFF}'))
                && (decoded.detected() != format || *decoded.text() != **text)
            {
                return Err(format!("decoded {decoded:?}"));
            }
            Ok(())
        },
    );
}
