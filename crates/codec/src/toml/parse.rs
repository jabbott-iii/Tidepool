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

//! The TOML parser.
//!
//! It reads the document one line at a time: a `[header]` or `[[header]]`
//! moves to the table it names, and a key/value pair adds to that table.
//! The current table is held as a reference into the tree, so a line never
//! walks down from the root again.
//!
//! - Values are read without recursing: open arrays and inline tables live
//!   on a stack on the heap (see `Parser::build`), so the thread's stack
//!   use does not grow with nesting whatever [`Limits::max_depth`] is.
//! - Depth counts every table and array between the root and a value, so it
//!   bounds a dotted key's or header's parts, arrays of tables (an array
//!   and its element are two levels), inline tables and arrays alike. A
//!   key's parts are checked as they are read, so a key with thousands of
//!   parts is stopped at the first one too deep.
//! - What may be added to a table is decided by its [`TableKind`]; see
//!   `Parser::descend`, `Parser::descend_dotted`, `Parser::define_table` and
//!   `Parser::add_array_element`.
//! - A failed parse takes apart what it built without recursing (see
//!   `dismantle`), so an error cannot overflow the stack either.
//!
//! The parser only ever stops at ASCII bytes (or after a whole leading
//! byte-order mark), so every position it slices the text at, and every
//! span, is at a character boundary.

mod scalar;

pub(super) use scalar::datetime_fault;

use super::table::reserve_one;
use super::{Array, ErrorKind, Key, Span, Table, TableKind, Value, ValueKind};
use crate::{CodecError, Limits};

/// U+FEFF in UTF-8.
const BYTE_ORDER_MARK: &[u8] = "\u{feff}".as_bytes();

// What the parser says it expected, in error messages.
const EXPRESSION: &str = "a key, a `[` header, a `#` comment or a line break";
const LINE_END: &str = "a line break or a `#` comment";
const KEY: &str = "a key";
const KEY_AFTER_DOT: &str = "a key after `.`";
const EQUALS: &str = "`.` or `=`";
const HEADER_CLOSE: &str = "`.` or `]`";
const ARRAY_HEADER_CLOSE: &str = "`.` or `]]`";
const ARRAY_HEADER_SECOND: &str = "a second `]` to close the `[[` header";
const VALUE: &str = "a value";
const ARRAY_NEXT: &str = "`,` or `]`";
const ARRAY_ITEM: &str = "a value or `]`";
const INLINE_NEXT: &str = "`,` or `}`";
const INLINE_KEY: &str = "a key or `}`";

/// Parses a TOML document and returns its root table.
///
/// See the [module documentation](super) for exactly what is accepted and
/// what the value tree holds.
///
/// # Errors
///
/// Returns [`CodecError::Toml`] with the offset, line and column of the
/// problem if the text is not valid TOML 1.1 or exceeds one of `limits`. It
/// never panics, whatever the text.
pub fn parse(text: &str, limits: Limits) -> Result<Table, CodecError> {
    check_size(text.as_bytes(), limits)?;
    Parser::new(text, limits).document()
}

/// Parses a TOML document from bytes, which must be UTF-8: the form a file
/// arrives in. Bytes that are not UTF-8 are an error
/// ([`ErrorKind::InvalidUtf8`]) wherever they are, in strings and comments
/// too.
///
/// Otherwise the same as [`parse`](fn@parse).
///
/// # Errors
///
/// As for [`parse`](fn@parse). [`Limits::max_input_bytes`] is checked before
/// the UTF-8 is.
pub fn parse_bytes(bytes: &[u8], limits: Limits) -> Result<Table, CodecError> {
    check_size(bytes, limits)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|error| error_at(bytes, error.valid_up_to(), ErrorKind::InvalidUtf8))?;
    Parser::new(text, limits).document()
}

/// Fails if the input is longer than [`Limits::max_input_bytes`], reporting
/// the first character that does not fit: the one that contains the first
/// byte past the limit.
fn check_size(bytes: &[u8], limits: Limits) -> Result<(), CodecError> {
    let limit = limits.max_input_bytes;
    if bytes.len() > limit {
        let kind = ErrorKind::InputTooLarge {
            length: bytes.len(),
            limit,
        };
        return Err(error_at(bytes, character_start(bytes, limit), kind));
    }
    Ok(())
}

