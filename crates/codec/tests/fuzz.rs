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

//! The fuzz targets of `tidepool-codec`: `json` and `toml`, one test each,
//! named after the target (ADR-0027, `docs/04-quality.md` → "Fuzz
//! smoke").
//!
//! A plain `cargo test` runs each target's saved crashes
//! (`fuzz/crashes/<target>/`) and seeds unchanged: the hand-made ones in
//! `fuzz/corpus/<target>/`, and test data already in this crate, read in
//! place. `cargo xtask fuzz-smoke` runs them with mutated inputs too, in an
//! optimized profile with overflow checks.
//!
//! Each target parses with small [`Limits`], and among its seeds are
//! documents that sit exactly at each limit (`edge-*`), so that one
//! mutation takes an input over it and into the check. Beyond "no panic",
//! each checks what the module docs and [`CodecError`] promise:
//!
//! - an error's offset is in the input, at a character boundary when the
//!   input is UTF-8, and its line and column are what that offset is;
//! - an `InvalidUtf8` error is where the UTF-8 stops being valid;
//! - `json`: a strict JSON document is JSONC too, with the same value; JSONC
//!   accepts more only for a comment or a trailing comma; and a value
//!   written back, compact or pretty, reads back equal;
//! - `toml`: every key's and value's span is a non-empty range of the
//!   input on character boundaries, and holds what its kind says: a quoted
//!   key or string its quotes, a bare key its name, a boolean its word, an
//!   array or table its brackets or braces. A UTF-8 input that parses is
//!   also read as a `toml::Document`, which must hold the same text and
//!   tree, and four edits are made on copies of it, two of them in a table
//!   the input picks (see [`check_document`]): each is made, changing
//!   exactly the bytes it should, or refused for a limit (or as not
//!   replaceable, where the rules refuse it), and never as unverified.
//!
//! Input that is not UTF-8 stops at the first check, so it is also parsed
//! as `String::from_utf8_lossy` gives it, through the `&str` entry points,
//! with the same checks: the parser itself then sees every input.

use tidepool_codec::json::{self, Dialect};
use tidepool_codec::toml::{
    self, Document, EditErrorKind, Span, Step, Table, TableKind, ValueKind,
};
use tidepool_codec::{CodecError, Limits};
use tidepool_core::testing::fuzz::{Outcome, Target};

/// The repository root, where `fuzz/` is.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The limits both targets parse with. Each is at or below what a mutation
/// can reach from the `edge-*` seeds, which sit exactly at it, and above
/// every other seed: the largest is `tests/json/cargo_metadata.json`, at
/// 3,918 bytes, and every toml-test case is under 1.2 KiB. Built from the
/// defaults, so that a field added later keeps its default.
const LIMITS: Limits = {
    let mut limits = Limits::DEFAULT;
    limits.max_input_bytes = 4 * 1024;
    limits.max_depth = 32;
    limits.max_string_bytes = 1024;
    limits.max_number_bytes = 64;
    limits.max_entries = 256;
    limits
};

/// The limits a written-back JSON document, or text that lossy decoding
/// made longer, is read with: those of [`LIMITS`] that bound the value, but
/// not the text's length or a number's, since output may be longer than its
/// input (`1e3` is written `1000.0`; see the `json` module docs, "Writing").
const WRITTEN: Limits = {
    let mut limits = LIMITS;
    limits.max_input_bytes = Limits::DEFAULT.max_input_bytes;
    limits.max_number_bytes = Limits::DEFAULT.max_number_bytes;
    limits
};

/// The cap on the rise of the written peak: the targets' peak rose by
/// about 0.4 MiB over a 60 s run, and `toml`'s by about 0.7 MiB once it
/// edited documents too (M1.5).
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

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

/// The line and column `CodecError` gives for `offset` in `input`: lines
/// end at each LF, columns count characters from 1, and a leading
/// byte-order mark is not counted. `None` when the text before `offset` is
/// not UTF-8.
fn position(input: &[u8], offset: usize) -> Option<(usize, usize)> {
    let before = std::str::from_utf8(input.get(..offset)?).ok()?;
    let line = 1 + before.bytes().filter(|&byte| byte == b'\n').count();
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    let mut column = 1 + before[start..].chars().count();
    if start == 0 && before.starts_with('\u{FEFF}') {
        column -= 1;
    }
    Some((line, column))
}

