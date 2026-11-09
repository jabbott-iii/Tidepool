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

//! Prints what `tidepool-text` does with a few small texts: lines and
//! positions counted in UTF-8 bytes, UTF-16 units and chars; grapheme
//! clusters; a multi-cursor edit through the undo tree, undone, branched
//! and jumped back to; and UTF-16 decoded, saved atomically and loaded
//! back.
//!
//! Tidepool's demo page shows what it prints (ADR-0030). The inputs are
//! fixed, so the output is the same bytes on every run and every OS. The
//! save goes to a new directory in the system's temporary directory, which
//! is removed afterwards; only the file's name is printed.
//!
//! ```text
//! cargo run -p tidepool-text --example editing
//! ```

use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use tidepool_text::{
    Cursor, Format, Group, History, LineBreak, LineCol, LineEndings, LineSpan, Place, Rope,
    Selection, Step, TextError, Transaction, Unit, encoding, file,
};

/// A text with each kind of line break: CR LF, LF and a lone CR.
const LINES: &str = "Tidepool\r\nnaïve 😀 café\nend\rlast";

/// An accent written as a combining mark, a ZWJ sequence and a flag.
const CLUSTERS: &str = "e\u{301}👩\u{200D}💻🇯🇵!";

/// The name of the file the example saves.
const FILE_NAME: &str = "notes.txt";

fn main() -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    positions(&mut out)?;
    writeln!(out)?;
    graphemes(&mut out)?;
    writeln!(out)?;
    edits(&mut out)?;
    writeln!(out)?;
    files(&mut out)?;
    out.flush()?;
    Ok(())
}

/// Lines and their breaks, and one position in each unit, and back.
fn positions(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "== Rope: lines and positions ==")?;
    let text = Rope::from(LINES);
    writeln!(out, "text  {}", quoted(LINES))?;
    for line in 0..text.len_lines() {
        let span = text.line_span(line)?;
        let content = text.slice(span.content())?.to_string();
        let ending = match span.line_break {
            Some(LineBreak::CrLf) => "CR LF",
            Some(LineBreak::Lf) => "LF",
            Some(LineBreak::Cr) => "CR",
            None => "none",
        };
        writeln!(
            out,
            "line {line}  bytes {:>2}..{:<2}  break {ending:<5}  {}",
            span.start,
            span.content_end,
            quoted(&content)
        )?;
    }
    let endings = LineEndings::of(&text);
    writeln!(
        out,
        "{} lines: {} LF, {} CR LF and {} CR; mixed: {}",
        text.len_lines(),
        endings.lf,
        endings.crlf,
        endings.cr,
        yes_no(endings.is_mixed())
    )?;

    let byte = LINES.find("café").ok_or("the sample text has \"café\"")?;
    writeln!(
        out,
        "\"café\" starts at byte {byte}; its line and column, and back:"
    )?;
    for (unit, name) in [
        (Unit::Byte, "UTF-8 bytes"),
        (Unit::Utf16, "UTF-16 units"),
        (Unit::Char, "chars"),
    ] {
        let pos = text.byte_to_line_col(byte, unit)?;
        let back = text.line_col_to_byte(pos, unit)?;
        let at = format!("line {}, column {}", pos.line, pos.col);
        writeln!(out, "  in {name:<12}  {at:<17}  →  byte {back}")?;
    }

    writeln!(
        out,
        "a position inside a character is an error, never a panic:"
    )?;
    let emoji = LINES.find('😀').ok_or("the sample text has an emoji")?;
    let start = text.byte_to_line_col(emoji, Unit::Utf16)?;
    let inside = LineCol::new(start.line, start.col + 1);
    let result = text.line_col_to_byte(inside, Unit::Utf16);
    writeln!(
        out,
        "  between the halves of 😀 in UTF-16: {}",
        outcome(result)
    )?;
    let accent = LINES.find('ï').ok_or("the sample text has an ï")? + 1;
    let result = text.byte_to_line(accent);
    writeln!(
        out,
        "  in the middle of ï in UTF-8:        {}",
        outcome(result)
    )?;
    Ok(())
}

/// The extended grapheme clusters of a text, and where the cursor stops.
fn graphemes(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "== Grapheme clusters (UAX #29, Unicode 17.0.0) ==")?;
    let text = Rope::from(CLUSTERS);
    let spans: Vec<_> = text.graphemes_in(0..text.len())?.collect();
    writeln!(
        out,
        "text  {CLUSTERS}  {} bytes, {} UTF-16 units, {} chars, {} clusters",
        text.len(),
        text.len_in(Unit::Utf16),
        text.len_in(Unit::Char),
        spans.len()
    )?;
    writeln!(out, "  bytes   chars  code points             cluster")?;
    for span in spans {
        let cluster = text.slice(span.clone())?.to_string();
        let points: Vec<String> = cluster
            .chars()
            .map(|ch| format!("U+{:04X}", u32::from(ch)))
            .collect();
        writeln!(
            out,
            "  {:<7} {:<6} {:<23} {cluster}",
            format!("{}..{}", span.start, span.end),
            cluster.chars().count(),
            points.join(" ")
        )?;
    }
    let mut stops = vec![0];
    while let Some(next) = stops.last().and_then(|&at| text.next_grapheme_boundary(at)) {
        stops.push(next);
    }
    let shown: Vec<String> = stops.iter().map(usize::to_string).collect();
    writeln!(
        out,
        "the cursor moves by cluster: bytes {} ({} of the {} char boundaries)",
        shown.join(" → "),
        stops.len(),
        text.len_in(Unit::Char) + 1
    )?;
    Ok(())
}

