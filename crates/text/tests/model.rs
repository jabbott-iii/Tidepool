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

//! The rope against a `String` model at the real shape (leaves of 1-4 KiB,
//! nodes of 8-16 children): texts up to 64 KiB, inserts up to 12 KiB,
//! 8 MiB builds, and `chunks_in` from and to every chunk seam.
//!
//! The unit tests (`src/rope/tests.rs`) run 10,000 sequences in a tiny shape
//! with every check after every step; these show that the real constants
//! work too. Each case's input is a seed, so a failure report stays short.

use std::fmt::Debug;

use tidepool_core::rng::Pcg32;
use tidepool_core::testing::{check, check_sized};
use tidepool_text::rope::{CharIndices, Chars, Chunks};
use tidepool_text::{Rope, RopeBuilder, TextError, Unit};

// `Rope` and its iterators can be sent to and shared with workers.
const _: fn() = || {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Rope>();
    send_sync::<RopeBuilder>();
    send_sync::<Chunks<'static>>();
    send_sync::<Chars<'static>>();
    send_sync::<CharIndices<'static>>();
    send_sync::<TextError>();
};

const MAX_TEXT: usize = 64 * 1024;

/// Non-ASCII and break units of generated text. The 4-byte chars cover
/// every lead byte, F0 to F4.
const ALPHABET: [&str; 26] = [
    "a",
    "b",
    "z",
    " ",
    "\n",
    "\r",
    "\r\n",
    "\r\n",
    "é",
    "€",
    "中",
    "😀",
    "𐐀",
    "🇺🇸",
    "\u{301}",
    "\u{200D}",
    "क्",
    "\u{85}",
    "\u{2028}",
    "\0",
    "\u{40000}",
    "\u{80000}",
    "\u{E0100}",
    "\u{E0067}",
    "\u{F0000}",
    "\u{10FFFF}",
];

/// Text of about `bytes` bytes, mostly ASCII words and line breaks.
fn text(rng: &mut Pcg32, bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + 8);
    while out.len() < bytes {
        if rng.range(0..3u32) == 0 {
            out.push_str(rng.choose(&ALPHABET).copied().unwrap_or("a"));
        } else {
            out.push(char::from(rng.range(0x20..0x7Fu32) as u8));
        }
    }
    out
}

/// An insert length in bytes: mostly small, up to 12 KiB.
fn insert_len(rng: &mut Pcg32) -> usize {
    let n = match rng.range(0..100u32) {
        0..60 => rng.range(0..17u32),
        60..85 => rng.range(17..513u32),
        85..95 => rng.range(513..4097u32),
        _ => rng.range(4097..12_289u32),
    };
    n as usize
}

fn floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// A position: often a chunk seam or a line break (± a few bytes), now and
/// then raw (possibly invalid).
fn position(rng: &mut Pcg32, rope: &Rope, model: &str) -> usize {
    let len = model.len();
    match rng.range(0..100u32) {
        0..5 => 0,
        5..10 => len,
        10..30 => {
            let mut seams = vec![0];
            for chunk in rope.chunks() {
                seams.push(seams.last().copied().unwrap_or(0) + chunk.len());
            }
            let seam = seams[rng.range(0..seams.len() as u32) as usize];
            let delta = rng.range(-3..4);
            floor(model, seam.saturating_add_signed(delta as isize))
        }
        30..45 => match model.rfind(['\r', '\n']) {
            Some(last) => {
                let near = floor(model, rng.range(0..last as u32 + 1) as usize);
                let at = model[near..].find(['\r', '\n']).map_or(near, |i| near + i);
                floor(model, at + rng.range(0..3u32) as usize)
            }
            None => floor(model, rng.range(0..len as u32 + 1) as usize),
        },
        45..95 => floor(model, rng.range(0..len as u32 + 1) as usize),
        _ => rng.range(0..len as u32 + 3) as usize,
    }
}

