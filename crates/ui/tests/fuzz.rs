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

//! The fuzz target of `tidepool-ui`: `theme`, one test named after it
//! (ADR-0027, ADR-0047, `docs/04-quality.md` → "Fuzz smoke").
//!
//! A plain `cargo test` runs its saved crashes (`fuzz/crashes/theme/`) and
//! its seeds unchanged: the hand-made ones in `fuzz/corpus/theme/`, and the
//! themes in `themes/`, read in place. `cargo xtask fuzz-smoke` runs them
//! with mutated inputs too, in an optimized profile with overflow checks.
//!
//! The target reads with small [`Limits`] ([`LIMITS`]), and among its seeds
//! are files that sit exactly at each limit (`edge-*`), so that one
//! mutation takes an input over it and into the check. Input that is not
//! UTF-8 is read as `String::from_utf8_lossy` gives it. Beyond "no panic",
//! it checks what `ui::theme`'s documentation promises:
//!
//! - a theme that reads keeps the format's rules: `bg` is opaque, a
//!   `solid` is opaque and given only for a translucent value, every
//!   token's solid color is opaque, `cut` is from 0 to `MAX_CUT`, and the
//!   name and fonts are each one line of plain text that is not blank, of
//!   at most `MAX_TEXT_BYTES` bytes; and reading the text again gives the
//!   same theme;
//! - a refusal as `UiError::Theme` names a key that is not empty, on a line
//!   of the input when it gives one, and its message holds no control
//!   character, line or paragraph separator or bidirectional control
//!   unescaped;
//! - nothing else but `UiError::ThemeSyntax`, whose checks are `codec`'s
//!   `toml` target's, is returned.

use tidepool_codec::toml::ErrorKind;
use tidepool_codec::{CodecError, Limits};
use tidepool_core::testing::fuzz::{Outcome, Target};
use tidepool_ui::UiError;
use tidepool_ui::metrics::Family;
use tidepool_ui::theme::{MAX_CUT, MAX_TEXT_BYTES, Theme, Token};

/// The repository root, where `fuzz/` and `themes/` are.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The limits the target reads with: 8 KiB of input, 8 levels, strings of
/// 128 bytes, numbers of 32 and 64 entries a table or array, so that a
/// mutated file is either over a limit or reads in microseconds. Every
/// seed is at or under each, the bundled theme at 5.4 KiB, 4 levels, 22
/// bytes, 3 bytes and 34 entries, and the `edge-*` seeds exactly at one.
/// Built from the defaults, so that a field added later keeps its default.
const LIMITS: Limits = {
    let mut limits = Limits::DEFAULT;
    limits.max_input_bytes = 8 * 1024;
    limits.max_depth = 8;
    limits.max_string_bytes = 128;
    limits.max_number_bytes = 32;
    limits.max_entries = 64;
    limits
};

/// The cap on the rise of the written peak, as for the other targets.
const MEMORY_CAP: u64 = 16 * 1024 * 1024;

/// The format's keys and values: the tables, a token's keys, the sizes'
/// and the fonts', colors opaque and translucent, and numbers on the edges
/// of `ansi` and `cut`.
const TOKENS: &[&[u8]] = &[
    b"version = 1\n",
    b"[colors]\n",
    b"[sizes]\n",
    b"[fonts]\n",
    b"[contrast]\n",
    b"[[contrast.use]]\n",
    b"value = ",
    b"solid = ",
    b"ansi = ",
    b"cut = ",
    b"font-ui = ",
    b"font-mono = ",
    b"bg = { value = \"#0b0c0d\" }\n",
    b"\"#0b0c0d\"",
    b"\"#0b0c0d80\"",
    b"\"rgba(16,19,21,0.78)\"",
    b"\"rgba(0,0,0,0)\"",
    b"255",
    b"256",
    b"-1",
    b"64",
    b"64.0",
    b"-0.0",
    b"nan",
    b"inf",
    b"\"\"",
    b"\" \"",
    b"\"\\u2028\"",
    b"\"\\u202e\"",
    b"\"a.b\"",
];

/// A run's result line for `fuzz-smoke`, then the verdict.
#[expect(
    clippy::print_stdout,
    reason = "`cargo xtask fuzz-smoke` reads the result line from the test's output; libtest \
              shows it only with --nocapture or when the test fails"
)]
fn report(outcome: &Outcome) {
    println!("{}", outcome.summary());
    outcome.assert_clean();
}

