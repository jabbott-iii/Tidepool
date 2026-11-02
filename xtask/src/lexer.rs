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

//! Just enough of a Rust lexer for `unsafe-audit`: it tells code from
//! comments, string literals, raw strings and character literals, so that
//! text inside those can neither hide nor fake a keyword.
//!
//! Ported from nadir xtask `unsafe_audit.rs` (the scanner). Adapted for
//! Tidepool: literals are kept as tokens of their own kind (so the audit can
//! see the ABI string of `extern "C"` and the text of `#[doc = "..."]`), and
//! every comment is kept with its kind and whether it is a doc comment,
//! and raw identifiers (`r#name`) are tokens of their own kind, so they are
//! never mistaken for a keyword but can still name a lint or an attribute.
//!
//! The lexer never fails and never panics: text that is not valid Rust
//! gives some answer, and the compiler is the one to complain about it.

/// What kind of code a [`Token`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// An identifier, keyword or number.
    Word,
    /// A raw identifier, `r#name`: its text is the bare `name`, and it
    /// starts after the `r#`. It is a name, never a keyword.
    RawIdent,
    /// One punctuation character.
    Punct,
    /// A string, raw string, byte string, C string, character or byte
    /// literal, quotes and prefix included.
    Literal,
}

/// A piece of code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    /// What kind of token it is.
    pub(crate) kind: TokenKind,
    /// Its text.
    pub(crate) text: &'a str,
    /// The line it starts on, counted from 1.
    pub(crate) line: usize,
    /// Where it starts, in bytes.
    pub(crate) offset: usize,
}

impl Token<'_> {
    /// Whether this is the word `word` (never true for a literal or a raw
    /// identifier): use this for keywords.
    pub(crate) fn is_word(&self, word: &str) -> bool {
        self.kind == TokenKind::Word && self.text == word
    }

    /// Whether this is the name `name`, written plainly or as a raw
    /// identifier: use this for lints, attributes and macros, which rustc
    /// accepts in either spelling (`r#allow(r#unsafe_code)`).
    pub(crate) fn is_name(&self, name: &str) -> bool {
        self.is_ident() && self.text == name
    }

    /// Whether this is an identifier (or keyword), plain or raw.
    pub(crate) fn is_ident(&self) -> bool {
        matches!(self.kind, TokenKind::Word | TokenKind::RawIdent)
    }

    /// Whether this is the punctuation character `punct`.
    pub(crate) fn is_punct(&self, punct: &str) -> bool {
        self.kind == TokenKind::Punct && self.text == punct
    }

    /// The offset just past the token.
    pub(crate) fn end(&self) -> usize {
        self.offset + self.text.len()
    }
}

/// How a comment is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommentKind {
    /// `// ...` to the end of the line.
    Line,
    /// `/* ... */`, which may nest.
    Block,
}

/// Whether a comment is documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Doc {
    /// An ordinary comment, including `////` and `/***`.
    No,
    /// `///` or `/** */`: documents the item after it.
    Outer,
    /// `//!` or `/*! */`: documents the item it is in.
    Inner,
}

/// One comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Comment<'a> {
    /// Its text, markers included; a line comment's ends before the line
    /// break.
    pub(crate) text: &'a str,
    /// Where it starts, in bytes.
    pub(crate) offset: usize,
    /// The line it starts on, counted from 1.
    pub(crate) line: usize,
    /// How it is written.
    pub(crate) kind: CommentKind,
    /// Whether it is documentation.
    pub(crate) doc: Doc,
}

impl<'a> Comment<'a> {
    /// The comment's text without its markers: for a line comment, what
    /// follows `//`, `///` or `//!`; for a block comment, what lies between
    /// the opening and closing markers.
    pub(crate) fn body(&self) -> &'a str {
        match (self.kind, self.doc) {
            (CommentKind::Line, Doc::No) => self.text.get(2..),
            (CommentKind::Line, _) => self.text.get(3..),
            (CommentKind::Block, Doc::No) => self
                .text
                .get(2..)
                .map(|rest| rest.strip_suffix("*/").unwrap_or(rest)),
            (CommentKind::Block, _) => self
                .text
                .get(3..)
                .map(|rest| rest.strip_suffix("*/").unwrap_or(rest)),
        }
        .unwrap_or("")
    }
}

