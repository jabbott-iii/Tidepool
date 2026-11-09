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

//! Unit tests for the document model's parts: the line scans, the splices
//! it plans for every valid toml-test case, and its refusal of a splice
//! that is wrong. The public API's behaviour is tested in
//! `tests/toml_document.rs`, and over every toml-test case in
//! `tests/toml.rs`.

use std::collections::BTreeSet;
use std::path::Path;

use super::place::{content_start, line_end, newline, root_point};
use super::*;
use crate::toml::{Array, Date, Datetime, Offset, Time};

/// The text of every valid case on toml-test's TOML 1.1 list.
fn valid_cases() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/toml-test");
    let list = std::fs::read_to_string(root.join("files-toml-1.1.0")).expect("the list of cases");
    let cases: Vec<(String, String)> = list
        .lines()
        .filter(|line| line.starts_with("valid/") && line.ends_with(".toml"))
        .map(|name| {
            let bytes = std::fs::read(root.join(name)).expect("a case");
            let text = String::from_utf8(bytes).expect("a valid case is UTF-8");
            (name.to_string(), text)
        })
        .collect();
    assert_eq!(cases.len(), 214);
    cases
}

/// A step of a path, owned.
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
type Found<'t> = (Vec<Owned>, &'t Value);

/// Every value in `root` and its path, and the paths of every table (the
/// root's first).
fn walk(root: &Table) -> (Vec<Found<'_>>, Vec<Vec<Owned>>) {
    let mut values = Vec::new();
    let mut tables = vec![Vec::new()];
    let mut work: Vec<Found<'_>> = root
        .entries()
        .iter()
        .map(|(key, value)| (vec![Owned::Key(key.name.clone())], value))
        .collect();
    while let Some((path, value)) = work.pop() {
        match &value.kind {
            ValueKind::Table(table) => {
                tables.push(path.clone());
                for (key, inner) in table.entries() {
                    let mut inner_path = path.clone();
                    inner_path.push(Owned::Key(key.name.clone()));
                    work.push((inner_path, inner));
                }
            }
            ValueKind::Array(array) => {
                for (index, item) in array.items().iter().enumerate() {
                    let mut inner_path = path.clone();
                    inner_path.push(Owned::Index(index));
                    work.push((inner_path, item));
                }
            }
            _ => {}
        }
        values.push((path, value));
    }
    (values, tables)
}

fn replaceable(value: &Value) -> bool {
    not_replaceable(&value.kind).is_none()
}

/// A value that uses every part of the writer: strings that need each kind
/// of escape and quote, a time read without seconds, every special float,
/// each kind of date-time, and nested and empty arrays and tables.
fn rich_value() -> ValueKind {
    let date = Date {
        year: 1979,
        month: 5,
        day: 27,
    };
    let time = Time {
        hour: 7,
        minute: 32,
        second: 0,
        nanosecond: 0,
    };
    let scalars = [
        ValueKind::String("\u{1b}\u{0}\u{7f}\t\n\r\u{8}\u{c}\"'\\é".into()),
        ValueKind::String("C:\\Users\\#me".into()),
        ValueKind::String("it's # not a comment, ] }".into()),
        ValueKind::Integer(i64::MIN),
        ValueKind::Float(f64::NAN),
        ValueKind::Float(-f64::NAN),
        ValueKind::Float(f64::NEG_INFINITY),
        ValueKind::Float(-0.0),
        ValueKind::Float(1e300),
        ValueKind::Boolean(false),
        ValueKind::Datetime(Datetime::LocalTime(time)),
        ValueKind::Datetime(Datetime::LocalDatetime { date, time }),
        ValueKind::Datetime(Datetime::OffsetDatetime {
            date,
            time,
            offset: Offset::Minutes(-420),
        }),
        ValueKind::Datetime(Datetime::LocalDate(date)),
    ];
    let mut inner = Table::new_inline();
    inner.insert("a b", ValueKind::Array(Array::new(Vec::new())));
    inner.insert("t", ValueKind::Table(Table::new_inline()));
    let mut items: Vec<Value> = scalars.into_iter().map(Value::from).collect();
    items.push(Value::from(ValueKind::Table(inner.clone())));
    let mut table = Table::new_inline();
    table.insert("all", ValueKind::Array(Array::new(items)));
    table.insert("in", ValueKind::Table(inner));
    ValueKind::Table(table)
}

