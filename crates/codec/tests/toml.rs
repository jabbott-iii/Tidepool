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

//! TOML compliance against toml-test's TOML 1.1 cases, and property tests
//! of the parser on mutated and generated documents.
//!
//! The cases in `tests/toml-test/` are toml-test v2.2.0's: its list of the
//! TOML 1.1 cases, `files-toml-1.1.0`, and the files it names, copied byte
//! for byte. They are third-party data under the MIT license; their source
//! and notice are in `tests/toml-test/NOTICE.md`. Never edit them: a newer
//! release replaces the directory (ADR-0026).
//!
//! toml-test drives a decoder through a Go program; its comparison rules
//! (`runner.go` and `json.go` there) are written again below, in
//! [`compare_value`] and the functions it calls:
//!
//! - Every `valid/X.toml` must parse, and its value must match
//!   `valid/X.json`. Every `invalid/X.toml` must fail.
//! - The expected value is JSON. A JSON object with exactly two members,
//!   `type` and `value`, is a scalar: `type` is `string`, `integer`,
//!   `float`, `bool`, `datetime` (an offset date-time), `datetime-local`,
//!   `date-local` or `time-local`, and `value` is its text, always a JSON
//!   string. Any other object is a table, and a JSON array is an array (of
//!   either kind). Anything else in the expected JSON is a bug in the case.
//! - A table matches if it has the same keys, as many entries as it has
//!   keys, and each value matches; order does not matter. (toml-test reads
//!   a decoder's output into a map, which cannot hold a key twice; here the
//!   entries are counted instead.) An array matches if it has the same
//!   length and each element matches the one in the same place. A scalar
//!   matches only a scalar of the same type: an integer never matches a
//!   float.
//! - Strings and integers match if the text is the same, the integer
//!   written in decimal. Booleans match ignoring case.
//! - Floats: if either is NaN (text ending in `nan`, any case and sign),
//!   both must be NaN, whatever their signs. Otherwise they are compared as
//!   `f64` numbers. toml-test's `==` takes `0.0` and `-0.0` as equal; this
//!   test is stricter and also requires a zero's sign to match, which every
//!   expected zero on the list states (`"0"` or `"-0"`).
//! - Date-times are read as RFC 3339 after a space or `t` between date and
//!   time becomes `T`, and `z` becomes `Z`; digits of a fraction of a
//!   second past the ninth are dropped. Offset date-times match if they are
//!   the same instant (`07:32:00Z` is `00:32:00-07:00`); local date-times,
//!   dates and times match if every field is the same.
//!
//! The suite's counts are checked, so that a file that goes missing, or a
//! file that is not on the list, fails the test. For an invalid case, the
//! error's offset must be inside the input and agree with its line and
//! column. For a valid one, every span in the tree must be inside the input
//! and on character boundaries. A failure prints each case that failed and
//! what differed.
//!
//! The property tests print their seed and a command to rerun them if they
//! fail; set `TIDEPOOL_SEED` to run them with another seed. Real fuzzing
//! arrives with `xtask fuzz-smoke` (roadmap M1.6).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use tidepool_codec::json::{self, Dialect};
use tidepool_codec::toml::{self, Datetime, ErrorKind, Offset, Table, TableKind, Value, ValueKind};
use tidepool_codec::{CodecError, Limits};
use tidepool_core::Pcg32;
use tidepool_core::testing::{check, check_sized};

/// Where the vendored cases are.
fn suite() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/toml-test")
}

/// The list of TOML 1.1 cases, and the two files beside the cases that are
/// not on it.
const LIST: &str = "files-toml-1.1.0";
const NOTICE: &str = "NOTICE.md";

/// How many cases toml-test v2.2.0 lists for TOML 1.1.
const VALID_CASES: usize = 214;
const INVALID_CASES: usize = 467;

/// The cases on the list, by kind: names without `.toml`, such as
/// `valid/array/array`.
struct Cases {
    valid: Vec<String>,
    invalid: Vec<String>,
}

/// Reads the list and checks it against the directory: every listed file
/// exists, every file is listed, every valid case has its `.json`, and the
/// counts are toml-test's.
fn cases() -> Cases {
    let root = suite();
    let list = std::fs::read_to_string(root.join(LIST)).expect("read the list of cases");
    let listed: BTreeSet<String> = list
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(
        listed.len(),
        list.lines().count(),
        "the list repeats a line"
    );

    let mut found = BTreeSet::new();
    let mut directories = vec![root.clone()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).expect("list a directory") {
            let path = entry.expect("read a directory entry").path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .expect("under the suite")
                .to_str()
                .expect("a UTF-8 name")
                .replace('\\', "/");
            if relative != LIST && relative != NOTICE {
                found.insert(relative);
            }
        }
    }
    let missing: Vec<&String> = listed.difference(&found).collect();
    let extra: Vec<&String> = found.difference(&listed).collect();
    assert!(missing.is_empty(), "listed but missing: {missing:?}");
    assert!(extra.is_empty(), "present but not listed: {extra:?}");

    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    let mut expectations = 0;
    for file in &listed {
        if let Some(name) = file.strip_suffix(".toml") {
            if name.starts_with("valid/") {
                assert!(
                    listed.contains(&format!("{name}.json")),
                    "{name} has no expected value"
                );
                valid.push(name.to_string());
            } else if name.starts_with("invalid/") {
                invalid.push(name.to_string());
            } else {
                panic!("a case that is neither valid nor invalid: {file}");
            }
        } else if file.starts_with("valid/") && file.ends_with(".json") {
            expectations += 1;
        } else {
            panic!("an unexpected file on the list: {file}");
        }
    }
    assert_eq!(valid.len(), VALID_CASES, "valid cases");
    assert_eq!(expectations, VALID_CASES, "expected values");
    assert_eq!(invalid.len(), INVALID_CASES, "invalid cases");
    assert_eq!(listed.len(), 2 * VALID_CASES + INVALID_CASES);
    Cases { valid, invalid }
}

fn read(name: &str, extension: &str) -> Vec<u8> {
    std::fs::read(suite().join(format!("{name}.{extension}")))
        .unwrap_or_else(|error| panic!("read {name}.{extension}: {error}"))
}

/// Panics with every failure, one case per line, if there are any.
fn report(kind: &str, total: usize, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} of {total} {kind} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_list_names_every_file_and_the_counts_are_toml_tests() {
    let cases = cases();
    assert_eq!(cases.valid.len() + cases.invalid.len(), 681);
}

