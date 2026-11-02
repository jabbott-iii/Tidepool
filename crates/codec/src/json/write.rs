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

//! The JSON writers. Both produce strict RFC 8259 JSON: no comments and no
//! trailing commas, so any JSON parser can read the result.
//!
//! Ported from nadir-codec `json/write.rs`. Adapted for Tidepool: numbers
//! are a [`Number`](super::Number), which is never NaN or infinite, so
//! nothing has to be written as `null` and the writers cannot fail; float
//! formatting moved to `Number`'s `Display`.

use std::fmt::Write as _;

use super::Value;

/// Spaces per nesting level in [`to_string_pretty`].
const INDENT: usize = 2;

/// Writes `value` as JSON on one line with no optional whitespace.
///
/// Object members are written in the order they are stored. The result
/// reads back as an equal value if it fits the limits it is read with (see
/// the [module documentation](super#writing)).
pub fn to_string_compact(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None);
    out
}

/// Writes `value` as JSON for people to read: one array element or object
/// member per line, indented by two spaces per level, with `": "` after
/// each key. Empty arrays and objects stay on one line (`[]`, `{}`). There
/// is no trailing newline.
///
/// Object members are written in the order they are stored. The result
/// reads back as an equal value if it fits the limits it is read with (see
/// the [module documentation](super#writing)).
///
/// # Output size
///
/// The indentation makes the output larger than [`to_string_compact`]'s by
/// a factor that grows with nesting. For a value nested `d` deep (as
/// [`Limits::max_depth`] counts), it is at most `2d + 2` times as large:
/// each byte of compact output gains at most a line break and `2d` spaces.
/// Values shaped for it get close. Many short chains of one-entry arrays
/// near the deepest level (`[[[0]]],` repeated) reach about 236 times at
/// the default depth limit of 128, so a 64 MiB document can pretty-print to
/// about 15 GiB. Before pretty-printing a value that came from outside,
/// bound its depth or its compact size.
///
/// [`Limits::max_depth`]: crate::Limits::max_depth
pub fn to_string_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(0));
    out
}

/// Writes `value`. `level` is `None` for compact output, or the current
/// nesting level for pretty output.
fn write_value(out: &mut String, value: &Value, level: Option<usize>) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        // Writing to a `String` cannot fail.
        Value::Number(number) => _ = write!(out, "{number}"),
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            write_sequence(out, ['[', ']'], items, level, |out, item, inner| {
                write_value(out, item, inner);
            });
        }
        Value::Object(members) => {
            write_sequence(
                out,
                ['{', '}'],
                members,
                level,
                |out, (key, value), inner| {
                    write_string(out, key);
                    out.push_str(if inner.is_some() { ": " } else { ":" });
                    write_value(out, value, inner);
                },
            );
        }
    }
}

/// Writes the entries of an array or object between `brackets`, calling
/// `write_entry` for each with the nesting level inside the container.
fn write_sequence<T>(
    out: &mut String,
    [open, close]: [char; 2],
    entries: &[T],
    level: Option<usize>,
    write_entry: impl Fn(&mut String, &T, Option<usize>),
) {
    out.push(open);
    let inner = level.map(|level| level.saturating_add(1));
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        new_line(out, inner);
        write_entry(out, entry, inner);
    }
    // An empty container stays on one line: `[]` or `{}`.
    if !entries.is_empty() {
        new_line(out, level);
    }
    out.push(close);
}

/// In pretty output, starts a new line indented to `level`. In compact
/// output, does nothing.
fn new_line(out: &mut String, level: Option<usize>) {
    if let Some(level) = level {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', level.saturating_mul(INDENT)));
    }
}