/// Checks an error's place: in the input, at a character boundary of a
/// UTF-8 input, with the line and column of that offset.
fn check_place(input: &[u8], offset: usize, line: usize, column: usize, format: &str) {
    assert!(
        offset <= input.len(),
        "a {format} error at offset {offset} of {} bytes",
        input.len()
    );
    if let Ok(text) = std::str::from_utf8(input) {
        assert!(
            text.is_char_boundary(offset),
            "a {format} error at offset {offset}, inside a character"
        );
    }
    if let Some(expected) = position(input, offset) {
        assert!(
            expected == (line, column),
            "a {format} error at offset {offset} says {line}:{column}, but that is {}:{}",
            expected.0,
            expected.1
        );
    }
}

/// Checks that an `InvalidUtf8` error is where the input stops being UTF-8.
fn check_utf8_offset(input: &[u8], offset: usize, format: &str) {
    let valid = std::str::from_utf8(input)
        .err()
        .map(|error| error.valid_up_to());
    assert!(
        valid == Some(offset),
        "a {format} InvalidUtf8 error at {offset}, but the UTF-8 is valid up to {valid:?}"
    );
}

/// JSON structure, numbers on the edges of `u64`, `i64` and `f64`, every
/// escape, lone and paired surrogates, JSONC's comments and trailing
/// commas, a byte-order mark and multi-byte characters.
const JSON_TOKENS: &[&[u8]] = &[
    b"{",
    b"}",
    b"[",
    b"]",
    b":",
    b",",
    b"\"",
    b"\"\":",
    b"true",
    b"false",
    b"null",
    b"-",
    b"0",
    b"-0",
    b"0.5",
    b"1e308",
    b"1e309",
    b"-1E-400",
    b"e+",
    b"18446744073709551615",
    b"18446744073709551616",
    b"-9223372036854775808",
    b"-9223372036854775809",
    b"\\\"",
    b"\\\\",
    b"\\/",
    b"\\b",
    b"\\f",
    b"\\n",
    b"\\r",
    b"\\t",
    b"\\u0000",
    b"\\u001f",
    b"\\uD800",
    b"\\uDC00",
    b"\\uD83D\\uDE00",
    b"\\uFFFF",
    b"//",
    b"/*",
    b"*/",
    b",]",
    b",}",
    b"\r\n",
    b"\xef\xbb\xbf",
    b"\xc3\xa9",
    b"\xf0\x9f\x98\x80",
    b"\xed\xa0\x80",
];

/// `json`: [`json::parse_bytes`] in both dialects, and for input that is not
/// UTF-8, [`json::parse`] of its lossy decoding too.
fn parse_json(input: &[u8]) {
    let strict = json::parse_bytes(input, Dialect::Json, LIMITS);
    let jsonc = json::parse_bytes(input, Dialect::Jsonc, LIMITS);
    check_json(input, &strict, &jsonc, true);
    if std::str::from_utf8(input).is_err() {
        let text = String::from_utf8_lossy(input);
        let strict = json::parse(&text, Dialect::Json, WRITTEN);
        let jsonc = json::parse(&text, Dialect::Jsonc, WRITTEN);
        check_json(text.as_bytes(), &strict, &jsonc, false);
    }
}

/// The checks of [`parse_json`] on the results for `input`; `bytes` when
/// they came from `parse_bytes`, which alone reports `InvalidUtf8`.
fn check_json(
    input: &[u8],
    strict: &Result<json::Value, CodecError>,
    jsonc: &Result<json::Value, CodecError>,
    bytes: bool,
) {
    for result in [strict, jsonc] {
        match result {
            Ok(_) => {}
            Err(CodecError::Json {
                offset,
                line,
                column,
                kind,
            }) => {
                check_place(input, *offset, *line, *column, "JSON");
                if matches!(kind, json::ErrorKind::InvalidUtf8) {
                    assert!(bytes, "text that is UTF-8 reported as not UTF-8");
                    check_utf8_offset(input, *offset, "JSON");
                }
            }
            Err(other) => panic!("a JSON parse failed with a {other:?}"),
        }
    }
    match (strict, jsonc) {
        (Ok(strict), Ok(jsonc)) => assert!(jsonc == strict, "JSONC reads strict JSON differently"),
        (Ok(_), Err(error)) => panic!("JSONC rejects strict JSON: {error}"),
        (Err(CodecError::Json { kind, .. }), Ok(_)) => assert!(
            matches!(
                kind,
                json::ErrorKind::CommentNotAllowed | json::ErrorKind::TrailingCommaNotAllowed
            ),
            "JSONC accepts what strict JSON rejects for {kind:?}, not a comment or a trailing \
             comma"
        ),
        _ => {}
    }
    if let Ok(value) = jsonc {
        for (form, text) in [
            ("compact", json::to_string_compact(value)),
            ("pretty", json::to_string_pretty(value)),
        ] {
            match json::parse(&text, Dialect::Json, WRITTEN) {
                Ok(back) => assert!(back == *value, "the {form} output reads back differently"),
                Err(error) => panic!("the {form} output does not read back: {error}"),
            }
        }
    }
}

