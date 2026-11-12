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

//! The documentation of a source file, read for the code blocks that
//! rustdoc compiles as doc tests, so that `unsafe-audit` can fail on
//! `unsafe` in them outside `tidepool-sys` and `tidepool-platform`
//! (ADR-0037).
//!
//! [`gather`] collects each documented item's pieces of documentation, in
//! source order, as rustdoc joins them:
//!
//! - doc comments: `///`, `//!`, `/** */` and `/*! */`, a block comment
//!   with its `*` decoration removed as rustdoc removes it;
//! - doc attributes, `#[doc = ...]` and `#![doc = ...]`, also under
//!   `cfg_attr`: a string literal, raw or not, with its escapes decoded, or
//!   `include_str!` of a plain string literal, whose file the audit reads
//!   later, once it knows the package that holds this file. A `doc =` value
//!   of any other form (a macro's `$x`, `concat!`, `env!`) is unreadable.
//!
//! The pieces of one item are those with nothing but attributes between
//! them, and an item's outer docs join the inner docs of the module they
//! open (`/// a` then `mod m { //! b`), as rustdoc joins them.
//!
//! [`code_blocks`] unindents an item's text as rustdoc does, then finds its
//! code blocks as CommonMark does, closely enough: every fenced block (three
//! or more backticks or tildes, whatever the info string, `ignore` and
//! `text` too) and every indented one. It follows list items, the columns
//! their content starts at, paragraphs and their lazy continuation lines
//! (which an indented block cannot interrupt), and block quotes, so that a
//! list's continuation lines are not read as code, and a fence ends where
//! its list item or quote does. As rustdoc's parser does, its structure
//! sees only spaces and tabs (a closing fence, only spaces), lines break at
//! CR LF, CR and LF, an empty list item closes at a blank line, a setext
//! underline ends a paragraph, and a footnote definition is a container.
//!
//! It errs toward finding code, but it is not rustdoc's parser, and it is
//! a second layer (ADR-0038): `unsafe-audit` bans at its source the macro
//! shape that would let a misread doc test's `unsafe` past the compiler.
//! Not modelled, as `docs/04-quality.md` writes down: HTML blocks; block
//! quotes inside list items, list items that outlive their quote, a quote
//! that interrupts a paragraph, a tab after `>`; outer docs joined with the
//! inner docs of a fn, impl or trait, or of the file of a `mod name;`.

use std::mem;

use super::string_value;
use crate::lexer::{CommentKind, Doc, Lexed, Token, TokenKind};

/// Where a line of documentation is, for messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// This line of the `.rs` file.
    Here(usize),
    /// Line `line` of the file shown as `file`, which a `doc =
    /// include_str!(...)` on line `by` of the `.rs` file names.
    Included {
        /// The included file, as messages show it.
        file: String,
        /// The line in it.
        line: usize,
        /// The line of the attribute that includes it.
        by: usize,
    },
}

/// One line of documentation text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DocLine {
    /// The text, without its line break.
    pub(super) text: String,
    /// Where it is.
    pub(super) at: Place,
}

/// One piece of an item's documentation.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Fragment {
    /// The text of a doc comment (`sugared`, as rustdoc calls it) or of a
    /// doc attribute's string.
    Text {
        /// Its lines.
        lines: Vec<DocLine>,
        /// Whether it is a doc comment.
        sugared: bool,
    },
    /// `doc = include_str!(path)`, on this line.
    Include {
        /// The path, as the string literal gives it.
        path: String,
        /// The line of the attribute.
        line: usize,
    },
}

/// What [`gather`] finds in one file.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Docs {
    /// Each documented item's pieces, in source order.
    pub(super) items: Vec<Vec<Fragment>>,
    /// The lines of the `doc =` values that cannot be read.
    pub(super) unreadable: Vec<usize>,
}

/// One piece of documentation found in the file, before grouping.
struct Piece {
    /// Where it starts and ends, in bytes.
    start: usize,
    end: usize,
    /// Whether it documents the item it is in (`//!`, `#![doc]`).
    inner: bool,
    /// The fragment, or the line of a value that cannot be read.
    fragment: Result<Fragment, usize>,
}

/// An attribute, `#[...]` or `#![...]`: its first and last tokens.
struct Attribute {
    first: usize,
    last: usize,
    inner: bool,
}

