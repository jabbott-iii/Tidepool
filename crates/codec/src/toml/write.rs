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

//! The writer behind [`Document`](super::Document): the text an edit
//! creates, always TOML 1.0 on one line (ADR-0031), written within a budget
//! of bytes. See the [module documentation](super#the-text-the-writer-creates)
//! for what each kind is written as.

use super::parse::{datetime_fault, is_bare};
use super::{EditErrorKind, ValueKind};
use crate::json::Number;

/// Text being written, which never grows past `room` bytes: a write that
/// would is refused before anything is copied or allocated for it.
pub(super) struct Out {
    text: String,
    room: usize,
    /// The limit `room` was worked out from, for the error.
    limit: usize,
}

impl Out {
    /// An empty text that may grow to `room` bytes, `room` being what
    /// `limit` (the document's `max_input_bytes`) leaves.
    pub(super) fn new(room: usize, limit: usize) -> Self {
        Self {
            text: String::new(),
            room,
            limit,
        }
    }

    /// Appends `piece`, or fails with [`EditErrorKind::TooLarge`] if it
    /// does not fit in the room left. Capacity grows by doubling, as a
    /// `String`'s does, but never past the room.
    pub(super) fn push(&mut self, piece: &str) -> Result<(), EditErrorKind> {
        let too_large = EditErrorKind::TooLarge { limit: self.limit };
        let needed = self
            .text
            .len()
            .checked_add(piece.len())
            .filter(|&needed| needed <= self.room)
            .ok_or(too_large)?;
        if needed > self.text.capacity() {
            let target = needed
                .max(self.text.capacity().saturating_mul(2))
                .min(self.room);
            self.text
                .reserve_exact(target.saturating_sub(self.text.len()));
        }
        self.text.push_str(piece);
        Ok(())
    }

    /// The text written so far.
    #[cfg(test)]
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    /// The text.
    pub(super) fn into_string(self) -> String {
        self.text
    }
}

/// How deep `value` nests: 0 for a scalar, and one more than its deepest
/// element or entry for an array or table, so `[[1]]` is 2 and `{}` is 1.
/// `None` as soon as it is found to be deeper than `limit`. It keeps its own
/// list rather than recursing, so a value of any depth can be measured; the
/// writer, which recurses, runs only on one within the limit.
pub(super) fn value_depth(value: &ValueKind, limit: usize) -> Option<usize> {
    let mut deepest = 0;
    let mut work = vec![(value, 0usize)];
    while let Some((value, depth)) = work.pop() {
        let inner = depth.checked_add(1)?;
        match value {
            ValueKind::Array(array) => {
                if inner > limit {
                    return None;
                }
                deepest = deepest.max(inner);
                work.extend(array.items().iter().map(|item| (&item.kind, inner)));
            }
            ValueKind::Table(table) => {
                if inner > limit {
                    return None;
                }
                deepest = deepest.max(inner);
                work.extend(
                    table
                        .entries()
                        .iter()
                        .map(|(_, value)| (&value.kind, inner)),
                );
            }
            _ => {}
        }
    }
    Some(deepest)
}

/// Writes `value` on one line: a scalar as its literal, an array as
/// `[a, b]`, and a table, of whatever kind, inline as `{ k = v }`. Recurses
/// once per level, so the caller first checks the depth with
/// [`value_depth`].
///
/// Fails with [`EditErrorKind::InvalidDatetime`] for a date-time with a
/// field out of range, or [`EditErrorKind::TooLarge`] once the room is
/// used up.
pub(super) fn write_value(out: &mut Out, value: &ValueKind) -> Result<(), EditErrorKind> {
    match value {
        ValueKind::String(text) => write_string(out, text),
        ValueKind::Integer(integer) => out.push(&integer.to_string()),
        ValueKind::Float(float) => write_float(out, *float),
        ValueKind::Boolean(boolean) => out.push(if *boolean { "true" } else { "false" }),
        ValueKind::Datetime(datetime) => {
            if let Some(expected) = datetime_fault(datetime) {
                return Err(EditErrorKind::InvalidDatetime { expected });
            }
            out.push(&datetime.to_string())
        }
        ValueKind::Array(array) => {
            out.push("[")?;
            for (index, item) in array.items().iter().enumerate() {
                if index > 0 {
                    out.push(", ")?;
                }
                write_value(out, &item.kind)?;
            }
            out.push("]")
        }
        ValueKind::Table(table) if table.is_empty() => out.push("{}"),
        ValueKind::Table(table) => {
            out.push("{ ")?;
            for (index, (key, value)) in table.entries().iter().enumerate() {
                if index > 0 {
                    out.push(", ")?;
                }
                write_key(out, &key.name)?;
                out.push(" = ")?;
                write_value(out, &value.kind)?;
            }
            out.push(" }")
        }
    }
}

