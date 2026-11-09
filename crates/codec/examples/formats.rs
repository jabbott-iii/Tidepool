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

//! Prints what `tidepool-codec` does with a few small documents: JSONC
//! written back as strict JSON, a TOML 1.1 document's values and their
//! types beside the text each came from, errors that say where the input
//! went wrong, and `Limits` refusing input nested deeper than they allow.
//!
//! Tidepool's demo page shows what it prints (ADR-0030). The inputs are
//! fixed, so the output is the same bytes on every run and every OS:
//!
//! ```text
//! cargo run -p tidepool-codec --example formats
//! ```

use std::error::Error;
use std::fmt;
use std::io::{self, Write};

use tidepool_codec::json::{self, Dialect};
use tidepool_codec::toml::{self, Datetime, TableKind, ValueKind};
use tidepool_codec::{CodecError, Limits};

/// Settings as a person writes them: comments, trailing commas, escapes.
const SETTINGS: &str = r#"{
  // Settings as a person writes them.
  "theme": "dark",
  "rulers": [80, 100,],
  "title": "caf\u00e9 \ud83c\udf0a", /* one emoji, two escapes */
  "id": 18446744073709551615,
}"#;

/// A manifest-like TOML 1.1 document.
const MANIFEST: &str = r#"title = "caf\xE9 \U0001F30A"
born = 1979-05-27 07:32-08:00
lints = { unsafe = "deny",
  unwrap = "deny", }
[package]
name = "tidepool"
version = '0.1.0'
edition = 2024
[[bin]]
name = "tp"
test = false
[[bin]]
name = "tp-bench"
nightly = 07:30
ratio = 1_500.5e-3
"#;

fn main() -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    jsonc(&mut out)?;
    writeln!(out)?;
    toml_values(&mut out)?;
    writeln!(out)?;
    errors(&mut out)?;
    writeln!(out)?;
    limits(&mut out)?;
    out.flush()?;
    Ok(())
}

/// JSONC in, compact strict JSON out, and the same text refused as JSON.
fn jsonc(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "== JSONC → JSON ==")?;
    for line in SETTINGS.lines() {
        writeln!(out, "jsonc  │ {line}")?;
    }
    let value = json::parse(SETTINGS, Dialect::Jsonc, Limits::DEFAULT)?;
    let compact = json::to_string_compact(&value);
    writeln!(out, "json   │ {compact}")?;
    let back = json::parse(&compact, Dialect::Json, Limits::DEFAULT)?;
    writeln!(
        out,
        "       │ read back as strict JSON: {}",
        if back == value {
            "the same value"
        } else {
            "a different value"
        }
    )?;
    if let Some(number) = value.get("id").and_then(json::Value::as_number)
        && number.as_u64().is_some()
    {
        writeln!(
            out,
            "       │ \"id\" stays an exact u64; an f64 would round it to {:.0}",
            number.as_f64()
        )?;
    }
    match json::parse(SETTINGS, Dialect::Json, Limits::DEFAULT) {
        Ok(_) => return Err("strict JSON accepted a comment".into()),
        Err(error) => writeln!(out, "strict │ {error}")?,
    }
    Ok(())
}

/// One value the TOML parser found: where its text starts, its dotted
/// path, its type and what it holds.
struct Row {
    at: usize,
    path: String,
    kind: &'static str,
    value: String,
}

/// Each line of a TOML document beside the values that start on it.
fn toml_values(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "== TOML 1.1: each line, and the values the parser found on it =="
    )?;
    let root = toml::parse(MANIFEST, Limits::DEFAULT)?;
    let mut rows = Vec::new();
    rows_of(&root, "", &mut rows);
    // Stable, so a table stays before the entries that start where it does.
    rows.sort_by_key(|row| row.at);

    let left = MANIFEST.lines().map(str::len).max().unwrap_or(0);
    let path = rows.iter().map(|row| row.path.len()).max().unwrap_or(0);
    let kind = rows.iter().map(|row| row.kind.len()).max().unwrap_or(0);
    let mut rows = rows.into_iter().peekable();
    let mut start = 0;
    for line in MANIFEST.split_inclusive('\n') {
        let end = start + line.len();
        let mut text = line.trim_end();
        let mut printed = false;
        while let Some(row) = rows.next_if(|row| row.at < end) {
            writeln!(
                out,
                "{text:<left$} │ {:<path$}  {:<kind$}  {}",
                row.path, row.kind, row.value
            )?;
            text = "";
            printed = true;
        }
        if !printed {
            writeln!(out, "{text:<left$} │")?;
        }
        start = end;
    }
    writeln!(
        out,
        "TOML 1.1 here: \\xE9, times without seconds, an inline table on two lines, a trailing comma"
    )?;
    Ok(())
}

/// The rows for the entries of `table`, whose path is `prefix`.
fn rows_of(table: &toml::Table, prefix: &str, rows: &mut Vec<Row>) {
    for (key, value) in table.entries() {
        let path = match prefix {
            "" => key.name.clone(),
            _ => format!("{prefix}.{}", key.name),
        };
        rows_of_value(value, path, rows);
    }
}

