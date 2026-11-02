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

//! Unit tests for the parser.
//!
//! Ported from nadir-codec `json/parse.rs` (its `tests` module). Adapted for
//! Tidepool: every case runs in both dialects, which must agree unless the
//! case is about JSONC; the depth default is 128, not 64; repeated keys
//! are kept, not rejected; and the limits on input size and number length,
//! byte input, the strict-mode errors, allocation sizes and failing without
//! recursing are new.

use super::*;
use ErrorKind::{
    CommentNotAllowed, ControlCharacter, InputTooLarge, InvalidEscape, InvalidLiteral, InvalidUtf8,
    LeadingZero, LoneSurrogate, NumberOutOfRange, NumberTooLong, StringTooLong, TooDeep,
    TooManyEntries, TrailingCommaNotAllowed, UnexpectedCharacter, UnexpectedEnd,
};

/// Line, column and kind of an error.
type Position = (usize, usize, ErrorKind);

const DEFAULT: Limits = Limits::DEFAULT;
const DEFAULT_DEPTH: usize = Limits::DEFAULT.max_depth;

fn try_parse(text: &str, dialect: Dialect, limits: Limits) -> Result<Value, CodecError> {
    parse(text, dialect, limits)
}

/// The value of `text`, which both dialects must parse alike.
fn ok(text: &str) -> Value {
    let json = try_parse(text, Dialect::Json, DEFAULT)
        .unwrap_or_else(|error| panic!("{text:?} should parse as JSON, but: {error}"));
    let jsonc = ok_jsonc(text);
    assert_eq!(json, jsonc, "the dialects disagree about {text:?}");
    json
}

/// The value of `text` in JSONC.
fn ok_jsonc(text: &str) -> Value {
    try_parse(text, Dialect::Jsonc, DEFAULT)
        .unwrap_or_else(|error| panic!("{text:?} should parse as JSONC, but: {error}"))
}