#[test]
fn json() {
    let outcome = Target::new(ROOT, "json", parse_json)
        .seeds_in("crates/codec/tests/json", &["json", "jsonc"])
        .dictionary(JSON_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

/// TOML structure, every kind of string and escape (TOML 1.1's `\e` and
/// `\x` too), integers in each base and on the edges of `i64`, the special
/// floats, each kind of date and time, comments, line ends, a byte-order
/// mark and multi-byte characters.
const TOML_TOKENS: &[&[u8]] = &[
    b"[",
    b"]",
    b"[[",
    b"]]",
    b"{",
    b"}",
    b"=",
    b" = ",
    b".",
    b",",
    b"\"",
    b"'",
    b"\"\"\"",
    b"'''",
    b"#",
    b"\n",
    b"\r\n",
    b"\r",
    b"\\\n",
    b"inf",
    b"+inf",
    b"-inf",
    b"nan",
    b"+nan",
    b"-nan",
    b"true",
    b"false",
    b"0x",
    b"0o",
    b"0b",
    b"_",
    b"1e10",
    b"-0.0",
    b"9223372036854775807",
    b"9223372036854775808",
    b"-9223372036854775808",
    b"1979-05-27",
    b"07:32:00",
    b"07:32",
    b".999999",
    b"1979-05-27T07:32:00Z",
    b"1979-05-27 07:32:00.999999-07:00",
    b"2000-02-29",
    b"T",
    b"Z",
    b"+00:00",
    b"\\b",
    b"\\t",
    b"\\n",
    b"\\f",
    b"\\r",
    b"\\e",
    b"\\\"",
    b"\\\\",
    b"\\x41",
    b"\\u00E9",
    b"\\U0001F600",
    b"\\uD800",
    b"\xef\xbb\xbf",
    b"\xc3\xa9",
    b"\x7f",
];

/// `toml`: [`toml::parse_bytes`], and for input that is not UTF-8,
/// [`toml::parse`] of its lossy decoding too. A document that parses is
/// edited too (see [`check_document`]).
fn parse_toml(input: &[u8]) {
    let result = toml::parse_bytes(input, LIMITS);
    check_toml(input, &result, true);
    if let Ok(root) = &result {
        check_document(input, root);
    }
    if std::str::from_utf8(input).is_err() {
        let text = String::from_utf8_lossy(input);
        check_toml(text.as_bytes(), &toml::parse(&text, WRITTEN), false);
    }
}

/// The checks of [`parse_toml`] on the result for `input`.
fn check_toml(input: &[u8], result: &Result<Table, CodecError>, bytes: bool) {
    match result {
        Ok(root) => {
            let text = std::str::from_utf8(input).expect("a document that parsed is UTF-8");
            check_table(text, root);
        }
        Err(CodecError::Toml {
            offset,
            line,
            column,
            kind,
        }) => {
            check_place(input, *offset, *line, *column, "TOML");
            if matches!(kind, toml::ErrorKind::InvalidUtf8) {
                assert!(bytes, "text that is UTF-8 reported as not UTF-8");
                check_utf8_offset(input, *offset, "TOML");
            }
        }
        Err(other) => panic!("a TOML parse failed with a {other:?}"),
    }
}

/// The text of `span`, which must be a non-empty range of characters of
/// `text`.
fn spanned<'a>(text: &'a str, span: Span, what: &str) -> &'a str {
    assert!(
        span.start < span.end
            && span.end <= text.len()
            && text.is_char_boundary(span.start)
            && text.is_char_boundary(span.end),
        "the span of {what}, {}..{}, is not a non-empty range of characters in the {} bytes",
        span.start,
        span.end,
        text.len()
    );
    &text[span.range()]
}

