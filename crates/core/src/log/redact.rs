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

//! The secret registry behind [`register_secret`](super::register_secret).
//!
//! New in Tidepool; not ported.
//!
//! Every registered secret becomes one or more literal patterns, one for
//! each form the secret can take in a line (duplicates are dropped):
//!
//! - the secret itself;
//! - the secret as it reads in a written line, with control characters
//!   escaped and continuation lines indented (`super::escape_controls`);
//! - the forms `std`'s escapers give it, applied on top of each other up to
//!   [`ESCAPE_DEPTH`] times, so that a secret `{:?}`-formatted into a string
//!   that is itself `{:?}`-formatted is found too. The escapers are: `{:?}`
//!   without the quotes; `str::escape_debug` at the start of the escaped
//!   string and anywhere else (`escape_debug` treats a leading combining
//!   mark differently); and `char::escape_debug`, character by character.
//!   For a secret without quotes, backslashes, control characters or
//!   combining marks every escaper gives the secret itself, so such a
//!   secret is one pattern.
//!
//! A secret that ends with line breaks is registered without them as well,
//! because a record never ends with one: that is how it reads at the end of
//! a record.
//!
//! Other encodings are not recognised: the `Debug` form of the secret's
//! bytes (`[112, 97, …]`), JSON's `\u001b` escapes, percent-encoding,
//! base64, and so on. Register each encoded form that might be logged.
//!
//! A line is redacted by replacing every occurrence of every pattern,
//! overlapping ones included, with [`REDACTED`]; overlapping and touching
//! matches merge into one marker. Matching runs on the original line, so
//! the marker is never matched itself. A secret's patterns are searched for
//! only if the line contains the secret's anchor, a run of its characters
//! that every form shares, so a line costs one search per secret rather
//! than one per pattern.
//!
//! After redaction the line is checked again. If any pattern still occurs
//! (possible only when a secret overlaps the marker text, for example a
//! secret `"ACT"` or `"D]x"`), the whole line is withheld rather than
//! written. So no sink ever receives a registered secret, whatever it is.
//! The price: a secret that is part of the marker itself (`"ACT"`) withholds
//! every line in which any secret is redacted. Real secrets (passwords,
//! tokens, keys) never are.

use std::fmt;

use super::{LINE_BREAKS, REDACTED};

/// How many times the escapers are applied on top of each other: a secret
/// `{:?}`-formatted into a string that is `{:?}`-formatted again, and once
/// more. Each level escapes every backslash and quote of the one before.
const ESCAPE_DEPTH: usize = 3;

/// What redacting a line produced.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Redaction {
    /// No secret occurs in the line; use it as it is.
    Clean,
    /// The line with every secret replaced.
    Redacted(String),
    /// The line cannot be made safe and must not be written anywhere.
    Withheld,
}

/// The registered secrets, as literal patterns.
///
/// Patterns are only ever appended, so the patterns a registration added are
/// those from the [`pattern_count`](Self::pattern_count) before it on.
///
/// Its `Debug` output shows how many patterns there are, never what they
/// are.
#[derive(Clone, Default)]
pub(super) struct Secrets {
    /// Non-empty and without duplicates.
    patterns: Vec<String>,
    /// The patterns, in order, in groups that share an anchor.
    groups: Vec<Group>,
}

/// The patterns added for one secret, which all contain its anchor.
#[derive(Clone)]
struct Group {
    /// The longest run of the secret's characters that no escaper changes
    /// ([`anchor`]). Every form of the secret contains it, so text without
    /// it contains none of the group's patterns and need not be searched
    /// for them. Empty if the secret has no such character.
    anchor: String,
    /// The group's patterns: `patterns[start..end]`.
    start: usize,
    end: usize,
}