/// Checks that created text is TOML 1.0 (ADR-0031): outside strings, no
/// comment, no line break inside brackets or braces, and no comma before a
/// closing one; inside strings, only 1.0's escapes; and every time written
/// with its seconds.
fn check_toml_1_0(text: &str) -> Result<(), String> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0usize;
    let mut last = ' ';
    let mut outside = String::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        at += 1;
        match c {
            '#' => return Err("a comment".into()),
            '"' => {
                loop {
                    let Some(&c) = chars.get(at) else {
                        return Err("an unclosed string".into());
                    };
                    at += 1;
                    match c {
                        '"' => break,
                        '\\' => {
                            let escape = chars.get(at).copied().unwrap_or(' ');
                            at += 1;
                            match escape {
                                '"' | '\\' | 'b' | 't' | 'n' | 'f' | 'r' => {}
                                'u' => at += 4,
                                other => return Err(format!("the escape \\{other}")),
                            }
                        }
                        c if c <= '\u{1f}' || c == '\u{7f}' => {
                            return Err(format!("a raw {c:?} in a string"));
                        }
                        _ => {}
                    }
                }
                outside.push('"');
                last = '"';
                continue;
            }
            '\'' => {
                loop {
                    let Some(&c) = chars.get(at) else {
                        return Err("an unclosed literal string".into());
                    };
                    at += 1;
                    if c == '\'' {
                        break;
                    }
                    if c <= '\u{1f}' || c == '\u{7f}' {
                        return Err(format!("a raw {c:?} in a literal string"));
                    }
                }
                outside.push('\'');
                last = '\'';
                continue;
            }
            '[' | '{' => depth += 1,
            ']' | '}' => {
                if last == ',' {
                    return Err("a trailing comma".into());
                }
                depth = depth.saturating_sub(1);
            }
            '\n' | '\r' if depth > 0 => return Err("a line break inside brackets".into()),
            _ => {}
        }
        outside.push(c);
        if c != ' ' && c != '\t' {
            last = c;
        }
    }
    // A time is two digits, `:` and two digits, not right after a digit,
    // `:` or a sign (which an offset has), and must go on to its seconds.
    let bytes = outside.as_bytes();
    for start in 0..bytes.len() {
        let time = bytes.get(start..start + 5).is_some_and(|piece| {
            piece[0].is_ascii_digit()
                && piece[1].is_ascii_digit()
                && piece[2] == b':'
                && piece[3].is_ascii_digit()
                && piece[4].is_ascii_digit()
        });
        let before = start.checked_sub(1).and_then(|at| bytes.get(at));
        if time && !before.is_some_and(|&b| b.is_ascii_digit() || b"+-:".contains(&b)) {
            let seconds = bytes.get(start + 5..start + 8).is_some_and(|piece| {
                piece[0] == b':' && piece[1].is_ascii_digit() && piece[2].is_ascii_digit()
            });
            if !seconds {
                return Err("a time without seconds".into());
            }
        }
    }
    Ok(())
}

#[test]
fn the_toml_1_0_check_catches_each_form() {
    for bad in [
        "{ a = 1, }",
        "[1,\n2]",
        "{ a = 1 # c\n}",
        "\"\\e\"",
        "\"\\x41\"",
        "\"\\U0001F600\"",
        "k = 07:32",
        "k = 1979-05-27T07:32Z",
        "\"a\u{1b}\"",
    ] {
        assert!(check_toml_1_0(bad).is_err(), "{bad:?}");
    }
    for good in [
        "k = 07:32:00",
        "k = 1979-05-27T07:32:00-07:00",
        "[a.b]\nk = [1, 2]\n",
        "k = \"07:32 # [,]\"",
        "k = '#,]'",
    ] {
        assert_eq!(check_toml_1_0(good), Ok(()), "{good:?}");
    }
}