/// Whether `text` starts and ends with the same quote, `"` or `'`.
fn quoted(text: &str) -> bool {
    text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
}

/// Checks every span in `table`, and in the tables and arrays under it.
/// The depth is at most [`LIMITS`]' `max_depth`, so this recursion is
/// bounded.
fn check_table(text: &str, table: &Table) {
    for (key, value) in table.entries() {
        let written = spanned(text, key.span, "a key");
        if written.starts_with(['"', '\'']) {
            assert!(quoted(written), "a quoted key's span is not quoted");
        } else {
            assert!(
                written == key.name,
                "a bare key's span holds other text than its name"
            );
        }
        check_value(text, value);
    }
}

/// Checks a value's span, and the spans under it.
fn check_value(text: &str, value: &toml::Value) {
    let written = spanned(text, value.span, "a value");
    match &value.kind {
        ValueKind::String(_) => assert!(quoted(written), "a string's span is not quoted"),
        ValueKind::Boolean(boolean) => assert!(
            written == if *boolean { "true" } else { "false" },
            "a boolean's span is not its word"
        ),
        ValueKind::Array(array) => {
            let (open, close) = if array.is_array_of_tables() {
                ("[[", "]]")
            } else {
                ("[", "]")
            };
            assert!(
                written.starts_with(open) && written.ends_with(close),
                "an array's span is not in {open} {close}"
            );
            for item in array.items() {
                check_value(text, item);
            }
        }
        ValueKind::Table(table) => {
            let ends = match table.kind() {
                TableKind::Header => Some(("[", "]")),
                TableKind::ArrayHeader => Some(("[[", "]]")),
                TableKind::Inline => Some(("{", "}")),
                TableKind::Root | TableKind::Implicit | TableKind::Dotted => None,
            };
            if let Some((open, close)) = ends {
                assert!(
                    written.starts_with(open) && written.ends_with(close),
                    "a {:?} table's span is not in {open} {close}",
                    table.kind()
                );
                if table.kind() == TableKind::Header {
                    assert!(
                        !written.starts_with("[["),
                        "a header table's span is in [[ ]]"
                    );
                }
            }
            check_table(text, table);
        }
        ValueKind::Integer(_) | ValueKind::Float(_) | ValueKind::Datetime(_) => {}
    }
}

/// A step of a path, owned, for the paths a walk of a tree lists.
#[derive(Clone, Debug)]
enum Owned {
    Key(String),
    Index(usize),
}

fn steps(path: &[Owned]) -> Vec<Step<'_>> {
    path.iter()
        .map(|step| match step {
            Owned::Key(name) => Step::Key(name),
            Owned::Index(index) => Step::Index(*index),
        })
        .collect()
}

/// A value and its path.
type Found<'t> = (Vec<Owned>, &'t toml::Value);

/// Every value under `root` with its path, and the path of every table
/// (the root's first, then in the order a walk with its own list meets
/// them).
fn walk(root: &Table) -> (Vec<Found<'_>>, Vec<Vec<Owned>>) {
    let mut values = Vec::new();
    let mut tables = vec![Vec::new()];
    let mut work: Vec<Found<'_>> = root
        .entries()
        .iter()
        .map(|(key, value)| (vec![Owned::Key(key.name.clone())], value))
        .collect();
    while let Some((path, value)) = work.pop() {
        let mut inner = |step: Owned, value| {
            let mut path = path.clone();
            path.push(step);
            work.push((path, value));
        };
        match &value.kind {
            ValueKind::Table(table) => {
                tables.push(path.clone());
                for (key, value) in table.entries() {
                    inner(Owned::Key(key.name.clone()), value);
                }
            }
            ValueKind::Array(array) => {
                for (index, item) in array.items().iter().enumerate() {
                    inner(Owned::Index(index), item);
                }
            }
            _ => {}
        }
        values.push((path, value));
    }
    (values, tables)
}