/// What the audit needs from one source file, in source order.
#[derive(Debug, Default)]
pub(crate) struct Lexed<'a> {
    /// The code.
    pub(crate) tokens: Vec<Token<'a>>,
    /// The comments.
    pub(crate) comments: Vec<Comment<'a>>,
}

/// Splits Rust source into tokens and comments.
pub(crate) fn lex(text: &str) -> Lexed<'_> {
    let mut lexer = Lexer {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        line: 1,
        lexed: Lexed::default(),
    };
    while let Some(byte) = lexer.peek(0) {
        match byte {
            b'\n' => lexer.newline(),
            b'/' if lexer.peek(1) == Some(b'/') => lexer.line_comment(),
            b'/' if lexer.peek(1) == Some(b'*') => lexer.block_comment(),
            b'"' => {
                let (start, line) = (lexer.pos, lexer.line);
                lexer.pos += 1;
                lexer.string();
                lexer.literal(start, line);
            }
            b'\'' => lexer.quote(),
            _ if byte.is_ascii_whitespace() => lexer.pos += 1,
            _ => match lexer.peek_char() {
                Some(character) if is_word_char(character) => lexer.word(),
                Some(character) if character.is_ascii() => {
                    lexer.token(TokenKind::Punct, lexer.pos, lexer.pos + 1, lexer.line);
                    lexer.pos += 1;
                }
                // Anything else separates tokens, as white space does.
                Some(character) => lexer.pos += character.len_utf8(),
                None => lexer.pos += 1,
            },
        }
    }
    lexer.lexed
}

