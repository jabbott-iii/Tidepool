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

//! Unit tests for the parser: hand-made vectors for each kind of value,
//! TOML 1.1's additions, the rules on defining tables, spans, error
//! positions, every limit, failing without recursing, and the cost of large
//! tables. toml-test's cases and the property tests are in `tests/toml.rs`.

use std::time::{Duration, Instant};

use super::scalar::*;
use super::*;
use crate::toml::{Date, Datetime, Offset, Time};
use ErrorKind::{
    BareCarriageReturn, ControlCharacter, DuplicateKey, DuplicateTable, ExtendsDefinedTable,
    ExtendsInlineTable, FloatOutOfRange, InputTooLarge, IntegerOutOfRange, InvalidDatetime,
    InvalidEscape, InvalidLiteral, InvalidUnicodeEscape, InvalidUtf8, LeadingZero,
    MisplacedUnderscore, MultilineKey, NotATable, NotAnArrayOfTables, NumberTooLong, StringTooLong,
    TooDeep, TooManyEntries, UnexpectedCharacter, UnexpectedEnd,
};

/// Line, column and kind of an error.
type Position = (usize, usize, ErrorKind);

const DEFAULT: Limits = Limits::DEFAULT;
const DEFAULT_DEPTH: usize = Limits::DEFAULT.max_depth;

fn ok_with(text: &str, limits: Limits) -> Table {
    parse(text, limits).unwrap_or_else(|error| panic!("{text:?} should parse, but: {error}"))
}

fn ok(text: &str) -> Table {
    ok_with(text, DEFAULT)
}

/// The line, column and kind of the error for `text`. Also checks that the
/// byte offset is consistent with them, and that bytes fail the same way.
fn err_with(text: &str, limits: Limits) -> Position {
    let result = parse(text, limits);
    assert_eq!(
        parse_bytes(text.as_bytes(), limits),
        result,
        "text and bytes disagree about {text:?}"
    );
    match result {
        Err(CodecError::Toml {
            offset,
            line,
            column,
            kind,
        }) => {
            let before = text
                .get(..offset)
                .unwrap_or_else(|| panic!("{text:?}: offset {offset} is not a character boundary"));
            let before = before.strip_prefix('\u{feff}').unwrap_or(before);
            assert_eq!(line, before.matches('\n').count() + 1, "line of {text:?}");
            let line_text = before.rsplit('\n').next().unwrap_or(before);
            assert_eq!(column, line_text.chars().count() + 1, "column of {text:?}");
            (line, column, kind)
        }
        Err(other) => panic!("{text:?}: not a TOML error: {other}"),
        Ok(table) => panic!("{text:?} should not parse, but gave {table:?}"),
    }
}

fn err(text: &str) -> Position {
    err_with(text, DEFAULT)
}

/// The kind of the value `text` is, written as `v = {text}`.
fn kind_of(text: &str) -> ValueKind {
    let document = format!("v = {text}");
    ok(&document)
        .get("v")
        .map(|value| value.kind.clone())
        .unwrap_or_else(|| panic!("no value in {document:?}"))
}

/// The string `text` is, written as `v = {text}`.
fn string(text: &str) -> String {
    match kind_of(text) {
        ValueKind::String(value) => value,
        other => panic!("{text} is {other:?}, not a string"),
    }
}

/// The error for `v = {text}`: its column on the first line, and kind.
fn value_err(text: &str) -> (usize, ErrorKind) {
    let (line, column, kind) = err(&format!("v = {text}"));
    assert_eq!(line, 1, "{text:?}");
    (column, kind)
}

/// The value at `path`, keys separated by `.`, from `table`.
fn get<'a>(table: &'a Table, path: &str) -> &'a Value {
    let mut parts = path.split('.');
    let first = parts.next().unwrap_or_default();
    let mut value = table
        .get(first)
        .unwrap_or_else(|| panic!("no {first:?} in {table:?}"));
    for part in parts {
        value = value
            .get(part)
            .unwrap_or_else(|| panic!("no {part:?} in {path:?}"));
    }
    value
}

/// The kinds of the tables along `path`.
fn table_kind(table: &Table, path: &str) -> TableKind {
    get(table, path)
        .as_table()
        .map(Table::kind)
        .expect("a table")
}

fn names(table: &Table) -> Vec<&str> {
    table
        .entries()
        .iter()
        .map(|(key, _)| key.name.as_str())
        .collect()
}

fn found(found: char, expected: &'static str) -> ErrorKind {
    UnexpectedCharacter { found, expected }
}

fn end(expected: &'static str) -> ErrorKind {
    UnexpectedEnd { expected }
}

fn bad_datetime(expected: &'static str) -> ErrorKind {
    InvalidDatetime { expected }
}

fn date(year: u16, month: u8, day: u8) -> Date {
    Date { year, month, day }
}

fn time(hour: u8, minute: u8, second: u8, nanosecond: u32) -> Time {
    Time {
        hour,
        minute,
        second,
        nanosecond,
    }
}

fn datetime(text: &str) -> Datetime {
    match kind_of(text) {
        ValueKind::Datetime(value) => value,
        other => panic!("{text} is {other:?}, not a date-time"),
    }
}

/// The text a span covers.
fn text_of(text: &str, span: Span) -> &str {
    text.get(span.range())
        .unwrap_or_else(|| panic!("{span:?} is not in {text:?}"))
}

// ---------------------------------------------------------------- strings