/// Whether a value can be replaced whole: a scalar, an array written as a
/// value or an inline table.
fn replaceable(value: &toml::Value) -> bool {
    match &value.kind {
        ValueKind::Table(table) => table.kind() == TableKind::Inline,
        ValueKind::Array(array) => !array.is_array_of_tables(),
        _ => true,
    }
}

/// Whether two values hold the same data, ignoring spans and how tables
/// and arrays were written; floats by their bits, and any two NaNs of one
/// sign alike. The depth is bounded by [`LIMITS`].
fn same(a: &ValueKind, b: &ValueKind) -> bool {
    match (a, b) {
        (ValueKind::Table(a), ValueKind::Table(b)) => {
            a.len() == b.len()
                && a.entries()
                    .iter()
                    .zip(b.entries())
                    .all(|((a_key, a), (b_key, b))| {
                        a_key.name == b_key.name && same(&a.kind, &b.kind)
                    })
        }
        (ValueKind::Array(a), ValueKind::Array(b)) => {
            a.len() == b.len()
                && a.items()
                    .iter()
                    .zip(b.items())
                    .all(|(a, b)| same(&a.kind, &b.kind))
        }
        (ValueKind::Float(a), ValueKind::Float(b)) if a.is_nan() && b.is_nan() => {
            a.is_sign_negative() == b.is_sign_negative()
        }
        (ValueKind::Float(a), ValueKind::Float(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

/// The value at `path` in `root`, found by this test's own walk.
fn value_at<'t>(root: &'t Table, path: &[Owned]) -> Option<&'t ValueKind> {
    let (Owned::Key(first), rest) = path.split_first()? else {
        return None;
    };
    let mut here = &root.get(first)?.kind;
    for step in rest {
        here = match (step, here) {
            (Owned::Key(name), ValueKind::Table(table)) => &table.get(name)?.kind,
            (Owned::Index(index), ValueKind::Array(array)) => &array.get(*index)?.kind,
            _ => return None,
        };
    }
    Some(here)
}

/// Sets `path` to `value` on a copy of `document`. The edit must be made,
/// and then `text` must be what `expected` says and hold the value when
/// the parser reads it again; or be refused for a limit or (if `may_refuse`
/// allows) as not replaceable, and change nothing. Anything else, an
/// `Unverified` refusal above all, is a finding.
fn edit_document(
    document: &Document,
    path: &[Owned],
    value: &ValueKind,
    expected: &dyn Fn(&str) -> bool,
    may_refuse: bool,
) {
    let mut edited = document.clone();
    match edited.set(&steps(path), value) {
        Ok(()) => {
            let text = edited.as_str();
            assert!(expected(text), "an edit wrote other text than it should");
            let root = toml::parse(text, LIMITS).expect("an edited document parses");
            assert!(
                value_at(&root, path).is_some_and(|found| same(found, value)),
                "an edited document does not hold the value set"
            );
        }
        Err(CodecError::TomlEdit { kind, .. }) => {
            let allowed = match kind {
                EditErrorKind::TooLarge { .. } | EditErrorKind::Limit(_) => true,
                EditErrorKind::NotReplaceable { .. } => may_refuse,
                _ => false,
            };
            assert!(allowed, "an edit was refused: {kind}");
            assert!(
                edited.as_str() == document.as_str(),
                "a refused edit changed the text"
            );
        }
        Err(other) => panic!("an edit failed with a {other:?}"),
    }
}

/// FNV-1a, 64 bits, of `bytes`: a hash that picks a table from the input
/// alone, so that a replay picks the same one.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The path of the table, of `tables`, that `input` picks for edits 2 and
/// 3 of [`check_document`]. The root is always one, so there is one to
/// pick.
fn picked(input: &[u8], tables: &[Vec<Owned>]) -> Vec<Owned> {
    let pick = usize::try_from(fnv1a(input) % tables.len() as u64).unwrap_or(0);
    tables.get(pick).cloned().unwrap_or_default()
}

/// The document model on a UTF-8 input that parses to `root`: it reads the
/// same text and tree, and takes four edits, each on a copy:
///
/// 1. the leaf that starts last becomes a string that needs every kind of
///    escape and quote, written exactly in its span's place;
/// 2. `fz = 7` goes into a table that the input picks (by [`fnv1a`], from
///    every table of every kind, the root included): a pure insertion, the
///    replacement of a `fz` there, or, for a `fz` that is a table, a
///    refusal as not replaceable;
/// 3. a new table and a key in it, `fz-c….fz-d = 1`, go into that same
///    table: a pure insertion;
/// 4. a new table and a key in it, `fz-a….fz-b = true`, go into the root:
///    a pure insertion.
///
/// Between them, 2 and 3 reach every rule of where a value is added: a
/// section's own key line, the root point, a dotted or inline key, and a
/// header block after any table's subtree, an element of an array of
/// tables' before the next element.
fn check_document(input: &[u8], root: &Table) {
    let document = Document::parse_bytes(input, LIMITS).expect("a document reads what parse does");
    let old = document.as_str();
    assert!(
        old.as_bytes() == input,
        "a document's text is not its input"
    );
    assert!(
        format!("{:?}", document.root()) == format!("{root:?}"),
        "a document's tree is not the parser's"
    );
    let (values, tables) = walk(document.root());

    let string = ValueKind::String("\u{1b}\t\"'\\é".into());
    let written = "\"\\u001B\\t\\\"'\\\\é\"";
    if let Some((path, leaf)) = values
        .iter()
        .filter(|(_, value)| replaceable(value))
        .max_by_key(|(_, value)| value.span.start)
    {
        let wanted = if same(&leaf.kind, &string) {
            old.to_string()
        } else {
            format!(
                "{}{written}{}",
                &old[..leaf.span.start],
                &old[leaf.span.end..]
            )
        };
        edit_document(&document, path, &string, &|text| text == wanted, false);
    }

    let table_path = picked(input, &tables);
    let table = if table_path.is_empty() {
        document.root()
    } else {
        document
            .get(&steps(&table_path))
            .and_then(toml::Value::as_table)
            .expect("a table the walk found")
    };
    let seven = ValueKind::Integer(7);
    let mut path = table_path.clone();
    path.push(Owned::Key("fz".into()));
    match document.get(&steps(&path)) {
        None => edit_document(
            &document,
            &path,
            &seven,
            &|text| is_insertion(old, text),
            false,
        ),
        Some(found) if replaceable(found) => {
            let span = found.span;
            let wanted = if same(&found.kind, &seven) {
                old.to_string()
            } else {
                format!("{}7{}", &old[..span.start], &old[span.end..])
            };
            edit_document(&document, &path, &seven, &|text| text == wanted, false);
        }
        Some(_) => edit_document(&document, &path, &seven, &|_| false, true),
    }

    let mut first = "fz-c".to_string();
    while table.contains_key(&first) {
        first.push('c');
    }
    let mut path = table_path;
    path.extend([Owned::Key(first), Owned::Key("fz-d".into())]);
    let one = ValueKind::Integer(1);
    edit_document(
        &document,
        &path,
        &one,
        &|text| is_insertion(old, text),
        false,
    );

    let mut first = "fz-a".to_string();
    while document.root().contains_key(&first) {
        first.push('a');
    }
    let path = [Owned::Key(first), Owned::Key("fz-b".into())];
    let yes = ValueKind::Boolean(true);
    edit_document(
        &document,
        &path,
        &yes,
        &|text| is_insertion(old, text),
        false,
    );
}

/// Whether `new` is `old` with text inserted at one place.
fn is_insertion(old: &str, new: &str) -> bool {
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    new.len() > old.len() && prefix + suffix >= old.len()
}

#[test]
fn toml() {
    // toml-test's cases, valid and invalid, read where they are vendored
    // (ADR-0026); their notice stays beside them.
    let outcome = Target::new(ROOT, "toml", parse_toml)
        .seeds_in("crates/codec/tests/toml-test", &["toml"])
        .dictionary(TOML_TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

/// The `edge-*` seeds sit exactly at each limit: they parse with
/// [`LIMITS`], and fail with that limit one lower.
#[test]
fn the_edge_seeds_sit_exactly_at_each_limit() {
    use json::ErrorKind as J;
    use toml::ErrorKind as T;
    let seed = |path: &str| {
        std::fs::read(format!("{ROOT}/fuzz/corpus/{path}"))
            .unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    let lower = |field: &str| {
        let mut limits = LIMITS;
        match field {
            "input" => limits.max_input_bytes -= 1,
            "string" => limits.max_string_bytes -= 1,
            "entries" => limits.max_entries -= 1,
            "number" => limits.max_number_bytes -= 1,
            "depth" => limits.max_depth -= 1,
            _ => panic!("{field}"),
        }
        limits
    };
    for (file, field) in [
        ("edge-input.jsonc", "input"),
        ("edge-string.json", "string"),
        ("edge-key.json", "string"),
        ("edge-array.json", "entries"),
        ("edge-object.json", "entries"),
        ("edge-number.json", "number"),
        ("edge-depth.json", "depth"),
    ] {
        let input = seed(&format!("json/{file}"));
        assert!(input.len() <= LIMITS.max_input_bytes);
        // The input limit's seed ends in a comment, so that it can end in
        // multi-byte characters.
        let dialect = if file.ends_with(".jsonc") {
            Dialect::Jsonc
        } else {
            Dialect::Json
        };
        assert!(json::parse_bytes(&input, dialect, LIMITS).is_ok(), "{file}");
        let error = json::parse_bytes(&input, dialect, lower(field));
        let Err(CodecError::Json { kind, .. }) = error else {
            panic!("{file}: {error:?}");
        };
        let expected = match field {
            "input" => matches!(kind, J::InputTooLarge { .. }),
            "string" => matches!(kind, J::StringTooLong { .. }),
            "entries" => matches!(kind, J::TooManyEntries { .. }),
            "number" => matches!(kind, J::NumberTooLong { .. }),
            _ => matches!(kind, J::TooDeep { .. }),
        };
        assert!(expected, "{file}: {kind:?}");
    }
    for (name, field) in [
        ("input", "input"),
        ("string", "string"),
        ("key", "string"),
        ("array", "entries"),
        ("table", "entries"),
        ("number", "number"),
        ("depth", "depth"),
    ] {
        let input = seed(&format!("toml/edge-{name}.toml"));
        assert!(input.len() <= LIMITS.max_input_bytes);
        assert!(
            toml::parse_bytes(&input, LIMITS).is_ok(),
            "edge-{name}.toml"
        );
        let error = toml::parse_bytes(&input, lower(field));
        let Err(CodecError::Toml { kind, .. }) = error else {
            panic!("edge-{name}.toml: {error:?}");
        };
        let expected = match field {
            "input" => matches!(kind, T::InputTooLarge { .. }),
            "string" => matches!(kind, T::StringTooLong { .. }),
            "entries" => matches!(kind, T::TooManyEntries { .. }),
            "number" => matches!(kind, T::NumberTooLong { .. }),
            _ => matches!(kind, T::TooDeep { .. }),
        };
        assert!(expected, "edge-{name}.toml: {kind:?}");
    }
    // The input limit's seeds are exactly at it, and end in multi-byte
    // characters, so that the character that does not fit is one.
    for file in ["json/edge-input.jsonc", "toml/edge-input.toml"] {
        let input = seed(file);
        assert_eq!(input.len(), LIMITS.max_input_bytes, "{file}");
        assert!(input.ends_with("é".as_bytes()), "{file}");
    }
    // And every seed fits the input limit: the hand-made ones, and those
    // read in place (the files with the extensions the targets take).
    let mut dirs = vec![
        std::path::PathBuf::from(format!("{ROOT}/fuzz/corpus/json")),
        std::path::PathBuf::from(format!("{ROOT}/fuzz/corpus/toml")),
        std::path::PathBuf::from(format!("{ROOT}/crates/codec/tests/json")),
        std::path::PathBuf::from(format!("{ROOT}/crates/codec/tests/toml-test")),
    ];
    let mut checked = 0;
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            let seed = entry.path().extension().is_some_and(|extension| {
                ["json", "jsonc", "toml"].contains(&extension.to_str().unwrap_or(""))
            });
            if metadata.is_dir() {
                dirs.push(entry.path());
            } else if seed || dir.starts_with(format!("{ROOT}/fuzz")) {
                assert!(
                    metadata.len() <= LIMITS.max_input_bytes as u64,
                    "{} is over the input limit",
                    entry.path().display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 700, "{checked}");
}

/// The hand-made seeds meant to be valid parse, so that mutations start
/// from documents that reach deep into the parser; the others fail.
#[test]
fn the_hand_made_seeds_parse_as_they_are_meant_to() {
    let read = |path: &str| std::fs::read(format!("{ROOT}/fuzz/corpus/{path}")).unwrap();
    for (file, valid) in [
        ("empty-object.json", true),
        ("empty-array.json", true),
        ("scalars.json", true),
        ("integers.json", true),
        ("escapes.json", true),
        ("nested.json", true),
        ("repeated-keys.json", true),
        ("bom.json", true),
        ("whitespace.json", true),
        ("top-level-string.json", true),
        ("unterminated-string.json", false),
        ("lone-surrogates.json", false),
        ("bad-numbers.json", false),
    ] {
        let result = json::parse_bytes(&read(&format!("json/{file}")), Dialect::Json, LIMITS);
        assert_eq!(result.is_ok(), valid, "{file}: {result:?}");
    }
    for (file, valid) in [
        ("comments.jsonc", true),
        ("unterminated-comment.jsonc", false),
    ] {
        let result = json::parse_bytes(&read(&format!("json/{file}")), Dialect::Jsonc, LIMITS);
        assert_eq!(result.is_ok(), valid, "{file}: {result:?}");
    }
    for (file, valid) in [
        ("manifest.toml", true),
        ("lockfile.toml", true),
        ("settings.toml", true),
        ("strings.toml", true),
        ("numbers.toml", true),
        ("dates.toml", true),
        ("tables.toml", true),
        ("bom-crlf.toml", true),
        ("document-shapes.toml", true),
        ("document-inline-1.1.toml", true),
        ("document-crlf-bom-nofinal.toml", true),
        ("document-fz.toml", true),
        ("duplicate-key.toml", false),
        ("unterminated.toml", false),
        ("redefined.toml", false),
    ] {
        let result = toml::parse_bytes(&read(&format!("toml/{file}")), LIMITS);
        assert_eq!(result.is_ok(), valid, "{file}: {result:?}");
    }
}

/// `document-fz.toml` is worded so that edit 2 of [`check_document`] picks
/// a table whose `fz` is a table made by dotted keys, which a document
/// refuses to replace whole: so every replay takes that path too, and the
/// edits that follow meet names already taken.
#[test]
fn the_fz_seed_makes_edit_2_meet_a_fz_that_is_a_table() {
    let input = std::fs::read(format!("{ROOT}/fuzz/corpus/toml/document-fz.toml")).unwrap();
    let document = Document::parse_bytes(&input, LIMITS).expect("parses");
    let (_, tables) = walk(document.root());
    let mut path = picked(&input, &tables);
    path.push(Owned::Key("fz".into()));
    let fz = document.get(&steps(&path)).expect("a `fz` there");
    assert!(!replaceable(fz), "{path:?}");
    let refused = document.clone().set(&steps(&path), &ValueKind::Integer(7));
    assert!(
        matches!(
            refused,
            Err(CodecError::TomlEdit {
                kind: EditErrorKind::NotReplaceable {
                    found: "a table made by dotted keys"
                },
                ..
            })
        ),
        "{refused:?}"
    );
    path.pop();
    path.push(Owned::Key("fz-c".into()));
    assert!(document.get(&steps(&path)).is_some(), "fz-c is taken");
    assert!(document.root().contains_key("fz-a"), "fz-a is taken");
}

#[test]
fn error_places_are_recomputed_as_the_docs_say() {
    assert_eq!(position(b"abc", 0), Some((1, 1)));
    assert_eq!(position(b"ab\ncd", 4), Some((2, 2)));
    assert_eq!(position(b"a\r\nb", 3), Some((2, 1)));
    assert_eq!(position("é\nxé".as_bytes(), 6), Some((2, 3)));
    // A leading byte-order mark is not a column.
    assert_eq!(position(b"\xef\xbb\xbfab", 4), Some((1, 2)));
    assert_eq!(position(b"\xffab", 2), None);
    // A parse error's place agrees.
    let input = b"{\n  \"a\": tru\n}";
    let Err(CodecError::Json {
        offset,
        line,
        column,
        ..
    }) = json::parse_bytes(input, Dialect::Json, LIMITS)
    else {
        panic!("not JSON");
    };
    check_place(input, offset, line, column, "JSON");
}