/// Whether `byte` can start the name of a raw identifier (`r#name`). Names
/// that start outside ASCII are not recognized; they then read as an
/// ordinary word, which errs towards reporting.
fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Whether `character` can be part of an identifier, keyword or number.
///
/// Outside ASCII only letters and digits count. A byte-order mark or an
/// exotic space next to `unsafe` therefore cannot hide the keyword by
/// becoming part of the word.
fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// The next byte to look at.
    pos: usize,
    /// The line `pos` is on, counted from 1.
    line: usize,
    lexed: Lexed<'a>,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    /// The character at `pos`, if `pos` is at the start of one.
    fn peek_char(&self) -> Option<char> {
        self.text.get(self.pos..)?.chars().next()
    }

    /// Steps over the rest of a word.
    fn skip_word(&mut self) {
        while let Some(character) = self.peek_char().filter(|c| is_word_char(*c)) {
            self.pos += character.len_utf8();
        }
    }

    /// Steps over the line break at `pos`.
    fn newline(&mut self) {
        self.pos += 1;
        self.line += 1;
    }

    fn token(&mut self, kind: TokenKind, start: usize, end: usize, line: usize) {
        if let Some(text) = self.text.get(start..end) {
            self.lexed.tokens.push(Token {
                kind,
                text,
                line,
                offset: start,
            });
        }
    }

    /// Records the literal from `start`, on `line`, to `pos`.
    fn literal(&mut self, start: usize, line: usize) {
        self.token(TokenKind::Literal, start, self.pos, line);
    }

    /// Records the comment from `start`, on `line`, to `pos`.
    fn comment(&mut self, start: usize, line: usize, kind: CommentKind) {
        let Some(text) = self.text.get(start..self.pos) else {
            return;
        };
        let doc = match kind {
            CommentKind::Line if text.starts_with("////") => Doc::No,
            CommentKind::Line if text.starts_with("///") => Doc::Outer,
            CommentKind::Line if text.starts_with("//!") => Doc::Inner,
            CommentKind::Block if text.starts_with("/***") || text.starts_with("/**/") => Doc::No,
            CommentKind::Block if text.starts_with("/**") => Doc::Outer,
            CommentKind::Block if text.starts_with("/*!") => Doc::Inner,
            _ => Doc::No,
        };
        self.lexed.comments.push(Comment {
            text,
            offset: start,
            line,
            kind,
            doc,
        });
    }

    /// Reads a `//` comment, up to the end of the line.
    fn line_comment(&mut self) {
        let (start, line) = (self.pos, self.line);
        while self.peek(0).is_some_and(|byte| byte != b'\n') {
            self.pos += 1;
        }
        self.comment(start, line, CommentKind::Line);
    }

    /// Reads a `/* */` comment. These nest.
    fn block_comment(&mut self) {
        let (start, line) = (self.pos, self.line);
        let mut depth = 0_usize;
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(b'/'), Some(b'*')) => {
                    self.pos += 2;
                    depth += 1;
                }
                (Some(b'*'), Some(b'/')) => {
                    self.pos += 2;
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                (Some(b'\n'), _) => self.newline(),
                (Some(_), _) => self.pos += 1,
                (None, _) => break,
            }
        }
        self.comment(start, line, CommentKind::Block);
    }

    /// Skips the rest of a string literal. The opening quote is behind
    /// `pos`.
    fn string(&mut self) {
        while let Some(byte) = self.peek(0) {
            match byte {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                // An escape: whatever comes next does not end the string.
                b'\\' => {
                    self.pos += 1;
                    match self.peek(0) {
                        Some(b'\n') => self.newline(),
                        Some(_) => self.pos += 1,
                        None => {}
                    }
                }
                b'\n' => self.newline(),
                _ => self.pos += 1,
            }
        }
    }

    /// Skips the rest of a raw string literal, which ends at a quote
    /// followed by `hashes` `#` characters. The opening quote is behind
    /// `pos`.
    fn raw_string(&mut self, hashes: usize) {
        while let Some(byte) = self.peek(0) {
            match byte {
                b'"' => {
                    self.pos += 1;
                    if (0..hashes).all(|ahead| self.peek(ahead) == Some(b'#')) {
                        self.pos += hashes;
                        break;
                    }
                }
                b'\n' => self.newline(),
                _ => self.pos += 1,
            }
        }
    }

    /// Handles the `'` at `pos`: reads a character literal, or steps over
    /// the quote of a lifetime or loop label, whose name is then read as an
    /// ordinary word. `start` is where a prefix (`b`) began, if any.
    fn quote_from(&mut self, start: usize) {
        let line = self.line;
        if self.peek(1) == Some(b'\\') {
            // `'\n'`, `'\''`, `'\u{1F600}'`: the quote, the backslash and
            // the escaped byte (which may be a quote), then everything up
            // to the closing quote. A literal cannot span lines.
            self.pos += 2;
            if self.peek(0).is_some_and(|byte| byte != b'\n') {
                self.pos += 1;
            }
            while let Some(byte) = self.peek(0) {
                if byte == b'\n' {
                    break;
                }
                self.pos += 1;
                if byte == b'\'' {
                    break;
                }
            }
            self.literal(start, line);
            return;
        }
        let mut after = self.text.get(self.pos + 1..).unwrap_or("").chars();
        match (after.next(), after.next()) {
            // `'x'`: one character and the closing quote.
            (Some(character), Some('\'')) if !matches!(character, '\'' | '\n') => {
                self.pos += 2 + character.len_utf8();
                self.literal(start, line);
            }
            _ => self.pos += 1,
        }
    }

    /// [`Lexer::quote_from`] for a quote without a prefix.
    fn quote(&mut self) {
        self.quote_from(self.pos);
    }

    /// Reads a word. A word directly before a quote can be the prefix of a
    /// literal instead (`b"..."`, `r#"..."#`, `b'x'`).
    fn word(&mut self) {
        let (start, line) = (self.pos, self.line);
        self.skip_word();
        let word = self.text.get(start..self.pos).unwrap_or("");
        match (word, self.peek(0)) {
            ("b" | "c", Some(b'"')) => {
                self.pos += 1;
                self.string();
                self.literal(start, line);
            }
            ("b", Some(b'\'')) => self.quote_from(start),
            ("r" | "br" | "cr", Some(b'"' | b'#')) => self.raw(start, line, word == "r"),
            _ => self.token(TokenKind::Word, start, self.pos, line),
        }
    }

    /// Handles what follows the word `r`, `br` or `cr` (which started at
    /// `start`) when that is `"` or `#`: a raw string, or after `r#` a raw
    /// identifier.
    fn raw(&mut self, start: usize, line: usize, may_be_identifier: bool) {
        let mut hashes = 0;
        while self.peek(hashes) == Some(b'#') {
            hashes += 1;
        }
        if self.peek(hashes) == Some(b'"') {
            self.pos += hashes + 1;
            self.raw_string(hashes);
            self.literal(start, line);
        } else if may_be_identifier && hashes == 1 && self.peek(1).is_some_and(is_word_start) {
            // A raw identifier such as `r#type` is a name, never a keyword.
            self.pos += 1;
            let name = self.pos;
            self.skip_word();
            self.token(TokenKind::RawIdent, name, self.pos, line);
        } else {
            self.token(TokenKind::Word, start, self.pos, line);
        }
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::testing;

    use super::*;

    /// The code tokens of `text` that are not literals, as text.
    fn code(text: &str) -> Vec<&str> {
        lex(text)
            .tokens
            .iter()
            .filter(|token| token.kind != TokenKind::Literal)
            .map(|token| token.text)
            .collect()
    }

    /// The literals of `text`.
    fn literals(text: &str) -> Vec<&str> {
        lex(text)
            .tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Literal)
            .map(|token| token.text)
            .collect()
    }

    /// How many `unsafe` keywords `text` has in code.
    fn unsafe_count(text: &str) -> usize {
        lex(text)
            .tokens
            .iter()
            .filter(|token| token.is_word("unsafe"))
            .count()
    }

    #[test]
    fn code_becomes_words_punctuation_and_literals() {
        let lexed = lex("let x: u32 = unsafe { *ptr.add(1) }; f(\"s\", 'c');");
        let kinds: Vec<(TokenKind, &str)> = lexed
            .tokens
            .iter()
            .map(|token| (token.kind, token.text))
            .collect();
        use TokenKind::{Literal as L, Punct as P, Word as W};
        assert_eq!(
            kinds,
            [
                (W, "let"),
                (W, "x"),
                (P, ":"),
                (W, "u32"),
                (P, "="),
                (W, "unsafe"),
                (P, "{"),
                (P, "*"),
                (W, "ptr"),
                (P, "."),
                (W, "add"),
                (P, "("),
                (W, "1"),
                (P, ")"),
                (P, "}"),
                (P, ";"),
                (W, "f"),
                (P, "("),
                (L, "\"s\""),
                (P, ","),
                (L, "'c'"),
                (P, ")"),
                (P, ";"),
            ]
        );
        assert!(lex("").tokens.is_empty());
        assert!(lex(" \t\r\n\n").tokens.is_empty());
    }

    #[test]
    fn tokens_know_their_line_and_offset() {
        let lexed = lex("fn f() {\n    unsafe { g() }\n}\n");
        let found = lexed.tokens.iter().find(|token| token.is_word("unsafe"));
        assert_eq!(
            found,
            Some(&Token {
                kind: TokenKind::Word,
                text: "unsafe",
                line: 2,
                offset: 13
            })
        );
        assert_eq!(lexed.tokens.last().map(|token| token.line), Some(3));
        let lines: Vec<usize> = lex("a\r\nunsafe\r\n\r\nunsafe")
            .tokens
            .iter()
            .map(|token| token.line)
            .collect();
        assert_eq!(lines, [1, 2, 4]);
    }

    #[test]
    fn unsafe_is_matched_as_a_whole_word() {
        assert_eq!(unsafe_count("unsafe"), 1);
        assert_eq!(unsafe_count("(unsafe)"), 1);
        assert_eq!(unsafe_count("unsafe{unsafe}"), 2);
        for text in [
            "unsafe_code",
            "is_unsafe",
            "unsafely",
            "Unsafe",
            "UNSAFE",
            "unsafe1",
            "_unsafe",
            "unsafeé",
            "éunsafe",
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
        }
    }

    #[test]
    fn characters_that_are_not_letters_do_not_hide_a_keyword() {
        for stray in ['\u{feff}', '\u{a0}', '\u{200e}', '\u{2028}', '\u{1F600}'] {
            assert_eq!(
                code(&format!("{stray}unsafe{stray}{{{stray}}}")),
                ["unsafe", "{", "}"],
                "{stray:?}"
            );
            assert_eq!(code(&format!("r#type{stray}unsafe")), ["type", "unsafe"]);
        }
    }

    #[test]
    fn comments_are_not_code_and_are_classified() {
        for text in [
            "// unsafe { }\nafter",
            "/// unsafe { }\nafter",
            "//! unsafe { }\nafter",
            "/* unsafe { } */ after",
            "/* a\n unsafe\n */ after",
            "/* /* unsafe */ unsafe */ after",
            "/* /* /* */ unsafe */ unsafe */ after",
            "/** unsafe */ after",
            "/*/ unsafe */ after",
            "/* unsafe **/ after",
            "x /* unsafe */.after",
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            assert_eq!(code(text).last(), Some(&"after"), "{text}");
        }
        let docs: Vec<(CommentKind, Doc)> =
            lex("// a\n/// b\n//! c\n//// d\n/* e */ /** f */ /*! g */ /*** h */ /**/\n")
                .comments
                .iter()
                .map(|comment| (comment.kind, comment.doc))
                .collect();
        use CommentKind::{Block, Line};
        assert_eq!(
            docs,
            [
                (Line, Doc::No),
                (Line, Doc::Outer),
                (Line, Doc::Inner),
                (Line, Doc::No),
                (Block, Doc::No),
                (Block, Doc::Outer),
                (Block, Doc::Inner),
                (Block, Doc::No),
                (Block, Doc::No),
            ]
        );
        // A comment marker inside a string is not a comment.
        assert_eq!(unsafe_count(r#"let url = "http://x"; unsafe { }"#), 1);
        assert_eq!(unsafe_count(r#"let s = "/*"; unsafe { } /* "*/"#), 1);
        assert!(lex(r#""// SAFETY: no""#).comments.is_empty());
    }

    #[test]
    fn comment_bodies_drop_the_markers() {
        let bodies: Vec<&str> = lex("// a\n/// b\n//! c\n/* d */\n/** e */\n/*! f */\n")
            .comments
            .iter()
            .map(|comment| comment.body())
            .collect();
        assert_eq!(bodies, [" a", " b", " c", " d ", " e ", " f "]);
        // Unterminated: the body runs to the end.
        assert_eq!(lex("/* open").comments[0].body(), " open");
        let line_comment = lex("// SAFETY: x\r\nf();").comments[0];
        assert_eq!(line_comment.body(), " SAFETY: x\r");
        assert_eq!(line_comment.line, 1);
    }

    #[test]
    fn string_literals_are_literals() {
        for text in [
            r#""unsafe { }" after"#,
            r#""a \" unsafe \" b" after"#,
            r#""a \\" after"#,
            r#""a \\\" unsafe" after"#,
            r#"b"unsafe" after"#,
            r#"c"unsafe" after"#,
            r#"b"\"unsafe" after"#,
            "\"line one\nunsafe\nline three\" after",
            "\"continued \\\n   unsafe\" after",
            r#""" after"#,
            r#""'" after"#,
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            assert_eq!(code(text), ["after"], "{text}");
            assert_eq!(literals(text).len(), 1, "{text}");
        }
        assert_eq!(literals(r#"x = b"bytes";"#), [r#"b"bytes""#]);
    }

    #[test]
    fn raw_string_literals_are_literals() {
        for text in [
            r###"r"unsafe \" after"###,
            r###"r#"unsafe " unsafe"# after"###,
            r###"r##"unsafe "# unsafe"## after"###,
            r###"br"unsafe" after"###,
            r###"br#"unsafe " unsafe"# after"###,
            r###"cr#"unsafe " unsafe"# after"###,
            "r#\"line one\nunsafe\"# after",
            r###"r#""# after"###,
        ] {
            assert_eq!(unsafe_count(text), 0, "{text}");
            assert_eq!(code(text), ["after"], "{text}");
        }
        assert_eq!(literals(r###"x(r#"a"b"#)"###), [r###"r#"a"b"#"###]);
        // In a raw string a backslash is an ordinary character.
        assert_eq!(code(r#"r"\" after r"\" last"#), ["after", "last"]);
    }

    #[test]
    fn words_that_only_look_like_literal_prefixes_are_words() {
        assert_eq!(code("r + b - c"), ["r", "+", "b", "-", "c"]);
        assert_eq!(code("br"), ["br"]);
        assert_eq!(code(r#"attr "unsafe" x"#), ["attr", "x"]);
        assert_eq!(code("r # x"), ["r", "#", "x"]);
        assert_eq!(code("br#x"), ["br", "#", "x"]);
    }

    #[test]
    fn raw_identifiers_are_names_but_not_keywords() {
        let lexed = lex("let r#type = r#unsafe;");
        let kinds: Vec<(TokenKind, &str)> = lexed
            .tokens
            .iter()
            .map(|token| (token.kind, token.text))
            .collect();
        use TokenKind::{Punct as P, RawIdent as R, Word as W};
        assert_eq!(
            kinds,
            [(W, "let"), (R, "type"), (P, "="), (R, "unsafe"), (P, ";")]
        );
        // The token starts after `r#`, at its bare name.
        assert_eq!(lexed.tokens[1].offset, 6);
        assert_eq!(unsafe_count("r#unsafe { }"), 0);
        let allow = lex("#![r#allow(r#unsafe_code)]");
        assert!(allow.tokens.iter().any(|token| token.is_name("allow")));
        assert!(
            allow
                .tokens
                .iter()
                .any(|token| token.is_name("unsafe_code"))
        );
        assert!(!allow.tokens.iter().any(|token| token.is_word("allow")));
        assert!(lex("unsafe_code").tokens[0].is_name("unsafe_code"));
        assert!(!lex("\"unsafe_code\"").tokens[0].is_name("unsafe_code"));
    }

    #[test]
    fn character_literals_are_literals() {
        for text in [
            "'a' after",
            "'\"' after",
            "'{' after",
            "'é' after",
            "'\u{1F600}' after",
            r"'\'' after",
            r"'\\' after",
            r"'\n' after",
            r"'\x41' after",
            r"'\u{1F600}' after",
            r"'\u{22}' after",
            "b'a' after",
            "b'\"' after",
            r"b'\'' after",
            r"b'\\' after",
        ] {
            assert_eq!(code(text), ["after"], "{text}");
            assert_eq!(literals(text).len(), 1, "{text}");
        }
        assert_eq!(literals(r"[b'\'', 'x']"), [r"b'\''", "'x'"]);
        // A quote character literal must not open a string, and the string
        // after it must still be read as one.
        assert_eq!(
            code(r#"let q = '"'; unsafe { } "unsafe""#),
            ["let", "q", "=", ";", "unsafe", "{", "}"]
        );
        assert_eq!(
            unsafe_count(r#"match c { b'"' => unsafe { }, b'\\' => 1 }"#),
            1
        );
    }

    #[test]
    fn a_character_literal_ends_exactly_at_its_closing_quote() {
        for text in [
            r#"['a','"'] after"#,
            r#"['é','"'] after"#,
            "['\u{1F600}','\"'] after",
            r#"['\'','"'] after"#,
            r#"['\\','"'] after"#,
            r#"['\n','"'] after"#,
            r#"['\u{e9}','"'] after"#,
            r#"[b'a',b'"'] after"#,
            r#"[b'\'',b'"'] after"#,
        ] {
            assert_eq!(code(text), ["[", ",", "]", "after"], "{text}");
        }
    }

    #[test]
    fn lifetimes_and_labels_are_not_character_literals() {
        assert_eq!(
            code("fn f<'a>(x: &'a str) -> &'a str { unsafe { x } }"),
            [
                "fn", "f", "<", "a", ">", "(", "x", ":", "&", "a", "str", ")", "-", ">", "&", "a",
                "str", "{", "unsafe", "{", "x", "}", "}"
            ]
        );
        assert_eq!(
            unsafe_count("'outer: loop { unsafe { }; break 'outer; }"),
            1
        );
        assert_eq!(unsafe_count("&'static str; unsafe { }"), 1);
        assert_eq!(
            unsafe_count("fn f<'a>(c: &'a u8) -> bool { *c == b'\"' } unsafe"),
            1
        );
        assert_eq!(unsafe_count("x: &'a T, y: 'b', unsafe"), 1);
    }

    #[test]
    fn lines_are_counted_through_comments_and_literals() {
        let text = "/* one\ntwo */ \"three\nfour\" r#\"five\nsix\"# \"seven \\\neight\" // nine\n\
                    unsafe\n/*\n*/ unsafe\n'x' unsafe\n\n\nunsafe";
        let lines: Vec<usize> = lex(text)
            .tokens
            .iter()
            .filter(|token| token.is_word("unsafe"))
            .map(|token| token.line)
            .collect();
        assert_eq!(lines, [6, 8, 9, 12]);
        let literal_lines: Vec<usize> = lex(text)
            .tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Literal)
            .map(|token| token.line)
            .collect();
        // Each literal is on the line where it starts.
        assert_eq!(literal_lines, [2, 3, 4, 9]);
    }

    #[test]
    fn text_that_ends_early_does_not_hide_what_came_before() {
        for text in [
            "unsafe { } \"never closed",
            "unsafe { } /* never closed",
            "unsafe { } r#\"never closed\"",
            "unsafe { } '",
            "unsafe { } '\\",
            "unsafe { } b'",
            "unsafe { } \"ends with a backslash \\",
            "unsafe { } /",
            "unsafe { } r#",
        ] {
            assert_eq!(unsafe_count(text), 1, "{text}");
        }
    }

    #[test]
    fn lexing_never_panics_and_keeps_its_bookkeeping_consistent() {
        const PIECES: &[&str] = &[
            "unsafe",
            "unsafe_code",
            "allow",
            "impl",
            "extern",
            "pub",
            "fn",
            "r",
            "b",
            "br",
            "c",
            "x",
            "é",
            "\u{1F600}",
            "\"",
            "'",
            "#",
            "\\",
            "/",
            "*",
            "//",
            "///",
            "/*",
            "/**",
            "*/",
            "{",
            "}",
            "(",
            ")",
            "[",
            "]",
            ";",
            "\n",
            " ",
            "!",
            "SAFETY:",
            "# Safety",
            "r#\"",
            "\"#",
            "b'",
            "'\\",
            "\\\n",
            "\r\n",
        ];
        testing::check_sized(
            "xtask_lexer",
            2000,
            |rng, size| {
                let mut text = String::new();
                for _ in 0..rng.range(0..size + 1) {
                    text.push_str(rng.choose(PIECES).copied().unwrap_or(""));
                }
                text
            },
            |text| {
                let lexed = lex(text);
                let line_count = text.matches('\n').count() + 1;
                let mut previous_end = 0;
                for token in &lexed.tokens {
                    if text.get(token.offset..token.end()) != Some(token.text) {
                        return Err(format!("{token:?} is not at its offset"));
                    }
                    if token.text.is_empty() {
                        return Err(format!("{token:?} is empty"));
                    }
                    let expected_line = text[..token.offset].matches('\n').count() + 1;
                    if token.line != expected_line {
                        return Err(format!("{token:?} is on line {expected_line}"));
                    }
                    if token.offset < previous_end {
                        return Err(format!("{token:?} overlaps the token before it"));
                    }
                    previous_end = token.end();
                }
                for comment in &lexed.comments {
                    if text.get(comment.offset..comment.offset + comment.text.len())
                        != Some(comment.text)
                    {
                        return Err(format!("{comment:?} is not at its offset"));
                    }
                    if comment.line > line_count {
                        return Err(format!("{comment:?} is outside the text"));
                    }
                    let _ = comment.body();
                }
                Ok(())
            },
        );
    }
}