#[test]
fn every_valid_case_parses_to_its_expected_value() {
    let cases = cases();
    let mut failures = Vec::new();
    for name in &cases.valid {
        let input = read(name, "toml");
        let expected = json::parse_bytes(&read(name, "json"), Dialect::Json, Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}.json is not JSON: {error}"));
        let outcome = match toml::parse_bytes(&input, Limits::DEFAULT) {
            Err(error) => Err(format!("rejected: {error}")),
            Ok(root) => compare_table("", &expected, &root)
                .and_then(|()| spans_are_in_bounds(&input, &root)),
        };
        if let Err(problem) = outcome {
            failures.push(format!("{name}: {problem}"));
        }
    }
    report("valid", cases.valid.len(), &failures);
}

#[test]
fn every_invalid_case_is_rejected_where_it_says() {
    let cases = cases();
    let mut failures = Vec::new();
    for name in &cases.invalid {
        let input = read(name, "toml");
        let outcome = match toml::parse_bytes(&input, Limits::DEFAULT) {
            Ok(root) => Err(format!("accepted, as {root:?}")),
            Err(error) => error_is_consistent(&input, &error),
        };
        if let Err(problem) = outcome {
            failures.push(format!("{name}: {problem}"));
        }
    }
    report("invalid", cases.invalid.len(), &failures);
}

// ------------------------------------------------- toml-test's comparison

/// Joins a table's path and a key, for messages: `a.b`.
fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// `expected`'s members, the last one winning for a repeated name, as Go's
/// `encoding/json` reads an object into a map.
fn members(expected: &json::Value) -> Option<BTreeMap<&str, &json::Value>> {
    let members = expected.as_object()?;
    Some(
        members
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect(),
    )
}

/// The `type` and `value` of a tagged scalar, if `expected` is one: an
/// object with exactly those two members.
fn tagged(expected: &json::Value) -> Option<(&json::Value, &json::Value)> {
    let members = members(expected)?;
    if members.len() != 2 {
        return None;
    }
    Some((*members.get("type")?, *members.get("value")?))
}

/// Compares the table at `path` with `expected`.
fn compare_table(path: &str, expected: &json::Value, actual: &Table) -> Result<(), String> {
    if tagged(expected).is_some() {
        return Err(format!("{path:?} should be a value, but is a table"));
    }
    let Some(expected) = members(expected) else {
        return Err(format!(
            "bug in the case: {path:?} expects {expected:?}, not an object or array"
        ));
    };
    for name in expected.keys() {
        if !actual.contains_key(name) {
            return Err(format!("{:?} is missing", join(path, name)));
        }
    }
    for (key, _) in actual.entries() {
        if !expected.contains_key(key.name.as_str()) {
            return Err(format!("{:?} is not expected", join(path, &key.name)));
        }
    }
    // The same names, and as many entries: a table that holds a key twice
    // has more entries than the expected object has names.
    if actual.len() != expected.len() {
        return Err(format!(
            "{path:?} has {} entries, not {}",
            actual.len(),
            expected.len()
        ));
    }
    for (name, value) in expected {
        let actual = actual.get(name).ok_or("checked above")?;
        compare_value(&join(path, name), value, actual)?;
    }
    Ok(())
}

/// Compares the value at `path` with `expected`, by toml-test's rules.
fn compare_value(path: &str, expected: &json::Value, actual: &Value) -> Result<(), String> {
    if let Some((kind, text)) = tagged(expected) {
        let kind = kind
            .as_str()
            .ok_or_else(|| format!("bug in the case: {path:?} has a type that is not a string"))?;
        if kind == "array" {
            return compare_array(path, text, actual);
        }
        let text = text
            .as_str()
            .ok_or_else(|| format!("bug in the case: {path:?} has a value that is not a string"))?;
        return compare_scalar(path, kind, text, actual);
    }
    match &actual.kind {
        ValueKind::Table(table) if expected.as_object().is_some() => {
            compare_table(path, expected, table)
        }
        ValueKind::Array(_) if expected.as_array().is_some() => {
            compare_array(path, expected, actual)
        }
        _ if expected.as_object().is_none() && expected.as_array().is_none() => Err(format!(
            "bug in the case: {path:?} expects {expected:?}, not an object or array"
        )),
        other => Err(format!(
            "{path:?} should be {}, but is {}",
            if expected.as_array().is_some() {
                "an array"
            } else {
                "a table"
            },
            type_of(other)
        )),
    }
}

fn compare_array(path: &str, expected: &json::Value, actual: &Value) -> Result<(), String> {
    let Some(expected) = expected.as_array() else {
        return Err(format!("bug in the case: {path:?} expects an array"));
    };
    let Some(actual) = actual.as_array() else {
        return Err(format!(
            "{path:?} should be an array, but is {}",
            type_of(&actual.kind)
        ));
    };
    if expected.len() != actual.len() {
        return Err(format!(
            "{path:?} has {} elements, not {}",
            actual.len(),
            expected.len()
        ));
    }
    for (index, (expected, actual)) in expected.iter().zip(actual.items()).enumerate() {
        compare_value(&format!("{path}[{index}]"), expected, actual)?;
    }
    Ok(())
}

/// toml-test's name for the type of `kind`.
fn type_of(kind: &ValueKind) -> &'static str {
    match kind {
        ValueKind::String(_) => "string",
        ValueKind::Integer(_) => "integer",
        ValueKind::Float(_) => "float",
        ValueKind::Boolean(_) => "bool",
        ValueKind::Datetime(Datetime::OffsetDatetime { .. }) => "datetime",
        ValueKind::Datetime(Datetime::LocalDatetime { .. }) => "datetime-local",
        ValueKind::Datetime(Datetime::LocalDate(_)) => "date-local",
        ValueKind::Datetime(Datetime::LocalTime(_)) => "time-local",
        ValueKind::Array(_) => "array",
        ValueKind::Table(_) => "table",
    }
}

fn compare_scalar(path: &str, kind: &str, text: &str, actual: &Value) -> Result<(), String> {
    let found = type_of(&actual.kind);
    if found != kind {
        return Err(format!(
            "{path:?} should be a {kind} ({text:?}), but is a {found}"
        ));
    }
    let same = match &actual.kind {
        ValueKind::String(value) => value == text,
        ValueKind::Integer(value) => value.to_string() == text,
        ValueKind::Boolean(value) => value.to_string() == text.to_ascii_lowercase(),
        ValueKind::Float(value) => same_float(text, *value)
            .map_err(|problem| format!("bug in the case: {path:?}: {problem}"))?,
        ValueKind::Datetime(value) => {
            let expected = Rfc3339::read(text).ok_or_else(|| {
                format!("bug in the case: {path:?}: {text:?} is not an RFC 3339 {kind}")
            })?;
            expected.matches(value)
        }
        ValueKind::Array(_) | ValueKind::Table(_) => false,
    };
    if same {
        Ok(())
    } else {
        Err(format!("{path:?} should be {text:?}, but is {actual:?}"))
    }
}