/// The start of the UTF-8 character that byte `at` of `bytes` belongs to:
/// `at` itself, or up to three bytes before it (the most continuation bytes
/// a character has) if `at` is a continuation byte.
fn character_start(bytes: &[u8], at: usize) -> usize {
    let mut start = at;
    while start > 0 && at - start < 3 && bytes.get(start).is_some_and(|&byte| byte & 0xC0 == 0x80) {
        start -= 1;
    }
    start
}

/// Builds an error for the byte offset `at` of `bytes`, working out its line
/// and column. This rescans the input, which is fine for a path taken once.
///
/// Lines end at LF, so a CR LF ends one line (a CR alone is an error). The
/// column counts the bytes that start a UTF-8 character, which is the number
/// of characters wherever the input before `at` is valid UTF-8. It always
/// is, except for [`ErrorKind::InputTooLarge`] from [`parse_bytes`], which
/// is checked before the UTF-8: every other error in an invalid input is
/// reported at the first bad byte.
fn error_at(bytes: &[u8], at: usize, kind: ErrorKind) -> CodecError {
    let before = bytes.get(..at).unwrap_or(bytes);
    let before = before.strip_prefix(BYTE_ORDER_MARK).unwrap_or(before);
    let line = before.iter().filter(|&&byte| byte == b'\n').count() + 1;
    let line_start = before
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |newline| newline + 1);
    let line_text = before.get(line_start..).unwrap_or_default();
    let column = line_text
        .iter()
        .filter(|&&byte| byte & 0xC0 != 0x80)
        .count()
        + 1;
    CodecError::Toml {
        offset: at,
        line,
        column,
        kind,
    }
}

/// What a dotted key or a header holds, for error messages (the document
/// model's too).
pub(super) fn described(kind: &ValueKind) -> &'static str {
    match kind {
        ValueKind::String(_) => "a string",
        ValueKind::Integer(_) => "an integer",
        ValueKind::Float(_) => "a float",
        ValueKind::Boolean(_) => "a boolean",
        ValueKind::Datetime(_) => "a date-time",
        ValueKind::Array(array) if array.of_tables => "an array of tables",
        ValueKind::Array(_) => "an array",
        ValueKind::Table(table) if table.kind() == TableKind::Inline => "an inline table",
        ValueKind::Table(_) => "a table",
    }
}

/// Drops the values in `work` without recursing once per level of nesting,
/// as dropping a [`Value`] does, so that throwing away a deep tree cannot
/// overflow the stack. Each non-empty container's entries move to a work
/// list and the container is dropped empty; scalars and empty containers
/// are dropped where they are, which recurses at most once.
fn dismantle(mut work: Vec<Value>) {
    fn has_entries(value: &Value) -> bool {
        match &value.kind {
            ValueKind::Array(array) => !array.items.is_empty(),
            ValueKind::Table(table) => !table.is_empty(),
            _ => false,
        }
    }
    while let Some(value) = work.pop() {
        match value.kind {
            ValueKind::Array(array) => work.extend(array.items.into_iter().filter(has_entries)),
            ValueKind::Table(table) => work.extend(
                table
                    .into_entries()
                    .into_iter()
                    .map(|(_, value)| value)
                    .filter(has_entries),
            ),
            _ => {}
        }
    }
}

/// The values of a table's entries, for [`dismantle`].
fn values_of(table: Table) -> Vec<Value> {
    table
        .into_entries()
        .into_iter()
        .map(|(_, value)| value)
        .collect()
}

/// `entries` in an allocation of exactly their size. `Vec::with_capacity`
/// guarantees the exact capacity; moving into a fresh allocation, rather
/// than shrinking in place, frees the old one whole.
fn exact<T>(mut entries: Vec<T>) -> Vec<T> {
    if entries.capacity() == entries.len() {
        return entries;
    }
    let mut exact = Vec::with_capacity(entries.len());
    exact.append(&mut entries);
    exact
}

