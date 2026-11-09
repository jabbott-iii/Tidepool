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

//! Where an edit goes: the one splice of the text that it makes, worked out
//! from the tree's spans, and the text to put there.
//!
//! Every place is found going forward from a span: the end of a line after
//! a value or header ([`line_end`]), the end of a table's last leaf or of
//! its subtree. The one scan back, over the comment lines above the first
//! header ([`root_point`]), runs only when the root has no keys, so that
//! only blank and comment lines can be there. The rules are in the
//! [module documentation](super::super#adding-a-value).

use std::ops::Range;

use super::super::write::{Out, write_path, write_value};
use super::super::{EditErrorKind, Key, Span, Table, TableKind, Value, ValueKind};
use super::{Missing, Step};
use crate::Limits;

/// One change to a text: the bytes in `range` (empty for an insertion)
/// become `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Splice {
    /// The bytes replaced.
    pub(super) range: Range<usize>,
    /// What replaces them.
    pub(super) text: String,
}

/// The error for a text that is not laid out the way its tree says, which
/// would be a bug here or in the parser: the edit is refused.
fn layout() -> EditErrorKind {
    EditErrorKind::Unverified {
        reason: "the text around the edit is not laid out as its tree says",
    }
}

/// c0: where the content starts, after a leading byte-order mark if there
/// is one. Nothing is ever written before it.
pub(super) fn content_start(text: &str) -> usize {
    if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }
}

/// nl: the line break new lines end with, which is the text's first: CR LF
/// if that is one, otherwise LF (also for a text with no line break).
pub(super) fn newline(text: &str) -> &'static str {
    match text.find('\n').and_then(|at| at.checked_sub(1)) {
        Some(before) if text.as_bytes().get(before) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

/// le(at): the end of the line that `at`, the end of a value or header, is
/// on. Skips spaces and tabs, then a comment, then the line break, and
/// returns the offset after it, or the end of the text. `None` if anything
/// else comes first, which the grammar does not allow after a value or a
/// header.
pub(super) fn line_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if at > bytes.len() {
        return None;
    }
    let mut pos = at;
    while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
        pos += 1;
    }
    if bytes.get(pos) == Some(&b'#') {
        while !matches!(bytes.get(pos), None | Some(b'\n')) {
            pos += 1;
        }
    }
    match bytes.get(pos) {
        None => Some(pos),
        Some(b'\n') => Some(pos + 1),
        Some(b'\r') if bytes.get(pos + 1) == Some(&b'\n') => Some(pos + 2),
        Some(_) => None,
    }
}

/// The start of the line `at` is on, never before `c0`.
fn line_start(text: &str, c0: usize, at: usize) -> Option<usize> {
    let before = text.get(..at)?;
    Some(before.rfind('\n').map_or(c0, |newline| newline + 1).max(c0))
}

/// The line that starts at `start`, without its line break.
fn line_at(text: &str, start: usize) -> Option<&str> {
    let line = text.get(start..)?.split('\n').next().unwrap_or_default();
    Some(line.strip_suffix('\r').unwrap_or(line))
}

/// Whether a line, without its line break, holds only spaces and tabs.
fn is_blank(line: &str) -> bool {
    line.bytes().all(|byte| byte == b' ' || byte == b'\t')
}

/// Whether a line, without its line break, is a comment, perhaps indented.
fn is_comment(line: &str) -> bool {
    line.trim_start_matches([' ', '\t']).starts_with('#')
}

/// The leading spaces and tabs of the line `key` is written on.
fn indent<'t>(text: &'t str, c0: usize, key: &Key) -> Option<&'t str> {
    let start = line_start(text, c0, key.span.start)?;
    let line = text.get(start..)?;
    let len = line
        .bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .count();
    line.get(..len)
}