/// Whether `actual` is the float `expected` writes.
fn same_float(expected: &str, actual: f64) -> Result<bool, String> {
    let expected = expected.to_ascii_lowercase();
    if expected.ends_with("nan") {
        return Ok(actual.is_nan());
    }
    let expected: f64 = expected
        .parse()
        .map_err(|_| format!("{expected:?} is not a float"))?;
    Ok(!actual.is_nan()
        && expected == actual
        && expected.is_sign_negative() == actual.is_sign_negative())
}

/// A date-time as toml-test writes the expected ones, read by this test
/// alone rather than by the parser under test: `YYYY-MM-DD`,
/// `HH:MM:SS[.fraction]`, or both separated by `T`, then `Z` or `±HH:MM` for
/// an offset date-time.
#[derive(Debug, PartialEq, Eq)]
struct Rfc3339 {
    date: Option<(u16, u8, u8)>,
    /// Hour, minute, second and nanosecond.
    time: Option<(u8, u8, u8, u32)>,
    /// Minutes east of UTC.
    offset: Option<i32>,
}

impl Rfc3339 {
    fn read(text: &str) -> Option<Self> {
        // As toml-test does before reading one.
        let text = text.replace([' ', 't'], "T").replace('z', "Z");
        let number = |piece: &str| -> Option<u32> {
            if piece.is_empty() || !piece.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            piece.parse().ok()
        };
        let (date, rest) = if text.as_bytes().get(4) == Some(&b'-') {
            let date = (
                u16::try_from(number(text.get(0..4)?)?).ok()?,
                u8::try_from(number(text.get(5..7)?)?).ok()?,
                u8::try_from(number(text.get(8..10)?)?).ok()?,
            );
            if text.get(4..5)? != "-" || text.get(7..8)? != "-" {
                return None;
            }
            match text.get(10..)? {
                "" => (Some(date), None),
                rest => (Some(date), Some(rest.strip_prefix('T')?.to_string())),
            }
        } else {
            (None, Some(text.clone()))
        };
        let Some(rest) = rest else {
            return Some(Self {
                date,
                time: None,
                offset: None,
            });
        };
        let hour = u8::try_from(number(rest.get(0..2)?)?).ok()?;
        let minute = u8::try_from(number(rest.get(3..5)?)?).ok()?;
        let second = u8::try_from(number(rest.get(6..8)?)?).ok()?;
        if rest.get(2..3)? != ":" || rest.get(5..6)? != ":" {
            return None;
        }
        let mut rest = rest.get(8..)?;
        let mut nanosecond = 0;
        if let Some(fraction) = rest.strip_prefix('.') {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            let kept = fraction.get(..digits.min(9))?;
            nanosecond = number(kept)? * 10u32.pow(9 - kept.len() as u32);
            rest = fraction.get(digits..)?;
        }
        let offset = match rest {
            "" => None,
            "Z" => Some(0),
            _ => {
                let sign = match rest.get(0..1)? {
                    "+" => 1,
                    "-" => -1,
                    _ => return None,
                };
                if rest.len() != 6 || rest.get(3..4)? != ":" {
                    return None;
                }
                let hours = i32::try_from(number(rest.get(1..3)?)?).ok()?;
                let minutes = i32::try_from(number(rest.get(4..6)?)?).ok()?;
                Some(sign * (hours * 60 + minutes))
            }
        };
        if offset.is_some() && date.is_none() {
            return None;
        }
        Some(Self {
            date,
            time: Some((hour, minute, second, nanosecond)),
            offset,
        })
    }

    /// Whether `actual` is this date-time: the same instant for an offset
    /// date-time, the same fields otherwise.
    fn matches(&self, actual: &Datetime) -> bool {
        let date = |date: &toml::Date| (date.year, date.month, date.day);
        let time = |time: &toml::Time| (time.hour, time.minute, time.second, time.nanosecond);
        match (actual, self) {
            (
                Datetime::OffsetDatetime {
                    date: actual_date,
                    time: actual_time,
                    offset,
                },
                Self {
                    date: Some(expected_date),
                    time: Some(expected_time),
                    offset: Some(expected_offset),
                },
            ) => {
                let minutes = match *offset {
                    Offset::Utc => 0,
                    Offset::Minutes(minutes) => i32::from(minutes),
                };
                instant(date(actual_date), time(actual_time), minutes)
                    == instant(*expected_date, *expected_time, *expected_offset)
            }
            (Datetime::LocalDatetime { date: d, time: t }, expected) => {
                expected.offset.is_none()
                    && expected.date == Some(date(d))
                    && expected.time == Some(time(t))
            }
            (Datetime::LocalDate(d), expected) => {
                *expected
                    == Self {
                        date: Some(date(d)),
                        time: None,
                        offset: None,
                    }
            }
            (Datetime::LocalTime(t), expected) => {
                *expected
                    == Self {
                        date: None,
                        time: Some(time(t)),
                        offset: None,
                    }
            }
            _ => false,
        }
    }
}

/// Nanoseconds since 1970-01-01T00:00:00Z of a date and time at an offset
/// of `minutes` east of UTC.
fn instant(date: (u16, u8, u8), time: (u8, u8, u8, u32), minutes: i32) -> i128 {
    let (year, month, day) = (i64::from(date.0), i64::from(date.1), i64::from(date.2));
    // Days from the civil date: the usual algorithm, in 400-year eras that
    // start on March 1st.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds =
        days * 86_400 + i64::from(time.0) * 3600 + i64::from(time.1) * 60 + i64::from(time.2)
            - i64::from(minutes) * 60;
    i128::from(seconds) * 1_000_000_000 + i128::from(time.3)
}