/// The rows for `value` at `path`, and for what it holds.
fn rows_of_value(value: &toml::Value, path: String, rows: &mut Vec<Row>) {
    let at = value.span.start;
    let (kind, shown) = match &value.kind {
        ValueKind::String(text) => ("string", format!("{text:?}")),
        ValueKind::Integer(number) => ("integer", number.to_string()),
        ValueKind::Float(number) => ("float", number.to_string()),
        ValueKind::Boolean(flag) => ("boolean", flag.to_string()),
        ValueKind::Datetime(datetime) => (datetime_kind(datetime), datetime.to_string()),
        ValueKind::Array(array) => {
            let (kind, items) = if array.is_array_of_tables() {
                ("array of tables", "tables")
            } else {
                ("array", "items")
            };
            rows.push(Row {
                at,
                path: path.clone(),
                kind,
                value: format!("{} {items}", array.len()),
            });
            for (index, item) in array.items().iter().enumerate() {
                rows_of_value(item, format!("{path}[{index}]"), rows);
            }
            return;
        }
        ValueKind::Table(table) => {
            rows.push(Row {
                at,
                path: path.clone(),
                kind: table_kind(table.kind()),
                value: format!("{} keys", table.len()),
            });
            rows_of(table, &path, rows);
            return;
        }
    };
    rows.push(Row {
        at,
        path,
        kind,
        value: shown,
    });
}

/// The name of a date-time's kind.
fn datetime_kind(datetime: &Datetime) -> &'static str {
    match datetime {
        Datetime::OffsetDatetime { .. } => "offset date-time",
        Datetime::LocalDatetime { .. } => "local date-time",
        Datetime::LocalDate(_) => "local date",
        Datetime::LocalTime(_) => "local time",
    }
}

/// How a table was defined, as a type name.
fn table_kind(kind: TableKind) -> &'static str {
    match kind {
        TableKind::Root => "root table",
        TableKind::Header | TableKind::ArrayHeader => "table",
        TableKind::Implicit => "implicit table",
        TableKind::Dotted => "dotted-key table",
        TableKind::Inline => "inline table",
    }
}

/// Malformed input, and the error each parser gives for it.
fn errors(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "== Errors say where, and what was wrong ==")?;
    let text = r#"{"tab_size": 04}"#;
    let error = json::parse(text, Dialect::Json, Limits::DEFAULT).err();
    show_error(out, "json", text, error)?;
    let text = r#"path = "C:\Program Files""#;
    show_error(out, "toml", text, toml::parse(text, Limits::DEFAULT).err())?;
    let text = "name = \"tidepool\"\nname = \"tidewater\"";
    show_error(out, "toml", text, toml::parse(text, Limits::DEFAULT).err())
}

/// Prints `text` under `label`, a caret at the error's line and column,
/// and the error.
fn show_error(
    out: &mut dyn Write,
    label: &str,
    text: &str,
    error: Option<CodecError>,
) -> Result<(), Box<dyn Error>> {
    let error = error.ok_or("malformed input was accepted")?;
    let (line, column) = match error {
        CodecError::Json { line, column, .. } | CodecError::Toml { line, column, .. } => {
            (line, column)
        }
        _ => (0, 0),
    };
    let blank = " ".repeat(label.len());
    for (index, text) in text.lines().enumerate() {
        let label = if index == 0 { label } else { &blank };
        writeln!(out, "{label} │ {text}")?;
        if index + 1 == line {
            writeln!(out, "{blank} │ {:>column$}", "^")?;
        }
    }
    writeln!(out, "{blank} └ {error}")?;
    Ok(())
}

/// `Limits` bounding nesting: small limits, then the default against a
/// hostile document.
fn limits(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "== Limits: nesting is bounded, and deep input cannot overflow the stack =="
    )?;
    let small = Limits {
        max_depth: 4,
        ..Limits::DEFAULT
    };
    writeln!(out, "JSON, Limits {{ max_depth: 4, ..Limits::DEFAULT }}")?;
    for text in ["[[[[1]]]]", "[[[[[1]]]]]"] {
        show_limit(out, text, json::parse(text, Dialect::Json, small).map(drop))?;
    }
    writeln!(out, "TOML, Limits {{ max_depth: 4, ..Limits::DEFAULT }}")?;
    for text in ["a.b.c.d.e = 1", "a.b.c.d.e.f = 1"] {
        show_limit(out, text, toml::parse(text, small).map(drop))?;
    }
    writeln!(
        out,
        "JSON, Limits::DEFAULT (max_depth: {})",
        Limits::DEFAULT.max_depth
    )?;
    let hostile = "[".repeat(100_000);
    let result = json::parse(&hostile, Dialect::Json, Limits::DEFAULT).map(drop);
    show_limit(out, "\"[\" × 100,000", result)
}

/// Prints one input checked against a limit, and whether it was accepted.
fn show_limit(
    out: &mut dyn Write,
    text: &str,
    result: Result<(), CodecError>,
) -> Result<(), Box<dyn Error>> {
    let verdict = match result {
        Ok(()) => "accepted".to_string(),
        Err(CodecError::Json {
            line, column, kind, ..
        }) => refusal(line, column, &kind),
        Err(CodecError::Toml {
            line, column, kind, ..
        }) => refusal(line, column, &kind),
        Err(error) => format!("refused: {error}"),
    };
    writeln!(out, "  {text:<15}  {verdict}")?;
    Ok(())
}

/// Where an input was refused, and why.
fn refusal(line: usize, column: usize, kind: &dyn fmt::Display) -> String {
    match line {
        1 => format!("refused at column {column}: {kind}"),
        _ => format!("refused at line {line}, column {column}: {kind}"),
    }
}
