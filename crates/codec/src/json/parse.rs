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

//! The JSON / JSONC parser.
//!
//! Ported from nadir-codec `json/parse.rs`. Adapted for Tidepool:
//!
//! - It is iterative: open arrays and objects live on a stack on the heap,
//!   not in recursive calls, so the thread's stack use does not grow with
//!   nesting whatever [`Limits::max_depth`] is.
//! - Strict JSON and JSONC are separate [`Dialect`]s; strict mode reports
//!   comments and trailing commas as such.
//! - Repeated keys are accepted and every member is kept, as RFC 8259 §9
//!   requires of a parser (Nadir rejected them).
//! - [`parse_bytes`] validates UTF-8 itself; [`Limits::max_input_bytes`]
//!   and [`Limits::max_number_bytes`] are new.
//! - A failed parse takes apart what it built without recursing (see
//!   `dismantle`), so an error cannot overflow the stack either.
//! - Arrays and objects are built with no spare capacity, so that the
//!   memory a parsed tree takes is bounded by its text (see
//!   [`Limits`]).
//! - Errors carry the byte offset as well as the line and column.
//! - After a comma, "expected" names what the dialect allows there.
//!
//! The parser only ever stops at ASCII bytes (or after a whole leading
//! byte-order mark), so every position it slices the text at is a character
//! boundary.

use super::{Dialect, ErrorKind, Number, Value};
use crate::{CodecError, Limits};

/// U+FEFF in UTF-8.
const BYTE_ORDER_MARK: &[u8] = "\u{feff}".as_bytes();

// What the parser says it expected, in error messages.
const VALUE: &str = "a value";
const END: &str = "the end of the text";
const DIGIT: &str = "a digit";
const ARRAY_NEXT: &str = "`,` or `]`";
const ARRAY_ITEM: &str = "a value or `]`";
const OBJECT_NEXT: &str = "`,` or `}`";
const KEY: &str = "a string key";
const KEY_OR_CLOSE: &str = "a string key or `}`";
const COLON: &str = "`:` after the key";
// For a construct that the text ends inside. These errors are reported
// where the construct starts, which is more use than the end of the text.
const STRING_CLOSE: &str = "`\"` to close the string that starts here";
const COMMENT_CLOSE: &str = "`*/` to close the comment that starts here";
const ESCAPE_REST: &str = "the rest of the escape sequence that starts here";
const HEX_DIGITS: &str = "four hex digits for the escape that starts here";

/// Parses JSON or JSONC text into a [`Value`].
///
/// See the [module documentation](super) for exactly what each [`Dialect`]
/// accepts.
///
/// # Errors
///
/// Returns [`CodecError::Json`] with the offset, line and column of the
/// problem if the text is malformed or exceeds one of `limits`. It never
/// panics, whatever the text.
pub fn parse(text: &str, dialect: Dialect, limits: Limits) -> Result<Value, CodecError> {
    check_size(text.as_bytes(), limits)?;
    Parser::new(text, dialect, limits).document()
}

/// Parses JSON or JSONC from bytes, which must be UTF-8: the form a child
/// process's output or a file arrives in. Bytes that are not UTF-8 are an
/// error ([`ErrorKind::InvalidUtf8`]) wherever they are, inside strings too.
///
/// Otherwise the same as [`parse`](fn@parse).
///
/// # Errors
///
/// As for [`parse`](fn@parse). [`Limits::max_input_bytes`] is checked before the
/// UTF-8 is.
pub fn parse_bytes(bytes: &[u8], dialect: Dialect, limits: Limits) -> Result<Value, CodecError> {
    check_size(bytes, limits)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|error| error_at(bytes, error.valid_up_to(), ErrorKind::InvalidUtf8))?;
    Parser::new(text, dialect, limits).document()
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
/// The column counts the bytes that start a UTF-8 character, which is the
/// number of characters wherever the input before `at` is valid UTF-8. It
/// always is, except for [`ErrorKind::InputTooLarge`] from [`parse_bytes`],
/// which is checked before the UTF-8: every other error in an invalid input
/// is reported at the first bad byte.
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
    CodecError::Json {
        offset: at,
        line,
        column,
        kind,
    }
}

/// An array or object that has been opened and not yet closed.
enum Frame {
    /// The elements so far.
    Array(Vec<Value>),
    /// The members so far, and the key of the member whose value is being
    /// parsed.
    Object {
        members: Vec<(String, Value)>,
        key: String,
    },
}

impl Frame {
    /// The container as far as it got.
    fn into_value(self) -> Value {
        match self {
            Self::Array(items) => Value::Array(items),
            Self::Object { members, .. } => Value::Object(members),
        }
    }
}