#[test]
fn the_test_reads_rfc_3339_on_its_own_terms() {
    let read = |text| Rfc3339::read(text).expect(text);
    assert_eq!(
        read("1979-05-27T07:32:00.123456789999Z"),
        Rfc3339 {
            date: Some((1979, 5, 27)),
            time: Some((7, 32, 0, 123_456_789)),
            offset: Some(0),
        }
    );
    assert_eq!(read("1979-05-27 07:32:00-07:30").offset, Some(-450));
    assert_eq!(read("07:32:00.5").time, Some((7, 32, 0, 500_000_000)));
    assert_eq!(read("1979-05-27").time, None);
    for bad in ["1979-05-27T", "7:32:00", "07:32:00Z", "1979-5-27", "x"] {
        assert_eq!(Rfc3339::read(bad), None, "{bad}");
    }
    // The epoch, a day later, and an hour west.
    assert_eq!(instant((1970, 1, 1), (0, 0, 0, 0), 0), 0);
    assert_eq!(instant((1970, 1, 2), (0, 0, 0, 7), 0), 86_400_000_000_007);
    assert_eq!(instant((1970, 1, 1), (0, 0, 0, 0), -60), 3_600_000_000_000);
    assert_eq!(
        instant((2000, 3, 1), (0, 0, 0, 0), 0),
        951_868_800_000_000_000
    );
    assert!(same_float("-0", -0.0).expect("a float"));
    assert!(!same_float("0", -0.0).expect("a float"));
    assert!(same_float("nan", -f64::NAN).expect("a float"));
    assert!(same_float("-inf", f64::NEG_INFINITY).expect("a float"));
}

// ----------------------------------------------------------- consistency

/// Checks that an error's offset is inside the input, at a character
/// boundary, and agrees with its line and column; that an `InvalidUtf8`
/// error is exactly where the UTF-8 goes wrong; and that an input that is
/// not UTF-8 fails for nothing else (only the size is checked first).
fn error_is_consistent(bytes: &[u8], error: &CodecError) -> Result<(), String> {
    let CodecError::Toml {
        offset,
        line,
        column,
        kind,
    } = error
    else {
        return Err(format!("not a TOML error: {error}"));
    };
    let (offset, line, column) = (*offset, *line, *column);
    let valid_up_to = std::str::from_utf8(bytes)
        .err()
        .map(|error| error.valid_up_to());
    let size_error = matches!(kind, ErrorKind::InputTooLarge { .. });
    if *kind == ErrorKind::InvalidUtf8 {
        if valid_up_to != Some(offset) {
            return Err(format!(
                "{error}: offset {offset}, but UTF-8 is valid to {valid_up_to:?}"
            ));
        }
    } else if valid_up_to.is_some() && !size_error {
        return Err(format!("{error}: the input is not UTF-8"));
    }
    if offset > bytes.len() {
        return Err(format!(
            "{error}: offset {offset} is past the end ({})",
            bytes.len()
        ));
    }
    let before = match std::str::from_utf8(bytes.get(..offset).unwrap_or_default()) {
        Ok(before) => before,
        Err(_) if size_error && valid_up_to.is_some() => return Ok(()),
        Err(_) => {
            return Err(format!(
                "{error}: offset {offset} is not a character boundary"
            ));
        }
    };
    let before = before.strip_prefix('\u{feff}').unwrap_or(before);
    let expected_line = before.matches('\n').count() + 1;
    let line_text = before.rsplit('\n').next().unwrap_or(before);
    let expected_column = line_text.chars().count() + 1;
    if (line, column) != (expected_line, expected_column) {
        return Err(format!(
            "{error}: offset {offset} is line {expected_line}, column {expected_column}"
        ));
    }
    Ok(())
}