fn byte_error(text: &str, byte: usize) -> Result<(), TextError> {
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

fn range_error(text: &str, start: usize, end: usize) -> Result<(), TextError> {
    if start > end {
        return Err(TextError::InvertedRange { start, end });
    }
    if end > text.len() {
        return Err(TextError::ByteOutOfBounds {
            byte: end,
            len: text.len(),
        });
    }
    // Both are now in bounds, so only boundaries are left: start, then end.
    byte_error(text, start).and_then(|()| byte_error(text, end))
}

/// Compares results; errors by their `Debug` form (numbers only).
fn same<T: Debug>(
    what: &str,
    got: &Result<T, TextError>,
    want: &Result<(), TextError>,
) -> Result<(), String> {
    let ok = match (got, want) {
        (Ok(_), Ok(())) => true,
        (Err(a), Err(b)) => format!("{a:?}") == format!("{b:?}"),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

fn content(rope: &Rope, model: &str, what: &str) -> Result<(), String> {
    if *rope != *model {
        return Err(format!("{what}: the text differs ({} bytes)", model.len()));
    }
    // CR LF is one break; any other CR or LF is one.
    let lines = 1 + model.matches(['\r', '\n']).count() - model.matches("\r\n").count();
    let sizes = (
        rope.len_in(Unit::Char),
        rope.len_in(Unit::Utf16),
        rope.len_lines(),
    );
    let want = (model.chars().count(), model.encode_utf16().count(), lines);
    if sizes != want {
        return Err(format!(
            "{what}: (chars, utf16, lines) = {sizes:?}, want {want:?}"
        ));
    }
    Ok(())
}

/// One case: its input is the seed of its own generator, and its size.
#[derive(Debug)]
struct Plan {
    seed: u64,
    size: u32,
}

fn run(plan: &Plan) -> Result<(), String> {
    let mut rng = Pcg32::new(plan.seed, u64::from(plan.size));
    let initial_len = plan.size as usize * (MAX_TEXT / 100);
    let mut model = text(&mut rng, initial_len);
    let mut rope = Rope::from(model.as_str());
    rope.__check_invariants()?;
    content(&rope, &model, "the initial rope")?;
    let mut snapshots = Vec::new();
    let steps = plan.size / 4 + 1;
    for step in 0..steps {
        let before = rope.clone();
        let mut unchanged = false;
        match rng.range(0..100u32) {
            0..30 => {
                let at = position(&mut rng, &rope, &model);
                let len = insert_len(&mut rng).min(MAX_TEXT.saturating_sub(model.len()));
                let insert = text(&mut rng, len);
                let want = byte_error(&model, at);
                let got = rope.insert(at, &insert);
                same("insert", &got, &want)?;
                match got {
                    Ok(()) => model.insert_str(at, &insert),
                    Err(_) => unchanged = true,
                }
            }
            30..65 => {
                let start = position(&mut rng, &rope, &model);
                let end = if rng.range(0..2u32) == 0 {
                    floor(&model, start + insert_len(&mut rng))
                } else {
                    position(&mut rng, &rope, &model)
                };
                let replace = if rng.range(0..7u32) < 3 {
                    let len = insert_len(&mut rng).min(MAX_TEXT.saturating_sub(model.len()));
                    text(&mut rng, len)
                } else {
                    String::new()
                };
                let want = range_error(&model, start, end);
                let got = rope.replace(start..end, &replace);
                same("replace", &got, &want)?;
                match got {
                    Ok(()) => model.replace_range(start..end, &replace),
                    Err(_) => unchanged = true,
                }
            }
            65..77 => {
                let at = position(&mut rng, &rope, &model);
                let want = byte_error(&model, at);
                let got = rope.split_off(at);
                same("split_off", &got, &want)?;
                match got {
                    Ok(mut right) => {
                        rope.__check_invariants()?;
                        right.__check_invariants()?;
                        content(&rope, &model[..at], "the left part")?;
                        content(&right, &model[at..], "the right part")?;
                        if rng.range(0..2u32) == 0 {
                            rope.append(right);
                        } else {
                            right.append(rope);
                            rope = right;
                            model = format!("{}{}", &model[at..], &model[..at]);
                        }
                    }
                    Err(_) => unchanged = true,
                }
            }
            77..82 => {
                let len = insert_len(&mut rng).min(MAX_TEXT.saturating_sub(model.len()));
                let tail = text(&mut rng, len);
                rope.append(Rope::from(tail.as_str()));
                model.push_str(&tail);
            }
            82..90 => {
                let (a, b) = (
                    position(&mut rng, &rope, &model),
                    position(&mut rng, &rope, &model),
                );
                let want = range_error(&model, a, b);
                let got = rope.slice(a..b);
                same("slice", &got, &want)?;
                if let Ok(slice) = got {
                    slice.__check_invariants()?;
                    content(&slice, &model[a..b], "the slice")?;
                }
                unchanged = true;
            }
            90..95 => snapshots.push((rope.clone(), model.clone())),
            _ => {
                let mut builder = RopeBuilder::new();
                let mut from = 0;
                while from < model.len() {
                    let to = floor(&model, from + rng.range(1..2049u32) as usize).max(from + 1);
                    let to = model.ceil_char_boundary(to);
                    builder.push_str(&model[from..to]);
                    from = to;
                }
                rope = builder.finish();
            }
        }
        rope.__check_invariants()
            .map_err(|e| format!("step {step}: {e}"))?;
        if unchanged && rope != before {
            return Err(format!(
                "step {step}: a refused edit or a slice changed the rope"
            ));
        }
        if step % 8 == 7 {
            content(&rope, &model, &format!("step {step}"))?;
        }
    }
    content(&rope, &model, "the final rope")?;
    for (index, (snapshot, text)) in snapshots.iter().enumerate() {
        snapshot.__check_invariants()?;
        content(snapshot, text, &format!("snapshot {index}"))?;
    }
    Ok(())
}

#[test]
fn rope_matches_string_model_at_real_sizes() {
    check_sized(
        "rope matches a String model at real sizes",
        300,
        |rng, size| Plan {
            seed: (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32()),
            size,
        },
        run,
    );
}

#[test]
fn eight_mebibytes_build_in_one_push_and_in_kibibyte_pushes() {
    // 37 bytes per repeat, so CR LF pairs and 4-byte chars fall across the
    // 1 KiB push boundaries.
    let pattern = "fn main() {\r\n  \"😀 中 é\"\r}\n\r\n";
    let mut text = String::with_capacity((8 << 20) + pattern.len());
    while text.len() < 8 << 20 {
        text.push_str(pattern);
    }
    let one = Rope::from(text.as_str());
    let mut builder = RopeBuilder::new();
    let mut from = 0;
    while from < text.len() {
        let to = floor(&text, from + 1024).max(from + 1);
        let to = text.ceil_char_boundary(to);
        builder.push_str(&text[from..to]);
        from = to;
    }
    let pieces = builder.finish();
    let owned = Rope::from(text.clone());
    for (rope, what) in [
        (&one, "one push"),
        (&pieces, "1 KiB pushes"),
        (&owned, "From<String>"),
    ] {
        rope.__check_invariants()
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        content(rope, &text, what).unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(one == pieces && pieces == owned);
    assert_eq!(one.to_string(), text);
}

#[test]
fn debug_never_shows_the_text() {
    let shown = format!("{:?}", Rope::from("secret"));
    assert!(!shown.contains("secret"), "{shown}");
    let big = Rope::from("secret\n".repeat(2_000).as_str());
    let shown = format!("{big:?}");
    assert!(!shown.contains("secret"), "{shown}");
    assert_eq!(
        shown,
        "Rope { bytes: 14000, chars: 14000, utf16: 14000, lines: 2001 }"
    );
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
/// be non-empty, they must spell `model[start..end]`, and the iterator must
/// stay empty at both ends once it is done.
fn chunks_in_every_order(rope: &Rope, model: &str, start: usize, end: usize) -> Result<(), String> {
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
        if head.concat() != model[start..end] {
            return Err(format!(
                "{order}: the chunks of {start}..{end} do not spell the range"
            ));
        }
    }
    Ok(())
}

/// `chunks_in` never yields an empty chunk at the real shape: on ropes of
/// about 12,000 bytes appended in pieces of 3,072, for ranges that start
/// and end at every chunk seam and the bytes either side of it (so `0..s`,
/// `s..len` and `s..s` for every seam `s`), from either end and in mixed
/// orders.
#[test]
fn chunks_in_never_yields_an_empty_chunk_at_a_seam() {
    check(
        "chunks_in never yields an empty chunk at a seam",
        20,
        |rng| (u64::from(rng.next_u32()), u64::from(rng.next_u32())),
        |&(seed, stream)| {
            let mut rng = Pcg32::new(seed, stream);
            let model = text(&mut rng, 12_000);
            let mut rope = Rope::new();
            let mut from = 0;
            while from < model.len() {
                let to = model.floor_char_boundary(from + 3_072);
                rope.append(Rope::from(&model[from..to]));
                from = to;
            }
            rope.__check_invariants()?;
            content(&rope, &model, "the rope")?;
            let mut seams = vec![0];
            for chunk in rope.chunks() {
                seams.push(seams.last().copied().unwrap_or(0) + chunk.len());
            }
            if seams.len() < 5 {
                return Err(format!("only {} seams", seams.len()));
            }
            let mut ends: Vec<usize> = seams
                .into_iter()
                .flat_map(|seam| [seam.saturating_sub(1), seam, seam + 1])
                .filter(|&byte| model.is_char_boundary(byte))
                .collect();
            ends.sort_unstable();
            ends.dedup();
            for (i, &start) in ends.iter().enumerate() {
                for &end in &ends[i..] {
                    chunks_in_every_order(&rope, &model, start, end)?;
                }
            }
            Ok(())
        },
    );
}