/// Adds an element to an array of tables and returns it. Room grows by a
/// quarter, as a table's does.
fn push_element(items: &mut Vec<Value>, span: Span) -> &mut Table {
    let element = Value {
        kind: ValueKind::Table(Table::new(TableKind::ArrayHeader)),
        span,
    };
    reserve_one(items);
    match &mut items.push_mut(element).kind {
        ValueKind::Table(table) => table,
        _ => unreachable!("the element just added is a table"),
    }
}

/// Leaves no room to spare in any table or array of tables of a parsed
/// tree. While parsing they keep a little room (see [`reserve_one`]), since
/// later lines can add to them; once the parse is over, none can grow.
/// Arrays written as values and inline tables already have none. The walk
/// keeps its own list, so it does not recurse once per level either.
fn settle(root: &mut Table) {
    root.settle();
    let mut work: Vec<&mut Value> = root.values_mut().collect();
    while let Some(value) = work.pop() {
        match &mut value.kind {
            ValueKind::Table(table) => {
                table.settle();
                work.extend(table.values_mut());
            }
            ValueKind::Array(array) => {
                array.items.shrink_to_fit();
                work.extend(array.items.iter_mut());
            }
            _ => {}
        }
    }
}

/// A key as written: its parts, and the span from the first to the last.
/// `prefix` names the tables the last part goes in.
#[derive(Clone, Debug)]
struct KeyPath {
    prefix: Vec<Key>,
    last: Key,
    span: Span,
}

/// An array or inline table that has been opened and not yet closed.
enum Frame {
    /// The elements so far.
    Array {
        items: Vec<Value>,
        /// Where its `[` is.
        start: usize,
        /// How deep the array itself is.
        depth: usize,
    },
    /// The entries so far, and the key of the entry whose value is being
    /// read.
    Inline {
        table: Table,
        /// Where its `{` is.
        start: usize,
        /// How deep the inline table itself is.
        depth: usize,
        key: KeyPath,
    },
}

impl Frame {
    /// The values the container holds so far, for [`dismantle`].
    fn into_values(self) -> Vec<Value> {
        match self {
            Self::Array { items, .. } => items,
            Self::Inline { table, .. } => values_of(table),
        }
    }
}