/// Collects the documentation of a lexed file ([`Docs`]).
pub(super) fn gather(lexed: &Lexed<'_>) -> Docs {
    let tokens = &lexed.tokens;
    let attributes = attributes(tokens);
    let mut in_attribute = vec![false; tokens.len()];
    for attribute in &attributes {
        if let Some(flags) = in_attribute.get_mut(attribute.first..=attribute.last) {
            flags.fill(true);
        }
    }

    let mut pieces = Vec::new();
    for comment in &lexed.comments {
        if comment.doc == Doc::No {
            continue;
        }
        let body = comment.body();
        let lines = match comment.kind {
            CommentKind::Line => vec![DocLine {
                text: body.to_string(),
                at: Place::Here(comment.line),
            }],
            CommentKind::Block => beautify(body)
                .into_iter()
                .map(|(index, text)| DocLine {
                    text,
                    at: Place::Here(comment.line + index),
                })
                .collect(),
        };
        pieces.push(Piece {
            start: comment.offset,
            end: comment.offset + comment.text.len(),
            inner: comment.doc == Doc::Inner,
            fragment: Ok(Fragment::Text {
                lines,
                sugared: true,
            }),
        });
    }
    for attribute in &attributes {
        for at in attribute.first..=attribute.last {
            let (Some(name), Some(next)) = (tokens.get(at), tokens.get(at + 1)) else {
                continue;
            };
            if !name.is_name("doc") || !(next.is_punct("=") || next.is_punct("$")) {
                continue;
            }
            pieces.push(Piece {
                start: name.offset,
                end: next.end(),
                inner: attribute.inner,
                // `doc $...`, which a macro completes, cannot be read.
                fragment: next
                    .is_punct("=")
                    .then(|| doc_value(tokens, at + 2))
                    .flatten()
                    .ok_or(name.line),
            });
        }
    }
    // `doc = "..."` outside an attribute, as in a macro's arguments that it
    // turns into one (`m!(doc = "...")` for `#[$m]`): read as an item of its
    // own, when the value is a string.
    for (at, name) in tokens.iter().enumerate() {
        if in_attribute.get(at).copied().unwrap_or(true) || !name.is_name("doc") {
            continue;
        }
        let (Some(equals), Some(value)) = (tokens.get(at + 1), tokens.get(at + 2)) else {
            continue;
        };
        if !equals.is_punct("=") || value.kind != TokenKind::Literal {
            continue;
        }
        let Some(lines) = decode(value.text, value.line) else {
            continue;
        };
        pieces.push(Piece {
            start: name.offset,
            end: value.end(),
            inner: false,
            fragment: Ok(Fragment::Text {
                lines: lines
                    .into_iter()
                    .map(|(text, line)| DocLine {
                        text,
                        at: Place::Here(line),
                    })
                    .collect(),
                sugared: false,
            }),
        });
    }
    pieces.sort_by_key(|piece| piece.start);

    let mut docs = Docs::default();
    let mut item = Vec::new();
    let mut previous: Option<(usize, bool)> = None;
    for piece in pieces {
        let joins = previous.is_some_and(|(end, inner)| {
            let first = tokens.partition_point(|token| token.offset < end);
            let last = tokens.partition_point(|token| token.offset < piece.start);
            let between = tokens.get(first..last).unwrap_or(&[]);
            let flags = in_attribute.get(first..last).unwrap_or(&[]);
            let outside: Vec<&Token<'_>> = between
                .iter()
                .zip(flags)
                .filter(|(_, in_attribute)| !**in_attribute)
                .map(|(token, _)| token)
                .collect();
            (inner == piece.inner && outside.is_empty())
                || (!inner && piece.inner && opens_module(&outside))
        });
        if !joins && !item.is_empty() {
            docs.items.push(mem::take(&mut item));
        }
        match piece.fragment {
            Ok(fragment) => item.push(fragment),
            Err(line) => docs.unreadable.push(line),
        }
        previous = Some((piece.end, piece.inner));
    }
    if !item.is_empty() {
        docs.items.push(item);
    }
    docs
}

/// Every attribute of `tokens`, outermost only. One that does not close
/// runs to the end of the file.
fn attributes(tokens: &[Token<'_>]) -> Vec<Attribute> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(token) = tokens.get(at) {
        let is =
            |ahead: usize, punct: &str| tokens.get(at + ahead).is_some_and(|t| t.is_punct(punct));
        let open = if !token.is_punct("#") {
            None
        } else if is(1, "!") && is(2, "[") {
            Some((at + 2, true))
        } else if is(1, "[") {
            Some((at + 1, false))
        } else {
            None
        };
        let Some((open, inner)) = open else {
            at += 1;
            continue;
        };
        let mut depth = 0_usize;
        let mut last = tokens.len().saturating_sub(1);
        for (index, token) in tokens.iter().enumerate().skip(open) {
            if token.is_punct("[") {
                depth += 1;
            } else if token.is_punct("]") {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    last = index;
                    break;
                }
            }
        }
        found.push(Attribute {
            first: at,
            last,
            inner,
        });
        at = last + 1;
    }
    found
}