/// Writes one part of a key: bare when it is not empty and every byte may
/// be in a bare key, otherwise quoted as [`write_string`] quotes a string.
pub(super) fn write_key(out: &mut Out, name: &str) -> Result<(), EditErrorKind> {
    if !name.is_empty() && name.bytes().all(is_bare) {
        out.push(name)
    } else {
        write_string(out, name)
    }
}

/// Writes a dotted key, its parts joined by `.` without spaces.
pub(super) fn write_path<'n>(
    out: &mut Out,
    names: impl IntoIterator<Item = &'n str>,
) -> Result<(), EditErrorKind> {
    for (index, name) in names.into_iter().enumerate() {
        if index > 0 {
            out.push(".")?;
        }
        write_key(out, name)?;
    }
    Ok(())
}

/// Whether `c` is a control character TOML lets a string hold only as an
/// escape (a literal string not at all, but for tab): U+0000 to U+001F,
/// and U+007F.
fn is_control(c: char) -> bool {
    c <= '\u{1f}' || c == '\u{7f}'
}

/// Writes a string on one line, in TOML 1.0: a literal string when the
/// text holds a `\` or a `"`, which would need escapes, and nothing a
/// literal string cannot hold (a `'` or a control character); otherwise a
/// basic string, with 1.0's short escapes and `\uXXXX` for any other
/// control character. Never `\e`, `\xHH` or `\U`.
pub(super) fn write_string(out: &mut Out, text: &str) -> Result<(), EditErrorKind> {
    let literal =
        text.contains(['\\', '"']) && !text.contains('\'') && !text.chars().any(is_control);
    if literal {
        out.push("'")?;
        out.push(text)?;
        return out.push("'");
    }
    out.push("\"")?;
    let mut run_start = 0;
    for (at, c) in text.char_indices() {
        let short = match c {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\u{8}' => "\\b",
            '\t' => "\\t",
            '\n' => "\\n",
            '\u{c}' => "\\f",
            '\r' => "\\r",
            c if is_control(c) => "",
            _ => continue,
        };
        out.push(text.get(run_start..at).unwrap_or_default())?;
        if short.is_empty() {
            out.push(&format!("\\u{:04X}", u32::from(c)))?;
        } else {
            out.push(short)?;
        }
        // Every character escaped is ASCII, one byte.
        run_start = at + 1;
    }
    out.push(text.get(run_start..).unwrap_or_default())?;
    out.push("\"")
}