#[test]
fn basic_strings_decode_their_escapes() {
    assert_eq!(string(r#""""#), "");
    assert_eq!(string(r#""plain text""#), "plain text");
    assert_eq!(string(r#""tab\there""#), "tab\there");
    assert_eq!(string(r#""\b\t\n\f\r\"\\""#), "\u{8}\t\n\u{c}\r\"\\");
    assert_eq!(string(r#""\u00e9\u20AC\U0001F600""#), "é€😀");
    assert_eq!(string(r#""\U0010FFFF\u0000""#), "\u{10ffff}\0");
    // Raw characters, a raw tab, and a byte-order mark, which inside a
    // string is an ordinary character.
    assert_eq!(string("\"é€😀\t\u{feff}\""), "é€😀\t\u{feff}");
    // `#` inside a string is not a comment.
    assert_eq!(string(r##""# not a comment""##), "# not a comment");
}

#[test]
fn toml_1_1_adds_hex_and_escape_escapes() {
    assert_eq!(string(r#""\x41\x7a\xe9\xFF""#), "Azéÿ");
    assert_eq!(string(r#""\x00\x7F""#), "\0\u{7f}");
    assert_eq!(string(r#""\e[1m""#), "\u{1b}[1m");
    assert_eq!(string("\"\"\"\\x41\\e\"\"\""), "A\u{1b}");
    // Two hex digits, no more and no fewer.
    assert_eq!(value_err(r#""\x4""#), (6, InvalidEscape));
    assert_eq!(value_err(r#""\xG1""#), (6, InvalidEscape));
    assert_eq!(string(r#""\x411""#), "A1");
    // A literal string has no escapes at all.
    assert_eq!(string(r"'\x41\e'"), r"\x41\e");
}

#[test]
fn literal_strings_are_as_written() {
    assert_eq!(string("''"), "");
    assert_eq!(
        string(r"'C:\Users\nodejs\templates'"),
        r"C:\Users\nodejs\templates"
    );
    assert_eq!(
        string(r#"'Tom "Dubs" Preston-Werner'"#),
        r#"Tom "Dubs" Preston-Werner"#
    );
    assert_eq!(string("'<\\i\\c*\\s*>'"), "<\\i\\c*\\s*>");
    assert_eq!(string("'tab\there'"), "tab\there");
}

#[test]
fn multi_line_basic_strings() {
    // The line break right after the opening quotes is dropped, LF or CR LF.
    assert_eq!(string("\"\"\"\nRoses\nViolets\"\"\""), "Roses\nViolets");
    assert_eq!(string("\"\"\"\r\nRoses\r\nViolets\"\"\""), "Roses\nViolets");
    // Only one.
    assert_eq!(string("\"\"\"\n\nx\"\"\""), "\nx");
    // A CR LF inside becomes LF; an escaped one stays.
    assert_eq!(string("\"\"\"a\r\nb\\r\\nc\"\"\""), "a\nb\r\nc");
    // A line-ending backslash trims the line break and the whitespace and
    // line breaks after it, also when whitespace comes before the break.
    assert_eq!(
        string("\"\"\"\nThe quick brown \\\n\n\n  fox jumps over \\   \r\n    the lazy dog.\"\"\""),
        "The quick brown fox jumps over the lazy dog."
    );
    assert_eq!(
        string("\"\"\"\\\n       The quick \\\n       fox.\\\n       \"\"\""),
        "The quick fox."
    );
    // One or two quotes anywhere, also just before the closing three.
    assert_eq!(
        string(r#""""Here are two quotation marks: "". Simple enough.""""#),
        "Here are two quotation marks: \"\". Simple enough."
    );
    assert_eq!(string(r#"""""This," she said.""""#), "\"This,\" she said.");
    assert_eq!(string("\"\"\"x\"\"\"\""), "x\"");
    assert_eq!(string("\"\"\"x\"\"\"\"\""), "x\"\"");
    assert_eq!(string(r#""""""""#), "");
    assert_eq!(string(r#""""Three: ""\".""""#), "Three: \"\"\".");
    // Six quotes at the end are two and the end, then a stray quote.
    assert_eq!(value_err("\"\"\"x\"\"\"\"\"\""), (14, found('"', LINE_END)));
    // A backslash then whitespace that is not at the end of the line is not
    // an escape.
    assert_eq!(value_err("\"\"\"a\\ b\"\"\""), (9, InvalidEscape));
}

#[test]
fn multi_line_literal_strings() {
    assert_eq!(
        string("'''\nThe first newline is\ntrimmed.'''"),
        "The first newline is\ntrimmed."
    );
    assert_eq!(string("'''\r\na\r\nb'''"), "a\nb");
    assert_eq!(
        string(r"'''I [dw]on't need \d{2} apples'''"),
        r"I [dw]on't need \d{2} apples"
    );
    assert_eq!(string("'''a \\\n b'''"), "a \\\n b");
    assert_eq!(string("''''That,' she said.''''"), "'That,' she said.'");
    assert_eq!(string("'''x'''''"), "x''");
    assert_eq!(string("''''''"), "");
    assert_eq!(value_err("'''x''''''"), (14, found('\'', LINE_END)));
}

#[test]
fn strings_that_do_not_end_are_reported_where_they_start() {
    assert_eq!(value_err("\"abc"), (5, end(BASIC_CLOSE)));
    assert_eq!(value_err("'abc"), (5, end(LITERAL_CLOSE)));
    assert_eq!(value_err("\"\"\"abc\n"), (5, end(MULTILINE_BASIC_CLOSE)));
    assert_eq!(value_err("'''abc\n''"), (5, end(MULTILINE_LITERAL_CLOSE)));
    assert_eq!(value_err("\"ab\\"), (8, end(ESCAPE_REST)));
    assert_eq!(value_err("\"\\u12"), (6, end(HEX_DIGITS)));
    // A single-line string ends at the end of its line.
    assert_eq!(
        value_err("\"abc\ndef\""),
        (9, found('\n', BASIC_CLOSE_ON_LINE))
    );
    assert_eq!(
        value_err("\"abc\r\ndef\""),
        (9, found('\r', BASIC_CLOSE_ON_LINE))
    );
    assert_eq!(
        value_err("'abc\ndef'"),
        (9, found('\n', LITERAL_CLOSE_ON_LINE))
    );
}

#[test]
fn strings_reject_bad_escapes_and_control_characters() {
    // Escapes that TOML does not have, JSON's `\/` among them.
    for escape in [r"\a", r"\/", r"\ ", r"\0", r"\'", r"\é"] {
        assert_eq!(
            value_err(&format!("\"x{escape}\"")),
            (7, InvalidEscape),
            "{escape}"
        );
    }
    // Escapes that are not Unicode scalar values.
    assert_eq!(value_err(r#""\uD800""#), (6, InvalidUnicodeEscape));
    assert_eq!(value_err(r#""\uDFFF""#), (6, InvalidUnicodeEscape));
    assert_eq!(value_err(r#""\U00110000""#), (6, InvalidUnicodeEscape));
    assert_eq!(value_err(r#""\UFFFFFFFF""#), (6, InvalidUnicodeEscape));
    assert_eq!(value_err(r#""\u12G4""#), (6, InvalidEscape));
    // Raw control characters, other than tab, in every kind of string.
    for (open, close) in [
        ("\"", "\""),
        ("'", "'"),
        ("\"\"\"", "\"\"\""),
        ("'''", "'''"),
    ] {
        for control in [
            '\0', '\u{8}', '\u{b}', '\u{c}', '\u{1b}', '\u{1f}', '\u{7f}',
        ] {
            let text = format!("{open}a{control}b{close}");
            let column = 5 + open.len() + 1;
            assert_eq!(
                value_err(&text),
                (column, ControlCharacter { found: control }),
                "{text:?}"
            );
        }
        let text = format!("{open}a\rb{close}");
        assert_eq!(
            value_err(&text),
            (5 + open.len() + 1, BareCarriageReturn),
            "{text:?}"
        );
    }
}

// ---------------------------------------------------------------- numbers

#[test]
fn integers() {
    let int = |text| match kind_of(text) {
        ValueKind::Integer(value) => value,
        other => panic!("{text} is {other:?}"),
    };
    assert_eq!(int("0"), 0);
    assert_eq!(int("+0"), 0);
    assert_eq!(int("-0"), 0);
    assert_eq!(int("+99"), 99);
    assert_eq!(int("-17"), -17);
    assert_eq!(int("1_000"), 1000);
    assert_eq!(int("5_349_221"), 5_349_221);
    assert_eq!(int("1_2_3_4_5"), 12345);
    assert_eq!(int("9223372036854775807"), i64::MAX);
    assert_eq!(int("-9223372036854775808"), i64::MIN);
    assert_eq!(int("0xDEADBEEF"), 0xDEAD_BEEF);
    assert_eq!(int("0xdead_beef"), 0xDEAD_BEEF);
    assert_eq!(int("0x00987"), 0x987);
    assert_eq!(int("0o01234567"), 0o1234567);
    assert_eq!(int("0o755"), 0o755);
    assert_eq!(int("0b11010110"), 0b1101_0110);
    assert_eq!(int("0x7FFFFFFFFFFFFFFF"), i64::MAX);
    assert_eq!(int("0o777777777777777777777"), i64::MAX);
    assert_eq!(int(&format!("0b{}", "1".repeat(63))), i64::MAX);
}

#[test]
fn integers_outside_i64_are_rejected() {
    assert_eq!(value_err("9223372036854775808"), (5, IntegerOutOfRange));
    assert_eq!(value_err("-9223372036854775809"), (5, IntegerOutOfRange));
    assert_eq!(value_err("99999999999999999999999"), (5, IntegerOutOfRange));
    assert_eq!(value_err("0x8000000000000000"), (5, IntegerOutOfRange));
    assert_eq!(
        value_err("0o1000000000000000000000"),
        (5, IntegerOutOfRange)
    );
    assert_eq!(
        value_err(&format!("0b1{}", "0".repeat(63))),
        (5, IntegerOutOfRange)
    );
}

#[test]
fn malformed_integers_are_rejected() {
    assert_eq!(value_err("01"), (5, LeadingZero));
    assert_eq!(value_err("-01"), (5, LeadingZero));
    assert_eq!(value_err("+00"), (5, LeadingZero));
    assert_eq!(value_err("0_1"), (5, LeadingZero));
    assert_eq!(value_err("1__2"), (6, MisplacedUnderscore));
    assert_eq!(value_err("1_"), (6, MisplacedUnderscore));
    assert_eq!(value_err("_1"), (5, found('_', VALUE)));
    assert_eq!(value_err("+_1"), (6, MisplacedUnderscore));
    assert_eq!(value_err("0x"), (7, end(HEX_DIGIT)));
    assert_eq!(value_err("0x_1"), (7, MisplacedUnderscore));
    assert_eq!(value_err("0xg"), (7, found('g', HEX_DIGIT)));
    assert_eq!(value_err("0o8"), (7, found('8', OCTAL_DIGIT)));
    assert_eq!(value_err("0b2"), (7, found('2', BINARY_DIGIT)));
    assert_eq!(value_err("0b1_"), (8, MisplacedUnderscore));
    // The prefixes are lowercase, and take no sign.
    assert_eq!(value_err("0X1"), (6, found('X', LINE_END)));
    assert_eq!(value_err("+0x1"), (7, found('x', LINE_END)));
    assert_eq!(value_err("-0b1"), (7, found('b', LINE_END)));
    assert_eq!(value_err("1 2"), (7, found('2', LINE_END)));
}

#[test]
fn floats() {
    let float = |text| match kind_of(text) {
        ValueKind::Float(value) => value,
        other => panic!("{text} is {other:?}"),
    };
    assert_eq!(float("+1.0"), 1.0);
    assert_eq!(float("3.25"), 3.25);
    assert_eq!(float("-0.01"), -0.01);
    assert_eq!(float("5e+22"), 5e22);
    assert_eq!(float("1e06"), 1e6);
    assert_eq!(float("-2E-2"), -0.02);
    assert_eq!(float("6.626e-34"), 6.626e-34);
    assert_eq!(float("224_617.445_991_228"), 224_617.445_991_228);
    assert_eq!(float("3e1_4"), 3e14);
    assert_eq!(float("0.1"), 0.1);
    // Read correctly rounded, like Rust's own literals.
    assert_eq!(float("9007199254740993.0"), 9_007_199_254_740_992.0);
    // The largest subnormal, which takes the most digits to read.
    assert_eq!(
        float("2.2250738585072011e-308"),
        f64::from_bits(0x000F_FFFF_FFFF_FFFF)
    );
    assert_eq!(float("1.7976931348623157e308"), f64::MAX);
    assert_eq!(float("5e-324"), 5e-324);
    // Too small for an `f64` is zero, which is still a float.
    assert_eq!(float("1e-400"), 0.0);
    // A float is not an integer, even when it is a whole number.
    assert_eq!(kind_of("2.0"), ValueKind::Float(2.0));
    assert_ne!(kind_of("2.0"), ValueKind::Integer(2));
}

#[test]
fn special_floats_and_signed_zeros() {
    let float = |text| match kind_of(text) {
        ValueKind::Float(value) => value,
        other => panic!("{text} is {other:?}"),
    };
    assert_eq!(float("inf"), f64::INFINITY);
    assert_eq!(float("+inf"), f64::INFINITY);
    assert_eq!(float("-inf"), f64::NEG_INFINITY);
    assert!(float("nan").is_nan());
    assert!(float("+nan").is_nan());
    assert!(float("nan").is_sign_positive());
    assert!(float("-nan").is_nan() && float("-nan").is_sign_negative());
    for (text, negative) in [
        ("0.0", false),
        ("+0.0", false),
        ("-0.0", true),
        ("0e0", false),
        ("-0e0", true),
        ("-0E00", true),
    ] {
        let zero = float(text);
        assert_eq!(zero, 0.0, "{text}");
        assert_eq!(zero.is_sign_negative(), negative, "{text}");
    }
}

#[test]
fn floats_too_large_for_f64_are_rejected() {
    assert_eq!(value_err("1e400"), (5, FloatOutOfRange));
    assert_eq!(value_err("-1e400"), (5, FloatOutOfRange));
    assert_eq!(value_err("1.8e308"), (5, FloatOutOfRange));
    assert_eq!(
        value_err(&format!("1{}.0", "0".repeat(400))),
        (5, FloatOutOfRange)
    );
}

#[test]
fn malformed_floats_are_rejected() {
    assert_eq!(value_err("1."), (7, end(DIGIT)));
    assert_eq!(value_err("1.\n"), (7, found('\n', DIGIT)));
    assert_eq!(value_err(".5"), (5, found('.', VALUE)));
    assert_eq!(value_err("-.5"), (6, found('.', DIGIT)));
    assert_eq!(value_err("1.e5"), (7, found('e', DIGIT)));
    assert_eq!(value_err("1e"), (7, end(DIGIT)));
    assert_eq!(value_err("1e+"), (8, end(DIGIT)));
    assert_eq!(value_err("1e_5"), (7, MisplacedUnderscore));
    assert_eq!(value_err("1_.5"), (6, MisplacedUnderscore));
    assert_eq!(value_err("1._5"), (7, MisplacedUnderscore));
    assert_eq!(value_err("1.5_"), (8, MisplacedUnderscore));
    assert_eq!(value_err("01.5"), (5, LeadingZero));
    assert_eq!(value_err("1.5.3"), (8, found('.', LINE_END)));
    assert_eq!(value_err("1e5.3"), (8, found('.', LINE_END)));
    assert_eq!(value_err("1e5e3"), (8, found('e', LINE_END)));
    // The special floats are lowercase words.
    assert_eq!(value_err("Inf"), (5, found('I', VALUE)));
    assert_eq!(value_err("NaN"), (5, found('N', VALUE)));
    assert_eq!(value_err("infinity"), (8, found('i', LINE_END)));
    assert_eq!(value_err("in"), (5, InvalidLiteral));
    assert_eq!(value_err("-na"), (5, InvalidLiteral));
    assert_eq!(value_err("inf_"), (8, found('_', LINE_END)));
}

#[test]
fn booleans_and_bare_words() {
    assert_eq!(kind_of("true"), ValueKind::Boolean(true));
    assert_eq!(kind_of("false"), ValueKind::Boolean(false));
    assert_eq!(value_err("tru"), (5, InvalidLiteral));
    assert_eq!(value_err("fals"), (5, InvalidLiteral));
    assert_eq!(value_err("nope"), (5, InvalidLiteral));
    assert_eq!(value_err("True"), (5, found('T', VALUE)));
    assert_eq!(value_err("yes"), (5, found('y', VALUE)));
    assert_eq!(value_err("truex"), (9, found('x', LINE_END)));
    assert_eq!(value_err("true false"), (10, found('f', LINE_END)));
}

// ------------------------------------------------------------- date-times

#[test]
fn the_four_kinds_of_date_time() {
    let utc = |date, time| Datetime::OffsetDatetime {
        date,
        time,
        offset: Offset::Utc,
    };
    let at = |date, time, minutes| Datetime::OffsetDatetime {
        date,
        time,
        offset: Offset::Minutes(minutes),
    };
    let day = date(1979, 5, 27);
    assert_eq!(
        datetime("1979-05-27T07:32:00Z"),
        utc(day, time(7, 32, 0, 0))
    );
    // `T`, `t` or a space between date and time; `Z` or `z`.
    assert_eq!(
        datetime("1979-05-27t07:32:00z"),
        utc(day, time(7, 32, 0, 0))
    );
    assert_eq!(
        datetime("1979-05-27 07:32:00Z"),
        utc(day, time(7, 32, 0, 0))
    );
    assert_eq!(
        datetime("1979-05-27T00:32:00-07:00"),
        at(day, time(0, 32, 0, 0), -420)
    );
    assert_eq!(
        datetime("1979-05-27T00:32:00.999999+05:30"),
        at(day, time(0, 32, 0, 999_999_000), 330)
    );
    assert_eq!(
        datetime("1979-05-27T00:32:00+00:00"),
        at(day, time(0, 32, 0, 0), 0)
    );
    assert_eq!(
        datetime("1979-05-27T00:32:00-00:00"),
        at(day, time(0, 32, 0, 0), 0)
    );
    assert_eq!(
        datetime("1979-05-27T07:32:00.5"),
        Datetime::LocalDatetime {
            date: day,
            time: time(7, 32, 0, 500_000_000)
        }
    );
    assert_eq!(
        datetime("1979-05-27 07:32:00"),
        Datetime::LocalDatetime {
            date: day,
            time: time(7, 32, 0, 0)
        }
    );
    assert_eq!(datetime("1979-05-27"), Datetime::LocalDate(day));
    assert_eq!(datetime("07:32:00"), Datetime::LocalTime(time(7, 32, 0, 0)));
    assert_eq!(
        datetime("00:32:00.999999"),
        Datetime::LocalTime(time(0, 32, 0, 999_999_000))
    );
    // A space ends a date unless a time follows it.
    let root = ok("d = 1979-05-27 # a date\n");
    assert_eq!(
        get(&root, "d").as_datetime(),
        Some(&Datetime::LocalDate(day))
    );
    assert_eq!(value_err("1979-05-27 07"), (16, found('0', LINE_END)));
}

#[test]
fn toml_1_1_lets_seconds_be_left_out() {
    assert_eq!(datetime("07:32"), Datetime::LocalTime(time(7, 32, 0, 0)));
    assert_eq!(
        datetime("1979-05-27T07:32"),
        Datetime::LocalDatetime {
            date: date(1979, 5, 27),
            time: time(7, 32, 0, 0)
        }
    );
    assert_eq!(
        datetime("1979-05-27 07:32Z"),
        Datetime::OffsetDatetime {
            date: date(1979, 5, 27),
            time: time(7, 32, 0, 0),
            offset: Offset::Utc
        }
    );
    assert_eq!(
        datetime("1979-05-27 07:32-07:00"),
        Datetime::OffsetDatetime {
            date: date(1979, 5, 27),
            time: time(7, 32, 0, 0),
            offset: Offset::Minutes(-420)
        }
    );
    // A fraction needs the seconds before it.
    assert_eq!(value_err("07:32.5"), (10, found('.', LINE_END)));
}

#[test]
fn fractions_of_a_second_keep_nine_digits_and_drop_the_rest() {
    let fraction = |text: &str| match datetime(&format!("00:00:00.{text}")) {
        Datetime::LocalTime(time) => time.nanosecond,
        other => panic!("{other:?}"),
    };
    assert_eq!(fraction("5"), 500_000_000);
    assert_eq!(fraction("123"), 123_000_000);
    assert_eq!(fraction("000000001"), 1);
    assert_eq!(fraction("123456789"), 123_456_789);
    // Dropped, not rounded.
    assert_eq!(fraction("1234567899"), 123_456_789);
    assert_eq!(fraction("9999999999999"), 999_999_999);
    assert_eq!(fraction("0000000009"), 0);
}

#[test]
fn date_time_fields_are_checked() {
    // Leap years: every fourth, but centuries only every 400 years.
    for text in [
        "2024-02-29",
        "2000-02-29",
        "1600-02-29",
        "0000-02-29",
        "2023-02-28",
    ] {
        assert!(matches!(datetime(text), Datetime::LocalDate(_)), "{text}");
    }
    for text in ["2023-02-29", "1900-02-29", "2100-02-29", "2024-02-30"] {
        assert_eq!(value_err(text), (13, bad_datetime(DAY_RANGE)), "{text}");
    }
    // Days per month.
    for (month, days) in [
        (1, 31),
        (3, 31),
        (4, 30),
        (6, 30),
        (9, 30),
        (11, 30),
        (12, 31),
    ] {
        assert!(matches!(
            datetime(&format!("2023-{month:02}-{days:02}")),
            Datetime::LocalDate(_)
        ));
        let text = format!("2023-{month:02}-{:02}", days + 1);
        assert_eq!(value_err(&text), (13, bad_datetime(DAY_RANGE)), "{text}");
    }
    assert_eq!(value_err("2023-01-00"), (13, bad_datetime(DAY_RANGE)));
    assert_eq!(value_err("2023-00-01"), (10, bad_datetime(MONTH_RANGE)));
    assert_eq!(value_err("2023-13-01"), (10, bad_datetime(MONTH_RANGE)));
    assert!(matches!(datetime("0000-01-01"), Datetime::LocalDate(_)));
    assert!(matches!(datetime("9999-12-31"), Datetime::LocalDate(_)));
    // Hours, minutes and seconds; 60 seconds is a leap second.
    assert!(matches!(datetime("23:59:59"), Datetime::LocalTime(_)));
    assert_eq!(
        datetime("23:59:60.999"),
        Datetime::LocalTime(time(23, 59, 60, 999_000_000))
    );
    assert_eq!(value_err("24:00:00"), (5, bad_datetime(HOUR_RANGE)));
    assert_eq!(value_err("23:60:00"), (8, bad_datetime(MINUTE_RANGE)));
    assert_eq!(value_err("23:59:61"), (11, bad_datetime(SECOND_RANGE)));
    assert_eq!(
        value_err("2023-01-01T24:00"),
        (16, bad_datetime(HOUR_RANGE))
    );
    // Offsets: 00 to 23 hours and 00 to 59 minutes, either way.
    assert_eq!(
        datetime("2023-01-01T00:00:00+23:59"),
        Datetime::OffsetDatetime {
            date: date(2023, 1, 1),
            time: time(0, 0, 0, 0),
            offset: Offset::Minutes(1439)
        }
    );
    assert!(matches!(
        datetime("2023-01-01T00:00:00-23:59"),
        Datetime::OffsetDatetime {
            offset: Offset::Minutes(-1439),
            ..
        }
    ));
    assert_eq!(
        value_err("2023-01-01T00:00:00+24:00"),
        (25, bad_datetime(OFFSET_HOUR_RANGE))
    );
    assert_eq!(
        value_err("2023-01-01T00:00:00-12:60"),
        (28, bad_datetime(OFFSET_MINUTE_RANGE))
    );
}

#[test]
fn malformed_date_times_are_rejected() {
    assert_eq!(value_err("1979-5-27"), (11, found('-', MONTH)));
    assert_eq!(value_err("1979-05-7\n"), (14, found('\n', DAY)));
    assert_eq!(value_err("1979-05/27"), (12, found('/', DAY_DASH)));
    assert_eq!(value_err("1979-05-27T"), (16, end(HOUR)));
    assert_eq!(value_err("1979-05-27T7:32"), (17, found(':', HOUR)));
    assert_eq!(value_err("07:3"), (9, end(MINUTE)));
    // Not a time without its `:`, so a number with a leading zero.
    assert_eq!(value_err("07-32"), (5, LeadingZero));
    assert_eq!(value_err("07:32:0"), (12, end(SECOND)));
    assert_eq!(value_err("07:32:00."), (14, end(FRACTION)));
    assert_eq!(
        value_err("1979-05-27T07:32:00+0700"),
        (27, found('0', OFFSET_COLON))
    );
    assert_eq!(value_err("1979-05-27T07:32:00+07"), (27, end(OFFSET_COLON)));
    assert_eq!(
        value_err("1979-05-27T07:32:00+07:0"),
        (29, end(OFFSET_MINUTE))
    );
    // A time alone has no offset, and a date ends where a time does not
    // follow it.
    assert_eq!(value_err("07:32:00Z"), (13, found('Z', LINE_END)));
    assert_eq!(value_err("1979-05-27x"), (15, found('x', LINE_END)));
    assert_eq!(value_err("1979-05-2717:45:00"), (15, found('1', LINE_END)));
    // Five-digit years are not dates.
    assert_eq!(value_err("10000-01-01"), (10, found('-', LINE_END)));
}

// ----------------------------------------------------------------- tables

#[test]
fn keys() {
    let root = ok("bare_key-1 = 1\n1234 = 2\n\"127.0.0.1\" = 3\n\"ʎǝʞ\" = 4\n\
                   'quoted \"value\"' = 5\n\"\" = 6\n\"a\\tb\\u00e9\" = 7\n");
    assert_eq!(
        names(&root),
        [
            "bare_key-1",
            "1234",
            "127.0.0.1",
            "ʎǝʞ",
            "quoted \"value\"",
            "",
            "a\tbé"
        ]
    );
    // A multi-line string is never a key, in a key/value pair or a header.
    assert_eq!(err("\"\"\"key\"\"\" = 1"), (1, 1, MultilineKey));
    assert_eq!(err("'''key''' = 1"), (1, 1, MultilineKey));
    assert_eq!(err("[\"\"\"key\"\"\"]"), (1, 2, MultilineKey));
    assert_eq!(err("t = {'''k''' = 1}"), (1, 6, MultilineKey));
    // Bare keys are ASCII; other characters need quotes.
    assert_eq!(err("é = 1"), (1, 1, found('é', EXPRESSION)));
    assert_eq!(err("a$b = 1"), (1, 2, found('$', EQUALS)));
    assert_eq!(err("= 1"), (1, 1, found('=', EXPRESSION)));
    assert_eq!(err("a. = 1"), (1, 4, found('=', KEY_AFTER_DOT)));
    assert_eq!(err(".a = 1"), (1, 1, found('.', EXPRESSION)));
    assert_eq!(err("a..b = 1"), (1, 3, found('.', KEY_AFTER_DOT)));
    assert_eq!(err("a"), (1, 2, end(EQUALS)));
    assert_eq!(err("a ="), (1, 4, end(VALUE)));
}

#[test]
fn headers_define_tables() {
    let root = ok("[a]\nx = 1\n[a.b]\ny = 2\n[ c . \"d.e\" . 'f' ]\nz = 3\n[\"\"]\n");
    assert_eq!(names(&root), ["a", "c", ""]);
    assert_eq!(get(&root, "a.x").as_integer(), Some(1));
    assert_eq!(get(&root, "a.b.y").as_integer(), Some(2));
    let z = get(&root, "c")
        .get("d.e")
        .and_then(|e| e.get("f"))
        .and_then(|f| f.get("z"))
        .and_then(Value::as_integer);
    assert_eq!(z, Some(3));
    assert_eq!(table_kind(&root, "a"), TableKind::Header);
    assert_eq!(table_kind(&root, ""), TableKind::Header);
    // Keys that look like other things are strings.
    let root = ok("[true]\n[1.2]\n[inf]\n");
    assert_eq!(names(&root), ["true", "1", "inf"]);
    assert_eq!(table_kind(&root, "1.2"), TableKind::Header);
}

#[test]
fn super_tables_may_be_defined_after_their_sub_tables() {
    let root = ok("[x.y.z.w]\na = 1\n[x]\nb = 2\n");
    assert_eq!(table_kind(&root, "x"), TableKind::Header);
    assert_eq!(table_kind(&root, "x.y"), TableKind::Implicit);
    assert_eq!(table_kind(&root, "x.y.z"), TableKind::Implicit);
    assert_eq!(table_kind(&root, "x.y.z.w"), TableKind::Header);
    // The entries keep the order the keys first appear in.
    assert_eq!(names(get(&root, "x").as_table().expect("x")), ["y", "b"]);
}

#[test]
fn arrays_of_tables() {
    let text = "[[fruits]]\nname = \"apple\"\n[fruits.physical]\ncolor = \"red\"\n\
                [[fruits.varieties]]\nname = \"red delicious\"\n[[fruits.varieties]]\n\
                name = \"granny smith\"\n[[fruits]]\nname = \"banana\"\n[[fruits.varieties]]\n\
                name = \"plantain\"\n";
    let root = ok(text);
    let fruits = get(&root, "fruits").as_array().expect("an array");
    assert!(fruits.is_array_of_tables());
    assert_eq!(fruits.len(), 2);
    let apple = fruits.get(0).expect("apple");
    assert_eq!(apple.get("name").and_then(Value::as_str), Some("apple"));
    assert_eq!(
        apple
            .get("physical")
            .and_then(|p| p.get("color"))
            .and_then(Value::as_str),
        Some("red")
    );
    let varieties = apple
        .get("varieties")
        .and_then(Value::as_array)
        .expect("varieties");
    assert_eq!(varieties.len(), 2);
    let banana = fruits.get(1).expect("banana");
    assert_eq!(
        banana
            .get("varieties")
            .and_then(Value::as_array)
            .map(Array::len),
        Some(1)
    );
    assert_eq!(
        apple.as_table().map(Table::kind),
        Some(TableKind::ArrayHeader)
    );
    // An empty element, and an empty name.
    let root = ok("[[a]]\n[[a]]\nx = 1\n[['']]\n");
    let a = get(&root, "a").as_array().expect("a");
    assert_eq!(a.get(0).and_then(Value::as_table).map(Table::len), Some(0));
    assert!(get(&root, "").as_array().is_some());
}

#[test]
fn dotted_keys_create_tables() {
    let root = ok(
        "name.first = 'Arthur'\n\"name\".'last' = 'Dent'\nsite.\"google.com\" = true\n\
                   fruit . color = 'yellow'\n3.14159 = 'pi'\n",
    );
    assert_eq!(names(&root), ["name", "site", "fruit", "3"]);
    assert_eq!(get(&root, "name.last").as_str(), Some("Dent"));
    assert_eq!(table_kind(&root, "name"), TableKind::Dotted);
    assert_eq!(
        get(&root, "site")
            .get("google.com")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(get(&root, "3.14159").as_str(), Some("pi"));
    // Under a header, and in each element of an array of tables.
    let root = ok("[t]\na.b.c = 1\na.b.d = 2\n[[arr]]\na.b = 1\n[[arr]]\na.b = 2\n");
    assert_eq!(
        names(get(&root, "t.a.b").as_table().expect("b")),
        ["c", "d"]
    );
    assert_eq!(get(&root, "arr").as_array().map(Array::len), Some(2));
    // A header can define sub-tables of a table dotted keys made.
    let root = ok(
        "[fruit]\napple.color = 'red'\n[fruit.apple.texture]\nsmooth = true\n\
                   [[fruit.apple.seeds]]\n",
    );
    assert_eq!(table_kind(&root, "fruit.apple"), TableKind::Dotted);
    assert_eq!(table_kind(&root, "fruit.apple.texture"), TableKind::Header);
    // A dotted key may define a table headers only created as their prefix.
    let root = ok("[a.b.c]\n[a]\nb.d = 1\n");
    assert_eq!(table_kind(&root, "a.b"), TableKind::Dotted);
    assert_eq!(names(get(&root, "a.b").as_table().expect("b")), ["c", "d"]);
}

#[test]
fn inline_tables_and_toml_1_1s_newlines_and_trailing_commas() {
    let root = ok("p = {x = 1, y.z = 2}\ne = {}\nn = { a = { b = [] } }\n");
    assert_eq!(table_kind(&root, "p"), TableKind::Inline);
    assert_eq!(table_kind(&root, "p.y"), TableKind::Dotted);
    assert_eq!(get(&root, "p.y.z").as_integer(), Some(2));
    assert_eq!(get(&root, "e").as_table().map(Table::len), Some(0));
    assert_eq!(table_kind(&root, "n.a"), TableKind::Inline);
    // New in TOML 1.1: line breaks and comments inside, and a trailing
    // comma.
    let text = "t = {\n  a = 1, # one\n  # between\n  b = \"\"\"\nmulti\"\"\",\r\n  c = [\n 1,\n ],\n}\nu = { x = 1, }\n";
    let root = ok(text);
    assert_eq!(
        names(get(&root, "t").as_table().expect("t")),
        ["a", "b", "c"]
    );
    assert_eq!(get(&root, "t.b").as_str(), Some("multi"));
    assert_eq!(get(&root, "u.x").as_integer(), Some(1));
    // Not an empty entry, though, nor two commas.
    assert_eq!(value_err("{,}"), (6, found(',', INLINE_KEY)));
    assert_eq!(value_err("{a = 1,,}"), (12, found(',', INLINE_KEY)));
    assert_eq!(value_err("{a = 1 b = 2}"), (12, found('b', INLINE_NEXT)));
    assert_eq!(value_err("{a = 1"), (11, end(INLINE_NEXT)));
    // The key and `=` must still be on one line.
    assert_eq!(value_err("{a\n= 1}"), (7, found('\n', EQUALS)));
    assert_eq!(value_err("{a =\n1}"), (9, found('\n', VALUE)));
}

#[test]
fn arrays() {
    let root = ok(
        "a = [ 1, 'two', [3, [4]], {five = 5}, ]\ne = [\n]\nc = [ # comment\n  1,\n  # line\n  2 # after\n]\n",
    );
    let a = get(&root, "a").as_array().expect("a");
    assert_eq!(a.len(), 4);
    assert!(!a.is_array_of_tables());
    assert_eq!(a.get(2).and_then(Value::as_array).map(Array::len), Some(2));
    assert_eq!(
        a.get(3).and_then(Value::as_table).map(Table::kind),
        Some(TableKind::Inline)
    );
    assert!(get(&root, "e").as_array().is_some_and(Array::is_empty));
    assert_eq!(get(&root, "c").as_array().map(Array::len), Some(2));
    assert_eq!(value_err("[,]"), (6, found(',', ARRAY_ITEM)));
    assert_eq!(value_err("[1,,2]"), (8, found(',', ARRAY_ITEM)));
    assert_eq!(value_err("[1 2]"), (8, found('2', ARRAY_NEXT)));
    assert_eq!(value_err("[1,"), (8, end(ARRAY_ITEM)));
    assert_eq!(value_err("[1 # ]"), (11, end(ARRAY_NEXT)));
}

#[test]
fn arrays_and_inline_tables_are_allocated_at_exactly_their_size() {
    let root = ok(
        "a = [1, 2, 3]\nb = [[1], [1, 2], [], {x = 1, y = 2, z = 3}]\nc = {p = 1, q = 2, r = 3, s = 4, t = 5}\n",
    );
    let mut values: Vec<&Value> = root.entries().iter().map(|(_, value)| value).collect();
    while let Some(value) = values.pop() {
        match &value.kind {
            ValueKind::Array(array) => {
                assert_eq!(array.items.capacity(), array.items.len(), "{value:?}");
                values.extend(array.items());
            }
            ValueKind::Table(table) => {
                values.extend(table.entries().iter().map(|(_, value)| value));
            }
            _ => {}
        }
    }
}

#[test]
fn table_kinds_record_how_each_table_was_defined() {
    let root = ok("a.b = 1\n[c.d]\n[[e]]\n[f]\ng = {}\n");
    assert_eq!(root.kind(), TableKind::Root);
    assert_eq!(table_kind(&root, "a"), TableKind::Dotted);
    assert_eq!(table_kind(&root, "c"), TableKind::Implicit);
    assert_eq!(table_kind(&root, "c.d"), TableKind::Header);
    let e = get(&root, "e").as_array().expect("e");
    assert_eq!(
        e.get(0).and_then(Value::as_table).map(Table::kind),
        Some(TableKind::ArrayHeader)
    );
    assert_eq!(table_kind(&root, "f"), TableKind::Header);
    assert_eq!(table_kind(&root, "f.g"), TableKind::Inline);
    // Implicit until defined.
    assert_eq!(table_kind(&ok("[c.d]\n[c]\n"), "c"), TableKind::Header);
    assert_eq!(table_kind(&ok("[[a.b]]\n[a]\n"), "a"), TableKind::Header);
}

/// Documents that break a rule on defining keys and tables, with the line,
/// column and kind of the error each gives.
const REDEFINITIONS: &[(&str, Position)] = &[
    // A key twice, however it is written.
    ("a = 1\na = 2\n", (2, 1, DuplicateKey)),
    ("a = 1\n\"a\" = 2\n", (2, 1, DuplicateKey)),
    ("a = 1\n'a' = 2\n", (2, 1, DuplicateKey)),
    ("a = 1\n\"\\u0061\" = 2\n", (2, 1, DuplicateKey)),
    ("\"\" = 1\n'' = 2\n", (2, 1, DuplicateKey)),
    ("a.b = 1\na.b = 2\n", (2, 3, DuplicateKey)),
    ("a = {b = 1, b = 2}\n", (1, 13, DuplicateKey)),
    ("a = {b.c = 1, b.c = 2}\n", (1, 17, DuplicateKey)),
    ("a = {b.a = 1, b = 2}\n", (1, 15, DuplicateKey)),
    // A key over a table, or a table over a key.
    ("a.b = 1\na = 2\n", (2, 1, DuplicateKey)),
    ("[a.b]\n[a]\nb = 1\n", (3, 1, DuplicateKey)),
    ("a = 1\n[a]\n", (2, 2, DuplicateKey)),
    ("[a]\nb = 1\n[a.b]\n", (3, 4, DuplicateKey)),
    ("a = []\n[a]\n", (2, 2, DuplicateKey)),
    ("a.b = 1\na = {}\n", (2, 1, DuplicateKey)),
    // A table twice.
    ("[a]\n[a]\n", (2, 2, DuplicateTable)),
    ("[a.b]\n[a]\n[a]\n", (3, 2, DuplicateTable)),
    ("[a]\nb.c = 1\n[a.b]\n", (3, 4, DuplicateTable)),
    ("a.b = 1\n[a]\n", (2, 2, DuplicateTable)),
    ("[t]\na.b.c = 1\n[t.a.b]\n", (3, 6, DuplicateTable)),
    ("a = {}\n[a]\n", (2, 2, DuplicateTable)),
    ("[[a]]\n[a]\n", (2, 2, DuplicateTable)),
    ("[a.b.c]\n[a]\nb.d = 1\n[a.b]\n", (4, 4, DuplicateTable)),
    // Through something that is not a table.
    (
        "a = 1\na.b = 2\n",
        (
            2,
            1,
            NotATable {
                found: "an integer",
            },
        ),
    ),
    ("a = 'x'\n[a.b]\n", (2, 2, NotATable { found: "a string" })),
    ("a = [{}]\n[a.b]\n", (2, 2, NotATable { found: "an array" })),
    ("a = []\n[[a.b]]\n", (2, 3, NotATable { found: "an array" })),
    (
        "t = {a = [{}], a.b = 1}\n",
        (1, 16, NotATable { found: "an array" }),
    ),
    (
        "[[a.b]]\n[a]\nb.c = 1\n",
        (
            3,
            1,
            NotATable {
                found: "an array of tables",
            },
        ),
    ),
    (
        "a = 1979-05-27\n[a.b]\n",
        (
            2,
            2,
            NotATable {
                found: "a date-time",
            },
        ),
    ),
    // `[[...]]` on something that is not an array of tables.
    (
        "[a]\n[[a]]\n",
        (2, 3, NotAnArrayOfTables { found: "a table" }),
    ),
    (
        "a = []\n[[a]]\n",
        (2, 3, NotAnArrayOfTables { found: "an array" }),
    ),
    (
        "a = true\n[[a]]\n",
        (2, 3, NotAnArrayOfTables { found: "a boolean" }),
    ),
    (
        "a = {}\n[[a]]\n",
        (
            2,
            3,
            NotAnArrayOfTables {
                found: "an inline table",
            },
        ),
    ),
    (
        "a.b = 1\n[[a]]\n",
        (2, 3, NotAnArrayOfTables { found: "a table" }),
    ),
    (
        "[[a.b]]\n[[a]]\n",
        (2, 3, NotAnArrayOfTables { found: "a table" }),
    ),
    // Adding to an inline table after it closes.
    ("a = {}\na.b = 1\n", (2, 1, ExtendsInlineTable)),
    ("a = {b = 1}\n[a.c]\n", (2, 2, ExtendsInlineTable)),
    ("a = {b = {}}\n[[a.b.c]]\n", (2, 3, ExtendsInlineTable)),
    ("t = {a = {b = 1}, a.c = 2}\n", (1, 19, ExtendsInlineTable)),
    // Dotted keys adding to a table a header defined.
    (
        "[a.b.c]\nz = 9\n[a]\nb.c.t = 1\n",
        (4, 3, ExtendsDefinedTable),
    ),
    ("[a.b]\n[a]\nb.c = 1\n", (3, 1, ExtendsDefinedTable)),
    (
        "[[a.b]]\n[a.b.c]\n[a]\nb.c.d = 1\n",
        (
            4,
            1,
            NotATable {
                found: "an array of tables",
            },
        ),
    ),
];

#[test]
fn keys_and_tables_are_defined_once_and_closed_ones_stay_closed() {
    for (text, expected) in REDEFINITIONS {
        assert_eq!(&err(text), expected, "{text:?}");
    }
}

// --------------------------------------------- lines, comments, encoding

#[test]
fn comments_whitespace_and_line_breaks() {
    assert_eq!(ok("").len(), 0);
    assert_eq!(ok("\n\r\n \t\n# only a comment").len(), 0);
    let root = ok("# head\n\ta = 1 # tail\r\n[t] # header\n  b = 2\t#\n#end");
    assert_eq!(get(&root, "a").as_integer(), Some(1));
    assert_eq!(get(&root, "t.b").as_integer(), Some(2));
    // Comments may hold any character but the control characters.
    assert_eq!(ok("# é € 😀 \t \u{feff} \" ' [ ] { } = #\na = 1").len(), 1);
    assert_eq!(err("# \u{0}"), (1, 3, ControlCharacter { found: '\0' }));
    assert_eq!(
        err("a = 1 # \u{7f}"),
        (1, 9, ControlCharacter { found: '\u{7f}' })
    );
    // One key/value pair or header per line.
    assert_eq!(err("a = 1 b = 2"), (1, 7, found('b', LINE_END)));
    assert_eq!(err("[a] b = 2"), (1, 5, found('b', LINE_END)));
    assert_eq!(err("[a]]"), (1, 4, found(']', LINE_END)));
    // Whitespace is space and tab only.
    assert_eq!(err("a =\u{a0}1"), (1, 4, found('\u{a0}', VALUE)));
    assert_eq!(err("\u{3000}a = 1"), (1, 1, found('\u{3000}', EXPRESSION)));
}

#[test]
fn a_carriage_return_must_end_a_line_with_a_line_feed() {
    assert_eq!(ok("a = 1\r\nb = 2\r\n").len(), 2);
    for (text, line, column) in [
        ("a = 1\r", 1, 6),
        ("a = 1\rb = 2", 1, 6),
        ("\r", 1, 1),
        ("# x\r", 1, 4),
        ("a = [\r1]", 1, 6),
        ("a = { b = 1,\r}", 1, 13),
        ("a = \"x\ry\"", 1, 7),
        ("a = 'x\ry'", 1, 7),
        ("a = \"\"\"x\ry\"\"\"", 1, 9),
        ("a = '''x\ry'''", 1, 9),
        ("a = \"\"\"x\\\n\ry\"\"\"", 2, 1),
        // Where a key, `=`, `.`, `]`, a value or a field of a date-time
        // should be, too.
        ("a\r= 1", 1, 2),
        ("a.\rb = 1", 1, 3),
        ("[a\r]", 1, 3),
        ("[\ra]", 1, 2),
        ("a = \r1", 1, 5),
        ("t = {a\r= 1}", 1, 7),
        ("v = 1979-05-27T07:\r", 1, 19),
    ] {
        assert_eq!(err(text), (line, column, BareCarriageReturn), "{text:?}");
    }
    // A CR LF where it cannot be is a line break that comes too soon.
    assert_eq!(err("a\r\n= 1"), (1, 2, found('\r', EQUALS)));
}

#[test]
fn a_leading_byte_order_mark_is_skipped() {
    let text = "\u{feff}a = 1\n";
    let root = ok(text);
    let (key, value) = root.get_key_value("a").expect("a");
    // Spans are offsets into the input as given, the mark included.
    assert_eq!(key.span, Span::new(3, 4));
    assert_eq!(text_of(text, value.span), "1");
    // Columns do not count it; offsets do.
    assert_eq!(err("\u{feff}a = x"), (1, 5, found('x', VALUE)));
    match parse("\u{feff}a = x", DEFAULT) {
        Err(CodecError::Toml { offset, .. }) => assert_eq!(offset, 7),
        other => panic!("{other:?}"),
    }
    // Only one, and only at the start.
    assert_eq!(
        err("\u{feff}\u{feff}a = 1"),
        (1, 1, found('\u{feff}', EXPRESSION))
    );
    assert_eq!(
        err("a = 1\n\u{feff}b = 2"),
        (2, 1, found('\u{feff}', EXPRESSION))
    );
    assert_eq!(
        parse_bytes(b"\xef\xbb\xbfa = 1", DEFAULT),
        Ok(ok("\u{feff}a = 1"))
    );
}

/// The offset, line, column and kind of the error for `bytes`.
fn bytes_err(bytes: &[u8]) -> (usize, usize, usize, ErrorKind) {
    match parse_bytes(bytes, DEFAULT) {
        Err(CodecError::Toml {
            offset,
            line,
            column,
            kind,
        }) => (offset, line, column, kind),
        other => panic!("{bytes:?} should not parse, but gave {other:?}"),
    }
}

#[test]
fn bytes_that_are_not_utf8_are_rejected_where_they_go_wrong() {
    assert_eq!(bytes_err(b"\xff"), (0, 1, 1, InvalidUtf8));
    assert_eq!(bytes_err(b"a = \"x\xffy\""), (6, 1, 7, InvalidUtf8));
    assert_eq!(bytes_err(b"# \xc3\n"), (2, 1, 3, InvalidUtf8));
    assert_eq!(
        bytes_err(b"a = 1\n# \xc3\xa9 \xe5"),
        (11, 2, 5, InvalidUtf8)
    );
    // A surrogate encoded in UTF-8 is not UTF-8.
    assert_eq!(bytes_err(b"a = '\xed\xa0\x80'"), (5, 1, 6, InvalidUtf8));
    // UTF-16 with its byte-order mark is not UTF-8; without it, the NUL
    // bytes are valid UTF-8, and fail as TOML.
    assert_eq!(bytes_err(b"\xff\xfea\x00"), (0, 1, 1, InvalidUtf8));
    assert_eq!(
        bytes_err(b"\x00a\x00 \x00=\x00 \x001"),
        (0, 1, 1, found('\0', EXPRESSION))
    );
    let text = "a = \"é😀\"";
    assert_eq!(parse_bytes(text.as_bytes(), DEFAULT), Ok(ok(text)));
}

// ------------------------------------------------------------------ spans

#[test]
fn keys_and_scalars_span_their_text() {
    let text = "bare = 1\n\"quoted key\" = 'lit'\n'single'.dotted . key = 1979-05-27T07:32:00Z\nf = -1.5e3\ns = \"\"\"\nmulti\"\"\"\n";
    let root = ok(text);
    let (key, value) = root.get_key_value("bare").expect("bare");
    assert_eq!(
        (text_of(text, key.span), text_of(text, value.span)),
        ("bare", "1")
    );
    let (key, value) = root.get_key_value("quoted key").expect("quoted");
    assert_eq!(text_of(text, key.span), "\"quoted key\"");
    assert_eq!(text_of(text, value.span), "'lit'");
    let single = get(&root, "single").as_table().expect("single");
    let (key, _) = single.get_key_value("dotted").expect("dotted");
    assert_eq!(text_of(text, key.span), "dotted");
    let dotted = get(&root, "single.dotted").as_table().expect("dotted");
    let (key, value) = dotted.get_key_value("key").expect("key");
    assert_eq!(text_of(text, key.span), "key");
    assert_eq!(text_of(text, value.span), "1979-05-27T07:32:00Z");
    assert_eq!(text_of(text, get(&root, "f").span), "-1.5e3");
    assert_eq!(text_of(text, get(&root, "s").span), "\"\"\"\nmulti\"\"\"");
}

#[test]
fn arrays_and_inline_tables_span_their_brackets() {
    let text = "a = [ 1, [2, 3], { x = 4 } ]\nt = {\n  y = [],\n}\n";
    let root = ok(text);
    let a = get(&root, "a");
    assert_eq!(text_of(text, a.span), "[ 1, [2, 3], { x = 4 } ]");
    let items = a.as_array().expect("a").items();
    assert_eq!(text_of(text, items[1].span), "[2, 3]");
    assert_eq!(text_of(text, items[2].span), "{ x = 4 }");
    assert_eq!(text_of(text, get(&root, "t").span), "{\n  y = [],\n}");
    assert_eq!(text_of(text, get(&root, "t.y").span), "[]");
}

#[test]
fn header_tables_span_their_header() {
    let text = "[ a . b ]\nx = 1\n[[c]]\n[[c]] # two\n";
    let root = ok(text);
    let b = get(&root, "a.b");
    assert_eq!(text_of(text, b.span), "[ a . b ]");
    let (key, _) = get(&root, "a")
        .as_table()
        .and_then(|a| a.get_key_value("b"))
        .expect("b");
    assert_eq!(text_of(text, key.span), "b");
    // The array spans the header that made it; each element its own.
    let c = get(&root, "c");
    assert_eq!(c.span, Span::new(16, 21));
    let elements = c.as_array().expect("c").items();
    assert_eq!(elements[0].span, Span::new(16, 21));
    assert_eq!(elements[1].span, Span::new(22, 27));
    assert_eq!(text_of(text, elements[1].span), "[[c]]");
}

#[test]
fn implicit_tables_span_what_created_them_until_they_are_defined() {
    // A header's prefix: the whole header.
    let text = "[x.y.z]\n";
    let root = ok(text);
    assert_eq!(text_of(text, get(&root, "x").span), "[x.y.z]");
    assert_eq!(text_of(text, get(&root, "x.y").span), "[x.y.z]");
    let (key, _) = root.get_key_value("x").expect("x");
    assert_eq!(key.span, Span::new(1, 2));
    // A dotted key: the whole key.
    let text = "a . b.c = 1\na.d = 2\n";
    let root = ok(text);
    assert_eq!(text_of(text, get(&root, "a").span), "a . b.c");
    assert_eq!(text_of(text, get(&root, "a.b").span), "a . b.c");
    // Defined later by a header: that header, and the key there.
    let text = "[x.y]\n[x]\n";
    let root = ok(text);
    let (key, value) = root.get_key_value("x").expect("x");
    assert_eq!(value.span, Span::new(6, 9));
    assert_eq!(key.span, Span::new(7, 8));
    // Defined later by a dotted key: that key.
    let text = "[a.b.c]\n[a]\nb.d = 1\n";
    let root = ok(text);
    let a = get(&root, "a").as_table().expect("a");
    let (key, value) = a.get_key_value("b").expect("b");
    assert_eq!(text_of(text, value.span), "b.d");
    assert_eq!(key.span, Span::new(12, 13));
    // An array of tables' implicit parent.
    let text = "[[p.q]]\n";
    let root = ok(text);
    assert_eq!(text_of(text, get(&root, "p").span), "[[p.q]]");
}

// ------------------------------------------------------- error positions

#[test]
fn errors_report_the_line_and_column() {
    assert_eq!(err("a = 1\nb = 2\nc = nope\n"), (3, 5, InvalidLiteral));
    // CR LF ends one line.
    assert_eq!(err("a = 1\r\n\r\nb = ?"), (3, 5, found('?', VALUE)));
    // Lines inside multi-line strings and arrays count.
    assert_eq!(
        err("a = \"\"\"\n\n\"\"\"\nb = [\n1,\n?]"),
        (6, 1, found('?', ARRAY_ITEM))
    );
    // Columns count characters, not bytes: `é` is 2 bytes, `€` 3, `😀` 4.
    assert_eq!(err("\"é€😀\" = x"), (1, 9, found('x', VALUE)));
    assert_eq!(err("a = 'é' 1"), (1, 9, found('1', LINE_END)));
    assert_eq!(err("\t\ta = x"), (1, 7, found('x', VALUE)));
    assert_eq!(err("[a\n"), (1, 3, found('\n', HEADER_CLOSE)));
    assert_eq!(err("a = [\n\n"), (3, 1, end(ARRAY_ITEM)));
}

#[test]
fn errors_display_with_their_position() {
    let message = |text: &str| parse(text, DEFAULT).expect_err("an error").to_string();
    assert_eq!(
        message("name = \"Tom\"\nname = \"Pradyun\""),
        "TOML error at line 2, column 1 (byte 13): the key is already defined"
    );
    assert_eq!(
        message("a = [1 2]"),
        "TOML error at line 1, column 8 (byte 7): found '2' where `,` or `]` was expected"
    );
    assert_eq!(
        message("s = \"abc"),
        "TOML error at line 1, column 5 (byte 4): the text ends where `\"` to close the string \
         that starts here was expected"
    );
    assert_eq!(
        message("d = 2023-02-29"),
        "TOML error at line 1, column 13 (byte 12): no such date or time; expected a day that \
         the month has"
    );
}

// ----------------------------------------------------------------- limits

fn depth(max_depth: usize) -> Limits {
    Limits {
        max_depth,
        ..DEFAULT
    }
}

#[test]
fn limit_max_depth_counts_every_kind_of_nesting() {
    let too_deep = |limit| TooDeep { limit };
    // A scalar in the root is depth 0.
    ok_with("a = 1", depth(0));
    // An array or inline table is one level, empty or not.
    assert_eq!(err_with("a = []", depth(0)), (1, 5, too_deep(0)));
    assert_eq!(err_with("a = {}", depth(0)), (1, 5, too_deep(0)));
    ok_with("a = [[1], {b = 2}]", depth(2));
    assert_eq!(err_with("a = [[[1]]]", depth(2)), (1, 7, too_deep(2)));
    assert_eq!(err_with("a = [{b = [1]}]", depth(2)), (1, 11, too_deep(2)));
    // Each part of a dotted key before the last is a table.
    ok_with("a.b = 1", depth(1));
    assert_eq!(err_with("a.b.c = 1", depth(1)), (1, 3, too_deep(1)));
    assert_eq!(err_with("a.b = []", depth(1)), (1, 7, too_deep(1)));
    assert_eq!(err_with("t = {a.b = 1}", depth(1)), (1, 6, too_deep(1)));
    // Each part of a header is a table.
    ok_with("[a.b]", depth(2));
    assert_eq!(err_with("[a.b.c]", depth(2)), (1, 6, too_deep(2)));
    assert_eq!(err_with("[a]", depth(0)), (1, 2, too_deep(0)));
    // A section's tables count for the keys under it.
    ok_with("[a]\nb.c = 1", depth(2));
    assert_eq!(err_with("[a]\nb.c = 1", depth(1)), (2, 1, too_deep(1)));
    assert_eq!(err_with("[a]\nb = [1]", depth(1)), (2, 5, too_deep(1)));
    // An array of tables is two levels: the array and its element.
    ok_with("[[a]]", depth(2));
    assert_eq!(err_with("[[a]]", depth(1)), (1, 3, too_deep(1)));
    ok_with("[[a]]\n[a.b]", depth(3));
    assert_eq!(err_with("[[a]]\n[a.b]", depth(2)), (2, 4, too_deep(2)));
    assert_eq!(err_with("[[a]]\n[[a.b]]", depth(3)), (2, 5, too_deep(3)));
    assert_eq!(err_with("[[a]]\nb = {}", depth(2)), (2, 5, too_deep(2)));
}

#[test]
fn limit_max_depth_default_is_128_for_every_kind_of_nesting() {
    assert_eq!(DEFAULT_DEPTH, 128);
    let too_deep = TooDeep {
        limit: DEFAULT_DEPTH,
    };
    let parts = |count: usize| vec!["a"; count].join(".");
    // Arrays and inline tables.
    ok(&format!("v = {}{}", "[".repeat(128), "]".repeat(128)));
    assert_eq!(
        err(&format!("v = {}{}", "[".repeat(129), "]".repeat(129))),
        (1, 133, too_deep.clone())
    );
    ok(&format!(
        "v = {}{}",
        "{a=".repeat(127) + "{}",
        "}".repeat(127)
    ));
    assert_eq!(
        err(&format!(
            "v = {}{}",
            "{a=".repeat(128) + "{}",
            "}".repeat(128)
        )),
        (1, 5 + 128 * 3, too_deep.clone())
    );
    // Dotted keys: 128 tables and a value.
    ok(&format!("{} = 1", parts(129)));
    assert_eq!(
        err(&format!("{} = 1", parts(130))),
        (1, 257, too_deep.clone())
    );
    // Headers: 128 tables.
    ok(&format!("[{}]", parts(128)));
    assert_eq!(
        err(&format!("[{}]", parts(129))),
        (1, 258, too_deep.clone())
    );
    // Arrays of tables: 64 of them nest 128 deep.
    let aot: String = (1..=64).map(|n| format!("[[{}]]\n", parts(n))).collect();
    ok(&aot);
    assert_eq!(err(&format!("{aot}[[{}]]", parts(65))).2, too_deep);
}

#[test]
fn limit_max_depth_stops_ten_thousand_part_keys_at_the_first_part_too_deep() {
    let too_deep = TooDeep {
        limit: DEFAULT_DEPTH,
    };
    let parts = vec!["a"; 10_000].join(".");
    // The 129th part is the one too deep: column 2 * 128 + 1.
    assert_eq!(err(&format!("{parts} = 1")), (1, 257, too_deep.clone()));
    assert_eq!(err(&format!("[{parts}]")), (1, 258, too_deep.clone()));
    assert_eq!(err(&format!("[[{parts}]]")), (1, 259, too_deep.clone()));
    // In an inline table, which is one level itself, the 128th part.
    assert_eq!(
        err(&format!("t = {{{parts} = 1}}")),
        (1, 260, too_deep.clone())
    );
    // With two arrays of tables on the way, each two levels, a header's
    // 127th part is the one too deep, before its 129th is reached.
    let text = format!("[[a]]\n[[a.a]]\n[{parts}]");
    assert_eq!(err(&text), (3, 2 * 127, too_deep.clone()));
    // And very deep arrays and inline tables.
    assert_eq!(
        err(&format!("v = {}", "[".repeat(1_000_000))),
        (1, 133, too_deep.clone())
    );
    assert_eq!(
        err(&format!("v = {}", "{a=".repeat(500_000))),
        (1, 5 + 128 * 3, too_deep)
    );
}

#[test]
fn a_failed_parse_does_not_overflow_the_stack_however_deep() {
    // With the depth limit raised, a deep tree is built before the error is
    // found. Dropping it the ordinary way would recurse once per level and
    // overflow this thread's 2 MiB stack, aborting the test run.
    let deep = 200_000;
    let limits = depth(deep + 10);
    let run = move || {
        let parts = vec!["a"; deep].join(".");
        let arrays = format!("{}{}", "[".repeat(deep), "]".repeat(deep));
        let inline = format!("{}{{}}{}", "{a=".repeat(deep), "}".repeat(deep));
        [
            // Inside an array that never closes, which a section holds.
            format!("[t]\nv = [{arrays} x"),
            // A complete value, then text after it.
            format!("v = {arrays} x"),
            format!("v = {inline} x"),
            // Deep tables from a dotted key and from a header.
            format!("{parts} = 1\n?"),
            format!("[{parts}]\n?"),
            // Inside an inline table that never closes.
            format!("v = {{a = 1, b = {inline},"),
            // An inline table's first key, whose tables are deep, then no
            // `=`.
            format!("v = {{{parts} ?"),
        ]
        .map(|text| match parse(&text, limits) {
            Ok(table) => {
                dismantle(values_of(table));
                None
            }
            Err(CodecError::Toml { kind, .. }) => Some(kind),
            Err(other) => panic!("{other}"),
        })
    };
    let kinds = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(run)
        .expect("a thread")
        .join()
        .expect("no panic");
    assert_eq!(
        kinds,
        [
            Some(found('x', ARRAY_NEXT)),
            Some(found('x', LINE_END)),
            Some(found('x', LINE_END)),
            Some(found('?', EXPRESSION)),
            Some(found('?', EXPRESSION)),
            Some(end(INLINE_KEY)),
            Some(found('?', EQUALS)),
        ]
    );
}

#[test]
fn the_parser_itself_does_not_recurse() {
    // With the limit raised, documents far deeper than any thread's stack
    // could take in recursive calls still parse, on a small stack.
    let deep = 100_000;
    let run = move || {
        let parts = vec!["a"; deep].join(".");
        for text in [
            format!("v = {}{}", "[".repeat(deep), "]".repeat(deep)),
            format!("v = {}1{}", "{a=".repeat(deep), "}".repeat(deep)),
            format!("{parts} = 1"),
            format!("[{parts}]\nx = 1"),
        ] {
            let table = parse(&text, depth(deep + 1)).expect("parses");
            // Dropping a value recurses once per level (see
            // `Limits::max_depth`), so this one is taken apart the way a
            // failed parse does it.
            dismantle(values_of(table));
        }
    };
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(run)
        .expect("a thread")
        .join()
        .expect("no panic");
}

#[test]
fn limit_max_string_bytes() {
    let limits = Limits {
        max_string_bytes: 5,
        ..DEFAULT
    };
    let too_long = StringTooLong { limit: 5 };
    for fits in [
        "'12345'",
        "\"12345\"",
        "\"\"\"12345\"\"\"",
        "'''12345'''",
        "\"\"\"\n12345\"\"\"",
    ] {
        ok_with(&format!("v = {fits}"), limits);
    }
    for text in [
        "'123456'",
        "\"123456\"",
        "\"\"\"123456\"\"\"",
        "'''123456'''",
        "'''12345''''",
    ] {
        assert_eq!(
            err_with(&format!("v = {text}"), limits),
            (1, 5, too_long.clone()),
            "{text}"
        );
    }
    // Decoded bytes count: `é` is two, however it is written; a CR LF that
    // becomes LF is one.
    ok_with("v = \"\\u00e9\\u00e9\\x41\"", limits);
    assert_eq!(err_with("v = \"ééé\"", limits), (1, 5, too_long.clone()));
    assert_eq!(
        err_with("v = \"\\u00e9\\u00e9\\u00e9\"", limits),
        (1, 5, too_long.clone())
    );
    ok_with("v = \"\"\"1\r\n2\r\n3\"\"\"", limits);
    // Keys too, bare and quoted, each part on its own.
    ok_with("abcde.fghij = 1", limits);
    assert_eq!(err_with("abcdef = 1", limits), (1, 1, too_long.clone()));
    assert_eq!(
        err_with("a.\"bcdefg\" = 1", limits),
        (1, 3, too_long.clone())
    );
    assert_eq!(err_with("[abcdef]", limits), (1, 2, too_long));
    // The default is 16 MiB.
    assert_eq!(DEFAULT.max_string_bytes, 16 * 1024 * 1024);
    let long = "x".repeat(DEFAULT.max_string_bytes);
    ok(&format!("v = '{long}'"));
    assert_eq!(
        err(&format!("v = '{long}x'")),
        (
            1,
            5,
            StringTooLong {
                limit: DEFAULT.max_string_bytes
            }
        )
    );
}

#[test]
fn limit_max_number_bytes() {
    let limit = |max_number_bytes| Limits {
        max_number_bytes,
        ..DEFAULT
    };
    let too_long = |limit| NumberTooLong { limit };
    ok_with("v = 1_000", limit(5));
    assert_eq!(err_with("v = 10_000", limit(5)), (1, 5, too_long(5)));
    assert_eq!(err_with("v = -1.5e3", limit(5)), (1, 5, too_long(5)));
    assert_eq!(err_with("v = 0xffff", limit(5)), (1, 5, too_long(5)));
    ok_with("v = 0xfff", limit(5));
    // Date-times count too, fraction and all.
    ok_with("v = 1979-05-27", limit(10));
    assert_eq!(err_with("v = 1979-05-27", limit(9)), (1, 5, too_long(9)));
    assert_eq!(
        err_with("v = 07:32:00.123", limit(10)),
        (1, 5, too_long(10))
    );
    // `inf` and `nan` are numbers too, signs included.
    ok_with("a = inf\nb = nan\nc = 100", limit(3));
    assert_eq!(err_with("a = -inf", limit(3)), (1, 5, too_long(3)));
    assert_eq!(err_with("a = +nan", limit(3)), (1, 5, too_long(3)));
    assert_eq!(err_with("a = -100", limit(3)), (1, 5, too_long(3)));
    assert_eq!(err_with("a = [inf, -nan]", limit(3)), (1, 11, too_long(3)));
    assert_eq!(err_with("a = inf", limit(2)), (1, 5, too_long(2)));
    assert_eq!(DEFAULT.max_number_bytes, 1024);
    assert_eq!(
        err(&format!("v = 0.{}", "1".repeat(1023))),
        (1, 5, too_long(1024))
    );
    ok(&format!("v = 0.{}", "1".repeat(1022)));
}

#[test]
fn limit_max_entries_bounds_each_table_and_array() {
    let limits = Limits {
        max_entries: 2,
        ..DEFAULT
    };
    let too_many = TooManyEntries { limit: 2 };
    ok_with("a = 1\n[t]\nx = [1, 2]\ny = {p = 1, q = 2}\n", limits);
    ok_with("[[r]]\n[[r]]\n", limits);
    // The root, a header's table, a dotted key's table, an inline table.
    assert_eq!(
        err_with("a = 1\nb = 2\nc = 3", limits),
        (3, 1, too_many.clone())
    );
    assert_eq!(
        err_with("a = 1\nb = 2\n[c]", limits),
        (3, 2, too_many.clone())
    );
    assert_eq!(
        err_with("[t]\na = 1\nb = 2\nc = 3", limits),
        (4, 1, too_many.clone())
    );
    assert_eq!(
        err_with("d.a = 1\nd.b = 2\nd.c = 3", limits),
        (3, 3, too_many.clone())
    );
    assert_eq!(
        err_with("v = {a = 1, b = 2, c = 3}", limits),
        (1, 20, too_many.clone())
    );
    // An array, and an array of tables.
    assert_eq!(err_with("v = [1, 2, 3]", limits), (1, 12, too_many.clone()));
    assert_eq!(
        err_with("[[r]]\n[[r]]\n[[r]]", limits),
        (3, 3, too_many.clone())
    );
    // A trailing comma is not an entry.
    ok_with("v = [1, 2,]\nw = {a = 1, b = 2,}", limits);
    let none = Limits {
        max_entries: 0,
        ..DEFAULT
    };
    ok_with(
        "v = []",
        Limits {
            max_entries: 1,
            ..DEFAULT
        },
    );
    assert_eq!(err_with("a = 1", none), (1, 1, TooManyEntries { limit: 0 }));
    ok_with("# nothing", none);
    assert_eq!(DEFAULT.max_entries, 1_000_000);
}

#[test]
fn limit_max_input_bytes() {
    let limit = |max_input_bytes| Limits {
        max_input_bytes,
        ..DEFAULT
    };
    let too_large = |length, limit| InputTooLarge { length, limit };
    ok_with("a = 1", limit(5));
    assert_eq!(err_with("a = 12", limit(5)), (1, 6, too_large(6, 5)));
    assert_eq!(err_with("a = 1\nb = 2", limit(6)), (2, 1, too_large(11, 6)));
    // A byte-order mark counts against the limit.
    assert_eq!(err_with("\u{feff}a = 1", limit(5)), (1, 3, too_large(8, 5)));
    // The error is at the start of the character that does not fit.
    assert_eq!(err_with("a = 'é'", limit(6)), (1, 6, too_large(8, 6)));
    // The size is checked before anything else, the UTF-8 included.
    match parse_bytes(b"\xff\xff\xff\xff", limit(2)) {
        Err(CodecError::Toml { kind, .. }) => assert_eq!(kind, too_large(4, 2)),
        other => panic!("{other:?}"),
    }
    ok_with("", limit(0));
    assert_eq!(err_with(" ", limit(0)), (1, 1, too_large(1, 0)));
}

// ------------------------------------------------------------------- cost

/// Parses `text`, which must have `entries` entries in its root or in the
/// table at `path`, and checks that it takes less than `budget`. A parser
/// that compared every key with every other would take minutes on these.
fn parses_quickly(text: &str, path: Option<&str>, entries: usize, budget: Duration) {
    let start = Instant::now();
    let root = ok(text);
    let elapsed = start.elapsed();
    let table = match path {
        Some(path) => get(&root, path).as_table().expect("a table"),
        None => &root,
    };
    assert_eq!(table.len(), entries);
    assert!(elapsed < budget, "took {elapsed:?}");
}

/// Far more than these take in a debug build (a tenth to a third of a
/// second each, measured), so that a slow machine does not fail them, and
/// far less than a quadratic parser would take.
const BUDGET: Duration = Duration::from_secs(2);

#[test]
fn a_table_with_100000_keys_parses_in_linear_time() {
    let text: String = (0..100_000).map(|n| format!("key{n} = {n}\n")).collect();
    parses_quickly(&text, None, 100_000, BUDGET);
    let text: String = (0..100_000).map(|n| format!("t.key{n} = {n}\n")).collect();
    parses_quickly(&text, Some("t"), 100_000, BUDGET);
    let entries: Vec<String> = (0..100_000).map(|n| format!("k{n} = {n}")).collect();
    let text = format!("t = {{ {} }}", entries.join(", "));
    parses_quickly(&text, Some("t"), 100_000, BUDGET);
}

#[test]
fn ten_thousand_sibling_tables_parse_in_linear_time() {
    let text: String = (0..10_000).map(|n| format!("[t{n}]\nx = {n}\n")).collect();
    parses_quickly(&text, None, 10_000, BUDGET);
    let text: String = (0..10_000)
        .map(|n| format!("[a.t{n}]\nx = {n}\n"))
        .collect();
    parses_quickly(&text, Some("a"), 10_000, BUDGET);
    let text: String = (0..10_000).map(|n| format!("[[a]]\nx = {n}\n")).collect();
    let start = Instant::now();
    let root = ok(&text);
    assert!(start.elapsed() < BUDGET);
    assert_eq!(get(&root, "a").as_array().map(Array::len), Some(10_000));
}

// ----------------------------------------------------------------- memory

/// The chunk glibc's `malloc` takes for a request of `size` bytes: the
/// request and 8 bytes of header, rounded up to 16, and at least 32. An
/// estimate of what each allocation really costs.
fn malloc_chunk(size: usize) -> usize {
    if size == 0 {
        0
    } else {
        ((size + 8).div_ceil(16) * 16).max(32)
    }
}

/// The sizes of the allocations a parsed tree holds, at their capacity.
fn allocations(root: &Table) -> Vec<usize> {
    let mut sizes = Vec::new();
    let mut tables = vec![root];
    let mut values: Vec<&Value> = Vec::new();
    loop {
        if let Some(table) = tables.pop() {
            sizes.extend(table.own_allocations());
            for (key, value) in table.entries() {
                sizes.push(key.name.capacity());
                values.push(value);
            }
        } else if let Some(value) = values.pop() {
            match &value.kind {
                ValueKind::String(text) => sizes.push(text.capacity()),
                ValueKind::Array(array) => {
                    sizes.push(array.items.capacity() * std::mem::size_of::<Value>());
                    values.extend(array.items());
                }
                ValueKind::Table(table) => tables.push(table),
                _ => {}
            }
        } else {
            return sizes;
        }
    }
}

/// A short name for `n`, in the 64 characters a bare key may hold.
fn short(mut n: usize) -> String {
    const DIGITS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-_ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut name = String::new();
    loop {
        name.push(char::from(DIGITS[n % 64]));
        n /= 64;
        if n == 0 {
            return name;
        }
    }
}

/// `count` one-letter keys, one per line.
fn letter_keys(count: usize) -> String {
    (0..count).map(|n| format!("{}=0\n", short(n))).collect()
}

/// Documents shaped to take the most memory for their text: many small
/// tables, arrays and keys, and long chains of one-entry tables. Each is
/// about 150 KB.
fn dense_documents() -> Vec<(&'static str, String)> {
    let nine: Vec<String> = (0..9).map(|n| format!("{}=0", short(n))).collect();
    let nine = nine.join(",");
    let chain = ".b".repeat(119);
    vec![
        (
            "arrays of integers",
            format!("a = [{}0]", "0,".repeat(75_000)),
        ),
        (
            "arrays of one-element arrays",
            format!("a = [{}[0]]", "[0],".repeat(37_000)),
        ),
        (
            "arrays of empty inline tables",
            format!("a = [{}{{}}]", "{},".repeat(50_000)),
        ),
        (
            "arrays of one-key inline tables",
            format!("a = [{}{{b=0}}]", "{b=0},".repeat(25_000)),
        ),
        (
            "arrays of nine-key inline tables",
            format!("a = [{}{{{nine}}}]", format!("{{{nine}}},").repeat(4_000)),
        ),
        (
            "arrays of empty strings",
            format!("a = [{}\"\"]", "\"\",".repeat(50_000)),
        ),
        (
            "a key on each line",
            (0..25_000).map(|n| format!("{}=0\n", short(n))).collect(),
        ),
        (
            "a dotted table on each line",
            (0..20_000).map(|n| format!("{}.a=0\n", short(n))).collect(),
        ),
        (
            "an empty header on each line",
            (0..30_000).map(|n| format!("[{}]\n", short(n))).collect(),
        ),
        ("[[a]] on each line", "[[a]]\n".repeat(25_000)),
        (
            "[[aN]] and nine keys",
            (0..3_500)
                .map(|n| format!("[[a{}]]\n{}", short(n % 64), letter_keys(9)))
                .collect(),
        ),
        (
            "[[aN]] and seventeen keys",
            (0..2_000)
                .map(|n| format!("[[a{}]]\n{}", short(n % 64), letter_keys(17)))
                .collect(),
        ),
        (
            "[tN] and nine keys",
            (0..3_500)
                .map(|n| format!("[t{}]\n{}", short(n), letter_keys(9)))
                .collect(),
        ),
        (
            "dotted keys of 120 parts",
            (0..600)
                .map(|n| format!("{}{chain}=0\n", short(n)))
                .collect(),
        ),
        (
            "headers of 120 parts",
            (0..600)
                .map(|n| format!("[{}{chain}]\n", short(n)))
                .collect(),
        ),
        (
            "inline dotted keys of 120 parts",
            (0..600)
                .map(|n| format!("{} = {{a{chain}=0}}\n", short(n)))
                .collect(),
        ),
    ]
}

/// The bounds `Limits` documents for a TOML tree: bytes of heap per byte
/// of text, counting each allocation at its capacity, and with each
/// allocation rounded up to glibc's chunk.
const HEAP_PER_BYTE: f64 = 49.0;
const GLIBC_PER_BYTE: f64 = 72.0;

#[test]
fn dense_documents_take_no_more_memory_than_documented() {
    let mut worst = (0.0_f64, 0.0_f64);
    for (name, text) in dense_documents() {
        let root = ok(&text);
        let sizes = allocations(&root);
        let heap = sizes.iter().sum::<usize>() as f64 / text.len() as f64;
        let chunks =
            sizes.iter().map(|&size| malloc_chunk(size)).sum::<usize>() as f64 / text.len() as f64;
        assert!(
            heap <= HEAP_PER_BYTE && chunks <= GLIBC_PER_BYTE,
            "{name}: {heap:.1} bytes of heap a byte of text, {chunks:.1} with glibc"
        );
        worst = (worst.0.max(heap), worst.1.max(chunks));
    }
    // The chains of one-entry tables come close to the bounds, which are
    // what a chain of any length tends to (97 bytes of heap, 144 with glibc,
    // for each 2-byte `.b`). If this fails, the documented figures are too
    // high and should come down with it.
    assert!(worst.0 > 47.0 && worst.1 > 70.0, "{worst:?}");
}

#[test]
fn a_parsed_tree_has_no_room_to_spare() {
    let mut texts: Vec<String> = dense_documents()
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    texts.push(
        "s = \"\\u00e9scaped \\U0001F600 and more\"\nm = \"\"\"\nmulti\\tline\r\nstring\"\"\"\n\
         [a]\nb.c = 1\nb.d = 2\nb.e = 3\n[a.f]\n[[g]]\n[[g]]\n[[g]]\nh = [[1, 2], {i = 3, j.k = 4, j.l = 5}]\n"
            .to_string(),
    );
    for text in &texts {
        let root = ok(text);
        let mut tables = vec![&root];
        let mut values: Vec<&Value> = Vec::new();
        loop {
            if let Some(table) = tables.pop() {
                let entry = std::mem::size_of::<(Key, Value)>();
                assert_eq!(table.own_allocations()[0], table.len() * entry);
                for (key, value) in table.entries() {
                    assert_eq!(key.name.capacity(), key.name.len(), "{key:?}");
                    values.push(value);
                }
            } else if let Some(value) = values.pop() {
                match &value.kind {
                    ValueKind::String(string) => assert_eq!(string.capacity(), string.len()),
                    ValueKind::Array(array) => {
                        assert_eq!(array.items.capacity(), array.len());
                        values.extend(array.items());
                    }
                    ValueKind::Table(table) => tables.push(table),
                    _ => {}
                }
            } else {
                break;
            }
        }
    }
}
