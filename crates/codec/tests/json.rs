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

//! JSON tests against the hand-made vectors in `tests/json/`, and property
//! tests of the parser and the writers working together.
//!
//! Ported from nadir-codec `tests/json.rs`, with its RFC 8259 example files
//! and `escapes.json`. Adapted for Tidepool: both dialects, `u64` integers,
//! byte input (random bytes and byte-level mutations, which can break
//! UTF-8), checks on error offsets, and Tidepool fixtures (`settings.jsonc`,
//! `cargo_metadata.json`) in place of Nadir's game prefabs.
//!
//! Every property test prints its seed and a command to rerun it if it
//! fails; set `TIDEPOOL_SEED` to run it with another seed.
//!
//! The no-panic properties also run each input under random small
//! [`Limits`], so that every limit's error path is reached, and give each
//! input a deadline, so that a parser that hangs fails with its seed
//! instead of stalling the run. Real fuzzing arrives with `xtask
//! fuzz-smoke` (roadmap M1.6).

mod common;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use common::identical;
use tidepool_codec::json::{self, Dialect, ErrorKind, Number, Value};
use tidepool_codec::{CodecError, Limits};
use tidepool_core::Pcg32;
use tidepool_core::testing::{check, check_sized};

const DIALECTS: [Dialect; 2] = [Dialect::Json, Dialect::Jsonc];

fn parse(text: &str, dialect: Dialect) -> Result<Value, CodecError> {
    json::parse(text, dialect, Limits::DEFAULT)
}

fn parse_ok(text: &str, dialect: Dialect) -> Value {
    parse(text, dialect).unwrap_or_else(|error| panic!("{error}\nin:\n{text}"))
}

fn position_and_kind(text: &str, dialect: Dialect) -> (usize, usize, ErrorKind) {
    match parse(text, dialect) {
        Err(CodecError::Json {
            line, column, kind, ..
        }) => (line, column, kind),
        Err(other) => panic!("not a JSON error: {other}"),
        Ok(value) => panic!("should not parse, but gave {value:?}"),
    }
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("an object")
        .iter()
        .map(|(key, _)| key.as_str())
        .collect()
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no string {key:?} in {value:?}"))
}

fn int(value: i64) -> Value {
    Value::from(value)
}

fn float(value: f64) -> Value {
    Value::Number(Number::from_f64(value).expect("finite"))
}