/// Writes a float: a finite one as [`Number`] writes it, which reads back
/// bit for bit, then `inf`, `-inf`, and `nan` or `-nan` by the sign bit.
pub(super) fn write_float(out: &mut Out, value: f64) -> Result<(), EditErrorKind> {
    match Number::from_f64(value) {
        Some(number) => out.push(&number.to_string()),
        None if value.is_nan() => out.push(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }),
        None => out.push(if value < 0.0 { "-inf" } else { "inf" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;
    use crate::toml::{Array, Date, Datetime, Offset, Table, Time, Value, parse};
    use tidepool_core::testing::check;

    /// Room enough for any test value.
    fn out() -> Out {
        Out::new(1 << 20, 1 << 20)
    }

    fn written(value: &ValueKind) -> String {
        let mut out = out();
        write_value(&mut out, value).expect("written");
        out.into_string()
    }

    /// Reads `text` back as the value of a key, through the parser.
    fn read_back(text: &str) -> ValueKind {
        let root = parse(&format!("k = {text}\n"), Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{text:?} does not read back: {error}"));
        root.get("k").expect("the key").kind.clone()
    }

    fn string(text: &str) -> String {
        written(&ValueKind::String(text.to_string()))
    }

    #[test]
    fn keys_are_bare_only_when_toml_allows() {
        for (name, expected) in [
            ("a-b_1", "a-b_1"),
            ("3", "3"),
            ("true", "true"),
            ("tab-size", "tab-size"),
            ("", "\"\""),
            ("a.b", "\"a.b\""),
            ("é", "\"é\""),
            ("a\"b", "'a\"b'"),
            ("a b", "\"a b\""),
        ] {
            let mut out = out();
            write_key(&mut out, name).expect("written");
            assert_eq!(out.as_str(), expected, "{name:?}");
            let root = parse(&format!("{expected} = 1\n"), Limits::DEFAULT).expect("a key");
            assert!(root.contains_key(name), "{name:?} reads back");
        }
        let mut out = out();
        write_path(&mut out, ["a", "b c", ""]).expect("written");
        assert_eq!(out.as_str(), "a.\"b c\".\"\"");
    }

    #[test]
    fn strings_use_only_toml_1_0_escapes() {
        for code in (0..0x20).chain([0x7f]) {
            let c = char::from_u32(code).expect("a char");
            let text = format!("a{c}b");
            let written = string(&text);
            assert!(
                !written.contains("\\e") && !written.contains("\\x"),
                "{written}"
            );
            assert!(!written.chars().any(is_control), "{code:#x} written raw");
            let expected = match c {
                '\u{8}' => "\"a\\bb\"".to_string(),
                '\t' => "\"a\\tb\"".to_string(),
                '\n' => "\"a\\nb\"".to_string(),
                '\u{c}' => "\"a\\fb\"".to_string(),
                '\r' => "\"a\\rb\"".to_string(),
                _ => format!("\"a\\u{code:04X}b\""),
            };
            assert_eq!(written, expected);
            assert_eq!(read_back(&written), ValueKind::String(text));
        }
        assert_eq!(string("\u{1b}"), "\"\\u001B\"");
        assert_eq!(string("\u{7f}"), "\"\\u007F\"");
        // Everything else is written as it is.
        assert_eq!(string("é€😀\u{feff}#"), "\"é€😀\u{feff}#\"");
        assert_eq!(string(""), "\"\"");
    }

    #[test]
    fn strings_are_literal_only_when_that_needs_no_escape() {
        for (text, expected) in [
            ("C:\\Users", "'C:\\Users'"),
            ("say \"hi\"", "'say \"hi\"'"),
            ("it's \"x\"", "\"it's \\\"x\\\"\""),
            ("a\tb", "\"a\\tb\""),
            ("a\\\tb", "\"a\\\\\\tb\""),
            ("plain", "\"plain\""),
            ("it's", "\"it's\""),
        ] {
            assert_eq!(string(text), expected, "{text:?}");
            assert_eq!(read_back(expected), ValueKind::String(text.to_string()));
        }
    }

    #[test]
    fn integers_are_decimal() {
        for value in [i64::MIN, i64::MAX, 0, -1, 7] {
            let text = written(&ValueKind::Integer(value));
            assert_eq!(text, value.to_string());
            assert_eq!(read_back(&text), ValueKind::Integer(value));
        }
    }

    /// The float `text` reads back as, by the parser.
    fn float_back(text: &str) -> f64 {
        match read_back(text) {
            ValueKind::Float(value) => value,
            other => panic!("{text:?} reads back as {other:?}"),
        }
    }

    #[test]
    fn floats_read_back_bit_for_bit() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (0.1, "0.1"),
            (3.0, "3.0"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e16"),
            (1e-5, "0.00001"),
            (1e-6, "1e-6"),
            (1.5e300, "1.5e300"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ] {
            let text = written(&ValueKind::Float(value));
            assert_eq!(text, expected);
            assert_eq!(float_back(&text).to_bits(), value.to_bits(), "{text}");
        }
        // A NaN keeps its sign, not its payload.
        assert_eq!(written(&ValueKind::Float(f64::NAN)), "nan");
        assert_eq!(written(&ValueKind::Float(-f64::NAN)), "-nan");
        assert!(float_back("-nan").is_sign_negative());
        check(
            "toml floats read back",
            10_000,
            |rng| f64::from_bits((u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())),
            |&value| {
                let text = written(&ValueKind::Float(value));
                let back = float_back(&text);
                let same = if value.is_nan() {
                    back.is_nan() && back.is_sign_negative() == value.is_sign_negative()
                } else {
                    back.to_bits() == value.to_bits()
                };
                if same {
                    Ok(())
                } else {
                    Err(format!(
                        "{value:?} is written {text:?}, which reads back as {back:?}"
                    ))
                }
            },
        );
    }

    #[test]
    fn datetimes_have_seconds_and_are_checked() {
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
        for (value, expected) in [
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Utc,
                },
                "1979-05-27T07:32:00Z",
            ),
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Minutes(0),
                },
                "1979-05-27T07:32:00+00:00",
            ),
            (
                Datetime::OffsetDatetime {
                    date,
                    time: Time {
                        nanosecond: 500_000_000,
                        ..time
                    },
                    offset: Offset::Minutes(-(23 * 60 + 59)),
                },
                "1979-05-27T07:32:00.5-23:59",
            ),
            (
                Datetime::LocalDatetime { date, time },
                "1979-05-27T07:32:00",
            ),
            (Datetime::LocalDate(date), "1979-05-27"),
            (Datetime::LocalTime(time), "07:32:00"),
        ] {
            let text = written(&ValueKind::Datetime(value));
            assert_eq!(text, expected);
            assert_eq!(read_back(&text), ValueKind::Datetime(value));
        }
        // `07:32` read without seconds is written with them.
        assert_eq!(written(&read_back("07:32")), "07:32:00");

        let bad_date = |date: Date| Datetime::LocalDate(date);
        let bad_time = |time: Time| Datetime::LocalTime(time);
        let offset = |minutes| Datetime::OffsetDatetime {
            date,
            time,
            offset: Offset::Minutes(minutes),
        };
        for (value, expected) in [
            (
                bad_date(Date {
                    year: 10_000,
                    ..date
                }),
                "a year from 0000 to 9999",
            ),
            (bad_date(Date { month: 0, ..date }), "a month from 01 to 12"),
            (
                bad_date(Date { month: 13, ..date }),
                "a month from 01 to 12",
            ),
            (
                bad_date(Date {
                    year: 1900,
                    month: 2,
                    day: 29,
                }),
                "a day that the month has",
            ),
            (
                bad_date(Date { day: 0, ..date }),
                "a day that the month has",
            ),
            (bad_time(Time { hour: 24, ..time }), "an hour from 00 to 23"),
            (
                bad_time(Time { minute: 60, ..time }),
                "a minute from 00 to 59",
            ),
            (
                bad_time(Time { second: 61, ..time }),
                "seconds from 00 to 60",
            ),
            (
                bad_time(Time {
                    nanosecond: 1_000_000_000,
                    ..time
                }),
                "a fraction of a second of 0 to 999999999 nanoseconds",
            ),
            (offset(1440), "an offset of 00 to 23 hours"),
            (offset(-1440), "an offset of 00 to 23 hours"),
            (offset(i16::MIN), "an offset of 00 to 23 hours"),
        ] {
            let mut out = out();
            assert_eq!(
                write_value(&mut out, &ValueKind::Datetime(value)),
                Err(EditErrorKind::InvalidDatetime { expected }),
                "{value:?}"
            );
            assert_eq!(out.as_str(), "", "nothing is written for {value:?}");
        }
        // The edges are in range: a leap second, and 23:59 either way.
        for value in [
            bad_time(Time { second: 60, ..time }),
            offset(1439),
            offset(-1439),
        ] {
            assert_eq!(
                read_back(&written(&ValueKind::Datetime(value))),
                ValueKind::Datetime(value)
            );
        }
    }

    #[test]
    fn arrays_and_inline_tables_are_one_line() {
        let int = |value| Value::from(ValueKind::Integer(value));
        let mut inner = Table::new_inline();
        inner.insert("x", int(1));
        inner.insert("a b", ValueKind::Array(Array::new(vec![int(2), int(3)])));
        let mut table = Table::new_inline();
        table.insert("t", ValueKind::Table(inner.clone()));
        table.insert("e", ValueKind::Table(Table::new_inline()));
        let mixed = ValueKind::Array(Array::new(vec![
            int(1),
            Value::from(ValueKind::String("s".into())),
            Value::from(ValueKind::Array(Array::new(Vec::new()))),
            Value::from(ValueKind::Table(inner.clone())),
        ]));
        for (value, expected) in [
            (ValueKind::Array(Array::new(Vec::new())), "[]"),
            (ValueKind::Table(Table::new_inline()), "{}"),
            (mixed, "[1, \"s\", [], { x = 1, \"a b\" = [2, 3] }]"),
            (
                ValueKind::Table(table),
                "{ t = { x = 1, \"a b\" = [2, 3] }, e = {} }",
            ),
        ] {
            let text = written(&value);
            assert_eq!(text, expected);
            assert!(!text.contains('\n'));
            // What reads back writes the same text again.
            assert_eq!(written(&read_back(&text)), text);
        }
        // An array of tables from a document is written as an array of
        // inline tables.
        let root = parse("[[a]]\nx = 1\n[[a]]\n[a.b]\n", Limits::DEFAULT).expect("parses");
        let array = &root.get("a").expect("a").kind;
        assert_eq!(written(array), "[{ x = 1 }, { b = {} }]");
    }

    #[test]
    fn the_budget_stops_the_writer() {
        let big = ValueKind::String("x".repeat(1 << 20));
        let mut out = Out::new(1024, 4096);
        assert_eq!(
            write_value(&mut out, &big),
            Err(EditErrorKind::TooLarge { limit: 4096 })
        );
        assert!(out.as_str().len() <= 1024);
        assert!(out.text.capacity() <= 1024, "{}", out.text.capacity());
        // Exactly the room fits.
        let mut out = Out::new(4, 4);
        assert_eq!(
            write_value(&mut out, &ValueKind::String("ab".into())),
            Ok(())
        );
        assert_eq!(out.as_str(), "\"ab\"");
        assert_eq!(out.push("x"), Err(EditErrorKind::TooLarge { limit: 4 }));
    }

    #[test]
    fn a_value_past_max_depth_is_refused_before_writing() {
        let mut value = ValueKind::Integer(1);
        for _ in 0..10 {
            value = ValueKind::Array(Array::new(vec![Value::from(value)]));
        }
        assert_eq!(value_depth(&value, 10), Some(10));
        assert_eq!(value_depth(&value, 9), None);
        assert_eq!(value_depth(&ValueKind::Integer(1), 0), Some(0));
        assert_eq!(value_depth(&ValueKind::Table(Table::new_inline()), 0), None);
        // A value far deeper than the stack could recurse over is measured
        // all the same, and taken apart without recursing.
        let mut deep = ValueKind::Integer(1);
        for _ in 0..100_000 {
            deep = ValueKind::Array(Array::new(vec![Value::from(deep)]));
        }
        assert_eq!(value_depth(&deep, Limits::DEFAULT.max_depth), None);
        let mut work = vec![deep];
        while let Some(value) = work.pop() {
            if let ValueKind::Array(array) = value {
                work.extend(array.items.into_iter().map(|item| item.kind));
            }
        }
    }
}