/// Drops the values in `work` without recursing once per level of nesting,
/// as dropping a [`Value`] does, so that throwing away a deep tree cannot
/// overflow the stack. Each non-empty container's entries move to a work list and the
/// container is dropped empty; scalars and empty containers are dropped
/// where they are, which recurses at most once.
fn dismantle(mut work: Vec<Value>) {
    fn has_entries(value: &Value) -> bool {
        match value {
            Value::Array(items) => !items.is_empty(),
            Value::Object(members) => !members.is_empty(),
            _ => false,
        }
    }
    while let Some(value) = work.pop() {
        match value {
            Value::Array(items) => work.extend(items.into_iter().filter(has_entries)),
            Value::Object(members) => work.extend(
                members
                    .into_iter()
                    .map(|(_, value)| value)
                    .filter(has_entries),
            ),
            _ => {}
        }
    }
}

/// `entries` in an allocation of exactly their size. `Vec::with_capacity`
/// guarantees the exact capacity; moving into a fresh allocation, rather
/// than shrinking in place, frees the old one whole, so the allocator can
/// reuse it for the next container instead of keeping a sliver.
fn exact<T>(mut entries: Vec<T>) -> Vec<T> {
    if entries.capacity() == entries.len() {
        return entries;
    }
    let mut exact = Vec::with_capacity(entries.len());
    exact.append(&mut entries);
    exact
}