#[test]
fn line_scans_handle_every_line_ending() {
    // le: after spaces, a comment and the line break, or at the end.
    for (text, at, expected) in [
        ("a = 1\nb", 5, Some(6)),
        ("a = 1\r\nb", 5, Some(7)),
        ("a = 1  # c\r\nb", 5, Some(12)),
        ("a = 1 # c", 5, Some(9)),
        ("a = 1", 5, Some(5)),
        ("a = 1  ", 5, Some(7)),
        ("[t]\t#\n", 3, Some(6)),
        // Anything else is not what follows a value.
        ("a = 1 b\n", 5, None),
        ("a = 1\r", 5, None),
        ("a", 2, None),
    ] {
        assert_eq!(line_end(text, at), expected, "{text:?} at {at}");
    }
    assert_eq!(newline("a = 1\r\nb = 2\n"), "\r\n");
    assert_eq!(newline("a = 1\nb = 2\r\n"), "\n");
    assert_eq!(newline("\n"), "\n");
    assert_eq!(newline(""), "\n");
    assert_eq!(content_start("\u{feff}a"), 3);
    assert_eq!(content_start("a\u{feff}"), 0);

    // The root point: above the first header and its comments, never
    // before a byte-order mark; the end of the text with no header.
    for (text, expected) in [
        ("[a]\n", 0),
        ("# c\n[a]\n", 0),
        ("# c\n\n[a]\n", 5),
        ("# c\r\n\r\n  # d\r\n[a]\r\n", 7),
        ("\u{feff}# c\n[a]\n", 3),
        ("\u{feff}[a]", 3),
        ("\n\n[[a]]\n", 2),
        ("# c", 3),
        ("# c\n", 4),
        ("   ", 3),
        ("", 0),
        ("\u{feff}", 3),
    ] {
        let root = parse(text, Limits::DEFAULT).expect("parses");
        assert_eq!(
            root_point(text, content_start(text), &root),
            Some(expected),
            "{text:?}"
        );
    }
}

#[test]
fn created_text_is_toml_1_0_for_every_case() {
    let rich = rich_value();
    let mut planned = 0;
    for (name, text) in valid_cases() {
        let document = Document::parse(&text, Limits::DEFAULT).expect("a valid case");
        let (values, tables) = walk(document.root());
        let mut paths: Vec<Vec<Owned>> = values
            .iter()
            .filter(|(_, value)| replaceable(value))
            .map(|(path, _)| path.clone())
            .collect();
        for table in tables {
            let mut key = table.clone();
            key.push(Owned::Key("new key".into()));
            let mut chain = table;
            chain.push(Owned::Key("fresh-a".into()));
            chain.push(Owned::Key("fresh-b".into()));
            paths.push(key);
            paths.push(chain);
        }
        for path in paths {
            let splice = document
                .plan(&steps(&path), &rich)
                .unwrap_or_else(|error| panic!("{name} at {path:?}: {error:?}"))
                .unwrap_or_else(|| panic!("{name} at {path:?}: already holds it"));
            if let Err(problem) = check_toml_1_0(&splice.text) {
                panic!("{name} at {path:?}: {problem} in {:?}", splice.text);
            }
            planned += 1;
        }
    }
    assert!(planned > 2000, "{planned}");
}

#[test]
fn every_plan_is_one_splice_at_an_allowed_point() {
    let mut planned = 0;
    for (name, text) in valid_cases() {
        let document = Document::parse(&text, Limits::DEFAULT).expect("a valid case");
        let (values, tables) = walk(document.root());
        let ends: BTreeSet<usize> = values.iter().map(|(_, value)| value.span.end).collect();
        let c0 = content_start(&text);
        for (path, value) in &values {
            if !replaceable(value) {
                continue;
            }
            let splice = document
                .plan(&steps(path), &ValueKind::Boolean(true))
                .expect("planned");
            if let Some(splice) = splice {
                assert_eq!(splice.range, value.span.range(), "{name} at {path:?}");
                planned += 1;
            }
        }
        for mut path in tables {
            path.push(Owned::Key("new key".into()));
            let splice = document
                .plan(&steps(&path), &ValueKind::Integer(7))
                .expect("planned")
                .expect("an insertion");
            let at = splice.range.start;
            assert_eq!(splice.range.end, at, "{name} at {path:?}: not an insertion");
            let before = text.get(..at).expect("at a character boundary");
            let allowed = at == text.len()
                || at == c0
                || before.ends_with('\n')
                || before.ends_with('{')
                || ends.contains(&at);
            assert!(allowed, "{name} at {path:?}: inserts at {at}");
            planned += 1;
        }
    }
    assert!(planned > 1500, "{planned}");
}