/// Whether `tokens`, the code between an item's outer docs and the inner
/// docs after them, opens a module that the inner docs document:
/// `mod name {`, perhaps after `pub` or `pub(...)`.
fn opens_module(tokens: &[&Token<'_>]) -> bool {
    let mut at = 0;
    if tokens.first().is_some_and(|token| token.is_word("pub")) {
        at = 1;
        if tokens.get(1).is_some_and(|token| token.is_punct("(")) {
            let Some(close) = tokens.iter().position(|token| token.is_punct(")")) else {
                return false;
            };
            at = close + 1;
        }
    }
    matches!(
        tokens.get(at..),
        Some([module, name, open]) if module.is_word("mod") && name.is_ident() && open.is_punct("{")
    )
}

/// The fragment that the `doc =` value at `at` gives, if the audit can read
/// it: a string literal, or `include_str!` of a plain string literal
/// (perhaps through `std::` or `core::`), which then ends the attribute or
/// its part of a `cfg_attr` (`]`, `,` or `)`).
fn doc_value(tokens: &[Token<'_>], at: usize) -> Option<Fragment> {
    let value = tokens.get(at)?;
    let (fragment, next) = if value.kind == TokenKind::Literal {
        let lines = decode(value.text, value.line)?
            .into_iter()
            .map(|(text, line)| DocLine {
                text,
                at: Place::Here(line),
            })
            .collect();
        (
            Fragment::Text {
                lines,
                sugared: false,
            },
            at + 1,
        )
    } else {
        let is = |index: usize, punct: &str| tokens.get(index).is_some_and(|t| t.is_punct(punct));
        let mut at = at;
        if is(at, ":") && is(at + 1, ":") {
            at += 2;
        }
        if tokens
            .get(at)
            .is_some_and(|token| token.is_name("std") || token.is_name("core"))
            && is(at + 1, ":")
            && is(at + 2, ":")
        {
            at += 3;
        }
        if !tokens.get(at)?.is_name("include_str") || !is(at + 1, "!") {
            return None;
        }
        let close = match tokens.get(at + 2)?.text {
            "(" => ")",
            "[" => "]",
            "{" => "}",
            _ => return None,
        };
        let literal = tokens.get(at + 3)?;
        if literal.kind != TokenKind::Literal || !is(at + 4, close) {
            return None;
        }
        (
            Fragment::Include {
                path: string_value(literal.text)?.to_string(),
                line: literal.line,
            },
            at + 5,
        )
    };
    let end = tokens.get(next)?;
    (end.is_punct("]") || end.is_punct(",") || end.is_punct(")")).then_some(fragment)
}

/// The lines of the string literal `literal`, which starts on line `line`,
/// with its escapes decoded, each with the line it starts on. `None` for a
/// literal that is not a string, raw or not (a byte or C string), or has an
/// escape rustc rejects.
fn decode(literal: &str, line: usize) -> Option<Vec<(String, usize)>> {
    let mut out = LineSplitter {
        done: Vec::new(),
        current: String::new(),
        start: line,
        line,
        after_cr: false,
    };
    if let Some(raw) = literal.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let quoted = raw.get(hashes..raw.len().checked_sub(hashes)?)?;
        for character in quoted.strip_prefix('"')?.strip_suffix('"')?.chars() {
            out.push(character, true);
        }
        return Some(out.finish());
    }
    let mut characters = literal
        .strip_prefix('"')?
        .strip_suffix('"')?
        .chars()
        .peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character, true);
            continue;
        }
        let decoded = match characters.next()? {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            'x' => {
                let high = characters.next()?.to_digit(8)?;
                let low = characters.next()?.to_digit(16)?;
                char::from_u32(high * 16 + low)?
            }
            'u' => {
                if characters.next()? != '{' {
                    return None;
                }
                let mut value = 0_u32;
                let mut digits = 0;
                loop {
                    match characters.next()? {
                        '}' => break,
                        '_' => {}
                        digit => {
                            value = value.checked_mul(16)?.checked_add(digit.to_digit(16)?)?;
                            digits += 1;
                        }
                    }
                }
                if !(1..=6).contains(&digits) {
                    return None;
                }
                char::from_u32(value)?
            }
            // A line continuation: the line break and the white space after
            // it are skipped.
            '\n' | '\r' => {
                out.line += 1;
                while let Some(&space @ (' ' | '\t' | '\n' | '\r')) = characters.peek() {
                    if space == '\n' {
                        out.line += 1;
                    }
                    characters.next();
                }
                // A `\r\n` counted its line once, at the `\n`.
                continue;
            }
            _ => return None,
        };
        out.push(decoded, false);
    }
    Some(out.finish())
}

/// Splits decoded text into lines at CR LF, CR and LF, as CommonMark does,
/// keeping the source line each starts on.
struct LineSplitter {
    done: Vec<(String, usize)>,
    current: String,
    /// The source line the current line started on.
    start: usize,
    /// The source line now.
    line: usize,
    /// Whether the last character was a CR, whose line break an LF after
    /// it completes.
    after_cr: bool,
}

impl LineSplitter {
    /// Adds `character`; `raw` is whether it is written as itself in the
    /// source, so that an LF moves to the next source line.
    fn push(&mut self, character: char, raw: bool) {
        let after_cr = mem::take(&mut self.after_cr);
        match character {
            '\n' if after_cr => {
                if raw {
                    self.line += 1;
                    self.start = self.line;
                }
            }
            '\n' | '\r' => {
                self.done.push((mem::take(&mut self.current), self.start));
                if raw && character == '\n' {
                    self.line += 1;
                }
                self.start = self.line;
                self.after_cr = character == '\r';
            }
            _ => self.current.push(character),
        }
    }

    fn finish(mut self) -> Vec<(String, usize)> {
        self.done.push((self.current, self.start));
        self.done
    }
}

/// The lines of `text`, split at CR LF, CR and LF, as CommonMark splits
/// them: for a file included as documentation.
pub(super) fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(['\r', '\n']) {
        lines.push(rest.get(..at).unwrap_or(""));
        let width = if rest.get(at..).is_some_and(|tail| tail.starts_with("\r\n")) {
            2
        } else {
            1
        };
        rest = rest.get(at + width..).unwrap_or("");
    }
    lines.push(rest);
    lines
}

/// The lines of a block doc comment's body, each with its index in the
/// body, with the `*` decoration removed as rustdoc's
/// `beautify_doc_string` removes it: a first line of only `*`, a last line
/// of only `*`, and a common prefix of white space then `*` (and a space
/// after it).
fn beautify(body: &str) -> Vec<(usize, String)> {
    if !body.contains('\n') {
        return vec![(0, body.to_string())];
    }
    // As `str::lines` reads it: no empty last line after a final LF.
    let mut all: Vec<(usize, &str)> = body.split('\n').enumerate().collect();
    if body.ends_with('\n') {
        all.pop();
    }
    let mut first = 0;
    let mut end = all.len();
    if all
        .first()
        .is_some_and(|(_, line)| line.chars().all(|c| c == '*'))
    {
        first += 1;
    }
    if end > first
        && all
            .get(end - 1)
            .is_some_and(|(_, line)| !line.is_empty() && line.chars().all(|c| c == '*'))
    {
        end -= 1;
    }
    let lines = all.get(first..end).unwrap_or(&[]);
    let prefix = horizontal_trim(lines);
    lines
        .iter()
        .map(|&(index, line)| {
            let mut text = line;
            if let Some(prefix) = &prefix
                && let Some(rest) = line.strip_prefix(prefix.as_str())
            {
                text = rest;
                if text == "*" || text.starts_with("* ") || text.starts_with("**") {
                    text = text.get(1..).unwrap_or("");
                }
            }
            (index, text.to_string())
        })
        .collect()
}