/// What follows an entry of an array or inline table.
enum After {
    /// A comma, then another entry.
    Entry,
    /// The closing bracket or brace (after a trailing comma, perhaps),
    /// which has been consumed.
    Close,
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Byte offset of the next unread byte.
    pos: usize,
    limits: Limits,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, limits: Limits) -> Self {
        let bytes = text.as_bytes();
        let pos = if bytes.starts_with(BYTE_ORDER_MARK) {
            BYTE_ORDER_MARK.len()
        } else {
            0
        };
        Self {
            text,
            bytes,
            pos,
            limits,
        }
    }

    /// Parses the whole document into its root table, and settles it (see
    /// [`settle`]). If that fails, what was built is dismantled rather than
    /// dropped.
    fn document(mut self) -> Result<Table, CodecError> {
        let mut root = Table::new(TableKind::Root);
        match self.expressions(&mut root) {
            Ok(()) => {
                settle(&mut root);
                Ok(root)
            }
            Err(error) => {
                dismantle(values_of(root));
                Err(error)
            }
        }
    }

    /// Reads every line: a key/value pair, a header, or nothing, each with
    /// an optional comment.
    fn expressions(&mut self, root: &mut Table) -> Result<(), CodecError> {
        // The table key/value pairs go in, and how deep it is.
        let mut section: &mut Table = &mut *root;
        let mut depth = 0;
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => return Ok(()),
                Some(b'[') => {
                    let (table, table_depth) = self.header(&mut *root)?;
                    section = table;
                    depth = table_depth;
                }
                Some(b'#' | b'\n' | b'\r') => {}
                Some(_) => self.keyval(&mut *section, depth)?,
            }
            self.end_of_line()?;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn error(&self, at: usize, kind: ErrorKind) -> CodecError {
        error_at(self.bytes, at, kind)
    }

    /// Builds the error for finding the wrong thing, or nothing, at `at`. A
    /// CR that does not start a CR LF is reported as that, wherever it is,
    /// since it is wrong anywhere.
    fn unexpected(&self, at: usize, expected: &'static str) -> CodecError {
        let kind = match self.text.get(at..).and_then(|rest| rest.chars().next()) {
            Some('\r') if self.bytes.get(at + 1) != Some(&b'\n') => ErrorKind::BareCarriageReturn,
            Some(found) => ErrorKind::UnexpectedCharacter { found, expected },
            None => ErrorKind::UnexpectedEnd { expected },
        };
        self.error(at, kind)
    }

    /// Consumes `byte`, or fails saying that `expected` was needed.
    fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), CodecError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(self.pos, expected))
        }
    }

    /// Skips spaces and tabs.
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Skips whitespace, line breaks and comments: what may come between
    /// the entries of an array or inline table.
    fn skip_trivia(&mut self) -> Result<(), CodecError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n') => self.pos += 1,
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => self.pos += 2,
                Some(b'\r') => return Err(self.error(self.pos, ErrorKind::BareCarriageReturn)),
                Some(b'#') => self.comment()?,
                _ => return Ok(()),
            }
        }
    }

    /// Skips a comment from its `#` to the end of its line, leaving the line
    /// break. Tab is the only control character a comment may hold.
    fn comment(&mut self) -> Result<(), CodecError> {
        self.pos += 1; // `#`
        loop {
            match self.peek() {
                None | Some(b'\n') => return Ok(()),
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => return Ok(()),
                Some(b'\r') => return Err(self.error(self.pos, ErrorKind::BareCarriageReturn)),
                Some(b'\t') => self.pos += 1,
                Some(byte) if byte < 0x20 || byte == 0x7F => {
                    let kind = ErrorKind::ControlCharacter {
                        found: char::from(byte),
                    };
                    return Err(self.error(self.pos, kind));
                }
                // Every other byte, including each byte of a non-ASCII
                // character, which UTF-8 has already validated.
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Reads the end of a line: whitespace, an optional comment, then a line
    /// break or the end of the text.
    fn end_of_line(&mut self) -> Result<(), CodecError> {
        self.skip_whitespace();
        if self.peek() == Some(b'#') {
            self.comment()?;
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.pos += 1;
                Ok(())
            }
            Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                self.pos += 2;
                Ok(())
            }
            Some(b'\r') => Err(self.error(self.pos, ErrorKind::BareCarriageReturn)),
            Some(_) => Err(self.unexpected(self.pos, LINE_END)),
        }
    }

    /// Fails if a table or array may not be `depth` deep, reporting `at`.
    fn check_depth(&self, depth: usize, at: usize) -> Result<(), CodecError> {
        let limit = self.limits.max_depth;
        if depth > limit {
            return Err(self.error(at, ErrorKind::TooDeep { limit }));
        }
        Ok(())
    }

    /// Fails if a table or array that already has `len` entries may not get
    /// another, reporting `at`.
    fn check_room(&self, len: usize, at: usize) -> Result<(), CodecError> {
        let limit = self.limits.max_entries;
        if len >= limit {
            return Err(self.error(at, ErrorKind::TooManyEntries { limit }));
        }
        Ok(())
    }

    // ------------------------------------------------------------- keys

    /// Reads a key of a key/value pair, simple or dotted, whose tables go
    /// in a table `depth` deep. `expected` is what to name if no key starts
    /// here.
    ///
    /// Each part before the last names a table one level deeper than the
    /// one before, so it is checked against the depth limit as soon as the
    /// `.` after it shows that it is one.
    fn key_path(&mut self, depth: usize, expected: &'static str) -> Result<KeyPath, CodecError> {
        let start = self.pos;
        let mut prefix = Vec::new();
        let mut last = self.simple_key(expected)?;
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'.') {
                break;
            }
            self.check_depth(depth + prefix.len() + 1, last.span.start)?;
            self.pos += 1;
            self.skip_whitespace();
            let next = self.simple_key(KEY_AFTER_DOT)?;
            prefix.push(std::mem::replace(&mut last, next));
        }
        let span = Span::new(start, last.span.end);
        Ok(KeyPath { prefix, last, span })
    }

    /// Reads a bare or quoted key.
    fn simple_key(&mut self, expected: &'static str) -> Result<Key, CodecError> {
        let start = self.pos;
        let rest = self.bytes.get(start..).unwrap_or_default();
        let name = match self.peek() {
            Some(b'"' | b'\'') if rest.starts_with(b"\"\"\"") || rest.starts_with(b"'''") => {
                return Err(self.error(start, ErrorKind::MultilineKey));
            }
            Some(b'"') => self.basic_string()?,
            Some(b'\'') => self.literal_string()?,
            Some(byte) if is_bare(byte) => {
                while self.peek().is_some_and(is_bare) {
                    self.pos += 1;
                }
                let name = self.text.get(start..self.pos).unwrap_or_default();
                self.check_string_len(start, 0, name.len())?;
                name.to_string()
            }
            _ => return Err(self.unexpected(start, expected)),
        };
        Ok(Key {
            name,
            span: Span::new(start, self.pos),
        })
    }

    // ----------------------------------------------------------- tables

    /// Reads a key/value pair and adds it to `table`, which is `depth`
    /// deep.
    fn keyval(&mut self, table: &mut Table, depth: usize) -> Result<(), CodecError> {
        let path = self.key_path(depth, EXPRESSION)?;
        let target = self.prepare(table, &path, depth)?;
        self.skip_whitespace();
        self.expect(b'=', EQUALS)?;
        self.skip_whitespace();
        let value = self.value(depth + path.prefix.len())?;
        target.push(path.last, value);
        Ok(())
    }

    /// Walks the tables of `path`'s prefix down from `table`, which is
    /// `depth` deep, creating those that do not exist, and checks that the
    /// last part can be added to the table it reaches, which it returns.
    fn prepare<'t>(
        &self,
        mut table: &'t mut Table,
        path: &KeyPath,
        depth: usize,
    ) -> Result<&'t mut Table, CodecError> {
        for (index, key) in path.prefix.iter().enumerate() {
            table = self.descend_dotted(table, key, path.span, depth + index + 1)?;
        }
        let at = path.last.span.start;
        if table.contains_key(&path.last.name) {
            return Err(self.error(at, ErrorKind::DuplicateKey));
        }
        self.check_room(table.len(), at)?;
        Ok(table)
    }

    /// Goes from `table` into the table named `key`, one part of a dotted
    /// key that spans `span`, creating it `depth` deep if it does not exist.
    ///
    /// A dotted key creates and defines tables. It may go into a table it
    /// created, or define one that headers only created as their prefix,
    /// but not add to a table defined some other way: by a header, inline,
    /// or as an array of tables. A table dotted keys made can only be
    /// reached by dotted keys from the section that made it, since every
    /// other way in passes through a table they cannot enter.
    fn descend_dotted<'t>(
        &self,
        table: &'t mut Table,
        key: &Key,
        span: Span,
        depth: usize,
    ) -> Result<&'t mut Table, CodecError> {
        let (entry_key, value) = match table.position(&key.name) {
            Some(at) => table.entry_mut(at),
            None => {
                self.check_depth(depth, key.span.start)?;
                self.check_room(table.len(), key.span.start)?;
                let new = Table::new(TableKind::Dotted);
                return Ok(table.push_table(key.clone(), new, span));
            }
        };
        let at = key.span.start;
        match &mut value.kind {
            ValueKind::Table(inner) => match inner.kind() {
                TableKind::Implicit => {
                    // Defined here, so it is where it was defined.
                    inner.set_kind(TableKind::Dotted);
                    value.span = span;
                    entry_key.span = key.span;
                    Ok(inner)
                }
                TableKind::Dotted => Ok(inner),
                TableKind::Inline => Err(self.error(at, ErrorKind::ExtendsInlineTable)),
                TableKind::Root | TableKind::Header | TableKind::ArrayHeader => {
                    Err(self.error(at, ErrorKind::ExtendsDefinedTable))
                }
            },
            other => {
                let found = described(other);
                Err(self.error(at, ErrorKind::NotATable { found }))
            }
        }
    }

    /// Reads a `[header]` or `[[header]]` and returns the table it defines,
    /// and how deep that is.
    fn header<'t>(&mut self, root: &'t mut Table) -> Result<(&'t mut Table, usize), CodecError> {
        let start = self.pos;
        let array = self.peek_at(1) == Some(b'[');
        self.pos += if array { 2 } else { 1 };
        self.skip_whitespace();
        let mut prefix = Vec::new();
        let mut last = self.simple_key(KEY)?;
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'.') {
                break;
            }
            if prefix.len() + 1 > self.limits.max_depth {
                // Every part is at least one level deeper than the one
                // before, so `last` is too deep for any table to exist or
                // be made there, and walking the parts read so far fails
                // at it or at an earlier part that is already too deep.
                let at = last.span.start;
                prefix.push(last);
                let mut table = root;
                let mut depth = 0;
                for key in &prefix {
                    table = self.descend(table, key, Span::new(start, self.pos), &mut depth)?;
                }
                return Err(self.error(
                    at,
                    ErrorKind::TooDeep {
                        limit: self.limits.max_depth,
                    },
                ));
            }
            self.pos += 1;
            self.skip_whitespace();
            let next = self.simple_key(KEY_AFTER_DOT)?;
            prefix.push(std::mem::replace(&mut last, next));
        }
        if array {
            self.expect(b']', ARRAY_HEADER_CLOSE)?;
            self.expect(b']', ARRAY_HEADER_SECOND)?;
        } else {
            self.expect(b']', HEADER_CLOSE)?;
        }
        let span = Span::new(start, self.pos);

        let mut table = root;
        let mut depth = 0;
        for key in &prefix {
            table = self.descend(table, key, span, &mut depth)?;
        }
        if array {
            self.add_array_element(table, last, span, depth)
        } else {
            self.define_table(table, last, span, depth)
        }
    }

    /// Goes from `table`, `depth` deep, into the table named `key`, a part
    /// of a header's prefix, creating it if it does not exist. Adds to
    /// `depth` the levels it goes down: one for a table, two for an array of
    /// tables, whose last element it goes into.
    ///
    /// A header may go through any table but an inline one, which is
    /// complete as written, and through an array of tables, but not through
    /// any other value.
    fn descend<'t>(
        &self,
        table: &'t mut Table,
        key: &Key,
        span: Span,
        depth: &mut usize,
    ) -> Result<&'t mut Table, CodecError> {
        let at = key.span.start;
        let (_, value) = match table.position(&key.name) {
            Some(position) => table.entry_mut(position),
            None => {
                self.check_depth(*depth + 1, at)?;
                self.check_room(table.len(), at)?;
                *depth += 1;
                let new = Table::new(TableKind::Implicit);
                return Ok(table.push_table(key.clone(), new, span));
            }
        };
        match &mut value.kind {
            ValueKind::Table(inner) if inner.kind() == TableKind::Inline => {
                Err(self.error(at, ErrorKind::ExtendsInlineTable))
            }
            ValueKind::Table(inner) => {
                *depth += 1;
                Ok(inner)
            }
            ValueKind::Array(array) => match (array.of_tables, array.items.last_mut()) {
                (
                    true,
                    Some(Value {
                        kind: ValueKind::Table(element),
                        ..
                    }),
                ) => {
                    *depth += 2;
                    Ok(element)
                }
                _ => {
                    let found = "an array";
                    Err(self.error(at, ErrorKind::NotATable { found }))
                }
            },
            other => {
                let found = described(other);
                Err(self.error(at, ErrorKind::NotATable { found }))
            }
        }
    }

    /// Defines the table `key` names in `table`, which is `depth` deep, for
    /// a `[header]` that spans `span`, and returns it and its depth. The
    /// table may exist only as a prefix of earlier headers.
    fn define_table<'t>(
        &self,
        table: &'t mut Table,
        key: Key,
        span: Span,
        depth: usize,
    ) -> Result<(&'t mut Table, usize), CodecError> {
        let at = key.span.start;
        let depth = depth + 1;
        let (entry_key, value) = match table.position(&key.name) {
            Some(position) => table.entry_mut(position),
            None => {
                self.check_depth(depth, at)?;
                self.check_room(table.len(), at)?;
                let new = Table::new(TableKind::Header);
                return Ok((table.push_table(key, new, span), depth));
            }
        };
        match &mut value.kind {
            ValueKind::Table(inner) if inner.kind() == TableKind::Implicit => {
                // Defined here, so it is where it was defined.
                inner.set_kind(TableKind::Header);
                value.span = span;
                entry_key.span = key.span;
                Ok((inner, depth))
            }
            ValueKind::Table(_) => Err(self.error(at, ErrorKind::DuplicateTable)),
            ValueKind::Array(array) if array.of_tables => {
                Err(self.error(at, ErrorKind::DuplicateTable))
            }
            _ => Err(self.error(at, ErrorKind::DuplicateKey)),
        }
    }

    /// Adds a table to the array of tables `key` names in `table`, which is
    /// `depth` deep, for an `[[header]]` that spans `span`, creating the
    /// array if it does not exist, and returns the new table and its depth.
    fn add_array_element<'t>(
        &self,
        table: &'t mut Table,
        key: Key,
        span: Span,
        depth: usize,
    ) -> Result<(&'t mut Table, usize), CodecError> {
        let at = key.span.start;
        let element_depth = depth + 2;
        let (_, value) = match table.position(&key.name) {
            Some(position) => table.entry_mut(position),
            None => {
                self.check_depth(element_depth, at)?;
                self.check_room(table.len(), at)?;
                let array = Value {
                    kind: ValueKind::Array(Array {
                        items: Vec::with_capacity(1),
                        of_tables: true,
                    }),
                    span,
                };
                table.push(key, array)
            }
        };
        let found = match &mut value.kind {
            ValueKind::Array(array) => {
                if array.of_tables {
                    self.check_room(array.items.len(), at)?;
                    return Ok((push_element(&mut array.items, span), element_depth));
                }
                "an array"
            }
            other => described(other),
        };
        Err(self.error(at, ErrorKind::NotAnArrayOfTables { found }))
    }

    // ----------------------------------------------------------- values

    /// Reads a value, with everything nested in it, for a table or array
    /// `depth` deep. If that fails, what was built so far is dismantled
    /// rather than dropped.
    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        let mut stack: Vec<Frame> = Vec::new();
        let result = self.build(&mut stack, depth);
        if result.is_err() {
            dismantle(stack.into_iter().flat_map(Frame::into_values).collect());
        }
        result
    }

    /// The loop behind [`Parser::value`]. It alternates between two steps.
    /// The first reads the start of a value: a scalar, which is then
    /// complete, or the opening of a non-empty array or inline table, which
    /// is pushed onto `stack` (with the key of its first entry, for an
    /// inline table). The second adds a complete value to the container on
    /// top of the stack and reads what follows it: a comma and the next
    /// entry's start (back to the first step), or the closing bracket or
    /// brace, which completes that container in turn.
    ///
    /// Every value it has built is in `stack` whenever it returns an error,
    /// and `stack` is empty when it returns a value.
    ///
    /// An inline table's key is checked when it is read, before its value,
    /// and the value is added when it is complete: by then the key's tables
    /// exist, and nothing else can have changed that inline table.
    fn build(&mut self, stack: &mut Vec<Frame>, depth: usize) -> Result<Value, CodecError> {
        // What to name in the error if no value starts here.
        let mut expected = VALUE;
        loop {
            // How deep the table or array the next value goes in is.
            let outer = match stack.last() {
                None => depth,
                Some(Frame::Array { depth, .. }) => *depth,
                Some(Frame::Inline { depth, key, .. }) => depth + key.prefix.len(),
            };
            let start = self.pos;
            let mut value = match self.peek() {
                Some(b'[') => {
                    self.check_depth(outer + 1, start)?;
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        let empty = Array {
                            items: Vec::new(),
                            of_tables: false,
                        };
                        Value {
                            kind: ValueKind::Array(empty),
                            span: Span::new(start, self.pos),
                        }
                    } else {
                        if self.peek().is_some() {
                            self.check_room(0, self.pos)?;
                        }
                        stack.push(Frame::Array {
                            items: Vec::with_capacity(1),
                            start,
                            depth: outer + 1,
                        });
                        expected = ARRAY_ITEM;
                        continue;
                    }
                }
                Some(b'{') => {
                    self.check_depth(outer + 1, start)?;
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        Value {
                            kind: ValueKind::Table(Table::new(TableKind::Inline)),
                            span: Span::new(start, self.pos),
                        }
                    } else {
                        let mut table = Table::new(TableKind::Inline);
                        // The key's tables are not on the stack yet, so
                        // they are taken apart here if it fails.
                        let key = match self.inline_key(&mut table, outer + 1, INLINE_KEY) {
                            Ok(key) => key,
                            Err(error) => {
                                dismantle(values_of(table));
                                return Err(error);
                            }
                        };
                        stack.push(Frame::Inline {
                            table,
                            start,
                            depth: outer + 1,
                            key,
                        });
                        expected = VALUE;
                        continue;
                    }
                }
                _ => self.scalar(expected)?,
            };

            // `value` is complete: add it to the container it is in.
            loop {
                match stack.last_mut() {
                    None => return Ok(value),
                    Some(Frame::Array { items, start, .. }) => {
                        items.push(value);
                        match self.after_entry(b']', ARRAY_NEXT)? {
                            After::Entry => {
                                if self.peek().is_some() {
                                    self.check_room(items.len(), self.pos)?;
                                }
                                expected = ARRAY_ITEM;
                                break;
                            }
                            After::Close => {
                                let array = Array {
                                    items: exact(std::mem::take(items)),
                                    of_tables: false,
                                };
                                value = Value {
                                    kind: ValueKind::Array(array),
                                    span: Span::new(*start, self.pos),
                                };
                                stack.pop();
                            }
                        }
                    }
                    Some(Frame::Inline {
                        table,
                        start,
                        depth,
                        key,
                    }) => {
                        match self.prepare(table, key, *depth) {
                            Ok(target) => {
                                target.push(key.last.clone(), value);
                            }
                            Err(error) => {
                                dismantle(vec![value]);
                                return Err(error);
                            }
                        }
                        match self.after_entry(b'}', INLINE_NEXT)? {
                            After::Entry => {
                                *key = self.inline_key(table, *depth, INLINE_KEY)?;
                                expected = VALUE;
                                break;
                            }
                            After::Close => {
                                let mut table =
                                    std::mem::replace(table, Table::new(TableKind::Inline));
                                table.shrink();
                                value = Value {
                                    kind: ValueKind::Table(table),
                                    span: Span::new(*start, self.pos),
                                };
                                stack.pop();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Reads the key of an inline table's entry, and the `=` after it, and
    /// checks that it can be added to `table`, which is `depth` deep,
    /// creating the tables of a dotted key. `expected` is what to name if
    /// no key starts here.
    fn inline_key(
        &mut self,
        table: &mut Table,
        depth: usize,
        expected: &'static str,
    ) -> Result<KeyPath, CodecError> {
        let path = self.key_path(depth, expected)?;
        self.prepare(table, &path, depth)?;
        self.skip_whitespace();
        self.expect(b'=', EQUALS)?;
        self.skip_whitespace();
        Ok(path)
    }

    /// Reads what follows an entry of an array or inline table that `close`
    /// ends: a comma (leaving the position at the next entry), or `close`.
    /// A comma right before `close` is a trailing comma, and the container
    /// ends. Line breaks and comments may come before and after the comma.
    fn after_entry(&mut self, close: u8, next: &'static str) -> Result<After, CodecError> {
        self.skip_trivia()?;
        match self.peek() {
            Some(b',') => {
                self.pos += 1;
                self.skip_trivia()?;
                if self.peek() == Some(close) {
                    self.pos += 1;
                    return Ok(After::Close);
                }
                Ok(After::Entry)
            }
            Some(byte) if byte == close => {
                self.pos += 1;
                Ok(After::Close)
            }
            _ => Err(self.unexpected(self.pos, next)),
        }
    }
}

/// Whether `byte` may be part of a bare key: an ASCII letter or digit, `-`
/// or `_`. The writer writes a key bare only when every byte is one.
pub(super) fn is_bare(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

#[cfg(test)]
mod tests;