#[test]
fn a_wrong_splice_is_refused_and_changes_nothing() {
    let insert = |at: usize, text: &str| Splice {
        range: at..at,
        text: text.to_string(),
    };
    let replace = |range: std::ops::Range<usize>, text: &str| Splice {
        range,
        text: text.to_string(),
    };
    // What `verify` and `commit` say, one for each check.
    const OUTSIDE: &str = "the splice is not in the text";
    const UNPARSED: &str = "the edited text does not parse";
    const NO_PATH: &str = "the edited text does not have the path";
    const BESIDE: &str = "an entry beside the path changed";
    const ARRAY: &str = "an array on the path changed its length or kind";
    const MADE: &str = "a table the edit made holds more than the path";
    const VALUE: &str = "the value at the path is not the value set";
    let seven = ValueKind::Integer(7);
    for (text, path, splice, reason) in [
        // A key placed in the wrong section.
        (
            "[a]\nx = 1\n[b]\ny = 2\n",
            vec![Step::Key("a"), Step::Key("k")],
            insert(20, "k = 7\n"),
            NO_PATH,
        ),
        // Inside a multi-line string.
        (
            "s = \"\"\"\nabc\n\"\"\"\n",
            vec![Step::Key("k")],
            insert(8, "k = 7\n"),
            NO_PATH,
        ),
        // In the wrong element of an array of tables.
        (
            "[[a]]\n[[a]]\n",
            vec![Step::Key("a"), Step::Index(0), Step::Key("k")],
            insert(12, "k = 7\n"),
            NO_PATH,
        ),
        // A dotted key in a header's table, above the header.
        (
            "[a]\nx = 1\n",
            vec![Step::Key("a"), Step::Key("k")],
            insert(0, "a.k = 7\n"),
            UNPARSED,
        ),
        // A dotted key in a header's table, under another header.
        (
            "[a]\nx = 1\n[b]\n",
            vec![Step::Key("a"), Step::Key("k")],
            insert(14, "a.k = 7\n"),
            NO_PATH,
        ),
        // The right place, the wrong value.
        (
            "[a]\nx = 1\n",
            vec![Step::Key("a"), Step::Key("k")],
            insert(10, "k = 8\n"),
            VALUE,
        ),
        // A replacement that takes a neighbour with it: the count of
        // entries beside the path differs.
        (
            "a = 1\nb = 2\n",
            vec![Step::Key("a")],
            replace(4..11, "7"),
            BESIDE,
        ),
        // One that changes a neighbour's value: the same names, so only
        // the comparison of what is beside the path finds it.
        (
            "a = 1\nb = 2\n",
            vec![Step::Key("a")],
            replace(4..11, "7\nb = 3"),
            BESIDE,
        ),
        // One that renames a neighbour: as many entries, another name.
        (
            "a = 1\nb = 2\n",
            vec![Step::Key("a")],
            replace(4..7, "7\nc"),
            BESIDE,
        ),
        // Another element of the array changed.
        (
            "a = [1, 2]\n",
            vec![Step::Key("a"), Step::Index(0)],
            replace(5..9, "7, 3"),
            BESIDE,
        ),
        // The array gained an element.
        (
            "a = [1, 2]\n",
            vec![Step::Key("a"), Step::Index(0)],
            replace(5..9, "7, 2, 3"),
            ARRAY,
        ),
        // A table the edit made holds another key.
        (
            "a = 1\n",
            vec![Step::Key("t"), Step::Key("k")],
            insert(6, "\n[t]\nk = 7\nz = 8\n"),
            MADE,
        ),
        // A splice outside the text, or not at a character boundary.
        (
            "a = 1\n",
            vec![Step::Key("k")],
            insert(99, "k = 7\n"),
            OUTSIDE,
        ),
        (
            "a = \"é\"\n",
            vec![Step::Key("k")],
            insert(6, "k = 7\n"),
            OUTSIDE,
        ),
        (
            "a = 1\n",
            vec![Step::Key("a")],
            // A range that ends before it starts.
            replace(std::ops::Range { start: 5, end: 4 }, "7"),
            OUTSIDE,
        ),
    ] {
        let mut document = Document::parse(text, Limits::DEFAULT).expect("parses");
        let root = format!("{:?}", document.root());
        let error = document.commit(&path, &seven, splice.clone());
        assert_eq!(
            error,
            Err((path.len(), EditErrorKind::Unverified { reason })),
            "{text:?} with {splice:?}"
        );
        assert_eq!(document.as_str(), text);
        assert_eq!(format!("{:?}", document.root()), root);
    }
    // The right splice is kept.
    let mut document = Document::parse("[a]\nx = 1\n", Limits::DEFAULT).expect("parses");
    let path = [Step::Key("a"), Step::Key("k")];
    assert_eq!(
        document.commit(&path, &seven, insert(10, "k = 7\n")),
        Ok(())
    );
    assert_eq!(document.as_str(), "[a]\nx = 1\nk = 7\n");
}