/// The line, column and kind of the error for `text` in `dialect`. Also
/// checks that the byte offset is consistent with them.
fn err_in(text: &str, dialect: Dialect, limits: Limits) -> Position {
    match try_parse(text, dialect, limits) {
        Err(CodecError::Json {
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
        Ok(value) => panic!("{text:?} should not parse as {dialect:?}, but gave {value:?}"),
    }
}

/// The error for `text`, which both dialects must report alike.
fn err_with(text: &str, limits: Limits) -> Position {
    let json = err_in(text, Dialect::Json, limits);
    let jsonc = err_in(text, Dialect::Jsonc, limits);
    assert_eq!(json, jsonc, "the dialects disagree about {text:?}");
    json
}

fn err(text: &str) -> Position {
    err_with(text, DEFAULT)
}

fn err_json(text: &str) -> Position {
    err_in(text, Dialect::Json, DEFAULT)
}

fn err_jsonc(text: &str) -> Position {
    err_in(text, Dialect::Jsonc, DEFAULT)
}

/// Test texts write a JSON `\u` escape as `~u`, so that an escape cannot be
/// mistaken for (or edited into) the character it stands for. This turns
/// each `~u` into `\u`.
fn esc(text: &str) -> String {
    text.replace("~u", "\\u")
}

fn found(found: char, expected: &'static str) -> ErrorKind {
    UnexpectedCharacter { found, expected }
}

fn end(expected: &'static str) -> ErrorKind {
    UnexpectedEnd { expected }
}

fn string(text: &str) -> Value {
    Value::from(text)
}

fn int(value: i64) -> Value {
    Value::from(value)
}

fn float(value: f64) -> Value {
    Value::Number(Number::from_f64(value).expect("finite"))
}

fn ints(values: &[i64]) -> Value {
    Value::Array(values.iter().copied().map(int).collect())
}

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

// --------------------------------------------------------------- scalars

#[test]
fn literals() {
    assert_eq!(ok("null"), Value::Null);
    assert_eq!(ok("true"), Value::Bool(true));
    assert_eq!(ok("false"), Value::Bool(false));
    assert_eq!(ok(" \t\r\n null \n"), Value::Null);
}

#[test]
fn misspelled_literals_are_rejected() {
    assert_eq!(err("nul"), (1, 1, InvalidLiteral));
    assert_eq!(err("none"), (1, 1, InvalidLiteral));
    assert_eq!(err("tru"), (1, 1, InvalidLiteral));
    assert_eq!(err("fals"), (1, 1, InvalidLiteral));
    assert_eq!(err("nan"), (1, 1, InvalidLiteral));
    assert_eq!(err("[1, nil]"), (1, 5, InvalidLiteral));
    // Other spellings do not even start like a literal.
    assert_eq!(err("True"), (1, 1, found('T', VALUE)));
    assert_eq!(err("NULL"), (1, 1, found('N', VALUE)));
    assert_eq!(err("undefined"), (1, 1, found('u', VALUE)));
    // A literal with something stuck to it.
    assert_eq!(err("nullx"), (1, 5, found('x', END)));
    assert_eq!(err("truefalse"), (1, 5, found('f', END)));
}

#[test]
fn integers() {
    assert_eq!(ok("0"), int(0));
    assert_eq!(ok("42"), int(42));
    assert_eq!(ok("-17"), int(-17));
    assert_eq!(ok("9223372036854775807"), int(i64::MAX));
    assert_eq!(ok("-9223372036854775808"), int(i64::MIN));
    // Above i64::MAX, integers are still exact up to u64::MAX.
    assert_eq!(ok("18446744073709551615"), Value::from(u64::MAX));
    assert_eq!(ok("9223372036854775808").as_u64(), Some(1 << 63));
}

#[test]
fn numbers_with_a_fraction_or_exponent_are_floats() {
    assert_eq!(ok("1.5"), float(1.5));
    assert_eq!(ok("-0.25"), float(-0.25));
    assert_eq!(ok("1.0"), float(1.0));
    assert_eq!(ok("0.1"), float(0.1));
    assert_eq!(ok("1e2"), float(100.0));
    assert_eq!(ok("1E+2"), float(100.0));
    assert_eq!(ok("25e-1"), float(2.5));
    assert_eq!(ok("-1.5E3"), float(-1500.0));
    assert_eq!(ok("0e0"), float(0.0));
    assert_eq!(ok("1.7976931348623157e308"), float(f64::MAX));
    assert_eq!(ok("5e-324"), float(5e-324));
    // Too small to represent rounds to zero, which is still a number.
    assert_eq!(ok("1e-999"), float(0.0));
    // A float is not an integer, even when it is a whole number.
    assert_eq!(ok("2.0").as_i64(), None);
    assert_ne!(ok("2.0"), int(2));
}

#[test]
fn integers_too_large_for_u64_or_i64_become_floats() {
    assert_eq!(
        ok("18446744073709551616"),
        float(18_446_744_073_709_551_616.0)
    );
    assert_eq!(
        ok("-9223372036854775809"),
        float(-9_223_372_036_854_775_808.0)
    );
    assert_eq!(
        ok("123456789012345678901234567890"),
        float(1.234_567_890_123_456_8e29)
    );
}

#[test]
fn negative_zero_keeps_its_sign() {
    for text in ["-0", "-0.0", "-0e0"] {
        let number = ok(text).as_number().expect("a number");
        assert!(!number.is_integer(), "{text} gave an integer");
        let value = number.as_f64();
        assert!(
            value == 0.0 && value.is_sign_negative(),
            "{text} gave {value}"
        );
    }
    assert_eq!(ok("0"), int(0));
}

#[test]
fn numbers_too_large_for_f64_are_rejected() {
    assert_eq!(err("1e999"), (1, 1, NumberOutOfRange));
    assert_eq!(err("-1e999"), (1, 1, NumberOutOfRange));
    assert_eq!(err("[0, 1.8e308]"), (1, 5, NumberOutOfRange));
    assert_eq!(err(&"9".repeat(400)), (1, 1, NumberOutOfRange));
    // A huge exponent is no different.
    assert_eq!(
        err(&format!("1e{}", "9".repeat(1000))),
        (1, 1, NumberOutOfRange)
    );
    assert_eq!(ok(&format!("1e-{}", "9".repeat(1000))), float(0.0));
}

#[test]
fn malformed_numbers_are_rejected() {
    assert_eq!(err("01"), (1, 1, LeadingZero));
    assert_eq!(err("-007"), (1, 1, LeadingZero));
    assert_eq!(err("[1, 00]"), (1, 5, LeadingZero));
    assert_eq!(err("-"), (1, 2, end(DIGIT)));
    assert_eq!(err("1."), (1, 3, end(DIGIT)));
    assert_eq!(err("1.e5"), (1, 3, found('e', DIGIT)));
    assert_eq!(err("1e"), (1, 3, end(DIGIT)));
    assert_eq!(err("1e+"), (1, 4, end(DIGIT)));
    assert_eq!(err("1e+x"), (1, 4, found('x', DIGIT)));
    assert_eq!(err("--1"), (1, 2, found('-', DIGIT)));
    assert_eq!(err("- 1"), (1, 2, found(' ', DIGIT)));
    assert_eq!(err(".5"), (1, 1, found('.', VALUE)));
    assert_eq!(err("+1"), (1, 1, found('+', VALUE)));
    assert_eq!(err("0x10"), (1, 2, found('x', END)));
    assert_eq!(err("1_000"), (1, 2, found('_', END)));
    assert_eq!(err("1.5.2"), (1, 4, found('.', END)));
    assert_eq!(err("Infinity"), (1, 1, found('I', VALUE)));
    assert_eq!(err("-Infinity"), (1, 2, found('I', DIGIT)));
    assert_eq!(err("NaN"), (1, 1, found('N', VALUE)));
}

// --------------------------------------------------------------- strings

#[test]
fn strings() {
    assert_eq!(ok(r#""""#), string(""));
    assert_eq!(ok(r#""hello world""#), string("hello world"));
    assert_eq!(ok("\"é€😀\u{7f}\""), string("é€😀\u{7f}"));
    // Comment markers inside a string are just text.
    assert_eq!(
        ok(r#""// not a comment /* nor this */""#),
        string("// not a comment /* nor this */")
    );
}

#[test]
fn escapes() {
    assert_eq!(
        ok(r#""\" \\ \/ \b \f \n \r \t""#),
        string("\" \\ / \u{8} \u{c} \n \r \t")
    );
    assert_eq!(ok(&esc(r#""~u0041~u00e9~u20AC~u0000""#)), string("Aé€\0"));
    assert_eq!(ok(&esc(r#""a~u0062c""#)), string("abc"));
    assert_eq!(ok(&esc(r#""~u00E9~u00e9""#)), string("éé"));
    // A surrogate pair is one character.
    assert_eq!(ok(&esc(r#""~ud83d~ude00""#)), string("😀"));
    assert_eq!(ok(&esc(r#""~uD83D~uDE00!""#)), string("😀!"));
    assert_eq!(ok(&esc(r#""~ud800~udc00""#)), string("\u{10000}"));
    assert_eq!(ok(&esc(r#""~udbff~udfff""#)), string("\u{10ffff}"));
    // Escaped or not, it is the same string.
    assert_eq!(ok(&esc(r#""~ud83d~ude00""#)), ok("\"😀\""));
    assert_eq!(esc("~u0041").len(), 6, "the helper makes a real escape");
}

#[test]
fn malformed_strings_are_rejected() {
    // Errors about a whole string point at its opening quote.
    assert_eq!(err(r#""abc"#), (1, 1, end(STRING_CLOSE)));
    assert_eq!(err(r#"[1, "abc]"#), (1, 5, end(STRING_CLOSE)));
    assert_eq!(err("'abc'"), (1, 1, found('\'', VALUE)));
    // Errors about an escape point at its backslash.
    assert_eq!(err(r#""a\x""#), (1, 3, InvalidEscape));
    assert_eq!(err(r#""a\ ""#), (1, 3, InvalidEscape));
    assert_eq!(err("\"a\\é\""), (1, 3, InvalidEscape));
    assert_eq!(err(r#""a\"#), (1, 3, end(ESCAPE_REST)));
    assert_eq!(err(r#""\u12""#), (1, 2, InvalidEscape));
    assert_eq!(err(r#""\u12G4""#), (1, 2, InvalidEscape));
    assert_eq!(err(r#""\u+123""#), (1, 2, InvalidEscape));
    assert_eq!(err(r#""\U0041""#), (1, 2, InvalidEscape));
    assert_eq!(err(r#""\u12"#), (1, 2, end(HEX_DIGITS)));
    assert_eq!(err(r#""\u"#), (1, 2, end(HEX_DIGITS)));
}

#[test]
fn only_the_rfc_escapes_are_accepted() {
    // Every ASCII byte after a backslash: exactly eight are short escapes,
    // and `u` needs hex digits. JSON5's `\v`, `\'`, `\0` and line
    // continuations are errors like the rest.
    for byte in 0u8..=0x7f {
        let text = format!("\"\\{}\"", char::from(byte));
        if matches!(byte, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') {
            assert_eq!(ok(&text).as_str().map(str::len), Some(1), "{text:?}");
        } else {
            assert_eq!(err(&text), (1, 2, InvalidEscape), "{text:?}");
        }
    }
}

#[test]
fn unpaired_surrogates_are_rejected() {
    assert_eq!(err(&esc(r#""~ud800""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~udc00""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800~u0041""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800~ud83d~ude00""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800A""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800x""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800\n""#)), (1, 2, LoneSurrogate));
    assert_eq!(err(&esc(r#""ab~ude00~ud83d""#)), (1, 4, LoneSurrogate));
    assert_eq!(err(&esc(r#""~ud800"#)), (1, 2, LoneSurrogate));
    // Inverted: low, then high.
    assert_eq!(err(&esc(r#""~udd1e~ud834""#)), (1, 2, LoneSurrogate));
    // The second half is itself malformed.
    assert_eq!(err(&esc(r#""~ud83d~uZZZZ""#)), (1, 8, InvalidEscape));
}

#[test]
fn raw_control_characters_in_strings_are_rejected() {
    assert_eq!(err("\"a\nb\""), (1, 3, ControlCharacter));
    assert_eq!(err("\"a\tb\""), (1, 3, ControlCharacter));
    assert_eq!(err("\"\0\""), (1, 2, ControlCharacter));
    assert_eq!(err("\"\u{1f}\""), (1, 2, ControlCharacter));
    assert_eq!(err("\"ok\",\n\"\r\""), (1, 5, found(',', END)));
    // DEL and the C1 controls are not in the RFC's range.
    assert_eq!(ok("\"\u{7f}\u{80}\u{9f}\""), string("\u{7f}\u{80}\u{9f}"));
}

// ---------------------------------------------------- arrays and objects

#[test]
fn arrays() {
    assert_eq!(ok("[]"), ints(&[]));
    assert_eq!(ok("[ \n ]"), ints(&[]));
    assert_eq!(ok("[1]"), ints(&[1]));
    assert_eq!(ok(" [ 1 , 2 ,3 ] "), ints(&[1, 2, 3]));
    assert_eq!(
        ok(r#"[1, "a", null, true, [2, []], {}]"#),
        Value::Array(vec![
            int(1),
            string("a"),
            Value::Null,
            Value::Bool(true),
            Value::Array(vec![int(2), ints(&[])]),
            object(&[]),
        ])
    );
}

#[test]
fn objects_keep_their_members_in_document_order() {
    assert_eq!(ok("{}"), object(&[]));
    assert_eq!(ok("{ }"), object(&[]));
    assert_eq!(
        ok(r#"{"zebra": 1, "apple": {"m": [], "b": null}, "": false}"#),
        object(&[
            ("zebra", int(1)),
            ("apple", object(&[("m", ints(&[])), ("b", Value::Null)])),
            ("", Value::Bool(false)),
        ])
    );
    // The same key in different objects is fine.
    assert_eq!(
        ok(r#"{"a": {"a": 1}, "b": {"a": 2}}"#),
        object(&[
            ("a", object(&[("a", int(1))])),
            ("b", object(&[("a", int(2))]))
        ])
    );
}

#[test]
fn containers_close_back_to_the_right_parent() {
    assert_eq!(
        ok(r#"[[1, [2]], {"a": [3, {"b": 4}], "c": 5}, 6]"#),
        Value::Array(vec![
            Value::Array(vec![int(1), ints(&[2])]),
            object(&[
                ("a", Value::Array(vec![int(3), object(&[("b", int(4))])])),
                ("c", int(5)),
            ]),
            int(6),
        ])
    );
    assert_eq!(
        ok(r#"{"a": {"b": {"c": []}}, "d": [[[]]]}"#),
        object(&[
            ("a", object(&[("b", object(&[("c", ints(&[]))]))])),
            ("d", Value::Array(vec![Value::Array(vec![ints(&[])])])),
        ])
    );
}

#[test]
fn malformed_arrays_are_rejected() {
    assert_eq!(err("[1 2]"), (1, 4, found('2', ARRAY_NEXT)));
    assert_eq!(err("[,1]"), (1, 2, found(',', ARRAY_ITEM)));
    assert_eq!(err("[,]"), (1, 2, found(',', ARRAY_ITEM)));
    assert_eq!(err("[1}"), (1, 3, found('}', ARRAY_NEXT)));
    assert_eq!(err("[1:2]"), (1, 3, found(':', ARRAY_NEXT)));
    assert_eq!(err("["), (1, 2, end(ARRAY_ITEM)));
    assert_eq!(err("[1"), (1, 3, end(ARRAY_NEXT)));
    assert_eq!(err("[[1]"), (1, 5, end(ARRAY_NEXT)));
    assert_eq!(err("]"), (1, 1, found(']', VALUE)));
    // After a comma, only JSONC also allows `]`.
    assert_eq!(err_json("[1,,2]"), (1, 4, found(',', VALUE)));
    assert_eq!(err_jsonc("[1,,2]"), (1, 4, found(',', ARRAY_ITEM)));
    assert_eq!(err_json("[1,"), (1, 4, end(VALUE)));
    assert_eq!(err_jsonc("[1,"), (1, 4, end(ARRAY_ITEM)));
}

#[test]
fn malformed_objects_are_rejected() {
    assert_eq!(err("{,}"), (1, 2, found(',', KEY_OR_CLOSE)));
    assert_eq!(err(r#"{"a" 1}"#), (1, 6, found('1', COLON)));
    assert_eq!(err(r#"{"a":}"#), (1, 6, found('}', VALUE)));
    assert_eq!(err(r#"{"a"}"#), (1, 5, found('}', COLON)));
    assert_eq!(err("{a: 1}"), (1, 2, found('a', KEY_OR_CLOSE)));
    assert_eq!(err("{1: 2}"), (1, 2, found('1', KEY_OR_CLOSE)));
    assert_eq!(err(r#"{"a": 1 "b": 2}"#), (1, 9, found('"', OBJECT_NEXT)));
    assert_eq!(err(r#"{"a":1]"#), (1, 7, found(']', OBJECT_NEXT)));
    assert_eq!(err(r#"{"a"=1}"#), (1, 5, found('=', COLON)));
    assert_eq!(err("{"), (1, 2, end(KEY_OR_CLOSE)));
    assert_eq!(err(r#"{"a""#), (1, 5, end(COLON)));
    assert_eq!(err(r#"{"a":"#), (1, 6, end(VALUE)));
    assert_eq!(err(r#"{"a":1"#), (1, 7, end(OBJECT_NEXT)));
    assert_eq!(err("}"), (1, 1, found('}', VALUE)));
    // After a comma, only JSONC also allows `}`.
    assert_eq!(err_json(r#"{"a":1,,}"#), (1, 8, found(',', KEY)));
    assert_eq!(err_jsonc(r#"{"a":1,,}"#), (1, 8, found(',', KEY_OR_CLOSE)));
    assert_eq!(err_json(r#"{"a":1,"#), (1, 8, end(KEY)));
    assert_eq!(err_jsonc(r#"{"a":1,"#), (1, 8, end(KEY_OR_CLOSE)));
}

#[test]
fn empty_and_trailing_text_is_rejected() {
    assert_eq!(err(""), (1, 1, end(VALUE)));
    assert_eq!(err("   "), (1, 4, end(VALUE)));
    assert_eq!(err("\n\n"), (3, 1, end(VALUE)));
    assert_eq!(err("1 2"), (1, 3, found('2', END)));
    assert_eq!(err("{} x"), (1, 4, found('x', END)));
    assert_eq!(err("[]]"), (1, 3, found(']', END)));
    assert_eq!(err("null,"), (1, 5, found(',', END)));
    assert_eq!(err(r#""a" "b""#), (1, 5, found('"', END)));
    assert_eq!(err_json("// only a comment"), (1, 1, CommentNotAllowed));
    assert_eq!(err_jsonc("// only a comment"), (1, 18, end(VALUE)));
}

#[test]
fn only_the_four_rfc_whitespace_bytes_are_whitespace() {
    for space in [
        "\u{b}", "\u{c}", "\u{a0}", "\u{2028}", "\u{feff}", "\u{3000}",
    ] {
        let text = format!("{{\"a\":{space}2}}");
        let found_char = space.chars().next().expect("one character");
        assert_eq!(err(&text), (1, 6, found(found_char, VALUE)), "{text:?}");
    }
}

// ------------------------------------------------------------------ JSONC

#[test]
fn line_comments() {
    assert_eq!(ok_jsonc("// leading\n1"), int(1));
    assert_eq!(ok_jsonc("1 // trailing"), int(1));
    assert_eq!(ok_jsonc("1 //"), int(1));
    assert_eq!(ok_jsonc("1//\n"), int(1));
    assert_eq!(
        ok_jsonc("// one\r\n// two\r\n[1, // three\n 2]"),
        ints(&[1, 2])
    );
    // A lone CR ends a line comment too.
    assert_eq!(ok_jsonc("[1, // one\r 2]"), ints(&[1, 2]));
    // `#` does not start a comment.
    assert_eq!(err("# no\n1"), (1, 1, found('#', VALUE)));
}

#[test]
fn block_comments() {
    assert_eq!(ok_jsonc("/* leading */ 1 /* trailing */"), int(1));
    assert_eq!(ok_jsonc("/**/1"), int(1));
    assert_eq!(ok_jsonc("/***/1/****/"), int(1));
    assert_eq!(ok_jsonc("/* * / ** // */1"), int(1));
    assert_eq!(
        ok_jsonc("/* spans\n   several\n   lines */\n[1]"),
        ints(&[1])
    );
    assert_eq!(ok_jsonc("/* a *//* b */1"), int(1));
}

#[test]
fn comments_are_allowed_wherever_whitespace_is() {
    let text = r#"/*a*/ { /*b*/ "k" /*c*/ : /*d*/ [ /*e*/ 1 /*f*/ , /*g*/ 2 /*h*/ ] /*i*/ , // j
        "n" // k
        : // l
        null // m
    } // n"#;
    assert_eq!(
        ok_jsonc(text),
        object(&[("k", ints(&[1, 2])), ("n", Value::Null)])
    );
}

#[test]
fn comments_are_errors_in_strict_json() {
    assert_eq!(err_json("// leading\n1"), (1, 1, CommentNotAllowed));
    assert_eq!(err_json("1 // trailing"), (1, 3, CommentNotAllowed));
    assert_eq!(err_json("/* c */ 1"), (1, 1, CommentNotAllowed));
    assert_eq!(err_json("[1, /* c */ 2]"), (1, 5, CommentNotAllowed));
    assert_eq!(err_json("{\"a\" /* c */: 1}"), (1, 6, CommentNotAllowed));
    assert_eq!(err_json("1 /* never closed"), (1, 3, CommentNotAllowed));
}

#[test]
fn malformed_comments_are_rejected() {
    // An unterminated comment is reported where it starts.
    assert_eq!(err_jsonc("1 /* never closed"), (1, 3, end(COMMENT_CLOSE)));
    assert_eq!(err_jsonc("/*/"), (1, 1, end(COMMENT_CLOSE)));
    assert_eq!(err_jsonc("[1, /* open\n\n"), (1, 5, end(COMMENT_CLOSE)));
    // Block comments do not nest: the first `*/` ends the comment.
    assert_eq!(err_jsonc("/* /* */ */ 1"), (1, 10, found('*', VALUE)));
    // A lone slash is not a comment in either dialect.
    assert_eq!(err("/ 1"), (1, 1, found('/', VALUE)));
    assert_eq!(err("1 /"), (1, 3, found('/', END)));
    assert_eq!(err("[1 / 2]"), (1, 4, found('/', ARRAY_NEXT)));
}

#[test]
fn trailing_commas() {
    assert_eq!(ok_jsonc("[1,]"), ints(&[1]));
    assert_eq!(ok_jsonc("[1, 2, ]"), ints(&[1, 2]));
    assert_eq!(
        ok_jsonc("[[1,],[2,],]"),
        Value::Array(vec![ints(&[1]), ints(&[2])])
    );
    assert_eq!(ok_jsonc(r#"{"a":1,}"#), object(&[("a", int(1))]));
    assert_eq!(ok_jsonc("{\"a\": [1,\n],\n}"), object(&[("a", ints(&[1]))]));
    assert_eq!(ok_jsonc("[1, /* gone */ ]"), ints(&[1]));
    assert_eq!(ok_jsonc("[1, // gone\n]"), ints(&[1]));
}

#[test]
fn trailing_commas_are_errors_in_strict_json() {
    // Reported at the comma.
    assert_eq!(err_json("[1,]"), (1, 3, TrailingCommaNotAllowed));
    assert_eq!(err_json("[1, 2, ]"), (1, 6, TrailingCommaNotAllowed));
    assert_eq!(err_json(r#"{"a":1,}"#), (1, 7, TrailingCommaNotAllowed));
    assert_eq!(err_json("[[1],\n]"), (1, 5, TrailingCommaNotAllowed));
}

#[test]
fn only_one_trailing_comma_and_only_after_an_entry() {
    assert_eq!(err_json("[1,,]"), (1, 4, found(',', VALUE)));
    assert_eq!(err_jsonc("[1,,]"), (1, 4, found(',', ARRAY_ITEM)));
    assert_eq!(err("[,]"), (1, 2, found(',', ARRAY_ITEM)));
    assert_eq!(err("{,}"), (1, 2, found(',', KEY_OR_CLOSE)));
    // Not at the top level either.
    assert_eq!(err("1,"), (1, 2, found(',', END)));
}

#[test]
fn a_leading_byte_order_mark_is_skipped() {
    assert_eq!(ok("\u{feff}1"), int(1));
    assert_eq!(ok("\u{feff} {}"), object(&[]));
    // It does not count as a column, though it counts in the offset.
    assert_eq!(err("\u{feff}x"), (1, 1, found('x', VALUE)));
    assert_eq!(err("\u{feff}[1 2]"), (1, 4, found('2', ARRAY_NEXT)));
    match try_parse("\u{feff}x", Dialect::Json, DEFAULT) {
        Err(CodecError::Json { offset, .. }) => assert_eq!(offset, 3),
        other => panic!("{other:?}"),
    }
    // Only at the very start.
    assert_eq!(err("1\u{feff}"), (1, 2, found('\u{feff}', END)));
    assert_eq!(err("\u{feff}\u{feff}1"), (1, 1, found('\u{feff}', VALUE)));
    // A byte-order mark alone is not a document.
    assert_eq!(err("\u{feff}"), (1, 1, end(VALUE)));
}

// ---------------------------------------------------------- repeated keys

#[test]
fn repeated_keys_are_kept_in_document_order() {
    let value = ok(r#"{"a":1,"b":2,"a":3}"#);
    assert_eq!(
        value,
        object(&[("a", int(1)), ("b", int(2)), ("a", int(3))])
    );
    // `get` returns the last, as `JSON.parse` does.
    assert_eq!(value.get("a"), Some(&int(3)));
    // JSONTestSuite's `y_object_duplicated_key` and `..._and_value`.
    assert_eq!(ok(r#"{"a":"b","a":"c"}"#).get("a"), Some(&string("c")));
    assert_eq!(
        ok(r#"{"a":"b","a":"b"}"#),
        object(&[("a", string("b")), ("a", string("b"))])
    );
    // Keys are compared after escapes are decoded.
    let escaped = ok(&esc(r#"{"a":1,"~u0061":2}"#));
    assert_eq!(escaped, object(&[("a", int(1)), ("a", int(2))]));
    assert_eq!(escaped.get("a"), Some(&int(2)));
    assert_eq!(ok(r#"{"":1,"":2}"#).get(""), Some(&int(2)));
    // In nested and large objects too.
    let nested = ok(r#"{"x":{"k":1,"k":2},"x":{"k":3}}"#);
    assert_eq!(nested.as_object().map(<[_]>::len), Some(2));
    assert_eq!(nested.get("x").and_then(|x| x.get("k")), Some(&int(3)));
    let (text, _) = numbered_object(200, &[(150, 7), (199, 7)]);
    let large = ok(&text);
    assert_eq!(large.as_object().map(<[_]>::len), Some(200));
    assert_eq!(large.get("k7"), Some(&int(199)));
}

#[test]
fn keys_that_differ_only_in_case_or_form_are_distinct() {
    let cased = ok(r#"{"a":1,"A":2}"#);
    assert_eq!(cased.get("a"), Some(&int(1)));
    assert_eq!(cased.get("A"), Some(&int(2)));
    // `é` as one character and as `e` with a combining acute accent.
    let forms = ok(&esc(r#"{"é":1,"e~u0301":2}"#));
    assert_eq!(forms.get("é"), Some(&int(1)));
    assert_eq!(forms.get("e\u{301}"), Some(&int(2)));
    let (mut text, _) = numbered_object(20, &[]);
    text.pop();
    text.push_str(r#","K0":20}"#);
    let large = ok(&text);
    assert_eq!(large.as_object().map(<[_]>::len), Some(21));
    assert_eq!(large.get("k0"), Some(&int(0)));
    assert_eq!(large.get("K0"), Some(&int(20)));
}

/// An object with keys `k0`, `k1`, ..., where `repeats` lists
/// `(position, key number)` pairs to overwrite. Member `n` has the value
/// `n`. Returns the text and the byte offset of each key.
fn numbered_object(len: usize, repeats: &[(usize, usize)]) -> (String, Vec<usize>) {
    let mut text = String::from("{");
    let mut offsets = Vec::new();
    for position in 0..len {
        let number = repeats
            .iter()
            .find(|&&(at, _)| at == position)
            .map_or(position, |&(_, number)| number);
        if position > 0 {
            text.push(',');
        }
        offsets.push(text.len());
        text.push_str(&format!("\"k{number}\":{position}"));
    }
    text.push('}');
    (text, offsets)
}

// ------------------------------------------------------------ allocation

/// Asserts that no array or object in `value` has spare capacity.
fn assert_no_spare_capacity(value: &Value, text: &str) {
    let mut work = vec![value];
    while let Some(value) = work.pop() {
        match value {
            Value::Array(items) => {
                assert_eq!(items.capacity(), items.len(), "an array in {text:?}");
                work.extend(items);
            }
            Value::Object(members) => {
                assert_eq!(members.capacity(), members.len(), "an object in {text:?}");
                work.extend(members.iter().map(|(_, value)| value));
            }
            _ => {}
        }
    }
}

#[test]
fn arrays_and_objects_are_allocated_at_exactly_their_size() {
    let long = format!("[{}0]", "0,".repeat(1000));
    let texts = [
        r#"[[0],[[0]],{"a":[0]}]"#,
        "[0,0]",
        "[0,0,0]",
        "[0,0,0,0]",
        "[0,0,0,0,0]",
        r#"{"a":1,"b":2,"c":3}"#,
        r#"[{"a":{"b":[1,2,3,4,5,6,7,8,9]}},[],{}]"#,
        &long,
    ];
    for text in texts {
        assert_no_spare_capacity(&ok(text), text);
    }
    for text in ["[0,0,]", "[[0,],[0,0,],]", r#"{"a":[1,2,],"b":{"c":3,},}"#] {
        assert_no_spare_capacity(&ok_jsonc(text), text);
    }
}

#[test]
fn a_failed_parse_does_not_overflow_the_stack_however_deep() {
    // With the depth limit raised, a deep tree is built before the error is
    // found. Dropping it the ordinary way would recurse once per level and
    // overflow this thread's 2 MiB stack, aborting the test run.
    let deep = 1_000_000;
    let limits = Limits {
        max_depth: deep + 1,
        ..DEFAULT
    };
    let run = move || {
        let inside = nested("[", "]", deep);
        [
            // The outer array never closes.
            format!("[{inside} x"),
            // The value is complete, but text follows it.
            format!("{inside} x"),
            // Inside an object, then the object never closes.
            format!("{{\"a\":{inside} x"),
        ]
        .map(|text| match try_parse(&text, Dialect::Json, limits) {
            Ok(value) => {
                dismantle(vec![value]);
                None
            }
            Err(CodecError::Json { kind, .. }) => Some(kind),
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
            Some(found('x', END)),
            Some(found('x', OBJECT_NEXT)),
        ]
    );
}

// ---------------------------------------------------------------- limits

fn nested(open: &str, close: &str, depth: usize) -> String {
    format!("{}{}", open.repeat(depth), close.repeat(depth))
}

fn depth_of(mut value: &Value) -> usize {
    let mut depth = 0;
    while let Some(inner) = value.as_array() {
        depth += 1;
        match inner.first() {
            Some(next) => value = next,
            None => break,
        }
    }
    depth
}

#[test]
fn limit_max_depth_accepts_nesting_up_to_the_limit() {
    assert_eq!(DEFAULT_DEPTH, 128);
    assert_eq!(
        depth_of(&ok(&nested("[", "]", DEFAULT_DEPTH))),
        DEFAULT_DEPTH
    );
    let objects = nested("{\"a\":", "}", DEFAULT_DEPTH).replace(":}", ":1}");
    assert!(ok(&objects).get("a").is_some());
}

#[test]
fn limit_max_depth_rejects_the_bracket_that_goes_too_deep() {
    let too_deep = TooDeep {
        limit: DEFAULT_DEPTH,
    };
    assert_eq!(
        err(&nested("[", "]", DEFAULT_DEPTH + 1)),
        (1, DEFAULT_DEPTH + 1, too_deep.clone())
    );
    assert_eq!(
        err(&"[".repeat(DEFAULT_DEPTH + 1)),
        (1, DEFAULT_DEPTH + 1, too_deep.clone())
    );
    // Objects and arrays count alike: 64 of each is fine, one more is not.
    let mixed = "{\"a\":[".repeat(DEFAULT_DEPTH / 2);
    let column = DEFAULT_DEPTH / 2 * 6 + 1;
    assert_eq!(err(&format!("{mixed}{{")), (1, column, too_deep.clone()));
    assert_eq!(err(&format!("{mixed}[")), (1, column, too_deep.clone()));
    // An empty container counts too.
    assert_eq!(err(&format!("{mixed}[]")), (1, column, too_deep));
    assert_eq!(err(&format!("{mixed}1")), (1, column + 1, end(ARRAY_NEXT)));
}

#[test]
fn limit_max_depth_stops_very_deep_nesting_without_overflowing_the_stack() {
    let too_deep = TooDeep {
        limit: DEFAULT_DEPTH,
    };
    assert_eq!(
        err(&"[".repeat(1_000_000)),
        (1, DEFAULT_DEPTH + 1, too_deep.clone())
    );
    assert_eq!(
        err(&"{\"a\":".repeat(1_000_000)),
        (1, DEFAULT_DEPTH * 5 + 1, too_deep.clone())
    );
    assert_eq!(
        err(&"[{\"a\":".repeat(500_000)),
        (1, DEFAULT_DEPTH / 2 * 6 + 1, too_deep)
    );
}

#[test]
fn limit_max_depth_is_configurable() {
    let depth = |max_depth| Limits {
        max_depth,
        ..DEFAULT
    };
    assert!(try_parse("1", Dialect::Json, depth(0)).is_ok());
    assert_eq!(err_with("[]", depth(0)), (1, 1, TooDeep { limit: 0 }));
    assert_eq!(err_with("{}", depth(0)), (1, 1, TooDeep { limit: 0 }));
    assert!(try_parse("[[1], {\"a\": 2}]", Dialect::Json, depth(2)).is_ok());
    assert_eq!(err_with("[[[1]]]", depth(2)), (1, 3, TooDeep { limit: 2 }));
    assert_eq!(
        err_with("[[1], {\"a\": [2]}]", depth(2)),
        (1, 13, TooDeep { limit: 2 })
    );
    assert!(try_parse(&nested("[", "]", 300), Dialect::Json, depth(300)).is_ok());
}

#[test]
fn the_parser_itself_does_not_recurse() {
    // With the limit raised, a document far deeper than any thread's stack
    // could take in recursive calls still parses.
    let deep = 200_000;
    let limits = Limits {
        max_depth: deep,
        ..DEFAULT
    };
    let value = try_parse(&nested("[", "]", deep), Dialect::Json, limits).expect("parses");
    assert_eq!(depth_of(&value), deep);
    // Dropping a value recurses once per level (see `Limits::max_depth`),
    // so this one is taken apart the way a failed parse does it.
    dismantle(vec![value]);
}

#[test]
fn limit_max_string_bytes() {
    let limits = Limits {
        max_string_bytes: 5,
        ..DEFAULT
    };
    let too_long = StringTooLong { limit: 5 };
    assert!(try_parse(r#""12345""#, Dialect::Json, limits).is_ok());
    assert_eq!(err_with(r#""123456""#, limits), (1, 1, too_long.clone()));
    // The limit is on the decoded bytes: `é` is two, however written.
    assert!(try_parse(&esc(r#""~u00e9~u00e9x""#), Dialect::Json, limits).is_ok());
    assert!(try_parse("\"ééx\"", Dialect::Json, limits).is_ok());
    assert_eq!(
        err_with(&esc(r#""~u00e9~u00e9xy""#), limits),
        (1, 1, too_long.clone())
    );
    assert_eq!(
        err_with(&esc(r#""1234~u00e9""#), limits),
        (1, 1, too_long.clone())
    );
    assert!(try_parse(&esc(r#""123~u00e9""#), Dialect::Json, limits).is_ok());
    assert_eq!(err_with("\"ééé\"", limits), (1, 1, too_long.clone()));
    assert_eq!(err_with(r#""12345\n""#, limits), (1, 1, too_long.clone()));
    // Keys are strings too. The error points at the string's start.
    assert_eq!(
        err_with(r#"{"123456": 1}"#, limits),
        (1, 2, too_long.clone())
    );
    assert_eq!(err_with(r#"["ok", "toolong"]"#, limits), (1, 8, too_long));
    // An empty limit still allows the empty string.
    let none = Limits {
        max_string_bytes: 0,
        ..DEFAULT
    };
    assert!(try_parse(r#""""#, Dialect::Json, none).is_ok());
    assert_eq!(err_with(r#""a""#, none), (1, 1, StringTooLong { limit: 0 }));
}

#[test]
fn limit_max_string_bytes_default_is_sixteen_mebibytes() {
    let limit = 16 * 1024 * 1024;
    assert_eq!(DEFAULT.max_string_bytes, limit);
    let exactly = format!("\"{}\"", "x".repeat(limit));
    assert!(try_parse(&exactly, Dialect::Json, DEFAULT).is_ok());
    let over = format!("\"{}\"", "x".repeat(limit + 1));
    assert_eq!(
        err_in(&over, Dialect::Json, DEFAULT),
        (1, 1, StringTooLong { limit })
    );
}

#[test]
fn limit_max_number_bytes() {
    let limits = Limits {
        max_number_bytes: 4,
        ..DEFAULT
    };
    let too_long = NumberTooLong { limit: 4 };
    for text in ["1234", "-123", "1.25", "1e10", "-0.5"] {
        assert!(
            try_parse(text, Dialect::Json, limits).is_ok(),
            "{text} fits"
        );
    }
    assert_eq!(err_with("12345", limits), (1, 1, too_long.clone()));
    assert_eq!(err_with("-1234", limits), (1, 1, too_long.clone()));
    assert_eq!(err_with("1.5e1", limits), (1, 1, too_long.clone()));
    assert_eq!(err_with("[1, 12345]", limits), (1, 5, too_long.clone()));
    assert_eq!(err_with(r#"{"a": 0.0001}"#, limits), (1, 7, too_long));
}

#[test]
fn limit_max_number_bytes_default_allows_long_but_not_endless_numbers() {
    let limit = DEFAULT.max_number_bytes;
    assert_eq!(limit, 1024);
    // `0.000…0` with 1,022 zeros after the point is exactly 1,024 bytes.
    let exactly = format!("0.{}", "0".repeat(limit - 2));
    assert_eq!(ok(&exactly), float(0.0));
    let over = format!("0.{}", "0".repeat(limit - 1));
    assert_eq!(err(&over), (1, 1, NumberTooLong { limit }));
    let digits = format!("[{}]", "1".repeat(100_000));
    assert_eq!(err(&digits), (1, 2, NumberTooLong { limit }));
}

#[test]
fn limit_max_entries() {
    let limits = Limits {
        max_entries: 3,
        ..DEFAULT
    };
    let too_many = TooManyEntries { limit: 3 };
    assert!(try_parse("[1,2,3]", Dialect::Json, limits).is_ok());
    assert!(try_parse("[1,2,3,]", Dialect::Jsonc, limits).is_ok());
    // The error points at the entry that does not fit.
    assert_eq!(err_with("[1,2,3,4]", limits), (1, 8, too_many.clone()));
    assert!(try_parse(r#"{"a":1,"b":2,"c":3}"#, Dialect::Json, limits).is_ok());
    assert_eq!(
        err_with(r#"{"a":1,"b":2,"c":3,"d":4}"#, limits),
        (1, 20, too_many.clone())
    );
    // The limit is per container, not for the whole document.
    assert!(try_parse("[[1,2,3],[4,5,6],[7,8,9]]", Dialect::Json, limits).is_ok());
    assert_eq!(err_with("[[1,2,3],[4,5,6,7]]", limits), (1, 17, too_many));
    let none = Limits {
        max_entries: 0,
        ..DEFAULT
    };
    assert!(try_parse("[]", Dialect::Json, none).is_ok());
    assert!(try_parse("{}", Dialect::Json, none).is_ok());
    assert_eq!(err_with("[1]", none), (1, 2, TooManyEntries { limit: 0 }));
    assert_eq!(
        err_with(r#"{"a":1}"#, none),
        (1, 2, TooManyEntries { limit: 0 })
    );
}

#[test]
fn limit_max_entries_default_is_one_million() {
    let limit = DEFAULT.max_entries;
    assert_eq!(limit, 1_000_000);
    let mut text = "0,".repeat(limit);
    text.insert(0, '[');
    text.pop();
    text.push(']');
    assert_eq!(
        try_parse(&text, Dialect::Json, DEFAULT)
            .expect("a million entries fit")
            .as_array()
            .map(<[_]>::len),
        Some(limit)
    );
    text.insert_str(1, "0,");
    assert_eq!(
        err_in(&text, Dialect::Json, DEFAULT),
        (1, 2 * limit + 2, TooManyEntries { limit })
    );
}

#[test]
fn limit_max_input_bytes() {
    let limits = Limits {
        max_input_bytes: 4,
        ..DEFAULT
    };
    let too_large = |length| InputTooLarge { length, limit: 4 };
    assert!(try_parse("1234", Dialect::Json, limits).is_ok());
    assert!(parse_bytes(b"[  ]", Dialect::Json, limits).is_ok());
    // Reported at the first byte past the limit.
    assert_eq!(err_with("12345", limits), (1, 5, too_large(5)));
    assert_eq!(err_with("[1,\n2]", limits), (2, 1, too_large(6)));
    // A byte-order mark counts against the limit.
    assert_eq!(err_with("\u{feff}12", limits), (1, 2, too_large(5)));
    match parse_bytes(b"[1, 2]", Dialect::Json, limits) {
        Err(CodecError::Json { offset, kind, .. }) => {
            assert_eq!((offset, kind), (4, too_large(6)));
        }
        other => panic!("{other:?}"),
    }
    // The size is checked before anything else, the UTF-8 included.
    match parse_bytes(b"\xff\xff\xff\xff\xff", Dialect::Json, limits) {
        Err(CodecError::Json { kind, .. }) => assert_eq!(kind, too_large(5)),
        other => panic!("{other:?}"),
    }
    let none = Limits {
        max_input_bytes: 0,
        ..DEFAULT
    };
    assert_eq!(err_with("", none), (1, 1, end(VALUE)));
    assert!(parse_bytes(b"", Dialect::Json, none).is_err());
    assert_eq!(
        err_with("1", none),
        (
            1,
            1,
            InputTooLarge {
                length: 1,
                limit: 0
            }
        )
    );
}

#[test]
fn limit_max_input_bytes_reports_the_character_that_does_not_fit() {
    let limit = |max_input_bytes| Limits {
        max_input_bytes,
        ..DEFAULT
    };
    let too_large = |length, limit| InputTooLarge { length, limit };
    // `é` is bytes 1 and 2 of `"é"`: a limit of 2 cuts it in half, and the
    // error is at its start, never inside it.
    assert_eq!(err_with("\"é\"", limit(2)), (1, 2, too_large(4, 2)));
    assert_eq!(err_with("\"é\"", limit(1)), (1, 2, too_large(4, 1)));
    assert_eq!(err_with("\"é\"", limit(3)), (1, 3, too_large(4, 3)));
    // `😀` is bytes 1 to 4 of `"😀"`.
    for cut in 1..=4 {
        assert_eq!(err_with("\"😀\"", limit(cut)), (1, 2, too_large(6, cut)));
    }
    assert_eq!(err_with("\"😀\"", limit(5)), (1, 3, too_large(6, 5)));
    assert_eq!(err_with("\"é\n😀\"", limit(6)), (2, 1, too_large(9, 6)));
    // Bytes that are not UTF-8 are checked after the size, so the walk back
    // stops after three continuation bytes, as far as a character goes.
    match parse_bytes(b"\x80\x80\x80\x80\x80\x80", Dialect::Json, limit(4)) {
        Err(CodecError::Json { offset, kind, .. }) => {
            assert_eq!((offset, kind), (1, too_large(6, 4)));
        }
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------ byte input

/// The offset, line, column and kind of the error for `bytes`.
fn bytes_err(bytes: &[u8]) -> (usize, usize, usize, ErrorKind) {
    let json = parse_bytes(bytes, Dialect::Json, DEFAULT);
    let jsonc = parse_bytes(bytes, Dialect::Jsonc, DEFAULT);
    assert_eq!(json, jsonc, "the dialects disagree about {bytes:?}");
    match json {
        Err(CodecError::Json {
            offset,
            line,
            column,
            kind,
        }) => (offset, line, column, kind),
        Ok(value) => panic!("{bytes:?} should not parse, but gave {value:?}"),
    }
}

#[test]
fn bytes_that_are_utf8_parse_like_text() {
    let text = "{\"a\": [1, \"é😀\"]}";
    assert_eq!(
        parse_bytes(text.as_bytes(), Dialect::Json, DEFAULT),
        Ok(ok(text))
    );
    assert_eq!(
        parse_bytes(b"\xef\xbb\xbf{}", Dialect::Json, DEFAULT),
        Ok(object(&[]))
    );
}

#[test]
fn bytes_that_are_not_utf8_are_rejected_where_they_go_wrong() {
    assert_eq!(bytes_err(b"\xff"), (0, 1, 1, InvalidUtf8));
    assert_eq!(bytes_err(b"[\"a\xffb\"]"), (3, 1, 4, InvalidUtf8));
    assert_eq!(bytes_err(b"[\"\xc3\xa9\", \xe5]"), (7, 1, 7, InvalidUtf8));
    assert_eq!(bytes_err(b"[1,\n \"\xc0\xaf\"]"), (6, 2, 3, InvalidUtf8));
    // A surrogate encoded in UTF-8 is not UTF-8.
    assert_eq!(bytes_err(b"\"\xed\xa0\x80\""), (1, 1, 2, InvalidUtf8));
    // Truncated in the middle of a character.
    assert_eq!(bytes_err(b"\"\xe2\x82"), (1, 1, 2, InvalidUtf8));
    // A broken byte-order mark.
    assert_eq!(bytes_err(b"\xef\xbb{}"), (0, 1, 1, InvalidUtf8));
    // UTF-16, with or without its byte-order mark.
    assert_eq!(bytes_err(b"\xff\xfe[\x00]\x00"), (0, 1, 1, InvalidUtf8));
    // Without its byte-order mark, UTF-16 of ASCII text is valid UTF-8 (NUL
    // is a character), so it fails as JSON instead.
    assert_eq!(bytes_err(b"\x00[\x00]"), (0, 1, 1, found('\0', VALUE)));
}

// -------------------------------------------------------- error positions

#[test]
fn errors_report_the_line() {
    let text = "{\n  \"a\": 1,\n  \"b\": oops\n}";
    assert_eq!(err(text), (3, 8, found('o', VALUE)));
    // Windows line endings count once.
    assert_eq!(err("{\r\n  \"a\":\r\n  x\r\n}"), (3, 3, found('x', VALUE)));
    // Lines inside comments count.
    assert_eq!(
        err_jsonc("// one\n/* two\nthree */\n?"),
        (4, 1, found('?', VALUE))
    );
    assert_eq!(err("[\n\n\n"), (4, 1, end(ARRAY_ITEM)));
}

#[test]
fn columns_count_characters_not_bytes() {
    // `é` is 2 bytes, `€` is 3 and `😀` is 4; each is one column.
    assert_eq!(err("{\"é€😀\": x}"), (1, 9, found('x', VALUE)));
    assert_eq!(err("\t\tx"), (1, 3, found('x', VALUE)));
    assert_eq!(err_jsonc("/* é */ é"), (1, 9, found('é', VALUE)));
    assert_eq!(err("[1]\u{a0}"), (1, 4, found('\u{a0}', END)));
}

#[test]
fn unterminated_constructs_say_they_are_reported_where_they_start() {
    let message = |text: &str| {
        try_parse(text, Dialect::Jsonc, DEFAULT)
            .expect_err("unterminated")
            .to_string()
    };
    assert_eq!(
        message(r#"["abc"#),
        "JSON error at line 1, column 2 (byte 1): the text ends where `\"` to close the \
         string that starts here was expected"
    );
    assert_eq!(
        message("[1, /* never"),
        "JSON error at line 1, column 5 (byte 4): the text ends where `*/` to close the \
         comment that starts here was expected"
    );
    assert_eq!(
        message(r#"["\u12"#),
        "JSON error at line 1, column 3 (byte 2): the text ends where four hex digits for the \
         escape that starts here was expected"
    );
}

#[test]
fn errors_display_with_their_position() {
    let error = try_parse("[1 2]", Dialect::Json, DEFAULT).expect_err("missing comma");
    assert_eq!(
        error.to_string(),
        "JSON error at line 1, column 4 (byte 3): found '2' where `,` or `]` was expected"
    );
    let error = try_parse("{\n  \"a\": ", Dialect::Json, DEFAULT).expect_err("missing value");
    assert_eq!(
        error.to_string(),
        "JSON error at line 2, column 8 (byte 9): the text ends where a value was expected"
    );
}