/// The common prefix of white space then `*` that rustdoc removes from a
/// block doc comment's lines, if they have one.
fn horizontal_trim(lines: &[(usize, &str)]) -> Option<String> {
    let mut start = usize::from(
        !lines
            .first()
            .is_some_and(|(_, line)| line.trim_start().starts_with('*')),
    );
    let mut end = lines.len();
    while start < end && lines.get(start).is_some_and(|(_, l)| l.trim().is_empty()) {
        start += 1;
    }
    while end > start && lines.get(end - 1).is_some_and(|(_, l)| l.trim().is_empty()) {
        end -= 1;
    }
    let lines = lines.get(start..end)?;
    let mut star = usize::MAX;
    for (_, line) in lines {
        for (column, character) in line.chars().enumerate() {
            if column > star || !matches!(character, '*' | ' ' | '\t') {
                return None;
            }
            if character == '*' {
                if star == usize::MAX {
                    star = column;
                } else if star != column {
                    return None;
                }
                break;
            }
        }
        if star >= line.len() {
            return None;
        }
    }
    Some(lines.first()?.1.get(..star)?.to_string())
}

/// The code blocks of one item's documentation: its fragments' lines, each
/// fragment with whether it is a doc comment, in order. Each block is its
/// lines, fenced or indented.
pub(super) fn code_blocks(fragments: Vec<(Vec<DocLine>, bool)>) -> Vec<Vec<DocLine>> {
    let mut markdown = Markdown::default();
    for line in unindent(fragments) {
        markdown.line(line);
    }
    markdown.finish()
}

/// The fragments' lines, unindented as rustdoc's `unindent_doc_fragments`
/// does: each loses the least indentation (spaces and tabs, one each) of
/// the item's lines that are not blank, a doc attribute's counting one more
/// when the item mixes doc comments and doc attributes. Blank lines are kept
/// as they are.
fn unindent(fragments: Vec<(Vec<DocLine>, bool)>) -> Vec<DocLine> {
    let mixed = fragments.windows(2).any(|pair| pair[0].1 != pair[1].1)
        && fragments.iter().any(|(_, sugared)| *sugared);
    let add = usize::from(mixed);
    let least = fragments
        .iter()
        .flat_map(|(lines, sugared)| {
            lines
                .iter()
                .filter(|line| !is_blank(&line.text))
                .map(move |line| leading(&line.text) + if *sugared { 0 } else { add })
        })
        .min();
    let mut out = Vec::new();
    for (lines, sugared) in fragments {
        let indent = match least {
            Some(least) if !sugared && least > 0 => least - add,
            Some(least) => least,
            None => 0,
        };
        for mut line in lines {
            if !is_blank(&line.text) {
                line.text = line.text.get(indent..).unwrap_or("").to_string();
            }
            out.push(line);
        }
    }
    out
}

/// How many spaces and tabs `text` starts with.
fn leading(text: &str) -> usize {
    text.chars().take_while(|c| matches!(c, ' ' | '\t')).count()
}

/// Whether `text` holds nothing but white space, as rustdoc's unindenting
/// sees it. Markdown's structure sees only spaces and tabs ([`is_space`]).
fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// Whether `text` holds nothing but spaces and tabs, the only white space
/// of Markdown's structure: a blank line, the end of a closing fence, what
/// follows a list marker or a heading's `#`.
fn is_space(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t'))
}

/// Whether `content` is a setext heading's underline: one or more `=`, or
/// one or more `-`, then only spaces and tabs.
fn is_setext_underline(content: &str) -> bool {
    let marks = content.trim_end_matches([' ', '\t']);
    !marks.is_empty() && (marks.bytes().all(|b| b == b'=') || marks.bytes().all(|b| b == b'-'))
}

/// If `content` starts a footnote definition, `[^label]:` with a label of
/// at least one character and no `]`: the width of that.
fn footnote_definition(content: &str) -> Option<usize> {
    let label = content.strip_prefix("[^")?;
    let end = label.find("]:")?;
    (end > 0 && !label.get(..end)?.contains(['[', ']'])).then_some(end + 4)
}

/// A CommonMark reader that keeps only what is code, closely enough.
#[derive(Default)]
struct Markdown {
    /// The columns where the content of the open list items starts,
    /// innermost last.
    items: Vec<usize>,
    /// Whether the last line was paragraph text, which the next line can
    /// continue (lazily, too) and an indented block cannot interrupt.
    paragraph: bool,
    /// Whether the last line opened an empty list item, which a blank line
    /// closes: an item begins with at most one blank line.
    empty: bool,
    /// The open fence, if any.
    fence: Option<Fence>,
    /// Whether `block` is an indented code block.
    indented: bool,
    /// The lines of the code block being read.
    block: Vec<DocLine>,
    blocks: Vec<Vec<DocLine>>,
}

/// An open fenced code block.
struct Fence {
    /// Its character and how many of it open it.
    marker: char,
    len: usize,
    /// How many list items and block quotes it is in.
    items: usize,
    quotes: usize,
}