/// One transaction at several cursors, then undo, a branch, and a jump
/// back, each through the buffer's `History`.
fn edits(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "== Edits through the undo tree: | is a caret, [ ] a selection =="
    )?;
    let mut text = Rope::from("one\ntwo\nthree\n");
    let mut history = History::new(&text);

    let carets = per_line(&text, |line| Cursor::caret(line.start))?;
    row(out, "start", &text, &carets, &history)?;
    let typed = Transaction::change_each(&text, &carets, |cursor| {
        Some((cursor.head..cursor.head, "- ", Place::End))
    })?;
    let step = history.commit(&mut text, typed, &carets, Instant::now(), Group::New)?;
    row(
        out,
        "type \"- \" at 3 carets",
        &text,
        &step.selection,
        &history,
    )?;

    let words = per_line(&text, |line| Cursor::new(line.start + 2, line.content_end))?;
    row(out, "select the words", &text, &words, &history)?;
    let step = change_words(&mut history, &mut text, &words, str::to_uppercase)?;
    let upper = step.revision;
    row(out, "uppercase them", &text, &step.selection, &history)?;

    let step = history.undo(&mut text)?.ok_or("nothing to undo")?;
    row(out, "undo", &text, &step.selection, &history)?;
    let step = change_words(&mut history, &mut text, &step.selection, capitalized)?;
    row(
        out,
        "capitalize: a new branch",
        &text,
        &step.selection,
        &history,
    )?;

    let step = history
        .jump_to(&mut text, upper)?
        .ok_or("already at the revision")?;
    let label = format!("jump to revision {}", upper.seq());
    row(out, &label, &text, &step.selection, &history)?;
    history.undo(&mut text)?;
    let step = history.undo(&mut text)?.ok_or("nothing to undo")?;
    row(out, "undo, undo", &text, &step.selection, &history)?;
    let step = history.redo(&mut text)?.ok_or("nothing to redo")?;
    row(out, "redo", &text, &step.selection, &history)?;
    writeln!(
        out,
        "{} revisions, the root included: no edit was lost",
        history.revision_count()
    )?;
    Ok(())
}

/// Commits `change` of the text of each cursor of `words`, leaving the new
/// text selected.
fn change_words(
    history: &mut History,
    text: &mut Rope,
    words: &Selection,
    change: fn(&str) -> String,
) -> Result<Step, TextError> {
    let edit = Transaction::change_each(text, words, |cursor| {
        let word = text.slice(cursor.range()).ok()?.to_string();
        Some((cursor.range(), change(&word), Place::Select))
    })?;
    history.commit(text, edit, words, Instant::now(), Group::New)
}

/// `word` with its first letter in upper case.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One cursor for each line of `text` that is not empty.
fn per_line(text: &Rope, cursor: impl Fn(LineSpan) -> Cursor) -> Result<Selection, TextError> {
    let mut cursors = Vec::new();
    for line in 0..text.len_lines() {
        let span = text.line_span(line)?;
        if span.content_end > span.start {
            cursors.push(cursor(span));
        }
    }
    let mut cursors = cursors.into_iter();
    let primary = cursors.next().unwrap_or(Cursor::caret(0));
    Ok(Selection::new(primary, cursors))
}

/// Prints one step of the edit: what was done, the revision and whether it
/// is the saved one, and the text with its cursors marked.
fn row(
    out: &mut dyn Write,
    label: &str,
    text: &Rope,
    selection: &Selection,
    history: &History,
) -> io::Result<()> {
    let state = if history.is_modified() {
        "modified"
    } else {
        "saved"
    };
    writeln!(
        out,
        "{label:<25} revision {}  {state:<8}  {}",
        history.current().seq(),
        marked(&text.to_string(), selection)
    )
}

/// `text` with `|` at each caret and `[` `]` around each selection.
fn marked(text: &str, selection: &Selection) -> String {
    let marks = |at: usize| -> String {
        let cursors = selection.cursors();
        let ends = cursors.iter().filter(|c| !c.is_caret() && c.end() == at);
        let carets = cursors.iter().filter(|c| c.is_caret() && c.head == at);
        let starts = cursors.iter().filter(|c| !c.is_caret() && c.start() == at);
        let ends = ends.map(|_| ']');
        ends.chain(carets.map(|_| '|'))
            .chain(starts.map(|_| '['))
            .collect()
    };
    let mut shown = String::new();
    for (at, ch) in text.char_indices() {
        shown.push_str(&marks(at));
        escape_into(&mut shown, ch);
    }
    shown.push_str(&marks(text.len()));
    shown
}