/// Writes `value` both ways and checks that each reads back identically in
/// both dialects.
fn round_trips(value: &Value) -> Result<(), String> {
    for written in [
        json::to_string_compact(value),
        json::to_string_pretty(value),
    ] {
        for dialect in DIALECTS {
            let back = parse(&written, dialect).map_err(|error| {
                format!("wrote {written:?}, which fails as {dialect:?}: {error}")
            })?;
            if !identical(&back, value) {
                return Err(format!("wrote {written:?}, which reads back as {back:?}"));
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------- RFC 8259

// json/rfc8259_image.json and json/rfc8259_locations.json are the examples
// of IETF RFC 8259, section 13, which the tests below also quote. They are
// the IETF Trust's, not the project's, and keep its notice (ADR-0021):
//
// Copyright (c) 2017 IETF Trust and the persons identified as authors of
// the code. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, is permitted pursuant to, and subject to the license terms
// contained in, the Revised BSD License set forth in Section 4.c of the
// IETF Trust's Legal Provisions Relating to IETF Documents
// (https://trustee.ietf.org/license-info).

const RFC_IMAGE: &str = include_str!("json/rfc8259_image.json");
const RFC_LOCATIONS: &str = include_str!("json/rfc8259_locations.json");

#[test]
fn rfc_8259_image_example() {
    let document = parse_ok(RFC_IMAGE, Dialect::Json);
    assert_eq!(keys(&document), ["Image"]);
    let image = document.get("Image").expect("Image");
    assert_eq!(
        keys(image),
        ["Width", "Height", "Title", "Thumbnail", "Animated", "IDs"]
    );
    assert_eq!(image.get("Width"), Some(&int(800)));
    assert_eq!(image.get("Height"), Some(&int(600)));
    assert_eq!(str_of(image, "Title"), "View from 15th Floor");
    assert_eq!(image.get("Animated"), Some(&Value::Bool(false)));
    let ids: Vec<Value> = [116, 943, 234, 38793].map(int).to_vec();
    assert_eq!(image.get("IDs"), Some(&Value::Array(ids)));

    let thumbnail = image.get("Thumbnail").expect("Thumbnail");
    assert_eq!(keys(thumbnail), ["Url", "Height", "Width"]);
    // The `//` in the URL is inside a string, so it is not a comment.
    assert_eq!(
        str_of(thumbnail, "Url"),
        "http://www.example.com/image/481989943"
    );
    assert_eq!(thumbnail.get("Height").and_then(Value::as_i64), Some(125));
    assert_eq!(thumbnail.get("Width").and_then(Value::as_u64), Some(100));
}

#[test]
fn rfc_8259_locations_example() {
    let document = parse_ok(RFC_LOCATIONS, Dialect::Json);
    let places = document.as_array().expect("an array");
    assert_eq!(places.len(), 2);
    let expected_keys = [
        "precision",
        "Latitude",
        "Longitude",
        "Address",
        "City",
        "State",
        "Zip",
        "Country",
    ];

    let san_francisco = &places[0];
    assert_eq!(keys(san_francisco), expected_keys);
    assert_eq!(str_of(san_francisco, "precision"), "zip");
    assert_eq!(san_francisco.get("Latitude"), Some(&float(37.7668)));
    assert_eq!(san_francisco.get("Longitude"), Some(&float(-122.3959)));
    assert_eq!(str_of(san_francisco, "Address"), "");
    assert_eq!(str_of(san_francisco, "City"), "SAN FRANCISCO");
    assert_eq!(str_of(san_francisco, "State"), "CA");
    // A zip code in quotes is a string, not a number.
    assert_eq!(str_of(san_francisco, "Zip"), "94107");
    assert_eq!(str_of(san_francisco, "Country"), "US");

    let sunnyvale = &places[1];
    assert_eq!(keys(sunnyvale), expected_keys);
    assert_eq!(sunnyvale.get("Latitude"), Some(&float(37.371991)));
    assert_eq!(sunnyvale.get("Longitude"), Some(&float(-122.02602)));
    assert_eq!(str_of(sunnyvale, "City"), "SUNNYVALE");
    assert_eq!(str_of(sunnyvale, "Zip"), "94085");
}

#[test]
fn rfc_8259_scalar_examples() {
    // "Here are three small JSON texts containing only values."
    assert_eq!(
        parse_ok("\"Hello world!\"", Dialect::Json),
        Value::from("Hello world!")
    );
    assert_eq!(parse_ok("42", Dialect::Json), int(42));
    assert_eq!(parse_ok("true", Dialect::Json), Value::Bool(true));
}

#[test]
fn rfc_8259_examples_survive_being_written_and_read_back() {
    for text in [RFC_IMAGE, RFC_LOCATIONS] {
        assert_eq!(round_trips(&parse_ok(text, Dialect::Json)), Ok(()));
    }
    let image = json::to_string_compact(&parse_ok(RFC_IMAGE, Dialect::Json));
    assert!(image.starts_with(
        r#"{"Image":{"Width":800,"Height":600,"Title":"View from 15th Floor","Thumbnail":{"#
    ));
    assert!(image.ends_with(r#""Animated":false,"IDs":[116,943,234,38793]}}"#));
}

#[test]
fn escapes_file_decodes_every_kind_of_escape() {
    let document = parse_ok(include_str!("json/escapes.json"), Dialect::Json);
    assert_eq!(
        keys(&document),
        ["simple", "bmp", "pair", "control", "mixed", "key"]
    );
    assert_eq!(
        str_of(&document, "simple"),
        "quote \" backslash \\ slash / \u{8}\u{c}\n\r\t"
    );
    assert_eq!(str_of(&document, "bmp"), "Aé€");
    assert_eq!(str_of(&document, "pair"), "😀😀");
    assert_eq!(str_of(&document, "control"), "\0\u{1f}\u{7f}");
    assert_eq!(str_of(&document, "mixed"), "abc é é 😀 😀");
    assert_eq!(str_of(&document, "key"), "escapes work in keys too");
}

// ---------------------------------------------------------- cargo metadata

const CARGO_METADATA: &[u8] = include_bytes!("json/cargo_metadata.json");

#[test]
fn cargo_metadata_output_reads_as_bytes() {
    let metadata = json::parse_bytes(CARGO_METADATA, Dialect::Json, Limits::DEFAULT)
        .expect("cargo metadata parses");
    assert_eq!(metadata.get("version").and_then(Value::as_u64), Some(1));
    assert_eq!(str_of(&metadata, "workspace_root"), "/work/demo");

    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .expect("packages");
    let sources: Vec<(&str, Option<&str>)> = packages
        .iter()
        .map(|package| {
            let source = package.get("source").expect("every package has a source");
            (str_of(package, "name"), source.as_str())
        })
        .collect();
    assert_eq!(
        sources,
        [
            ("demo-app", None),
            ("demo-core", None),
            (
                "itoa",
                Some("registry+https://github.com/rust-lang/crates.io-index")
            ),
        ]
    );
    let scale = packages[0]
        .get("metadata")
        .and_then(|metadata| metadata.get("tidepool"))
        .and_then(|tidepool| tidepool.get("scale"))
        .and_then(Value::as_f64);
    assert_eq!(scale, Some(1.5));

    let members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .expect("workspace members");
    assert_eq!(members.len(), 2);
    let nodes = metadata
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(Value::as_array)
        .expect("resolve nodes");
    assert_eq!(nodes.len(), packages.len());
    assert!(metadata.get("metadata").is_some_and(Value::is_null));

    // Written back compactly, it is the same text: member order is kept and
    // nothing is escaped that needn't be.
    let text = std::str::from_utf8(CARGO_METADATA).expect("UTF-8");
    assert_eq!(json::to_string_compact(&metadata), text.trim_end());
}

// ------------------------------------------------------------------ JSONC

const SETTINGS: &str = include_str!("json/settings.jsonc");
const SETTINGS_PRETTY: &str = include_str!("json/settings.pretty.json");

#[test]
fn jsonc_settings_parse_with_comments_and_trailing_commas() {
    let settings = parse_ok(SETTINGS, Dialect::Jsonc);
    assert_eq!(
        keys(&settings),
        ["theme", "font", "editor", "languages", "note"]
    );
    assert_eq!(str_of(&settings, "theme"), "tidepool-dark");

    let font = settings.get("font").expect("font");
    assert_eq!(keys(font), ["family", "size", "ligatures"]);
    assert_eq!(font.get("size"), Some(&float(13.5)));

    let editor = settings.get("editor").expect("editor");
    assert_eq!(
        keys(editor),
        [
            "rulers",
            "tab_width",
            "scroll_speed",
            "keymap",
            "format_on_save"
        ]
    );
    assert_eq!(
        editor.get("rulers"),
        Some(&Value::Array(vec![int(80), int(100)]))
    );
    assert_eq!(editor.get("scroll_speed"), Some(&float(0.25)));
    assert_eq!(editor.get("format_on_save"), Some(&Value::Bool(true)));

    let languages = settings.get("languages").expect("languages");
    assert_eq!(keys(languages), ["rust", "markdown"]);
    let rust = languages.get("rust").expect("rust");
    assert!(rust.get("env").is_some_and(Value::is_null));
    let zoom = languages
        .get("markdown")
        .and_then(|markdown| markdown.get("preview"))
        .and_then(|preview| preview.get("zoom"));
    assert_eq!(zoom, Some(&float(-1.0)));

    assert_eq!(
        str_of(&settings, "note"),
        "50% → \"ok\"\t(tab), url: http://example.com/a//b /* not a comment */"
    );
}

#[test]
fn jsonc_settings_are_not_strict_json() {
    assert_eq!(
        position_and_kind(SETTINGS, Dialect::Json),
        (1, 1, ErrorKind::CommentNotAllowed)
    );
}

#[test]
fn jsonc_settings_pretty_print_to_the_committed_file() {
    // The committed file may be checked out with Windows line endings.
    let expected = SETTINGS_PRETTY.replace("\r\n", "\n");
    assert_eq!(
        json::to_string_pretty(&parse_ok(SETTINGS, Dialect::Jsonc)),
        expected.trim_end()
    );
    // The pretty form is strict JSON for the same value.
    assert!(identical(
        &parse_ok(SETTINGS_PRETTY, Dialect::Json),
        &parse_ok(SETTINGS, Dialect::Jsonc)
    ));
}

#[test]
fn malformed_files_report_line_and_column() {
    let expected = ErrorKind::UnexpectedCharacter {
        found: '"',
        expected: "`,` or `}`",
    };
    assert_eq!(
        position_and_kind(
            include_str!("json/invalid_missing_comma.jsonc"),
            Dialect::Jsonc
        ),
        (4, 3, expected)
    );

    let expected = ErrorKind::UnexpectedEnd {
        expected: "`*/` to close the comment that starts here",
    };
    assert_eq!(
        position_and_kind(
            include_str!("json/invalid_unterminated_comment.jsonc"),
            Dialect::Jsonc
        ),
        (3, 3, expected)
    );
}

#[test]
fn a_key_pasted_twice_is_kept_twice_and_the_last_one_wins() {
    let settings = parse_ok(include_str!("json/repeated_key.jsonc"), Dialect::Jsonc);
    let languages = settings.get("languages").expect("languages");
    assert_eq!(keys(languages), ["rust", "toml", "rust"]);
    let args = languages
        .get("rust")
        .and_then(|rust| rust.get("args"))
        .and_then(Value::as_array);
    assert_eq!(args, Some(&[Value::from("--log-file")][..]));
    // Written back, both members are still there, in order.
    let written = json::to_string_compact(&settings);
    assert_eq!(written.matches(r#""rust":"#).count(), 2);
    assert!(identical(&parse_ok(&written, Dialect::Json), &settings));
}

// --------------------------------------------------------------- pretty size

/// How deeply `value`'s arrays and objects nest, as `Limits::max_depth`
/// counts: 0 for a scalar.
fn depth(value: &Value) -> usize {
    // Each container is one level deeper than the containers around it.
    let mut deepest = 0;
    let mut work = vec![(value, 1)];
    while let Some((value, depth)) = work.pop() {
        let children: Vec<&Value> = match value {
            Value::Array(items) => items.iter().collect(),
            Value::Object(members) => members.iter().map(|(_, value)| value).collect(),
            _ => continue,
        };
        deepest = deepest.max(depth);
        work.extend(children.into_iter().map(|child| (child, depth + 1)));
    }
    deepest
}

/// The pretty form's length divided by the compact form's.
fn pretty_ratio(text: &str) -> f64 {
    let value = parse_ok(text, Dialect::Json);
    let compact = json::to_string_compact(&value).len();
    let pretty = json::to_string_pretty(&value).len();
    pretty as f64 / compact as f64
}

#[test]
fn pretty_output_size_matches_the_documented_worst_cases() {
    let limit = Limits::DEFAULT.max_depth;
    assert_eq!(depth(&parse_ok("[[1], {\"a\": []}]", Dialect::Json)), 3);
    // One-digit numbers 127 levels down: about `d + 1.5` times.
    let zeros = format!(
        "{}{}0{}",
        "[".repeat(limit - 1),
        "0,".repeat(1000),
        "]".repeat(limit - 1)
    );
    let ratio = pretty_ratio(&zeros);
    assert!((128.0..129.0).contains(&ratio), "zeros: {ratio}");
    // Short chains of one-entry arrays at the deepest level: about 236
    // times at depth 128, as `to_string_pretty` documents, and under the
    // `2d + 2` bound.
    let chain = 11;
    let tooth = format!("{}0{}", "[".repeat(chain), "]".repeat(chain));
    let comb = format!(
        "{}{}{}",
        "[".repeat(limit - chain),
        vec![tooth; 1000].join(","),
        "]".repeat(limit - chain)
    );
    assert_eq!(depth(&parse_ok(&comb, Dialect::Json)), limit);
    let ratio = pretty_ratio(&comb);
    assert!(
        (230.0..=(2 * limit + 2) as f64).contains(&ratio),
        "comb: {ratio}"
    );
}

#[test]
fn pretty_output_is_at_most_2d_plus_2_times_the_compact_output() {
    check_sized("json pretty size", 1000, random_value, |value| {
        let bound = 2 * depth(value) + 2;
        let compact = json::to_string_compact(value).len();
        let pretty = json::to_string_pretty(value).len();
        if pretty > bound * compact {
            return Err(format!(
                "pretty is {pretty} bytes, compact {compact}, bound {bound}x"
            ));
        }
        Ok(())
    });
}

// ------------------------------------------------------------- generators

const INTERESTING_CHARS: &[char] = &[
    'a',
    'Z',
    '0',
    ' ',
    '"',
    '\\',
    '/',
    '\n',
    '\r',
    '\t',
    '\u{8}',
    '\u{c}',
    '\0',
    '\u{1f}',
    '\u{7f}',
    '\u{80}',
    'é',
    '€',
    '\u{2028}',
    '\u{feff}',
    '\u{fffd}',
    '\u{ffff}',
    '😀',
    '\u{10ffff}',
    '*',
    '{',
    '[',
    ',',
    ':',
    'u',
];

fn random_char(rng: &mut Pcg32) -> char {
    match rng.range(0..4u32) {
        0 => char::from_u32(rng.range(0x20..0x7fu32)).unwrap_or('?'),
        // Surrogates are not characters; fall back to something that is.
        1 => char::from_u32(rng.range(0..0x11_0000u32)).unwrap_or('\u{fffd}'),
        _ => rng.choose(INTERESTING_CHARS).copied().unwrap_or('?'),
    }
}

/// Usually short; one time in ten, long enough to pass the small string
/// limits the no-panic properties set by far.
fn random_string(rng: &mut Pcg32) -> String {
    let len = if rng.range(0..10u32) == 0 {
        rng.range(0..100u32)
    } else {
        rng.range(0..12u32)
    };
    (0..len).map(|_| random_char(rng)).collect()
}

fn random_u64(rng: &mut Pcg32) -> u64 {
    (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())
}

fn random_integer(rng: &mut Pcg32) -> Number {
    match rng.range(0..5u32) {
        0 => Number::from(rng.range(-100..100)),
        1 => Number::from(rng.next_u32() as i32),
        2 => Number::from(random_u64(rng) as i64),
        3 => Number::from(random_u64(rng)),
        _ => rng
            .choose(&[
                Number::from(0u64),
                Number::from(1u64),
                Number::from(-1i64),
                Number::from(i64::MIN),
                Number::from(i64::MAX),
                Number::from(1u64 << 63),
                Number::from(u64::MAX),
                Number::from(1i64 << 53),
                Number::from(-(1i64 << 53)),
            ])
            .copied()
            .unwrap_or(Number::from(0u64)),
    }
}

/// A finite float.
fn random_float(rng: &mut Pcg32) -> f64 {
    const SPECIAL: &[f64] = &[
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.1,
        1e-5,
        9.999e-6,
        1e16,
        9_999_999_999_999_998.0,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        5e-324,
        1e-320,
    ];
    match rng.range(0..5u32) {
        0 => f64::from(rng.range(-1000.0..1000.0f32)),
        1 => f64::from(rng.range(-1000..1000)),
        2 => 10f64.powi(rng.range(-320..309)),
        3 => loop {
            let value = f64::from_bits(random_u64(rng));
            if value.is_finite() {
                break value;
            }
        },
        _ => rng.choose(SPECIAL).copied().unwrap_or(0.0),
    }
}

fn random_number(rng: &mut Pcg32) -> Number {
    if rng.next_u32() & 1 == 0 {
        random_integer(rng)
    } else {
        Number::from_f64(random_float(rng)).unwrap_or(Number::from(0u64))
    }
}

/// A value with about `size` entries in its containers, nested at most six
/// deep, with a repeated key now and then. One time in eight it is then
/// wrapped in up to 23 more one-entry arrays and objects, so that some
/// values nest far deeper. All of it fits [`Limits::DEFAULT`], so the
/// writers promise to round-trip it.
fn random_value(rng: &mut Pcg32, size: u32) -> Value {
    let mut budget = size;
    let mut value = random_value_at(rng, &mut budget, 0);
    if rng.range(0..8u32) == 0 {
        for _ in 0..rng.range(1..24u32) {
            value = if rng.next_u32() & 1 == 0 {
                Value::Array(vec![value])
            } else {
                Value::Object(vec![(random_string(rng), value)])
            };
        }
    }
    value
}

fn random_value_at(rng: &mut Pcg32, budget: &mut u32, depth: u32) -> Value {
    let kinds = if *budget == 0 || depth >= 6 { 4 } else { 6 };
    match rng.range(0..kinds) {
        0 => Value::Null,
        1 => Value::Bool(rng.next_u32() & 1 == 0),
        2 => Value::Number(random_number(rng)),
        3 => Value::String(random_string(rng)),
        container => {
            let len = rng.range(0..(*budget).min(6) + 1);
            *budget -= len;
            if container == 4 {
                Value::Array(
                    (0..len)
                        .map(|_| random_value_at(rng, budget, depth + 1))
                        .collect(),
                )
            } else {
                let mut members: Vec<(String, Value)> = Vec::new();
                for _ in 0..len {
                    // One key in four repeats an earlier one, if there is one.
                    let earlier = members.len() as u32;
                    let key = if earlier > 0 && rng.range(0..4u32) == 0 {
                        let index = rng.range(0..earlier) as usize;
                        members.get(index).map(|(key, _)| key.clone())
                    } else {
                        None
                    };
                    let key = key.unwrap_or_else(|| random_string(rng));
                    members.push((key, random_value_at(rng, budget, depth + 1)));
                }
                Value::Object(members)
            }
        }
    }
}

/// Fragments that are, or look like, pieces of JSON.
const TOKENS: &[&str] = &[
    "{",
    "}",
    "[",
    "]",
    ",",
    ":",
    "\"",
    "\\",
    "\\u",
    "\\ud83d",
    "\\ude00",
    "\\u0041",
    "\\n",
    "\\x",
    "00e9",
    "/",
    "*",
    "//",
    "/*",
    "*/",
    "\n",
    "\r\n",
    " ",
    "\t",
    "null",
    "true",
    "false",
    "nul",
    "0",
    "1",
    "-",
    ".",
    "e",
    "E",
    "+",
    "12",
    "-0",
    "1e5",
    "1e400",
    "18446744073709551616",
    "a",
    "é",
    "😀",
    "\u{feff}",
    "\0",
    "\"a\"",
    "\"k\":",
    "[]",
    "{}",
    "\"\"",
    ",]",
    ",}",
];

fn random_tokens(rng: &mut Pcg32, count: u32) -> String {
    (0..count)
        .map(|_| rng.choose(TOKENS).copied().unwrap_or(""))
        .collect()
}

/// Checks what must hold for any input at all, in both dialects: the parser
/// returns instead of panicking; an error's offset, line and column agree
/// and point inside the input; an `InvalidUtf8` error is exactly where the
/// UTF-8 goes wrong; and whatever parses can be written and read back
/// unchanged. Strict JSON never accepts what JSONC rejects.
///
/// Under `limits`, which may be smaller than the defaults, the result must
/// be the same as under the defaults, or an error for exceeding one of
/// `limits` that names the right limit and is just as consistent.
fn parser_behaves(bytes: &[u8], limits: Limits) -> Result<(), String> {
    let mut accepted = [false; 2];
    for (index, dialect) in DIALECTS.into_iter().enumerate() {
        let result = json::parse_bytes(bytes, dialect, Limits::DEFAULT);
        match &result {
            Ok(value) => {
                accepted[index] = true;
                round_trips(value)?;
            }
            Err(error) => error_is_consistent(bytes, error)
                .map_err(|problem| format!("{dialect:?}: {error:?}: {problem}"))?,
        }
        // Text input must give the same result as bytes.
        if let Ok(text) = std::str::from_utf8(bytes)
            && json::parse(text, dialect, Limits::DEFAULT) != result
        {
            return Err(format!("{dialect:?}: text and bytes disagree"));
        }
        if limits != Limits::DEFAULT {
            let limited = json::parse_bytes(bytes, dialect, limits);
            if limited != result {
                let error = limited.err().ok_or_else(|| {
                    format!("{dialect:?}: accepted under {limits:?} but not the defaults")
                })?;
                limit_error_is_right(bytes, &error, limits)
                    .and_then(|()| error_is_consistent(bytes, &error))
                    .map_err(|problem| {
                        format!(
                            "{dialect:?} under {limits:?}: {error:?}: {problem}; \
                             under the defaults: {result:?}"
                        )
                    })?;
            }
        }
    }
    if accepted == [true, false] {
        return Err("strict JSON accepted what JSONC rejected".to_string());
    }
    Ok(())
}

/// Checks that `error`, which `limits` caused, is for exceeding one of
/// them, and names the limit it exceeded.
fn limit_error_is_right(bytes: &[u8], error: &CodecError, limits: Limits) -> Result<(), String> {
    let CodecError::Json { offset, kind, .. } = error else {
        return Err("not a JSON error".to_string());
    };
    let right = match *kind {
        ErrorKind::InputTooLarge { length, limit } => {
            // At the character that holds the first byte past the limit.
            length == bytes.len()
                && limit == limits.max_input_bytes
                && length > limit
                && *offset <= limit
                && limit - *offset <= 3
        }
        ErrorKind::TooDeep { limit } => limit == limits.max_depth,
        ErrorKind::StringTooLong { limit } => limit == limits.max_string_bytes,
        ErrorKind::NumberTooLong { limit } => limit == limits.max_number_bytes,
        ErrorKind::TooManyEntries { limit } => limit == limits.max_entries,
        _ => false,
    };
    if right {
        Ok(())
    } else {
        Err("not the error for a limit that was set".to_string())
    }
}

fn error_is_consistent(bytes: &[u8], error: &CodecError) -> Result<(), String> {
    let CodecError::Json {
        offset,
        line,
        column,
        kind,
    } = error
    else {
        return Err("not a JSON error".to_string());
    };
    let (offset, line, column) = (*offset, *line, *column);
    let valid_up_to = std::str::from_utf8(bytes)
        .err()
        .map(|error| error.valid_up_to());
    let size_error = matches!(kind, ErrorKind::InputTooLarge { .. });
    if *kind == ErrorKind::InvalidUtf8 {
        if valid_up_to != Some(offset) {
            return Err(format!(
                "offset {offset}, but UTF-8 is valid to {valid_up_to:?}"
            ));
        }
    } else if valid_up_to.is_some() && !size_error {
        // Only the size is checked before the UTF-8.
        return Err("an input that is not UTF-8 failed for another reason".to_string());
    }
    if offset > bytes.len() {
        return Err(format!("offset {offset} is past the end ({})", bytes.len()));
    }
    let before = match std::str::from_utf8(bytes.get(..offset).unwrap_or_default()) {
        Ok(before) => before,
        // A size error in an input that is not UTF-8 may follow bytes that
        // are not UTF-8 either; there is no character position to check.
        Err(_) if size_error && valid_up_to.is_some() => return Ok(()),
        Err(_) => return Err(format!("offset {offset} is not a character boundary")),
    };
    let before = before.strip_prefix('\u{feff}').unwrap_or(before);
    let expected_line = before.matches('\n').count() + 1;
    let line_text = before.rsplit('\n').next().unwrap_or(before);
    let expected_column = line_text.chars().count() + 1;
    if (line, column) != (expected_line, expected_column) {
        return Err(format!(
            "offset {offset} is line {expected_line}, column {expected_column}, \
             but the error says line {line}, column {column}"
        ));
    }
    Ok(())
}

/// Limits for a no-panic case: each one is the default or, half the time,
/// small enough that typical generated inputs exceed it. The input size is
/// limited one time in four, to a size up to the input's own.
fn random_limits(rng: &mut Pcg32, input_len: usize) -> Limits {
    let mut limits = Limits::DEFAULT;
    if rng.next_u32() & 1 == 0 {
        limits.max_depth = rng.range(0..9u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_entries = rng.range(0..7u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_string_bytes = rng.range(0..11u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_number_bytes = rng.range(1..9u32) as usize;
    }
    if rng.range(0..4u32) == 0 {
        limits.max_input_bytes = rng.range(0..input_len as u32 + 1) as usize;
    }
    limits
}

/// How long one no-panic case may take before it counts as a hang: far
/// longer than any case needs, even in a debug build on a busy machine.
const DEADLINE: Duration = Duration::from_secs(2);

/// [`parser_behaves`] on a thread of its own, failing if it takes longer
/// than [`DEADLINE`], so that a parser that hangs fails the property with
/// its seed instead of stalling the test run. A thread that hangs is left
/// behind; the test process ends it when it exits.
fn behaves_in_time((bytes, limits): &(Vec<u8>, Limits)) -> Result<(), String> {
    let (send, receive) = mpsc::channel();
    let (bytes, limits) = (bytes.clone(), *limits);
    thread::Builder::new()
        .spawn(move || {
            // The receiver only goes away after the deadline has passed.
            _ = send.send(parser_behaves(&bytes, limits));
        })
        .map_err(|error| format!("cannot start a thread: {error}"))?;
    match receive.recv_timeout(DEADLINE) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(format!("still running after {DEADLINE:?}")),
        Err(RecvTimeoutError::Disconnected) => Err("panicked; see the message above".to_string()),
    }
}

/// An input for the no-panic properties: `bytes`, with random limits.
fn with_limits(rng: &mut Pcg32, bytes: Vec<u8>) -> (Vec<u8>, Limits) {
    let limits = random_limits(rng, bytes.len());
    (bytes, limits)
}

// ------------------------------------------------------------- properties

#[test]
fn values_survive_being_written_and_read_back() {
    check_sized("json round-trip", 1000, random_value, round_trips);
}

#[test]
fn numbers_survive_being_written_and_read_back() {
    check("json number round-trip", 5000, random_number, |&number| {
        round_trips(&Value::Number(number))?;
        let written = number.to_string();
        let back = parse(&written, Dialect::Json).map_err(|error| error.to_string())?;
        match back.as_number() {
            Some(back) if back.is_integer() == number.is_integer() => Ok(()),
            other => Err(format!("{written} read back as {other:?}")),
        }
    });
}

/// Writes `text` as a JSON string in which every character is a `\u`
/// escape. Characters outside the basic plane become surrogate pairs.
fn fully_escaped(text: &str) -> String {
    let mut out = String::from("\"");
    for unit in text.encode_utf16() {
        // Hex digits are valid in either case; use both.
        let digits = if unit & 1 == 0 {
            format!("{unit:04x}")
        } else {
            format!("{unit:04X}")
        };
        out.push('\\');
        out.push('u');
        out.push_str(&digits);
    }
    out.push('"');
    out
}

#[test]
fn escaped_strings_decode_to_the_same_text() {
    check("json unicode escapes", 1000, random_string, |text| {
        let escaped = fully_escaped(text);
        match parse(&escaped, Dialect::Json) {
            Ok(Value::String(decoded)) if decoded == *text => Ok(()),
            other => Err(format!("{escaped} decoded as {other:?}")),
        }
    });
}

#[test]
fn compact_output_is_a_single_line() {
    check_sized("json compact is one line", 300, random_value, |value| {
        let written = json::to_string_compact(value);
        if written.contains('\n') || written.contains('\r') {
            return Err(format!("wrote {written:?}"));
        }
        Ok(())
    });
}

#[test]
fn writing_is_deterministic_and_keeps_member_order() {
    check_sized("json member order", 300, random_value, |value| {
        let first = json::to_string_pretty(value);
        if first != json::to_string_pretty(&value.clone()) {
            return Err("two writes differ".to_string());
        }
        if let Value::Object(members) = value {
            let back = parse(&first, Dialect::Json).map_err(|error| error.to_string())?;
            let order: Vec<&str> = members.iter().map(|(key, _)| key.as_str()).collect();
            if keys(&back) != order {
                return Err(format!("member order changed in {first}"));
            }
        }
        Ok(())
    });
}

/// The comments [`decorate`] starts with half the time.
const HEADER: &str = "// header\n/* block\n   comment */\n";

/// Rewrites pretty-printed JSON with the JSONC extensions: a trailing comma
/// after the last entry of every container, comments at random, and half
/// the time a header of comments.
fn decorate(pretty: &str, rng: &mut Pcg32) -> String {
    let lines: Vec<&str> = pretty.lines().collect();
    let mut out = String::new();
    if rng.next_u32() & 1 == 0 {
        out.push_str(HEADER);
    }
    for (index, line) in lines.iter().enumerate() {
        out.push_str(line);
        // The last entry of a container is followed by a line that closes
        // it. A line that opens a container is not an entry.
        let closes_next = lines
            .get(index + 1)
            .is_some_and(|next| next.trim_start().starts_with(['}', ']']));
        if closes_next && !line.ends_with(['{', '[']) {
            out.push(',');
        }
        match rng.range(0..6u32) {
            0 => out.push_str(" // note \" [ { , */"),
            1 => out.push_str(" /* note \" ] } , // */"),
            2 => out.push_str("\n   /* a line\n      of its own */"),
            _ => {}
        }
        out.push('\n');
    }
    out
}

#[test]
fn comments_and_trailing_commas_do_not_change_the_value() {
    check_sized(
        "jsonc decoration",
        500,
        |rng, size| {
            let value = random_value(rng, size);
            let decorated = decorate(&json::to_string_pretty(&value), rng);
            (value, decorated)
        },
        |(value, decorated)| {
            let back = parse(decorated, Dialect::Jsonc).map_err(|error| error.to_string())?;
            if !identical(&back, value) {
                return Err(format!("read back as {back:?}"));
            }
            // Strict JSON rejects the first comment or trailing comma: the
            // header's, if there is one. Only text left undecorated (a
            // scalar that drew no comment) is strict JSON.
            let undecorated = decorated.trim_end() == json::to_string_pretty(value);
            let header = decorated.starts_with(HEADER);
            match parse(decorated, Dialect::Json) {
                Err(CodecError::Json {
                    kind: ErrorKind::CommentNotAllowed,
                    offset: 0,
                    ..
                }) if header => Ok(()),
                Err(CodecError::Json {
                    kind: ErrorKind::CommentNotAllowed | ErrorKind::TrailingCommaNotAllowed,
                    ..
                }) if !header && !undecorated => Ok(()),
                Ok(back) if undecorated && identical(&back, value) => Ok(()),
                other => Err(format!("strict JSON gave {other:?}")),
            }
        },
    );
}

#[test]
fn random_token_soup_never_panics() {
    check_sized(
        "json token soup",
        3000,
        |rng, size| {
            let text = random_tokens(rng, size / 4 + 1);
            with_limits(rng, text.into_bytes())
        },
        behaves_in_time,
    );
}

#[test]
fn random_characters_never_panic() {
    check_sized(
        "json random characters",
        1000,
        |rng, size| {
            let text: String = (0..size).map(|_| random_char(rng)).collect();
            with_limits(rng, text.into_bytes())
        },
        behaves_in_time,
    );
}

#[test]
fn random_bytes_never_panic() {
    check_sized(
        "json random bytes",
        3000,
        |rng, size| {
            // Bytes that matter to JSON, so that inputs get far. Half the
            // inputs also get a uniformly random byte one time in twenty,
            // which is usually enough to break the UTF-8.
            const JSON_BYTES: &[u8] = b"{}[],:\"\\/*-+.0123456789eEuntrfals \n\t";
            let noisy = rng.next_u32() & 1 == 0;
            let bytes = (0..size)
                .map(|_| {
                    if noisy && rng.range(0..20u32) == 0 {
                        rng.next_u32() as u8
                    } else {
                        rng.choose(JSON_BYTES).copied().unwrap_or(b' ')
                    }
                })
                .collect();
            with_limits(rng, bytes)
        },
        behaves_in_time,
    );
}

/// A string literal of random escapes, valid and not, and raw characters,
/// left unclosed one time in four.
fn random_escapes(rng: &mut Pcg32, size: u32) -> String {
    // Surrogate halves, ordinary characters, and things that are not four
    // hex digits.
    const UNITS: &[&str] = &[
        "d800", "dbff", "dc00", "dfff", "D83D", "dE00", "0041", "00e9", "0000", "001f", "ffff",
        "12", "12G4", "+123", "",
    ];
    let mut text = String::from("\"");
    for _ in 0..size / 4 + 1 {
        match rng.range(0..6u32) {
            0 | 1 => {
                text.push_str("\\u");
                text.push_str(rng.choose(UNITS).copied().unwrap_or(""));
            }
            2 => text.push_str(&format!("\\u{:04x}", rng.range(0..0x1_0000u32))),
            3 => {
                text.push('\\');
                text.push(random_char(rng));
            }
            _ => text.push(random_char(rng)),
        }
    }
    if rng.range(0..4u32) != 0 {
        text.push('"');
    }
    text
}

#[test]
fn random_escapes_never_panic() {
    check_sized(
        "json random escapes",
        3000,
        |rng, size| {
            let text = random_escapes(rng, size);
            with_limits(rng, text.into_bytes())
        },
        behaves_in_time,
    );
}

/// One random edit of the text: delete, insert, replace, truncate, or
/// repeat a piece.
fn mutate_text(text: &mut String, rng: &mut Pcg32) {
    let chars: Vec<char> = text.chars().collect();
    let at = rng.range(0..chars.len() as u32 + 1) as usize;
    let (before, after) = chars.split_at(at);
    let before: String = before.iter().collect();
    let after: String = after.iter().collect();
    let token = rng.choose(TOKENS).copied().unwrap_or("");
    *text = match rng.range(0..5u32) {
        0 => before + &after.chars().skip(1).collect::<String>(),
        1 => before + token + &after,
        2 => before + token + &after.chars().skip(1).collect::<String>(),
        3 => before,
        _ => before.clone() + &after + &after,
    };
}

/// One random edit of the bytes, which may break UTF-8: flip a bit, set a
/// byte, delete or duplicate a byte, or cut the input short.
fn mutate_bytes(bytes: &mut Vec<u8>, rng: &mut Pcg32) {
    if bytes.is_empty() {
        bytes.push(rng.next_u32() as u8);
        return;
    }
    let at = rng.range(0..bytes.len() as u32) as usize;
    match rng.range(0..5u32) {
        0 => bytes[at] ^= 1 << rng.range(0..8u32),
        1 => bytes[at] = rng.next_u32() as u8,
        2 => _ = bytes.remove(at),
        3 => bytes.insert(at, bytes[at]),
        _ => bytes.truncate(at),
    }
}

/// A valid document, compact or pretty, or decorated as JSONC.
fn random_document(rng: &mut Pcg32, size: u32) -> String {
    let value = random_value(rng, size);
    match rng.range(0..3u32) {
        0 => json::to_string_compact(&value),
        1 => json::to_string_pretty(&value),
        _ => decorate(&json::to_string_pretty(&value), rng),
    }
}

#[test]
fn mutated_documents_never_panic() {
    check_sized(
        "json text mutation",
        3000,
        |rng, size| {
            let mut text = random_document(rng, size);
            for _ in 0..rng.range(1..4u32) {
                mutate_text(&mut text, rng);
            }
            with_limits(rng, text.into_bytes())
        },
        behaves_in_time,
    );
}

#[test]
fn byte_mutated_documents_never_panic() {
    check_sized(
        "json byte mutation",
        3000,
        |rng, size| {
            let mut bytes = random_document(rng, size).into_bytes();
            for _ in 0..rng.range(1..5u32) {
                mutate_bytes(&mut bytes, rng);
            }
            with_limits(rng, bytes)
        },
        behaves_in_time,
    );
}

#[test]
fn nesting_is_accepted_exactly_up_to_the_limit() {
    let limit = Limits::DEFAULT.max_depth;
    check(
        "json depth limit",
        300,
        |rng| {
            // Open a random mix of arrays and objects, then close them.
            let depth = rng.range(0..(2 * limit as u32 + 10)) as usize;
            let kinds: Vec<bool> = (0..depth).map(|_| rng.next_u32() & 1 == 0).collect();
            let open: String = kinds
                .iter()
                .map(|&array| if array { "[" } else { "{\"k\":" })
                .collect();
            let close: String = kinds
                .iter()
                .rev()
                .map(|&array| if array { "]" } else { "}" })
                .collect();
            (depth, format!("{open}null{close}"))
        },
        |(depth, text)| {
            for dialect in DIALECTS {
                match parse(text, dialect) {
                    Ok(_) if *depth <= limit => {}
                    Err(CodecError::Json {
                        kind: ErrorKind::TooDeep { limit: reported },
                        ..
                    }) if *depth > limit && reported == limit => {}
                    other => return Err(format!("depth {depth} gave {other:?}")),
                }
            }
            Ok(())
        },
    );
}