/// A list item's marker.
struct Marker<'t> {
    /// Its width: `-`, or the digits and `.` or `)`.
    width: usize,
    /// What follows it on the line.
    rest: &'t str,
    /// Whether it can start a list that interrupts a paragraph: a bullet,
    /// or the number 1.
    may_interrupt: bool,
}

impl Markdown {
    fn line(&mut self, line: DocLine) {
        let text = line.text.clone();
        // In a fence, only the quotes it is in are markup.
        let limit = self.fence.as_ref().map_or(usize::MAX, |fence| fence.quotes);
        let (quotes, rest) = strip_quotes(&text, limit);
        if let Some(fence) = &self.fence {
            let (column, content) = indentation(rest, 0);
            let base = fence
                .items
                .checked_sub(1)
                .and_then(|index| self.items.get(index))
                .copied()
                .unwrap_or(0);
            let ended = quotes < fence.quotes || (!is_space(content) && column < base);
            if !ended {
                // A closing fence is not blank, so it is at `base` or past it.
                if closes(content, fence) && column - base <= 3 {
                    self.end_block();
                    self.fence = None;
                } else {
                    self.block.push(line);
                }
                return;
            }
            // Its list item or block quote has ended, and the fence with it.
            self.end_block();
            self.fence = None;
        }
        self.open(line, quotes, rest);
    }

    /// Reads a line outside a fence.
    fn open(&mut self, line: DocLine, quotes: usize, rest: &str) {
        let (mut column, mut content) = indentation(rest, 0);
        let empty = mem::take(&mut self.empty);
        if is_space(content) {
            self.paragraph = false;
            if empty {
                self.items.pop();
            }
            if self.indented {
                self.block.push(line);
            }
            return;
        }
        loop {
            // The open items this line is indented into, and the column its
            // content is measured from.
            let depth = self
                .items
                .iter()
                .take_while(|&&start| start <= column)
                .count();
            let base = depth
                .checked_sub(1)
                .and_then(|index| self.items.get(index))
                .copied()
                .unwrap_or(0);
            if column - base >= 4 {
                if self.paragraph {
                    return;
                }
                self.items.truncate(depth);
                if !self.indented {
                    self.end_block();
                    self.indented = true;
                }
                self.block.push(line);
                return;
            }
            if self.indented {
                self.end_block();
            }
            // A setext underline ends the paragraph above it in the same
            // container, as a heading.
            if self.paragraph && depth == self.items.len() && is_setext_underline(content) {
                self.paragraph = false;
                return;
            }
            if let Some((marker, len)) = fence_opener(content) {
                self.items.truncate(depth);
                self.fence = Some(Fence {
                    marker,
                    len,
                    items: depth,
                    quotes,
                });
                self.paragraph = false;
                return;
            }
            if is_thematic_break(content) || is_heading(content) {
                self.items.truncate(depth);
                self.paragraph = false;
                return;
            }
            if let Some(marker) = list_marker(content) {
                // A new list that would interrupt a paragraph must start with
                // a bullet or 1, and not be empty. A sibling of an open item
                // is no new list.
                let interrupts = self.paragraph && depth == self.items.len();
                let empty = is_space(marker.rest);
                if !interrupts || (marker.may_interrupt && !empty) {
                    self.items.truncate(depth);
                    let after = column + marker.width;
                    let (spaces, item) = indentation(marker.rest, after);
                    self.paragraph = false;
                    if is_space(item) {
                        self.items.push(after + 1);
                        self.empty = true;
                        return;
                    }
                    let spaces = spaces - after;
                    // Five spaces or more: the item starts with an indented
                    // block, one space past the marker.
                    self.items
                        .push(after + if spaces >= 5 { 1 } else { spaces });
                    column = after + spaces;
                    content = item;
                    continue;
                }
            }
            // A footnote definition is a container too: its content starts
            // after `]:` and goes on in lines four columns past its start.
            if let Some(width) = footnote_definition(content) {
                self.items.truncate(depth);
                self.items.push(column + 4);
                // What follows `]:` on its line opens a fence or a paragraph,
                // wherever it starts.
                let (_, item) = indentation(content.get(width..).unwrap_or(""), column + width);
                self.paragraph = false;
                if let Some((marker, len)) = fence_opener(item) {
                    self.fence = Some(Fence {
                        marker,
                        len,
                        items: self.items.len(),
                        quotes,
                    });
                } else if !is_space(item) {
                    self.paragraph = true;
                }
                return;
            }
            if !self.paragraph {
                self.items.truncate(depth);
                self.paragraph = true;
            }
            return;
        }
    }

    fn end_block(&mut self) {
        if !self.block.is_empty() {
            self.blocks.push(mem::take(&mut self.block));
        }
        self.indented = false;
    }

    fn finish(mut self) -> Vec<Vec<DocLine>> {
        self.end_block();
        self.blocks
    }
}

/// Strips up to `limit` block quote markers (`>` after at most three
/// columns, and a space after it) from `text`: how many, and the rest.
fn strip_quotes(text: &str, limit: usize) -> (usize, &str) {
    let mut rest = text;
    let mut quotes = 0;
    while quotes < limit {
        let (column, after) = indentation(rest, 0);
        match after.strip_prefix('>') {
            Some(inner) if column <= 3 => {
                rest = inner.strip_prefix(' ').unwrap_or(inner);
                quotes += 1;
            }
            _ => break,
        }
    }
    (quotes, rest)
}

/// The column `text` starts its content at, if it starts at `start`, with
/// tabs to the next multiple of four, and the content.
fn indentation(text: &str, start: usize) -> (usize, &str) {
    let mut column = start;
    for (index, character) in text.char_indices() {
        match character {
            ' ' => column += 1,
            '\t' => column += 4 - column % 4,
            _ => return (column, text.get(index..).unwrap_or("")),
        }
    }
    (column, "")
}