/// What follows an entry of an array or object.
enum After {
    /// A comma, then another entry.
    Entry,
    /// The closing bracket or brace, which has been consumed.
    Close,
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Byte offset of the next unread byte.
    pos: usize,
    /// `true` for [`Dialect::Jsonc`].
    jsonc: bool,
    limits: Limits,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, dialect: Dialect, limits: Limits) -> Self {
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
            jsonc: dialect == Dialect::Jsonc,
            limits,
        }
    }

    /// Parses the whole text: one value, then nothing but trivia.
    fn document(mut self) -> Result<Value, CodecError> {
        let value = self.value_tree()?;
        if let Err(error) = self.end_of_text() {
            dismantle(vec![value]);
            return Err(error);
        }
        Ok(value)
    }

    /// Skips trivia, then fails unless that is the end of the text.
    fn end_of_text(&mut self) -> Result<(), CodecError> {
        self.skip_trivia()?;
        match self.peek() {
            None => Ok(()),
            Some(_) => Err(self.unexpected(self.pos, END)),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn error(&self, at: usize, kind: ErrorKind) -> CodecError {
        error_at(self.bytes, at, kind)
    }

    /// Builds the error for finding the wrong thing, or nothing, at `at`.
    fn unexpected(&self, at: usize, expected: &'static str) -> CodecError {
        let kind = match self.text.get(at..).and_then(|rest| rest.chars().next()) {
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

    /// Skips whitespace and, in JSONC, comments.
    fn skip_trivia(&mut self) -> Result<(), CodecError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') => {
                    let next = self.bytes.get(self.pos + 1).copied();
                    if !matches!(next, Some(b'/' | b'*')) {
                        // A lone slash is not trivia. Whoever wanted a token
                        // here reports it.
                        return Ok(());
                    }
                    if !self.jsonc {
                        return Err(self.error(self.pos, ErrorKind::CommentNotAllowed));
                    }
                    let body = self.bytes.get(self.pos + 2..).unwrap_or_default();
                    if next == Some(b'/') {
                        // To the end of the line, which is left for the
                        // whitespace rule.
                        let len = body
                            .iter()
                            .position(|&byte| byte == b'\n' || byte == b'\r')
                            .unwrap_or(body.len());
                        self.pos += 2 + len;
                    } else {
                        match body.windows(2).position(|pair| pair == b"*/") {
                            Some(close) => self.pos += 2 + close + 2,
                            None => {
                                let kind = ErrorKind::UnexpectedEnd {
                                    expected: COMMENT_CLOSE,
                                };
                                return Err(self.error(self.pos, kind));
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// Parses one value, with everything nested in it. If that fails, what
    /// was built so far is dismantled rather than dropped.
    fn value_tree(&mut self) -> Result<Value, CodecError> {
        let mut stack: Vec<Frame> = Vec::new();
        let result = self.build(&mut stack);
        if result.is_err() {
            dismantle(stack.into_iter().map(Frame::into_value).collect());
        }
        result
    }

    /// The loop behind [`Parser::value_tree`]. It alternates between two
    /// steps. The first reads the start of a value: a scalar, which is then
    /// complete, or the opening of a non-empty container, which is pushed
    /// onto `stack`. The second adds a complete value to the container on
    /// top of the stack and reads what follows it: a comma and the next
    /// entry's start (back to the first step), or the closing bracket,
    /// which completes that container in turn.
    ///
    /// Every value it has built is in `stack` whenever it returns an error,
    /// and `stack` is empty when it returns a value.
    ///
    /// A container starts with room for one entry and, when it closes, is
    /// moved to an allocation of exactly its size if it has room to spare
    /// (see `exact`). Growing from empty would leave a one-entry container
    /// with room for four, and many small containers are what a document
    /// needs to take the most memory per byte of text.
    fn build(&mut self, stack: &mut Vec<Frame>) -> Result<Value, CodecError> {
        // What to name in the error if no value starts here.
        let mut expected = VALUE;
        loop {
            self.skip_trivia()?;
            let mut value = match self.peek() {
                Some(b'[') => {
                    self.check_depth(stack.len())?;
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        Value::Array(Vec::new())
                    } else {
                        if self.peek().is_some() {
                            self.check_room(0)?;
                        }
                        stack.push(Frame::Array(Vec::with_capacity(1)));
                        expected = ARRAY_ITEM;
                        continue;
                    }
                }
                Some(b'{') => {
                    self.check_depth(stack.len())?;
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        Value::Object(Vec::new())
                    } else {
                        let key = self.key(0, KEY_OR_CLOSE)?;
                        stack.push(Frame::Object {
                            members: Vec::with_capacity(1),
                            key,
                        });
                        expected = VALUE;
                        continue;
                    }
                }
                Some(b'"') => Value::String(self.string()?),
                Some(b'-' | b'0'..=b'9') => Value::Number(self.number()?),
                Some(b'n') => self.literal("null", Value::Null)?,
                Some(b't') => self.literal("true", Value::Bool(true))?,
                Some(b'f') => self.literal("false", Value::Bool(false))?,
                _ => return Err(self.unexpected(self.pos, expected)),
            };

            // `value` is complete: add it to the container it is in.
            loop {
                match stack.last_mut() {
                    None => return Ok(value),
                    Some(Frame::Array(items)) => {
                        items.push(value);
                        match self.after_entry(b']', ARRAY_NEXT)? {
                            After::Entry => {
                                expected = if self.jsonc { ARRAY_ITEM } else { VALUE };
                                if self.peek().is_some() {
                                    self.check_room(items.len())?;
                                }
                                break;
                            }
                            After::Close => {
                                value = Value::Array(exact(std::mem::take(items)));
                                stack.pop();
                            }
                        }
                    }
                    Some(Frame::Object { members, key }) => {
                        members.push((std::mem::take(key), value));
                        match self.after_entry(b'}', OBJECT_NEXT)? {
                            After::Entry => {
                                let expected_key = if self.jsonc { KEY_OR_CLOSE } else { KEY };
                                *key = self.key(members.len(), expected_key)?;
                                expected = VALUE;
                                break;
                            }
                            After::Close => {
                                value = Value::Object(exact(std::mem::take(members)));
                                stack.pop();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Reads what follows an entry of a container that `close` ends: a
    /// comma (leaving the position at the next entry), or `close`. In
    /// JSONC, a comma right before `close` is a trailing comma, and the
    /// container ends.
    fn after_entry(&mut self, close: u8, next: &'static str) -> Result<After, CodecError> {
        self.skip_trivia()?;
        match self.peek() {
            Some(b',') => {
                let comma = self.pos;
                self.pos += 1;
                self.skip_trivia()?;
                if self.peek() == Some(close) {
                    if !self.jsonc {
                        return Err(self.error(comma, ErrorKind::TrailingCommaNotAllowed));
                    }
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

    /// Reads an object member's key and the colon after it. `len` is how
    /// many members the object has so far; `expected` is what to name if
    /// there is no key here.
    ///
    /// A key may repeat one before it: RFC 8259 allows that, and the object
    /// keeps every member.
    fn key(&mut self, len: usize, expected: &'static str) -> Result<String, CodecError> {
        if self.peek() != Some(b'"') {
            return Err(self.unexpected(self.pos, expected));
        }
        self.check_room(len)?;
        let key = self.string()?;
        self.skip_trivia()?;
        self.expect(b':', COLON)?;
        Ok(key)
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, CodecError> {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        if !rest.starts_with(word.as_bytes()) {
            return Err(self.error(self.pos, ErrorKind::InvalidLiteral));
        }
        self.pos += word.len();
        Ok(value)
    }

    /// Fails if a container may not open inside `depth` others.
    fn check_depth(&self, depth: usize) -> Result<(), CodecError> {
        let limit = self.limits.max_depth;
        if depth >= limit {
            return Err(self.error(self.pos, ErrorKind::TooDeep { limit }));
        }
        Ok(())
    }

    /// Fails if a container that already has `len` entries may not get
    /// another.
    fn check_room(&self, len: usize) -> Result<(), CodecError> {
        let limit = self.limits.max_entries;
        if len >= limit {
            return Err(self.error(self.pos, ErrorKind::TooManyEntries { limit }));
        }
        Ok(())
    }

    /// Parses a string. `self.pos` is at its opening quote.
    fn string(&mut self) -> Result<String, CodecError> {
        let start = self.pos;
        self.pos += 1; // `"`
        let mut out = String::new();
        loop {
            // Copy everything up to the next byte that needs attention.
            let run_start = self.pos;
            while let Some(byte) = self.peek()
                && byte != b'"'
                && byte != b'\\'
                && byte >= 0x20
            {
                self.pos += 1;
            }
            let run = self.text.get(run_start..self.pos).unwrap_or_default();
            self.check_string_len(start, out.len(), run.len())?;
            out.push_str(run);

            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escaped = self.escape()?;
                    self.check_string_len(start, out.len(), escaped.len_utf8())?;
                    out.push(escaped);
                }
                Some(_) => return Err(self.error(self.pos, ErrorKind::ControlCharacter)),
                None => {
                    let kind = ErrorKind::UnexpectedEnd {
                        expected: STRING_CLOSE,
                    };
                    return Err(self.error(start, kind));
                }
            }
        }
    }

    /// Fails if a string of `len` bytes may not grow by `more`. Called
    /// before the bytes are copied, so an oversized string is never built.
    fn check_string_len(
        &self,
        string_start: usize,
        len: usize,
        more: usize,
    ) -> Result<(), CodecError> {
        let limit = self.limits.max_string_bytes;
        if len.checked_add(more).is_none_or(|total| total > limit) {
            return Err(self.error(string_start, ErrorKind::StringTooLong { limit }));
        }
        Ok(())
    }

    /// Parses an escape sequence. `self.pos` is at its backslash.
    fn escape(&mut self) -> Result<char, CodecError> {
        let start = self.pos;
        self.pos += 1; // `\`
        let Some(byte) = self.peek() else {
            return Err(self.error(
                start,
                ErrorKind::UnexpectedEnd {
                    expected: ESCAPE_REST,
                },
            ));
        };
        // Only an ASCII byte is consumed here; anything else is an error
        // below, before the position is used again.
        self.pos += 1;
        let simple = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(start),
            _ => return Err(self.error(start, ErrorKind::InvalidEscape)),
        };
        Ok(simple)
    }

    /// Parses the rest of a `\uXXXX` escape, and the low half that must
    /// follow a high surrogate. `self.pos` is just after the `u`.
    fn unicode_escape(&mut self, start: usize) -> Result<char, CodecError> {
        let first = self.hex4(start)?;
        let code = match first {
            0xD800..=0xDBFF => {
                // A high surrogate needs a low one right behind it.
                let low_start = self.pos;
                if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                    return Err(self.error(start, ErrorKind::LoneSurrogate));
                }
                self.pos += 2;
                let low = self.hex4(low_start)?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.error(start, ErrorKind::LoneSurrogate));
                }
                0x1_0000 + ((first - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(self.error(start, ErrorKind::LoneSurrogate)),
            code => code,
        };
        // Every code that is left is a valid `char`: the surrogate range
        // was handled above and a pair cannot exceed U+10FFFF.
        char::from_u32(code).ok_or_else(|| self.error(start, ErrorKind::InvalidEscape))
    }

    /// Reads four hex digits. `escape_start` is where to report a failure.
    fn hex4(&mut self, escape_start: usize) -> Result<u32, CodecError> {
        let digits = self.bytes.get(self.pos..self.pos + 4).unwrap_or_default();
        if digits.len() != 4 {
            let kind = match self.bytes.get(self.pos..) {
                // The text ends inside the escape.
                Some(rest) if rest.iter().all(u8::is_ascii_hexdigit) => ErrorKind::UnexpectedEnd {
                    expected: HEX_DIGITS,
                },
                _ => ErrorKind::InvalidEscape,
            };
            return Err(self.error(escape_start, kind));
        }
        let mut code = 0;
        for &digit in digits {
            let Some(value) = char::from(digit).to_digit(16) else {
                return Err(self.error(escape_start, ErrorKind::InvalidEscape));
            };
            code = code * 16 + value;
        }
        self.pos += 4;
        Ok(code)
    }

    /// Parses a number. `self.pos` is at its first character, a digit or a
    /// minus sign.
    fn number(&mut self) -> Result<Number, CodecError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.error(start, ErrorKind::LeadingZero));
                }
            }
            _ => self.digits()?,
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits()?;
        }

        let limit = self.limits.max_number_bytes;
        if self.pos - start > limit {
            return Err(self.error(start, ErrorKind::NumberTooLong { limit }));
        }
        let text = self.text.get(start..self.pos).unwrap_or_default();
        Number::from_lexeme(text, integer)
            .ok_or_else(|| self.error(start, ErrorKind::NumberOutOfRange))
    }

    /// Consumes one or more digits.
    fn digits(&mut self) -> Result<(), CodecError> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.unexpected(self.pos, DIGIT));
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