/// Whether `c` is what a name may not hold, and what an error's message
/// must not show unescaped, as `ui::theme` documents it.
fn is_escaped(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// The rule for a name or a font, written out from the documentation.
fn is_plain_line(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES
        && !text.chars().all(char::is_whitespace)
        && !text.chars().any(is_escaped)
}

/// `theme`: [`Theme::parse`] of the input, as UTF-8 or as its lossy
/// decoding, and the checks of the module documentation.
fn read_theme(input: &[u8]) {
    let text = String::from_utf8_lossy(input);
    match Theme::parse(&text, LIMITS) {
        Ok(theme) => check_theme(&text, &theme),
        Err(UiError::Theme(error)) => {
            assert!(!error.key.is_empty(), "an error names no key: {error:?}");
            if let Some(line) = error.line {
                let lines = text.matches('\n').count() + 1;
                assert!(
                    (1..=lines).contains(&line),
                    "line {line} is not one of the input's {lines}: {error:?}"
                );
            }
            let message = error.to_string();
            assert!(
                !message.chars().any(is_escaped),
                "the message shows a character unescaped: {message:?}"
            );
        }
        Err(UiError::ThemeSyntax(_)) => {}
        Err(other) => panic!("a theme file gave an error of another kind: {other:?}"),
    }
}

/// The rules a theme that reads keeps.
fn check_theme(text: &str, theme: &Theme) {
    assert_eq!(theme.color(Token::Bg).a, u8::MAX, "a translucent bg");
    for token in Token::ALL {
        let entry = theme.token(token);
        if let Some(solid) = entry.solid {
            assert_ne!(entry.value.a, u8::MAX, "`{token}` is opaque with a solid");
            assert_eq!(solid.a, u8::MAX, "`{token}`'s solid is translucent");
        }
        assert_eq!(theme.solid(token).a, u8::MAX, "`{token}`'s solid color");
        assert_eq!(theme.color(token), entry.value);
        assert_eq!(theme.ansi(token), entry.ansi);
    }
    assert!(
        theme.cut().is_finite() && (0.0..=MAX_CUT).contains(&theme.cut()),
        "cut {}",
        theme.cut()
    );
    for name in [
        theme.name(),
        theme.font(Family::Ui),
        theme.font(Family::Mono),
    ] {
        assert!(is_plain_line(name), "{name:?} was read");
    }
    assert_eq!(Theme::parse(text, LIMITS).as_ref(), Ok(theme), "read again");
}

#[test]
fn theme() {
    // Tidepool's own themes, read where they are.
    let outcome = Target::new(ROOT, "theme", read_theme)
        .seeds_in("themes", &["toml"])
        .dictionary(TOKENS)
        .memory_cap(MEMORY_CAP)
        .run();
    report(&outcome);
}

/// The `edge-*` seeds sit exactly at each limit: each reads as a theme with
/// [`LIMITS`], and fails for that limit with it one lower; and the other
/// seed reads as a theme.
#[test]
fn the_edge_seeds_sit_exactly_at_each_limit() {
    let seed = |name: &str| {
        let path = format!("{ROOT}/fuzz/corpus/theme/{name}");
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    assert!(Theme::parse(&seed("minimal.toml"), LIMITS).is_ok());
    for (name, field) in [
        ("input", "input"),
        ("string", "string"),
        ("entries", "entries"),
        ("number", "number"),
        ("depth", "depth"),
    ] {
        let file = format!("edge-{name}.toml");
        let text = seed(&file);
        assert!(text.len() <= LIMITS.max_input_bytes, "{file}");
        let theme = Theme::parse(&text, LIMITS);
        assert!(theme.is_ok(), "{file}: {theme:?}");
        let mut lower = LIMITS;
        match field {
            "input" => lower.max_input_bytes -= 1,
            "string" => lower.max_string_bytes -= 1,
            "entries" => lower.max_entries -= 1,
            "number" => lower.max_number_bytes -= 1,
            _ => lower.max_depth -= 1,
        }
        let error = Theme::parse(&text, lower);
        let Err(UiError::ThemeSyntax(CodecError::Toml { kind, .. })) = &error else {
            panic!("{file}: {error:?}");
        };
        let expected = match field {
            "input" => matches!(kind, ErrorKind::InputTooLarge { .. }),
            "string" => matches!(kind, ErrorKind::StringTooLong { .. }),
            "entries" => matches!(kind, ErrorKind::TooManyEntries { .. }),
            "number" => matches!(kind, ErrorKind::NumberTooLong { .. }),
            _ => matches!(kind, ErrorKind::TooDeep { .. }),
        };
        assert!(expected, "{file}: {kind:?}");
    }
}