/// If `content` opens a fence: its character and length. A backtick fence's
/// info string has no backtick.
fn fence_opener(content: &str) -> Option<(char, usize)> {
    let marker = content.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = content.chars().take_while(|&c| c == marker).count();
    let info = content.get(len..)?;
    (len >= 3 && !(marker == '`' && info.contains('`'))).then_some((marker, len))
}

/// Whether `content` closes `fence`: as many of its character or more, then
/// only spaces. rustdoc's parser takes no tab there, nor other white space.
fn closes(content: &str, fence: &Fence) -> bool {
    let len = content.chars().take_while(|&c| c == fence.marker).count();
    len >= fence.len
        && content
            .get(len..)
            .is_some_and(|rest| rest.bytes().all(|b| b == b' '))
}

/// Whether `content` is a thematic break: three or more of one of `-`,
/// `*` and `_`, with spaces between.
fn is_thematic_break(content: &str) -> bool {
    let mut marks = content.chars().filter(|c| !matches!(c, ' ' | '\t'));
    let Some(first @ ('-' | '*' | '_')) = marks.next() else {
        return false;
    };
    let mut count = 1;
    for mark in marks {
        if mark != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// Whether `content` is an ATX heading: one to six `#`, then white space or
/// nothing.
fn is_heading(content: &str) -> bool {
    let hashes = content.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes)
        && content
            .get(hashes..)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// If `content` starts with a list item's marker (`-`, `+` or `*`, or up to
/// nine digits then `.` or `)`, followed by white space or nothing): the
/// marker.
fn list_marker(content: &str) -> Option<Marker<'_>> {
    let (width, may_interrupt) = match content.as_bytes().first()? {
        b'-' | b'+' | b'*' => (1, true),
        b'0'..=b'9' => {
            let digits = content.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 9 || !matches!(content.as_bytes().get(digits), Some(b'.' | b')')) {
                return None;
            }
            (digits + 1, content.get(..digits)?.parse::<u32>().ok()? == 1)
        }
        _ => return None,
    };
    let rest = content.get(width..)?;
    (rest.is_empty() || rest.starts_with([' ', '\t'])).then_some(Marker {
        width,
        rest,
        may_interrupt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    /// The code blocks of `text` as one item's doc comments (`///`), each
    /// block as its lines' text, after the `/// ` space.
    fn blocks(text: &str) -> Vec<Vec<String>> {
        let lines = text
            .lines()
            .enumerate()
            .map(|(index, line)| DocLine {
                text: format!(" {line}"),
                at: Place::Here(index + 1),
            })
            .collect();
        code_blocks(vec![(lines, true)])
            .into_iter()
            .map(|block| {
                block
                    .into_iter()
                    .map(|line| line.text.trim().to_string())
                    .collect()
            })
            .collect()
    }

    /// What [`gather`] finds in `source`.
    fn gathered(source: &str) -> Docs {
        gather(&lexer::lex(source))
    }

    /// The text of each item's fragments, `+` marking an include.
    fn texts(docs: &Docs) -> Vec<Vec<String>> {
        docs.items
            .iter()
            .map(|item| {
                item.iter()
                    .map(|fragment| match fragment {
                        Fragment::Text { lines, .. } => lines
                            .iter()
                            .map(|line| line.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Fragment::Include { path, .. } => format!("+{path}"),
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn fenced_blocks_are_code_whatever_their_info_string() {
        for (open, close) in [
            ("```", "```"),
            ("````", "`````"),
            ("~~~", "~~~"),
            ("```rust", "```"),
            ("```ignore", "```"),
            ("```text", "```"),
            ("~~~ sh", "~~~"),
            ("   ```compile_fail", "```"),
        ] {
            assert_eq!(
                blocks(&format!(
                    "Prose.\n{open}\nlet a = 1;\n\n# hidden\n{close}\nMore prose."
                )),
                [vec!["let a = 1;", "", "# hidden"]],
                "{open}"
            );
        }
        // An unclosed fence runs to the end; a shorter or other fence does
        // not close one.
        assert_eq!(
            blocks("````\na\n```\n~~~~\nb"),
            [vec!["a", "```", "~~~~", "b"]]
        );
        // A backtick fence's info string has no backtick.
        assert_eq!(blocks("```a`b\nprose"), Vec::<Vec<String>>::new());
        // A fence can interrupt a paragraph, and sits in a list item.
        assert_eq!(blocks("Prose\n```\nx\n```"), [vec!["x"]]);
        assert_eq!(blocks("- item\n  ```\n  x\n  ```\n- next"), [vec!["x"]]);
    }

    #[test]
    fn a_fence_ends_with_its_list_item_or_block_quote() {
        // The item ends at the unindented line, and its unclosed fence with
        // it; the next fence then opens a block of its own.
        assert_eq!(
            blocks("- item\n  ```text\n  in it\nOut of the list.\n```\nunsafe {}\n```"),
            [vec!["in it"], vec!["unsafe {}"]]
        );
        assert_eq!(
            blocks("> ```text\n> quoted\nNot quoted.\n```\nunsafe {}\n```"),
            [vec!["> quoted"], vec!["unsafe {}"]]
        );
        // In a fence out of any quote, `> ```` is code, not its end.
        assert_eq!(blocks("```\n> ```\nx\n```"), [vec!["> ```", "x"]]);
        // A closing fence indented four columns past its container is code.
        assert_eq!(blocks("```\n    ```\nx\n```"), [vec!["```", "x"]]);
    }

    #[test]
    fn indented_blocks_are_code_but_list_continuations_are_not() {
        assert_eq!(
            blocks("Prose.\n\n    let a = 1;\n\n    let b = 2;\nProse."),
            [vec!["let a = 1;", "", "let b = 2;"]]
        );
        // An indented line cannot interrupt a paragraph.
        assert_eq!(blocks("Prose\n    continued"), Vec::<Vec<String>>::new());
        // List items, nested, with continuation lines at their content's
        // column, after a blank line too.
        let prose = "- **A rule**: the keyword in any form (block,\n  `fn`, `impl`) in a file.\n  - A nested\n    item, continued\n    here.\n\n    A second paragraph of it.\n- 10. An ordered item\n  of a bullet.\n1. One\n   continued.\n2) Two\n   continued.";
        assert_eq!(blocks(prose), Vec::<Vec<String>>::new());
        // An indented block in a list item is four columns past its content.
        assert_eq!(blocks("- item\n\n      code\n- next"), [vec!["code"]]);
        assert_eq!(blocks("-     code"), [vec!["-     code"]]);
        // Lazy continuation lines keep the item and its paragraph.
        assert_eq!(
            blocks("- item\nlazy\n      not code"),
            Vec::<Vec<String>>::new()
        );
        // Only a list starting with 1 or a bullet interrupts a paragraph:
        // `2.` continues it, and so does the indented line after it.
        assert_eq!(
            blocks("Prose\n2. not a list\n    not code"),
            Vec::<Vec<String>>::new()
        );
        assert_eq!(blocks("Prose\n\n2. a list\n\n       code"), [vec!["code"]]);
        // Headings and thematic breaks end a paragraph.
        assert_eq!(blocks("# Heading\n    code"), [vec!["code"]]);
        assert_eq!(blocks("Prose\n***\n    code"), [vec!["code"]]);
        // In a block quote.
        assert_eq!(blocks(">     code"), [vec![">     code"]]);
    }

    #[test]
    fn items_are_unindented_as_rustdoc_unindents_them() {
        let line = |text: &str, at: usize| DocLine {
            text: text.to_string(),
            at: Place::Here(at),
        };
        // `///     x` alone is prose, once its five spaces are unindented.
        assert_eq!(
            code_blocks(vec![(vec![line("     x", 1)], true)]),
            Vec::<Vec<DocLine>>::new()
        );
        // Beside `/// a`, it is four columns in: code.
        assert_eq!(
            code_blocks(vec![(
                vec![line(" a", 1), line("", 2), line("     x", 3)],
                true
            )]),
            [vec![line("    x", 3)]]
        );
        // A doc attribute's text counts one more when mixed with comments.
        assert_eq!(
            code_blocks(vec![
                (vec![line(" a", 1), line("", 2)], true),
                (vec![line("    x", 3)], false)
            ]),
            [vec![line("    x", 3)]]
        );
    }

    #[test]
    fn doc_comments_and_attributes_are_gathered_by_item() {
        let docs = gathered(
            "//! Crate.\n#![allow(dead_code)]\n//! More.\n\n/// One.\n#[inline]\n#[doc = \"Two\\nThree\"]\n\
             #[cfg_attr(doc, doc = r#\"Four\"#)]\nfn f() {}\n/// Other.\nfn g() {}\n\
             /// Outer.\npub(crate) mod m {\n    //! Inner.\n}\n#[doc = include_str!(\"../README.md\")]\n\
             struct ReadmeDoctests;\n",
        );
        assert_eq!(docs.unreadable, Vec::<usize>::new());
        assert_eq!(
            texts(&docs),
            [
                vec![" Crate.", " More."],
                vec![" One.", "Two\nThree", "Four"],
                vec![" Other."],
                vec![" Outer.", " Inner."],
                vec!["+../README.md"],
            ]
        );
        // Each line knows where it is.
        let Some(Fragment::Text { lines, .. }) = docs.items.get(1).and_then(|item| item.get(1))
        else {
            panic!("the doc attribute's text");
        };
        assert_eq!(
            lines.iter().map(|line| line.at.clone()).collect::<Vec<_>>(),
            [Place::Here(7), Place::Here(7)]
        );
    }

    #[test]
    fn doc_strings_are_decoded_line_by_line() {
        assert_eq!(
            decode("\"a\\nb\\\n   c\\t\\x41\\u{1F600}\\u{4_1}\\\"\\\\\nd\"", 5),
            Some(vec![
                ("a".to_string(), 5),
                ("bc\tA\u{1F600}A\"\\".to_string(), 5),
                ("d".to_string(), 7)
            ])
        );
        assert_eq!(
            decode("r#\"a\n\"b\"#", 1),
            Some(vec![("a".to_string(), 1), ("\"b".to_string(), 2)])
        );
        for bad in [
            "b\"x\"",
            "c\"x\"",
            "'x'",
            "\"\\q\"",
            "\"\\x80\"",
            "\"\\u{110000}\"",
            "\"\\u{}\"",
            "\"open",
        ] {
            assert_eq!(decode(bad, 1), None, "{bad}");
        }
    }

    #[test]
    fn doc_values_of_any_other_form_are_unreadable() {
        for value in [
            "concat!(\"a\", \"b\")",
            "env!(\"X\")",
            "$d",
            "stringify!(x)",
            "include_str!(concat!(\"a\", \".md\"))",
            "include_str!(\"a\\x41.md\")",
            "include_str!(\"a.md\",)",
            "\"a\" \"b\"",
            "b\"a\"",
            "SOME_CONST",
        ] {
            let docs = gathered(&format!("#[doc = {value}]\nfn f() {{}}\n"));
            assert_eq!(docs.unreadable, [1], "{value}");
        }
        // The forms it reads.
        for value in [
            "\"a\"",
            "r\"a\"",
            "include_str!(\"a.md\")",
            "std::include_str!(\"a.md\")",
            "::core::include_str![\"a.md\"]",
        ] {
            assert_eq!(
                gathered(&format!("#[doc = {value}]\nfn f() {{}}\n")).unreadable,
                Vec::<usize>::new(),
                "{value}"
            );
        }
        // In a macro's transcriber.
        assert_eq!(
            gathered("macro_rules! m {\n    ($d:expr) => {\n        #[doc = $d]\n        fn f() {}\n    };\n}\n").unreadable,
            [3]
        );
    }

    #[test]
    fn lines_break_at_cr_lf_at_cr_and_at_lf() {
        assert_eq!(split_lines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\r\n"), ["a", ""]);
        assert_eq!(split_lines("\r\r\n"), ["", "", ""]);
        let one = |text: &str| (text.to_string(), 1);
        assert_eq!(
            decode("\"a\\rb\\r\\nc\\nd\"", 1),
            Some(vec![one("a"), one("b"), one("c"), one("d")])
        );
        // An escaped CR, then an LF written as itself: one line break, and
        // the next line is on the next source line.
        assert_eq!(
            decode("\"a\\r\nb\"", 1),
            Some(vec![one("a"), ("b".to_string(), 2)])
        );
    }

    #[test]
    fn markdown_structure_sees_only_spaces_and_tabs() {
        assert!(is_space(" \t") && is_space(""));
        assert!(!is_space("\u{a0}") && !is_space("\u{c}"));
        assert!(
            list_marker("- x").is_some()
                && list_marker("-\tx").is_some()
                && list_marker("-").is_some()
        );
        for marker in ["-\u{a0}x", "-\u{c}x", "1.\u{a0}x", "+\u{85}x", "-\u{3000}x"] {
            assert!(list_marker(marker).is_none(), "{marker:?}");
        }
        assert!(is_heading("# x") && is_heading("#\tx") && is_heading("##"));
        assert!(!is_heading("#\u{a0}x") && !is_heading("#\u{3000}x"));
        assert!(is_thematic_break("* * *") && is_thematic_break("-\t-\t-"));
        assert!(!is_thematic_break("*\u{a0}*\u{a0}*") && !is_thematic_break("-\u{85}-\u{85}-"));
        let fence = Fence {
            marker: '`',
            len: 3,
            items: 0,
            quotes: 0,
        };
        assert!(closes("```", &fence) && closes("````  ", &fence));
        assert!(!closes("```\t", &fence) && !closes("```\u{a0}", &fence));
    }

    #[test]
    fn setext_underlines_empty_items_and_footnotes_are_read() {
        for underline in ["=", "===", "-", "--", "===  "] {
            assert!(is_setext_underline(underline), "{underline:?}");
        }
        for not in ["", "= =", "=-", "==x"] {
            assert!(!is_setext_underline(not), "{not:?}");
        }
        assert_eq!(blocks("Text.\n===\n    code"), [vec!["code"]]);
        assert_eq!(blocks("- a\n  ===\n      code"), [vec!["code"]]);
        // A lazy line is no underline.
        assert_eq!(
            blocks("- a\n===\n      not code"),
            Vec::<Vec<String>>::new()
        );
        // An empty item closes at a blank line; one that gets content does
        // not, nor an outer item whose content is the empty one.
        assert_eq!(blocks("-\n\n    code"), [vec!["code"]]);
        assert_eq!(
            blocks("-\n  content\n\n    not code"),
            Vec::<Vec<String>>::new()
        );
        assert_eq!(blocks("- -\n\n     not code"), Vec::<Vec<String>>::new());
        assert_eq!(blocks("- -\n\n      code"), [vec!["code"]]);
        // A footnote definition holds what is indented four columns past it.
        assert_eq!(footnote_definition("[^1]: x"), Some(5));
        assert_eq!(footnote_definition("[^a b]:"), Some(7));
        for not in ["[^]: x", "[1]: x", "[^a]b]: x", "[^a"] {
            assert_eq!(footnote_definition(not), None, "{not}");
        }
        assert_eq!(blocks("[^1]: ```\n    x\n    ```"), [vec!["x"]]);
        assert_eq!(blocks("[^1]:\n    ```\n    x\n    ```"), [vec!["x"]]);
        assert_eq!(
            blocks("Text.\n\n[^a b]: text\n\n    not code"),
            Vec::<Vec<String>>::new()
        );
    }

    #[test]
    fn block_doc_comments_lose_their_decoration() {
        assert_eq!(
            beautify("*\n * A.\n *\n * ```\n * x\n * ```\n "),
            vec![
                (1, " A.".to_string()),
                (2, "".to_string()),
                (3, " ```".to_string()),
                (4, " x".to_string()),
                (5, " ```".to_string()),
                (6, "".to_string())
            ]
        );
        assert_eq!(beautify(" one line "), vec![(0, " one line ".to_string())]);
        // As `str::lines` reads it, a final LF leaves no empty last line, so
        // a last line of only `*` is dropped, and the decoration goes.
        assert_eq!(
            beautify("\n * ```\n * x\n * ```\n***\n"),
            vec![
                (1, " ```".to_string()),
                (2, " x".to_string()),
                (3, " ```".to_string())
            ]
        );
    }
}