/// Writes `text` as a JSON string, escaping exactly what RFC 8259 requires:
/// `"`, `\` and the control characters U+0000 to U+001F, with the short
/// escapes where there is one.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            // The remaining control characters have no short form. Writing
            // to a `String` cannot fail.
            control if control < ' ' => _ = write!(out, "\\u{:04x}", u32::from(control)),
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Number;

    fn string(text: &str) -> Value {
        Value::from(text)
    }

    fn int(value: i64) -> Value {
        Value::from(value)
    }

    fn float(value: f64) -> Value {
        Value::Number(Number::from_f64(value).expect("finite"))
    }

    fn object(members: &[(&str, Value)]) -> Value {
        Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn scalars() {
        assert_eq!(to_string_compact(&Value::Null), "null");
        assert_eq!(to_string_compact(&Value::Bool(true)), "true");
        assert_eq!(to_string_compact(&Value::Bool(false)), "false");
        assert_eq!(to_string_compact(&int(0)), "0");
        assert_eq!(to_string_compact(&int(-5)), "-5");
        assert_eq!(to_string_compact(&int(i64::MAX)), "9223372036854775807");
        assert_eq!(to_string_compact(&int(i64::MIN)), "-9223372036854775808");
        assert_eq!(
            to_string_compact(&Value::from(u64::MAX)),
            "18446744073709551615"
        );
        assert_eq!(to_string_compact(&float(1.0)), "1.0");
        assert_eq!(to_string_compact(&float(-1e300)), "-1e300");
        assert_eq!(to_string_compact(&string("hi")), "\"hi\"");
    }

    #[test]
    fn strings_are_escaped_only_where_json_requires_it() {
        let text = "a\"b\\c/d\n\r\t\u{8}\u{c}\0\u{1}\u{1f} \u{7f}é€😀\u{2028}";
        let expected = "\"a\\\"b\\\\c/d\\n\\r\\t\\b\\f\\u0000\\u0001\\u001f \u{7f}é€😀\u{2028}\"";
        assert_eq!(to_string_compact(&string(text)), expected);
        assert_eq!(to_string_compact(&string("")), "\"\"");
    }

    #[test]
    fn every_control_character_is_escaped() {
        for code in 0..0x20u32 {
            let character = char::from_u32(code).expect("a control character");
            let written = to_string_compact(&string(&character.to_string()));
            assert!(
                written.chars().all(|c| c >= ' '),
                "U+{code:04X} written raw: {written:?}"
            );
        }
    }

    #[test]
    fn compact_output_has_no_whitespace() {
        let value = object(&[
            ("a", int(1)),
            (
                "b",
                Value::Array(vec![int(2), Value::Array(vec![]), object(&[])]),
            ),
            ("c d", object(&[("e", Value::Null)])),
        ]);
        assert_eq!(
            to_string_compact(&value),
            r#"{"a":1,"b":[2,[],{}],"c d":{"e":null}}"#
        );
        assert_eq!(to_string_compact(&Value::Array(vec![])), "[]");
        assert_eq!(to_string_compact(&object(&[])), "{}");
    }

    #[test]
    fn pretty_output_puts_each_entry_on_its_own_line() {
        let value = object(&[
            ("name", string("tidepool-codec")),
            ("version", int(1)),
            ("targets", Value::Array(vec![string("lib"), Value::Null])),
            (
                "stats",
                object(&[("ratio", float(1.5)), ("tags", Value::Array(vec![]))]),
            ),
            ("empty", object(&[])),
            (
                "grid",
                Value::Array(vec![Value::Array(vec![int(1), int(2)])]),
            ),
        ]);
        let expected = r#"{
  "name": "tidepool-codec",
  "version": 1,
  "targets": [
    "lib",
    null
  ],
  "stats": {
    "ratio": 1.5,
    "tags": []
  },
  "empty": {},
  "grid": [
    [
      1,
      2
    ]
  ]
}"#;
        assert_eq!(to_string_pretty(&value), expected);
    }

    #[test]
    fn pretty_scalars_and_empty_containers_match_compact() {
        for value in [
            Value::Null,
            int(7),
            float(0.5),
            string("x"),
            Value::Array(vec![]),
            object(&[]),
        ] {
            assert_eq!(to_string_pretty(&value), to_string_compact(&value));
        }
    }

    #[test]
    fn keys_are_written_in_stored_order_and_escaped() {
        let value = object(&[("z", int(1)), ("a\"\n", int(2)), ("", int(3))]);
        assert_eq!(to_string_compact(&value), r#"{"z":1,"a\"\n":2,"":3}"#);
        assert_eq!(
            to_string_pretty(&value),
            "{\n  \"z\": 1,\n  \"a\\\"\\n\": 2,\n  \"\": 3\n}"
        );
    }
}