/// Checks that every span in `root` is inside `input`, ends after it
/// starts, and is at character boundaries.
fn spans_are_in_bounds(input: &[u8], root: &Table) -> Result<(), String> {
    let text = std::str::from_utf8(input).map_err(|error| error.to_string())?;
    let check = |what: &str, span: toml::Span| -> Result<(), String> {
        if span.start > span.end || text.get(span.range()).is_none() {
            return Err(format!("the span {span:?} of {what} is not in the input"));
        }
        Ok(())
    };
    let mut tables = vec![root];
    while let Some(table) = tables.pop() {
        let mut values: Vec<&Value> = Vec::new();
        for (key, value) in table.entries() {
            check(&format!("key {:?}", key.name), key.span)?;
            values.push(value);
        }
        while let Some(value) = values.pop() {
            check(&format!("{value:?}"), value.span)?;
            match &value.kind {
                ValueKind::Table(inner) => tables.push(inner),
                ValueKind::Array(array) => values.extend(array.items()),
                _ => {}
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------- properties

/// Checks what must hold for any input at all: the parser returns instead
/// of panicking; an error is consistent (see [`error_is_consistent`]);
/// text input gives the same result as bytes; and whatever parses has
/// every span in bounds.
///
/// Under `limits`, which may be smaller than the defaults, the result must
/// be the same as under the defaults, or an error for exceeding one of
/// `limits` that names the right limit and is just as consistent.
fn parser_behaves(bytes: &[u8], limits: Limits) -> Result<(), String> {
    let result = toml::parse_bytes(bytes, Limits::DEFAULT);
    match &result {
        Ok(root) => spans_are_in_bounds(bytes, root)?,
        Err(error) => error_is_consistent(bytes, error)?,
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && !same_result(&toml::parse(text, Limits::DEFAULT), &result)
    {
        return Err("text and bytes disagree".to_string());
    }
    if limits != Limits::DEFAULT {
        let limited = toml::parse_bytes(bytes, limits);
        if !same_result(&limited, &result) {
            let error = limited
                .err()
                .ok_or_else(|| format!("accepted under {limits:?} but not the defaults"))?;
            limit_error_is_right(bytes, &error, limits)
                .and_then(|()| error_is_consistent(bytes, &error))
                .map_err(|problem| {
                    format!("under {limits:?}: {problem}; under the defaults: {result:?}")
                })?;
        }
    }
    Ok(())
}

/// Whether two results are the same, a NaN equal to a NaN: `PartialEq`
/// says a document with a NaN in it differs from itself.
fn same_result(a: &Result<Table, CodecError>, b: &Result<Table, CodecError>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => format!("{a:?}") == format!("{b:?}"),
        (Err(a), Err(b)) => a == b,
        _ => false,
    }
}

/// Checks that `error`, which `limits` caused, is for exceeding one of
/// them, and names the limit it exceeded.
fn limit_error_is_right(bytes: &[u8], error: &CodecError, limits: Limits) -> Result<(), String> {
    let CodecError::Toml { offset, kind, .. } = error else {
        return Err("not a TOML error".to_string());
    };
    let right = match *kind {
        ErrorKind::InputTooLarge { length, limit } => {
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
        Err(format!("{error} is not the error for a limit that was set"))
    }
}

/// Limits for a no-panic case: each one is the default or, half the time,
/// small enough that typical inputs exceed it. The input size is limited
/// one time in four, to a size up to the input's own.
fn random_limits(rng: &mut Pcg32, input_len: usize) -> Limits {
    let mut limits = Limits::DEFAULT;
    if rng.next_u32() & 1 == 0 {
        limits.max_depth = rng.range(0..6u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_entries = rng.range(0..6u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_string_bytes = rng.range(0..11u32) as usize;
    }
    if rng.next_u32() & 1 == 0 {
        limits.max_number_bytes = rng.range(1..30u32) as usize;
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

/// Pieces of TOML that mutations insert: delimiters, keywords, escapes,
/// number and date-time fragments, line breaks and quotes.
const TOKENS: &[&str] = &[
    "[",
    "]",
    "[[",
    "]]",
    "{",
    "}",
    ",",
    ".",
    "=",
    "#",
    "\n",
    "\r\n",
    "\r",
    " ",
    "\t",
    "\"",
    "'",
    "\"\"\"",
    "'''",
    "\\",
    "\\u",
    "\\U0010FFFF",
    "\\x4",
    "\\e",
    "a",
    "a.b",
    "1",
    "0x",
    "0o7",
    "0b1",
    "_",
    "+",
    "-",
    "e",
    "1e400",
    "inf",
    "nan",
    "true",
    "false",
    "1979-05-27",
    "T07:32:00",
    " 07:32",
    "Z",
    "-07:00",
    ".999999999999",
    "\u{feff}",
    "é",
    "\u{0}",
    "\u{7f}",
    "[a]\n",
    "[[a]]\n",
    "a = 1\n",
    "a.b = {}\n",
    // Whole lines that define the same few tables in every way, so that
    // documents made of them break the rules on defining tables.
    "\n[a.b]\n",
    "\n[a.b.c]\n",
    "\n[[a.b]]\n",
    "\n[[a.b.c]]\n",
    "\na = {b = {}}\n",
    "\na.b.c = 1\n",
    "\nb.c = []\n",
    "\nc = [{}]\n",
];

/// The documents the mutation properties start from: every valid case on
/// toml-test's list, and two of the project's own shape.
fn seed_documents() -> Vec<Vec<u8>> {
    let mut seeds: Vec<Vec<u8>> = cases()
        .valid
        .iter()
        .map(|name| read(name, "toml"))
        .collect();
    seeds.push(
        b"[package]\nname = \"tidepool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
          [dependencies]\ntidepool-core = { path = \"../core\" }\n\n[lints]\nworkspace = true\n"
            .to_vec(),
    );
    seeds.push(
        b"version = 1\n[editor]\nfont.size = 13\nrulers = [80, 100,]\n[[keymap]]\n\
          keys = \"ctrl+s\"\ncommand = 'save'\nwhen = { mode = \"normal\", \n  focus = true, }\n"
            .to_vec(),
    );
    seeds
}

/// One random edit of the bytes, which may break UTF-8: flip a bit, set a
/// byte, delete a byte, insert a token, repeat a piece or a line, or cut the
/// input short.
fn mutate(bytes: &mut Vec<u8>, rng: &mut Pcg32) {
    if bytes.is_empty() {
        bytes.extend_from_slice(rng.choose(TOKENS).copied().unwrap_or("").as_bytes());
        return;
    }
    let at = rng.range(0..bytes.len() as u32) as usize;
    match rng.range(0..7u32) {
        0 => bytes[at] ^= 1 << rng.range(0..8u32),
        1 => bytes[at] = rng.next_u32() as u8,
        2 => _ = bytes.remove(at),
        3 => {
            let token = rng.choose(TOKENS).copied().unwrap_or("");
            bytes.splice(at..at, token.bytes());
        }
        4 => {
            // Repeat a piece right after itself.
            let end = (at + rng.range(1..20u32) as usize).min(bytes.len());
            let piece = bytes[at..end].to_vec();
            bytes.splice(end..end, piece);
        }
        5 => {
            // Copy a whole line to the start of another, which defines its
            // key or table twice.
            let lines: Vec<&[u8]> = bytes.split_inclusive(|&byte| byte == b'\n').collect();
            let line = rng
                .choose(&lines)
                .map(|line| line.to_vec())
                .unwrap_or_default();
            let to = bytes[..at]
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(0, |newline| newline + 1);
            bytes.splice(to..to, line);
        }
        _ => bytes.truncate(at),
    }
}

#[test]
fn mutated_documents_never_panic() {
    let seeds = seed_documents();
    check_sized(
        "toml mutation",
        4000,
        |rng, size| {
            let mut bytes = rng.choose(&seeds).cloned().unwrap_or_default();
            for _ in 0..1 + size / 20 {
                mutate(&mut bytes, rng);
            }
            let limits = random_limits(rng, bytes.len());
            (bytes, limits)
        },
        behaves_in_time,
    );
}

#[test]
fn random_token_soup_never_panics() {
    check_sized(
        "toml token soup",
        3000,
        |rng, size| {
            let bytes: Vec<u8> = (0..size + 1)
                .flat_map(|_| rng.choose(TOKENS).copied().unwrap_or("").bytes())
                .collect();
            let limits = random_limits(rng, bytes.len());
            (bytes, limits)
        },
        behaves_in_time,
    );
}

#[test]
fn every_seed_document_behaves_under_random_limits() {
    let seeds = seed_documents();
    check(
        "toml seeds under limits",
        1000,
        |rng| {
            let bytes = rng.choose(&seeds).cloned().unwrap_or_default();
            let limits = random_limits(rng, bytes.len());
            (bytes, limits)
        },
        behaves_in_time,
    );
}

// ---------------------------------------------------- generated documents

/// What a generated document must parse to, with the text written for each
/// part, so that spans are checked too.
#[derive(Debug)]
enum Expected {
    /// A scalar, and its literal as written.
    Scalar(ValueKind, String),
    /// An array written as a value, and its text from `[` to `]`.
    Array(Vec<Expected>, String),
    /// A table: its entries (name, key as written, value), its kind, and
    /// the text of its span.
    Table(Vec<(String, String, Expected)>, TableKind, String),
    /// An array of tables, whose elements are each a `Table`.
    Tables(Vec<Expected>, String),
}

/// Writes random valid documents and what they hold. Every key comes from
/// a counter, so that none is defined twice.
struct Writer<'r> {
    rng: &'r mut Pcg32,
    names: u32,
    newline: &'static str,
}

/// Characters for generated strings: ones that need escapes or quotes,
/// control characters, and non-ASCII ones.
const STRING_CHARS: &[char] = &[
    'a', 'b', 'z', ' ', '"', '\'', '\\', '\t', '\n', '\r', '\0', '\u{1b}', '\u{7f}', 'é', '€',
    '😀', '#', '=', '[', '}', '\u{feff}',
];

fn days_in(year: u16, month: u8) -> u8 {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Writer<'_> {
    fn chance(&mut self, one_in: u32) -> bool {
        self.rng.range(0..one_in) == 0
    }

    /// Whitespace, possibly none.
    fn space(&mut self) -> &'static str {
        self.rng
            .choose(&["", " ", "  ", "\t", " \t "])
            .unwrap_or(&"")
    }

    /// What may come between the entries of an array or inline table:
    /// whitespace, line breaks and comments.
    fn trivia(&mut self) -> String {
        let newline = self.newline;
        match self.rng.range(0..5u32) {
            0 => format!("{newline}{}", self.space()),
            1 => format!(" # note é ] }} ,{newline}{}", self.space()),
            _ => self.space().to_string(),
        }
    }

    /// A fresh key: its name, and how it is written.
    fn key(&mut self) -> (String, String) {
        self.names += 1;
        let base = format!("k{}", self.names);
        let name = match self.rng.range(0..8u32) {
            0 => format!("{base} é.x"),
            1 => format!("{base}\t\"'"),
            2 => format!("{base}-_"),
            _ => base,
        };
        let bare = name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        let written = match self.rng.range(0..4u32) {
            0 | 1 if bare => name.clone(),
            2 if !name.contains('\'') => format!("'{name}'"),
            _ => self.basic(&name),
        };
        (name, written)
    }

    /// `text` as a basic string, escaped where it must be and, at random,
    /// where it need not be.
    fn basic(&mut self, text: &str) -> String {
        let mut out = String::from("\"");
        for c in text.chars() {
            let code = u32::from(c);
            let must = c == '"' || c == '\\' || (c.is_control() && c != '\t');
            if !must && !self.chance(8) {
                out.push(c);
                continue;
            }
            match (c, self.rng.range(0..4u32)) {
                ('"', 0) => out.push_str("\\\""),
                ('\\', 0) => out.push_str("\\\\"),
                ('\n', 0) => out.push_str("\\n"),
                ('\t', 0) => out.push_str("\\t"),
                ('\r', 0) => out.push_str("\\r"),
                ('\u{1b}', 0) => out.push_str("\\e"),
                (_, 1) if code <= 0xFF => out.push_str(&format!("\\x{code:02X}")),
                (_, 2) if code <= 0xFFFF => out.push_str(&format!("\\u{code:04x}")),
                _ => out.push_str(&format!("\\U{code:08X}")),
            }
        }
        out.push('"');
        out
    }

    /// A random string in one of TOML's four forms, where its content
    /// allows that form.
    fn string(&mut self) -> Expected {
        let length = self.rng.range(0..8u32);
        let content: String = (0..length)
            .map(|_| *self.rng.choose(STRING_CHARS).unwrap_or(&'a'))
            .collect();
        let raw_ok = |allow_newline: bool| {
            content.chars().all(|c| {
                c != '\'' && (!c.is_control() || c == '\t' || (allow_newline && c == '\n'))
            })
        };
        let written = match self.rng.range(0..4u32) {
            1 if raw_ok(false) => format!("'{content}'"),
            2 => {
                // Multi-line basic: every quote, backslash and control
                // character but tab and LF escaped; LF written as a line
                // break, perhaps CR LF.
                // A line break right after the quotes is dropped, so one
                // that starts the content needs another before it.
                let mut out = String::from("\"\"\"");
                if content.starts_with('\n') || self.chance(2) {
                    out.push_str(self.newline);
                }
                for c in content.chars() {
                    match c {
                        '\n' => out.push_str(self.newline),
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        c if c.is_control() && c != '\t' => {
                            out.push_str(&format!("\\u{:04X}", u32::from(c)));
                        }
                        c => out.push(c),
                    }
                }
                out.push_str("\"\"\"");
                out
            }
            3 if raw_ok(true) => {
                let mut out = String::from("'''");
                if content.starts_with('\n') || self.chance(2) {
                    out.push_str(self.newline);
                }
                out.push_str(&content.replace('\n', self.newline));
                out.push_str("'''");
                out
            }
            _ => self.basic(&content),
        };
        Expected::Scalar(ValueKind::String(content), written)
    }

    /// `digits` with an underscore between two of them, now and then.
    fn underscored(&mut self, digits: &str) -> String {
        let mut out = String::new();
        for (index, c) in digits.chars().enumerate() {
            if index > 0 && self.chance(5) {
                out.push('_');
            }
            out.push(c);
        }
        out
    }

    fn integer(&mut self) -> Expected {
        let value: i64 = match self.rng.range(0..5u32) {
            0 => i64::from(self.rng.range(0..100u32)),
            1 => -i64::from(self.rng.range(0..100u32)),
            2 => *self.rng.choose(&[0, i64::MAX, i64::MIN, -1]).unwrap_or(&0),
            _ => ((u64::from(self.rng.next_u32()) << 32) | u64::from(self.rng.next_u32())) as i64,
        };
        let written = if value >= 0 && self.chance(2) {
            let (prefix, digits) = match self.rng.range(0..4u32) {
                0 => ("0x", format!("{value:x}")),
                1 => ("0x", format!("{value:X}")),
                2 => ("0o", format!("{value:o}")),
                _ => ("0b", format!("{value:b}")),
            };
            let zeros = if self.chance(4) { "00" } else { "" };
            format!("{prefix}{zeros}{}", self.underscored(&digits))
        } else {
            let sign = if value < 0 {
                "-"
            } else if self.chance(4) {
                "+"
            } else {
                ""
            };
            format!(
                "{sign}{}",
                self.underscored(&value.unsigned_abs().to_string())
            )
        };
        Expected::Scalar(ValueKind::Integer(value), written)
    }

    fn float(&mut self) -> Expected {
        let special = [
            0.0,
            -0.0,
            1.5,
            -2.25,
            1e-7,
            6.02e23,
            f64::MAX,
            f64::MIN_POSITIVE,
            5e-324,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            -f64::NAN,
        ];
        let value = if self.chance(2) {
            *self.rng.choose(&special).unwrap_or(&0.0)
        } else {
            let bits = (u64::from(self.rng.next_u32()) << 32) | u64::from(self.rng.next_u32());
            let value = f64::from_bits(bits);
            if value.is_finite() { value } else { 1.0 }
        };
        let written = if value.is_nan() {
            let sign = if value.is_sign_negative() {
                "-"
            } else if self.chance(2) {
                "+"
            } else {
                ""
            };
            format!("{sign}nan")
        } else if value.is_infinite() {
            let sign = if value < 0.0 {
                "-"
            } else {
                *self.rng.choose(&["", "+"]).unwrap_or(&"")
            };
            format!("{sign}inf")
        } else {
            // The shortest text that reads back as the same `f64`, which
            // always has a `.` or an exponent.
            let shortest = format!("{value:?}");
            let (mantissa, exponent) = match shortest.split_once('e') {
                Some((mantissa, exponent)) => (mantissa.to_string(), format!("e{exponent}")),
                None => (shortest.clone(), String::new()),
            };
            let exponent = if exponent.is_empty() || self.chance(2) {
                exponent
            } else {
                exponent.to_uppercase()
            };
            format!("{mantissa}{exponent}")
        };
        Expected::Scalar(ValueKind::Float(value), written)
    }

    fn datetime(&mut self) -> Expected {
        let year = self.rng.range(0..10_000u32) as u16;
        let month = self.rng.range(1..13u32) as u8;
        let day = self.rng.range(1..u32::from(days_in(year, month)) + 1) as u8;
        let date = toml::Date { year, month, day };
        let mut time = toml::Time {
            hour: self.rng.range(0..24u32) as u8,
            minute: self.rng.range(0..60u32) as u8,
            second: self.rng.range(0..61u32) as u8,
            nanosecond: if self.chance(2) {
                0
            } else {
                self.rng.range(0..1_000_000_000u32)
            },
        };
        if self.chance(4) {
            time.second = 0;
            time.nanosecond = 0;
        }
        let mut time_text = format!("{:02}:{:02}", time.hour, time.minute);
        if time.second != 0 || time.nanosecond != 0 || self.chance(2) {
            time_text.push_str(&format!(":{:02}", time.second));
            if time.nanosecond != 0 || self.chance(4) {
                let mut digits = format!("{:09}", time.nanosecond);
                if self.chance(3) {
                    // Digits past the ninth, which are dropped.
                    digits.push_str(&self.rng.range(0..1000u32).to_string());
                } else {
                    let trimmed = digits.trim_end_matches('0');
                    digits = if trimmed.is_empty() {
                        "0".to_string()
                    } else {
                        trimmed.to_string()
                    };
                }
                time_text.push('.');
                time_text.push_str(&digits);
            }
        }
        let date_text = format!("{year:04}-{month:02}-{day:02}");
        let separator = *self.rng.choose(&["T", "t", " "]).unwrap_or(&"T");
        let (value, written) = match self.rng.range(0..4u32) {
            0 => {
                let (offset, offset_text) = if self.chance(2) {
                    (
                        Offset::Utc,
                        self.rng.choose(&["Z", "z"]).unwrap_or(&"Z").to_string(),
                    )
                } else {
                    let minutes = self.rng.range(0..1440u32) as i16;
                    let minutes = if self.chance(2) { -minutes } else { minutes };
                    let sign = if minutes < 0 { '-' } else { '+' };
                    let abs = minutes.unsigned_abs();
                    (
                        Offset::Minutes(minutes),
                        format!("{sign}{:02}:{:02}", abs / 60, abs % 60),
                    )
                };
                (
                    Datetime::OffsetDatetime { date, time, offset },
                    format!("{date_text}{separator}{time_text}{offset_text}"),
                )
            }
            1 => (
                Datetime::LocalDatetime { date, time },
                format!("{date_text}{separator}{time_text}"),
            ),
            2 => (Datetime::LocalDate(date), date_text),
            _ => (Datetime::LocalTime(time), time_text),
        };
        Expected::Scalar(ValueKind::Datetime(value), written)
    }

    /// A random value, with arrays and inline tables while `depth` lasts.
    fn value(&mut self, depth: u32) -> Expected {
        let kinds = if depth == 0 { 5 } else { 7 };
        match self.rng.range(0..kinds) {
            0 => self.string(),
            1 => self.integer(),
            2 => self.float(),
            3 => {
                let value = self.chance(2);
                Expected::Scalar(ValueKind::Boolean(value), value.to_string())
            }
            4 => self.datetime(),
            5 => self.array(depth - 1),
            _ => self.inline_table(depth - 1),
        }
    }

    fn array(&mut self, depth: u32) -> Expected {
        let count = self.rng.range(0..4u32);
        let mut text = String::from("[");
        let mut items = Vec::new();
        for index in 0..count {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&self.trivia());
            let item = self.value(depth);
            text.push_str(written(&item));
            items.push(item);
            text.push_str(&self.trivia());
        }
        if count > 0 && self.chance(3) {
            text.push(',');
            text.push_str(&self.trivia());
        }
        text.push(']');
        Expected::Array(items, text)
    }

    fn inline_table(&mut self, depth: u32) -> Expected {
        let count = self.rng.range(0..4u32);
        let mut text = String::from("{");
        let mut entries = Vec::new();
        for index in 0..count {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&self.trivia());
            self.entry(&mut text, &mut entries, depth);
            text.push_str(&self.trivia());
        }
        if count > 0 && self.chance(3) {
            text.push(',');
            text.push_str(&self.trivia());
        }
        text.push('}');
        Expected::Table(entries, TableKind::Inline, text)
    }

    /// Writes a key/value pair to `text`, its key simple or dotted (with a
    /// fresh table for each part before the last), and adds it to
    /// `entries`.
    fn entry(
        &mut self,
        text: &mut String,
        entries: &mut Vec<(String, String, Expected)>,
        depth: u32,
    ) {
        let parts = if self.chance(3) {
            self.rng.range(2..4u32)
        } else {
            1
        };
        let keys: Vec<(String, String)> = (0..parts).map(|_| self.key()).collect();
        let mut dotted = String::new();
        for (index, (_, written)) in keys.iter().enumerate() {
            if index > 0 {
                dotted.push_str(self.space());
                dotted.push('.');
                dotted.push_str(self.space());
            }
            dotted.push_str(written);
        }
        text.push_str(&dotted);
        text.push_str(self.space());
        text.push('=');
        text.push_str(self.space());
        let mut value = self.value(depth);
        text.push_str(written(&value));
        for (name, key) in keys.iter().skip(1).rev() {
            value = Expected::Table(
                vec![(name.clone(), key.clone(), value)],
                TableKind::Dotted,
                dotted.clone(),
            );
        }
        let (name, key) = keys.into_iter().next().unwrap_or_default();
        entries.push((name, key, value));
    }

    /// Writes key/value pairs, one per line, to `text`.
    fn lines(
        &mut self,
        text: &mut String,
        entries: &mut Vec<(String, String, Expected)>,
        count: u32,
    ) {
        for _ in 0..count {
            text.push_str(self.space());
            self.entry(text, entries, 2);
            if self.chance(4) {
                text.push_str(" # comment ' \" é");
            }
            text.push_str(self.newline);
            if self.chance(5) {
                text.push_str(self.newline);
            }
        }
    }

    /// A whole document: key/value pairs in the root, then sections under
    /// `[header]`s, `[parent.header]`s and runs of `[[header]]`s.
    fn document(&mut self, size: u32) -> (String, Vec<(String, String, Expected)>) {
        let mut text = String::new();
        if self.chance(4) {
            text.push_str("# a document");
            text.push_str(self.newline);
        }
        let mut root = Vec::new();
        let count = self.rng.range(0..size / 10 + 2);
        self.lines(&mut text, &mut root, count);
        for _ in 0..self.rng.range(0..size / 20 + 2) {
            let (name, key) = self.key();
            let open = self.space();
            let close = self.space();
            match self.rng.range(0..3u32) {
                0 => {
                    let header = format!("[{open}{key}{close}]");
                    text.push_str(&header);
                    text.push_str(self.newline);
                    let mut entries = Vec::new();
                    let count = self.rng.range(0..4u32);
                    self.lines(&mut text, &mut entries, count);
                    root.push((
                        name,
                        key,
                        Expected::Table(entries, TableKind::Header, header),
                    ));
                }
                1 => {
                    let (child, child_key) = self.key();
                    let header = format!("[{open}{key}.{child_key}{close}]");
                    text.push_str(&header);
                    text.push_str(self.newline);
                    let mut entries = Vec::new();
                    let count = self.rng.range(0..4u32);
                    self.lines(&mut text, &mut entries, count);
                    let inner = Expected::Table(entries, TableKind::Header, header.clone());
                    let parent = vec![(child, child_key, inner)];
                    root.push((
                        name,
                        key,
                        Expected::Table(parent, TableKind::Implicit, header),
                    ));
                }
                _ => {
                    let mut elements = Vec::new();
                    let mut first = String::new();
                    for index in 0..self.rng.range(1..4u32) {
                        let header = format!("[[{open}{key}{close}]]");
                        if index == 0 {
                            first.clone_from(&header);
                        }
                        text.push_str(&header);
                        text.push_str(self.newline);
                        let mut entries = Vec::new();
                        let count = self.rng.range(0..3u32);
                        self.lines(&mut text, &mut entries, count);
                        elements.push(Expected::Table(entries, TableKind::ArrayHeader, header));
                    }
                    root.push((name, key, Expected::Tables(elements, first)));
                }
            }
        }
        (text, root)
    }
}

/// The text written for `expected`.
fn written(expected: &Expected) -> &str {
    match expected {
        Expected::Scalar(_, text)
        | Expected::Array(_, text)
        | Expected::Table(_, _, text)
        | Expected::Tables(_, text) => text,
    }
}

/// Whether two scalars are the same, a float by its bits but any NaN like
/// any other of the same sign.
fn same_scalar(a: &ValueKind, b: &ValueKind) -> bool {
    match (a, b) {
        (ValueKind::Float(a), ValueKind::Float(b)) if a.is_nan() && b.is_nan() => {
            a.is_sign_negative() == b.is_sign_negative()
        }
        (ValueKind::Float(a), ValueKind::Float(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

/// Checks that `actual` holds what `expected` says, with spans that cover
/// the text written for each part.
fn check_value(input: &str, path: &str, expected: &Expected, actual: &Value) -> Result<(), String> {
    let text = input.get(actual.span.range()).unwrap_or("<out of bounds>");
    if text != written(expected) {
        return Err(format!(
            "{path}: spans {text:?}, not {:?}",
            written(expected)
        ));
    }
    match (expected, &actual.kind) {
        (Expected::Scalar(kind, _), found) if same_scalar(kind, found) => Ok(()),
        (Expected::Array(items, _), ValueKind::Array(array)) if !array.is_array_of_tables() => {
            check_items(input, path, items, array)
        }
        (Expected::Tables(items, _), ValueKind::Array(array)) if array.is_array_of_tables() => {
            check_items(input, path, items, array)
        }
        (Expected::Table(entries, kind, _), ValueKind::Table(table)) if table.kind() == *kind => {
            check_entries(input, path, entries, table)
        }
        _ => Err(format!("{path}: expected {expected:?}, found {actual:?}")),
    }
}

fn check_items(
    input: &str,
    path: &str,
    items: &[Expected],
    array: &toml::Array,
) -> Result<(), String> {
    if items.len() != array.len() {
        return Err(format!(
            "{path}: {} elements, not {}",
            array.len(),
            items.len()
        ));
    }
    for (index, (expected, actual)) in items.iter().zip(array.items()).enumerate() {
        check_value(input, &format!("{path}[{index}]"), expected, actual)?;
    }
    Ok(())
}

fn check_entries(
    input: &str,
    path: &str,
    entries: &[(String, String, Expected)],
    table: &Table,
) -> Result<(), String> {
    let names: Vec<&str> = table
        .entries()
        .iter()
        .map(|(key, _)| key.name.as_str())
        .collect();
    let expected_names: Vec<&str> = entries.iter().map(|(name, _, _)| name.as_str()).collect();
    if names != expected_names {
        return Err(format!("{path}: keys {names:?}, not {expected_names:?}"));
    }
    for ((name, written_key, expected), (key, actual)) in entries.iter().zip(table.entries()) {
        let path = join(path, name);
        let key_text = input.get(key.span.range()).unwrap_or("<out of bounds>");
        if key_text != written_key {
            return Err(format!(
                "{path}: the key spans {key_text:?}, not {written_key:?}"
            ));
        }
        check_value(input, &path, expected, actual)?;
    }
    Ok(())
}

#[test]
fn generated_documents_parse_to_what_was_written() {
    check_sized(
        "toml generated documents",
        2000,
        |rng, size| {
            let newline = if rng.next_u32() & 1 == 0 {
                "\n"
            } else {
                "\r\n"
            };
            let mut writer = Writer {
                rng,
                names: 0,
                newline,
            };
            let (text, root) = writer.document(size);
            (text, root)
        },
        |(text, expected)| {
            let root = toml::parse(text, Limits::DEFAULT)
                .map_err(|error| format!("rejected: {error}\nin:\n{text}"))?;
            check_entries(text, "", expected, &root)
                .map_err(|problem| format!("{problem}\nin:\n{text}"))
        },
    );
}