/// The leaf of `table` whose text ends last, and its key. A table's leaves
/// are its entries whose value is a scalar, an array written as a value or
/// an inline table, and the leaves of its tables made by dotted keys, and
/// theirs: never those of a header's table, an array of tables or a table
/// made by a header's prefix, which are elsewhere in the text.
fn last_leaf(table: &Table) -> Option<(&Key, &Value)> {
    let mut last: Option<(&Key, &Value)> = None;
    let mut work = vec![table];
    while let Some(table) = work.pop() {
        for (key, value) in table.entries() {
            let leaf = match &value.kind {
                ValueKind::Table(inner) => match inner.kind() {
                    TableKind::Dotted => {
                        work.push(inner);
                        false
                    }
                    TableKind::Inline => true,
                    _ => false,
                },
                ValueKind::Array(array) => !array.is_array_of_tables(),
                _ => true,
            };
            if leaf && last.is_none_or(|(_, best)| value.span.end > best.span.end) {
                last = Some((key, value));
            }
        }
    }
    last
}

/// Where the first header of the document starts: the smallest span start
/// of a header's table or an element of an array of tables. `None` if
/// there is no header.
fn first_header(root: &Table) -> Option<usize> {
    let mut first: Option<usize> = None;
    let mut earlier = |start: usize| {
        first = Some(first.map_or(start, |first| first.min(start)));
    };
    let mut work = vec![root];
    while let Some(table) = work.pop() {
        for (_, value) in table.entries() {
            match &value.kind {
                ValueKind::Table(inner) if inner.kind() != TableKind::Inline => {
                    if inner.kind() == TableKind::Header {
                        earlier(value.span.start);
                    }
                    work.push(inner);
                }
                ValueKind::Array(array) if array.is_array_of_tables() => {
                    for item in array.items() {
                        if let ValueKind::Table(element) = &item.kind {
                            earlier(item.span.start);
                            work.push(element);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    first
}

/// The root point, where a key goes into a root without leaves: the start
/// of the first header's line, moved up over the comment lines right above
/// it (a blank line stops it), so that they stay with the header; or the
/// end of the text if there is no header. With no key at the root, only
/// blank and comment lines can come before the first header, so the scan
/// back meets nothing else.
pub(super) fn root_point(text: &str, c0: usize, root: &Table) -> Option<usize> {
    let Some(first) = first_header(root) else {
        return Some(text.len());
    };
    let mut point = line_start(text, c0, first)?;
    while point > c0 {
        // The line break that ends the line above.
        let above = point.checked_sub(1)?;
        let start = line_start(text, c0, above)?;
        let line = text.get(start..above)?;
        if !is_comment(line) {
            break;
        }
        point = start;
    }
    Some(point)
}

/// The end of `table`'s subtree: the end of the line of the last thing in
/// it or under it, which is the latest end of a leaf of any table that is
/// not inline, and of a header, its own (`own`) included. The walk goes
/// into tables of every kind but inline ones, and into arrays of tables,
/// but not into arrays written as values. The end of the text if there is
/// nothing.
fn subtree_end(text: &str, table: &Table, own: Option<Span>) -> Option<usize> {
    let mut last = match table.kind() {
        TableKind::Header | TableKind::ArrayHeader | TableKind::Implicit => own.map(|own| own.end),
        _ => None,
    };
    let mut later = |end: usize| {
        last = Some(last.map_or(end, |last| last.max(end)));
    };
    let mut work = vec![table];
    while let Some(table) = work.pop() {
        for (_, value) in table.entries() {
            match &value.kind {
                ValueKind::Table(inner) if inner.kind() == TableKind::Inline => {
                    later(value.span.end);
                }
                ValueKind::Table(inner) => {
                    if inner.kind() == TableKind::Header {
                        later(value.span.end);
                    }
                    work.push(inner);
                }
                ValueKind::Array(array) if array.is_array_of_tables() => {
                    for item in array.items() {
                        if let ValueKind::Table(element) = &item.kind {
                            later(item.span.end);
                            work.push(element);
                        }
                    }
                }
                _ => later(value.span.end),
            }
        }
    }
    match last {
        Some(end) => line_end(text, end),
        None => Some(text.len()),
    }
}

/// A line an insertion adds.
enum Line<'a> {
    /// A header, `[a.b]`.
    Header(Vec<&'a str>),
    /// A key/value pair, `a.b = value`, after `indent`.
    Pair {
        indent: &'a str,
        names: Vec<&'a str>,
        value: &'a ValueKind,
    },
}

fn write_line(out: &mut Out, line: &Line<'_>) -> Result<(), EditErrorKind> {
    match line {
        Line::Header(names) => {
            out.push("[")?;
            write_path(out, names.iter().copied())?;
            out.push("]")
        }
        Line::Pair {
            indent,
            names,
            value,
        } => {
            out.push(indent)?;
            write_path(out, names.iter().copied())?;
            out.push(" = ")?;
            write_value(out, value)
        }
    }
}

/// Writes to `out` what inserting `lines` at `at` takes: `at` is the start
/// of a line or the end of the text. With `blank_before`, the lines go
/// after a blank line (one is added unless there is one, or they start the
/// content); with `blank_after`, a blank line follows them unless one is
/// there or they end the text.
///
/// - Into empty content (`""` or a byte-order mark alone), each line is
///   followed by a line break.
/// - At the end of a text that does not end in a line break, a line break
///   comes first (two, for `blank_before`, unless the last line is blank),
///   then the lines with line breaks between them but not after: the text
///   still does not end in one.
/// - Anywhere else, each line is followed by a line break.
fn insert_lines(
    text: &str,
    at: usize,
    lines: &[Line<'_>],
    blank_before: bool,
    blank_after: bool,
    out: &mut Out,
) -> Result<(), EditErrorKind> {
    let c0 = content_start(text);
    let nl = newline(text);
    if text.len() == c0 {
        for line in lines {
            write_line(out, line)?;
            out.push(nl)?;
        }
        return Ok(());
    }
    if at == text.len() && !text.ends_with('\n') {
        out.push(nl)?;
        let last = line_start(text, c0, at)
            .and_then(|start| line_at(text, start))
            .ok_or_else(layout)?;
        if blank_before && !is_blank(last) {
            out.push(nl)?;
        }
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                out.push(nl)?;
            }
            write_line(out, line)?;
        }
        return Ok(());
    }
    // Anywhere else, `at` starts the content or follows a line break.
    if at != c0 && !text.get(..at).is_some_and(|before| before.ends_with('\n')) {
        return Err(layout());
    }
    if blank_before && at > c0 {
        // The line break that ends the line above.
        let above = at - 1;
        let start = line_start(text, c0, above).ok_or_else(layout)?;
        let line = text.get(start..above).ok_or_else(layout)?;
        if !is_blank(line.strip_suffix('\r').unwrap_or(line)) {
            out.push(nl)?;
        }
    }
    for line in lines {
        write_line(out, line)?;
        out.push(nl)?;
    }
    if blank_after && at != text.len() && !is_blank(line_at(text, at).ok_or_else(layout)?) {
        out.push(nl)?;
    }
    Ok(())
}

/// Plans replacing the value whose text is `span` with `value`, written on
/// one line.
pub(super) fn plan_replace(
    text: &str,
    limits: Limits,
    span: Span,
    value: &ValueKind,
) -> Result<Splice, EditErrorKind> {
    let limit = limits.max_input_bytes;
    let removed = span.end.checked_sub(span.start).ok_or_else(layout)?;
    let kept = text.len().checked_sub(removed).ok_or_else(layout)?;
    let room = limit
        .checked_sub(kept)
        .ok_or(EditErrorKind::TooLarge { limit })?;
    let mut out = Out::new(room, limit);
    write_value(&mut out, value)?;
    Ok(Splice {
        range: span.range(),
        text: out.into_string(),
    })
}

/// The names of the [`Step::Key`]s in `steps`, leaving out the indexes.
fn keys<'p>(steps: &[Step<'p>]) -> Vec<&'p str> {
    steps
        .iter()
        .filter_map(|step| match *step {
            Step::Key(name) => Some(name),
            Step::Index(_) => None,
        })
        .collect()
}

/// Plans adding `value` at `path`, of which `missing` says what exists: the
/// key line, inline entry or header block that adds it, and where.
pub(super) fn plan_insert<'p>(
    text: &str,
    root: &Table,
    limits: Limits,
    path: &[Step<'p>],
    missing: &Missing<'_>,
    value: &ValueKind,
) -> Result<Splice, EditErrorKind> {
    let limit = limits.max_input_bytes;
    let room = limit
        .checked_sub(text.len())
        .ok_or(EditErrorKind::TooLarge { limit })?;
    let mut out = Out::new(room, limit);
    let steps = |range: Range<usize>| path.get(range).map(keys).ok_or_else(layout);
    let anchor = missing.anchor;
    // t1…tm, then k.
    let added = steps(missing.first..path.len())?;
    let Some((key, tables)) = added.split_last() else {
        return Err(layout());
    };
    // The names from the nearest table that dotted keys did not make down
    // to N: p in a section, q in an inline table.
    let mut dotted = steps(missing.container..missing.first)?;
    dotted.extend(&added);
    let c0 = content_start(text);
    let inline = anchor.kind() == TableKind::Inline
        || (anchor.kind() == TableKind::Dotted && missing.container_kind == TableKind::Inline);

    let at = if inline {
        match last_leaf(anchor) {
            Some((_, leaf)) => {
                out.push(", ")?;
                write_path(&mut out, dotted.iter().copied())?;
                out.push(" = ")?;
                write_value(&mut out, value)?;
                leaf.span.end
            }
            None => {
                // Only an inline table can be empty; dotted keys made the
                // others with a leaf.
                let span = missing.anchor_span.ok_or_else(layout)?;
                if anchor.kind() != TableKind::Inline
                    || text.as_bytes().get(span.start) != Some(&b'{')
                {
                    return Err(layout());
                }
                let at = span.start + 1;
                out.push(" ")?;
                write_path(&mut out, added.iter().copied())?;
                out.push(" = ")?;
                write_value(&mut out, value)?;
                if text.as_bytes().get(at) == Some(&b'}') {
                    out.push(" ")?;
                }
                at
            }
        }
    } else if anchor.kind() == TableKind::Dotted {
        let (leaf_key, leaf) = last_leaf(anchor).ok_or_else(layout)?;
        let at = line_end(text, leaf.span.end).ok_or_else(layout)?;
        let indent = indent(text, c0, leaf_key).ok_or_else(layout)?;
        let line = Line::Pair {
            indent,
            names: dotted,
            value,
        };
        insert_lines(text, at, &[line], false, false, &mut out)?;
        at
    } else if tables.is_empty() && anchor.kind() != TableKind::Implicit {
        // The root, a header's table or an element of an array of tables
        // takes the key in its own section.
        let (at, indent, blank_after) = match (last_leaf(anchor), missing.anchor_span) {
            (Some((leaf_key, leaf)), _) => (
                line_end(text, leaf.span.end),
                indent(text, c0, leaf_key),
                false,
            ),
            (None, Some(header)) => (line_end(text, header.end), Some(""), false),
            (None, None) => (root_point(text, c0, root), Some(""), true),
        };
        let (at, indent) = at.zip(indent).ok_or_else(layout)?;
        let line = Line::Pair {
            indent,
            names: vec![*key],
            value,
        };
        insert_lines(text, at, &[line], false, blank_after, &mut out)?;
        at
    } else {
        // A header block after N's subtree, naming N and the tables made.
        let at = subtree_end(text, anchor, missing.anchor_span).ok_or_else(layout)?;
        let mut header = steps(0..missing.first)?;
        header.extend(tables);
        let lines = [
            Line::Header(header),
            Line::Pair {
                indent: "",
                names: vec![*key],
                value,
            },
        ];
        insert_lines(text, at, &lines, true, true, &mut out)?;
        at
    };
    Ok(Splice {
        range: at..at,
        text: out.into_string(),
    })
}
