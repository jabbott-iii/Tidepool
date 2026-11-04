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

//! Scalars: strings (also read for quoted keys), booleans, numbers and
//! date-times.

use super::super::datetime::days_in_month;
use super::super::{Date, Datetime, ErrorKind, Offset, Span, Time, Value, ValueKind};
use super::Parser;
use crate::CodecError;

// For a construct that the text ends inside. These errors are reported
// where the construct starts, which is more use than the end of the text.
pub(super) const BASIC_CLOSE: &str = "`\"` to close the string that starts here";
pub(super) const LITERAL_CLOSE: &str = "`'` to close the string that starts here";
pub(super) const MULTILINE_BASIC_CLOSE: &str = "`\"\"\"` to close the string that starts here";
pub(super) const MULTILINE_LITERAL_CLOSE: &str = "`'''` to close the string that starts here";
pub(super) const ESCAPE_REST: &str = "the rest of the escape sequence that starts here";
pub(super) const HEX_DIGITS: &str = "hex digits for the escape that starts here";
// A single-line string that a line break ends.
pub(super) const BASIC_CLOSE_ON_LINE: &str = "`\"` to close the string before the line ends";
pub(super) const LITERAL_CLOSE_ON_LINE: &str = "`'` to close the string before the line ends";

pub(super) const DIGIT: &str = "a digit";
pub(super) const HEX_DIGIT: &str = "a hex digit";
pub(super) const OCTAL_DIGIT: &str = "an octal digit";
pub(super) const BINARY_DIGIT: &str = "a binary digit";

// The fields of a date-time, as written.
pub(super) const MONTH: &str = "a two-digit month";
pub(super) const DAY_DASH: &str = "`-` before the day";
pub(super) const DAY: &str = "a two-digit day";
pub(super) const HOUR: &str = "a two-digit hour";
pub(super) const MINUTE_COLON: &str = "`:` after the hour";
pub(super) const MINUTE: &str = "a two-digit minute";
pub(super) const SECOND: &str = "two-digit seconds";
pub(super) const FRACTION: &str = "a digit of the fraction of a second";
pub(super) const OFFSET_HOUR: &str = "a two-digit hour for the offset";
pub(super) const OFFSET_COLON: &str = "`:` in the offset";
pub(super) const OFFSET_MINUTE: &str = "a two-digit minute for the offset";
// The ranges they must be in.
pub(super) const MONTH_RANGE: &str = "a month from 01 to 12";
pub(super) const DAY_RANGE: &str = "a day that the month has";
pub(super) const HOUR_RANGE: &str = "an hour from 00 to 23";
pub(super) const MINUTE_RANGE: &str = "a minute from 00 to 59";
pub(super) const SECOND_RANGE: &str = "seconds from 00 to 60";
pub(super) const OFFSET_HOUR_RANGE: &str = "an offset of 00 to 23 hours";
pub(super) const OFFSET_MINUTE_RANGE: &str = "an offset of 00 to 59 minutes";

/// Whether a string holds `byte` as it is: tab, printable ASCII other than
/// `quote` (and `\`, if the string has escapes), and every byte of a
/// non-ASCII character.
fn is_plain(byte: u8, quote: u8, escapes: bool) -> bool {
    byte == b'\t' || (byte >= 0x20 && byte != 0x7F && byte != quote && !(escapes && byte == b'\\'))
}

/// Whether `bytes` has `count` ASCII digits from `at` on.
fn digits_at(bytes: &[u8], at: usize, count: usize) -> bool {
    bytes
        .get(at..at + count)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
}