impl Secrets {
    pub(super) const fn new() -> Self {
        Self {
            patterns: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// The patterns from index `from` on that `text` could contain: those
    /// whose group's anchor occurs in it. One search per secret rules out
    /// all of its forms at once.
    fn candidates<'a>(&'a self, text: &'a str, from: usize) -> impl Iterator<Item = &'a str> {
        self.groups
            .iter()
            .filter(move |group| group.end > from)
            .filter(move |group| group.anchor.is_empty() || text.contains(group.anchor.as_str()))
            .flat_map(move |group| {
                let start = group.start.max(from);
                self.patterns
                    .get(start..group.end)
                    .unwrap_or_default()
                    .iter()
                    .map(String::as_str)
            })
    }

    /// Registers `secret`, and `secret` without its trailing line breaks if
    /// it has any. Returns `true` if that added a pattern that was not
    /// registered before. An empty secret is ignored.
    pub(super) fn add(&mut self, secret: &str) -> bool {
        let trimmed = secret.trim_end_matches(LINE_BREAKS);
        let mut added = self.add_forms(secret);
        if trimmed != secret {
            added |= self.add_forms(trimmed);
        }
        added
    }

    /// Adds the patterns for every form of `text`; see [`Self::add`].
    fn add_forms(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let start = self.patterns.len();
        for pattern in forms(text) {
            if !pattern.is_empty() && !self.patterns.contains(&pattern) {
                self.patterns.push(pattern);
            }
        }
        let end = self.patterns.len();
        if end == start {
            return false;
        }
        // A pattern another secret added already stays in that secret's
        // group, whose anchor it contains as well.
        self.groups.push(Group {
            anchor: anchor(text),
            start,
            end,
        });
        true
    }

    /// How many patterns there are.
    pub(super) fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Redacts every registered secret in `text`.
    pub(super) fn redact(&self, text: &str) -> Redaction {
        self.redact_with_tail(text, 0)
    }

    /// Like [`Self::redact`], for text whose continuation was cut off: the
    /// longest end of `text` that is the start of a pattern from index
    /// `from` on ([`Self::partial_at_end`]) is replaced too, as part of the
    /// same marker as any occurrence it overlaps or touches.
    pub(super) fn redact_cut(&self, text: &str, from: usize) -> Redaction {
        self.redact_with_tail(text, self.partial_at_end(text, from))
    }

    /// Redacts every pattern in `text`, and its last `tail` bytes.
    fn redact_with_tail(&self, text: &str, tail: usize) -> Redaction {
        if self.patterns.is_empty() {
            return Redaction::Clean;
        }
        let mut spans: Vec<(usize, usize)> = Vec::new();
        if tail > 0 {
            spans.push((text.len().saturating_sub(tail), text.len()));
        }
        for pattern in self.candidates(text, 0) {
            // Every occurrence, overlapping ones included: `match_indices`
            // would skip an occurrence that starts inside the previous one
            // (`"abab"` in `"ababab"`) and leave its tail in the line. The
            // next search starts one character into the match, which is the
            // pattern's first character.
            let step = pattern.chars().next().map_or(1, char::len_utf8);
            let mut from = 0;
            while let Some(found) = text.get(from..).and_then(|rest| rest.find(pattern)) {
                let start = from + found;
                spans.push((start, start + pattern.len()));
                from = start + step;
            }
        }
        if spans.is_empty() {
            return Redaction::Clean;
        }
        spans.sort_unstable();

        let mut out = String::with_capacity(text.len());
        // How much of `text` has been copied or replaced.
        let mut copied = 0;
        let mut current: Option<(usize, usize)> = None;
        for (start, end) in spans {
            current = match current {
                // Overlapping or touching: one marker covers both.
                Some((first, last)) if start <= last => Some((first, last.max(end))),
                Some((first, last)) => {
                    if !replace(text, &mut out, &mut copied, first, last) {
                        return Redaction::Withheld;
                    }
                    Some((start, end))
                }
                None => Some((start, end)),
            };
        }
        if let Some((first, last)) = current
            && !replace(text, &mut out, &mut copied, first, last)
        {
            return Redaction::Withheld;
        }
        match text.get(copied..) {
            Some(rest) => out.push_str(rest),
            None => return Redaction::Withheld,
        }

        if self.occurs_in(&out) {
            Redaction::Withheld
        } else {
            Redaction::Redacted(out)
        }
    }

    /// Returns `true` if any pattern occurs in `text`.
    pub(super) fn occurs_in(&self, text: &str) -> bool {
        self.occurs_since(text, 0)
    }

    /// Returns `true` if any pattern from index `from` on occurs in `text`:
    /// for a line that is known to be free of the patterns before `from`,
    /// whether it needs redacting again.
    pub(super) fn occurs_since(&self, text: &str, from: usize) -> bool {
        self.candidates(text, from)
            .any(|pattern| text.contains(pattern))
    }

    /// Returns `true` if `text` followed by a newline contains a pattern
    /// that ends there: `text` ends with all of a pattern but its final
    /// newline. For a line that is already redacted, whether the newline
    /// that ends the record would complete a secret.
    pub(super) fn completed_by_newline(&self, text: &str) -> bool {
        self.patterns.iter().any(|pattern| {
            pattern
                .strip_suffix('\n')
                .is_some_and(|start| text.ends_with(start))
        })
    }

    /// The length in bytes of the longest end of `text` that is the start,
    /// but not the whole, of a pattern from index `from` on; 0 if there is
    /// none. When the text that followed `text` is gone, this is what may be
    /// left of a secret that the cut went through.
    pub(super) fn partial_at_end(&self, text: &str, from: usize) -> usize {
        let mut longest = 0;
        for pattern in self.patterns.get(from..).unwrap_or_default() {
            // The pattern's proper prefixes end on its character boundaries;
            // try the longest first, and stop at the first `text` ends with
            // or at one no longer than the best so far.
            for (end, _) in pattern.char_indices().rev() {
                if end <= longest {
                    break;
                }
                if pattern
                    .get(..end)
                    .is_some_and(|start| text.ends_with(start))
                {
                    longest = end;
                    break;
                }
            }
        }
        longest
    }
}

/// The forms `secret` can take in a log line; see the module documentation.
/// The secret itself comes first.
fn forms(secret: &str) -> Vec<String> {
    let mut forms = vec![secret.to_string()];
    // Each level escapes every form of the level before. A form that is
    // already known needs no escaping again: it was, or will be, escaped
    // from an earlier level, with as many levels left or more.
    let mut level = vec![secret.to_string()];
    for _ in 0..ESCAPE_DEPTH {
        let mut next = Vec::new();
        for text in &level {
            for escaped in escapes(text) {
                if !forms.contains(&escaped) {
                    forms.push(escaped.clone());
                    next.push(escaped);
                }
            }
        }
        level = next;
    }
    // Only the secret itself can contain control characters or line
    // breaks: every escaper above escapes them.
    let as_written = super::escape_controls(secret).unwrap_or_else(|| secret.to_string());
    if !forms.contains(&as_written) {
        forms.push(as_written);
    }
    forms
}

/// The longest run of characters in `secret` that read the same in every
/// form ([`forms`]): characters that `char::escape_debug`, the strictest
/// escaper, leaves as they are. The others escape no more than it does, and
/// every escaper works one character at a time, so the run appears in every
/// form. The first such run wins a tie.
fn anchor(secret: &str) -> String {
    let unchanged = |c: char| {
        let mut escaped = c.escape_debug();
        escaped.next() == Some(c) && escaped.next().is_none()
    };
    secret
        .split(|c: char| !unchanged(c))
        .fold(
            "",
            |best, run| if run.len() > best.len() { run } else { best },
        )
        .to_string()
}

/// How `text` reads inside a longer string escaped by each of `std`'s
/// escapers: the module documentation lists them.
fn escapes(text: &str) -> [String; 4] {
    // `{:?}` on a `str` escapes each character independently, so the
    // escaped text appears verbatim inside the escaped form of any string
    // that contains it. Strip the quotes it adds.
    let debug = format!("{text:?}");
    let debug = debug
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(&debug)
        .to_string();
    // `str::escape_debug` escapes a grapheme extender (a combining mark)
    // only as the first character of the string, so the text reads
    // differently at the start of the escaped string and after anything
    // else. A space escapes to itself, one byte, so strip it off.
    let at_start = text.escape_debug().to_string();
    let after_text = format!(" {text}").escape_debug().to_string();
    let after_text = after_text
        .strip_prefix(' ')
        .unwrap_or(&after_text)
        .to_string();
    let char_by_char = text.chars().flat_map(char::escape_debug).collect();
    [debug, at_start, after_text, char_by_char]
}

/// Copies `text[*copied..start]` to `out`, then the marker for
/// `text[start..end]`. Returns `false` if the offsets are not character
/// boundaries, which matches from `match_indices` always are; the caller
/// then withholds the line rather than risk writing it.
fn replace(text: &str, out: &mut String, copied: &mut usize, start: usize, end: usize) -> bool {
    match text.get(*copied..start) {
        Some(before) if text.is_char_boundary(end) => {
            out.push_str(before);
            out.push_str(REDACTED);
            *copied = end;
            true
        }
        _ => false,
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Secrets({} patterns, values hidden)",
            self.patterns.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check;

    fn secrets(list: &[&str]) -> Secrets {
        let mut secrets = Secrets::new();
        for secret in list {
            secrets.add(secret);
        }
        secrets
    }

    fn redacted(secrets: &Secrets, text: &str) -> String {
        match secrets.redact(text) {
            Redaction::Clean => text.to_string(),
            Redaction::Redacted(out) => out,
            Redaction::Withheld => "<withheld>".to_string(),
        }
    }

    #[test]
    fn nothing_registered_means_nothing_changes() {
        assert_eq!(Secrets::new().redact("hunter2"), Redaction::Clean);
        assert_eq!(secrets(&["hunter2"]).redact("no secrets"), Redaction::Clean);
    }

    #[test]
    fn empty_secrets_are_ignored() {
        let mut secrets = Secrets::new();
        assert!(!secrets.add(""));
        assert!(secrets.patterns.is_empty());
        assert!(secrets.add("x1"));
        assert!(!secrets.add("x1"), "registering twice adds nothing");
    }

    #[test]
    fn every_occurrence_is_replaced() {
        let s = secrets(&["hunter2"]);
        assert_eq!(
            redacted(&s, "pw=hunter2, again hunter2!"),
            "pw=[REDACTED], again [REDACTED]!"
        );
        assert_eq!(redacted(&s, "hunter2hunter2"), "[REDACTED]");
    }

    #[test]
    fn overlapping_secrets_merge_into_one_marker() {
        let s = secrets(&["abcd", "cdef"]);
        assert_eq!(redacted(&s, "xabcdefx"), "x[REDACTED]x");
        // A shorter secret inside a longer one.
        let s = secrets(&["token", "tok"]);
        assert_eq!(redacted(&s, "a token, a tok"), "a [REDACTED], a [REDACTED]");
        // A shorter secret that ends before the longer one does: the marker
        // must cover the longer one to its end.
        let s = secrets(&["tp-token-ABCDEF", "token"]);
        assert_eq!(redacted(&s, "auth tp-token-ABCDEF"), "auth [REDACTED]");
        let s = secrets(&["token", "tp-token-ABCDEF"]);
        assert_eq!(redacted(&s, "auth tp-token-ABCDEF!"), "auth [REDACTED]!");
    }

    #[test]
    fn overlapping_occurrences_of_one_secret_are_all_covered() {
        // Non-overlapping matching would find only the first "abab" in
        // "ababab" and leave the second one's tail, "ab", in the line.
        let s = secrets(&["abab"]);
        assert_eq!(redacted(&s, "xababab"), "x[REDACTED]");
        let s = secrets(&["éé"]);
        assert_eq!(redacted(&s, "éééx"), "[REDACTED]x");
    }

    #[test]
    fn debug_escaped_forms_are_redacted_too() {
        let secret = "p\"a\\ss\n'é\u{301}";
        let s = secrets(&[secret]);
        let as_debug = format!("{:?}", secret.to_string());
        assert_eq!(redacted(&s, &as_debug), "\"[REDACTED]\"");
        let in_struct = format!("{:?}", ("user", secret));
        assert_eq!(redacted(&s, &in_struct), "(\"user\", \"[REDACTED]\")");
        let escaped = secret.escape_debug().to_string();
        assert_eq!(redacted(&s, &escaped), "[REDACTED]");
    }

    #[test]
    fn escape_debug_forms_after_other_text_are_redacted_too() {
        // `escape_debug` escapes a leading combining mark only at the very
        // start of the string, so after other text the secret reads
        // differently than it does alone.
        let secret = "\u{301}pw\\x";
        let s = secrets(&[secret]);
        assert_eq!(
            redacted(&s, &secret.escape_debug().to_string()),
            "[REDACTED]"
        );
        let inside = format!("name={secret}").escape_debug().to_string();
        assert_eq!(redacted(&s, &inside), "name=[REDACTED]");
        // `char::escape_debug`, one character at a time, escapes the
        // combining mark everywhere and quotes as well.
        let secret = "a\u{301}'b";
        let s = secrets(&[secret]);
        let by_char: String = format!("x{secret}")
            .chars()
            .flat_map(char::escape_debug)
            .collect();
        assert_eq!(redacted(&s, &by_char), "x[REDACTED]");
    }

    #[test]
    fn the_written_form_of_a_secret_with_control_characters_is_redacted() {
        // A line is written with control characters escaped and
        // continuation lines indented; the secret must be found in that
        // form too, for lines re-scrubbed after it was registered.
        let secret = "a\u{1b}\"b\nc";
        let s = secrets(&[secret]);
        let written = super::super::escape_controls(&format!("key {secret}!"))
            .expect("the line has control characters");
        assert_eq!(written, "key a\\u{1b}\"b\n  c!");
        assert_eq!(redacted(&s, &written), "key [REDACTED]!");
    }

    #[test]
    fn a_secret_that_overlaps_the_marker_withholds_the_line() {
        // Replacing "ACT" with "[REDACTED]" would write "ACT" again.
        let s = secrets(&["ACT"]);
        assert_eq!(s.redact("ACT"), Redaction::Withheld);
        assert_eq!(s.redact("unrelated"), Redaction::Clean);
        // A secret that would form across the marker's edge.
        let s = secrets(&["D]x"]);
        assert_eq!(s.redact("D]xx"), Redaction::Withheld);
    }

    #[test]
    fn nested_debug_forms_are_redacted() {
        // A password `{:?}`-formatted into an error message, and the error
        // `{:?}`-formatted again when it is logged: each level escapes the
        // backslashes and quotes of the one before.
        let pw = "pa\"ss\\word-tp";
        let s = secrets(&[pw]);
        let err: Result<(), String> = Err(format!("bad password {pw:?}"));
        let line = format!("login: {err:?}");
        assert_eq!(
            line,
            r#"login: Err("bad password \"pa\\\"ss\\\\word-tp\"")"#
        );
        assert_eq!(
            redacted(&s, &line),
            r#"login: Err("bad password \"[REDACTED]\"")"#
        );
        // Three levels deep.
        let three = format!("{:?}", format!("{:?}", format!("{pw:?}")));
        assert_eq!(redacted(&s, &three), r#""\"\\\"[REDACTED]\\\"\"""#);
        // `escape_debug` of a `{:?}` form, and the other way round.
        let mixed = format!("{:?}", format!("{pw:?}").escape_debug().to_string());
        assert!(!redacted(&s, &mixed).contains("word-tp"), "{mixed}");
    }

    #[test]
    fn a_secret_with_trailing_line_breaks_is_registered_without_them_too() {
        let s = secrets(&["tok-5a\r\n"]);
        assert_eq!(redacted(&s, "using tok-5a"), "using [REDACTED]");
        assert_eq!(redacted(&s, "raw tok-5a\r\n!"), "raw [REDACTED]!");
        // Only trailing ones: a line break inside stays part of the secret.
        let s = secrets(&["a\nb"]);
        assert_eq!(s.redact("a b"), Redaction::Clean);
        // Nothing but line breaks leaves nothing extra to register.
        let mut only = Secrets::new();
        assert!(only.add("\r\n"));
        assert_eq!(only.patterns, forms("\r\n"));
    }

    #[test]
    fn plain_secrets_are_one_pattern_and_escaped_ones_stay_few() {
        // Escaping changes nothing in an alphanumeric key, so every form is
        // the key itself.
        assert_eq!(secrets(&["sk-live-0123456789abcdef"]).pattern_count(), 1);
        // A secret with everything that escapes differently: a leading
        // combining mark, both quotes, a backslash, a control character and
        // line breaks. Each pattern costs a search of every line, so the
        // number must stay small: 54 when this was written, for the secret
        // and its trimmed form together.
        let nasty = secrets(&["\u{301}a'\"\\\u{1b}\n\r\n"]);
        assert!(nasty.pattern_count() <= 64, "{}", nasty.pattern_count());
    }

    #[test]
    fn every_form_of_a_secret_contains_its_anchor() {
        assert_eq!(anchor("pa\"ss\\w'ord-1-\u{301}"), "ord-1-");
        assert_eq!(anchor("é😀k\u{301}x"), "é😀k");
        assert_eq!(anchor("\"'\\\n"), "");
        // Random secrets: the anchor is in every pattern, so skipping a
        // secret's patterns when its anchor is absent loses nothing.
        const ALPHABET: [&str; 12] = [
            "a", "Z", "7", "-", "é", "😀", "\"", "'", "\\", "\n", "\u{1b}", "\u{301}",
        ];
        check(
            "every form contains the anchor",
            2000,
            |rng| {
                let len = rng.range(1..12u32);
                (0..len)
                    .map(|_| *rng.choose(&ALPHABET).expect("not empty"))
                    .collect::<String>()
            },
            |secret| {
                let anchor = anchor(secret);
                match forms(secret)
                    .into_iter()
                    .find(|form| !form.contains(&anchor))
                {
                    Some(form) => Err(format!("{form:?} lacks {anchor:?}")),
                    None => Ok(()),
                }
            },
        );
    }

    #[test]
    fn a_pattern_shared_by_two_secrets_is_found_through_either() {
        // The second secret is one of the first one's forms (its
        // `escape_debug` form, `x\'y`), so that pattern is not added again
        // and is in the first secret's group only. It is still found,
        // through that group, whose anchor it contains.
        let s = secrets(&["x'y", "x\\'y"]);
        assert_eq!(redacted(&s, "a x\\'y b"), "a [REDACTED] b");
        assert_eq!(redacted(&s, "a x'y b"), "a [REDACTED] b");
    }

    #[test]
    fn occurs_since_looks_only_at_newer_patterns() {
        let mut s = secrets(&["old-secret"]);
        let from = s.pattern_count();
        s.add("new-secret");
        assert!(s.occurs_in("an old-secret"));
        assert!(!s.occurs_since("an old-secret", from));
        assert!(s.occurs_since("a new-secret", from));
        assert!(!s.occurs_since("a new-secret", s.pattern_count()));
    }

    #[test]
    fn completed_by_newline_finds_secrets_the_record_end_would_finish() {
        let mut s = Secrets::new();
        // Registered by hand, without the trimmed form `add` also adds, to
        // see the check on its own.
        s.patterns.push("tok\n".to_string());
        assert!(s.completed_by_newline("using tok"));
        assert!(!s.completed_by_newline("using tok!"));
        assert!(!s.completed_by_newline("using to"));
        assert!(!secrets(&["tok"]).completed_by_newline("using tok"));
        // A lone newline ends every record.
        assert!(secrets(&["\n"]).completed_by_newline("anything"));
    }

    #[test]
    fn partial_at_end_is_the_longest_start_of_a_newer_pattern() {
        let s = secrets(&["abcdef", "cdxy"]);
        assert_eq!(s.partial_at_end("xxabcd", 0), 4, "abcd starts abcdef");
        assert_eq!(s.partial_at_end("xxabc", 0), 3);
        assert_eq!(s.partial_at_end("xxcd", 0), 2, "cd starts cdxy");
        assert_eq!(
            s.partial_at_end("xxabcdef", 0),
            0,
            "a whole pattern is no start"
        );
        assert_eq!(s.partial_at_end("xxz", 0), 0);
        assert_eq!(s.partial_at_end("", 0), 0);
        // Only patterns from `from` on count.
        let from = secrets(&["abcdef"]).pattern_count();
        assert_eq!(
            s.partial_at_end("xxabcd", from),
            2,
            "only cdxy's start counts"
        );
        assert_eq!(s.partial_at_end("xxab", from), 0);
        // Prefixes end on character boundaries.
        let s = secrets(&["é😀k"]);
        assert_eq!(s.partial_at_end("xé", 0), 2);
        assert_eq!(s.partial_at_end("xé😀", 0), 6);
    }

    #[test]
    fn redact_cut_covers_the_end_together_with_what_it_overlaps() {
        // `tok` occurs whole and is also the start of the longer secret
        // that the cut went through: one marker covers both.
        let s = secrets(&["tok", "tok-and-more"]);
        assert_eq!(
            s.redact_cut("x tok-an", 0),
            Redaction::Redacted("x [REDACTED]".to_string())
        );
        // Without a cut, only `tok` is a match.
        assert_eq!(
            s.redact("x tok-an"),
            Redaction::Redacted("x [REDACTED]-an".to_string())
        );
        assert_eq!(
            s.redact_cut("x tok-an", s.pattern_count()),
            s.redact("x tok-an")
        );
        assert_eq!(s.redact_cut("nothing", 0), Redaction::Clean);
    }

    #[test]
    fn debug_never_shows_the_values() {
        let s = secrets(&["hunter2", "s3cr3t"]);
        let debug = format!("{s:?}");
        assert!(
            !debug.contains("hunter2") && !debug.contains("s3cr3t"),
            "{debug}"
        );
        assert!(debug.contains("values hidden"), "{debug}");
    }

    /// The reference model for [`Secrets::redact`]: mark every byte that
    /// any occurrence of any pattern covers, then replace each maximal run of
    /// marked bytes with the marker.
    fn model(patterns: &[String], line: &str) -> String {
        let mut covered = vec![false; line.len()];
        for pattern in patterns {
            for start in 0..line.len() {
                if line
                    .get(start..)
                    .is_some_and(|rest| rest.starts_with(pattern.as_str()))
                {
                    covered[start..start + pattern.len()].fill(true);
                }
            }
        }
        let mut out = String::new();
        let mut at = 0;
        while at < line.len() {
            if covered[at] {
                while at < line.len() && covered[at] {
                    at += 1;
                }
                out.push_str(REDACTED);
            } else {
                // Runs start and end on character boundaries, because
                // patterns only match there.
                let c = line[at..].chars().next().expect("a character");
                out.push(c);
                at += c.len_utf8();
            }
        }
        out
    }

    #[test]
    fn redaction_matches_the_reference_model() {
        // Secrets and text from a small alphabet that includes the marker's
        // letters, quotes, backslashes, a combining mark, a control
        // character and multi-byte characters, so that overlaps, escapes and
        // marker collisions all happen.
        const ALPHABET: [&str; 15] = [
            "a", "b", "A", "C", "D", "]", "[", "\"", "'", "\\", "\n", "\u{1b}", "é", "\u{301}",
            "😀",
        ];
        check(
            "redaction matches the model",
            3000,
            |rng| {
                let mut word = |max: u32| -> String {
                    let len = rng.range(1..max + 1);
                    (0..len)
                        .map(|_| *rng.choose(&ALPHABET).expect("not empty"))
                        .collect()
                };
                let secrets: Vec<String> = (0..3).map(|_| word(4)).collect();
                let text = word(40);
                (secrets, text)
            },
            |(list, text)| {
                let mut registry = Secrets::new();
                for secret in list {
                    registry.add(secret);
                }
                // Embed the secrets raw, `{:?}`-formatted, inside a longer
                // string passed through `escape_debug`, `{:?}`-formatted
                // twice, and in the `{:?}` form of an error whose message
                // holds a `{:?}`-formatted secret.
                let escaped_text = text.escape_debug().to_string();
                let escaped = format!("{text}{}", list[2]).escape_debug().to_string();
                let twice = format!("{:?}", format!("{:?}", list[1]));
                let error = format!("{:?}", Err::<(), _>(format!("{text}{:?}", list[2])));
                let line = format!(
                    "{text}{}{text}{:?}{escaped}{twice}{error}",
                    list[0], list[1]
                );
                // How each secret reads in the line, worked out from `std`
                // alone rather than from the patterns under test.
                let inner = |text: &str| {
                    let debug = format!("{text:?}");
                    debug[1..debug.len() - 1].to_string()
                };
                let in_line = [
                    list[0].clone(),
                    inner(&list[1]),
                    escaped[escaped_text.len()..].to_string(),
                    inner(&inner(&list[1])),
                    inner(&inner(&list[2])),
                ];

                let expected = model(&registry.patterns, &line);
                let out = match registry.redact(&line) {
                    Redaction::Clean if expected == line => return Ok(()),
                    Redaction::Clean => return Err(format!("clean, but expected {expected:?}")),
                    Redaction::Withheld if registry.occurs_in(&expected) => return Ok(()),
                    Redaction::Withheld => {
                        return Err(format!("withheld, but {expected:?} is safe"));
                    }
                    Redaction::Redacted(out) => out,
                };
                if out != expected {
                    return Err(format!("got {out:?}, expected {expected:?}"));
                }
                if let Some(form) = in_line.iter().find(|form| out.contains(form.as_str())) {
                    return Err(format!("{form:?} survives in {out:?}"));
                }
                if registry.occurs_in(&out) {
                    return Err(format!("a pattern survives in {out:?}"));
                }
                Ok(())
            },
        );
    }
}