/// UTF-16 decoded, saved unchanged and edited, and loaded back; then bytes
/// that are not text.
fn files(out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "== Decoding, and the atomic save ==")?;
    let sample = "hé 🌊\r\n";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in sample.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    let decoded = encoding::decode(&bytes);
    writeln!(out, "bytes    {}  ({} bytes)", hex(&bytes), bytes.len())?;
    writeln!(
        out,
        "decoded  {}: {}, read-only: {}",
        quoted(&decoded.text().to_string()),
        format_name(decoded.detected()),
        yes_no(decoded.read_only())
    )?;
    let format = decoded
        .format()
        .ok_or("the sample has bytes that are not text")?;

    let dir = env::temp_dir().join(format!("tidepool-editing-{}", process::id()));
    fs::create_dir(&dir)?;
    let saved = save_and_load(out, &dir.join(FILE_NAME), decoded.text(), format, &bytes);
    let removed = fs::remove_dir_all(&dir);
    saved?;
    removed?;

    let broken = b"ok \xFF\n";
    let decoded = encoding::decode(broken);
    writeln!(out, "bytes    {}  (FF is not UTF-8)", hex(broken))?;
    writeln!(
        out,
        "decoded  {}: read-only: {}, a format to save in: {}",
        quoted(&decoded.text().to_string()),
        yes_no(decoded.read_only()),
        yes_no(decoded.format().is_some())
    )?;
    writeln!(
        out,
        "kept     {}  (the bytes as read, kept beside the text)",
        hex(&decoded.original_bytes())
    )?;
    Ok(())
}

/// Saves `text` unchanged to `path` and compares the file with `read`;
/// then edits it through a `History`, saves it from a checkpoint, and
/// loads it back.
fn save_and_load(
    out: &mut dyn Write,
    path: &Path,
    text: &Rope,
    format: Format,
    read: &[u8],
) -> Result<(), Box<dyn Error>> {
    file::save(path, text, format)?;
    let written = fs::read(path)?;
    writeln!(
        out,
        "saved    {FILE_NAME} unchanged: {} bytes, {}",
        written.len(),
        if written == read {
            "the same bytes as read"
        } else {
            "different bytes from those read"
        }
    )?;

    let mut text = text.clone();
    let mut history = History::new(&text);
    let end = text.line_span(0)?.content_end;
    let caret = Selection::caret(end);
    let edit = Transaction::change_each(&text, &caret, |cursor| {
        Some((cursor.head..cursor.head, " ok", Place::End))
    })?;
    history.commit(&mut text, edit, &caret, Instant::now(), Group::New)?;
    let checkpoint = history.checkpoint();
    file::save(path, checkpoint.text(), format)?;
    history.mark_saved(&checkpoint)?;

    let loaded = file::load(path)?;
    let endings = loaded.line_endings();
    writeln!(
        out,
        "edited   {}, saved: {FILE_NAME} is {} bytes; modified: {}",
        quoted(&text.to_string()),
        loaded.file_len(),
        yes_no(history.is_modified())
    )?;
    writeln!(
        out,
        "loaded   {FILE_NAME}: {}, {} CR LF, equal to the edited text: {}",
        format_name(loaded.detected()),
        endings.crlf,
        yes_no(*loaded.text() == text)
    )?;
    Ok(())
}

/// How a format is shown: `UTF-16 LE with BOM`.
fn format_name(format: Format) -> String {
    let bom = if format.bom { "with" } else { "without" };
    format!("{} {bom} BOM", format.encoding.name())
}

/// The bytes in hex, separated by spaces.
fn hex(bytes: &[u8]) -> String {
    let pairs: Vec<String> = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    pairs.join(" ")
}

/// The byte a conversion gave, or its error.
fn outcome(result: Result<usize, TextError>) -> String {
    match result {
        Ok(byte) => format!("byte {byte}"),
        Err(error) => format!("error: {error}"),
    }
}

/// `text` in quotes, with its line breaks and other control characters
/// escaped.
fn quoted(text: &str) -> String {
    let mut shown = String::from('"');
    for ch in text.chars() {
        escape_into(&mut shown, ch);
    }
    shown.push('"');
    shown
}

/// Pushes `ch` onto `shown`, escaped if it is a quote, a backslash or a
/// control character.
fn escape_into(shown: &mut String, ch: char) {
    match ch {
        '\n' => shown.push_str("\\n"),
        '\r' => shown.push_str("\\r"),
        '\t' => shown.push_str("\\t"),
        '"' | '\\' => {
            shown.push('\\');
            shown.push(ch);
        }
        _ if ch.is_control() => shown.push_str(&format!("\\u{{{:x}}}", u32::from(ch))),
        _ => shown.push(ch),
    }
}

/// `yes` or `no`.
fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