impl Parser<'_> {
    /// Reads a scalar value. `expected` is what to name if no value starts
    /// here.
    pub(super) fn scalar(&mut self, expected: &'static str) -> Result<Value, CodecError> {
        let start = self.pos;
        let rest = self.bytes.get(start..).unwrap_or_default();
        let kind = match self.peek() {
            Some(b'"') if rest.starts_with(b"\"\"\"") => {
                ValueKind::String(self.multiline_string(b'"')?)
            }
            Some(b'\'') if rest.starts_with(b"'''") => {
                ValueKind::String(self.multiline_string(b'\'')?)
            }
            Some(b'"') => ValueKind::String(self.basic_string()?),
            Some(b'\'') => ValueKind::String(self.literal_string()?),
            Some(b't') => self.word(start, "true", ValueKind::Boolean(true))?,
            Some(b'f') => self.word(start, "false", ValueKind::Boolean(false))?,
            Some(b'i') => self.special_float(start, "inf", f64::INFINITY)?,
            Some(b'n') => self.special_float(start, "nan", f64::NAN)?,
            Some(b'+' | b'-' | b'0'..=b'9') => self.number_or_datetime()?,
            _ => return Err(self.unexpected(start, expected)),
        };
        Ok(Value {
            kind,
            span: Span::new(start, self.pos),
        })
    }

    /// Consumes `word` at the current position, which is `kind`, or fails
    /// with [`ErrorKind::InvalidLiteral`] at `start`.
    fn word(&mut self, start: usize, word: &str, kind: ValueKind) -> Result<ValueKind, CodecError> {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        if !rest.starts_with(word.as_bytes()) {
            return Err(self.error(start, ErrorKind::InvalidLiteral));
        }
        self.pos += word.len();
        Ok(kind)
    }

    /// Consumes `inf` or `nan` (`word`) at the current position, after any
    /// sign, as the float `value`. Like any number, it counts against
    /// [`Limits::max_number_bytes`] from `start`, its sign included.
    ///
    /// [`Limits::max_number_bytes`]: crate::Limits::max_number_bytes
    fn special_float(
        &mut self,
        start: usize,
        word: &str,
        value: f64,
    ) -> Result<ValueKind, CodecError> {
        let kind = self.word(start, word, ValueKind::Float(value))?;
        self.check_number_len(start)?;
        Ok(kind)
    }

    // ---------------------------------------------------------- strings

    /// Fails if a string of `len` bytes may not grow by `more`. Called
    /// before the bytes are copied, so an oversized string is never built.
    pub(super) fn check_string_len(
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

    /// Copies the bytes from the current position that the string holds as
    /// they are (see [`is_plain`]) to `out`.
    fn copy_plain(
        &mut self,
        out: &mut String,
        string_start: usize,
        quote: u8,
        escapes: bool,
    ) -> Result<(), CodecError> {
        let run_start = self.pos;
        while self
            .peek()
            .is_some_and(|byte| is_plain(byte, quote, escapes))
        {
            self.pos += 1;
        }
        let run = self.text.get(run_start..self.pos).unwrap_or_default();
        self.check_string_len(string_start, out.len(), run.len())?;
        out.push_str(run);
        Ok(())
    }

    /// Appends `count` copies of `c` to `out`.
    fn push_chars(
        &self,
        out: &mut String,
        string_start: usize,
        c: char,
        count: usize,
    ) -> Result<(), CodecError> {
        self.check_string_len(string_start, out.len(), c.len_utf8() * count)?;
        out.extend(std::iter::repeat_n(c, count));
        Ok(())
    }

    /// The error for `byte`, at the current position, which a single-line
    /// string cannot hold: a line break, which `close` should have come
    /// before, or a control character.
    fn line_string_error(&self, byte: u8, close: &'static str) -> CodecError {
        match byte {
            b'\n' => self.unexpected(self.pos, close),
            b'\r' if self.peek_at(1) == Some(b'\n') => self.unexpected(self.pos, close),
            _ => self.control_error(byte),
        }
    }

    /// The error for the control character `byte` at the current position,
    /// which a string cannot hold as it is. A CR here is not part of a CR
    /// LF.
    fn control_error(&self, byte: u8) -> CodecError {
        let kind = match byte {
            b'\r' => ErrorKind::BareCarriageReturn,
            _ => ErrorKind::ControlCharacter {
                found: char::from(byte),
            },
        };
        self.error(self.pos, kind)
    }

    /// Reads a basic string, `"..."`. `self.pos` is at its opening quote.
    pub(super) fn basic_string(&mut self) -> Result<String, CodecError> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            self.copy_plain(&mut out, start, b'"', true)?;
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    // Escapes grow the string as they go: give back the room
                    // it has to spare, since it is complete.
                    out.shrink_to_fit();
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escaped = self.escape()?;
                    self.push_chars(&mut out, start, escaped, 1)?;
                }
                Some(byte) => return Err(self.line_string_error(byte, BASIC_CLOSE_ON_LINE)),
                None => {
                    let kind = ErrorKind::UnexpectedEnd {
                        expected: BASIC_CLOSE,
                    };
                    return Err(self.error(start, kind));
                }
            }
        }
    }

    /// Reads a literal string, `'...'`. `self.pos` is at its opening quote.
    pub(super) fn literal_string(&mut self) -> Result<String, CodecError> {
        let start = self.pos;
        self.pos += 1;
        let run_start = self.pos;
        while self.peek().is_some_and(|byte| is_plain(byte, b'\'', false)) {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'\'') => {
                let run = self.text.get(run_start..self.pos).unwrap_or_default();
                self.check_string_len(start, 0, run.len())?;
                self.pos += 1;
                Ok(run.to_string())
            }
            Some(byte) => Err(self.line_string_error(byte, LITERAL_CLOSE_ON_LINE)),
            None => {
                let kind = ErrorKind::UnexpectedEnd {
                    expected: LITERAL_CLOSE,
                };
                Err(self.error(start, kind))
            }
        }
    }

    /// Reads a multi-line string: basic (`"""..."""`, with escapes) if
    /// `quote` is `"`, literal (`'''...'''`) if it is `'`. `self.pos` is at
    /// its first quote.
    ///
    /// A line break right after the opening quotes is dropped, and each CR
    /// LF becomes LF. One or two quotes may appear anywhere, also just
    /// before the closing three: a run of four or five quotes is one or two
    /// quotes and the end of the string.
    fn multiline_string(&mut self, quote: u8) -> Result<String, CodecError> {
        let basic = quote == b'"';
        let start = self.pos;
        self.pos += 3;
        match (self.peek(), self.peek_at(1)) {
            (Some(b'\n'), _) => self.pos += 1,
            (Some(b'\r'), Some(b'\n')) => self.pos += 2,
            _ => {}
        }
        let mut out = String::new();
        loop {
            self.copy_plain(&mut out, start, quote, basic)?;
            match self.peek() {
                Some(byte) if byte == quote => {
                    // Count no further than one quote past the most a run
                    // that ends the string can hold, so that a long run of
                    // quotes is not counted again and again.
                    let quotes = self
                        .bytes
                        .get(self.pos..)
                        .unwrap_or_default()
                        .iter()
                        .take(6)
                        .take_while(|&&byte| byte == quote)
                        .count();
                    let c = char::from(quote);
                    if quotes >= 3 {
                        // Any quote past the fifth is left for whoever reads
                        // what follows the string, and is an error there.
                        let kept = (quotes - 3).min(2);
                        self.push_chars(&mut out, start, c, kept)?;
                        self.pos += kept + 3;
                        out.shrink_to_fit();
                        return Ok(out);
                    }
                    self.push_chars(&mut out, start, c, quotes)?;
                    self.pos += quotes;
                }
                Some(b'\\') if basic => {
                    if !self.line_ending_backslash() {
                        let escaped = self.escape()?;
                        self.push_chars(&mut out, start, escaped, 1)?;
                    }
                }
                Some(b'\n') => {
                    self.push_chars(&mut out, start, '\n', 1)?;
                    self.pos += 1;
                }
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                    self.push_chars(&mut out, start, '\n', 1)?;
                    self.pos += 2;
                }
                Some(byte) => return Err(self.control_error(byte)),
                None => {
                    let expected = if basic {
                        MULTILINE_BASIC_CLOSE
                    } else {
                        MULTILINE_LITERAL_CLOSE
                    };
                    return Err(self.error(start, ErrorKind::UnexpectedEnd { expected }));
                }
            }
        }
    }

    /// At a backslash in a multi-line basic string: if nothing but
    /// whitespace follows it on its line, skips it, the line break, and all
    /// the whitespace and line breaks after it, and returns `true`.
    /// Otherwise leaves it, for [`Parser::escape`].
    fn line_ending_backslash(&mut self) -> bool {
        let mut at = self.pos + 1;
        while matches!(self.bytes.get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        match (self.bytes.get(at), self.bytes.get(at + 1)) {
            (Some(b'\n'), _) | (Some(b'\r'), Some(b'\n')) => {}
            _ => return false,
        }
        self.pos = at;
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n') => self.pos += 1,
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => self.pos += 2,
                // A CR alone is an error, which the string reports next.
                _ => return true,
            }
        }
    }

    /// Reads an escape sequence. `self.pos` is at its backslash.
    fn escape(&mut self) -> Result<char, CodecError> {
        let start = self.pos;
        self.pos += 1; // `\`
        let Some(byte) = self.peek() else {
            let kind = ErrorKind::UnexpectedEnd {
                expected: ESCAPE_REST,
            };
            return Err(self.error(start, kind));
        };
        // Only an ASCII byte is consumed here; anything else is an error
        // below, before the position is used again.
        self.pos += 1;
        let simple = match byte {
            b'b' => '\u{8}',
            b't' => '\t',
            b'n' => '\n',
            b'f' => '\u{c}',
            b'r' => '\r',
            b'e' => '\u{1b}',
            b'"' => '"',
            b'\\' => '\\',
            b'x' => return self.hex_escape(start, 2),
            b'u' => return self.hex_escape(start, 4),
            b'U' => return self.hex_escape(start, 8),
            _ => return Err(self.error(start, ErrorKind::InvalidEscape)),
        };
        Ok(simple)
    }

    /// Reads the `count` hex digits of an escape that starts at
    /// `escape_start`, and the character they name.
    fn hex_escape(&mut self, escape_start: usize, count: usize) -> Result<char, CodecError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + count)
            .unwrap_or_default();
        if digits.len() != count {
            let kind = match self.bytes.get(self.pos..) {
                // The text ends inside the escape.
                Some(rest) if rest.iter().all(u8::is_ascii_hexdigit) => ErrorKind::UnexpectedEnd {
                    expected: HEX_DIGITS,
                },
                _ => ErrorKind::InvalidEscape,
            };
            return Err(self.error(escape_start, kind));
        }
        let mut code: u32 = 0;
        for &digit in digits {
            let Some(value) = char::from(digit).to_digit(16) else {
                return Err(self.error(escape_start, ErrorKind::InvalidEscape));
            };
            // At most eight digits, so at most 0xFFFF_FFFF: no overflow.
            code = code * 16 + value;
        }
        self.pos += count;
        char::from_u32(code)
            .ok_or_else(|| self.error(escape_start, ErrorKind::InvalidUnicodeEscape))
    }

    // ---------------------------------------------- numbers, date-times

    /// Reads a number or date-time, which starts with a sign or a digit.
    /// Four digits and `-` start a date; two digits and `:` start a time.
    fn number_or_datetime(&mut self) -> Result<ValueKind, CodecError> {
        let start = self.pos;
        let kind = if digits_at(self.bytes, start, 4) && self.bytes.get(start + 4) == Some(&b'-') {
            ValueKind::Datetime(self.datetime()?)
        } else if digits_at(self.bytes, start, 2) && self.bytes.get(start + 2) == Some(&b':') {
            ValueKind::Datetime(Datetime::LocalTime(self.time()?))
        } else {
            return self.number();
        };
        self.check_number_len(start)?;
        Ok(kind)
    }

    /// Fails if the number or date-time that starts at `start` and ends at
    /// the current position is longer than [`Limits::max_number_bytes`].
    ///
    /// [`Limits::max_number_bytes`]: crate::Limits::max_number_bytes
    fn check_number_len(&self, start: usize) -> Result<(), CodecError> {
        let limit = self.limits.max_number_bytes;
        if self.pos - start > limit {
            return Err(self.error(start, ErrorKind::NumberTooLong { limit }));
        }
        Ok(())
    }

    /// Reads a date-time that starts with a date: a local date, or a local
    /// or offset date-time.
    fn datetime(&mut self) -> Result<Datetime, CodecError> {
        let date = self.date()?;
        // A space separates a time only if a time follows it; otherwise it
        // ends the date.
        let time_follows = match self.peek() {
            Some(b'T' | b't') => true,
            Some(b' ') => digits_at(self.bytes, self.pos + 1, 2) && self.peek_at(3) == Some(b':'),
            _ => false,
        };
        if !time_follows {
            return Ok(Datetime::LocalDate(date));
        }
        self.pos += 1;
        let time = self.time()?;
        let offset = match self.peek() {
            Some(b'Z' | b'z') => {
                self.pos += 1;
                Offset::Utc
            }
            Some(sign @ (b'+' | b'-')) => {
                self.pos += 1;
                self.offset(sign == b'-')?
            }
            _ => return Ok(Datetime::LocalDatetime { date, time }),
        };
        Ok(Datetime::OffsetDatetime { date, time, offset })
    }

    /// Reads two digits, the field of a date or time that `expected`
    /// names.
    fn two_digits(&mut self, expected: &'static str) -> Result<u8, CodecError> {
        let mut value = 0;
        for _ in 0..2 {
            match self.peek() {
                Some(digit @ b'0'..=b'9') => {
                    value = value * 10 + (digit - b'0');
                    self.pos += 1;
                }
                _ => return Err(self.unexpected(self.pos, expected)),
            }
        }
        Ok(value)
    }

    /// Fails with [`ErrorKind::InvalidDatetime`] at `at` unless `ok`.
    fn check_field(&self, ok: bool, at: usize, expected: &'static str) -> Result<(), CodecError> {
        if ok {
            Ok(())
        } else {
            Err(self.error(at, ErrorKind::InvalidDatetime { expected }))
        }
    }

    /// Reads `YYYY-MM-DD`. The four digits and `-` are known to be there.
    fn date(&mut self) -> Result<Date, CodecError> {
        let year = self
            .bytes
            .get(self.pos..self.pos + 4)
            .unwrap_or_default()
            .iter()
            .fold(0u16, |year, &digit| year * 10 + u16::from(digit - b'0'));
        self.pos += 5; // `YYYY-`
        let month_at = self.pos;
        let month = self.two_digits(MONTH)?;
        self.check_field((1..=12).contains(&month), month_at, MONTH_RANGE)?;
        self.expect(b'-', DAY_DASH)?;
        let day_at = self.pos;
        let day = self.two_digits(DAY)?;
        let days = days_in_month(year, month);
        self.check_field((1..=days).contains(&day), day_at, DAY_RANGE)?;
        Ok(Date { year, month, day })
    }

    /// Reads `HH:MM`, then `:SS` and a fraction of a second if they are
    /// there.
    fn time(&mut self) -> Result<Time, CodecError> {
        let hour_at = self.pos;
        let hour = self.two_digits(HOUR)?;
        self.check_field(hour <= 23, hour_at, HOUR_RANGE)?;
        self.expect(b':', MINUTE_COLON)?;
        let minute_at = self.pos;
        let minute = self.two_digits(MINUTE)?;
        self.check_field(minute <= 59, minute_at, MINUTE_RANGE)?;
        let mut time = Time {
            hour,
            minute,
            second: 0,
            nanosecond: 0,
        };
        if self.peek() != Some(b':') {
            return Ok(time);
        }
        self.pos += 1;
        let second_at = self.pos;
        time.second = self.two_digits(SECOND)?;
        self.check_field(time.second <= 60, second_at, SECOND_RANGE)?;
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.unexpected(self.pos, FRACTION));
            }
            // Nine digits are nanoseconds; any after them are dropped.
            let mut digits = 0;
            while let Some(digit @ b'0'..=b'9') = self.peek() {
                if digits < 9 {
                    time.nanosecond = time.nanosecond * 10 + u32::from(digit - b'0');
                    digits += 1;
                }
                self.pos += 1;
            }
            for _ in digits..9 {
                time.nanosecond *= 10;
            }
        }
        Ok(time)
    }

    /// Reads the `HH:MM` of an offset, after its sign.
    fn offset(&mut self, negative: bool) -> Result<Offset, CodecError> {
        let hours_at = self.pos;
        let hours = self.two_digits(OFFSET_HOUR)?;
        self.check_field(hours <= 23, hours_at, OFFSET_HOUR_RANGE)?;
        self.expect(b':', OFFSET_COLON)?;
        let minutes_at = self.pos;
        let minutes = self.two_digits(OFFSET_MINUTE)?;
        self.check_field(minutes <= 59, minutes_at, OFFSET_MINUTE_RANGE)?;
        let total = i16::from(hours) * 60 + i16::from(minutes);
        Ok(Offset::Minutes(if negative { -total } else { total }))
    }

    /// Reads an integer or float: decimal with an optional sign, `inf` or
    /// `nan` with an optional sign, or `0x`, `0o` or `0b` and digits.
    fn number(&mut self) -> Result<ValueKind, CodecError> {
        let start = self.pos;
        let sign = self.peek().filter(|&byte| byte == b'+' || byte == b'-');
        if sign.is_some() {
            self.pos += 1;
        }
        let negative = sign == Some(b'-');
        match self.peek() {
            Some(b'i') => {
                let infinity = if negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                return self.special_float(start, "inf", infinity);
            }
            Some(b'n') => {
                // `-nan` keeps its sign bit, for what it is worth.
                let nan = if negative { -f64::NAN } else { f64::NAN };
                return self.special_float(start, "nan", nan);
            }
            _ => {}
        }
        if sign.is_none() && self.peek() == Some(b'0') {
            match self.peek_at(1) {
                Some(b'x') => return self.based(start, 16, HEX_DIGIT),
                Some(b'o') => return self.based(start, 8, OCTAL_DIGIT),
                Some(b'b') => return self.based(start, 2, BINARY_DIGIT),
                _ => {}
            }
        }
        self.decimal_digits(start, false)?;
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.pos += 1;
            self.decimal_digits(start, true)?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.decimal_digits(start, true)?;
        }
        self.check_number_len(start)?;
        let text = self.text.get(start..self.pos).unwrap_or_default();
        if float {
            // The grammar above is a subset of what `f64::from_str` reads,
            // once the underscores are gone, and it rounds correctly.
            let digits: String = text.chars().filter(|&c| c != '_').collect();
            match digits.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(ValueKind::Float(value)),
                _ => Err(self.error(start, ErrorKind::FloatOutOfRange)),
            }
        } else {
            decimal_integer(text)
                .map(ValueKind::Integer)
                .ok_or_else(|| self.error(start, ErrorKind::IntegerOutOfRange))
        }
    }

    /// Consumes one or more decimal digits with single underscores between
    /// them. Unless `leading_zero_ok`, a first `0` must stand alone.
    /// `number_start` is where to report leading zeros.
    fn decimal_digits(
        &mut self,
        number_start: usize,
        leading_zero_ok: bool,
    ) -> Result<(), CodecError> {
        match self.peek() {
            Some(b'0'..=b'9') => {}
            Some(b'_') => return Err(self.error(self.pos, ErrorKind::MisplacedUnderscore)),
            _ => return Err(self.unexpected(self.pos, DIGIT)),
        }
        let first = self.peek();
        self.pos += 1;
        if !leading_zero_ok
            && first == Some(b'0')
            && matches!(self.peek(), Some(b'0'..=b'9' | b'_'))
        {
            return Err(self.error(number_start, ErrorKind::LeadingZero));
        }
        self.digits_and_underscores(|byte| byte.is_ascii_digit())
    }

    /// Consumes digits (as `is_digit` says) and underscores, each of which
    /// must be followed by a digit. The digit before is already consumed.
    fn digits_and_underscores(&mut self, is_digit: impl Fn(u8) -> bool) -> Result<(), CodecError> {
        loop {
            match self.peek() {
                Some(byte) if is_digit(byte) => self.pos += 1,
                Some(b'_') => {
                    if !self.peek_at(1).is_some_and(&is_digit) {
                        return Err(self.error(self.pos, ErrorKind::MisplacedUnderscore));
                    }
                    self.pos += 2;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Reads an integer in `radix` (16, 8 or 2) after its `0x`, `0o` or
    /// `0b`, at `start`. `expected` names the radix's digits.
    fn based(
        &mut self,
        start: usize,
        radix: u32,
        expected: &'static str,
    ) -> Result<ValueKind, CodecError> {
        self.pos += 2;
        let is_digit = |byte: u8| char::from(byte).is_digit(radix);
        match self.peek() {
            Some(byte) if is_digit(byte) => self.pos += 1,
            Some(b'_') => return Err(self.error(self.pos, ErrorKind::MisplacedUnderscore)),
            _ => return Err(self.unexpected(self.pos, expected)),
        }
        self.digits_and_underscores(is_digit)?;
        self.check_number_len(start)?;
        let digits = self.text.get(start + 2..self.pos).unwrap_or_default();
        let mut value: i64 = 0;
        for digit in digits.chars().filter_map(|c| c.to_digit(radix)) {
            value = value
                .checked_mul(i64::from(radix))
                .and_then(|value| value.checked_add(i64::from(digit)))
                .ok_or_else(|| self.error(start, ErrorKind::IntegerOutOfRange))?;
        }
        Ok(ValueKind::Integer(value))
    }
}

/// The value of a decimal integer's text: an optional sign, digits and
/// underscores. `None` if it does not fit an `i64`. Negative numbers are
/// built below zero, so that -2^63 fits.
fn decimal_integer(text: &str) -> Option<i64> {
    let negative = text.starts_with('-');
    let mut value: i64 = 0;
    for digit in text.chars().filter_map(|c| c.to_digit(10)) {
        let digit = i64::from(digit);
        value = value.checked_mul(10)?;
        value = if negative {
            value.checked_sub(digit)?
        } else {
            value.checked_add(digit)?
        };
    }
    Some(value)
}
